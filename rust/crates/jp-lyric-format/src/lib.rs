//! 所有歌词格式的唯一解析入口（[#61](https://github.com/Yisoragoto/jpop-corpus-tool/issues/61)）。
//!
//! 导入、内嵌、外挂、手选文件、在线下回来的歌词都走 [`decode`]：给了扩展名或来源格式就按它解析，
//! 没给（内嵌歌词、`.txt`）就看内容判断，顺序是
//! TTML → QRC 的 XML 外壳 → 逐字明文（LYS / QRC / YRC / KRC，看行首的样子区分）→ LRC → SRT → 纯文本。
//!
//! 每种格式最后都落成同一个结果 [`Decoded`]：原文（存进 `source_raw`）、
//! 入库用的 LRC 文本、解析好的行（带逐字时间）。逐字格式还带着毫秒精度的 [`TimedLyrics`]。
//!
//! 各个解析器本身在子模块里：[`lrc`]（含 Lyrics A2 和 ESLyric 两种逐字写法）、[`srt`]、
//! [`timed`]（TTML / QRC / YRC / KRC / LYS）、[`enhanced`]（只认真逐字的严格版 LRC）、
//! [`crypt`]（QRC / KRC 加密外壳）、[`text`]（字节 → 文本的编码回退）、
//! [`external`]（音频旁边同名文件的挑选）。

pub mod crypt;
pub mod enhanced;
pub mod external;
pub mod lrc;
pub mod srt;
pub mod text;
pub mod timed;

use anyhow::{Result, ensure};

pub use external::{ExternalFile, ExternalLyrics, find_external};
pub use lrc::{Credit, LyricLine, ParsedLrc, WordTime};
pub use timed::{TimedLine, TimedLyrics, TimedWord};

/// 一份歌词的格式。[`Format::name`] 就是库里 `raw_format` 记的那个词。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Ttml,
    Qrc,
    Yrc,
    Krc,
    Lys,
    /// 带逐字时间的 LRC：Lyrics A2 的 `<mm:ss.xx>`，或 ESLyric 的行内 `[mm:ss.xx]`
    EnhancedLrc,
    Lrc,
    Srt,
    Text,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Ttml => "ttml",
            Format::Qrc => "qrc",
            Format::Yrc => "yrc",
            Format::Krc => "krc",
            Format::Lys => "lys",
            Format::EnhancedLrc => "enhanced_lrc",
            Format::Lrc => "lrc",
            Format::Srt => "srt",
            Format::Text => "text",
        }
    }

    /// 逐字格式（自带字级时间，入库前要转成 LRC 文本）
    pub fn is_word_timed(self) -> bool {
        matches!(self, Format::Ttml | Format::Qrc | Format::Yrc | Format::Krc | Format::Lys)
    }

    /// LRC 一家（LRC、带逐字的 LRC、纯文本）：原文本身就是入库用的 LRC 文本
    pub fn is_lrc_family(self) -> bool {
        matches!(self, Format::EnhancedLrc | Format::Lrc | Format::Text)
    }
}

/// 解析提示：扩展名（`qrc`、`.lrc`）或库里记的格式名（`enhanced_lrc`）。不分大小写。
///
/// - 逐字格式、SRT：照提示解析。
/// - `lrc` / `enhanced_lrc`：照 LRC 解析，只按内容细分带不带逐字、有没有时间轴。
/// - `txt` / `text`、空串、认不得的：当没给，看内容判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hint {
    Exact(Format),
    LrcFamily,
    Detect,
}

fn hint(value: Option<&str>) -> Hint {
    let Some(value) = value else { return Hint::Detect };
    match value.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
        "ttml" => Hint::Exact(Format::Ttml),
        "qrc" => Hint::Exact(Format::Qrc),
        "yrc" => Hint::Exact(Format::Yrc),
        "krc" => Hint::Exact(Format::Krc),
        "lys" => Hint::Exact(Format::Lys),
        "srt" => Hint::Exact(Format::Srt),
        "lrc" | "enhanced_lrc" | "elrc" | "eslrc" => Hint::LrcFamily,
        _ => Hint::Detect,
    }
}

/// 解析结果。
#[derive(Debug, Clone)]
pub struct Decoded {
    pub format: Format,
    /// 解码后的原文（加密的 QRC / KRC 是解出来的明文，去掉了 BOM）。存进 `source_raw` 的就是它
    pub original: String,
    /// 入库用的 LRC 文本：LRC 一家就是原文，其余格式转换得到
    pub lrc: String,
    /// 解析好的行、署名、夹带的译文
    pub parsed: ParsedLrc,
    /// 和 `parsed.lines` 一一对应：来源明确给了这一行的结束时间才有（目前只有 SRT）
    pub ends: Vec<Option<f64>>,
    /// 逐字格式的原始时间轴（毫秒），其余格式是 None
    pub timed: Option<TimedLyrics>,
}

/// 字节 → 解析结果。`hint` 是扩展名或库里记的格式名，没有就看内容判断。
///
/// 加密的 QRC（QQ 音乐本地缓存的文件头，或者不是文本的 `.qrc`）和 KRC（`krc1` 文件头）先解密；
/// 其余按 UTF-8 → CP932 → GBK 解成文本（带 BOM 的 UTF-16 也认，见 [`text::decode_bytes`]）。
pub fn decode(bytes: &[u8], hint_value: Option<&str>) -> Result<Decoded> {
    let wanted = hint(hint_value);
    let binary_krc = bytes.starts_with(b"krc1");
    let binary_qrc = bytes.starts_with(crypt::LOCAL_QRC_HEADER);
    match wanted {
        Hint::Exact(Format::Krc) | Hint::Detect if binary_krc => {
            return decode_str(&crypt::decode_krc(bytes)?, Some("krc"));
        }
        Hint::Exact(Format::Qrc) | Hint::Detect if binary_qrc => {
            return decode_str(&crypt::decode_qrc(bytes)?, Some("qrc"));
        }
        Hint::Exact(Format::Qrc) if text::decode_unicode(bytes).is_none() => {
            // 不是 Unicode 文本、又没有本地文件头的 .qrc：只可能是网络下回来的密文
            return decode_str(&crypt::decode_qrc(bytes)?, Some("qrc"));
        }
        _ => {}
    }
    decode_str(&text::decode_bytes(bytes), hint_value)
}

/// 已经是文本的歌词 → 解析结果。规则同 [`decode`]。
pub fn decode_str(text: &str, hint_value: Option<&str>) -> Result<Decoded> {
    let original = text.trim_start_matches('\u{feff}');
    decode_as(original, resolve_format(original, hint_value))
}

/// [`decode_str`] 会照哪种格式解析：提示说了算，`lrc` 只按内容细分，没有提示就看内容。不解析。
pub fn resolve_format(text: &str, hint_value: Option<&str>) -> Format {
    let text = text.trim_start_matches('\u{feff}');
    match hint(hint_value) {
        Hint::Exact(format) => format,
        Hint::LrcFamily => lrc_flavor(text),
        Hint::Detect => detect(text),
    }
}

/// 照指定格式解析，不再判断。
pub fn decode_as(text: &str, format: Format) -> Result<Decoded> {
    let original = text.trim_start_matches('\u{feff}').to_string();
    let timed = match format {
        Format::Ttml => Some(timed::parse_ttml(&original)?),
        Format::Qrc => Some(timed::parse_qrc(&original)?),
        Format::Yrc => Some(timed::parse_yrc(&original)?),
        Format::Krc => Some(timed::parse_krc(&original)?),
        Format::Lys => Some(timed::parse_lys(&original)?),
        _ => None,
    };
    if format == Format::Srt {
        let cues = srt::parse_cues(&original);
        ensure!(!cues.is_empty(), "这份 SRT 里没有认得出的字幕");
        let parsed = srt::parse(&original);
        let lrc = srt::to_lrc(&cues);
        let ends = parsed.ends.iter().copied().map(Some).collect();
        return Ok(Decoded {
            format,
            original,
            lrc,
            parsed: ParsedLrc { lines: parsed.lines, credits: Vec::new(), translations: parsed.translations },
            ends,
            timed: None,
        });
    }
    let lrc = match &timed {
        Some(timed) => timed.to_enhanced_lrc(),
        None => original.clone(),
    };
    let parsed = lrc::parse(&lrc);
    let ends = vec![None; parsed.lines.len()];
    Ok(Decoded { format, original, lrc, parsed, ends, timed })
}

/// 看内容判断格式。顺序：TTML → QRC 的 XML 外壳 → 逐字明文（LYS / QRC / YRC / KRC）→ LRC → SRT → 纯文本。
pub fn detect(text: &str) -> Format {
    let text = text.trim_start_matches('\u{feff}');
    let head = text.trim_start();
    if head.starts_with('<') {
        if xml_root(head) == Some("tt") {
            return Format::Ttml;
        }
        if head.contains("LyricContent") {
            return Format::Qrc;
        }
    }
    if let Some(format) = word_timed_plain(text) {
        return format;
    }
    match lrc_flavor(text) {
        Format::Text if is_srt(text) => Format::Srt,
        other => other,
    }
}

/// XML 根元素的本地名（跳过声明、注释、DOCTYPE）。只看开头，不解析整份。
fn xml_root(text: &str) -> Option<&str> {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            rest = &after[after.find("?>")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else if let Some(after) = rest.strip_prefix("<!") {
            rest = &after[after.find('>')? + 1..];
        } else {
            let name = rest.strip_prefix('<')?;
            let end = name.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(name.len());
            let name = &name[..end];
            return Some(name.rsplit(':').next().unwrap_or(name));
        }
    }
}

/// 逐字明文的四种写法，看第一行正文的行首区分：
///
/// | 格式 | 行首 |
/// |---|---|
/// | YRC | `[行起,时长](字起,时长,0)字` |
/// | KRC | `[行起,时长]<字偏移,时长,0>字` |
/// | QRC | `[行起,时长]字(字起,时长)` |
/// | LYS | `[属性]字(字起,时长)`，属性是一位数字 |
///
/// `[ti:…]` 这类标签行、网易云的 JSON 署名行跳过，往下找第一行正文。
fn word_timed_plain(text: &str) -> Option<Format> {
    for line in text.lines().take(400) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('{') || is_tag_line(line) {
            continue;
        }
        let rest = line.strip_prefix('[')?;
        let (head, body) = rest.split_once(']')?;
        if let Some((start, duration)) = head.split_once(',') {
            if !digits(start) || !digits(duration) {
                return None;
            }
            let body = body.trim_start();
            if tuple_at(body, '(', ')', 3) {
                return Some(Format::Yrc);
            }
            if tuple_at(body, '<', '>', 3) {
                return Some(Format::Krc);
            }
            return has_tuple(body, '(', ')', 2).then_some(Format::Qrc);
        }
        if head.len() == 1 && digits(head) && has_tuple(body, '(', ')', 2) {
            return Some(Format::Lys);
        }
        return None;
    }
    None
}

/// `[ar:歌手]`、`[kana:…]`、`[offset:0]` 这类整行标签：冒号前是字母
fn is_tag_line(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|r| r.split_once(':'))
        .is_some_and(|(key, _)| !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphabetic()))
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `text` 开头是 `open 数字,数字… close`，正好 `fields` 段
fn tuple_at(text: &str, open: char, close: char, fields: usize) -> bool {
    text.strip_prefix(open).and_then(|r| r.split_once(close)).is_some_and(|(inner, _)| {
        let parts: Vec<&str> = inner.split(',').collect();
        parts.len() == fields && parts.iter().all(|p| digits(p.trim()))
    })
}

/// `text` 里某处有 `(数字,数字)`
fn has_tuple(text: &str, open: char, close: char, fields: usize) -> bool {
    text.match_indices(open).any(|(at, _)| tuple_at(&text[at..], open, close, fields))
}

/// LRC 一家里细分：带逐字时间的 LRC、普通 LRC、纯文本。
///
/// 看前 400 行里有没有 `[数字:` 开头的行；这种行里有 `<数字` 的（Lyrics A2），
/// 或者字后面又跟着 `[mm:ss]` 的（ESLyric），算带逐字。
pub fn lrc_flavor(text: &str) -> Format {
    let mut lrc = false;
    for line in text.lines().take(400) {
        let line = line.trim().trim_start_matches('\u{feff}');
        let Some(rest) = line.strip_prefix('[') else { continue };
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || !rest[digits..].starts_with(':') {
            continue;
        }
        lrc = true;
        if line.contains('<') && line.contains('>') && line.contains(':') {
            let inner = line.split('<').nth(1).unwrap_or("");
            if inner.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Format::EnhancedLrc;
            }
        }
        if lrc::has_inline_square_marks(line) {
            return Format::EnhancedLrc;
        }
    }
    if lrc { Format::Lrc } else { Format::Text }
}

/// SRT：有一行以数字开头、含 `-->`
fn is_srt(text: &str) -> bool {
    text.lines()
        .take(400)
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .any(|l| l.contains("-->") && l.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_are_case_insensitive_and_unknown_hints_fall_back_to_detection() {
        assert_eq!(decode_str("[00:01.00]夜", Some("LRC")).unwrap().format, Format::Lrc);
        assert_eq!(decode_str("[00:01.00]夜", Some(".lrc")).unwrap().format, Format::Lrc);
        assert_eq!(decode_str("[00:01.00]<00:01.00>夜", Some("lrc")).unwrap().format, Format::EnhancedLrc);
        assert_eq!(decode_str("[1000,500](1000,500,0)夜", Some("txt")).unwrap().format, Format::Yrc);
        assert_eq!(decode_str("夜が明ける", None).unwrap().format, Format::Text);
    }

    #[test]
    fn detection_order_follows_the_issue() {
        assert_eq!(detect("<?xml version=\"1.0\"?>\n<tt xmlns=\"http://www.w3.org/ns/ttml\"><body/></tt>"), Format::Ttml);
        assert_eq!(detect("<QrcInfos><Lyric_1 LyricContent=\"[0,1]a(0,1)\"/></QrcInfos>"), Format::Qrc);
        assert_eq!(detect("[ti:x]\n[kana:1か(1,2)]\n[0,500]夜(0,500)"), Format::Qrc);
        assert_eq!(detect("[ti:x]\n[0,500](0,500,0)夜"), Format::Yrc);
        assert_eq!(detect("[0,500]<0,500,0>夜"), Format::Krc);
        assert_eq!(detect("[4]夜(0,500)が(500,300)"), Format::Lys);
        assert_eq!(detect("[00:01.00]夜[00:01.50]が[00:02.00]"), Format::EnhancedLrc);
        assert_eq!(detect("[00:01.00]<00:01.00>夜<00:01.50>が"), Format::EnhancedLrc);
        assert_eq!(detect("[ar:x]\n[00:01.00]夜が明ける"), Format::Lrc);
        assert_eq!(detect("1\n00:00:01,000 --> 00:00:02,000\n夜\n"), Format::Srt);
        assert_eq!(detect("夜が明ける\n朝が来る"), Format::Text);
        // 括号里是普通文字的 LRC 不会被认成逐字明文
        assert_eq!(detect("[00:01.00]夜(よる)が明ける"), Format::Lrc);
    }

    #[test]
    fn binary_krc_is_recognised_without_a_hint() {
        let bytes = crypt::encode_krc("[1000,600]<0,200,0>夜<200,400,0>が");
        let decoded = decode(&bytes, None).unwrap();
        assert_eq!(decoded.format, Format::Krc);
        assert_eq!(decoded.parsed.lines[0].text, "夜が");
        assert_eq!(decoded.parsed.lines[0].words.len(), 2);
    }
}
