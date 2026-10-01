//! 流水线各层之间传递的数据结构。
//!
//! ```text
//! TrackFile        本地文件「看起来是什么」（内嵌 tag + 文件名 + 目录）
//!   ↓
//! SearchQuery      要拿去问 provider 的一组词
//!   ↓
//! ScrapeCandidate  provider 返回的一条候选
//!   ↓
//! MatchBreakdown   这条候选为什么得这个分
//!   ↓
//! ResolvedTrack    最终采纳的结果（含来源和时间戳，可追溯、可重跑）
//! ```

use std::collections::BTreeMap;

use jp_normalize::{NormalizedText, normalize_album, normalize_artist, normalize_title};
use serde::{Deserialize, Serialize};

use crate::status::{ErrorType, ScrapeStatus};

// ────────────────────────────── 输入 ──────────────────────────────

/// 一个本地音频文件，以及从它身上能读到的一切。
///
/// `embedded_*` 是文件内嵌 tag 的原值，`filename_*` 是从文件名猜的。
/// 两者都保留——匹配时优先信 tag，tag 空了才退到文件名，
/// 而落库时两份都要存下来（需求：永远不覆盖原始 metadata）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackFile {
    pub path: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub embedded_title: String,
    #[serde(default)]
    pub embedded_artist: String,
    #[serde(default)]
    pub embedded_album: String,
    #[serde(default)]
    pub embedded_albumartist: String,
    #[serde(default)]
    pub embedded_year: String,
    #[serde(default)]
    pub embedded_genre: String,
    #[serde(default)]
    pub filename_title: String,
    #[serde(default)]
    pub filename_artist: String,
    #[serde(default)]
    pub folder_artist: String,
    #[serde(default)]
    pub folder_album: String,
    pub duration_sec: Option<f64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub size: Option<i64>,
    pub mtime: Option<f64>,
}

impl TrackFile {
    /// 有效曲名：tag 优先，其次文件名。
    pub fn title(&self) -> &str {
        if self.embedded_title.is_empty() {
            &self.filename_title
        } else {
            &self.embedded_title
        }
    }

    /// 有效歌手：tag → albumartist → 文件名 → 目录。
    pub fn artist(&self) -> &str {
        for candidate in [
            &self.embedded_artist,
            &self.embedded_albumartist,
            &self.filename_artist,
            &self.folder_artist,
        ] {
            if !candidate.is_empty() {
                return candidate;
            }
        }
        ""
    }

    pub fn album(&self) -> &str {
        if self.embedded_album.is_empty() {
            &self.folder_album
        } else {
            &self.embedded_album
        }
    }

    /// 有没有足够信息去搜。只要有曲名就能搜，歌手可以没有。
    pub fn has_identity(&self) -> bool {
        !self.title().trim().is_empty()
    }
}

/// 一次 provider 查询。
///
/// `kind` 标记这条查询是怎么造出来的（完整 / 去版本 / 仅曲名），
/// 调试「为什么这首歌搜不到」时能一眼看出试过哪几种写法。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    pub duration_sec: Option<f64>,
    pub track_number: Option<i64>,
    #[serde(default)]
    pub kind: String,
}

impl SearchQuery {
    /// 拼给搜索接口的查询串。
    pub fn term(&self) -> String {
        [self.artist.as_str(), self.title.as_str()]
            .iter()
            .filter(|p| !p.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.term().is_empty()
    }
}

// ────────────────────────── 候选与评分 ──────────────────────────

/// provider 返回的一条候选。
///
/// 字段刻意保持扁平且全是原值：归一化的结果不存在这里，由 `matching`
/// 层按需计算，这样换了归一化规则不用重新抓一遍网络。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeCandidate {
    pub provider: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub genre: String,
    pub duration_sec: Option<f64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    #[serde(default)]
    pub artwork_url: String,
    #[serde(default)]
    pub thumb_url: String,
    /// provider 自身的可信度先验（MusicBrainz 结构化程度高于 iTunes）
    #[serde(default = "one")]
    pub provider_confidence: f64,
    /// 打分后由 matching 层填回，未打分时为 None
    #[serde(default)]
    pub breakdown: Option<MatchBreakdown>,
    /// provider 原始响应里可能有用但没进结构的部分
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

fn one() -> f64 {
    1.0
}

impl ScrapeCandidate {
    pub fn score(&self) -> f64 {
        self.breakdown
            .as_ref()
            .map(|b| b.final_score)
            .unwrap_or(0.0)
    }

    /// 跨 provider 去重用的键。
    pub fn identity_key(&self) -> (String, String, String) {
        (
            normalize_artist(&self.artist).normalized,
            normalize_title(&self.title).normalized,
            normalize_album(&self.album).normalized,
        )
    }

    pub fn with_breakdown(mut self, breakdown: MatchBreakdown) -> Self {
        self.breakdown = Some(breakdown);
        self
    }
}

/// 可解释的匹配分数。
///
/// 需求明确要求「知道为什么匹配错」，所以每个分项和每一次扣分都留痕，
/// 整个对象会被 JSON 化存进 `scrape_state.breakdown_json`。
///
/// `weights` 里只包含**实际参与计算**的分项：duration 未知时它不该
/// 以 0 分拖低总分，而是退出计算、把权重让给其他项。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchBreakdown {
    #[serde(default)]
    pub title_score: f64,
    #[serde(default)]
    pub artist_score: f64,
    #[serde(default)]
    pub album_score: f64,
    #[serde(default)]
    pub duration_score: f64,
    #[serde(default)]
    pub track_number_score: f64,
    #[serde(default)]
    pub provider_score: f64,
    /// BTreeMap 而不是 HashMap：解释文本要稳定可比，顺序不能随机
    #[serde(default)]
    pub weights: BTreeMap<String, f64>,
    #[serde(default)]
    pub penalties: Vec<Penalty>,
    #[serde(default)]
    pub final_score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Penalty {
    pub reason: String,
    pub delta: f64,
}

impl MatchBreakdown {
    pub fn scores(&self) -> [(&'static str, f64); 6] {
        [
            ("album", self.album_score),
            ("artist", self.artist_score),
            ("duration", self.duration_score),
            ("provider", self.provider_score),
            ("title", self.title_score),
            ("track_number", self.track_number_score),
        ]
    }

    /// 给 UI / 日志用的一行人类可读说明。
    pub fn explain(&self) -> String {
        let scores: BTreeMap<&str, f64> = self.scores().into_iter().collect();
        let used = self
            .weights
            .iter()
            .map(|(name, weight)| {
                format!(
                    "{name} {:.2}×{weight:.2}",
                    scores.get(name.as_str()).copied().unwrap_or(0.0)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let penalty: String = self
            .penalties
            .iter()
            .map(|p| format!("  {} {:+.2}", p.reason, p.delta))
            .collect();
        format!("{:.3} = {used}{penalty}", self.final_score)
    }
}

// ────────────────────────────── 输出 ──────────────────────────────

/// 一次解析的最终结论。
///
/// 既是「解析成功」的载体，也是「解析失败」的载体——`status` 决定读哪些字段。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTrack {
    pub file_path: String,
    pub status: ScrapeStatus,
    pub candidate: Option<ScrapeCandidate>,
    #[serde(default)]
    pub candidates: Vec<ScrapeCandidate>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub error_type: ErrorType,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub queries_tried: Vec<String>,
    #[serde(default)]
    pub providers_tried: Vec<String>,
    #[serde(default)]
    pub matched_at: String,
}

impl ResolvedTrack {
    pub fn ok(&self) -> bool {
        self.status == ScrapeStatus::Success
    }

    pub fn needs_review(&self) -> bool {
        self.status == ScrapeStatus::LowConfidence
    }

    pub fn provider(&self) -> &str {
        self.candidate
            .as_ref()
            .map(|c| c.provider.as_str())
            .unwrap_or("")
    }

    pub fn breakdown(&self) -> Option<&MatchBreakdown> {
        self.candidate.as_ref().and_then(|c| c.breakdown.as_ref())
    }
}

/// `TrackFile` 的归一化视图。matching 层反复用到，算一次缓存住。
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedTrack {
    pub title: NormalizedText,
    pub artist: NormalizedText,
    pub album: NormalizedText,
}

impl NormalizedTrack {
    pub fn of(title: &str, artist: &str, album: &str) -> Self {
        Self {
            title: normalize_title(title),
            artist: normalize_artist(artist),
            album: normalize_album(album),
        }
    }

    pub fn from_file(track: &TrackFile) -> Self {
        Self::of(track.title(), track.artist(), track.album())
    }

    pub fn from_candidate(candidate: &ScrapeCandidate) -> Self {
        Self::of(&candidate.title, &candidate.artist, &candidate.album)
    }
}

/// ISO-8601 带时区的时间戳，和 Python 的 `utc_now()` 同形
/// （`2026-09-08T10:11:12+00:00`）。
///
/// 手算而不是引 chrono：这里只需要一个格式固定的 UTC 串，
/// 为它拖一个日期库进来不划算。
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_utc(secs)
}

fn format_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let rem = unix_secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}+00:00")
}

/// Howard Hinnant 的 days→civil 算法。闰年和世纪规则都在里面。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_values_fall_back_in_order() {
        let track = TrackFile {
            embedded_albumartist: "AlbumArtist".into(),
            filename_artist: "FromName".into(),
            folder_artist: "FromFolder".into(),
            ..Default::default()
        };
        assert_eq!(track.artist(), "AlbumArtist");

        let track = TrackFile {
            filename_artist: "FromName".into(),
            folder_artist: "FromFolder".into(),
            ..Default::default()
        };
        assert_eq!(track.artist(), "FromName");
    }

    #[test]
    fn a_title_is_enough_to_search() {
        let track = TrackFile {
            filename_title: "群青".into(),
            ..Default::default()
        };
        assert!(track.has_identity(), "有曲名就能搜，歌手可以没有");
        assert!(!TrackFile::default().has_identity());
    }

    #[test]
    fn query_term_puts_artist_first() {
        let q = SearchQuery {
            title: "夜に駆ける".into(),
            artist: "YOASOBI".into(),
            ..Default::default()
        };
        assert_eq!(q.term(), "YOASOBI 夜に駆ける");

        let q = SearchQuery {
            title: "夜に駆ける".into(),
            ..Default::default()
        };
        assert_eq!(q.term(), "夜に駆ける");
        assert!(SearchQuery::default().is_empty());
    }

    #[test]
    fn identity_key_ignores_writing_differences() {
        let a = ScrapeCandidate {
            title: "夜に 駆ける".into(),
            artist: "ＹＯＡＳＯＢＩ".into(),
            ..Default::default()
        };
        let b = ScrapeCandidate {
            title: "夜に駆ける".into(),
            artist: "YOASOBI".into(),
            ..Default::default()
        };
        assert_eq!(a.identity_key(), b.identity_key());
    }

    #[test]
    fn utc_now_has_the_same_shape_as_python() {
        // Python: datetime.now(timezone.utc).isoformat(timespec="seconds")
        let now = utc_now();
        assert_eq!(now.len(), 25, "{now}");
        assert!(now.ends_with("+00:00"), "{now}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], "T");
    }

    #[test]
    fn civil_from_days_handles_leap_years() {
        // 2000-03-01 是 946684800 + 60 天
        assert_eq!(format_utc(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(format_utc(951_868_800), "2000-03-01T00:00:00+00:00");
        // 2000 是闰年（能被 400 整除），2 月有 29 天
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00+00:00");
        // 1900 不是闰年（能被 100 整除但不能被 400 整除），
        // 所以 2 月 28 日的下一天直接是 3 月 1 日。两个时间戳差正好一天。
        assert_eq!(format_utc(-2_203_977_600), "1900-02-28T00:00:00+00:00");
        assert_eq!(format_utc(-2_203_891_200), "1900-03-01T00:00:00+00:00");
    }

    #[test]
    fn explain_lists_only_the_weights_that_counted() {
        let b = MatchBreakdown {
            title_score: 1.0,
            duration_score: 0.5,
            weights: [("title".to_string(), 0.42)].into_iter().collect(),
            penalties: vec![Penalty {
                reason: "候选是精选集".into(),
                delta: -0.04,
            }],
            final_score: 0.7,
            ..Default::default()
        };
        let text = b.explain();
        assert!(text.contains("title 1.00×0.42"), "{text}");
        assert!(
            !text.contains("duration"),
            "未参与计算的分项不该出现：{text}"
        );
        assert!(text.contains("-0.04"), "{text}");
    }
}
