//! Search is read-only; only a cached, explicitly chosen candidate can replace lyrics.
//! Apple credentials live in this session and are never returned, logged or saved.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use jp_scraper::timed_sources::{SourceOptions, SourceWarning, TimedFound};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::lyrics::{Attached, LyricsSource};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub netease: bool,
    pub qq: bool,
    pub kugou: bool,
    pub amll: bool,
    pub lrclib: bool,
    pub apple: bool,
    pub apple_storefront: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            netease: true,
            qq: true,
            kugou: true,
            amll: true,
            lrclib: true,
            apple: false,
            apple_storefront: "jp".into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    preferences: Preferences,
    apple_developer_token_set: bool,
    apple_music_user_token_set: bool,
}

// Do not derive Debug: this IPC input contains secrets.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    #[serde(flatten)]
    pub preferences: Preferences,
    pub apple_developer_token: Option<String>,
    pub apple_music_user_token: Option<String>,
}

pub struct Session {
    preferences: Preferences,
    config_path: PathBuf,
    developer_token: String,
    user_token: String,
    pending: HashMap<String, Pending>,
    sequence: u64,
    configuration_generation: u64,
    searches: HashMap<String, SearchState>,
}

struct SearchState {
    sequence: u64,
    in_flight: bool,
}

/// Opaque search identity; contains no credentials. Return it to the same Session
/// after releasing the session lock for network work.
#[derive(Debug)]
pub struct SearchTicket {
    song_id: String,
    configuration_generation: u64,
    request_sequence: u64,
}

impl Session {
    pub fn load(root: &Path) -> Self {
        let config_path = root.join("lyric-sources.json");
        let preferences = std::fs::read(&config_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Preferences>(&b).ok())
            .unwrap_or_default();
        Self {
            preferences,
            config_path,
            developer_token: String::new(),
            user_token: String::new(),
            pending: HashMap::new(),
            sequence: 0,
            configuration_generation: 0,
            searches: HashMap::new(),
        }
    }

    pub fn view(&self) -> SettingsView {
        SettingsView {
            preferences: self.preferences.clone(),
            apple_developer_token_set: !self.developer_token.is_empty(),
            apple_music_user_token_set: !self.user_token.is_empty(),
        }
    }

    pub fn configure(&mut self, mut config: SettingsInput) -> Result<SettingsView> {
        config.preferences.apple_storefront = config
            .preferences
            .apple_storefront
            .trim()
            .to_ascii_lowercase();
        ensure!(
            config.preferences.apple_storefront.len() == 2
                && config
                    .preferences
                    .apple_storefront
                    .bytes()
                    .all(|b| b.is_ascii_lowercase()),
            "Apple 地区应为两位字母，例如 jp、cn、us"
        );
        for value in [
            &config.apple_developer_token,
            &config.apple_music_user_token,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                value.len() <= 16_384 && !value.chars().any(char::is_control),
                "Apple 凭据格式不正确，请只粘贴令牌本身"
            );
        }
        // Only this allowlist of non-secret fields reaches disk.
        let bytes = serde_json::to_vec_pretty(&config.preferences)?;
        std::fs::write(&self.config_path, bytes).context("保存歌词来源设置失败")?;
        self.preferences = config.preferences;
        if let Some(token) = config.apple_developer_token {
            self.developer_token = token.trim().to_string();
        }
        if let Some(token) = config.apple_music_user_token {
            self.user_token = token.trim().to_string();
        }
        // Advance only after a successful save. Even saving identical settings
        // invalidates requests which started before the user's save action.
        self.configuration_generation = self.configuration_generation.wrapping_add(1);
        self.searches.clear();
        self.pending.clear();
        Ok(self.view())
    }

    pub fn options(&self) -> SourceOptions {
        SourceOptions {
            netease: self.preferences.netease,
            qq: self.preferences.qq,
            kugou: self.preferences.kugou,
            amll: self.preferences.amll,
            lrclib: self.preferences.lrclib,
            apple: self.preferences.apple,
            apple_storefront: self.preferences.apple_storefront.clone(),
            apple_developer_token: self.developer_token.clone(),
            apple_music_user_token: self.user_token.clone(),
        }
    }

    /// Capture settings and the latest request identity together under the
    /// session lock. Starting another search only invalidates this song's work.
    pub fn begin_search(&mut self, song_id: &str) -> (SourceOptions, SearchTicket) {
        let state = self
            .searches
            .entry(song_id.to_string())
            .or_insert(SearchState {
                sequence: 0,
                in_flight: false,
            });
        state.sequence = state.sequence.wrapping_add(1);
        state.in_flight = true;
        let ticket = SearchTicket {
            song_id: song_id.to_string(),
            configuration_generation: self.configuration_generation,
            request_sequence: state.sequence,
        };
        self.pending.retain(|_, p| p.song_id != song_id);
        (self.options(), ticket)
    }

    /// Publish only the latest search for this song and configuration. A stale
    /// response must not clear or replace candidates from a newer response.
    pub fn finish_search(
        &mut self,
        ticket: SearchTicket,
        snapshot: &Snapshot,
        found: Vec<TimedFound>,
    ) -> Result<Vec<CandidateView>> {
        ensure!(
            ticket.song_id == snapshot.song_id
                && ticket.configuration_generation == self.configuration_generation
                && self.searches.get(&ticket.song_id).is_some_and(|state| {
                    state.in_flight && state.sequence == ticket.request_sequence
                }),
            "搜索已失效：来源设置已保存、同一首歌已重新搜索或搜索已结束，请重新搜索"
        );
        let candidates = self.remember(snapshot, found);
        if let Some(state) = self.searches.get_mut(&ticket.song_id) {
            state.in_flight = false;
        }
        Ok(candidates)
    }

    /// Synchronous cache helper retained for existing callers/tests. Async
    /// searches must pair begin_search/finish_search to reject stale responses.
    pub fn remember(&mut self, snapshot: &Snapshot, found: Vec<TimedFound>) -> Vec<CandidateView> {
        self.pending.retain(|_, p| {
            p.created.elapsed() < Duration::from_secs(1200) && p.song_id != snapshot.song_id
        });
        let mut out = Vec::new();
        for candidate in found.into_iter().take(10) {
            self.sequence += 1;
            let id = format!("{}-{}", std::process::id(), self.sequence);
            let timed = candidate
                .lyrics
                .lines
                .iter()
                .filter(|l| !l.words.is_empty())
                .count();
            out.push(CandidateView {
                id: id.clone(),
                source: candidate.source.clone(),
                format: candidate.format.clone(),
                title: candidate.title.clone(),
                artist: candidate.artist.clone(),
                line_count: candidate.lyrics.lines.len(),
                timed_line_count: timed,
                mode: candidate_mode(&candidate).into(),
                preview: candidate
                    .lyrics
                    .lines
                    .iter()
                    .take(4)
                    .map(|l| l.text.clone())
                    .collect(),
                match_note: if snapshot.lines.is_empty() {
                    "新歌词，请试听核对录音版本".into()
                } else if body_key(&snapshot.text())
                    == body_key(
                        &candidate
                            .lyrics
                            .lines
                            .iter()
                            .map(|l| l.text.as_str())
                            .collect::<String>(),
                    )
                {
                    "正文一致，请试听核对时间偏移".into()
                } else {
                    "正文有差异，采用后可从歌词版本中恢复旧版".into()
                },
            });
            self.pending.insert(
                id,
                Pending {
                    created: Instant::now(),
                    song_id: snapshot.song_id.clone(),
                    fingerprint: snapshot.fingerprint(),
                    candidate,
                    automatic: false,
                },
            );
        }
        // Bound memory even if many songs were searched without applying.
        while self.pending.len() > 40 {
            if let Some(oldest) = self
                .pending
                .iter()
                .min_by_key(|(_, p)| p.created)
                .map(|(k, _)| k.clone())
            {
                self.pending.remove(&oldest);
            }
        }
        out
    }

    pub fn candidate(&self, song_id: &str, candidate_id: &str) -> Result<Pending> {
        let pending = self
            .pending
            .get(candidate_id)
            .context("候选已失效，请重新搜索逐字歌词")?;
        ensure!(
            pending.song_id == song_id && pending.created.elapsed() < Duration::from_secs(1200),
            "候选已失效或属于另一首歌，请重新搜索"
        );
        Ok(pending.clone())
    }

    pub fn forget(&mut self, song_id: &str) {
        self.pending.retain(|_, p| p.song_id != song_id);
        if let Some(state) = self.searches.get_mut(song_id) {
            state.in_flight = false;
        }
    }
}

#[derive(Clone)]
pub struct Pending {
    created: Instant,
    song_id: String,
    fingerprint: String,
    candidate: TimedFound,
    automatic: bool,
}

pub(crate) fn automatic_pending(before: &Snapshot, candidate: TimedFound) -> Pending {
    Pending { created: Instant::now(), song_id: before.song_id.clone(), fingerprint: before.fingerprint(), candidate, automatic: true }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub song_id: String,
    title: String,
    artist: String,
    lyric_lines: usize,
    timed_lines: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    id: String,
    source: String,
    format: String,
    title: String,
    artist: String,
    line_count: usize,
    timed_line_count: usize,
    mode: String,
    preview: Vec<String>,
    match_note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchView {
    pub song_id: String,
    pub title: String,
    pub current_lines: usize,
    pub current_timed_lines: usize,
    pub candidates: Vec<CandidateView>,
    pub warnings: Vec<SourceWarning>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SavedLine {
    id: i64,
    text: String,
    time: Option<f64>,
    timings: Vec<jp_corpus::word_timing::WordTiming>,
}

pub struct Snapshot {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub duration: Option<f64>,
    lines: Vec<SavedLine>,
}

impl Snapshot {
    fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect()
    }
    fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(&(&self.title, &self.artist, self.duration, &self.lines))
            .expect("finite SQLite timing data");
        format!("{:x}", Sha256::digest(bytes))
    }
    pub fn timed_lines(&self) -> usize {
        self.lines
            .iter()
            .enumerate()
            .filter(|(i, line)| {
                let next = self.lines[i + 1..]
                    .iter()
                    .filter_map(|l| l.time)
                    .find(|t| t.is_finite() && line.time.is_none_or(|s| *t > s));
                let duration = self.duration.filter(|d| d.is_finite() && *d > 0.0);
                let end = match (next, duration) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let row = jp_corpus::LyricLine {
                    utterance_id: line.id,
                    line_idx: *i as i64,
                    time_sec: line.time,
                    text: line.text.clone(),
                    tokens: Vec::new(),
                };
                jp_corpus::display_timeline::for_line(&row, &line.timings, end).mode
                    == jp_corpus::display_timeline::DisplayMode::Karaoke
            })
            .count()
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

pub fn snapshot(conn: &Connection, song_id: &str) -> Result<Snapshot> {
    let (title, artist, duration) = conn
        .query_row(
            "SELECT title, artist, duration_sec FROM songs WHERE id=?1",
            [song_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .context("找不到这首歌，可能已经删除")?;
    let mut lines: Vec<SavedLine> = conn
        .prepare(
            "SELECT id, text, time_sec FROM utterances WHERE song_id=?1 ORDER BY line_idx, id",
        )?
        .query_map([song_id], |r| {
            Ok(SavedLine {
                id: r.get(0)?,
                text: r.get(1)?,
                time: r.get(2)?,
                timings: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let ids: Vec<i64> = lines.iter().map(|l| l.id).collect();
    let mut timings = BTreeMap::<i64, Vec<_>>::new();
    for t in jp_corpus::word_timing::for_utterances(conn, &ids)? {
        timings.entry(t.utterance_id).or_default().push(t);
    }
    for line in &mut lines {
        line.timings = timings.remove(&line.id).unwrap_or_default();
    }
    Ok(Snapshot {
        song_id: song_id.into(),
        title,
        artist,
        duration,
        lines,
    })
}

pub fn targets(conn: &Connection) -> Result<Vec<Target>> {
    let ids = conn
        .prepare("SELECT id FROM songs WHERE corpus_type='song' ORDER BY artist, title")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for id in ids {
        let s = snapshot(conn, &id)?;
        let timed_lines = s.timed_lines();
        if s.lines.is_empty() || timed_lines < s.lines.len() {
            out.push(Target {
                song_id: id,
                title: s.title,
                artist: s.artist,
                lyric_lines: s.lines.len(),
                timed_lines,
            });
        }
    }
    Ok(out)
}

fn body_key(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !"、。！？!?・，,.「」『』（）()…―—-\"'♪".contains(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn candidate_mode(found: &TimedFound) -> &'static str {
    match found.format.to_ascii_lowercase().as_str() {
        "text" => "plain",
        "lrc" => "lineSynced",
        _ => "karaoke",
    }
}

pub fn from_basic(basic: jp_scraper::timed_sources::BasicFound) -> Option<TimedFound> {
    let parsed = jp_import::lrc::parse(&basic.raw_text);
    if parsed.lines.is_empty() {
        return None;
    }
    let synced = parsed.lines.iter().any(|l| l.time_sec.is_some());
    Some(TimedFound {
        source: basic.source,
        format: if synced { "LRC" } else { "TEXT" }.into(),
        reference: basic.reference,
        title: basic.title,
        artist: basic.artist,
        raw_text: basic.raw_text,
        lyrics: jp_scraper::timed_lyrics::TimedLyrics {
            lines: parsed
                .lines
                .into_iter()
                .map(|l| jp_scraper::timed_lyrics::TimedLine {
                    start_ms: l.time_sec.map(|t| (t * 1000.0).round() as u64).unwrap_or(0),
                    end_ms: None,
                    text: l.text,
                    words: Vec::new(),
                })
                .collect(),
        },
    })
}

pub(crate) fn canonical(found: &TimedFound) -> String {
    if candidate_mode(found) == "karaoke" {
        found.lyrics.to_enhanced_lrc()
    } else {
        found.raw_text.clone()
    }
}

/// Reject clearly different text and timing outside the local recording, before showing a candidate.
pub fn compatible(snapshot: &Snapshot, found: &TimedFound) -> bool {
    let high = candidate_mode(found) == "karaoke";
    if high && jp_scraper::timed_lyrics::validate(&found.lyrics).is_err() {
        return false;
    }
    // An explicitly offered fallback still must never downgrade reliable current timing.
    if !high
        && (snapshot.timed_lines() > 0
            || (candidate_mode(found) == "plain"
                && snapshot.lines.iter().any(|l| l.time.is_some())))
    {
        return false;
    }
    let text = found
        .lyrics
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<String>();
    if text.len() > 512 * 1024 || found.lyrics.lines.len() > 10_000 {
        return false;
    }
    if text.trim().is_empty() || (high && !found.lyrics.lines.iter().any(|l| !l.words.is_empty())) {
        return false;
    }
    // Preserve adjacent/embedded line lyrics as local file content, matching
    // ordinary import. The player still bounds their display by the audio.
    // Remote candidates and all native unit clocks keep strict duration checks.
    let local_lines = !high && matches!(found.source.as_str(), "embedded" | "sibling");
    if let Some(duration) = snapshot.duration.filter(|d| !local_lines && d.is_finite() && *d > 0.0) {
        let limit = (duration + 2.0) * 1000.0;
        if found.lyrics.lines.iter().any(|l| {
            l.start_ms as f64 > limit
                || l.words.iter().any(|w| {
                    w.start_ms as f64 > limit || w.end_ms.is_some_and(|e| e as f64 > limit)
                })
        }) {
            return false;
        }
    }
    let current = body_key(&snapshot.text());
    current.is_empty() || jp_scraper::similarity::difflib_ratio(&current, &body_key(&text)) >= 0.80
}

pub(crate) fn agrees_for_automatic(before: &Snapshot, found: &TimedFound) -> bool {
    let text = jp_import::lrc::parse(&canonical(found)).lines.into_iter().map(|l| l.text).collect::<String>();
    before.lines.is_empty() || jp_scraper::similarity::difflib_ratio(&body_key(&before.text()), &body_key(&text)) >= 0.97
}

fn stamp(sec: f64) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}.{:03}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    )
}

/// Recoverable backup even for legacy versions which never kept their raw text.
pub(crate) fn archive_before(conn: &Connection, snapshot: &Snapshot) -> Result<()> {
    if snapshot.lines.is_empty() {
        return Ok(());
    }
    let raw_exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM lyric_versions WHERE song_id=?1 AND is_current=1 AND raw_text IS NOT NULL)", [&snapshot.song_id], |r| r.get(0))?;
    if !raw_exists {
        let mut raw = String::new();
        for line in &snapshot.lines {
            if let Some(time) = line.time {
                raw.push_str(&format!("[{}]", stamp(time)));
            }
            let mut times = BTreeMap::new();
            for t in &line.timings {
                times.insert(t.char_start as usize, t.start_sec);
            }
            // A calibrated start takes precedence over the source time.
            for t in line.timings.iter().filter(|t| t.source == "manual") {
                times.insert(t.char_start as usize, t.start_sec);
            }
            for (i, c) in line.text.chars().enumerate() {
                if let Some(time) = times.get(&i) {
                    raw.push_str(&format!("<{}>", stamp(*time)));
                }
                raw.push(c);
            }
            raw.push('\n');
        }
        let inputs: Vec<_> = snapshot
            .lines
            .iter()
            .map(|l| jp_corpus::quality::LineInput {
                text: &l.text,
                time_sec: l.time,
            })
            .collect();
        jp_corpus::lineage::record_version(
            conn,
            &snapshot.song_id,
            &jp_import::LyricSource::new("snapshot", "升级逐字前的歌词", "", "user"),
            Some(&raw),
            &inputs,
        )?;
    }
    let version_id: i64 = conn.query_row(
        "SELECT id FROM lyric_versions WHERE song_id=?1 AND is_current=1",
        [&snapshot.song_id],
        |r| r.get(0),
    )?;
    jp_corpus::lineage::log(
        conn,
        &jp_corpus::lineage::Revision {
            song_id: snapshot.song_id.clone(),
            kind: "lyric_timing_snapshot".into(),
            version_id: Some(version_id),
            after: Some(serde_json::to_value(&snapshot.lines)?),
            actor: "system".into(),
            ..Default::default()
        },
    )?;
    Ok(())
}

pub(crate) fn version_timings(
    conn: &Connection,
    song: &str,
    version: i64,
) -> Result<Option<Vec<SavedLine>>> {
    let json: Option<String> = conn.query_row("SELECT after_json FROM corpus_revisions WHERE song_id=?1 AND version_id=?2 AND kind='lyric_timing_snapshot' ORDER BY id DESC LIMIT 1", params![song, version], |r| r.get(0)).optional()?;
    json.map(|s| serde_json::from_str(&s).context("歌词历史时间轴损坏"))
        .transpose()
}

pub(crate) fn saved_lines(lines: &[SavedLine]) -> Vec<jp_import::lrc::LyricLine> {
    lines
        .iter()
        .map(|l| jp_import::lrc::LyricLine::new(l.time, &l.text))
        .collect()
}

pub(crate) fn restore_timings(conn: &Connection, song: &str, lines: &[SavedLine]) -> Result<()> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM utterances WHERE song_id=?1 ORDER BY line_idx,id")?
        .query_map([song], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ensure!(ids.len() == lines.len(), "历史歌词的行数与时间轴不一致");
    for (id, line) in ids.iter().zip(lines) {
        for t in &line.timings {
            conn.execute("INSERT OR REPLACE INTO word_timings(utterance_id,char_start,char_end,start_sec,end_sec,source) VALUES(?1,?2,?3,?4,?5,?6)", params![id,t.char_start,t.char_end,t.start_sec,t.end_sec,t.source])?;
        }
    }
    Ok(())
}

/// User edits may change another line; unchanged occurrences retain their exact
/// timings. Moving a line's clock deliberately invalidates its previous units.
pub(crate) fn preserve_unchanged(conn: &Connection, before: &Snapshot) -> Result<()> {
    let after = snapshot(conn, &before.song_id)?;
    let mut used = HashMap::<(String, Option<u64>), usize>::new();
    for line in &after.lines {
        let occurrence = used
            .entry((line.text.clone(), line.time.map(f64::to_bits)))
            .or_default();
        if let Some(old) = before
            .lines
            .iter()
            .filter(|l| l.text == line.text && l.time == line.time)
            .nth(*occurrence)
        {
            for t in &old.timings {
                conn.execute("INSERT OR REPLACE INTO word_timings(utterance_id,char_start,char_end,start_sec,end_sec,source) VALUES(?1,?2,?3,?4,?5,?6)",params![line.id,t.char_start,t.char_end,t.start_sec,t.end_sec,t.source])?;
            }
        }
        *occurrence += 1;
    }
    Ok(())
}

pub fn apply(
    conn: &mut Connection,
    lyrics_dir: &Path,
    pending: &Pending,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    ensure!(
        !pending.song_id.is_empty()
            && !pending.song_id.contains(['/', '\\', ':'])
            && pending.song_id != "."
            && pending.song_id != "..",
        "曲目编号不能作为歌词文件名"
    );
    // An IMMEDIATE transaction closes the check/use race with a second app instance.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let before = snapshot(&tx, &pending.song_id)?;
    ensure!(!pending.automatic || !crate::lyric_upgrade::protected(&tx, &pending.song_id)?, "人工修改的歌词或校准已保留，请逐首预览后更换");
    ensure!(
        before.fingerprint() == pending.fingerprint,
        "搜索之后歌词或校准已改变，请重新搜索再采用"
    );
    ensure!(
        compatible(&before, &pending.candidate),
        "候选的正文或时间轴和本地音频不符，没有更换歌词"
    );
    let found = &pending.candidate;
    let raw = canonical(found);
    let parsed = jp_import::lrc::parse(&raw);
    ensure!(!parsed.lines.is_empty(), "候选没有可用的歌词正文");

    // Manual calibrations attach to the nth exact occurrence of a line, not just its text.
    // Reject a change which cannot preserve every calibrated line.
    let mut occurrence = HashMap::<String, usize>::new();
    let mut manual = Vec::new();
    for old in &before.lines {
        let n = occurrence.entry(old.text.clone()).or_default();
        for timing in old.timings.iter().filter(|t| t.source == "manual") {
            let new_index = parsed
                .lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.text == old.text)
                .nth(*n)
                .map(|(i, _)| i)
                .context(
                    "这份歌词改变了你校准过的行，无法保留校准，没有替换；请选正文一致的候选",
                )?;
            manual.push((new_index, timing.clone()));
        }
        *n += 1;
    }

    archive_before(&tx, &before)?;
    jp_import::maintain::clear_lyrics(&tx, &pending.song_id)?;
    let source = source(&found.source)?;
    let stats = jp_import::attach_lyrics_text(
        &tx,
        &pending.song_id,
        &raw,
        &source.lineage(&found.reference),
        analyzer,
        &before.artist,
        &before.title,
    )?;
    let native = candidate_mode(found) == "karaoke";
    let report = serde_json::json!({"status":"validated", "mode":candidate_mode(found),"native":native,"algorithmGenerated":false,"textAgreement":if before.lines.is_empty(){None}else{Some(jp_scraper::similarity::difflib_ratio(&body_key(&before.text()),&body_key(&found.lyrics.lines.iter().map(|l|l.text.as_str()).collect::<String>())))}, "manualPreserved":manual.len(), "lineCount":parsed.lines.len()});
    tx.execute("UPDATE lyric_versions SET raw_format=?2,source_raw=?3,sync_granularity=?4,timing_origin=?5,timing_validation_json=?6 WHERE song_id=?1 AND is_current=1",params![pending.song_id,found.format,found.raw_text,if native {"unit"} else if candidate_mode(found)=="plain" {"plain"} else {"line"},if native {"native"} else {"source"},report.to_string()])?;
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?
        .query_map([&pending.song_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for (i, timing) in manual {
        jp_corpus::word_timing::set_manual(
            &tx,
            ids[i],
            timing.char_start,
            timing.char_end,
            timing.start_sec,
        )?;
        tx.execute("UPDATE word_timings SET end_sec=?3 WHERE utterance_id=?1 AND char_start=?2 AND source='manual'", params![ids[i],timing.char_start,timing.end_sec])?;
    }
    std::fs::create_dir_all(lyrics_dir)?;
    let dest = lyrics_dir.join(format!("{}.lrc", pending.song_id));
    tx.execute(
        "UPDATE songs SET source_file=?2 WHERE id=?1",
        params![pending.song_id, dest.display().to_string()],
    )?;
    commit_with_file(tx, &dest, raw.as_bytes())?;
    Ok(Attached {
        song_id: pending.song_id.clone(),
        source,
        matched: format!("{} — {} ({})", found.title, found.artist, found.format),
        lyric_lines: stats.lyric_lines,
        tokens: stats.tokens,
        credits: stats.credits,
        corrections_restored: stats.corrections_restored,
        lrc_path: dest.display().to_string(),
    })
}

pub(crate) fn commit_with_file(
    tx: rusqlite::Transaction<'_>,
    dest: &Path,
    bytes: &[u8],
) -> Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    let temp = dest.with_extension(format!("lrc.{suffix}.new"));
    let backup = dest.with_extension(format!("lrc.{suffix}.previous"));
    // Permission and I/O errors must not be confused with an absent old file.
    let had_old = dest.try_exists()?;
    if had_old {
        ensure!(std::fs::metadata(dest)?.is_file(), "歌词目标不是文件");
    }
    let mut staged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    if let Err(e) = staged.write_all(bytes).and_then(|_| staged.sync_all()) {
        drop(staged);
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    drop(staged);
    if had_old && let Err(e) = std::fs::rename(dest, &backup) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    if let Err(error) = std::fs::rename(&temp, dest) {
        if had_old {
            std::fs::rename(&backup, dest).context("恢复原歌词文件失败")?;
        }
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    if let Err(error) = tx.commit() {
        std::fs::remove_file(dest)
            .context("数据库提交失败，移除新歌词文件失败；旧文件保存在 .previous 文件")?;
        if had_old {
            std::fs::rename(&backup, dest)
                .context("数据库提交失败，恢复旧歌词文件失败；请保留 .previous 文件")?;
        }
        return Err(error.into());
    }
    if had_old {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

fn source(name: &str) -> Result<LyricsSource> {
    Ok(match name {
        "embedded" => LyricsSource::Embedded,
        "sibling" => LyricsSource::Sibling,
        "netease" => LyricsSource::Netease,
        "qq" => LyricsSource::Qq,
        "kugou" => LyricsSource::Kugou,
        "amll" => LyricsSource::Amll,
        "apple" => LyricsSource::Apple,
        "lrclib" => LyricsSource::Lrclib,
        _ => bail!("未知歌词来源"),
    })
}

pub fn redact(message: &str, options: &SourceOptions) -> String {
    let mut result = message.to_string();
    for token in [
        &options.apple_developer_token,
        &options.apple_music_user_token,
    ] {
        if !token.is_empty() {
            result = result.replace(token, "[已隐藏]");
        }
    }
    result
}

/// Local files go through the same format parsers as online sources.
/// Ordinary LRC/text files keep the established encoding and parsing path.
pub struct ConvertedFile {
    pub text: String,
    pub original: String,
    pub format: String,
}

pub fn file_to_lrc(path: &Path) -> Result<Option<ConvertedFile>> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["yrc", "qrc", "krc", "ttml"].contains(&extension.as_str()) {
        return Ok(None);
    }
    ensure!(
        std::fs::metadata(path)?.len() <= 8 * 1024 * 1024,
        "歌词文件过大（最多 8 MB）"
    );
    let decoded = match extension.as_str() {
        "krc" => jp_scraper::timed_sources::decode_krc(&std::fs::read(path)?)?,
        "qrc" => {
            let bytes = std::fs::read(path)?;
            let text = crate::lyrics::read_text(path).ok();
            if let Some(xml) = text.filter(|s| s.trim_start().starts_with('<')) {
                xml
            } else {
                jp_scraper::timed_sources::decode_qrc(&bytes)?
            }
        }
        _ => crate::lyrics::read_text(path)?,
    };
    let timed = match extension.as_str() {
        "yrc" => jp_scraper::timed_lyrics::parse_yrc(&decoded)?,
        "qrc" => jp_scraper::timed_lyrics::parse_qrc(&decoded)?,
        "krc" => jp_scraper::timed_lyrics::parse_krc(&decoded)?,
        "ttml" => jp_scraper::timed_lyrics::parse_ttml(&decoded)?,
        _ => unreachable!(),
    };
    Ok(Some(ConvertedFile {
        text: timed.to_enhanced_lrc(),
        original: decoded,
        format: extension,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jp_scraper::timed_lyrics::{TimedLine, TimedLyrics, TimedWord};

    fn database() -> jp_corpus::Corpus {
        let corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        let conn = corpus.connection();
        conn.execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('timed','試験曲','テスト歌手',120)", []).unwrap();
        jp_import::attach_lyrics_text(
            conn,
            "timed",
            "[00:10.000]あの空\n[00:20.000]あの空\n",
            &jp_import::LyricSource::new("manual_file", "old.lrc", "", "user"),
            None,
            "テスト歌手",
            "試験曲",
        )
        .unwrap();
        corpus
    }

    fn candidate() -> TimedFound {
        TimedFound {
            source: "netease".into(),
            format: "YRC".into(),
            reference: "test:1".into(),
            title: "試験曲".into(),
            artist: "テスト歌手".into(),
            raw_text: "test-source-raw".into(),
            lyrics: TimedLyrics {
                lines: [10_000, 20_000]
                    .into_iter()
                    .map(|start_ms| TimedLine {
                        start_ms,
                        end_ms: Some(start_ms + 1400),
                        text: "あの空".into(),
                        words: vec![
                            TimedWord {
                                start: 0,
                                end: 2,
                                start_ms,
                                end_ms: Some(start_ms + 500),
                            },
                            TimedWord {
                                start: 2,
                                end: 3,
                                start_ms: start_ms + 700,
                                end_ms: Some(start_ms + 1400),
                            },
                        ],
                    })
                    .collect(),
            },
        }
    }

    fn session(tag: &str) -> (Session, PathBuf) {
        let dir = std::env::temp_dir().join(format!("jp-timed-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (Session::load(&dir), dir)
    }

    fn search_snapshot(song_id: &str) -> Snapshot {
        Snapshot {
            song_id: song_id.into(),
            title: "試験曲".into(),
            artist: "テスト歌手".into(),
            duration: Some(120.0),
            lines: Vec::new(),
        }
    }

    fn current_settings(session: &Session) -> SettingsInput {
        SettingsInput {
            preferences: session.preferences.clone(),
            apple_developer_token: None,
            apple_music_user_token: None,
        }
    }

    #[test]
    fn session_search_captures_options_and_rejects_results_after_configuration_save() {
        let (mut session, _) = session("search-configuration");
        let snapshot = search_snapshot("timed");
        let (old_options, old_ticket) = session.begin_search("timed");
        let mut settings = current_settings(&session);
        settings.preferences.netease = !old_options.netease;
        settings.preferences.apple = true;
        settings.preferences.apple_storefront = "us".into();
        settings.apple_developer_token = Some("synthetic-developer-token".into());
        settings.apple_music_user_token = Some("synthetic-user-token".into());
        session.configure(settings).unwrap();
        let (new_options, new_ticket) = session.begin_search("timed");
        assert_ne!(old_options.netease, new_options.netease);
        assert!(new_options.apple);
        assert_eq!(new_options.apple_storefront, "us");
        assert_eq!(
            new_options.apple_developer_token,
            "synthetic-developer-token"
        );
        assert_eq!(new_options.apple_music_user_token, "synthetic-user-token");
        assert!(old_options.apple_developer_token.is_empty());
        assert!(!format!("{new_ticket:?}").contains("synthetic-"));

        let views = session
            .finish_search(new_ticket, &snapshot, vec![candidate()])
            .unwrap();
        assert!(
            session
                .finish_search(old_ticket, &snapshot, vec![candidate()])
                .is_err()
        );
        assert_eq!(session.pending.len(), 1);
        assert!(session.candidate("timed", &views[0].id).is_ok());
        let _ = std::fs::remove_file(&session.config_path);
    }

    #[test]
    fn session_search_saving_identical_settings_also_invalidates_tickets_and_candidates() {
        let (mut session, _) = session("search-identical-configuration");
        let snapshot = search_snapshot("timed");
        let (_, ticket) = session.begin_search("timed");
        let cached = search_snapshot("cached");
        let views = session.remember(&cached, vec![candidate()]);
        let settings = current_settings(&session);
        session.configure(settings).unwrap();
        assert!(session.candidate("cached", &views[0].id).is_err());
        assert!(
            session
                .finish_search(ticket, &snapshot, vec![candidate()])
                .is_err()
        );
        assert!(session.pending.is_empty());
        let _ = std::fs::remove_file(&session.config_path);
    }

    #[test]
    fn session_search_old_response_cannot_publish_while_new_request_is_in_flight() {
        let (mut session, _) = session("search-old-first");
        let snapshot = search_snapshot("timed");
        let (_, older) = session.begin_search("timed");
        let (_, newer) = session.begin_search("timed");
        assert!(
            session
                .finish_search(older, &snapshot, vec![candidate()])
                .is_err()
        );
        assert!(session.pending.is_empty());
        let views = session
            .finish_search(newer, &snapshot, vec![candidate()])
            .unwrap();
        assert!(session.candidate("timed", &views[0].id).is_ok());
    }

    #[test]
    fn session_search_old_response_cannot_erase_newer_results() {
        let (mut session, _) = session("search-new-first");
        let snapshot = search_snapshot("timed");
        let (_, older) = session.begin_search("timed");
        let (_, newer) = session.begin_search("timed");
        let mut selected = candidate();
        selected.reference = "test:newest".into();
        let views = session
            .finish_search(newer, &snapshot, vec![selected])
            .unwrap();
        assert!(
            session
                .finish_search(older, &snapshot, vec![candidate()])
                .is_err()
        );
        assert_eq!(session.pending.len(), 1);
        assert_eq!(
            session
                .candidate("timed", &views[0].id)
                .unwrap()
                .candidate
                .reference,
            "test:newest"
        );
    }

    #[test]
    fn session_search_different_songs_finish_independently_and_new_search_clears_only_its_song() {
        let (mut session, _) = session("search-independent-songs");
        let first = search_snapshot("first");
        let second = search_snapshot("second");
        let (_, first_ticket) = session.begin_search("first");
        let (_, second_ticket) = session.begin_search("second");
        let second_views = session
            .finish_search(second_ticket, &second, vec![candidate()])
            .unwrap();
        let first_views = session
            .finish_search(first_ticket, &first, vec![candidate()])
            .unwrap();
        assert!(session.candidate("first", &first_views[0].id).is_ok());
        assert!(session.candidate("second", &second_views[0].id).is_ok());

        let (_, refresh) = session.begin_search("first");
        assert!(session.candidate("first", &first_views[0].id).is_err());
        assert!(session.candidate("second", &second_views[0].id).is_ok());
        assert!(
            session
                .finish_search(refresh, &first, Vec::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(session.pending.len(), 1);
    }

    #[test]
    fn session_search_wrong_song_snapshot_does_not_replace_existing_candidates() {
        let (mut session, _) = session("search-wrong-snapshot");
        let other = search_snapshot("other");
        let views = session.remember(&other, vec![candidate()]);
        let (_, ticket) = session.begin_search("timed");
        assert!(
            session
                .finish_search(ticket, &other, vec![candidate()])
                .is_err()
        );
        assert_eq!(session.pending.len(), 1);
        assert!(session.candidate("other", &views[0].id).is_ok());
    }

    #[test]
    fn session_search_failed_configuration_save_keeps_in_flight_request_valid() {
        let (mut session, dir) = session("search-failed-configuration");
        let snapshot = search_snapshot("timed");
        let (_, ticket) = session.begin_search("timed");
        let mut invalid = current_settings(&session);
        invalid.preferences.apple_storefront = "not-a-storefront".into();
        assert!(session.configure(invalid).is_err());
        // Writing a config to a directory fails after input validation.
        session.config_path = dir;
        let settings = current_settings(&session);
        assert!(session.configure(settings).is_err());
        let views = session
            .finish_search(ticket, &snapshot, vec![candidate()])
            .unwrap();
        assert!(session.candidate("timed", &views[0].id).is_ok());
    }

    #[test]
    fn session_search_forgetting_a_song_invalidates_its_in_flight_request() {
        let (mut session, _) = session("search-forget");
        let snapshot = search_snapshot("timed");
        let other = search_snapshot("other");
        let (_, ticket) = session.begin_search("timed");
        let (_, other_ticket) = session.begin_search("other");
        session.forget("timed");
        assert!(
            session
                .finish_search(ticket, &snapshot, vec![candidate()])
                .is_err()
        );
        assert!(
            session
                .finish_search(other_ticket, &other, vec![candidate()])
                .is_ok()
        );
        let (_, replacement) = session.begin_search("timed");
        assert!(
            session
                .finish_search(replacement, &snapshot, vec![candidate()])
                .is_ok()
        );
    }

    #[test]
    fn settings_persist_no_tokens_and_never_return_them() {
        let (mut session, dir) = session("secrets");
        let input = SettingsInput {
            preferences: Preferences {
                apple: true,
                ..Default::default()
            },
            apple_developer_token: Some("private-developer-token".into()),
            apple_music_user_token: Some("private-user-token".into()),
        };
        let view = serde_json::to_string(&session.configure(input).unwrap()).unwrap();
        let saved = std::fs::read_to_string(&session.config_path).unwrap();
        assert!(!view.contains("private-") && !saved.contains("private-"));
        assert!(view.contains("\"appleDeveloperTokenSet\":true"));
        let loaded = Session::load(&dir);
        assert!(
            loaded.preferences.apple
                && loaded.developer_token.is_empty()
                && loaded.user_token.is_empty()
        );
        assert_eq!(
            redact(
                "oops private-developer-token private-user-token",
                &session.options()
            ),
            "oops [已隐藏] [已隐藏]"
        );
        let _ = std::fs::remove_file(session.config_path);
    }

    #[test]
    fn targets_include_existing_line_lyrics_and_search_never_writes_them() {
        let corpus = database();
        let before = snapshot(corpus.connection(), "timed").unwrap();
        assert_eq!(targets(corpus.connection()).unwrap().len(), 1);
        let (mut session, _) = session("readonly");
        let views = session.remember(&before, vec![candidate()]);
        assert_eq!(views[0].timed_line_count, 2);
        assert_eq!(
            before.fingerprint(),
            snapshot(corpus.connection(), "timed")
                .unwrap()
                .fingerprint()
        );
        assert!(session.candidate("other-song", &views[0].id).is_err());
    }

    #[test]
    fn an_edit_since_search_invalidates_the_candidate() {
        let mut corpus = database();
        let before = snapshot(corpus.connection(), "timed").unwrap();
        let (mut session, dir) = session("stale");
        let views = session.remember(&before, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        jp_corpus::word_timing::set_manual(corpus.connection(), before.lines[0].id, 0, 2, 10.3)
            .unwrap();
        let error = apply(corpus.connection_mut(), &dir, &pending, None).unwrap_err();
        assert!(error.to_string().contains("已改变"));
        assert_eq!(
            snapshot(corpus.connection(), "timed")
                .unwrap()
                .timed_lines(),
            0
        );
    }

    #[test]
    fn upgrade_preserves_repeated_line_calibrations_history_and_real_end_times() {
        let mut corpus = database();
        let initial = snapshot(corpus.connection(), "timed").unwrap();
        jp_corpus::word_timing::set_manual(corpus.connection(), initial.lines[1].id, 0, 2, 20.25)
            .unwrap();
        let before = snapshot(corpus.connection(), "timed").unwrap();
        let (mut session, dir) = session("apply");
        let views = session.remember(&before, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        apply(corpus.connection_mut(), &dir, &pending, None).unwrap();
        let after = snapshot(corpus.connection(), "timed").unwrap();
        assert_eq!(after.timed_lines(), 2);
        assert!(after.lines[0].timings.iter().all(|t| t.source != "manual"));
        assert!(
            after.lines[1]
                .timings
                .iter()
                .any(|t| t.source == "manual" && t.start_sec == 20.25)
        );
        assert!(
            after.lines[0]
                .timings
                .iter()
                .any(|t| t.source == "lrc" && t.end_sec == Some(10.5))
        );
        let old: String = corpus.connection().query_row("SELECT raw_text FROM lyric_versions WHERE song_id='timed' AND is_current=0 ORDER BY version_no LIMIT 1", [], |r| r.get(0)).unwrap();
        assert!(old.contains("[00:10.000]あの空"));
        assert!(targets(corpus.connection()).unwrap().is_empty());
        assert!(
            apply(corpus.connection_mut(), &dir, &pending, None).is_err(),
            "同一候选不能重复采用"
        );
    }

    #[test]
    fn losing_a_calibration_or_failing_to_write_does_not_replace_lyrics() {
        let mut corpus = database();
        let initial = snapshot(corpus.connection(), "timed").unwrap();
        jp_corpus::word_timing::set_manual(corpus.connection(), initial.lines[0].id, 0, 2, 10.25)
            .unwrap();
        let before = snapshot(corpus.connection(), "timed").unwrap();
        let (mut session, dir) = session("rollback");
        let mut changed = candidate();
        changed.lyrics.lines[0].text = "あの海".into();
        let views = session.remember(&before, vec![changed]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        assert!(apply(corpus.connection_mut(), &dir, &pending, None).is_err());
        assert_eq!(
            before.fingerprint(),
            snapshot(corpus.connection(), "timed")
                .unwrap()
                .fingerprint()
        );

        let views = session.remember(&before, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        let blocker = dir.join("not-a-directory");
        std::fs::write(&blocker, b"block").unwrap();
        assert!(apply(corpus.connection_mut(), &blocker, &pending, None).is_err());
        assert_eq!(
            before.fingerprint(),
            snapshot(corpus.connection(), "timed")
                .unwrap()
                .fingerprint()
        );
        let _ = std::fs::remove_file(blocker);
    }

    #[test]
    fn unrelated_text_or_timing_past_the_audio_is_not_offered() {
        let corpus = database();
        let before = snapshot(corpus.connection(), "timed").unwrap();
        let mut wrong = candidate();
        wrong.lyrics.lines[0].text = "全然別の作品です".into();
        assert!(!compatible(&before, &wrong));
        wrong = candidate();
        wrong.lyrics.lines[0].words[0].start_ms = 200_000;
        assert!(!compatible(&before, &wrong));
    }

    #[test]
    fn restoring_a_version_restores_second_chorus_calibration_and_source_ends() {
        let mut corpus = database();
        let s = snapshot(corpus.connection(), "timed").unwrap();
        jp_corpus::word_timing::set_manual(corpus.connection(), s.lines[1].id, 0, 2, 20.25)
            .unwrap();
        corpus
            .connection()
            .execute(
                "UPDATE word_timings SET end_sec=20.6 WHERE source='manual'",
                [],
            )
            .unwrap();
        let s = snapshot(corpus.connection(), "timed").unwrap();
        let old: i64 = corpus
            .connection()
            .query_row(
                "SELECT id FROM lyric_versions WHERE is_current=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let (mut session, dir) = session("restore");
        let views = session.remember(&s, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        apply(corpus.connection_mut(), &dir, &pending, None).unwrap();
        crate::lyrics::restore_version(corpus.connection_mut(), &dir, "timed", old, None).unwrap();
        let after = snapshot(corpus.connection(), "timed").unwrap();
        assert!(after.lines[0].timings.is_empty());
        assert_eq!(after.lines[1].timings.len(), 1);
        let t = &after.lines[1].timings[0];
        assert_eq!(
            (t.source.as_str(), t.start_sec, t.end_sec),
            ("manual", 20.25, Some(20.6))
        );
    }

    #[test]
    fn failed_database_commit_keeps_original_file_and_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
        let (_, dir) = session("deferred-commit");
        let dest = dir.join("song.lrc");
        std::fs::write(&dest, b"original").unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute("INSERT INTO child(id) VALUES(42)", []).unwrap();
        assert!(commit_with_file(tx, &dest, b"replacement").is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
        let n: i64 = conn
            .query_row("SELECT count(*) FROM child", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn line_and_plain_fallback_never_downgrade_current_word_timing() {
        let mut corpus = database();
        let (mut session, dir) = session("fallback");
        let s = snapshot(corpus.connection(), "timed").unwrap();
        let views = session.remember(&s, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        apply(corpus.connection_mut(), &dir, &pending, None).unwrap();
        let mut line = candidate();
        line.format = "LRC".into();
        for l in &mut line.lyrics.lines {
            l.words.clear();
        }
        assert!(!compatible(
            &snapshot(corpus.connection(), "timed").unwrap(),
            &line
        ));
    }

    #[test]
    fn editing_first_chorus_preserves_exact_units_on_unchanged_second_chorus() {
        let mut corpus = database();
        let s = snapshot(corpus.connection(), "timed").unwrap();
        let (mut session, dir) = session("edit-unchanged");
        let views = session.remember(&s, vec![candidate()]);
        let pending = session.candidate("timed", &views[0].id).unwrap();
        apply(corpus.connection_mut(), &dir, &pending, None).unwrap();
        let edits = [
            crate::lyrics::EditedLine {
                time_sec: Some(10.0),
                text: "あの海".into(),
            },
            crate::lyrics::EditedLine {
                time_sec: Some(20.0),
                text: "あの空".into(),
            },
        ];
        crate::lyrics::save_edited_lines(
            corpus.connection_mut(),
            &dir,
            "timed",
            &edits,
            "test",
            None,
        )
        .unwrap();
        let after = snapshot(corpus.connection(), "timed").unwrap();
        assert!(after.lines[0].timings.is_empty());
        assert_eq!(after.lines[1].timings.len(), 2);
        assert_eq!(after.lines[1].timings[0].start_sec, 20.0);
        assert_eq!(after.lines[1].timings[0].end_sec, Some(20.5));
    }

    #[test]
    #[ignore = "explicit public-network verification; never opens the user's library"]
    fn public_sources_attach_real_units_to_an_isolated_library() {
        for provider in ["qq", "kugou", "amll"] {
            let options = SourceOptions {
                netease: false,
                qq: provider == "qq",
                kugou: provider == "kugou",
                amll: provider == "amll",
                lrclib: false,
                apple: false,
                ..Default::default()
            };
            let found =
                jp_scraper::timed_sources::search("夜に駆ける", "YOASOBI", Some(261.0), &options);
            let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
            corpus.ensure_schema().unwrap();
            corpus.connection().execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('network-word','夜に駆ける','YOASOBI',261)",[]).unwrap();
            let before = snapshot(corpus.connection(), "network-word").unwrap();
            let accepted: Vec<_> = found
                .candidates
                .into_iter()
                .filter(|c| compatible(&before, c))
                .collect();
            assert!(
                !accepted.is_empty(),
                "{provider}: no compatible real units; warnings={:?}",
                found.warnings
            );
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("jp-live-{}-{unique}", std::process::id()));
            std::fs::create_dir(&dir).unwrap();
            let mut session = Session::load(&dir);
            let views = session.remember(&before, accepted);
            let pending = session.candidate("network-word", &views[0].id).unwrap();
            let attached = apply(corpus.connection_mut(), &dir, &pending, None).unwrap();
            let after = snapshot(corpus.connection(), "network-word").unwrap();
            assert!(
                after.timed_lines() > 0,
                "{provider}: attachment produced no playable high-precision lines"
            );
            println!(
                "{provider}: {} lyric lines, {} validated karaoke lines",
                attached.lyric_lines,
                after.timed_lines()
            );
            std::fs::remove_file(dir.join("network-word.lrc")).unwrap();
            std::fs::remove_dir(dir).unwrap();
        }
    }
}
