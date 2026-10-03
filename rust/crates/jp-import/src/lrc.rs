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

/// 其余的署名标签：认出来就整行丢掉，不当歌词、也不进 `credits`。
///
/// 出处：网易云、QQ 音乐歌词开头那一串「标签 : 人名」署名行。存量的 415 份 .lrc 里
/// 实测出现过的只有作词 / 作曲 / 编曲 / 制作人（「制作人」13 行，以前被当成歌词入库）；
/// 其余按两家常见的署名补齐，简体、繁体都收。英文标签比较时不分大小写。
///
/// 「词」「曲」「Lyrics」「Music」「Arrangement」虽然说的也是作词作曲编曲，
/// 这里**只丢不收**：`credits` 的来源只认上面那张表的写法，不扩大。
const OTHER_CREDIT_LABELS: &[&str] = &[
    "词", "詞", "曲",
    "制作人", "製作人", "制作", "製作", "监制", "監製", "出品", "出品人", "发行", "發行",
    "统筹", "統籌", "企划", "企劃", "配唱制作人", "人声编辑", "人聲編輯",
    "混音", "混音师", "混音師", "母带", "母帶", "母带工程师", "母帶工程師",
    "录音", "錄音", "录音师", "錄音師", "录音室", "錄音室",
    "和声", "和聲", "和声编写", "和聲編寫",
    "吉他", "贝斯", "貝斯", "鼓", "键盘", "鍵盤", "钢琴", "鋼琴", "弦乐", "弦樂",
    "OP", "SP", "ISRC",
    "Lyrics", "Music", "Arrangement", "Arranger", "Composer", "Lyricist",
    "Producer", "Produced by", "Mix", "Mixing", "Mixed by", "Mastering", "Mastered by",
    "Recording", "Recorded by",
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
    let offset_sec = find_offset_sec(text);

    for raw in text.lines() {
        let (stamps, body) = split_timestamps_with(raw.trim(), format);
        let body = strip_word_timestamps(body);
        let body = body.trim();
        if body.is_empty() {
            continue;
        }

        // 元数据标签 [ar:...] [ti:...] 之类，不是歌词
        if is_metadata_tag(raw.trim()) {
            continue;
        }

        match parse_credit(body) {
            Some(CreditLine::Role(role, names)) => {
                for name in names {
                    let key = (role.to_string(), name.clone());
                    if !seen_credits.contains(&key) {
                        seen_credits.push(key);
                        out.credits.push(Credit { role, name });
                    }
                }
                continue;
            }
            Some(CreditLine::Other) => continue,
            None => {}
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
                    time_sec: Some((stamp - offset_sec).max(0.0)),
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

/// `[offset:N]` 标签给出的整体偏移，换算成秒。没有、或者写得认不出来就是 0。
///
/// 按 LRC 惯例 N 是毫秒，**正值让歌词提前**：显示时刻 = 时间戳 − N/1000，不小于 0。
///
/// **这是一次有意的行为变化**：0.1.x 的 `gui.py::_parse_lrc` 和
/// `scripts/02_parse_lrc_tokenize.py` 都没处理过 offset，标签被当成元数据整行丢掉，
/// 时间轴于是整体偏一截。改之前查过：存量的 415 份 .lrc 里一份带 offset 的都没有，
/// 所以这条只影响以后导入的歌词。
fn find_offset_sec(text: &str) -> f64 {
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let Some((inner, _)) = line.strip_prefix('[').and_then(|r| r.split_once(']')) else {
            continue;
        };
        let Some((key, value)) = inner.split_once(':') else { continue };
        if key.trim().eq_ignore_ascii_case("offset") {
            // 只认第一个：同一份文件里写两个 offset 的，后一个多半是编辑残留
            return value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|ms| ms.is_finite())
                .map_or(0.0, |ms| ms / 1000.0);
        }
    }
    0.0
}

/// 剥掉增强型 LRC 的逐字时间戳 `<mm:ss>` / `<mm:ss.xx>` / `<mm:ss.xxx>`。
///
/// 这个库只要行级时间轴，逐字的那一层不用；留在正文里的话，分词会把
/// `<00:12.34>` 切成一串符号 token，检索和制卡里都是噪音。
/// 尖括号里不是时间的（`<愛>`）原样保留——那是正文。
fn strip_word_timestamps(body: &str) -> std::borrow::Cow<'_, str> {
    if !body.contains('<') {
        return std::borrow::Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('>') {
            Some(close) if is_word_timestamp(&after[..close]) => rest = &after[close + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// `mm:ss`，可选 `.` 加一到三位小数。分钟允许一到三位。
fn is_word_timestamp(inner: &str) -> bool {
    let Some((minutes, seconds)) = inner.split_once(':') else { return false };
    let (whole, fraction) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (seconds, None),
    };
    let digits = |s: &str, range: std::ops::RangeInclusive<usize>| {
        range.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    digits(minutes, 1..=3) && digits(whole, 2..=2) && fraction.is_none_or(|f| digits(f, 1..=3))
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

/// 一行署名的两种去向。
enum CreditLine {
    /// 作词 / 作曲 / 编曲 / 译词：进 `credits`
    Role(&'static str, Vec<String>),
    /// 其余署名（制作人、混音、OP……）：不是歌词，也不进 `credits`，丢掉
    Other,
}

/// 「作词 : n-buna」→ `Role("lyricist", ["n-buna"])`，「混音：某人」→ `Other`，
/// 不是署名行 → None。
///
/// 要求「**标签表里的词** + 冒号 + 内容」的结构：冒号前的整段必须正好是表里的一个标签。
/// 所以正文里含「作曲」二字的歌词（「作曲家になりたかった」）、碰巧带冒号的歌词
/// （181 号存量歌词里的「目が開いてく4:30 A.M.」）都不会被误判。
fn parse_credit(body: &str) -> Option<CreditLine> {
    let colon = body.find([':', '：'])?;
    let label = body[..colon].trim();
    let rest = body[colon..].trim_start_matches([':', '：']).trim();
    if label.is_empty() || rest.is_empty() {
        return None;
    }
    if let Some((_, role)) = CREDIT_LABELS.iter().find(|(l, _)| *l == label) {
        let names: Vec<String> = rest
            // 斜杠是这个库里唯一可靠的合作分隔符
            .split(['/', '／'])
            .map(|n| n.trim().trim_matches(|c: char| " .,-".contains(c)).to_string())
            .filter(|n| !n.is_empty())
            .collect();
        return (!names.is_empty()).then_some(CreditLine::Role(role, names));
    }
    OTHER_CREDIT_LABELS
        .iter()
        .any(|l| l.eq_ignore_ascii_case(label))
        .then_some(CreditLine::Other)
}

/// 读文件并解析。编码按 UTF-8 → CP932 → GBK 依次尝试。
pub fn parse_file(path: &std::path::Path) -> anyhow::Result<ParsedLrc> {
    let bytes = std::fs::read(path)?;
    Ok(parse(decode(&bytes).trim_start_matches('\u{feff}')))
}

/// 字节 → 文本。UTF-8 → CP932 → GBK，第一个**不出错**的解释胜出。
///
/// 不能只用 `from_utf8_lossy`：日文站点下回来的 .lrc 有不少是 Shift-JIS，
/// 整份歌词会变成一串 U+FFFD。那比没有歌词更糟——库里记着「这首有歌词」，
/// 「补齐缺失歌词」再也不会看它一眼，而检索和制卡拿到的全是问号。
///
/// CP932 排在 GBK 前面：两者的双字节区大面积重叠，同一串字节往往两边都能
/// 解出「字」来，只有一边是对的。这是个日文语料库，所以先假定日文编码；
/// 真有 GBK 的中文歌词时，它多半含 CP932 解不出的字节（实测
/// 「风的记忆」的 GBK 编码在 CP932 下第 17 字节就非法），于是会落到 GBK。
fn decode(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return std::borrow::Cow::Borrowed(text);
    }
    for encoding in [encoding_rs::SHIFT_JIS, encoding_rs::GBK] {
        let (text, _, had_errors) = encoding.decode(bytes);
        if !had_errors {
            return text;
        }
    }
    // 三种都不干净：按 UTF-8 尽力而为，坏字节变 U+FFFD。
    // 走到这里的文件本身就是坏的，报错反而会让一首歌整个导不进来。
    String::from_utf8_lossy(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日文站点下回来的 .lrc 常是 Shift-JIS。整份当 UTF-8 读会变成一串 U+FFFD，
    /// 而且因为「有歌词」了，补齐流程再也不会来看它。
    #[test]
    fn a_shift_jis_file_is_not_read_as_mojibake() {
        let dir = std::env::temp_dir().join(format!("jp-lrc-enc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let sjis = dir.join("sjis.lrc");
        std::fs::write(
            &sjis,
            b"\x5b\x30\x30\x3a\x31\x35\x2e\x30\x30\x5d\x8c\x4e\x82\xf0\x91\xd2\x82\xc1\x82\xc4\x82\xa2\x82\xe9\x0a",
        )
        .unwrap();
        let parsed = parse_file(&sjis).unwrap();
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].text, "君を待っている");

        // GBK 的中文歌词：CP932 解到一半就非法，于是落到 GBK
        let gbk = dir.join("gbk.lrc");
        std::fs::write(
            &gbk,
            b"\x5b\x30\x30\x3a\x32\x30\x2e\x30\x30\x5d\xb7\xe7\xb5\xc4\xbc\xc7\xd2\xe4\x0a",
        )
        .unwrap();
        let parsed = parse_file(&gbk).unwrap();
        assert_eq!(parsed.lines[0].text, "风的记忆");
    }

    /// 存量的 207 份 .lrc 都是 UTF-8，这条路不能因为加了回退而变样
    #[test]
    fn utf8_is_still_read_as_utf8_bom_and_all() {
        let dir = std::env::temp_dir().join(format!("jp-lrc-enc8-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("utf8.lrc");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("[00:10.00]夜が明けるまで\n".as_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let parsed = parse_file(&path).unwrap();
        assert_eq!(parsed.lines[0].text, "夜が明けるまで");
    }

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

    /// `[offset:N]` 是毫秒，正值让歌词提前。以前被当成元数据整行吞掉，时间轴整体偏一截
    #[test]
    fn the_offset_tag_shifts_every_timestamp() {
        let parsed = parse("[offset:500]\n[00:10.00]一\n[00:20.00]二\n");
        assert_eq!(parsed.lines.len(), 2);
        assert!((parsed.lines[0].time_sec.unwrap() - 9.5).abs() < 1e-6);
        assert!((parsed.lines[1].time_sec.unwrap() - 19.5).abs() < 1e-6);

        let later = parse("[offset:-1500]\n[00:10.00]一\n");
        assert!((later.lines[0].time_sec.unwrap() - 11.5).abs() < 1e-6);

        let signed = parse("[offset:+250]\n[00:10.00]一\n");
        assert!((signed.lines[0].time_sec.unwrap() - 9.75).abs() < 1e-6);
    }

    #[test]
    fn an_offset_never_pushes_a_line_below_zero() {
        let parsed = parse("[offset:3000]\n[00:01.00]一\n");
        assert_eq!(parsed.lines[0].time_sec, Some(0.0));
    }

    #[test]
    fn a_garbled_offset_is_ignored_and_untimed_lines_stay_untimed() {
        let parsed = parse("[offset:abc]\n[00:10.00]一\n裸行\n");
        assert!((parsed.lines[0].time_sec.unwrap() - 10.0).abs() < 1e-6);
        let parsed = parse("[offset:500]\n裸行\n");
        assert_eq!(parsed.lines[0].time_sec, None);
    }

    /// 增强型 LRC 的逐字时间戳不是歌词正文：留着的话分词会把 `<00:12.34>` 切成一串符号
    #[test]
    fn word_level_timestamps_are_stripped_from_the_text() {
        let parsed = parse("[00:12.00]<00:12.00>夜が<00:12.50>明ける<00:13.10>\n");
        assert_eq!(parsed.lines[0].text, "夜が明ける");
        assert!((parsed.lines[0].time_sec.unwrap() - 12.0).abs() < 1e-6);

        // 三种精度都认；剥完只剩空白的整行不要
        let parsed = parse("[00:01.00]<00:01>あ<00:02.5>い<00:03.125> う \n[00:05.00]<00:05.00>\n");
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].text, "あい う");
    }

    /// 作词作曲编曲译词以外的署名行（网易云 / QQ 音乐歌词头部那一串）不是歌词：丢掉，也不进 credits
    #[test]
    fn other_credit_lines_are_dropped_not_kept_as_lyrics() {
        let lrc = "[00:00.00] 作词 : n-buna\n\
                   [00:00.50] 制作人 : n-buna\n\
                   [00:00.60] 词：某人\n\
                   [00:00.70] 曲 : 某人\n\
                   [00:01.00]混音：某人\n\
                   [00:01.10]母帶 : 某人\n\
                   [00:01.20]吉他：A/B\n\
                   [00:01.30]鼓 : C\n\
                   [00:01.40]OP : Some Publishing\n\
                   [00:01.50]SP：Sub Publishing\n\
                   [00:01.60]Lyrics: Someone\n\
                   [00:01.70]Music : Someone\n\
                   [00:01.80]Arrangement：Someone\n\
                   [00:01.90]Producer: Someone\n\
                   [00:02.00]Mix: Someone\n\
                   [00:02.10]mastering : Someone\n\
                   [00:10.00]歌詞\n";
        let parsed = parse(lrc);
        let texts: Vec<&str> = parsed.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["歌詞"]);
        // 只有作词进了 credits，其余丢弃
        assert_eq!(parsed.credits.len(), 1);
        assert_eq!(parsed.credits[0].role, "lyricist");
    }

    /// 结构判断仍然是「标签表里的词 + 冒号」：正文里碰巧有冒号、或以标签字开头的都不动
    #[test]
    fn lyrics_with_a_colon_or_a_label_like_start_are_not_credits() {
        // 181 号存量歌词里真有这一行
        let lrc = "[00:01.00]目が開いてく4:30 A.M.\n\
                   [00:02.00]作曲家になりたかった\n\
                   [00:03.00]曲がり角で\n\
                   [00:04.00]Love: is all you need\n\
                   [00:05.00]鼓動が鳴る\n\
                   [00:06.00]制作人 :\n";
        let parsed = parse(lrc);
        assert!(parsed.credits.is_empty());
        assert_eq!(parsed.lines.len(), 6, "{:?}", parsed.lines);
    }

    /// 尖括号里不是时间的，是正文
    #[test]
    fn angle_brackets_that_are_not_timestamps_are_kept() {
        let parsed = parse("[00:01.00]<愛>って何\n[00:02.00]a <b> c\n");
        assert_eq!(parsed.lines[0].text, "<愛>って何");
        assert_eq!(parsed.lines[1].text, "a <b> c");
    }
}
