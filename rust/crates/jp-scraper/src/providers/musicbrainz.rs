//! MusicBrainz。结构化程度最高（稳定的 MBID、录音时长、艺人类型/国家/成立年），
//! 但限每秒一次请求，超了会直接失败。
//!
//! 两个必须记住的坑：
//!
//! 1. 重试不能走普通的指数退避（0.6s 起步），那会直接违反每秒一次的限制，
//!    把「偶发失败」变成「必然失败」。退避起点必须 ≥ 限流间隔。
//! 2. `score` 字段不能用来判断是否匹配：查一个根本不存在的名字，
//!    MusicBrainz 照样会模糊匹配到某个乐队并给 100 分。判断要靠
//!    名字或别名归一化后相等。

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use jp_normalize::matching_key;

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpConfig, RateLimiter};
use crate::models::{ScrapeCandidate, SearchQuery};

use super::{ArtistInfo, MetadataProvider, as_f64, as_i64, as_str, year_of};

const RECORDING_URL: &str = "https://musicbrainz.org/ws/2/recording";
const ARTIST_URL: &str = "https://musicbrainz.org/ws/2/artist";

/// MusicBrainz 要求带上能联系到人的 UA，否则会被拒。
pub const DEFAULT_UA: &str =
    "jpop-corpus-tool/1.0 ( https://github.com/Yisoragoto/jpop-corpus-tool )";

/// 每秒最多一次，留一点余量。
pub const MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// 进程内共享。多个 worker 同时跑时也只有一条队。
pub static SHARED_RATE_LIMITER: LazyLock<Arc<RateLimiter>> =
    LazyLock::new(|| Arc::new(RateLimiter::new(MIN_INTERVAL)));

/// MusicBrainz 的查询走 Lucene 语法，特殊字符要转义，
/// 否则曲名里的括号和冒号会让整条查询变成语法错误。
pub fn lucene_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if r#"+-&|!(){}[]^"~*?:\/"#.contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Cover Art Archive 的两种取图入口
pub const CAA_RELEASE: &str = "release";
pub const CAA_RELEASE_GROUP: &str = "release-group";
/// CAA 提供 250 / 500 / 1200 三档；封面在界面上最大也就 300px，500 够用
pub const CAA_SIZE: u32 = 500;
pub const CAA_THUMB_SIZE: u32 = 250;

/// `https://coverartarchive.org/{kind}/{id}/front-{size}`。id 为空时返回空串
pub fn caa_front(kind: &str, id: &str, size: u32) -> String {
    if id.is_empty() {
        return String::new();
    }
    format!("https://coverartarchive.org/{kind}/{id}/front-{size}")
}

pub struct MusicBrainzProvider {
    http: HttpClient,
}

impl MusicBrainzProvider {
    /// 用给定的 client，不加任何默认配置。测试走这条，不会真的 sleep。
    pub fn new(http: HttpClient) -> Self {
        Self {
            http: http
                .with_provider_name("musicbrainz")
                .with_user_agent(DEFAULT_UA),
        }
    }

    /// 生产用：挂上共享限流器，退避起点 ≥ 限流间隔。
    pub fn production(http: HttpClient) -> Self {
        let mut p = Self::new(http);
        p.http = p
            .http
            .with_rate_limiter(SHARED_RATE_LIMITER.clone())
            // 退避起点必须 ≥ 限流间隔，否则重试本身就是违规请求
            .with_config(HttpConfig {
                timeout: Duration::from_secs(15),
                retries: 2,
                backoff: MIN_INTERVAL,
                ..Default::default()
            });
        p
    }

    fn to_candidate(&self, item: &serde_json::Value) -> Option<ScrapeCandidate> {
        let item = item.as_object()?;

        // artist-credit 是「A feat. B」这种拼接结构，joinphrase 是连接词
        let artist = item
            .get("artist-credit")
            .and_then(|v| v.as_array())
            .map(|credits| {
                credits
                    .iter()
                    .filter_map(|c| c.as_object())
                    .map(|c| {
                        let name = {
                            let n = as_str(c.get("name"));
                            if n.is_empty() {
                                as_str(c.get("artist").and_then(|a| a.get("name")))
                            } else {
                                n
                            }
                        };
                        format!("{name}{}", as_str(c.get("joinphrase")))
                    })
                    .collect::<String>()
            })
            .unwrap_or_default()
            .trim()
            .to_string();

        let empty = Vec::new();
        let releases = item
            .get("releases")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty);
        let release = pick_release(releases);

        let media = release
            .and_then(|r| r.get("media"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.iter().find(|m| m.is_object()));
        let track = media
            .and_then(|m| m.get("track"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.iter().find(|t| t.is_object()));

        let mut extra = std::collections::BTreeMap::new();
        let release_id = release.map(|r| as_str(r.get("id"))).unwrap_or_default();
        if let Some(r) = release {
            extra.insert("release_id".to_string(), release_id.clone());
            extra.insert(
                "release_group".to_string(),
                as_str(r.get("release-group").and_then(|g| g.get("id"))),
            );
        }
        // MusicBrainz 自己不存封面，封面在 Cover Art Archive，按 release / release-group 的 id 取。
        // **这里只拼地址、不发请求**：搜索阶段每个候选都去问一次 CAA 会慢得没法用，
        // 真正下载时按 `artwork_urls` 给的顺序挨个试（有的 release 没图、release-group 有）。
        let artwork_url = caa_front(CAA_RELEASE, &release_id, CAA_SIZE);
        let thumb_url = caa_front(CAA_RELEASE, &release_id, CAA_THUMB_SIZE);

        Some(ScrapeCandidate {
            provider: "musicbrainz".into(),
            provider_id: as_str(item.get("id")),
            title: as_str(item.get("title")),
            artist,
            album: release.map(|r| as_str(r.get("title"))).unwrap_or_default(),
            year: year_of(&release.map(|r| as_str(r.get("date"))).unwrap_or_default()),
            // length 是毫秒
            duration_sec: as_f64(item.get("length")).map(|ms| ms / 1000.0),
            track_number: track.and_then(|t| as_i64(t.get("number"))),
            disc_number: media.and_then(|m| as_i64(m.get("position"))),
            provider_confidence: self.confidence(),
            artwork_url,
            thumb_url,
            extra,
            ..Default::default()
        })
    }
}

impl MetadataProvider for MusicBrainzProvider {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    /// 结构化程度最高，先验给满。
    fn confidence(&self) -> f64 {
        1.0
    }

    fn search_tracks(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<ScrapeCandidate>, ProviderError> {
        if query.title.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut clauses = vec![format!(r#"recording:"{}""#, lucene_escape(&query.title))];
        if !query.artist.trim().is_empty() {
            clauses.push(format!(r#"artist:"{}""#, lucene_escape(&query.artist)));
        }
        if !query.album.trim().is_empty() {
            clauses.push(format!(r#"release:"{}""#, lucene_escape(&query.album)));
        }
        let url = self.http.build_url(
            RECORDING_URL,
            &[
                ("query", clauses.join(" AND ")),
                ("fmt", "json".into()),
                ("limit", limit.clamp(1, 100).to_string()),
            ],
        );

        let payload = self.http.get_json(&url)?;
        let object = payload.as_object().ok_or_else(|| {
            ProviderError::invalid_response("响应不是 JSON 对象").with_provider("musicbrainz")
        })?;
        let recordings = object.get("recordings").ok_or_else(|| {
            ProviderError::invalid_response("响应里没有 recordings 字段")
                .with_provider("musicbrainz")
        })?;
        let recordings = recordings.as_array().ok_or_else(|| {
            ProviderError::invalid_response("recordings 不是列表").with_provider("musicbrainz")
        })?;
        Ok(recordings
            .iter()
            .filter_map(|r| self.to_candidate(r))
            .collect())
    }

    fn get_track(&self, provider_id: &str) -> Result<Option<ScrapeCandidate>, ProviderError> {
        if provider_id.is_empty() {
            return Ok(None);
        }
        let url = self.http.build_url(
            &format!("{RECORDING_URL}/{provider_id}"),
            &[
                ("fmt", "json".into()),
                ("inc", "artist-credits+releases".into()),
            ],
        );
        let payload = self.http.get_json(&url)?;
        if !payload.is_object() {
            return Err(
                ProviderError::invalid_response("响应不是 JSON 对象").with_provider("musicbrainz")
            );
        }
        Ok(self.to_candidate(&payload))
    }

    /// 拿结构化的艺人信息：组合/个人、国家、成立年份、别名。
    ///
    /// 别名这一层是必须的：「森カリオペ」的条目主名是 Mori Calliope，
    /// 不看别名就永远对不上。
    fn get_artist(
        &self,
        name: &str,
        aliases: &[String],
    ) -> Result<Option<ArtistInfo>, ProviderError> {
        if name.trim().is_empty() {
            return Ok(None);
        }
        let url = self.http.build_url(
            ARTIST_URL,
            &[
                ("query", lucene_escape(name)),
                ("fmt", "json".into()),
                ("limit", "5".into()),
            ],
        );
        let payload = self.http.get_json(&url)?;
        let object = payload.as_object().ok_or_else(|| {
            ProviderError::invalid_response("响应不是 JSON 对象").with_provider("musicbrainz")
        })?;

        let accept: std::collections::BTreeSet<String> = std::iter::once(name.to_string())
            .chain(aliases.iter().cloned())
            .filter(|n| !n.is_empty())
            .map(|n| matching_key(&n))
            .collect();

        for item in object
            .get("artists")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
        {
            let Some(item) = item.as_object() else {
                continue;
            };
            let item_aliases: Vec<String> = item
                .get("aliases")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_object())
                        .map(|x| as_str(x.get("name")))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            let mut keys: std::collections::BTreeSet<String> =
                std::iter::once(matching_key(&as_str(item.get("name")))).collect();
            keys.extend(item_aliases.iter().map(|a| matching_key(a)));

            // score 不可信（不存在的名字也会拿 100 分），只认名字/别名相等
            if keys.intersection(&accept).next().is_none() {
                continue;
            }
            return Ok(Some(ArtistInfo {
                provider: "musicbrainz".into(),
                provider_id: as_str(item.get("id")),
                name: as_str(item.get("name")),
                aliases: item_aliases,
                artist_type: as_str(item.get("type")),
                country: as_str(item.get("country")),
                formed: as_str(item.get("life-span").and_then(|l| l.get("begin"))),
                ..Default::default()
            }));
        }
        Ok(None)
    }
}

/// release-group 的 secondary-type，带这些的不是「这首歌本来属于哪张专辑」。
const SECONDARY_PENALTY: &[&str] = &[
    "compilation",
    "live",
    "remix",
    "soundtrack",
    "dj-mix",
    "mixtape/street",
    "demo",
];

/// 从一条录音的多个发行版里挑最能代表「这首歌属于哪张专辑」的那个。
///
/// 直接取 `releases[0]` 是不行的——MusicBrainz 的顺序是任意的，
/// 「ダンスホール」取到的第一个就是《Halloween Mix for Kids》这种杂锦碟。
///
/// 排序规则：先排除精选集/现场辑/原声带，再取发行日期最早的
/// （原始发行版而不是再版），日期缺失的排最后。
fn pick_release(releases: &[serde_json::Value]) -> Option<&serde_json::Value> {
    releases
        .iter()
        .filter(|r| r.is_object())
        .min_by_key(|release| {
            let group = release.get("release-group");
            let secondary: Vec<String> = group
                .and_then(|g| g.get("secondary-types"))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|t| t.as_str().unwrap_or("").to_lowercase())
                        .collect()
                })
                .unwrap_or_default();
            let is_secondary = secondary
                .iter()
                .any(|t| SECONDARY_PENALTY.contains(&t.as_str()));
            let primary = as_str(group.and_then(|g| g.get("primary-type"))).to_lowercase();
            // 专辑/单曲优先于 Broadcast、Other 这类
            let primary_rank = match primary.as_str() {
                "album" | "single" => 0u8,
                "ep" => 1,
                _ => 2,
            };
            let date = {
                let d = as_str(release.get("date"));
                if d.is_empty() { "9999".to_string() } else { d }
            };
            (is_secondary, primary_rank, date)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::ScriptedTransport;
    use serde_json::json;

    fn provider(body: &str) -> MusicBrainzProvider {
        MusicBrainzProvider::new(HttpClient::new(Box::new(ScriptedTransport::new(vec![
            ScriptedTransport::ok(body),
        ]))))
    }

    #[test]
    fn lucene_special_characters_are_escaped() {
        // 不转义的话曲名里的括号和冒号会让整条查询变成语法错误
        assert_eq!(lucene_escape("夜に駆ける (Live)"), r"夜に駆ける \(Live\)");
        assert_eq!(lucene_escape("Re:Re:"), r"Re\:Re\:");
        assert_eq!(lucene_escape("A/B"), r"A\/B");
        assert_eq!(lucene_escape("普通"), "普通");
    }

    #[test]
    fn a_recording_is_parsed_with_artist_credit_joined() {
        let body = json!({"recordings":[{
            "id": "abc-123", "title": "夜に駆ける", "length": 261000,
            "artist-credit": [
                {"name": "YOASOBI", "joinphrase": " feat. "},
                {"name": "Ado", "joinphrase": ""}
            ],
            "releases": [{"id":"r1","title":"THE BOOK","date":"2021-01-06",
                          "release-group":{"primary-type":"Album"},
                          "media":[{"position":1,"track":[{"number":"3"}]}]}]
        }]})
        .to_string();
        let c = &provider(&body)
            .search_tracks(
                &SearchQuery {
                    title: "夜に駆ける".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap()[0];
        assert_eq!(c.provider_id, "abc-123");
        assert_eq!(c.artist, "YOASOBI feat. Ado");
        assert_eq!(c.album, "THE BOOK");
        assert_eq!(c.year, "2021");
        assert_eq!(c.duration_sec, Some(261.0));
        // MusicBrainz 的音轨号是字符串
        assert_eq!(c.track_number, Some(3));
        assert_eq!(c.disc_number, Some(1));
    }

    #[test]
    fn pick_release_skips_compilations() {
        // 这条钉住的是一个真实事故：直接取 releases[0]，
        // 「ダンスホール」匹配到了《Halloween Mix for Kids》
        let releases = vec![
            json!({"title":"Halloween Mix for Kids","date":"2015-10-01",
                   "release-group":{"primary-type":"Album","secondary-types":["Compilation"]}}),
            json!({"title":"ミュージック","date":"2013-01-23",
                   "release-group":{"primary-type":"Album"}}),
        ];
        let picked = pick_release(&releases).unwrap();
        assert_eq!(picked["title"], "ミュージック");
    }

    #[test]
    fn pick_release_prefers_the_earliest_original() {
        let releases = vec![
            json!({"title":"再版","date":"2020-01-01","release-group":{"primary-type":"Album"}}),
            json!({"title":"初版","date":"2013-01-23","release-group":{"primary-type":"Album"}}),
        ];
        assert_eq!(pick_release(&releases).unwrap()["title"], "初版");
    }

    #[test]
    fn a_release_without_a_date_sorts_last() {
        let releases = vec![
            json!({"title":"无日期","release-group":{"primary-type":"Album"}}),
            json!({"title":"有日期","date":"2013","release-group":{"primary-type":"Album"}}),
        ];
        assert_eq!(pick_release(&releases).unwrap()["title"], "有日期");
    }

    #[test]
    fn an_album_beats_a_broadcast() {
        let releases = vec![
            json!({"title":"广播","date":"2010","release-group":{"primary-type":"Broadcast"}}),
            json!({"title":"专辑","date":"2013","release-group":{"primary-type":"Album"}}),
        ];
        assert_eq!(pick_release(&releases).unwrap()["title"], "专辑");
    }

    #[test]
    fn a_missing_recordings_field_is_an_error() {
        let err = provider(r#"{"count":0}"#)
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::InvalidResponse);
    }

    #[test]
    fn artist_lookup_requires_a_name_or_alias_match_not_a_score() {
        // MusicBrainz 对不存在的名字也会给 100 分，score 不能用来判断
        let body = json!({"artists":[{
            "id":"x","name":"Some Other Band","score":100,
            "type":"Group","country":"JP","life-span":{"begin":"2005"}
        }]})
        .to_string();
        assert!(
            provider(&body)
                .get_artist("ヨルシカ", &[])
                .unwrap()
                .is_none(),
            "名字对不上就不该采纳，哪怕 score 是 100"
        );
    }

    #[test]
    fn artist_lookup_matches_through_aliases() {
        // 「森カリオペ」的条目主名是 Mori Calliope
        let body = json!({"artists":[{
            "id":"mbid-1","name":"Mori Calliope","score":90,
            "aliases":[{"name":"森カリオペ"}],
            "type":"Person","country":"JP","life-span":{"begin":"2020-08-12"}
        }]})
        .to_string();
        let got = provider(&body)
            .get_artist("森カリオペ", &[])
            .unwrap()
            .unwrap();
        assert_eq!(got.provider_id, "mbid-1");
        assert_eq!(got.name, "Mori Calliope");
        assert_eq!(got.artist_type, "Person");
        assert_eq!(got.formed, "2020-08-12");
    }

    #[test]
    fn the_backoff_never_violates_the_rate_limit() {
        // 退避起点小于限流间隔的话，重试本身就是违规请求，
        // 会把「偶发失败」变成「必然失败」
        let p = MusicBrainzProvider::production(HttpClient::new(Box::new(ScriptedTransport::new(
            vec![],
        ))));
        assert!(p.http.config.backoff >= MIN_INTERVAL);
    }
}
