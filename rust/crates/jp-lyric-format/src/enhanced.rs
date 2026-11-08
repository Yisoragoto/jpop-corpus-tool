//! 严格版的带逐字 LRC：只认真正带逐字时间的行，绝不把一行的时长均分给字。
//!
//! 给在线来源（LRCLIB 的 syncedLyrics）用：那边要的是「这是不是一份真的逐字歌词」，
//! 不是的就报错，让调用方退回逐行。入库那条宽松的路是 [`crate::lrc::parse`]。
//!
//! 两种行内写法都认：Lyrics A2 的 `<mm:ss.xx>`，和 ESLyric 的行内 `[mm:ss.xx]`
//! （ESLyric 的行首时间戳就是第一个字的开始）。

use anyhow::{Result, anyhow, ensure};

use crate::timed::{TimedLine, TimedLyrics, TimedWord};

const MAX_BYTES: usize = 8 * 1024 * 1024;

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
pub fn has_line_clocks(text: &str) -> bool {
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
        let eslyric = eslyric_as_a2(head, body);
        let body = eslyric.as_deref().unwrap_or(body);
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
            rubies: Vec::new(),
        });
    }
    ensure!(!lines.is_empty(), "没有 Enhanced LRC 正文");
    Ok(TimedLyrics { lines })
}

/// ESLyric 的一行正文（行首时间戳已经拿掉）改写成 Lyrics A2 的写法：
/// 行内的 `[mm:ss.xx]` 换成 `<mm:ss.xx>`，开头补上行首时间（它就是第一个字的开始）。
/// 正文里没有方括号时间的，返回 None，原样按 A2 解析。
fn eslyric_as_a2(head: &str, body: &str) -> Option<String> {
    let square = regex::Regex::new(r"\[([^\[\]]+)\]").expect("constant square tag regex");
    if !square.captures_iter(body).any(|c| elrc_clock(&c[1]).is_ok()) {
        return None;
    }
    let rewritten = square.replace_all(body, |c: &regex::Captures<'_>| {
        if elrc_clock(&c[1]).is_ok() { format!("<{}>", &c[1]) } else { c[0].to_string() }
    });
    Some(if rewritten.starts_with('<') { rewritten.into_owned() } else { format!("<{head}>{rewritten}") })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn eslyric_square_brackets_time_the_same_words_as_a2() {
        let a2 = parse_enhanced_lrc("[00:01.00]<00:01.00>夜<00:01.50>が<00:02.00>").unwrap();
        let es = parse_enhanced_lrc("[00:01.00]夜[00:01.50]が[00:02.00]").unwrap();
        assert_eq!(a2, es);
        assert_eq!(es.lines[0].words.len(), 2);
        assert_eq!(es.lines[0].words[1].start_ms, 1500);
        assert_eq!(es.lines[0].words[1].end_ms, Some(2000));
        // 方括号里不是时间的仍是正文；整行没有逐字时间照样报错
        assert!(parse_enhanced_lrc("[00:01.00][Chorus]夜").is_err());
    }
}
