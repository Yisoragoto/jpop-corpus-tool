//! 可解释的候选打分。
//!
//! 设计要点（照搬 Python 侧 `scraper/matching.py`）：
//!
//! 1. **只对双方都有值的字段打分。** duration 未知时把它当 0 分会系统性压低
//!    所有分数、逼得阈值只能调低，最后什么都能匹配上。正确做法是让它退出
//!    计算，把权重按比例分给其余分项——`weights` 里记录的就是实际用了哪些。
//! 2. **每一次扣分都留下原因。** 分数只是一个 f64 的话，debug
//!    「为什么把原版匹配成了 Live 版」只能靠重读代码。
//! 3. **不做网络、不碰数据库、不认识 UI。** 这一层可以纯离线测试。

use std::collections::{BTreeMap, BTreeSet};

use jp_normalize::{NormalizedText, matching_key};

use crate::models::{MatchBreakdown, NormalizedTrack, Penalty, ScrapeCandidate, TrackFile};
use crate::similarity::Similarity;

/// 两个归一化键的相似度，0~1。空串一律 0。
///
/// 默认走 difflib 那条——**Python 侧实际在跑的就是它**，
/// 因为 rapidfuzz 没装。详见 [`crate::similarity`]。
pub fn string_similarity(a: &str, b: &str) -> f64 {
    string_similarity_with(a, b, Similarity::default())
}

pub fn string_similarity_with(a: &str, b: &str, algo: Similarity) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    algo.compute(a, b)
}

/// 一个是另一个的子串时给多少分。
///
/// 短的那个太短就不算数，否则「CREAM」会匹配到「クリームソーダと〜」那种长曲名。
fn containment(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() || (!b.contains(a) && !a.contains(b)) {
        return 0.0;
    }
    // Python 的 len() 数的是字符不是字节，日文串上差别很大
    let (la, lb) = (a.chars().count(), b.chars().count());
    let (short, long) = if la <= lb { (la, lb) } else { (lb, la) };
    if short < 2 {
        return 0.0;
    }
    let ratio = short as f64 / long as f64;
    if ratio >= 0.5 {
        0.80
    } else if ratio >= 0.3 {
        0.55
    } else {
        0.0
    }
}

// ────────────────────────────── 分项 ──────────────────────────────

/// 曲名相似度。
///
/// 分四档：完整键相同 → 主标题（去括号去版本后缀）相同 → 包含 → 模糊。
/// 「夜に駆ける」和「夜に駆ける (Official)」主标题相同，给 0.95 而不是 1.0，
/// 以便真正一字不差的那条能排在前面。
pub fn title_similarity(want: &NormalizedText, got: &NormalizedText) -> f64 {
    if want.is_empty() || got.is_empty() {
        return 0.0;
    }
    if want.normalized == got.normalized {
        return 1.0;
    }
    if !want.base.is_empty() && want.base == got.base {
        return 0.95;
    }
    let contained = containment(&want.base, &got.base);
    if contained > 0.0 {
        return contained;
    }
    string_similarity(&want.normalized, &got.normalized) * 0.9
}

/// 歌手相似度。
///
/// 库里的合作曲写成「キタニタツヤ/suis」，provider 那边常常只写主唱、
/// 把另一位放进曲名的 feat.，所以拆开后任一段对上就算数。
pub fn artist_similarity(want: &NormalizedText, got: &NormalizedText) -> f64 {
    if want.is_empty() || got.is_empty() {
        return 0.0;
    }
    if want.normalized == got.normalized {
        return 1.0;
    }

    let keys = |n: &NormalizedText| -> BTreeSet<String> {
        n.parts
            .iter()
            .chain(n.featured.iter())
            .filter(|p| !p.is_empty())
            .map(|p| matching_key(p))
            .filter(|k| !k.is_empty())
            .collect()
    };
    let want_parts = keys(want);
    let got_parts = keys(got);

    if want_parts.intersection(&got_parts).next().is_some() {
        return 0.92;
    }
    if want_parts
        .iter()
        .any(|p| got.normalized.contains(p.as_str()))
        || got_parts
            .iter()
            .any(|p| want.normalized.contains(p.as_str()))
    {
        return 0.82;
    }
    string_similarity(&want.normalized, &got.normalized)
}

/// 专辑相似度。
///
/// 合辑/精选集要区别对待——同一首歌收进精选集也算对得上，只是不该因此加分，
/// 所以这里给中性分而不是低分。
pub fn album_similarity(want: &NormalizedText, got: &NormalizedText) -> f64 {
    if want.is_empty() || got.is_empty() {
        return 0.0;
    }
    if want.normalized == got.normalized {
        return 1.0;
    }
    if !want.base.is_empty() && want.base == got.base {
        return 0.95;
    }
    if got.tags.contains("compilation") && !want.tags.contains("compilation") {
        return 0.5;
    }
    let contained = containment(&want.base, &got.base);
    if contained > 0.0 {
        contained
    } else {
        string_similarity(&want.normalized, &got.normalized)
    }
}

/// 时长相似度。
///
/// 2 秒内视为同一版本（不同来源的时长本来就差个一两秒），30 秒以上视为
/// 不同版本，中间线性衰减。这是区分原版 / TV size / Extended 最可靠的信号。
pub fn duration_similarity(want: Option<f64>, got: Option<f64>) -> f64 {
    duration_similarity_with(want, got, 2.0, 30.0)
}

pub fn duration_similarity_with(
    want: Option<f64>,
    got: Option<f64>,
    exact_sec: f64,
    zero_sec: f64,
) -> f64 {
    let (Some(want), Some(got)) = (want, got) else {
        return 0.0;
    };
    if want <= 0.0 || got <= 0.0 {
        return 0.0;
    }
    let delta = (want - got).abs();
    if delta <= exact_sec {
        1.0
    } else if delta >= zero_sec {
        0.0
    } else {
        1.0 - (delta - exact_sec) / (zero_sec - exact_sec)
    }
}

/// 音轨号一致性。只在双方都知道时参与计算。
pub fn track_number_similarity(want: Option<i64>, got: Option<i64>) -> f64 {
    match (want, got) {
        // Python 用 `if not want or not got`，0 也算「不知道」
        (Some(w), Some(g)) if w != 0 && g != 0 && w == g => 1.0,
        _ => 0.0,
    }
}

// ────────────────────────────── 打分器 ──────────────────────────────

/// 权重与惩罚。全部可调，方便按 provider 或按库调参。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScorerConfig {
    pub weight_title: f64,
    pub weight_artist: f64,
    pub weight_album: f64,
    pub weight_duration: f64,
    pub weight_track_number: f64,
    pub weight_provider: f64,
    /// 文件是原版、候选是 Live/Remix 等衍生版本
    pub penalty_unwanted_variant: f64,
    /// 文件是衍生版本、候选是原版。方向相反，扣得轻一点：
    /// provider 的曲名经常不标 Live，而文件名标了。
    pub penalty_missing_variant: f64,
    /// 候选没有封面。只有在「这次刮削就是为了拿封面」时才启用。
    pub penalty_no_artwork: f64,
    /// 候选专辑是精选集/合辑/BOX。扣得很轻，只在其余分项打平时起作用。
    pub penalty_compilation: f64,
    /// 时长分到这个值以上就视为「同一段录音」，据此放宽版本标记的惩罚。
    /// 时长是客观的，版本标记只是某个人打的字符串。
    pub duration_trust_threshold: f64,
    /// 分项权重小于总权重这么多时认为信息太少，整体打个折
    pub min_weight_coverage: f64,
}

impl Default for ScorerConfig {
    fn default() -> Self {
        Self {
            weight_title: 0.42,
            weight_artist: 0.28,
            weight_album: 0.08,
            weight_duration: 0.16,
            weight_track_number: 0.03,
            weight_provider: 0.03,
            penalty_unwanted_variant: 0.18,
            penalty_missing_variant: 0.10,
            penalty_no_artwork: 0.0,
            penalty_compilation: 0.04,
            duration_trust_threshold: 0.9,
            min_weight_coverage: 0.60,
        }
    }
}

impl ScorerConfig {
    /// 封面刮削专用：没有封面的候选直接排到后面去。
    pub fn for_artwork() -> Self {
        Self {
            penalty_no_artwork: 0.50,
            ..Self::default()
        }
    }

    fn total_weight(&self) -> f64 {
        self.weight_title
            + self.weight_artist
            + self.weight_album
            + self.weight_duration
            + self.weight_track_number
            + self.weight_provider
    }
}

/// 分项的权重累加顺序。**照抄 Python 侧 `weights` 字典的插入顺序**，
/// 不是字母序——浮点加法不满足结合律，顺序变了结果的最后一位就变。
const WEIGHT_ORDER: &[&str] = &[
    "title",
    "artist",
    "album",
    "duration",
    "track_number",
    "provider",
];

/// 给候选打分，并解释这个分数是怎么来的。
#[derive(Debug, Clone, Default)]
pub struct MatchScorer {
    pub config: ScorerConfig,
}

impl MatchScorer {
    pub fn new(config: ScorerConfig) -> Self {
        Self { config }
    }

    pub fn score(&self, track: &TrackFile, candidate: &ScrapeCandidate) -> MatchBreakdown {
        self.score_normalized(
            &NormalizedTrack::from_file(track),
            candidate,
            track.duration_sec,
            track.track_number,
        )
    }

    pub fn score_normalized(
        &self,
        want: &NormalizedTrack,
        candidate: &ScrapeCandidate,
        duration_sec: Option<f64>,
        track_number: Option<i64>,
    ) -> MatchBreakdown {
        let got = NormalizedTrack::from_candidate(candidate);
        let cfg = &self.config;

        let title = title_similarity(&want.title, &got.title);
        let artist = artist_similarity(&want.artist, &got.artist);
        let album = album_similarity(&want.album, &got.album);
        let duration = duration_similarity(duration_sec, candidate.duration_sec);
        let track_num = track_number_similarity(track_number, candidate.track_number);
        let provider = candidate.provider_confidence.clamp(0.0, 1.0);

        // 只有双方都有值的分项才进入加权，避免「未知」被当成「不匹配」
        let mut weights: BTreeMap<String, f64> = BTreeMap::new();
        if !want.title.is_empty() && !got.title.is_empty() {
            weights.insert("title".into(), cfg.weight_title);
        }
        if !want.artist.is_empty() && !got.artist.is_empty() {
            weights.insert("artist".into(), cfg.weight_artist);
        }
        if !want.album.is_empty() && !got.album.is_empty() {
            weights.insert("album".into(), cfg.weight_album);
        }
        if is_truthy_f(duration_sec) && is_truthy_f(candidate.duration_sec) {
            weights.insert("duration".into(), cfg.weight_duration);
        }
        if is_truthy_i(track_number) && is_truthy_i(candidate.track_number) {
            weights.insert("track_number".into(), cfg.weight_track_number);
        }
        weights.insert("provider".into(), cfg.weight_provider);

        // 曲名无法比较（任一边为空）时整条匹配就没有意义。
        // 不加这一条的话，provider 先验会独自撑起分数：一个字段全空的候选
        // 能拿到 0.7 分，直接越过 review 阈值变成「需要确认」。
        if !weights.contains_key("title") {
            return MatchBreakdown {
                title_score: r4(title),
                artist_score: r4(artist),
                album_score: r4(album),
                duration_score: r4(duration),
                track_number_score: r4(track_num),
                provider_score: r4(provider),
                weights: BTreeMap::new(),
                penalties: vec![Penalty {
                    reason: "无可比较的曲名".into(),
                    delta: 0.0,
                }],
                final_score: 0.0,
            };
        }

        let scores: BTreeMap<&str, f64> = [
            ("title", title),
            ("artist", artist),
            ("album", album),
            ("duration", duration),
            ("track_number", track_num),
            ("provider", provider),
        ]
        .into_iter()
        .collect();

        // 求和有两个坑，都实测踩到过：
        //
        // 1. **顺序**：Python 的 `weights` 是普通 dict，按插入顺序累加；
        //    这里存 BTreeMap（为了 explain() 输出稳定），字母序累加结果不同。
        // 2. **补偿**：CPython 3.12 起内置 `sum()` 对浮点走 Neumaier 补偿
        //    求和（等价于 `math.fsum`），朴素左折叠对不上——
        //    `0.42+0.08+0.16+0.03+0.03` 补偿后是 0.72，不补偿是
        //    0.7200000000000001。
        //
        // 4000 个配对里 70 个因此差 1e-4，而分数要和 0.90 / 0.55 比大小，
        // 差一位就可能把「自动采纳」变成「需要确认」。
        let used: Vec<(&str, f64)> = WEIGHT_ORDER
            .iter()
            .filter_map(|name| weights.get(*name).map(|w| (*name, *w)))
            .collect();
        let total = neumaier_sum(used.iter().map(|(_, w)| *w));
        let base = neumaier_sum(used.iter().map(|(n, w)| scores[n] * w)) / total;

        let mut penalties = self.penalties(want, &got, candidate, &scores, &weights);

        // 参与计算的信息太少（比如只有曲名）时整体打折：这种匹配本质上
        // 就是不确定的，不该拿到和「曲名+歌手+时长全中」一样的分数。
        let coverage = total / cfg.total_weight();
        if coverage < cfg.min_weight_coverage {
            let shortfall = r4((cfg.min_weight_coverage - coverage) * 0.5);
            penalties.push(Penalty {
                reason: "信息不足".into(),
                delta: -shortfall,
            });
        }

        let final_score: f64 = base + neumaier_sum(penalties.iter().map(|p| p.delta));
        MatchBreakdown {
            title_score: r4(title),
            artist_score: r4(artist),
            album_score: r4(album),
            duration_score: r4(duration),
            track_number_score: r4(track_num),
            provider_score: r4(provider),
            weights: weights.into_iter().map(|(k, v)| (k, r4(v))).collect(),
            penalties,
            final_score: r4(final_score.clamp(0.0, 1.0)),
        }
    }

    /// 打分 + 排序，分数写回每个候选的 breakdown。
    pub fn rank(
        &self,
        track: &TrackFile,
        candidates: Vec<ScrapeCandidate>,
    ) -> Vec<ScrapeCandidate> {
        self.rank_normalized(
            &NormalizedTrack::from_file(track),
            candidates,
            track.duration_sec,
            track.track_number,
        )
    }

    pub fn rank_normalized(
        &self,
        want: &NormalizedTrack,
        candidates: Vec<ScrapeCandidate>,
        duration_sec: Option<f64>,
        track_number: Option<i64>,
    ) -> Vec<ScrapeCandidate> {
        let mut scored: Vec<ScrapeCandidate> = candidates
            .into_iter()
            .map(|c| {
                let b = self.score_normalized(want, &c, duration_sec, track_number);
                c.with_breakdown(b)
            })
            .collect();
        // 分数接近时先看写法：本地是日文的，就别取罗马字那条
        // （MusicBrainz 同一张专辑常常两种写法都有：「潜潜話」和「Hisohiso Banashi」）。
        // 再同分取年份早的，也就是原始发行版而不是精选集/再版。
        let want_japanese =
            has_japanese(&want.title.original) || has_japanese(&want.artist.original);
        scored.sort_by(|a, b| {
            b.score()
                .partial_cmp(&a.score())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| script_key(a, want_japanese).cmp(&script_key(b, want_japanese)))
                .then_with(|| year_key(a).cmp(&year_key(b)))
                .then_with(|| a.title.cmp(&b.title))
        });
        scored
    }

    fn penalties(
        &self,
        want: &NormalizedTrack,
        got: &NormalizedTrack,
        candidate: &ScrapeCandidate,
        scores: &BTreeMap<&str, f64>,
        weights: &BTreeMap<String, f64>,
    ) -> Vec<Penalty> {
        let cfg = &self.config;
        let mut out = Vec::new();

        let want_tags = want.title.variant_tags();
        let mut got_tags = got.title.variant_tags();
        got_tags.extend(got.album.variant_tags());
        let extra: Vec<&String> = got_tags.difference(&want_tags).collect();
        let missing: Vec<&String> = want_tags.difference(&got_tags).collect();

        // 时长对得上就说明是同一段录音。版本标记只是某个人打的字符串，
        // provider 经常压根不标 TV size / Album Version，而时长是客观的：
        // 两者冲突时信时长，否则一大批正确匹配会被标记成「需要确认」。
        let duration_confirms = weights.contains_key("duration")
            && scores.get("duration").copied().unwrap_or(0.0) >= cfg.duration_trust_threshold;

        if !extra.is_empty() && cfg.penalty_unwanted_variant != 0.0 {
            let label = extra
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("/");
            if duration_confirms {
                out.push(Penalty {
                    reason: format!("候选多出版本标记 {label}（时长一致，减半）"),
                    delta: -cfg.penalty_unwanted_variant / 2.0,
                });
            } else {
                out.push(Penalty {
                    reason: format!("候选多出版本标记 {label}"),
                    delta: -cfg.penalty_unwanted_variant,
                });
            }
        }

        if !missing.is_empty() && cfg.penalty_missing_variant != 0.0 && !duration_confirms {
            let label = missing
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("/");
            out.push(Penalty {
                reason: format!("候选缺少版本标记 {label}"),
                delta: -cfg.penalty_missing_variant,
            });
        }

        // 同一首歌几乎总能在某张精选集/杂锦碟/BOX 里找到，而那通常不是
        // 用户想要的「这首歌属于哪张专辑」。扣分很轻：它只该在其余分项
        // 打平时起作用，不该盖过真正的匹配信号。
        if cfg.penalty_compilation != 0.0
            && got.album.tags.contains("compilation")
            && !want.album.tags.contains("compilation")
        {
            out.push(Penalty {
                reason: "候选专辑是精选集/合辑".into(),
                delta: -cfg.penalty_compilation,
            });
        }

        if cfg.penalty_no_artwork != 0.0 && candidate.artwork_url.is_empty() {
            out.push(Penalty {
                reason: "候选没有封面".into(),
                delta: -cfg.penalty_no_artwork,
            });
        }

        out
    }
}

/// CPython 3.12+ 内置 `sum()` 对浮点用的 Neumaier 补偿求和。
///
/// 朴素左折叠会在最后一位上和 Python 分叉，而这个分数要拿去和
/// 0.90 / 0.55 两个阈值比大小。
fn neumaier_sum(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for x in values {
        let t = sum + x;
        compensation += if sum.abs() >= x.abs() {
            (sum - t) + x
        } else {
            (x - t) + sum
        };
        sum = t;
    }
    sum + compensation
}

/// Python 排序键里的 `c.year or "9999"`：空串也要退到 "9999"。
/// 含平假名、片假名或汉字
pub fn has_japanese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
    })
}

/// 排序用：本地是日文时，专辑名也是日文的排前面。**只在分数相同时起作用**。
///
/// 看的是专辑名而不是曲名：罗马字化出现在发行名上（「潜潜話」/「Hisohiso Banashi」），
/// 曲名两边通常都是日文，拿它判断分不出来。
fn script_key(candidate: &ScrapeCandidate, want_japanese: bool) -> u8 {
    if !want_japanese || has_japanese(&candidate.album) {
        return 0;
    }
    1
}

fn year_key(c: &ScrapeCandidate) -> String {
    if c.year.is_empty() {
        "9999".to_string()
    } else {
        c.year.clone()
    }
}

/// Python 的 `if duration_sec and candidate.duration_sec`——0.0 也算假。
fn is_truthy_f(value: Option<f64>) -> bool {
    matches!(value, Some(v) if v != 0.0)
}

fn is_truthy_i(value: Option<i64>) -> bool {
    matches!(value, Some(v) if v != 0)
}

/// 四位小数，和 Python 的 `round(x, 4)` 同结果。
///
/// **不能先乘 10000 再 round。** 那样做有两处错：乘法本身引入舍入误差，
/// 会把并非平局的值判成平局；而 `f64::round` 平局时远离零，Python 取偶。
/// 实测 4000 个配对里有 71 个因此差 1e-4——分数要和 0.90 / 0.55 两个
/// 阈值比大小，差 1e-4 就可能把「自动采纳」变成「需要确认」。
///
/// 格式化到 4 位小数再解析回来，走的是对二进制真值的正确舍入，
/// 和 CPython 的 `_Py_dg_dtoa` 一致。
fn r4(x: f64) -> f64 {
    format!("{x:.4}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jp_normalize::{normalize_album, normalize_artist, normalize_title};

    #[test]
    fn same_score_prefers_the_local_script_over_a_romanized_release() {
        // MusicBrainz 同一张专辑常常两种写法都有：「潜潜話」和「Hisohiso Banashi」。
        // 分数一样时取和本地写法同一种文字的那条，否则库里会混进罗马字专辑名
        let want = NormalizedTrack::of("居眠り遠征隊", "ずっと真夜中でいいのに。", "");
        let mut japanese = candidate("居眠り遠征隊", "ずっと真夜中でいいのに。");
        japanese.album = "潜潜話".into();
        japanese.year = "2019".into();
        let mut romanized = candidate("居眠り遠征隊", "ずっと真夜中でいいのに。");
        romanized.album = "Hisohiso Banashi".into();
        romanized.year = "2019".into();

        let ranked = MatchScorer::default().rank_normalized(
            &want,
            vec![romanized.clone(), japanese.clone()],
            None,
            None,
        );
        assert_eq!(ranked[0].album, "潜潜話");
        assert_eq!(
            ranked[0].score(),
            ranked[1].score(),
            "只在同分时起作用，不改分数"
        );

        // 本地就是英文的，不动原来的顺序（年份早的在前）
        let want_latin = NormalizedTrack::of("Blues in the Closet", "ZUTOMAYO", "");
        let mut old = candidate("Blues in the Closet", "ZUTOMAYO");
        old.album = "Blues in the Closet".into();
        old.year = "2024".into();
        let mut newer = candidate("Blues in the Closet", "ZUTOMAYO");
        newer.album = "ブルース".into();
        newer.year = "2025".into();
        let ranked =
            MatchScorer::default().rank_normalized(&want_latin, vec![newer, old], None, None);
        assert_eq!(ranked[0].year, "2024");
    }

    fn candidate(title: &str, artist: &str) -> ScrapeCandidate {
        ScrapeCandidate {
            provider: "test".into(),
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        }
    }

    #[test]
    fn an_uncomparable_title_scores_zero() {
        // 这一条钉住的是一个真实 bug：不加它的话 provider 先验会
        // 独自把分数撑到 0.715，一个字段全空的候选直接进 review 队列。
        let want = NormalizedTrack::of("", "YOASOBI", "");
        let got = ScrapeCandidate {
            provider: "itunes".into(),
            provider_confidence: 1.0,
            ..Default::default()
        };
        let b = MatchScorer::default().score_normalized(&want, &got, None, None);
        assert_eq!(b.final_score, 0.0);
        assert!(b.weights.is_empty());
    }

    #[test]
    fn an_unknown_duration_leaves_the_calculation_instead_of_scoring_zero() {
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let mut c = candidate("夜に駆ける", "YOASOBI");
        c.provider_confidence = 1.0;

        let without = MatchScorer::default().score_normalized(&want, &c, None, None);
        assert!(!without.weights.contains_key("duration"));

        c.duration_sec = Some(261.0);
        let with = MatchScorer::default().score_normalized(&want, &c, Some(261.0), None);
        assert!(with.weights.contains_key("duration"));
        // 时长未知不该把分数拖下去
        assert!(
            without.final_score > 0.8,
            "未知时长被当成不匹配了：{}",
            without.explain()
        );
    }

    #[test]
    fn duration_distinguishes_tv_size_from_the_full_version() {
        assert_eq!(duration_similarity(Some(261.0), Some(262.0)), 1.0);
        assert_eq!(duration_similarity(Some(261.0), Some(90.0)), 0.0);
        let mid = duration_similarity(Some(261.0), Some(261.0 + 16.0));
        assert!((0.4..0.6).contains(&mid), "{mid}");
    }

    #[test]
    fn a_live_candidate_is_penalised_for_an_original_file() {
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let live = candidate("夜に駆ける (Live)", "YOASOBI");
        let studio = candidate("夜に駆ける", "YOASOBI");
        let s = MatchScorer::default();
        let a = s.score_normalized(&want, &live, None, None);
        let b = s.score_normalized(&want, &studio, None, None);
        assert!(a.final_score < b.final_score);
        assert!(
            a.penalties.iter().any(|p| p.reason.contains("live")),
            "{:?}",
            a.penalties
        );
    }

    #[test]
    fn a_matching_duration_halves_the_variant_penalty() {
        // 时长是客观的，版本标记只是某个人打的字符串
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let mut live = candidate("夜に駆ける (Live)", "YOASOBI");
        live.duration_sec = Some(261.0);
        let s = MatchScorer::default();
        let confirmed = s.score_normalized(&want, &live, Some(261.0), None);
        let penalty = confirmed
            .penalties
            .iter()
            .find(|p| p.reason.contains("live"))
            .expect("应当有版本惩罚");
        assert!((penalty.delta + 0.09).abs() < 1e-9, "{penalty:?}");
        assert!(penalty.reason.contains("减半"));
    }

    #[test]
    fn containment_ignores_a_too_short_needle() {
        // 「CREAM」不该匹配到「クリームソーダと〜」那种长曲名
        assert_eq!(containment("ab", "abcdefghijk"), 0.0);
        assert_eq!(containment("a", "ab"), 0.0);
        assert_eq!(containment("abcde", "abcdefgh"), 0.80);
    }

    #[test]
    fn a_collaborating_artist_still_matches() {
        // 库里写「キタニタツヤ/suis」，provider 只写主唱
        let want = normalize_artist("キタニタツヤ/suis");
        let got = normalize_artist("キタニタツヤ");
        assert!(artist_similarity(&want, &got) >= 0.92);
    }

    #[test]
    fn a_compilation_album_is_neutral_not_wrong() {
        let want = normalize_album("盗作");
        let got = normalize_album("THE BEST");
        assert_eq!(album_similarity(&want, &got), 0.5);
    }

    #[test]
    fn ranking_prefers_the_earlier_release_when_scores_tie() {
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let mut old = candidate("夜に駆ける", "YOASOBI");
        old.year = "2019".into();
        let mut new = candidate("夜に駆ける", "YOASOBI");
        new.year = "2023".into();
        let ranked = MatchScorer::default().rank_normalized(&want, vec![new, old], None, None);
        assert_eq!(ranked[0].year, "2019");
    }

    #[test]
    fn a_candidate_with_no_year_sorts_last_among_ties() {
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let blank = candidate("夜に駆ける", "YOASOBI");
        let mut dated = candidate("夜に駆ける", "YOASOBI");
        dated.year = "2019".into();
        let ranked = MatchScorer::default().rank_normalized(&want, vec![blank, dated], None, None);
        assert_eq!(ranked[0].year, "2019");
    }

    #[test]
    fn thin_information_is_discounted() {
        // 只有曲名对上、其余全空，不该拿到和「全中」一样的分数
        let want = NormalizedTrack::of("夜に駆ける", "", "");
        let c = candidate("夜に駆ける", "");
        let b = MatchScorer::default().score_normalized(&want, &c, None, None);
        assert!(
            b.penalties.iter().any(|p| p.reason == "信息不足"),
            "{:?}",
            b.penalties
        );
    }

    #[test]
    fn artwork_config_pushes_coverless_candidates_down() {
        let want = NormalizedTrack::of("夜に駆ける", "YOASOBI", "");
        let bare = candidate("夜に駆ける", "YOASOBI");
        let mut with_art = candidate("夜に駆ける", "YOASOBI");
        with_art.artwork_url = "https://example/600x600bb.jpg".into();
        let s = MatchScorer::new(ScorerConfig::for_artwork());
        assert!(
            s.score_normalized(&want, &bare, None, None).final_score
                < s.score_normalized(&want, &with_art, None, None).final_score
        );
    }

    #[test]
    fn title_tiers_are_ordered() {
        let want = normalize_title("夜に駆ける");
        assert_eq!(title_similarity(&want, &normalize_title("夜に駆ける")), 1.0);
        assert_eq!(
            title_similarity(&want, &normalize_title("夜に駆ける (Official)")),
            0.95
        );
    }

    #[test]
    fn summation_is_compensated_like_python_sum() {
        // CPython 3.12 起内置 sum() 对浮点走 Neumaier 补偿求和。
        // 这五个权重朴素相加是 0.7200000000000001，补偿后才是 0.72——
        // 差这一位会让 base 落到 0.9562499999… 而不是 0.95625，
        // 四舍五入后 0.9562 vs 0.9563。实测 4000 个配对里 70 个中招。
        let w = [0.42f64, 0.08, 0.16, 0.03, 0.03];
        let naive = w.iter().sum::<f64>();
        assert_ne!(naive, 0.72, "朴素求和本来就该有误差，这条在保护下面那句");
        assert_eq!(neumaier_sum(w.into_iter()), 0.72);
    }

    #[test]
    fn round_four_matches_python() {
        // 期望值全部是拿 Python 的 round(x, 4) 实跑出来的。
        // 「看起来是平局」的十进制字面量，二进制真值几乎都不在平局上——
        // 0.12345 的真值是 0.1234500000000000041…，所以进位。
        assert_eq!(r4(0.123_45), 0.1235);
        assert_eq!(r4(0.123_55), 0.1235);
        // 这个是对账里真实踩到的：先乘 10000 会误判成平局而舍到 0.9562
        assert_eq!(r4(0.956_25), 0.9563);
        // 真值在平局之下的会舍去
        assert_eq!(r4(0.111_15), 0.1111);
        assert_eq!(r4(0.5), 0.5);
    }
}
