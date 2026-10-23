//! Direct, optional word-timed lyric sources. No database or credential discovery.
//!
//! Call on a blocking worker, BEFORE acquiring the application's database lock.
//! A source is best-effort: errors are warnings and never discard other sources.
//! Protocol reference (reviewed main, 2026-10-22):
//! https://github.com/chenmozhijin/LDDC/tree/84631e8cd011fcc3f71ca0ae017e2c9758958ffc/LDDC/core
//! QQ's DES uses two nonstandard S-box entries AND little-endian 32-bit words.
//! The table-driven implementation below is independent; the tables are protocol
//! constants. No Python, native decoder DLL, public API proxy, or token scraping.
//! LDDC code is GPL-3.0-only; this module is GPL-3.0-or-later. Copying upstream
//! implementation later requires preserving its notices and license conditions.
//! AMLL data has mixed provenance; its contributors' original work is CC0,
//! while imported material retains its original provider's conditions.
//! Apple lyrics are deliberately unsupported. This module never uses Apple
//! private interfaces or transmits the retained legacy credential fields.
//! LRCLIB fallback uses only https://lrclib.net/api/get and /api/search.
//! Protocol: https://github.com/tranxuanthang/lrclib/tree/04369d3f7c6e8cba1790629f26f866547a9b8f88/server/src/routes

use std::collections::BTreeMap;
use std::io::Read;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::ZlibDecoder;
use serde::Serialize;
use serde_json::{Value, json};

use crate::http::{HttpClient, HttpConfig, HttpResponse, Timeouts, Transport};
use crate::lyrics::{LyricsHit, pick_best};
use crate::timed_lyrics::{
    TimedLine, TimedLyrics, TimedWord, parse_krc, parse_qrc, parse_ttml, parse_yrc,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_HITS: usize = 2;
const SOURCE_BUDGET: Duration = Duration::from_secs(28);
const QQ_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const KG_SECRET: &str = "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA";
const QRC_KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";
const KRC_KEY: &[u8; 16] = b"@Gaw^2tGQ61-\xce\xd2ni";
const LOCAL_QRC_HEADER: &[u8] = b"\x98%\xb0\xac\xe3\x02\x83h\xe8\xfcl";

/// Never derive Debug: these values include bearer and account credentials.
#[derive(Clone)]
pub struct SourceOptions {
    pub netease: bool,
    pub qq: bool,
    pub kugou: bool,
    pub amll: bool,
    pub lrclib: bool,
    pub apple: bool,
    pub apple_storefront: String,
    pub apple_developer_token: String,
    pub apple_music_user_token: String,
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            netease: true,
            qq: true,
            kugou: true,
            amll: true,
            lrclib: true,
            apple: false,
            apple_storefront: "jp".into(),
            apple_developer_token: String::new(),
            apple_music_user_token: String::new(),
        }
    }
}

#[derive(Clone)]
pub struct TimedFound {
    pub source: String,
    pub format: String,
    /// Public provenance, NEVER a download URL containing accesskey or tokens.
    pub reference: String,
    pub title: String,
    pub artist: String,
    pub lyrics: TimedLyrics,
    /// Decoded ORIGINAL provider text, before canonical LRC conversion.
    pub raw_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceWarning {
    pub source: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct SearchOutput {
    pub candidates: Vec<TimedFound>,
    pub warnings: Vec<SourceWarning>,
    /// Honest line/plain fallback. These are NOT word-timed candidates and must
    /// not be passed to the word-timing attachment path without explicit choice.
    pub fallback: Vec<BasicFound>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BasicFound {
    pub source: String,
    pub reference: String,
    pub title: String,
    pub artist: String,
    pub raw_text: String,
    pub format: String,
    pub mode: String,
}

/// Production entry point. Network I/O only; never opens a database or files.
pub fn search(
    title: &str,
    artist: &str,
    duration: Option<f64>,
    options: &SourceOptions,
) -> SearchOutput {
    #[cfg(feature = "network")]
    {
        search_with_transport(
            title,
            artist,
            duration,
            options,
            Box::new(crate::net::UreqTransport),
        )
    }
    #[cfg(not(feature = "network"))]
    {
        let _ = (title, artist, duration);
        let mut out = SearchOutput::default();
        for (source, enabled) in enabled_sources(options) {
            if enabled {
                warn(&mut out, source, "网络功能未编译启用");
            }
        }
        out
    }
}

/// Injectable transport, including POST. Does not inspect environment credentials.
pub fn search_with_transport(
    title: &str,
    artist: &str,
    duration: Option<f64>,
    options: &SourceOptions,
    transport: Box<dyn Transport>,
) -> SearchOutput {
    let client = HttpClient::new(Box::new(SourceTransport(transport)));
    search_with_client(title, artist, duration, options, client)
}

/// Owns the client so retries/timeouts can be bounded even in injected tests.
/// Prefer search_with_transport when reproducing QQ's exact request headers.
pub fn search_with_client(
    title: &str,
    artist: &str,
    duration: Option<f64>,
    options: &SourceOptions,
    mut client: HttpClient,
) -> SearchOutput {
    client.config = HttpConfig {
        timeout: Duration::from_secs(8),
        connect_timeout: Duration::from_secs(4),
        retries: 0,
        ..HttpConfig::default()
    };
    let mut out = SearchOutput::default();
    if title.trim().is_empty() || artist.trim().is_empty() {
        for (source, enabled) in enabled_sources(options) {
            if enabled {
                warn(&mut out, source, "曲名和歌手不能为空，未发起模糊匹配");
            }
        }
        return out;
    }
    let duration = duration.filter(|d| d.is_finite() && *d > 0.0);
    // AMLL can use a metadata-only NetEase search even when NetEase lyrics are off.
    let mut ne_hits = Vec::new();
    if options.netease || options.amll {
        let io = SourceIo::new(&client);
        match netease_search(&io, title, artist, duration) {
            Ok(hits) => {
                if options.netease {
                    for hit in &hits {
                        if let Err(e) = netease_lyric(&io, hit, &mut out) {
                            warn(&mut out, "netease", &e.to_string());
                        }
                    }
                }
                ne_hits = hits;
            }
            Err(e) => {
                if options.netease {
                    warn(&mut out, "netease", &e.to_string());
                }
                if options.amll {
                    warn(
                        &mut out,
                        "amll",
                        "网易元数据搜索失败，无法取得 AMLL 索引 ID",
                    );
                }
            }
        }
    }
    if options.amll {
        let io = SourceIo::new(&client);
        for hit in &ne_hits {
            let url = format!(
                "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/refs/heads/main/ncm-lyrics/{}.ttml",
                hit.hit.id
            );
            let result = (|| {
                let text = amll_text(&io, &url, &hit.hit.id)?;
                let lyrics = parse_ttml(&text).context("AMLL TTML 解析失败")?;
                found("amll", "ttml", url, hit, &text, lyrics)
            })();
            add_result(&mut out, "amll", result);
        }
    }
    if options.qq {
        let io = SourceIo::new(&client);
        if let Err(e) = qq(&io, title, artist, duration, &mut out) {
            warn(&mut out, "qq", &e.to_string());
        }
    }
    if options.kugou {
        let io = SourceIo::new(&client);
        if let Err(e) = kugou(&io, title, artist, duration, &mut out) {
            warn(&mut out, "kugou", &e.to_string());
        }
    }
    if options.apple {
        warn(
            &mut out,
            "apple",
            "不支持 Apple 歌词获取：按配置约束禁用全部私有接口；官方元数据来源由上层单独处理",
        );
    }
    if options.lrclib && out.candidates.is_empty() {
        let io = SourceIo::new(&client);
        if let Err(e) = lrclib(&io, title, artist, duration, &mut out) {
            warn(&mut out, "lrclib", &e.to_string());
        }
    }
    out
}

fn enabled_sources(o: &SourceOptions) -> [(&'static str, bool); 6] {
    [
        ("netease", o.netease),
        ("qq", o.qq),
        ("kugou", o.kugou),
        ("amll", o.amll),
        ("lrclib", o.lrclib),
        ("apple", o.apple),
    ]
}

fn amll_text(io: &SourceIo<'_>, raw_url: &str, id: &str) -> Result<String> {
    match io.text(raw_url, &[]) {
        Ok(text) => Ok(text),
        Err(error) if error.to_string().starts_with("HTTP 404") => Err(error),
        Err(_) => {
            // Same public repository, official GitHub API; no third-party mirror
            // or credentials. This also works when the raw-content host fails DNS.
            let data=io.json(&format!("https://api.github.com/repos/amll-dev/amll-ttml-db/contents/ncm-lyrics/{id}.ttml?ref=main"), &[])?;
            ensure!(
                data["encoding"].as_str() == Some("base64"),
                "AMLL GitHub 响应没有可用的文件内容"
            );
            let content = data["content"]
                .as_str()
                .context("AMLL GitHub 响应缺少 content")?;
            let compact: String = content
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            let bytes = BASE64
                .decode(compact)
                .context("AMLL 文件内容不是合法 base64")?;
            ensure!(bytes.len() <= MAX_BYTES, "来源响应超过 8 MiB 限制");
            String::from_utf8(bytes).context("AMLL 文件不是 UTF-8 文本")
        }
    }
}

fn warn(out: &mut SearchOutput, source: &str, message: &str) {
    out.warnings.push(SourceWarning {
        source: source.into(),
        message: message.into(),
    });
}

fn add_result(out: &mut SearchOutput, source: &str, result: Result<TimedFound>) {
    match result {
        Ok(candidate) => {
            if out.candidates.iter().filter(|c| c.source == source).count() < MAX_HITS {
                out.candidates.push(candidate);
            }
        }
        Err(e) => warn(out, source, &parser_error_chain(&e)),
    }
}

struct SourceTransport(Box<dyn Transport>);
impl Transport for SourceTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeouts: Timeouts,
    ) -> std::result::Result<HttpResponse, crate::ProviderError> {
        self.0.get(url, headers, timeouts)
    }
    fn post(
        &self,
        url: &str,
        body: &[u8],
        headers: &[(String, String)],
        timeouts: Timeouts,
    ) -> std::result::Result<HttpResponse, crate::ProviderError> {
        let mut headers = headers.to_vec();
        if url == QQ_URL {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("user-agent"));
            headers.push(("User-Agent".into(), "okhttp/3.14.9".into()));
            headers.push(("Cookie".into(), "tmeLoginType=-1;".into()));
        }
        self.0.post(url, body, &headers, timeouts)
    }
}

struct SourceIo<'a> {
    client: &'a HttpClient,
    start: Instant,
}
impl<'a> SourceIo<'a> {
    fn new(client: &'a HttpClient) -> Self {
        Self {
            client,
            start: Instant::now(),
        }
    }
    fn check(&self) -> Result<()> {
        ensure!(
            self.start.elapsed() < SOURCE_BUDGET,
            "该来源达到查询时间预算"
        );
        Ok(())
    }
    fn bytes(&self, url: &str, headers: &[(String, String)]) -> Result<Vec<u8>> {
        self.check()?;
        // Do not propagate arbitrary transport messages: they can contain headers.
        let bytes = self
            .client
            .get_bytes_with(url, headers)
            .map_err(safe_http_error)?;
        ensure!(bytes.len() <= MAX_BYTES, "来源响应超过 8 MiB 限制");
        Ok(bytes)
    }
    fn text(&self, url: &str, headers: &[(String, String)]) -> Result<String> {
        String::from_utf8(self.bytes(url, headers)?).map_err(|_| anyhow!("来源响应不是 UTF-8 文本"))
    }
    fn json(&self, url: &str, headers: &[(String, String)]) -> Result<Value> {
        serde_json::from_slice(&self.bytes(url, headers)?)
            .map_err(|_| anyhow!("来源响应不是合法 JSON"))
    }
    fn post(&self, url: &str, body: Value) -> Result<Value> {
        self.check()?;
        self.client.post_json(url, &body).map_err(safe_http_error)
    }
}

fn safe_http_error(e: crate::ProviderError) -> anyhow::Error {
    // Inspect only the status classification made by HttpClient::classify_status,
    // and NEVER include the raw message in output. Transport errors can carry a
    // URL whose numeric song id happens to contain "401"/"404".
    if e.error_type == crate::status::ErrorType::NotFound {
        return anyhow!("HTTP 404：该曲目或歌词文件不存在");
    }
    if e.error_type == crate::status::ErrorType::InvalidResponse && e.message.ends_with("(401)") {
        return anyhow!("HTTP 401：令牌缺失、无效或已过期");
    }
    if e.error_type == crate::status::ErrorType::RateLimit && e.message.contains("(403)") {
        return anyhow!("HTTP 403：来源拒绝请求（不尝试绕过鉴权或限制）");
    }
    anyhow!("来源网络请求失败（{:?}）", e.error_type)
}

#[derive(Clone)]
struct Meta {
    hit: LyricsHit,
    album: String,
    hash: String,
}

fn matched(mut values: Vec<Meta>, title: &str, artist: &str, duration: Option<f64>) -> Vec<Meta> {
    // Run BEFORE normalize_title/pick_best: normalization can erase version tags.
    let wanted = version_markers(title);
    values.retain(|m| version_markers(&m.hit.title) == wanted);
    let mut out = Vec::new();
    while out.len() < MAX_HITS {
        let hits: Vec<_> = values.iter().map(|v| v.hit.clone()).collect();
        let Some(best) = pick_best(&hits, title, artist, duration) else {
            break;
        };
        let Some(i) = values.iter().position(|v| v.hit.id == best.id) else {
            break;
        };
        let item = values.remove(i);
        values.retain(|v| v.hit.id != item.hit.id);
        out.push(item);
    }
    out
}

fn found(
    source: &str,
    format: &str,
    reference: String,
    meta: &Meta,
    raw_text: &str,
    lyrics: TimedLyrics,
) -> Result<TimedFound> {
    ensure!(
        lyrics.lines.iter().any(|l| !l.text.trim().is_empty()),
        "未取得非空带时间歌词"
    );
    Ok(TimedFound {
        source: source.into(),
        format: format.into(),
        reference,
        title: meta.hit.title.clone(),
        artist: meta.hit.artist.clone(),
        lyrics,
        raw_text: raw_text.to_string(),
    })
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
}
fn singers(v: &Value, name: &str) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|x| s(&x[name]))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}
fn query(base: &str, params: &[(&str, String)]) -> String {
    format!(
        "{base}?{}",
        params
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    )
}
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn numeric_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 24 && id.bytes().all(|b| b.is_ascii_digit())
}
fn ne_headers() -> Vec<(String, String)> {
    vec![("Referer".into(), "https://music.163.com/".into())]
}

fn netease_search(
    io: &SourceIo<'_>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
) -> Result<Vec<Meta>> {
    let url = query(
        "https://music.163.com/api/search/get",
        &[
            ("type", "1".into()),
            ("limit", "20".into()),
            ("offset", "0".into()),
            ("s", format!("{artist} {title}")),
        ],
    );
    let data = io.json(&url, &ne_headers())?;
    ensure!(
        data["code"].as_i64().unwrap_or(200) == 200,
        "网易搜索接口返回失败状态"
    );
    let songs = data["result"]["songs"]
        .as_array()
        .ok_or_else(|| anyhow!("网易搜索未返回歌曲列表"))?;
    let metas = songs
        .iter()
        .take(40)
        .filter_map(|song| {
            let id = s(&song["id"]);
            if !numeric_id(&id) {
                return None;
            }
            Some(Meta {
                hit: LyricsHit {
                    id,
                    title: s(&song["name"]),
                    artist: singers(
                        if song["artists"].is_array() {
                            &song["artists"]
                        } else {
                            &song["ar"]
                        },
                        "name",
                    ),
                    duration_sec: number(&song["duration"])
                        .or_else(|| number(&song["dt"]))
                        .map(|n| n / 1000.0),
                },
                album: s(if song["album"].is_object() {
                    &song["album"]["name"]
                } else {
                    &song["al"]["name"]
                }),
                hash: String::new(),
            })
        })
        .collect();
    Ok(matched(metas, title, artist, duration))
}

fn netease_lyric(io: &SourceIo<'_>, meta: &Meta, out: &mut SearchOutput) -> Result<()> {
    // Direct provider endpoint, no NeteaseCloudMusicApi public proxy. Unlike the
    // old /api/song/lyric, v1 requests YRC with yv. A missing YRC is NOT success.
    let url = query(
        "https://music.163.com/api/song/lyric/v1",
        &[
            ("id", meta.hit.id.clone()),
            ("lv", "-1".into()),
            ("tv", "-1".into()),
            ("rv", "-1".into()),
            ("yv", "-1".into()),
        ],
    );
    let data = io.json(&url, &ne_headers())?;
    ensure!(
        data["code"].as_i64().unwrap_or(200) == 200,
        "网易歌词响应返回失败状态；不尝试未经授权的私有接口"
    );
    if let Some(text) = data["yrc"]["lyric"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
    {
        match parse_yrc(text) {
            Ok(lyrics) => {
                add_result(
                    out,
                    "netease",
                    found(
                        "netease",
                        "yrc",
                        format!("https://music.163.com/song?id={}", meta.hit.id),
                        meta,
                        text,
                        lyrics,
                    ),
                );
                return Ok(());
            }
            Err(error) => warn(
                out,
                "netease",
                &format!("网易 YRC 解析失败：{}", parser_error_chain(&error)),
            ),
        }
    } else {
        warn(
            out,
            "netease",
            "网易当前公开响应未返回 YRC；本适配器不调用未经授权的私有接口，只保留实际提供的普通歌词回退",
        );
    }
    let raw = data["lrc"]["lyric"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| anyhow!("网易没有可用的 YRC 或普通歌词正文"))?;
    let line_synced = has_line_clocks(raw);
    out.fallback.push(BasicFound {
        source: "netease".into(),
        reference: format!("https://music.163.com/song?id={}", meta.hit.id),
        title: meta.hit.title.clone(),
        artist: meta.hit.artist.clone(),
        raw_text: raw.into(),
        format: if line_synced { "lrc" } else { "text" }.into(),
        mode: if line_synced { "lineSynced" } else { "plain" }.into(),
    });
    Ok(())
}

/// Local parser diagnostics only, never a transport/server-provided error.
/// Exclude messages containing apparent lyric payloads or XML source excerpts.
fn parser_error_chain(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| {
            let message = cause.to_string();
            if message.chars().count() <= 240 && !message.contains(['\n', '\r', '<', '>', '"']) {
                message
            } else {
                "解析器返回无法安全显示的详细信息".into()
            }
        })
        .collect::<Vec<_>>()
        .join(": ")
}

/// QQ returns an authoritative name plus a decorated display title. A translation
/// in the display title must not dilute matching, but ALL recording-version
/// evidence is checked first, including a Live title whose name looks original.
fn qq_song_meta(song: &Value, wanted_title: &str) -> Option<Meta> {
    let wanted = version_markers(wanted_title);
    let observed = ["name", "title", "subtitle", "title_main", "title_extra"]
        .iter()
        .fold(0, |flags, key| flags | version_markers(&s(&song[*key])));
    if observed != wanted {
        return None;
    }
    let displayed = s(&song["title"]);
    let canonical = s(&song["name"]);
    let title = if wanted == 0
        && !canonical.trim().is_empty()
        && display_decorates_name(&displayed, &canonical)
    {
        canonical
    } else if !displayed.trim().is_empty() {
        displayed
    } else {
        canonical
    };
    let id = s(&song["id"]);
    if !numeric_id(&id) {
        return None;
    }
    Some(Meta {
        hit: LyricsHit {
            id,
            title,
            artist: singers(&song["singer"], "name"),
            duration_sec: number(&song["interval"]),
        },
        album: s(&song["album"]["name"]),
        hash: String::new(),
    })
}

fn display_decorates_name(displayed: &str, canonical: &str) -> bool {
    let Some(suffix) = displayed.trim().strip_prefix(canonical.trim()) else {
        return false;
    };
    let suffix = suffix.trim();
    suffix.is_empty()
        || [('(', ')'), ('（', '）')].iter().any(|&(open, close)| {
            suffix
                .strip_prefix(open)
                .and_then(|s| s.strip_suffix(close))
                .is_some_and(|s| {
                    !s.is_empty() && !s.contains(['(', ')', '（', '）']) && version_markers(s) == 0
                })
        })
}

fn qq_request(
    io: &SourceIo<'_>,
    comm: &Value,
    method: &str,
    module: &str,
    param: Value,
) -> Result<Value> {
    let data = io.post(
        QQ_URL,
        json!({"comm":comm,"request":{"method":method,"module":module,"param":param}}),
    )?;
    ensure!(
        data["code"].as_i64() == Some(0) && data["request"]["code"].as_i64() == Some(0),
        "QQ 接口返回失败状态（游客会话或接口可能已变更）"
    );
    ensure!(
        data["request"]["data"].is_object(),
        "QQ 响应缺少 request.data"
    );
    Ok(data["request"]["data"].clone())
}

fn qq(
    io: &SourceIo<'_>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
    out: &mut SearchOutput,
) -> Result<()> {
    let mut comm = json!({"ct":11,"cv":"1003006","v":"1003006","os_ver":"15","phonetype":"24122RKC7C","rom":"Redmi/miro/miro:15/AE3A.240806.005/OS2.0.105.0.VOMCNXM:user/release-keys","tmeAppID":"qqmusiclight","nettype":"NETWORK_WIFI","udid":"0"});
    let session = qq_request(
        io,
        &comm,
        "GetSession",
        "music.getSession.session",
        json!({"caller":0,"uid":"0","vkey":0}),
    )?;
    for key in ["uid", "sid", "userip"] {
        ensure!(
            !session["session"][key].is_null(),
            "QQ 游客响应缺少会话字段"
        );
        comm[key] = session["session"][key].clone();
    }
    let data = qq_request(
        io,
        &comm,
        "DoSearchForQQMusicLite",
        "music.search.SearchCgiService",
        json!({"search_id": (18_014_398_509_481_984u64 + now_ms() % 86_400_000).to_string(),"remoteplace":"search.android.keyboard","query":format!("{artist} {title}"),"search_type":0,"num_per_page":20,"page_num":1,"highlight":0,"nqc_flag":0,"page_id":1,"grp":1}),
    )?;
    let songs = data["body"]["item_song"]
        .as_array()
        .ok_or_else(|| anyhow!("QQ 搜索未返回 body.item_song"))?;
    let hits = matched(
        songs
            .iter()
            .take(40)
            .filter_map(|song| qq_song_meta(song, title))
            .collect(),
        title,
        artist,
        duration,
    );
    if hits.is_empty() && !songs.is_empty() {
        warn(
            out,
            "qq",
            "QQ 搜索有结果，但没有同时通过主标题、歌手、时长和录音版本检查的候选",
        );
    }
    for meta in hits {
        let result = (|| {
            let data = qq_request(
                io,
                &comm,
                "GetPlayLyricInfo",
                "music.musichallSong.PlayLyricInfo",
                json!({"albumName":BASE64.encode(meta.album.as_bytes()),"crypt":1,"ct":19,"cv":2111,"interval":meta.hit.duration_sec.unwrap_or(0.0) as u64,"lrc_t":0,"qrc":1,"qrc_t":0,"roma":1,"roma_t":0,"singerName":BASE64.encode(meta.hit.artist.as_bytes()),"songID":meta.hit.id.parse::<u64>()?,"songName":BASE64.encode(meta.hit.title.as_bytes()),"trans":1,"trans_t":0,"type":0}),
            )?;
            let cipher = data["lyric"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("QQ 未返回 QRC 密文"))?;
            let text = decode_qrc(&unhex(cipher)?).context("QQ QRC 解码失败")?;
            let lyrics = parse_qrc(&text).context("QQ QRC 解析失败（可能只提供逐行歌词）")?;
            found(
                "qq",
                "qrc",
                format!("https://y.qq.com/n/ryqq/songDetail/{}", meta.hit.id),
                &meta,
                &text,
                lyrics,
            )
        })();
        add_result(out, "qq", result);
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn kg_request(
    io: &SourceIo<'_>,
    base: &str,
    mut params: BTreeMap<String, String>,
    lyric: bool,
) -> Result<Value> {
    let now = now_ms();
    let mid = md5_hex(now.to_string().as_bytes());
    params.insert("appid".into(), "3116".into());
    params.insert("clientver".into(), "11070".into());
    if !lyric {
        for (k, v) in [
            ("userid", "0"),
            ("token", ""),
            ("iscorrection", "1"),
            ("uuid", "-"),
            ("dfid", "-"),
            ("platform", "AndroidFilter"),
        ] {
            params.insert(k.into(), v.into());
        }
        params.insert("clienttime".into(), (now / 1000).to_string());
        params.insert("mid".into(), mid.clone());
    }
    let signed = kg_signature(&params);
    params.insert("signature".into(), signed);
    // Keep empty token= in the query: it participates in the signature.
    let url = query(
        base,
        &params
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect::<Vec<_>>(),
    );
    let module = if lyric { "Lyric" } else { "SearchSong" };
    let mut headers = vec![
        (
            "User-Agent".into(),
            format!("Android14-1070-11070-201-0-{module}-wifi"),
        ),
        ("KG-Rec".into(), "1".into()),
        ("KG-RC".into(), "1".into()),
        ("KG-CLIENTTIMEMS".into(), now.to_string()),
        ("mid".into(), mid),
    ];
    if !lyric {
        headers.push(("x-router".into(), "complexsearch.kugou.com".into()));
    }
    let data = io.json(&url, &headers)?;
    ensure!(
        matches!(data["error_code"].as_i64().unwrap_or(0), 0 | 200)
            && data["status"].as_i64() != Some(0),
        "酷狗接口返回失败状态"
    );
    Ok(data)
}

fn kg_signature(params: &BTreeMap<String, String>) -> String {
    let mut signed = KG_SECRET.to_string();
    for (k, v) in params {
        signed.push_str(k);
        signed.push('=');
        signed.push_str(v);
    }
    signed.push_str(KG_SECRET);
    md5_hex(signed.as_bytes())
}
fn params(values: &[(&str, String)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn kugou(
    io: &SourceIo<'_>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
    out: &mut SearchOutput,
) -> Result<()> {
    let request = kg_request(
        io,
        "https://complexsearch.kugou.com/v2/search/song",
        params(&[
            ("sorttype", "0".into()),
            ("keyword", format!("{artist} {title}")),
            ("pagesize", "20".into()),
            ("page", "1".into()),
        ]),
        false,
    );
    let (data, old) = match request {
        Ok(data) => (data, false),
        Err(_) => {
            warn(out, "kugou", "酷狗新搜索失败，尝试同平台旧搜索接口");
            let url = query(
                "https://mobiles.kugou.com/api/v3/search/song",
                &[
                    ("showtype", "14".into()),
                    ("highlight", String::new()),
                    ("pagesize", "20".into()),
                    ("tag_aggr", "1".into()),
                    ("plat", "0".into()),
                    ("sver", "5".into()),
                    ("keyword", format!("{artist} {title}")),
                    ("correct", "1".into()),
                    ("api_ver", "1".into()),
                    ("version", "9108".into()),
                    ("page", "1".into()),
                ],
            );
            (io.json(&url, &[])?, true)
        }
    };
    let songs = data["data"][if old { "info" } else { "lists" }]
        .as_array()
        .ok_or_else(|| anyhow!("酷狗搜索未返回歌曲列表"))?;
    let hits = matched(
        songs
            .iter()
            .take(40)
            .filter_map(|song| {
                let id = s(&song[if old { "album_audio_id" } else { "ID" }]);
                if !numeric_id(&id) {
                    return None;
                }
                let artist = if old {
                    s(&song["singername"]).replace('、', "/")
                } else {
                    singers(&song["Singers"], "name")
                };
                Some(Meta {
                    hit: LyricsHit {
                        id,
                        title: s(&song[if old { "songname" } else { "SongName" }]),
                        artist,
                        duration_sec: number(&song[if old { "duration" } else { "Duration" }]),
                    },
                    album: s(&song[if old { "album_name" } else { "AlbumName" }]),
                    hash: s(&song[if old { "hash" } else { "FileHash" }]),
                })
            })
            .collect(),
        title,
        artist,
        duration,
    );
    for meta in hits {
        let result = (|| {
            ensure!(!meta.hash.is_empty(), "酷狗候选缺少文件 hash");
            let data = kg_request(
                io,
                "https://lyrics.kugou.com/v1/search",
                params(&[
                    ("album_audio_id", meta.hit.id.clone()),
                    (
                        "duration",
                        ((meta.hit.duration_sec.unwrap_or(0.0) * 1000.0) as u64).to_string(),
                    ),
                    ("hash", meta.hash.clone()),
                    (
                        "keyword",
                        format!(
                            "{} - {}",
                            meta.hit.artist.replace('/', "、"),
                            meta.hit.title
                        ),
                    ),
                    ("lrctxt", "1".into()),
                    ("man", "no".into()),
                ]),
                true,
            )?;
            let candidates = data["candidates"]
                .as_array()
                .ok_or_else(|| anyhow!("酷狗未返回歌词候选列表"))?;
            let mut last = anyhow!("酷狗未找到匹配的 KRC 歌词");
            // A plain-text candidate must not hide a later real KRC candidate.
            for item in candidates.iter().take(MAX_HITS) {
                let result = (|| {
                    let id = s(&item["id"]);
                    let accesskey = s(&item["accesskey"]);
                    ensure!(
                        numeric_id(&id) && !accesskey.is_empty() && accesskey.len() <= 512,
                        "酷狗歌词候选缺少 id/accesskey"
                    );
                    if let (Some(want), Some(got)) =
                        (meta.hit.duration_sec, number(&item["duration"]))
                    {
                        ensure!((want - got / 1000.0).abs() <= 8.0, "酷狗歌词候选时长不匹配");
                    }
                    let data = kg_request(
                        io,
                        "https://lyrics.kugou.com/download",
                        params(&[
                            ("accesskey", accesskey),
                            ("charset", "utf8".into()),
                            ("client", "mobi".into()),
                            ("fmt", "krc".into()),
                            ("id", id.clone()),
                            ("ver", "1".into()),
                        ]),
                        true,
                    )?;
                    ensure!(
                        data["contenttype"].as_i64() != Some(2),
                        "酷狗仅返回纯文本歌词，没有 KRC 时间轴"
                    );
                    let content = data["content"]
                        .as_str()
                        .ok_or_else(|| anyhow!("酷狗下载缺少 content"))?;
                    ensure!(content.len() <= MAX_BYTES, "酷狗歌词内容过大");
                    let bytes = BASE64
                        .decode(content)
                        .map_err(|_| anyhow!("酷狗 content 不是合法 base64"))?;
                    let text = decode_krc(&bytes)?;
                    let lyrics = parse_krc(&text).map_err(|error| {
                        anyhow!("酷狗 KRC 解析失败：{}", parser_error_chain(&error))
                    })?;
                    found(
                        "kugou",
                        "krc",
                        format!("kugou:album_audio_id={};lyric_id={id}", meta.hit.id),
                        &meta,
                        &text,
                        lyrics,
                    )
                })();
                match result {
                    Ok(found) => return Ok(found),
                    Err(e) => last = e,
                }
            }
            Err(last)
        })();
        add_result(out, "kugou", result);
    }
    Ok(())
}

/// Compare explicit recording/version markers before title normalization.
fn version_markers(title: &str) -> u8 {
    let lower = title.to_lowercase();
    let tokens = format!(
        " {} ",
        lower
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
            .collect::<String>()
    );
    let has = |words: &[&str], other: &[&str]| {
        words
            .iter()
            .any(|word| tokens.contains(&format!(" {word} ")))
            || other.iter().any(|word| lower.contains(word))
    };
    let mut flags = 0;
    if has(
        &["studio", "original"],
        &["スタジオ", "录音室", "錄音室", "原版", "原曲"],
    ) {
        flags |= 1;
    }
    if has(
        &["live", "concert", "unplugged"],
        &["ライブ", "现场", "現場", "演唱会", "演唱會"],
    ) {
        flags |= 2;
    }
    if has(
        &["acoustic", "acoustics"],
        &["アコースティック", "木吉他", "不插电", "不插電"],
    ) {
        flags |= 4;
    }
    if has(
        &["remix", "remixed", "mix", "rearrange", "rearranged"],
        &["リミックス", "混音", "再编曲", "再編曲"],
    ) {
        flags |= 8;
    }
    if has(
        &["instrumental", "karaoke", "offvocal"],
        &["伴奏", "カラオケ", "インスト"],
    ) {
        flags |= 16;
    }
    if has(
        &["rerecorded", "remaster", "remastered"],
        &["再録", "重录", "重錄", "リマスター"],
    ) {
        flags |= 32;
    }
    flags
}

/// LRCLIB is an official, public service, never a public proxy for another service.
/// /api/get is tried when duration is known; /api/search is the bounded fallback.
/// Keep basic LRC/plain text outside TimedFound. Neither hasWordSync nor the name
/// syncedLyrics proves word timing; actual Enhanced LRC tags must parse.
fn lrclib(
    io: &SourceIo<'_>,
    title: &str,
    artist: &str,
    duration: Option<f64>,
    out: &mut SearchOutput,
) -> Result<()> {
    let headers = vec![(
        "User-Agent".into(),
        "JPOP-Corpus-Tool/0.2 (https://github.com/Yisoragoto/jpop-corpus-tool)".into(),
    )];
    let mut records = Vec::new();
    if let Some(duration) = duration {
        let url = query(
            "https://lrclib.net/api/get",
            &[
                ("track_name", title.into()),
                ("artist_name", artist.into()),
                ("album_name", String::new()),
                ("duration", duration.to_string()),
            ],
        );
        match io.json(&url, &headers) {
            Ok(record) if record["id"].is_number() || record["id"].is_string() => {
                records.push(record)
            }
            Ok(_) => warn(out, "lrclib", "LRCLIB get 未返回歌曲记录，继续 search"),
            Err(e) => warn(out, "lrclib", &format!("LRCLIB get 失败，继续 search：{e}")),
        }
        // Avoid a search when the exact record already contains genuine word tags.
        if let Some(record) = records.first()
            && let Some(meta) = lrclib_meta(record)
            && !matched(vec![meta.clone()], title, artist, Some(duration)).is_empty()
            && let Some(text) = record["syncedLyrics"].as_str()
            && let Ok(lyrics) = parse_enhanced_lrc(text)
        {
            add_result(
                out,
                "lrclib",
                found(
                    "lrclib",
                    "elrc",
                    lrclib_reference(&meta),
                    &meta,
                    text,
                    lyrics,
                ),
            );
            return Ok(());
        }
    }
    let url = query(
        "https://lrclib.net/api/search",
        &[("track_name", title.into()), ("artist_name", artist.into())],
    );
    match io.json(&url, &headers) {
        Ok(value) => {
            let list = value
                .as_array()
                .ok_or_else(|| anyhow!("LRCLIB search 未返回数组"))?;
            records.extend(list.iter().take(40).cloned());
        }
        Err(e) if records.is_empty() => return Err(e),
        Err(e) => warn(
            out,
            "lrclib",
            &format!("LRCLIB search 失败，保留 get 的记录：{e}"),
        ),
    }
    let metas = records.iter().filter_map(lrclib_meta).collect();
    for meta in matched(metas, title, artist, duration) {
        let Some(record) = records.iter().find(|r| s(&r["id"]) == meta.hit.id) else {
            continue;
        };
        if record["instrumental"].as_bool() == Some(true) {
            continue;
        }
        let reference = lrclib_reference(&meta);
        let synced = record["syncedLyrics"]
            .as_str()
            .filter(|s| !s.trim().is_empty());
        if let Some(text) = synced
            && let Ok(lyrics) = parse_enhanced_lrc(text)
        {
            add_result(
                out,
                "lrclib",
                found("lrclib", "elrc", reference, &meta, text, lyrics),
            );
            continue;
        }
        let (raw, format, mode) = if let Some(text) = synced.filter(|t| has_line_clocks(t)) {
            (text, "lrc", "lineSynced")
        } else if let Some(text) = record["plainLyrics"]
            .as_str()
            .filter(|t| !t.trim().is_empty())
        {
            (text, "text", "plain")
        } else {
            warn(
                out,
                "lrclib",
                "LRCLIB 记录没有可验证的逐词、逐行或纯文本歌词",
            );
            continue;
        };
        out.fallback.push(BasicFound {
            source: "lrclib".into(),
            reference,
            title: meta.hit.title,
            artist: meta.hit.artist,
            raw_text: raw.into(),
            format: format.into(),
            mode: mode.into(),
        });
    }
    Ok(())
}
fn lrclib_meta(record: &Value) -> Option<Meta> {
    let id = s(&record["id"]);
    if !numeric_id(&id) {
        return None;
    }
    Some(Meta {
        hit: LyricsHit {
            id,
            title: s(&record["trackName"]),
            artist: s(&record["artistName"]),
            duration_sec: number(&record["duration"]),
        },
        album: s(&record["albumName"]),
        hash: String::new(),
    })
}
fn lrclib_reference(meta: &Meta) -> String {
    format!("https://lrclib.net/api/get/{}", meta.hit.id)
}

fn clock_re() -> &'static regex::Regex {
    static CLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    CLOCK.get_or_init(|| {
        regex::Regex::new(r"^(\d{1,6}):([0-5]\d)(?:[.:](\d{1,3}))?$").expect("constant clock regex")
    })
}
fn elrc_clock(text: &str) -> Result<u64> {
    let c = clock_re()
        .captures(text)
        .ok_or_else(|| anyhow!("不是受支持的 LRC 毫秒时间标签"))?;
    let minutes = c[1].parse::<u64>()?;
    let seconds = c[2].parse::<u64>()?;
    let fraction = c.get(3).map(|m| m.as_str()).unwrap_or("");
    let millis = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>()? * 10u64.pow(3 - fraction.len() as u32)
    };
    Ok(minutes * 60_000 + seconds * 1000 + millis)
}
fn has_line_clocks(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start()
            .strip_prefix('[')
            .and_then(|s| s.split_once(']'))
            .is_some_and(|(clock, _)| elrc_clock(clock).is_ok())
    })
}

/// Parses only genuine Enhanced LRC. Never divides a line's duration into words.
pub fn parse_enhanced_lrc(text: &str) -> Result<TimedLyrics> {
    ensure!(text.len() <= MAX_BYTES, "Enhanced LRC 超过大小限制");
    let tag = regex::Regex::new(r"<([^<>]+)>").expect("constant tag regex");
    let mut offset = 0i64;
    for line in text.lines().map(str::trim) {
        if let Some(value) = line
            .strip_prefix("[offset:")
            .and_then(|l| l.strip_suffix(']'))
        {
            offset = value
                .trim()
                .parse()
                .map_err(|_| anyhow!("LRC offset 无效"))?;
        }
    }
    let shifted = |time: u64| -> u64 {
        (i128::from(time) - i128::from(offset)).clamp(0, i128::from(u64::MAX)) as u64
    };
    let mut lines = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let Some((head, body)) = raw
            .trim_start()
            .strip_prefix('[')
            .and_then(|l| l.split_once(']'))
        else {
            ensure!(raw.trim().is_empty(), "Enhanced LRC 含没有时间轴的正文");
            continue;
        };
        let Ok(line_start) = elrc_clock(head) else {
            // Only normal LRC metadata may be ignored.
            ensure!(
                head.split_once(':').is_some_and(
                    |(key, _)| !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphabetic())
                ),
                "LRC 行时间无效"
            );
            continue;
        };
        if body.trim().is_empty() {
            continue;
        }
        let mut words = Vec::new();
        let mut line_text = String::new();
        let mut cursor = 0;
        let mut current = None;
        let mut last_tag = None;
        for mark in tag.captures_iter(body) {
            let full = mark.get(0).expect("whole regex match");
            let next = elrc_clock(&mark[1])?;
            ensure!(
                next >= line_start && last_tag.is_none_or(|p| next >= p),
                "Enhanced LRC 字时间倒序或早于行"
            );
            let segment = &body[cursor..full.start()];
            let start = line_text.chars().count();
            line_text.push_str(segment);
            let end = line_text.chars().count();
            if !segment.trim().is_empty() {
                let begin =
                    current.ok_or_else(|| anyhow!("首个 Enhanced 时间标签前存在无时间正文"))?;
                words.push(TimedWord {
                    start,
                    end,
                    start_ms: shifted(begin),
                    end_ms: Some(shifted(next)),
                });
            }
            current = Some(next);
            last_tag = Some(next);
            cursor = full.end();
        }
        let tail = &body[cursor..];
        let start = line_text.chars().count();
        line_text.push_str(tail);
        if !tail.trim().is_empty() {
            let begin = current.ok_or_else(|| anyhow!("只有逐行时间，没有 Enhanced 逐词标签"))?;
            words.push(TimedWord {
                start,
                end: line_text.chars().count(),
                start_ms: shifted(begin),
                end_ms: None,
            });
        }
        ensure!(!words.is_empty(), "没有真正携带正文的 Enhanced 时间标签");
        let end_ms = words.last().and_then(|w| w.end_ms);
        lines.push(TimedLine {
            start_ms: shifted(line_start),
            end_ms,
            text: line_text,
            words,
        });
    }
    ensure!(!lines.is_empty(), "没有 Enhanced LRC 正文");
    Ok(TimedLyrics { lines })
}

fn inflate(bytes: &[u8]) -> Result<String> {
    let mut decoder = ZlibDecoder::new(bytes).take((MAX_BYTES + 1) as u64);
    let mut plain = Vec::new();
    decoder
        .read_to_end(&mut plain)
        .map_err(|_| anyhow!("歌词 zlib 解压失败或密文格式不受支持"))?;
    ensure!(plain.len() <= MAX_BYTES, "解压后的歌词超过 8 MiB 限制");
    String::from_utf8(plain).map_err(|_| anyhow!("解码后的歌词不是 UTF-8"))
}

/// Decode a binary KRC file (`krc1` + XOR-obfuscated zlib stream).
/// Network JSON content must be base64-decoded by the caller first.
pub fn decode_krc(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() <= MAX_BYTES, "KRC 文件过大");
    ensure!(
        bytes.starts_with(b"krc1") && bytes.len() > 4,
        "不支持的 KRC：需要 krc1 二进制文件头；普通 LRC 请走文本解析"
    );
    let packed: Vec<u8> = bytes[4..]
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ KRC_KEY[i % 16])
        .collect();
    inflate(&packed)
}

/// Decode network QRC encrypted bytes (already hex-decoded), or the known local
/// QMC1-wrapped QRC binary format. This function never guesses plain LRC/XML.
pub fn decode_qrc(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= MAX_BYTES && !bytes.is_empty(),
        "QRC 为空或超过大小限制"
    );
    let local;
    let encrypted = if bytes.starts_with(LOCAL_QRC_HEADER) {
        local = bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let j = if i > 0x7fff { i % 0x7fff } else { i };
                b ^ QMC1[j & 127]
            })
            .collect::<Vec<_>>();
        &local[LOCAL_QRC_HEADER.len()..]
    } else {
        if let Ok(text) = std::str::from_utf8(bytes) {
            let text = text.trim_start_matches('\u{feff}').trim_start();
            ensure!(
                !text.starts_with('<') && !text.starts_with('['),
                "QRC/LRC 明文应走文本解析，不能作为 QRC 密文解码"
            );
        }
        bytes
    };
    ensure!(
        !encrypted.is_empty() && encrypted.len().is_multiple_of(8),
        "不支持的 QRC 密文长度或未知本地 QRC 头（需要 8 字节对齐）"
    );
    let keys = [
        des_keys(&QRC_KEY[16..]),
        des_keys(&QRC_KEY[8..16]),
        des_keys(&QRC_KEY[..8]),
    ];
    let mut packed = Vec::with_capacity(encrypted.len());
    for block in encrypted.as_chunks::<8>().0 {
        let mut b = *block;
        b = des_block(b, &keys[0], true);
        b = des_block(b, &keys[1], false);
        b = des_block(b, &keys[2], true);
        packed.extend_from_slice(&b);
    }
    inflate(&packed)
}

fn unhex(text: &str) -> Result<Vec<u8>> {
    ensure!(
        text.len() <= MAX_BYTES * 2 && text.len().is_multiple_of(2),
        "QRC hex 长度无效"
    );
    fn nibble(b: u8) -> Result<u8> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => bail!("QRC 密文不是合法 hex"),
        }
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| Ok(nibble(b[0])? * 16 + nibble(b[1])?))
        .collect()
}

// Generic DES bit permutations, expressed as one-based MSB positions. The QQ
// wire format reverses each 32-bit word on BOTH block and key boundaries.
fn permute(input: u64, width: u8, table: &[u8]) -> u64 {
    table
        .iter()
        .fold(0, |v, p| (v << 1) | ((input >> (width - *p)) & 1))
}
fn wire_word(mut b: [u8; 8]) -> [u8; 8] {
    b[..4].reverse();
    b[4..].reverse();
    b
}
fn des_keys(key: &[u8]) -> [u64; 16] {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(key);
    let k = permute(u64::from_be_bytes(wire_word(bytes)), 64, &PC1);
    let mut c = (k >> 28) as u32;
    let mut d = (k & 0x0fff_ffff) as u32;
    let mut out = [0; 16];
    for (i, shift) in [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1]
        .iter()
        .enumerate()
    {
        c = ((c << shift) | (c >> (28 - shift))) & 0x0fff_ffff;
        d = ((d << shift) | (d >> (28 - shift))) & 0x0fff_ffff;
        // QQ's key-compression D indices are displaced by one bit. Keep PC2 as
        // the standard table and select the D half separately; its final bit is
        // the zero just below the 28-bit register. Standard TripleDES is wrong.
        out[i] = PC2.iter().fold(0, |key, &position| {
            let bit = if position <= 28 {
                (c >> (28 - position)) & 1
            } else {
                let d_position = position - 28;
                if d_position < 28 {
                    (d >> (27 - d_position)) & 1
                } else {
                    0
                }
            };
            (key << 1) | bit as u64
        });
    }
    out
}
fn des_block(block: [u8; 8], keys: &[u64; 16], decrypt: bool) -> [u8; 8] {
    let ip = permute(u64::from_be_bytes(wire_word(block)), 64, &IP);
    let mut l = (ip >> 32) as u32;
    let mut r = ip as u32;
    for round in 0..16 {
        let expanded =
            permute(r as u64, 32, &EXPAND) ^ keys[if decrypt { 15 - round } else { round }];
        let mut substituted = 0u32;
        for (i, table) in SBOX.iter().enumerate() {
            let six = ((expanded >> (42 - i * 6)) & 63) as usize;
            let row = ((six & 32) >> 4) | (six & 1);
            let col = (six >> 1) & 15;
            substituted = (substituted << 4) | table[row * 16 + col] as u32;
        }
        let next = l ^ permute(substituted as u64, 32, &P) as u32;
        l = r;
        r = next;
    }
    wire_word(permute(((r as u64) << 32) | l as u64, 64, &FP).to_be_bytes())
}

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const EXPAND: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    // QQ: S2 row 1 col 7 is 15 (standard DES says 14).
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    // QQ: S4 row 3 col 5 is 10 (standard DES says 1).
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10,
        10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];
const QMC1: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];

// RFC 1321 MD5, only for the provider's request signature (not password hashing).
fn md5_hex(input: &[u8]) -> String {
    let mut data = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_le_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in data.as_chunks::<64>().0 {
        let mut words = [0u32; 16];
        for (word, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let next = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(MD5_K[i])
                    .wrapping_add(words[g])
                    .rotate_left(MD5_S[i]),
            );
            a = d;
            d = c;
            c = b;
            b = next;
        }
        for (s, v) in state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }
    state
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::collections::VecDeque;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    const QRC_HEX: &str = "ee7befb746f93831d42dcdf2e21f0d5d833c044042289012e0c11abf870c7dfb9c809065c5235ff1cf6f94cef240c69add963e0164dd48ddbfc31abddb1b167277d92192f367e70a22f0ba7ac5e81564dd377778fc4103ff";
    const QRC_TEXT: &str = "<QrcInfos><Lyric_1 LyricType=\"1\" LyricContent=\"[1000,600]hi(1000,200)!(1200,400)\"/></QrcInfos>";

    #[test]
    fn http_errors_are_classified_by_status_not_by_digits_in_the_message() {
        use crate::status::ErrorType;
        let network = crate::ProviderError::new(ErrorType::Network, "连不上 https://example.com/song?id=1401404");
        assert!(safe_http_error(network).to_string().starts_with("来源网络请求失败"));
        let not_found = crate::ProviderError::new(ErrorType::NotFound, "404");
        assert!(safe_http_error(not_found).to_string().starts_with("HTTP 404"));
        let unauthorized = crate::ProviderError::new(ErrorType::InvalidResponse, format!("{} (401)", "rejected"));
        assert!(safe_http_error(unauthorized).to_string().starts_with("HTTP 401"));
    }

    #[test]
    fn qrc_matches_pinned_lddc_reference_vector() {
        assert_eq!(decode_qrc(&unhex(QRC_HEX).unwrap()).unwrap(), QRC_TEXT);
        let k = [
            des_keys(&QRC_KEY[..8]),
            des_keys(&QRC_KEY[8..16]),
            des_keys(&QRC_KEY[16..]),
        ];
        let b = des_block(
            des_block(
                des_block([0, 1, 2, 3, 4, 5, 6, 7], &k[0], false),
                &k[1],
                true,
            ),
            &k[2],
            false,
        );
        assert_eq!(b.to_vec(), unhex("ce92cac4ea3c8406").unwrap());
    }
    #[test]
    fn local_qrc_qmc1_wrapper_is_supported() {
        let mut packed = vec![0; 11];
        packed.extend(unhex(QRC_HEX).unwrap());
        for (i, b) in packed.iter_mut().enumerate() {
            *b ^= QMC1[i & 127];
        }
        packed[..11].copy_from_slice(LOCAL_QRC_HEADER);
        assert_eq!(decode_qrc(&packed).unwrap(), QRC_TEXT);
    }
    fn krc_bytes(text: &str) -> Vec<u8> {
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(text.as_bytes()).unwrap();
        let mut out = b"krc1".to_vec();
        out.extend(
            z.finish()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ KRC_KEY[i % 16]),
        );
        out
    }
    #[test]
    fn krc_binary_decoding_and_plain_text_rejection() {
        let plain = "[1000,600]<0,200,0>hi<200,400,0>!";
        assert_eq!(decode_krc(&krc_bytes(plain)).unwrap(), plain);
        assert!(decode_krc(b"[00:01.00]hello").is_err());
        assert!(decode_qrc(b"[00:01.00]hello").is_err());
        assert!(decode_qrc(QRC_TEXT.as_bytes()).is_err());
        assert!(decode_qrc(b"bad").is_err());
        assert!(unhex("0z").is_err());
    }
    #[test]
    fn md5_rfc_vectors_and_multiple_blocks() {
        for (input, want) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(md5_hex(input.as_bytes()), want);
        }
    }

    type Recorded = Arc<Mutex<Vec<(String, Value, Vec<(String, String)>)>>>;
    struct Mock {
        steps: Mutex<VecDeque<HttpResponse>>,
        seen: Recorded,
    }
    impl Transport for Mock {
        fn get(
            &self,
            url: &str,
            headers: &[(String, String)],
            t: Timeouts,
        ) -> std::result::Result<HttpResponse, crate::ProviderError> {
            self.record(url, Value::Null, headers, t)
        }
        fn post(
            &self,
            url: &str,
            body: &[u8],
            headers: &[(String, String)],
            t: Timeouts,
        ) -> std::result::Result<HttpResponse, crate::ProviderError> {
            self.record(url, serde_json::from_slice(body).unwrap(), headers, t)
        }
    }
    impl Mock {
        fn record(
            &self,
            url: &str,
            body: Value,
            headers: &[(String, String)],
            t: Timeouts,
        ) -> std::result::Result<HttpResponse, crate::ProviderError> {
            assert!(t.total <= Duration::from_secs(8));
            self.seen
                .lock()
                .unwrap()
                .push((url.into(), body, headers.to_vec()));
            self.steps.lock().unwrap().pop_front().ok_or_else(|| {
                crate::ProviderError::network("mock exhausted; NEVER expose bearer-secret")
            })
        }
    }
    fn response(v: Value) -> HttpResponse {
        HttpResponse {
            status: 200,
            body: serde_json::to_vec(&v).unwrap(),
            ..Default::default()
        }
    }
    fn run(options: SourceOptions, steps: Vec<HttpResponse>) -> (SearchOutput, Recorded) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let result = search_with_transport(
            "Song",
            "Singer",
            Some(200.0),
            &options,
            Box::new(Mock {
                steps: Mutex::new(steps.into()),
                seen: seen.clone(),
            }),
        );
        (result, seen)
    }
    fn none() -> SourceOptions {
        SourceOptions {
            netease: false,
            qq: false,
            kugou: false,
            amll: false,
            lrclib: false,
            apple: false,
            ..Default::default()
        }
    }
    fn ne_song() -> Value {
        json!({"code":200,"result":{"songs":[{"id":123,"name":"Song","artists":[{"name":"Singer"}],"duration":200000}]}})
    }
    #[test]
    fn disabled_sources_make_no_requests_and_defaults_are_safe() {
        let (out, seen) = run(none(), vec![]);
        assert!(out.candidates.is_empty());
        assert!(out.warnings.is_empty());
        assert!(seen.lock().unwrap().is_empty());
        assert!(!SourceOptions::default().apple);
    }
    #[test]
    fn amll_only_uses_metadata_search_but_does_not_fetch_netease_lyrics() {
        let ttml = "<tt xmlns=\"http://www.w3.org/ns/ttml\"><body><div><p begin=\"1s\" end=\"2s\"><span begin=\"1s\" end=\"2s\">hi</span></p></div></body></tt>";
        let mut o = none();
        o.amll = true;
        let (out, seen) = run(
            o,
            vec![
                response(ne_song()),
                HttpResponse {
                    status: 200,
                    body: ttml.as_bytes().to_vec(),
                    ..Default::default()
                },
            ],
        );
        assert_eq!(out.candidates.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.candidates[0].source, "amll");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].0.ends_with("/ncm-lyrics/123.ttml"));
        assert!(!seen.iter().any(|r| r.0.contains("song/lyric")));
    }

    #[test]
    fn amll_raw_host_failure_uses_only_the_official_public_github_api() {
        let raw = r#"<tt><body xmlns=""><p begin="1s" end="2s"><span begin="1s" end="2s">光</span></p></body></tt>"#;
        let mut options = none();
        options.amll = true;
        let (out, seen) = run(
            options,
            vec![
                response(ne_song()),
                HttpResponse {
                    status: 503,
                    ..Default::default()
                },
                response(json!({"encoding":"base64","content":BASE64.encode(raw)})),
            ],
        );
        assert_eq!(out.candidates.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.candidates[0].raw_text, raw);
        let requests = seen.lock().unwrap();
        assert!(
            requests[2]
                .0
                .starts_with("https://api.github.com/repos/amll-dev/amll-ttml-db/contents/")
        );
        assert!(requests.iter().all(|r| {
            !r.2.iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        }));
    }
    #[test]
    fn qq_session_lite_search_and_qrc_decode_flow() {
        let mut o = none();
        o.qq = true;
        let ok = |data: Value| response(json!({"code":0,"request":{"code":0,"data":data}}));
        let (out, seen) = run(
            o,
            vec![
                ok(json!({"session":{"uid":"0","sid":"anonymous-session","userip":"0"}})),
                ok(
                    json!({"body":{"item_song":[{"id":42,"title":"Song","singer":[{"name":"Singer"}],"interval":200,"album":{"name":"Album"}}]}}),
                ),
                ok(json!({"lyric":QRC_HEX,"qrc_t":1})),
            ],
        );
        assert_eq!(out.candidates.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.candidates[0].source, "qq");
        assert_eq!(out.candidates[0].raw_text, QRC_TEXT);
        let seen = seen.lock().unwrap();
        assert_eq!(seen[1].1["request"]["method"], "DoSearchForQQMusicLite");
        assert_eq!(seen[2].1["request"]["param"]["songID"], 42);
        assert_eq!(seen[1].1["comm"]["sid"], "anonymous-session");
        assert!(
            seen[0]
                .2
                .iter()
                .any(|(k, v)| k == "Cookie" && v == "tmeLoginType=-1;")
        );
    }
    #[test]
    fn apple_is_unsupported_and_never_transmits_credentials() {
        let mut o = none();
        o.apple = true;
        o.apple_developer_token = "bearer-secret".into();
        o.apple_music_user_token = "user-secret".into();
        let (out, seen) = run(o, vec![]);
        assert!(out.candidates.is_empty());
        assert!(seen.lock().unwrap().is_empty());
        let warnings = serde_json::to_string(&out.warnings).unwrap();
        assert!(warnings.contains("不支持"));
        assert!(!warnings.contains("secret"));
    }
    #[test]
    fn a_source_failure_does_not_stop_next_source() {
        let mut o = none();
        o.netease = true;
        o.qq = true;
        let (out, seen) = run(
            o,
            vec![
                HttpResponse {
                    status: 500,
                    ..Default::default()
                },
                response(json!({"code":1})),
            ],
        );
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(out.warnings.iter().any(|w| w.source == "netease"));
        assert!(out.warnings.iter().any(|w| w.source == "qq"));
    }
    #[test]
    fn same_title_wrong_artist_is_never_fetched() {
        let mut o = none();
        o.netease = true;
        let (out, seen) = run(
            o,
            vec![response(
                json!({"code":200,"result":{"songs":[{"id":2,"name":"Song","artists":[{"name":"UnrelatedArtist"}],"duration":200000}]}}),
            )],
        );
        assert!(out.candidates.is_empty());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn explicit_recording_versions_are_rejected_before_normalization() {
        let mut o = none();
        o.netease = true;
        let mut songs = Vec::new();
        for (i, suffix) in [
            "(Live)",
            "(Acoustic)",
            "(Remix)",
            "(Studio Version)",
            "(ライブ)",
            "(现场版)",
        ]
        .iter()
        .enumerate()
        {
            songs.push(json!({"id": i + 1, "name": format!("Song {suffix}"), "artists": [{"name":"Singer"}], "duration":200000}));
        }
        let (out, seen) = run(
            o,
            vec![response(json!({"code":200,"result":{"songs":songs}}))],
        );
        assert!(out.candidates.is_empty());
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(version_markers("Believe"), 0);
        assert_eq!(version_markers("Olive"), 0);
        assert_ne!(
            version_markers("Song (Live)"),
            version_markers("Song (Studio)")
        );
        assert_ne!(
            version_markers("Song (Acoustic)"),
            version_markers("Song (Remix)")
        );
    }

    fn lrclib_record(id: u64, synced: Option<&str>, plain: Option<&str>) -> Value {
        json!({"id":id,"trackName":"Song","artistName":"Singer","duration":200.0,"instrumental":false,"syncedLyrics":synced,"plainLyrics":plain})
    }

    #[test]
    fn official_lrclib_get_then_search_keeps_genuine_unicode_word_tags() {
        let mut o = none();
        o.lrclib = true;
        let raw = "[00:01.000]<00:01.000>あ<00:01.250>いう<00:02.000>";
        let (out, seen) = run(
            o,
            vec![
                HttpResponse {
                    status: 404,
                    ..Default::default()
                },
                response(json!([lrclib_record(7, Some(raw), None)])),
            ],
        );
        assert_eq!(out.candidates.len(), 1, "{:?}", out.warnings);
        assert!(out.fallback.is_empty());
        let found = &out.candidates[0];
        assert_eq!(found.source, "lrclib");
        assert_eq!(found.raw_text, raw);
        assert_eq!(found.lyrics.lines[0].text, "あいう");
        assert_eq!(found.lyrics.lines[0].words[1].start, 1);
        assert_eq!(found.lyrics.lines[0].words[1].end, 3);
        assert_eq!(found.lyrics.lines[0].words[1].start_ms, 1250);
        assert_eq!(found.lyrics.lines[0].words[1].end_ms, Some(2000));
        let seen = seen.lock().unwrap();
        assert!(seen[0].0.starts_with("https://lrclib.net/api/get?"));
        assert!(seen[0].0.contains("duration=200"));
        assert!(seen[1].0.starts_with("https://lrclib.net/api/search?"));
    }

    #[test]
    fn lrclib_line_and_plain_are_separate_basic_fallbacks() {
        let mut o = none();
        o.lrclib = true;
        let raw = "[00:01.000]hello world";
        let line = lrclib_record(1, Some(raw), Some("hello world"));
        let plain = lrclib_record(2, None, Some("hello world"));
        let (out, _) = run(
            o,
            vec![response(line.clone()), response(json!([line, plain]))],
        );
        assert!(out.candidates.is_empty());
        assert_eq!(out.fallback.len(), 2);
        assert_eq!(out.fallback[0].raw_text, raw);
        assert_eq!(out.fallback[0].mode, "lineSynced");
        assert_eq!(out.fallback[0].format, "lrc");
        assert_eq!(out.fallback[1].mode, "plain");
        assert_eq!(out.fallback[1].format, "text");
    }

    #[test]
    fn enhanced_parser_never_fabricates_words_or_missing_ends() {
        assert!(parse_enhanced_lrc("[00:01.00]hello world").is_err());
        assert!(parse_enhanced_lrc("[00:01.00]<00:02.00>a<00:01.00>b").is_err());
        assert!(parse_enhanced_lrc("[00:01.00]untimed<00:01.00>word").is_err());
        let lyrics = parse_enhanced_lrc("[offset:500]\n[00:01.00]<00:01.00>日本語").unwrap();
        assert_eq!(lyrics.lines[0].words.len(), 1);
        assert_eq!(lyrics.lines[0].words[0].start_ms, 500);
        assert_eq!(lyrics.lines[0].words[0].end_ms, None);
    }

    #[test]
    fn kugou_mock_search_candidate_and_binary_download() {
        let mut o = none();
        o.kugou = true;
        let raw = "[1000,600]<0,200,0>hi<200,400,0>!";
        let (out, seen) = run(
            o,
            vec![
                response(
                    json!({"status":1,"error_code":0,"data":{"lists":[{"ID":"7","SongName":"Song","Singers":[{"name":"Singer"}],"Duration":200,"FileHash":"abc","AlbumName":"Album"}]}}),
                ),
                response(
                    json!({"status":200,"candidates":[{"id":"88","accesskey":"access-secret","duration":200000}]}),
                ),
                response(
                    json!({"status":200,"contenttype":0,"content":BASE64.encode(krc_bytes(raw))}),
                ),
            ],
        );
        assert_eq!(out.candidates.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.candidates[0].raw_text, raw);
        assert!(!out.candidates[0].reference.contains("secret"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen[0].0.contains("signature="));
        assert!(seen[0].0.contains("token="));
        assert!(seen[1].0.starts_with("https://lyrics.kugou.com/v1/search?"));
        assert!(seen[2].0.contains("fmt=krc"));
    }
}
