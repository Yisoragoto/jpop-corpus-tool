//! 注音分配与音高判断。
//!
//! 移植自 Yomitan `ext/js/language/ja/japanese.js`（Copyright (C) 2024-2026 Yomitan Authors；
//! Copyright (C) 2020-2022 Yomichan Authors；GPL-3.0-or-later）。
//! 前端 `app/src/dict/japanese.ts` 是同一段代码的 TS 版，两边用同一份基准
//! `tests/fixtures/yomitan-japanese-util.json`（Yomitan 自带用例 + 1500 个真实词头）对账。
//!
//! JS 版按 UTF-16 码元取长度、切子串；这里按字符。两者只在 BMP 以外的字上才会不同，
//! 那种字不会是假名，分组、比较的结果相同。

use serde::Serialize;
use serde_json::Value;

use crate::text::convert_katakana_to_hiragana;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FuriganaSegment {
    pub text: String,
    pub reading: String,
}

fn segment(text: impl Into<String>, reading: impl Into<String>) -> FuriganaSegment {
    FuriganaSegment { text: text.into(), reading: reading.into() }
}

fn is_kana(c: char) -> bool {
    // 平假名 U+3040–309F、片假名 U+30A0–30FF
    matches!(c as u32, 0x3040..=0x30ff)
}

const SMALL_KANA: &str = "ぁぃぅぇぉゃゅょゎァィゥェォャュョヮ";

fn is_small_kana(c: char) -> bool {
    SMALL_KANA.contains(c)
}

struct Group {
    is_kana: bool,
    text: Vec<char>,
    normalized: Option<Vec<char>>,
}

fn substring(chars: &[char], start: usize) -> &[char] {
    &chars[start.min(chars.len())..]
}

fn segmentize(reading: &[char], normalized: &[char], groups: &[Group], start: usize) -> Option<Vec<FuriganaSegment>> {
    let count = groups.len().saturating_sub(start);
    let Some(group) = groups.get(start) else {
        return reading.is_empty().then(Vec::new);
    };
    let text_len = group.text.len();
    if group.is_kana {
        let group_normalized = group.normalized.as_deref().unwrap_or(&[]);
        if normalized.starts_with(group_normalized)
            && let Some(mut segments) =
                segmentize(substring(reading, text_len), substring(normalized, text_len), groups, start + 1)
        {
            if reading.starts_with(&group.text) {
                segments.insert(0, segment(String::from_iter(&group.text), ""));
            } else {
                let mut kana = kana_segments(&group.text, reading);
                kana.append(&mut segments);
                segments = kana;
            }
            return Some(segments);
        }
        return None;
    }

    let mut result: Option<Vec<FuriganaSegment>> = None;
    let mut i = reading.len();
    while i >= text_len {
        if let Some(mut segments) = segmentize(substring(reading, i), substring(normalized, i), groups, start + 1) {
            if result.is_some() {
                // 尾部不止一种分法，算有歧义
                return None;
            }
            segments.insert(0, segment(String::from_iter(&group.text), String::from_iter(&reading[..i])));
            result = Some(segments);
        }
        // 最后一个非假名组只有一种分法
        if count == 1 || i == 0 {
            break;
        }
        i -= 1;
    }
    result
}

/// JS `substring(start, end)`：越界的下标夹到长度以内。
fn slice(chars: &[char], start: usize, end: usize) -> String {
    let end = end.min(chars.len());
    String::from_iter(&chars[start.min(end)..end])
}

fn kana_segments(text: &[char], reading: &[char]) -> Vec<FuriganaSegment> {
    let same = |i: usize| reading.get(i) == text.get(i);
    let mut out = Vec::new();
    let mut start = 0;
    let mut state = same(0);
    for i in 1..text.len() {
        let next = same(i);
        if next == state {
            continue;
        }
        let reading_part = if state { String::new() } else { slice(reading, start, i) };
        out.push(segment(slice(text, start, i), reading_part));
        state = next;
        start = i;
    }
    let end = text.len();
    let reading_part = if state { String::new() } else { slice(reading, start, end) };
    out.push(segment(slice(text, start, end), reading_part));
    out
}

/// 把读音分配到词形的各段上：汉字段带读音，假名段读音为空。分不出来时整词一段。
pub fn distribute_furigana(term: &str, reading: &str) -> Vec<FuriganaSegment> {
    if reading == term {
        return vec![segment(term, "")];
    }
    let mut groups: Vec<Group> = Vec::new();
    for c in term.chars() {
        let kana = is_kana(c);
        match groups.last_mut() {
            Some(last) if last.is_kana == kana => last.text.push(c),
            _ => groups.push(Group { is_kana: kana, text: vec![c], normalized: None }),
        }
    }
    for group in &mut groups {
        if group.is_kana {
            let text = String::from_iter(&group.text);
            group.normalized = Some(convert_katakana_to_hiragana(&text, false).chars().collect());
        }
    }
    let reading_chars: Vec<char> = reading.chars().collect();
    let normalized: Vec<char> = convert_katakana_to_hiragana(reading, false).chars().collect();
    segmentize(&reading_chars, &normalized, &groups, 0).unwrap_or_else(|| vec![segment(term, reading)])
}

fn stem_length(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// 活用形的注音：词干按词典形分配，活用词尾照原文显示。
pub fn distribute_furigana_inflected(term: &str, reading: &str, source: &str) -> Vec<FuriganaSegment> {
    let term_normalized: Vec<char> = convert_katakana_to_hiragana(term, false).chars().collect();
    let reading_normalized: Vec<char> = convert_katakana_to_hiragana(reading, false).chars().collect();
    let source_normalized: Vec<char> = convert_katakana_to_hiragana(source, false).chars().collect();
    let source_chars: Vec<char> = source.chars().collect();

    let mut main_text: Vec<char> = term.chars().collect();
    let mut reading_chars: Vec<char> = reading.chars().collect();
    let mut stem = stem_length(&term_normalized, &source_normalized);

    // 原文是从读音变过来的（而不是从词形）
    let reading_stem = stem_length(&reading_normalized, &source_normalized);
    if reading_stem > 0 && reading_stem >= stem {
        main_text = reading_chars.clone();
        stem = reading_stem;
        let mut replaced: Vec<char> = source_chars[..stem.min(source_chars.len())].to_vec();
        replaced.extend_from_slice(substring(&reading_chars, stem));
        reading_chars = replaced;
    }

    let mut segments: Vec<FuriganaSegment> = Vec::new();
    if stem > 0 {
        let mut replaced: Vec<char> = source_chars[..stem.min(source_chars.len())].to_vec();
        replaced.extend_from_slice(substring(&main_text, stem));
        main_text = replaced;
        let whole = distribute_furigana(&String::from_iter(&main_text), &String::from_iter(&reading_chars));
        let mut consumed = 0;
        for seg in whole {
            let start = consumed;
            consumed += seg.text.chars().count();
            if consumed < stem {
                segments.push(seg);
            } else if consumed == stem {
                segments.push(seg);
                break;
            } else {
                if start < stem {
                    segments.push(segment(String::from_iter(&main_text[start..stem.min(main_text.len())]), ""));
                }
                break;
            }
        }
    }

    if stem < source_chars.len() {
        let remainder = String::from_iter(&source_chars[stem..]);
        match segments.last_mut() {
            // 最后一段没有读音就接在它后面
            Some(last) if last.reading.is_empty() => last.text.push_str(&remainder),
            _ => segments.push(segment(remainder, "")),
        }
    }
    segments
}

pub fn kana_morae(text: &str) -> Vec<String> {
    let mut morae: Vec<String> = Vec::new();
    for c in text.chars() {
        match morae.last_mut() {
            Some(last) if is_small_kana(c) => last.push(c),
            _ => morae.push(c.to_string()),
        }
    }
    morae
}

pub fn kana_mora_count(text: &str) -> usize {
    let mut count = 0;
    for c in text.chars() {
        if !(is_small_kana(c) && count > 0) {
            count += 1;
        }
    }
    count
}

/// `LHHL` 这种高低串里的降调位置；没有降调时平板记 0、全高记 -1。
pub fn downstep_positions(pitch: &str) -> Vec<i64> {
    let chars: Vec<char> = pitch.chars().collect();
    let mut out: Vec<i64> = (1..chars.len()).filter(|&i| chars[i - 1] == 'H' && chars[i] == 'L').map(|i| i as i64).collect();
    if out.is_empty() {
        out.push(if pitch.starts_with('L') { 0 } else { -1 });
    }
    out
}

pub fn is_mora_pitch_high(mora_index: usize, positions: &Value) -> bool {
    match positions {
        Value::String(s) => s.chars().nth(mora_index) == Some('H'),
        Value::Number(n) => {
            let p = n.as_f64().unwrap_or(0.0);
            let i = mora_index as f64;
            if p == 0.0 {
                mora_index > 0
            } else if p == 1.0 {
                mora_index < 1
            } else {
                mora_index > 0 && i < p
            }
        }
        _ => false,
    }
}

/// 对应 JS `getPitchCategory`：heiban / kifuku / atamadaka / odaka / nakadaka。
pub fn pitch_category(text: &str, positions: &Value, is_verb_or_adjective: bool) -> Option<&'static str> {
    let downstep = match positions {
        Value::String(s) => downstep_positions(s)[0] as f64,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    if downstep == 0.0 {
        return Some("heiban");
    }
    if is_verb_or_adjective {
        return (downstep > 0.0).then_some("kifuku");
    }
    if downstep == 1.0 {
        return Some("atamadaka");
    }
    if downstep > 1.0 {
        return Some(if downstep >= kana_mora_count(text) as f64 { "odaka" } else { "nakadaka" });
    }
    None
}

/// 对应 JS `isNonNounVerbOrAdjective`：サ变动词兼名词（`vs` + `n`）不算。
pub fn is_non_noun_verb_or_adjective<S: AsRef<str>>(word_classes: &[S]) -> bool {
    let mut verb_or_adjective = false;
    let mut suru = false;
    let mut noun = false;
    for c in word_classes {
        match c.as_ref() {
            "v1" | "v5" | "vk" | "vz" | "adj-i" => verb_or_adjective = true,
            "vs" => {
                verb_or_adjective = true;
                suru = true;
            }
            "n" => noun = true,
            _ => {}
        }
    }
    verb_or_adjective && !(suru && noun)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn okurigana_is_split_and_plain_kanji_is_one_segment() {
        assert_eq!(
            distribute_furigana("打ち込む", "うちこむ"),
            vec![segment("打", "う"), segment("ち", ""), segment("込", "こ"), segment("む", "")]
        );
        assert_eq!(distribute_furigana("学校", "がっこう"), vec![segment("学校", "がっこう")]);
        assert_eq!(distribute_furigana("スムーズ", "スムーズ"), vec![segment("スムーズ", "")]);
    }

    #[test]
    fn pitch_categories_follow_the_downstep() {
        assert_eq!(pitch_category("はし", &Value::from(0), false), Some("heiban"));
        assert_eq!(pitch_category("はし", &Value::from(2), false), Some("odaka"));
        assert_eq!(pitch_category("おとうと", &Value::from(2), false), Some("nakadaka"));
        assert_eq!(pitch_category("たべる", &Value::from(2), true), Some("kifuku"));
        assert!(is_non_noun_verb_or_adjective(&["v5"]));
        assert!(!is_non_noun_verb_or_adjective(&["vs", "n"]));
    }
}
