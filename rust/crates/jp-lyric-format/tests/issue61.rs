//! #61 报告者那五种内嵌逐字歌词：QRC、YRC、ESLRC、Lyrics A2、TTML。
//!
//! 夹具照报告附的 mutagen 日志里五首歌 `©lyr` 的样子造（同样的 `[ti:…]` 头、`[kana:…]` 注音行、
//! TTML 的 itunes 命名空间和注释），歌词是编的。五份是同一份时间轴的五种写法，
//! 所以都不给提示、只看内容解析，应该得到一模一样的逐字时间。

use jp_lyric_format::{Format, decode};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

/// 每行：(行文本, 行开始毫秒, [(字, 开始毫秒, 结束毫秒)])
type Timing = Vec<(String, i64, Vec<(String, i64, Option<i64>)>)>;

fn ms(sec: f64) -> i64 {
    (sec * 1000.0).round() as i64
}

fn timing(name: &str) -> (Format, Timing) {
    let decoded = decode(&fixture(name), None).unwrap_or_else(|e| panic!("{name}: {e:#}"));
    let lines = decoded
        .parsed
        .lines
        .iter()
        .map(|line| {
            let chars: Vec<char> = line.text.chars().collect();
            let words = line
                .words
                .iter()
                .map(|w| (chars[w.start..w.end].iter().collect::<String>().trim().to_string(), ms(w.time_sec), w.end_sec.map(ms)))
                .collect();
            (line.text.clone(), ms(line.time_sec.unwrap()), words)
        })
        .collect();
    (decoded.format, lines)
}

fn expected() -> Timing {
    let w = |s: &str, a: i64, b: i64| (s.to_string(), a, Some(b));
    vec![
        ("夜が明ける".into(), 0, vec![w("夜", 0, 500), w("が", 500, 900), w("明", 900, 1500), w("け", 1500, 2000), w("る", 2000, 3000)]),
        ("朝の光".into(), 3200, vec![w("朝", 3200, 3900), w("の", 3900, 4200), w("光", 4200, 5200)]),
        ("Hello world".into(), 5400, vec![w("Hello", 5400, 6000), w("world", 6000, 7000)]),
    ]
}

#[test]
fn all_five_embedded_formats_decode_to_the_same_word_timing() {
    for (name, format) in [
        ("embedded_qrc.txt", Format::Qrc),
        ("embedded_yrc.txt", Format::Yrc),
        ("embedded_eslrc.txt", Format::EnhancedLrc),
        ("embedded_a2.txt", Format::EnhancedLrc),
        ("embedded_ttml.xml", Format::Ttml),
    ] {
        let (got_format, lines) = timing(name);
        assert_eq!(got_format, format, "{name}");
        assert_eq!(lines, expected(), "{name}");
    }
}

#[test]
fn header_tags_and_kana_lines_never_become_lyrics() {
    for name in ["embedded_qrc.txt", "embedded_yrc.txt", "embedded_eslrc.txt", "embedded_a2.txt", "embedded_ttml.xml"] {
        let decoded = decode(&fixture(name), None).unwrap();
        assert_eq!(decoded.parsed.lines.len(), 3, "{name}");
        assert!(decoded.parsed.lines.iter().all(|l| !l.text.contains("kana") && !l.text.contains('[')), "{name}");
    }
}

#[test]
fn word_timed_formats_keep_the_original_and_convert_to_enhanced_lrc() {
    let decoded = decode(&fixture("embedded_qrc.txt"), None).unwrap();
    assert!(decoded.original.starts_with("[ti:夜明けの歌]"));
    assert!(decoded.lrc.starts_with("[00:00.000]<00:00.000>夜<00:00.500>"), "{}", decoded.lrc);
    assert_eq!(decoded.timed.as_ref().unwrap().lines.len(), 3);
    // LRC 一家：入库的文本就是原文
    let decoded = decode(&fixture("embedded_eslrc.txt"), None).unwrap();
    assert_eq!(decoded.lrc, decoded.original);
    assert!(decoded.timed.is_none());
}

#[test]
fn the_extension_hint_wins_over_the_content() {
    // .lrc 里装的是 QRC 明文：照提示按 LRC 解析，不去猜
    let decoded = decode(&fixture("embedded_qrc.txt"), Some("lrc")).unwrap();
    assert_eq!(decoded.format, Format::Text);
    let decoded = decode(&fixture("embedded_qrc.txt"), Some("qrc")).unwrap();
    assert_eq!(decoded.format, Format::Qrc);
    assert!(decode(&fixture("embedded_a2.txt"), Some("yrc")).is_err());
}
