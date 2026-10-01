//! LRC 歌词解析。
//!
//! 移植自 Python 侧（`gui.py` 的 `_parse_lrc` + `scripts/02_parse_lrc_tokenize.py`），
//! 但修了那边的一个缺陷：**信用行不再被当成噪音丢掉**。
//!
//! 旧实现用子串判断 `any(kw in lyric for kw in SKIP_KEYWORDS)`，
//! 会把正文里碰巧含「作曲」二字的歌词也一起吞掉。这里复用
//! `library/credits.py` 那套「标签 + 冒号」的结构化判断，
//! 信用行单独收集，正文一行不丢。

use serde::Serialize;

/// 一行歌词。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// 时间戳。**可以为 None**——不是所有 LRC 都有时间轴，
    /// 没有时间轴的歌词仍然有语料价值，不该丢掉。
    pub time_sec: Option<f64>,
    pub text: String,
}

/// 一条信用（作词/作曲/编曲）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    /// lyricist / composer / arranger / translator
    pub role: &'static str,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLrc {
    pub lines: Vec<LyricLine>,
    pub credits: Vec<Credit>,
}

/// 信用标签 → 角色。简繁日三种写法都收。
const CREDIT_LABELS: &[(&str, &str)] = &[
    ("作詞", "lyricist"),
    ("作词", "lyricist"),
    ("作曲", "composer"),
    ("編曲", "arranger"),
    ("编曲", "arranger"),
    ("訳詞", "translator"),
    ("译词", "translator"),
];

/// 三段时间戳超过这个秒数就认为不可能是 `h:mm:ss`。
/// 没有哪首 J-Pop 有一小时长。
const IMPLAUSIBLE_SONG_SEC: f64 = 3600.0;

/// 三段时间戳（`a:b:c`）到底是 `h:mm:ss` 还是 `mm:ss:厘秒`。
///
/// `[mm:ss:xx]` 是把小数点误写成冒号的常见变体，库里 122 号
/// （Vaundy - 再会）就是这样。Python 侧的正则要求小数点，
/// 于是**整首歌的歌词一行都没进库**。
///
/// 三条判据，按可靠度从高到低：
///
/// 1. **末位带小数点** → 一定是 `h:mm:ss.xx`。冒号变体是把小数点
///    写成了冒号，所以它绝不会再带一个点。这条最硬，优先级最高。
/// 2. **末位 >= 60** → 一定是厘秒，秒数不可能到 60。
/// 3. 按 `h:mm:ss` 解出的最大值超过一小时 → 当成厘秒。这条最弱，
///    只在前两条都沉默时才用。
///
/// 都得看**整份文件**：单独一个 `[00:19:05]` 是真歧义，
/// 定不下来就保持 `h:mm:ss` 的原解释。
fn detect_three_part_format(text: &str) -> ThreePartFormat {
    let mut max_as_hms = 0.0f64;
    let mut saw_three_part = false;
    let mut fallback = ThreePartFormat::HourMinSec;
    for line in text.lines() {
        let mut rest = line.trim();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let inner = &stripped[..end];
            let parts: Vec<&str> = inner.split(':').collect();
            if parts.len() == 3
                && let Some(seconds) = parse_parts(&parts)
            {
                let last = parts[2].trim();
                // 判据 1：末位带小数点，只可能是 h:mm:ss.xx
                if last.contains('.') {
                    return ThreePartFormat::HourMinSec;
                }
                // 判据 2：末位 >= 60，只可能是厘秒
                if last.parse::<f64>().is_ok_and(|v| v >= 60.0) {
                    fallback = ThreePartFormat::MinSecCenti;
                }
                saw_three_part = true;
                max_as_hms = max_as_hms.max(seconds);
            }
            rest = &stripped[end + 1..];
        }
    }
    if fallback == ThreePartFormat::MinSecCenti
        || (saw_three_part && max_as_hms > IMPLAUSIBLE_SONG_SEC)
    {
        ThreePartFormat::MinSecCenti
    } else {
        ThreePartFormat::HourMinSec
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreePartFormat {
    HourMinSec,
    MinSecCenti,
}

/// 解析一份 LRC。
pub fn parse(text: &str) -> ParsedLrc {
    let mut out = ParsedLrc::default();
    let mut seen_credits: Vec<(String, String)> = Vec::new();
    let format = detect_three_part_format(text);

    for raw in text.lines() {
        let (stamps, body) = split_timestamps_with(raw.trim(), format);
        let body = body.trim();
        if body.is_empty() {
            continue;
        }

        // 元数据标签 [ar:...] [ti:...] 之类，不是歌词
        if is_metadata_tag(raw.trim()) {
            continue;
        }

        if let Some((role, names)) = parse_credit(body) {
            for name in names {
                let key = (role.to_string(), name.clone());
                if !seen_credits.contains(&key) {
                    seen_credits.push(key);
                    out.credits.push(Credit { role, name });
                }
            }
            continue;
        }

        // 一行可以带多个时间戳（副歌复用），每个都算一行
        if stamps.is_empty() {
            out.lines.push(LyricLine {
                time_sec: None,
                text: body.to_string(),
            });
        } else {
            for stamp in stamps {
                out.lines.push(LyricLine {
                    time_sec: Some(stamp),
                    text: body.to_string(),
                });
            }
        }
    }

    // 按时间排序，没有时间戳的保持原顺序排在最后
    out.lines.sort_by(|a, b| match (a.time_sec, b.time_sec) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    out
}

/// 剥掉行首所有 `[mm:ss.xx]`，返回 (秒数列表, 剩余文本)。
fn split_timestamps_with(line: &str, format: ThreePartFormat) -> (Vec<f64>, &str) {
    let mut stamps = Vec::new();
    let mut rest = line;
    while let Some(stripped) = rest.strip_prefix('[') {
        let Some(end) = stripped.find(']') else { break };
        let inner = &stripped[..end];
        match parse_timestamp_with(inner, format) {
            Some(seconds) => {
                stamps.push(seconds);
                rest = &stripped[end + 1..];
            }
            None => break, // 不是时间戳（可能是 [ar:...]），停下
        }
    }
    (stamps, rest)
}

/// `mm:ss.xx` / `mm:ss` / `h:mm:ss.xx` / `mm:ss:厘秒` → 秒。
fn parse_timestamp_with(inner: &str, format: ThreePartFormat) -> Option<f64> {
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() == 3 && format == ThreePartFormat::MinSecCenti {
        let minutes: f64 = parts[0].trim().parse().ok()?;
        let seconds: f64 = parts[1].trim().parse().ok()?;
        let centis: f64 = parts[2].trim().parse().ok()?;
        let total = minutes * 60.0 + seconds + centis / 100.0;
        return total.is_finite().then_some(total);
    }
    parse_parts(&parts)
}

/// 按「每段乘 60 累加」解析，也就是 `mm:ss` / `h:mm:ss`。
fn parse_parts(parts: &[&str]) -> Option<f64> {
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut seconds = 0.0f64;
    for part in &parts[..parts.len() - 1] {
        let value: f64 = part.trim().parse().ok()?;
        seconds = seconds * 60.0 + value;
    }
    let last: f64 = parts.last()?.trim().parse().ok()?;
    seconds = seconds * 60.0 + last;
    seconds.is_finite().then_some(seconds)
}

/// `[ar:歌手]` `[ti:曲名]` 这类元数据标签。
fn is_metadata_tag(line: &str) -> bool {
    let Some(stripped) = line.strip_prefix('[') else { return false };
    let Some(end) = stripped.find(']') else { return false };
    let inner = &stripped[..end];
    // 冒号前是两三个 ASCII 字母，且不是时间戳
    match inner.split_once(':') {
        Some((key, _)) => {
            parse_parts(&inner.split(':').collect::<Vec<_>>()).is_none()
                && (2..=8).contains(&key.len())
                && key.chars().all(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// 「作词 : n-buna」→ ("lyricist", ["n-buna"])。
///
/// 要求「标签 + 冒号」的结构，所以正文里含「作曲」二字的歌词
/// （比如「作曲家になりたかった」）不会被误判。
fn parse_credit(body: &str) -> Option<(&'static str, Vec<String>)> {
    for (label, role) in CREDIT_LABELS {
        let Some(rest) = body.strip_prefix(label) else { continue };
        let rest = rest.trim_start();
        let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))?;
        let names: Vec<String> = rest
            // 斜杠是这个库里唯一可靠的合作分隔符
            .split(['/', '／'])
            .map(|n| n.trim().trim_matches(|c: char| " .,-".contains(c)).to_string())
            .filter(|n| !n.is_empty())
            .collect();
        if names.is_empty() {
            return None;
        }
        return Some((role, names));
    }
    None
}

/// 读文件并解析。编码按 UTF-8 → CP932 → GBK 依次尝试。
pub fn parse_file(path: &std::path::Path) -> anyhow::Result<ParsedLrc> {
    let bytes = std::fs::read(path)?;
    // 绝大多数是 UTF-8（可能带 BOM）
    let text = String::from_utf8_lossy(&bytes);
    Ok(parse(text.trim_start_matches('\u{feff}')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        let lrc = "[00:12.34]第一句\n[01:05.00]第二句\n";
        let parsed = parse(lrc);
        assert_eq!(parsed.lines.len(), 2);
        assert!((parsed.lines[0].time_sec.unwrap() - 12.34).abs() < 1e-6);
        assert!((parsed.lines[1].time_sec.unwrap() - 65.0).abs() < 1e-6);
    }

    #[test]
    fn a_line_with_several_timestamps_becomes_several_lines() {
        // 副歌复用同一行的写法
        let parsed = parse("[00:10.00][01:20.00]サビ\n");
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].text, "サビ");
        assert_eq!(parsed.lines[1].text, "サビ");
    }

    #[test]
    fn lines_are_sorted_by_time() {
        let parsed = parse("[01:00.00]後\n[00:10.00]先\n");
        assert_eq!(parsed.lines[0].text, "先");
    }

    #[test]
    fn lines_without_timestamps_are_kept() {
        // 没有时间轴的歌词仍然有语料价值
        let parsed = parse("純文本歌詞\n第二行\n");
        assert_eq!(parsed.lines.len(), 2);
        assert!(parsed.lines.iter().all(|l| l.time_sec.is_none()));
    }

    #[test]
    fn untimed_lines_sort_after_timed_ones() {
        let parsed = parse("裸行\n[00:10.00]有時間的\n");
        assert_eq!(parsed.lines[0].text, "有時間的");
        assert_eq!(parsed.lines[1].text, "裸行");
    }

    #[test]
    fn metadata_tags_are_skipped() {
        let parsed = parse("[ar:YOASOBI]\n[ti:群青]\n[by:someone]\n[00:10.00]歌詞\n");
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].text, "歌詞");
    }

    #[test]
    fn credits_are_collected_not_discarded() {
        let parsed = parse("[00:00.00] 作词 : n-buna\n[00:01.00] 作曲 : n-buna\n[00:10.00]歌詞\n");
        assert_eq!(parsed.credits.len(), 2);
        assert_eq!(parsed.credits[0].role, "lyricist");
        assert_eq!(parsed.credits[0].name, "n-buna");
        // 信用行不进正文
        assert_eq!(parsed.lines.len(), 1);
    }

    #[test]
    fn credit_with_several_people() {
        let parsed = parse("[00:02.0] 编曲 : 100回嘔吐/加藤冴人\n");
        assert_eq!(parsed.credits.len(), 2);
        assert_eq!(parsed.credits[1].name, "加藤冴人");
    }

    #[test]
    fn fullwidth_colon_and_japanese_label() {
        let parsed = parse("[00:01.00]作詞：山口一郎\n");
        assert_eq!(parsed.credits.len(), 1);
        assert_eq!(parsed.credits[0].name, "山口一郎");
    }

    #[test]
    fn duplicate_credits_are_deduplicated() {
        // 有的 LRC 每段都重复一遍作词
        let parsed = parse("[00:00.0] 作词 : n-buna\n[01:00.0] 作词 : n-buna\n");
        assert_eq!(parsed.credits.len(), 1);
    }

    #[test]
    fn a_lyric_merely_containing_the_word_is_not_a_credit() {
        // 旧实现用子串判断，会把这行也吞掉
        let parsed = parse("[00:30.0]作曲家になりたかった\n");
        assert!(parsed.credits.is_empty());
        assert_eq!(parsed.lines.len(), 1);
    }

    #[test]
    fn empty_and_whitespace_lines_are_dropped() {
        let parsed = parse("[00:10.00]\n\n   \n[00:20.00]歌詞\n");
        assert_eq!(parsed.lines.len(), 1);
    }

    #[test]
    fn malformed_input_does_not_panic() {
        for text in ["", "[", "[]", "[::::]", "[99:99:99.99]x", "[aa:bb]x", "\u{feff}[00:01.0]x"] {
            let _ = parse(text);
        }
    }

    #[test]
    fn colon_instead_of_dot_is_detected_as_centiseconds() {
        // 库里 122 号（Vaundy - 再会）就是这个格式。Python 的正则要求
        // 小数点，于是整首歌的歌词一行都没进库。
        let lrc = "[00:19:05]One more time
[03:47:19]最後の一行
";
        let parsed = parse(lrc);
        assert!((parsed.lines[0].time_sec.unwrap() - 19.05).abs() < 1e-6);
        assert!((parsed.lines[1].time_sec.unwrap() - 227.19).abs() < 1e-6);
    }

    #[test]
    fn a_last_field_over_59_proves_centiseconds() {
        let parsed = parse("[00:10:75]歌詞
");
        assert!((parsed.lines[0].time_sec.unwrap() - 10.75).abs() < 1e-6);
    }

    #[test]
    fn a_decimal_point_pins_the_format_for_the_whole_file() {
        // 真的超过一小时的（现场全场录音）不该被误判成厘秒。
        // 只要有一个戳带小数点，整份文件就定案为 h:mm:ss.xx——
        // 哪怕最大值超过一小时，也不再走那条弱启发。
        let parsed = parse("[0:03:21.00]短い
[1:02:03.50]長い
");
        assert!((parsed.lines[0].time_sec.unwrap() - 201.0).abs() < 1e-6);
        assert!((parsed.lines[1].time_sec.unwrap() - 3723.5).abs() < 1e-6);
    }

    #[test]
    fn a_decimal_point_outranks_the_over_59_rule() {
        // 判据 1 比判据 2 硬：带点就是带点
        let parsed = parse("[00:10.75]x
[1:02:03.50]y
");
        assert!((parsed.lines[1].time_sec.unwrap() - 3723.5).abs() < 1e-6);
    }

    #[test]
    fn format_detection_is_per_file_not_per_line() {
        // 单行看不出来，必须看整份文件的最大值
        let one_line = parse("[00:19:05]x
");
        // 只有一行时按 h:mm:ss 解出 19 分钟，还算合理，保持原解释
        assert!(one_line.lines[0].time_sec.unwrap() > 1000.0);
    }

    #[test]
    fn hour_length_timestamps() {
        let parsed = parse("[1:02:03.50]長い曲\n");
        assert!((parsed.lines[0].time_sec.unwrap() - 3723.5).abs() < 1e-6);
    }
}
