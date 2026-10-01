//! 歌词振假名：把一行歌词切成（原文, 读音）段，读音为空表示这一段不用注。
//!
//! 照 Python 版 `dialogs/song_manager.py` 的 `_furigana_tokens` / `_kanji_only_furigana` 逐行移植
//! （SplitMode C、`reading_form()`、片假名转平假名），两种模式：
//!
//! - `Kanji`（默认）：只给汉字那几个字注。一个词里汉字和假名交替时，按词里的假名在读音里定位，切出每段汉字的读音；
//! - `Word`：整个词注一个读音。
//!
//! 字符串都按 Unicode 字符（码点）处理，和 Python 的 `str` 下标一致。对账见 `jp-app/examples/dump_furigana.rs`。

use serde::{Deserialize, Serialize};

use crate::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FuriganaMode {
    #[default]
    Kanji,
    Word,
}

/// 一行歌词的振假名段。`text` 是分词的原文：分词结果一个都没有时退回整行不注音。
pub fn furigana_segments(text: &str, tokens: &[Token], mode: FuriganaMode) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for token in tokens {
        let surface = token.surface.as_str();
        let mut reading = if token.reading.is_empty() || token.reading == "*" {
            String::new()
        } else {
            katakana_to_hiragana(&token.reading)
        };
        if !surface.chars().any(is_kanji_re) || reading.is_empty() || reading == surface {
            reading.clear();
        }
        if !reading.is_empty() && mode == FuriganaMode::Kanji {
            result.extend(kanji_only_furigana(surface, &reading));
        } else {
            result.push((surface.to_owned(), reading));
        }
    }
    if result.is_empty() {
        // Python 版 Sudachi 没切出东西时走 GiNZA，再不行就是整行不注音
        result.push((text.to_owned(), String::new()));
    }
    result
}

/// `_KANJI_RE = [㐀-䶿一-鿿]`（不含「々」）
fn is_kanji_re(ch: char) -> bool {
    matches!(ch, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
}

/// 一段注音在原文里的位置，按字符（码点）数，左闭右开
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ruby {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

/// 振假名段 → 需要注音的那几段在原文里的位置。段拼起来和原文对不上时返回 None（不猜位置）。
pub fn ruby_spans(text: &str, segments: &[(String, String)]) -> Option<Vec<Ruby>> {
    let mut rubies = Vec::new();
    let mut offset = 0;
    let mut joined = String::with_capacity(text.len());
    for (surface, reading) in segments {
        let len = surface.chars().count();
        if !reading.is_empty() {
            rubies.push(Ruby { start: offset, end: offset + len, reading: reading.clone() });
        }
        offset += len;
        joined.push_str(surface);
    }
    (joined == text).then_some(rubies)
}

/// `_is_kanji_char`：拆汉字段时「々」也算汉字
fn is_kanji_char(ch: char) -> bool {
    is_kanji_re(ch) || ch == '々'
}

fn is_kana_char(ch: char) -> bool {
    matches!(ch, '\u{3040}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}') || ch == 'ー'
}

pub fn katakana_to_hiragana(text: &str) -> String {
    text.chars()
        .map(|ch| if ('ァ'..='ヶ').contains(&ch) { char::from_u32(ch as u32 - 0x60).unwrap_or(ch) } else { ch })
        .collect()
}

/// 一段文字里的假名（转成平假名），用来在读音里定位
fn surface_reading_key(text: &str) -> Vec<char> {
    let kana: String = text.chars().filter(|&ch| is_kana_char(ch)).collect();
    katakana_to_hiragana(&kana).chars().collect()
}

fn split_kanji_runs(surface: &str) -> Vec<(String, bool)> {
    let mut parts: Vec<(String, bool)> = Vec::new();
    for ch in surface.chars() {
        let kanji = is_kanji_char(ch);
        match parts.last_mut() {
            Some((buf, is_kanji)) if *is_kanji == kanji => buf.push(ch),
            _ => parts.push((ch.to_string(), kanji)),
        }
    }
    parts
}

/// `str.find(sub, start)`，按字符
fn find_from(haystack: &[char], needle: &[char], start: usize) -> Option<usize> {
    if start > haystack.len() {
        return None;
    }
    if needle.is_empty() {
        return Some(start);
    }
    (start..=haystack.len().saturating_sub(needle.len())).find(|&i| haystack[i..].starts_with(needle))
}

fn kanji_only_furigana(surface: &str, reading_text: &str) -> Vec<(String, String)> {
    if !surface.chars().any(is_kanji_re) || reading_text.is_empty() {
        return vec![(surface.to_owned(), String::new())];
    }
    let reading: Vec<char> = katakana_to_hiragana(reading_text).chars().collect();
    let parts = split_kanji_runs(surface);
    let mut result = Vec::new();
    let mut cursor = 0usize;
    for (index, (part, is_kanji)) in parts.iter().enumerate() {
        if !is_kanji {
            let key = surface_reading_key(part);
            if !key.is_empty() {
                if reading[cursor.min(reading.len())..].starts_with(&key) {
                    cursor += key.len();
                } else if let Some(found) = find_from(&reading, &key, cursor) {
                    cursor = found + key.len();
                }
            }
            result.push((part.clone(), String::new()));
            continue;
        }

        let next_key = parts[index + 1..]
            .iter()
            .filter(|(_, kanji)| !kanji)
            .map(|(next, _)| surface_reading_key(next))
            .find(|key| !key.is_empty())
            .unwrap_or_default();
        let part_reading: String = if !next_key.is_empty() {
            match find_from(&reading, &next_key, cursor) {
                Some(next_pos) => {
                    let text = reading[cursor..next_pos].iter().collect();
                    cursor = next_pos;
                    text
                }
                None => String::new(),
            }
        } else {
            let text = reading[cursor.min(reading.len())..].iter().collect();
            cursor = reading.len();
            text
        };
        let keep = !part_reading.is_empty() && part_reading != *part;
        result.push((part.clone(), if keep { part_reading } else { String::new() }));
    }
    if result.is_empty() {
        return vec![(surface.to_owned(), reading.iter().collect())];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(surface: &str, reading: &str) -> Token {
        Token {
            surface: surface.into(),
            lemma: surface.into(),
            normalized: surface.into(),
            reading: reading.into(),
            pos: vec![],
            upos: "NOUN".into(),
        }
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn kanji_mode_splits_readings_around_okurigana() {
        let tokens = [token("思い出し", "オモイダシ"), token("た", "タ")];
        assert_eq!(
            furigana_segments("思い出した", &tokens, FuriganaMode::Kanji),
            pairs(&[("思", "おも"), ("い", ""), ("出", "だ"), ("し", ""), ("た", "")])
        );
        assert_eq!(
            furigana_segments("思い出した", &tokens, FuriganaMode::Word),
            pairs(&[("思い出し", "おもいだし"), ("た", "")])
        );
    }

    #[test]
    fn kana_only_and_same_reading_tokens_get_no_ruby() {
        let tokens = [token("ユリイカ", "ユリイカ"), token("love", "love"), token("僕", "*")];
        assert_eq!(
            furigana_segments("ユリイカlove僕", &tokens, FuriganaMode::Kanji),
            pairs(&[("ユリイカ", ""), ("love", ""), ("僕", "")])
        );
    }

    #[test]
    fn iteration_mark_belongs_to_the_kanji_run() {
        let tokens = [token("時々", "トキドキ")];
        assert_eq!(furigana_segments("時々", &tokens, FuriganaMode::Kanji), pairs(&[("時々", "ときどき")]));
    }

    #[test]
    fn a_reading_that_does_not_contain_the_kana_leaves_the_kanji_bare() {
        // 读音里找不到后面的假名：Python 版给空读音，不猜
        let tokens = [token("今日は", "キョウワ")];
        assert_eq!(furigana_segments("今日は", &tokens, FuriganaMode::Kanji), pairs(&[("今日", ""), ("は", "")]));
    }

    #[test]
    fn empty_lines_fall_back_to_the_whole_text() {
        assert_eq!(furigana_segments("", &[], FuriganaMode::Kanji), pairs(&[("", "")]));
    }

    #[test]
    fn ruby_spans_count_characters_not_bytes() {
        let segments = pairs(&[("🎵", ""), ("思", "おも"), ("い", ""), ("出", "だ")]);
        assert_eq!(
            ruby_spans("🎵思い出", &segments),
            Some(vec![
                Ruby { start: 1, end: 2, reading: "おも".into() },
                Ruby { start: 3, end: 4, reading: "だ".into() },
            ])
        );
        assert_eq!(ruby_spans("別の文", &segments), None);
    }

    #[test]
    fn katakana_becomes_hiragana_but_prolonged_mark_stays() {
        assert_eq!(katakana_to_hiragana("ヴァイオリンー"), "ゔぁいおりんー");
    }
}
