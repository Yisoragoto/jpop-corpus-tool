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

// 下面两份是 #61 评论附的真实 `©lyr`，各裁到两三行（版权）：
// - ruby_word.ttml：TTML 2.0（QRC 转来），`tts:ruby` 标振假名，时间写在注音上
// - apple_music_word.ttml：Apple Music 原生 TTML，head 里有英文翻译和罗马音

fn has_kana(text: &str) -> bool {
    text.chars().any(|c| ('\u{3041}'..='\u{3096}').contains(&c))
}

#[test]
fn ruby_readings_stay_out_of_the_text_but_keep_their_timing() {
    let decoded = decode(&fixture("ruby_word.ttml"), None).unwrap();
    assert_eq!(decoded.format, Format::Ttml);
    let timed = decoded.timed.as_ref().unwrap();
    assert_eq!(timed.lines.len(), 3);
    assert_eq!(timed.lines[0].text, "曲：n-buna");
    assert!(timed.lines.iter().all(|l| !l.words.is_empty()), "每行都要有逐字");
    // 「曲：n-buna」是署名行，LRC 那层收进 credits；两行歌词都有逐字
    let lines = &decoded.parsed.lines;
    let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["やっと雨が降ったんだ", "この青をずっと思っていたんだ"]);
    assert!(lines.iter().all(|l| !l.words.is_empty()));
    // 「雨」整个字跟着注音「あめ」的时间走
    let (line, chars) = (&lines[0], lines[0].text.chars().collect::<Vec<_>>());
    let rain = line.words.iter().find(|w| chars[w.start..w.end].iter().collect::<String>() == "雨").unwrap();
    assert_eq!((ms(rain.time_sec), rain.end_sec.map(ms)), (19_747, Some(20_059)));
    // 「思」的注音是两段「お」「も」：取最早的开始和最晚的结束
    let chars: Vec<char> = lines[1].text.chars().collect();
    let think = lines[1].words.iter().find(|w| chars[w.start..w.end].iter().collect::<String>() == "思").unwrap();
    assert_eq!((ms(think.time_sec), think.end_sec.map(ms)), (24_350, Some(24_610)));
    assert_eq!(lines[1].words.len(), 14);
}

#[test]
fn apple_music_translations_attach_by_key_and_romaji_is_ignored() {
    let decoded = decode(&fixture("apple_music_word.ttml"), None).unwrap();
    assert_eq!(decoded.format, Format::Ttml);
    let lines = &decoded.parsed.lines;
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| !l.words.is_empty()));
    assert_eq!(lines[0].text, "灰色に白んだ言葉はカプチーノみたいな色してる");
    assert!(lines.iter().all(|l| !l.text.is_ascii() && !l.text.contains("haiiro")));
    let translations = &decoded.parsed.translations;
    assert_eq!(translations.len(), 2);
    assert_eq!(translations[0].text, "The words that faded to gray are the color of a cappuccino");
    assert_eq!(translations[0].time_sec, lines[0].time_sec);
    assert_eq!(translations[1].time_sec, lines[1].time_sec);
    assert!(translations.iter().all(|l| !l.text.contains("kotoba") && !has_kana(&l.text)));
}
