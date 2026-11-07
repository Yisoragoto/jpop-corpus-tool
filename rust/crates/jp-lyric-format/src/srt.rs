//! SRT 字幕当歌词：每条字幕有开始和结束时间，一条里可以有几行字。
//!
//! 只认标准写法 `HH:MM:SS,mmm --> HH:MM:SS,mmm`（毫秒前用逗号或点都行，小时可以省）。
//! 序号行可有可无；字幕里的 `<i>` 之类的标签去掉。认不出时间的块整块跳过，不猜。
//!
//! 一条字幕里有好几行时，每行各算一行歌词、共用这条的时间：
//! 不少歌词字幕是「原文 + 译文」两行一条，交给 LRC 那边同一套译文识别去分开。

use crate::lrc::LyricLine;

/// 一条字幕。
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start_sec: f64,
    pub end_sec: f64,
    pub lines: Vec<String>,
}

/// 解析结果：原文行（和 LRC 一样分出了夹带的译文）+ 每行的结束时间。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedSrt {
    pub lines: Vec<LyricLine>,
    /// 和 `lines` 一一对应：这一行所在字幕的结束时间
    pub ends: Vec<f64>,
    pub translations: Vec<LyricLine>,
}

pub fn parse_cues(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let mut rows = block.lines().map(str::trim).filter(|l| !l.is_empty()).peekable();
        // 序号行
        if rows.peek().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit())) {
            rows.next();
        }
        let Some((start, end)) = rows.next().and_then(parse_range) else { continue };
        let lines: Vec<String> = rows.map(strip_tags).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        cues.push(Cue { start_sec: start, end_sec: end, lines });
    }
    cues
}

pub fn parse(text: &str) -> ParsedSrt {
    let cues = parse_cues(text);
    // 借 LRC 那套「同一时间戳下的中文行是译文」的识别：拼成 LRC 再解析一遍，
    // 两种格式分译文的规则才不会各说各的
    let lrc = to_lrc(&cues);
    let parsed = crate::lrc::parse(&lrc);
    let ends = parsed
        .lines
        .iter()
        .map(|l| {
            let t = l.time_sec.unwrap_or(0.0);
            cues.iter()
                .find(|c| (c.start_sec - t).abs() < 0.006 && c.lines.iter().any(|x| x == &l.text))
                .map_or(t, |c| c.end_sec)
        })
        .collect();
    ParsedSrt { lines: parsed.lines, ends, translations: parsed.translations }
}

/// 转成普通 LRC，好走现有的入库路径。结束时间在 LRC 里写不出来，丢掉；
/// 原文（SRT 本身）另外留在歌词来源里，时间轴从原文里拿结束时间。
pub fn to_lrc(cues: &[Cue]) -> String {
    let mut out = String::new();
    for cue in cues {
        let centis = (cue.start_sec.max(0.0) * 100.0).round() as u64;
        for line in &cue.lines {
            out.push_str(&format!("[{:02}:{:02}.{:02}]{}\n", centis / 6000, centis / 100 % 60, centis % 100, line));
        }
    }
    out
}

fn parse_range(line: &str) -> Option<(f64, f64)> {
    let (a, b) = line.split_once("-->")?;
    let start = parse_time(a.trim())?;
    // 结束时间后面可能还跟着位置参数（X1:… Y1:…）
    let end = parse_time(b.split_whitespace().next()?)?;
    (start.is_finite() && end.is_finite()).then_some((start, end))
}

fn parse_time(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?, sec.parse::<f64>().ok()?),
        [m, sec] => (0, m.parse::<u32>().ok()?, sec.parse::<f64>().ok()?),
        _ => return None,
    };
    (m < 60 && (0.0..60.0).contains(&sec)).then(|| f64::from(h) * 3600.0 + f64::from(m) * 60.0 + sec)
}

fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "1\r\n00:00:01,500 --> 00:00:04,000\r\n<i>夜が明ける</i>\r\n\r\n2\r\n00:00:05.000 --> 00:00:07,250 X1:10\r\n君を待つ\r\n等你\r\n\r\n壊れたブロック\r\n\r\n3\r\n01:02.000 --> 01:03.000\r\n最後\r\n";

    #[test]
    fn cues_carry_start_and_end_and_tags_are_stripped() {
        let cues = parse_cues(SAMPLE);
        assert_eq!(cues.len(), 3, "认不出时间的块跳过");
        assert_eq!(cues[0], Cue { start_sec: 1.5, end_sec: 4.0, lines: vec!["夜が明ける".into()] });
        assert_eq!(cues[1].end_sec, 7.25);
        assert_eq!(cues[2].start_sec, 62.0, "小时可以省");
    }

    #[test]
    fn a_translation_line_inside_a_cue_is_split_off_and_ends_follow_the_cue() {
        let parsed = parse(SAMPLE);
        let texts: Vec<_> = parsed.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["夜が明ける", "君を待つ", "最後"]);
        assert_eq!(parsed.ends, vec![4.0, 7.25, 63.0]);
        assert_eq!(parsed.translations.len(), 1);
        assert_eq!(parsed.translations[0].text, "等你");
        assert_eq!(parsed.translations[0].time_sec, Some(5.0));
    }

    #[test]
    fn bad_times_are_rejected() {
        assert_eq!(parse_time("00:61:00,000"), None);
        assert_eq!(parse_time("abc"), None);
        assert!(parse_cues("1\n00:00:01,000 -> 00:00:02,000\nx\n").is_empty());
    }
}
