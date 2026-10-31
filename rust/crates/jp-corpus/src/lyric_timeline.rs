//! 统一歌词时间轴：不管歌词来自 LRC、增强 LRC、SRT、纯文本还是逐字格式，
//! 显示、跳转、同步都读这一份结构。
//!
//! ## 三种时间
//!
//! | 名字 | 是什么 | 在哪 |
//! |---|---|---|
//! | 歌词时间 | 歌词文件里写的时间戳 | 本模块的 `start_ms` / `end_ms`、来源原文 |
//! | 曲目时间 | 这首歌从头算起的播放位置 | 播放器位置、`utterances.time_sec` |
//! | 文件时间 | 音频文件里的位置 | 只有一个文件装多首歌（CUE 拆轨）时和曲目时间不同 |
//!
//! 曲目时间 = 歌词时间 + 整体偏移；文件时间 = 曲目时间 + 曲目在文件里的起点。
//! 换算只走 [`lyric_to_track_ms`] / [`track_to_lyric_ms`] / [`track_to_file_ms`]，
//! 显示当前行和点行跳转用的是同一对函数，偏移改过之后也跳得准。
//!
//! 偏移落库时已经加进了 `utterances.time_sec`（见 `lyric_sources::set_offset`）：
//! 制卡截音频、单句循环、KWIC 试听读的都是那一列，不用每处各自记得加偏移。
//! 原文一个字不改，所以偏移随时能改回去。
//!
//! ## 不造时间
//!
//! * 纯文本没有时间戳就是 `Precision::None`，一行都不编；
//! * 行的结束时间只取「下一条有效歌词的开始」或「已知的歌曲时长」，
//!   都没有就留空，不按字数估；
//! * 逐字时间只收来源给的，没有就是逐行，不把一行均摊给每个字。

use serde::{Deserialize, Serialize};

/// 解析规则的版本。改了解析或时间轴规则就加一，旧缓存会被重新生成。
pub const PARSER_VERSION: i64 = 1;

/// 时间戳最多可以比曲长多出这么多（毫秒）。有的 LRC 末尾留一个空时间戳当结束标记，
/// 稍微超出一点很常见；超出太多说明时间轴不属于这一版音频。
const OVER_DURATION_TOLERANCE_MS: i64 = 2_000;

/// 两份歌词的同一行，开始时间差在这个范围内算对得上（毫秒）。
/// 网易云的原文和译文时间戳是同一个值，QQ 音乐偶尔差一两个厘秒。
const ALIGN_TOLERANCE_MS: i64 = 50;

/// 译文至少有这么大比例的行能按时间对上原文，才算「已对齐」。
const ALIGN_MIN_RATIO: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Precision {
    /// 一行时间都没有：只能看，不能跟着播放走
    None,
    /// 逐行时间
    Line,
    /// 至少有一行带来源给的逐字 / 逐词时间
    Word,
}

/// 一段逐字时间：行内第 `char_start..char_end` 个字（Unicode 标量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineWord {
    pub char_start: usize,
    pub char_end: usize,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineLine {
    /// 稳定标识：同一份内容重新解析，标识不变（时间 + 原文 + 第几次出现）
    pub id: String,
    /// 这一行已经入库时对应的 `utterances.id`；来源预览里是空的
    pub utterance_id: Option<i64>,
    /// 歌词时间（毫秒）。没有时间戳的行是空的
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub text: String,
    pub translation: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub words: Vec<TimelineWord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranslationState {
    /// 没有译文
    None,
    /// 按时间戳对上了
    Aligned,
    /// 对不上（没有时间戳、或者对上的太少）：两份都留着，不按行号硬拼
    Unaligned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationStatus {
    pub state: TranslationState,
    /// zh-Hans 之类；同一个文件里夹带的译文是空串
    pub language: String,
    /// 对上了几行
    pub aligned: usize,
    /// 译文一共几行
    pub total: usize,
    /// 对不上时原样留着的译文，按原来的顺序
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unaligned_lines: Vec<String>,
}

impl Default for TranslationStatus {
    fn default() -> Self {
        Self { state: TranslationState::None, language: String::new(), aligned: 0, total: 0, unaligned_lines: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricTimeline {
    pub song_id: String,
    /// `lyric_sources.id`；老库里还没登记来源的是空的
    pub source_id: Option<i64>,
    /// user / sibling / embedded / online / legacy
    pub source_kind: String,
    /// lrc / enhanced_lrc / srt / text / yrc / qrc / krc / ttml …
    pub raw_format: String,
    pub precision: Precision,
    /// 整体偏移（毫秒）：曲目时间 = 歌词时间 + 偏移
    pub offset_ms: i64,
    /// 曲目在音频文件里的起点（毫秒）。一个文件一首歌时是 0
    pub track_start_ms: i64,
    pub duration_ms: Option<i64>,
    pub parser_version: i64,
    pub lines: Vec<TimelineLine>,
    pub translation: TranslationStatus,
    /// 解析时发现的问题，一条一句人话
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// 喂给 [`build`] 的一行：来自任何格式的解析结果，时间是秒。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawLine {
    pub start_sec: Option<f64>,
    /// 来源明确给了结束时间（SRT）才有
    pub end_sec: Option<f64>,
    pub text: String,
    /// (char_start, char_end, start_sec, end_sec)
    pub words: Vec<(usize, usize, f64, Option<f64>)>,
    pub utterance_id: Option<i64>,
}

impl RawLine {
    pub fn new(start_sec: Option<f64>, text: impl Into<String>) -> Self {
        Self { start_sec, text: text.into(), ..Default::default() }
    }
}

// ── 时间换算 ──────────────────────────────────────────────────────

pub fn lyric_to_track_ms(lyric_ms: i64, offset_ms: i64) -> i64 {
    lyric_ms + offset_ms
}

pub fn track_to_lyric_ms(track_ms: i64, offset_ms: i64) -> i64 {
    track_ms - offset_ms
}

pub fn track_to_file_ms(track_ms: i64, track_start_ms: i64) -> i64 {
    track_ms + track_start_ms
}

pub fn file_to_track_ms(file_ms: i64, track_start_ms: i64) -> i64 {
    file_ms - track_start_ms
}

fn to_ms(sec: f64) -> Option<i64> {
    sec.is_finite().then(|| (sec * 1000.0).round() as i64)
}

/// 已经排好序的开始时间里，`t` 落在哪一行：最后一个 `start <= t` 的下标。
/// 第一行之前是 `None`。二分查找，每帧调也不贵。
pub fn line_at(starts: &[i64], t: i64) -> Option<usize> {
    let after = starts.partition_point(|&s| s <= t);
    after.checked_sub(1)
}

// ── 构建 ──────────────────────────────────────────────────────────

fn line_id(start_ms: Option<i64>, text: &str, occurrence: usize) -> String {
    // FNV-1a，够区分一首歌里的几十行，而且不依赖进程的随机种子
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    eat(start_ms.map(|t| t.to_string()).unwrap_or_default().as_bytes());
    eat(text.as_bytes());
    eat(occurrence.to_string().as_bytes());
    format!("{h:016x}")
}

/// 把解析结果整理成时间轴。
///
/// * 输入不要求排好序：有时间的按时间排（同一时间保持原顺序），没时间的按原顺序排在后面；
/// * 非有限值、负数、比曲长多出太多的时间戳算无效：这一行留着、时间清空，并记一条警告；
/// * 结束时间：来源给了就用（不早于开始），否则取下一条**更晚**的有效开始时间
///   （同一时间戳的几行共用一个结束），最后一行用曲长，曲长不知道就留空。
pub fn build(song_id: &str, lines: Vec<RawLine>, duration_sec: Option<f64>, offset_ms: i64) -> LyricTimeline {
    let duration_ms = duration_sec.filter(|d| d.is_finite() && *d > 0.0).and_then(to_ms);
    let mut warnings = Vec::new();
    let mut invalid = 0usize;
    let mut over = 0usize;

    let mut prepared: Vec<(Option<i64>, Option<i64>, RawLine)> = Vec::with_capacity(lines.len());
    for line in lines {
        if line.text.trim().is_empty() {
            continue;
        }
        let mut start = line.start_sec.and_then(|s| if s.is_finite() && s >= 0.0 { to_ms(s) } else { None });
        if line.start_sec.is_some() && start.is_none() {
            invalid += 1;
        }
        if let (Some(s), Some(d)) = (start, duration_ms)
            && s > d + OVER_DURATION_TOLERANCE_MS {
                over += 1;
                start = None;
            }
        let end = line.end_sec.and_then(|e| if e.is_finite() { to_ms(e) } else { None });
        prepared.push((start, end, line));
    }
    if invalid > 0 {
        warnings.push(format!("{invalid} 行的时间戳无效，按无时间处理"));
    }
    if over > 0 {
        warnings.push(format!("{over} 行的时间超出了歌曲时长，可能不是这一版音频的时间轴"));
    }

    // 稳定排序：有时间的在前按时间，没时间的保持原顺序
    prepared.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let starts: Vec<i64> = prepared.iter().filter_map(|p| p.0).collect();
    let mut seen: std::collections::HashMap<(Option<i64>, String), usize> = std::collections::HashMap::new();
    let mut out_lines = Vec::with_capacity(prepared.len());
    let mut has_words = false;
    let mut bad_end = 0usize;
    for (start, explicit_end, raw) in prepared {
        let next_start = start.and_then(|s| starts.get(starts.partition_point(|&x| x <= s)).copied());
        let end = match start {
            None => None,
            Some(s) => {
                let fallback = next_start.or(duration_ms.filter(|&d| d > s));
                match explicit_end {
                    Some(e) if e > s => Some(e),
                    Some(_) => {
                        bad_end += 1;
                        fallback
                    }
                    None => fallback,
                }
            }
        };
        let words = match start {
            None => Vec::new(),
            Some(_) => words_of(&raw, end),
        };
        has_words |= !words.is_empty();
        let key = (start, raw.text.clone());
        let n = seen.entry(key).or_insert(0);
        let id = line_id(start, &raw.text, *n);
        *n += 1;
        out_lines.push(TimelineLine {
            id,
            utterance_id: raw.utterance_id,
            start_ms: start,
            end_ms: end,
            text: raw.text,
            translation: None,
            words,
        });
    }
    if bad_end > 0 {
        warnings.push(format!("{bad_end} 行的结束时间不晚于开始时间，改用下一行的开始"));
    }
    let precision = if has_words {
        Precision::Word
    } else if out_lines.iter().any(|l| l.start_ms.is_some()) {
        Precision::Line
    } else {
        Precision::None
    };
    LyricTimeline {
        song_id: song_id.to_string(),
        source_id: None,
        source_kind: String::new(),
        raw_format: String::new(),
        precision,
        offset_ms,
        track_start_ms: 0,
        duration_ms,
        parser_version: PARSER_VERSION,
        lines: out_lines,
        translation: TranslationStatus::default(),
        warnings,
    }
}

/// 来源给的逐字时间。下标越界、时间无效、范围重叠的整行不要：
/// 半截可信的逐字时间显示出来就是乱跳，不如退回逐行。
fn words_of(raw: &RawLine, line_end: Option<i64>) -> Vec<TimelineWord> {
    let len = raw.text.chars().count();
    let mut words: Vec<TimelineWord> = Vec::with_capacity(raw.words.len());
    for &(char_start, char_end, start, end) in &raw.words {
        let Some(start_ms) = to_ms(start).filter(|_| start >= 0.0) else { return Vec::new() };
        if char_start >= char_end || char_end > len {
            return Vec::new();
        }
        if let Some(prev) = words.last()
            && (char_start < prev.char_end || start_ms < prev.start_ms) {
                return Vec::new();
            }
        words.push(TimelineWord { char_start, char_end, start_ms, end_ms: end.and_then(to_ms) });
    }
    // 没写结束的字：到下一个字开始，最后一个字到行尾
    let n = words.len();
    for i in 0..n {
        if words[i].end_ms.is_none_or(|e| e <= words[i].start_ms) {
            words[i].end_ms = if i + 1 < n { Some(words[i + 1].start_ms) } else { line_end.filter(|&e| e > words[i].start_ms) };
        }
    }
    words
}

/// 把译文按时间戳挂到原文上。
///
/// 每一行译文找开始时间差在 50ms 内、最近的原文行；一行原文只收一行译文。
/// 对上的行数不到译文的八成，或者译文根本没有时间戳，就算对不上：
/// 原文一个字不动，译文原样放进 `unaligned_lines`，**不按行号硬拼**——
/// 两份歌词的分行常常不一样，按行号拼会让每一句都错位。
pub fn align_translation(timeline: &mut LyricTimeline, translation: &[RawLine], language: &str) {
    let lines: Vec<&RawLine> = translation.iter().filter(|l| !l.text.trim().is_empty()).collect();
    let mut status = TranslationStatus { language: language.to_string(), total: lines.len(), ..Default::default() };
    if lines.is_empty() {
        timeline.translation = status;
        return;
    }
    let mut taken = vec![false; timeline.lines.len()];
    let mut pairs: Vec<(usize, String)> = Vec::new();
    for line in &lines {
        let Some(t) = line.start_sec.and_then(to_ms) else { continue };
        let best = timeline
            .lines
            .iter()
            .enumerate()
            .filter(|(i, l)| !taken[*i] && l.start_ms.is_some_and(|s| (s - t).abs() <= ALIGN_TOLERANCE_MS))
            .min_by_key(|(_, l)| (l.start_ms.unwrap_or(0) - t).abs())
            .map(|(i, _)| i);
        if let Some(i) = best {
            taken[i] = true;
            pairs.push((i, line.text.trim().to_string()));
        }
    }
    let ratio = pairs.len() as f64 / lines.len() as f64;
    if ratio >= ALIGN_MIN_RATIO {
        status.state = TranslationState::Aligned;
        status.aligned = pairs.len();
        for (i, text) in pairs {
            timeline.lines[i].translation = Some(text);
        }
        if status.aligned < status.total {
            timeline.warnings.push(format!("译文有 {} 行对不上原文的时间，没有显示", status.total - status.aligned));
        }
    } else {
        status.state = TranslationState::Unaligned;
        status.unaligned_lines = lines.iter().map(|l| l.text.trim().to_string()).collect();
        timeline.warnings.push("译文和原文的时间对不上，两份分开保留".into());
    }
    timeline.translation = status;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(t: f64, text: &str) -> RawLine {
        RawLine::new(Some(t), text)
    }

    #[test]
    fn a_line_ends_where_the_next_valid_line_starts_and_the_last_at_the_duration() {
        let tl = build("s", vec![timed(1.0, "一"), timed(3.5, "二"), timed(7.0, "三")], Some(10.0), 0);
        let ends: Vec<_> = tl.lines.iter().map(|l| l.end_ms).collect();
        assert_eq!(ends, vec![Some(3500), Some(7000), Some(10_000)]);
        assert_eq!(tl.precision, Precision::Line);
    }

    #[test]
    fn without_a_duration_the_last_line_has_no_end() {
        let tl = build("s", vec![timed(1.0, "一"), timed(2.0, "二")], None, 0);
        assert_eq!(tl.lines[1].end_ms, None);
        assert_eq!(tl.duration_ms, None);
    }

    #[test]
    fn duplicate_timestamps_share_the_next_later_start() {
        let tl = build("s", vec![timed(1.0, "一"), timed(1.0, "いち"), timed(4.0, "二")], Some(9.0), 0);
        assert_eq!(tl.lines[0].end_ms, Some(4000));
        assert_eq!(tl.lines[1].end_ms, Some(4000));
        assert_ne!(tl.lines[0].id, tl.lines[1].id);
    }

    #[test]
    fn empty_lines_are_dropped_and_unsorted_input_is_sorted() {
        let tl = build("s", vec![timed(5.0, "後"), timed(2.0, "  "), timed(1.0, "前")], None, 0);
        let texts: Vec<_> = tl.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["前", "後"]);
    }

    #[test]
    fn invalid_and_out_of_range_times_become_untimed_with_a_warning() {
        let tl = build(
            "s",
            vec![timed(f64::NAN, "壊れ"), timed(-1.0, "負"), timed(1.0, "正常"), timed(500.0, "遠すぎ")],
            Some(100.0),
            0,
        );
        let timed_texts: Vec<_> = tl.lines.iter().filter(|l| l.start_ms.is_some()).map(|l| l.text.as_str()).collect();
        assert_eq!(timed_texts, vec!["正常"]);
        assert_eq!(tl.lines.len(), 4, "无效时间的行留着，只是没有时间");
        assert_eq!(tl.warnings.len(), 2);
        // 正常那一行后面没有有效开始了，结束取曲长
        assert_eq!(tl.lines[0].end_ms, Some(100_000));
    }

    #[test]
    fn plain_text_gets_no_invented_times() {
        let tl = build("s", vec![RawLine::new(None, "一"), RawLine::new(None, "二")], Some(60.0), 0);
        assert_eq!(tl.precision, Precision::None);
        assert!(tl.lines.iter().all(|l| l.start_ms.is_none() && l.end_ms.is_none()));
    }

    #[test]
    fn an_explicit_end_is_kept_unless_it_is_not_after_the_start() {
        let mut a = timed(1.0, "一");
        a.end_sec = Some(2.0);
        let mut b = timed(5.0, "二");
        b.end_sec = Some(4.0);
        let tl = build("s", vec![a, b, timed(8.0, "三")], None, 0);
        assert_eq!(tl.lines[0].end_ms, Some(2000));
        assert_eq!(tl.lines[1].end_ms, Some(8000));
        assert_eq!(tl.warnings.len(), 1);
    }

    #[test]
    fn source_word_times_are_kept_and_bad_ones_drop_the_whole_line_to_line_level() {
        let mut good = timed(1.0, "夜が");
        good.words = vec![(0, 1, 1.0, None), (1, 2, 1.4, None)];
        let mut bad = timed(3.0, "明ける");
        bad.words = vec![(0, 2, 3.0, None), (1, 3, 3.2, None)]; // 重叠
        let tl = build("s", vec![good, bad], Some(5.0), 0);
        assert_eq!(tl.precision, Precision::Word);
        assert_eq!(
            tl.lines[0].words,
            vec![
                TimelineWord { char_start: 0, char_end: 1, start_ms: 1000, end_ms: Some(1400) },
                TimelineWord { char_start: 1, char_end: 2, start_ms: 1400, end_ms: Some(3000) },
            ]
        );
        assert!(tl.lines[1].words.is_empty());
    }

    #[test]
    fn line_ids_are_stable_across_rebuilds() {
        let lines = || vec![timed(1.0, "同じ"), timed(9.0, "同じ")];
        let a = build("s", lines(), None, 0);
        let b = build("s", lines(), None, 300);
        assert_eq!(a.lines.iter().map(|l| &l.id).collect::<Vec<_>>(), b.lines.iter().map(|l| &l.id).collect::<Vec<_>>());
        assert_ne!(a.lines[0].id, a.lines[1].id);
    }

    #[test]
    fn offset_round_trips_between_lyric_and_track_time() {
        for offset in [-1500, 0, 250] {
            let lyric = 12_340;
            let track = lyric_to_track_ms(lyric, offset);
            assert_eq!(track_to_lyric_ms(track, offset), lyric);
        }
        assert_eq!(lyric_to_track_ms(1000, 250), 1250, "正偏移让歌词晚一点出现");
        assert_eq!(track_to_file_ms(1000, 60_000), 61_000);
        assert_eq!(file_to_track_ms(61_000, 60_000), 1000);
    }

    #[test]
    fn line_at_finds_the_last_line_started() {
        let starts = [1000, 3000, 3000, 8000];
        assert_eq!(line_at(&starts, 999), None);
        assert_eq!(line_at(&starts, 1000), Some(0));
        assert_eq!(line_at(&starts, 3500), Some(2));
        assert_eq!(line_at(&starts, 99_999), Some(3));
        assert_eq!(line_at(&[], 5), None);
    }

    #[test]
    fn a_translation_with_matching_timestamps_is_aligned() {
        let mut tl = build("s", vec![timed(1.0, "夜が明ける"), timed(4.0, "君を待つ")], None, 0);
        align_translation(&mut tl, &[timed(1.0, "天亮了"), timed(4.02, "等你")], "zh-Hans");
        assert_eq!(tl.translation.state, TranslationState::Aligned);
        assert_eq!(tl.lines[0].translation.as_deref(), Some("天亮了"));
        assert_eq!(tl.lines[1].translation.as_deref(), Some("等你"));
    }

    #[test]
    fn a_translation_that_does_not_line_up_is_kept_apart_not_merged_by_line_number() {
        let mut tl = build("s", vec![timed(1.0, "一"), timed(4.0, "二")], None, 0);
        align_translation(&mut tl, &[timed(2.0, "壹"), timed(6.0, "贰")], "zh-Hans");
        assert_eq!(tl.translation.state, TranslationState::Unaligned);
        assert!(tl.lines.iter().all(|l| l.translation.is_none()));
        assert_eq!(tl.translation.unaligned_lines, vec!["壹", "贰"]);

        let mut tl = build("s", vec![timed(1.0, "一"), timed(4.0, "二")], None, 0);
        align_translation(&mut tl, &[RawLine::new(None, "壹"), RawLine::new(None, "贰")], "zh-Hans");
        assert_eq!(tl.translation.state, TranslationState::Unaligned, "没有时间戳的译文不按行号拼");
    }
}
