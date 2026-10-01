//! 日语文字处理：假名互转、全半角、合成用浊点、CJK 兼容字符、部首、强调符号、查词字符范围。
//!
//! 移植自 Yomitan（GPL-3.0-or-later，Copyright (C) 2024-2026 Yomitan Authors）：
//! - `ext/js/language/ja/japanese.js`
//! - `ext/js/language/ja/japanese-wanakana.js`
//! - `ext/js/language/CJK-util.js`
//!
//! 码表 `data/japanese-text-tables.json` 由 `tools/dump-yomitan-text-processing.mjs` 导出：
//! 半角片假名表、罗马字表原样取自 Yomitan 源码；NFKD 映射和字符范围是在 Node 里逐码位算出来的，
//! 这样不会因为 Rust 与 JS 引擎的 Unicode 版本不同而分叉。
//!
//! JS 版有几处按 UTF-16 码元而不是按字符遍历。涉及的特殊字符全在 BMP 内，
//! 对 BMP 以外的字符两种遍历结果相同（逐个函数核对过），这里统一按 `char` 处理。

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TablesJson {
    preprocessor_ids: Vec<String>,
    max_process_variants: usize,
    halfwidth_katakana: Vec<(String, String)>,
    vowel_to_kana: Vec<(String, String)>,
    romaji_to_hiragana: Vec<(String, String)>,
    cjk_compatibility_nfkd: Vec<(u32, String)>,
    radicals_nfkd: Vec<(u32, String)>,
    japanese_ranges: Vec<(u32, u32)>,
    lookup_ranges: Vec<(u32, u32)>,
}

pub(crate) struct Tables {
    /// 只在测试里用来核对预处理器顺序
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) preprocessor_ids: Vec<String>,
    pub(crate) max_process_variants: usize,
    halfwidth: HashMap<char, [char; 3]>,
    /// 片假名 → 长音「ー」该变成的平假名；`None` 表示保留「ー」
    prolonged: HashMap<char, Option<char>>,
    /// 顺序有意义：JS 按对象键顺序逐个 replaceAll，双写辅音必须先换
    romaji: Vec<(String, String)>,
    cjk_compatibility: HashMap<char, String>,
    radicals: HashMap<char, String>,
    japanese_ranges: Vec<(u32, u32)>,
    lookup_ranges: Vec<(u32, u32)>,
}

pub(crate) static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    let json: TablesJson = serde_json::from_str(include_str!("../data/japanese-text-tables.json"))
        .expect("内置的日语码表应当能解析");

    let halfwidth = json
        .halfwidth_katakana
        .into_iter()
        .map(|(k, v)| {
            let chars: Vec<char> = v.chars().collect();
            let mapping: [char; 3] = chars.try_into().expect("半角片假名映射应为 3 个字符");
            (single_char(&k), mapping)
        })
        .collect();

    // JS 的 KANA_TO_VOWEL_MAPPING 按声明顺序 set，后写的覆盖先写的（「の」最终落在空元音上）
    let mut prolonged = HashMap::new();
    for (vowel, kana) in &json.vowel_to_kana {
        let long = match vowel.as_str() {
            "a" => Some('あ'),
            "i" => Some('い'),
            "u" | "o" => Some('う'),
            "e" => Some('え'),
            _ => None,
        };
        for c in kana.chars() {
            prolonged.insert(c, long);
        }
    }

    let nfkd = |rows: Vec<(u32, String)>| -> HashMap<char, String> {
        rows.into_iter()
            .map(|(cp, s)| (char::from_u32(cp).expect("码表里的码位应当合法"), s))
            .collect()
    };

    Tables {
        preprocessor_ids: json.preprocessor_ids,
        max_process_variants: json.max_process_variants,
        halfwidth,
        prolonged,
        romaji: json.romaji_to_hiragana,
        cjk_compatibility: nfkd(json.cjk_compatibility_nfkd),
        radicals: nfkd(json.radicals_nfkd),
        japanese_ranges: json.japanese_ranges,
        lookup_ranges: json.lookup_ranges,
    }
});

fn single_char(s: &str) -> char {
    let mut it = s.chars();
    let c = it.next().expect("码表键不应为空");
    assert!(it.next().is_none(), "码表键应为单个字符: {s}");
    c
}

fn in_ranges(cp: u32, ranges: &[(u32, u32)]) -> bool {
    let i = ranges.partition_point(|&(_, max)| max < cp);
    ranges.get(i).is_some_and(|&(min, _)| min <= cp)
}

/// 平假名、片假名、汉字、半角片假名、日文标点、全角字母数字。对应 JS `isCodePointJapanese`。
pub fn is_code_point_japanese(cp: u32) -> bool {
    in_ranges(cp, &TABLES.japanese_ranges)
}

/// 日、中、韩任一文字。查词前按它截断输入，对应 JS `_getJapaneseChineseKoreanOnlyText`。
pub fn is_lookup_code_point(cp: u32) -> bool {
    in_ranges(cp, &TABLES.lookup_ranges)
}

/// 从头取到第一个非日中韩字符为止。
pub fn japanese_chinese_korean_only_prefix(text: &str) -> &str {
    match text.char_indices().find(|&(_, c)| !is_lookup_code_point(c as u32)) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

pub fn convert_katakana_to_hiragana(text: &str, keep_prolonged_sound_marks: bool) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        let out = match ch as u32 {
            // ヵ ヶ 不转
            0x30f5 | 0x30f6 => ch,
            0x30fc => {
                if !keep_prolonged_sound_marks
                    && let Some(prev) = result.chars().next_back()
                    && let Some(Some(long)) = TABLES.prolonged.get(&prev)
                {
                    *long
                } else {
                    ch
                }
            }
            cp @ 0x30a1..=0x30f6 => char::from_u32(cp - 0x60).unwrap_or(ch),
            _ => ch,
        };
        result.push(out);
    }
    result
}

pub fn convert_hiragana_to_katakana(text: &str) -> String {
    text.chars()
        .map(|ch| match ch as u32 {
            cp @ 0x3041..=0x3096 => char::from_u32(cp + 0x60).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

pub fn convert_alphanumeric_to_full_width(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let cp = ch as u32;
            let mapped = match cp {
                0x30..=0x39 => cp + (0xff10 - 0x30),
                0x41..=0x5a => cp + (0xff21 - 0x41),
                0x61..=0x7a => cp + (0xff41 - 0x61),
                _ => cp,
            };
            char::from_u32(mapped).unwrap_or(ch)
        })
        .collect()
}

pub fn convert_full_width_alphanumeric_to_normal(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let cp = ch as u32;
            let mapped = match cp {
                0xff10..=0xff19 => cp - (0xff10 - 0x30),
                0xff21..=0xff3a => cp - (0xff21 - 0x41),
                0xff41..=0xff5a => cp - (0xff41 - 0x61),
                _ => cp,
            };
            char::from_u32(mapped).unwrap_or(ch)
        })
        .collect()
}

/// `ｶﾞｯｺｳ` → `ガッコウ`。浊点/半浊点和前一个假名合并；不能合并的（`ﾅﾞ`）原样留下。
pub fn convert_half_width_kana_to_full_width(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let Some(mapping) = TABLES.halfwidth.get(&c) else {
            result.push(c);
            i += 1;
            continue;
        };
        let index = match chars.get(i + 1) {
            Some('\u{ff9e}') => 1,
            Some('\u{ff9f}') => 2,
            _ => 0,
        };
        let mut out = mapping[index];
        if index > 0 {
            if out == '-' {
                out = mapping[0];
            } else {
                i += 1;
            }
        }
        result.push(out);
        i += 1;
    }
    result
}

/// `yomichan` → `よみちゃん`。只转连续的（全角/半角）拉丁字母和连字符，其余字符原样保留。
pub fn convert_alphabetic_to_kana(text: &str) -> String {
    let mut part = String::new();
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        let mapped = match cp {
            0x41..=0x5a => cp + (0x61 - 0x41),
            0x61..=0x7a => cp,
            0xff21..=0xff3a => cp - 0xff21 + 0x61,
            0xff41..=0xff5a => cp - 0xff41 + 0x61,
            0x2d | 0xff0d => 0x2d,
            _ => {
                if !part.is_empty() {
                    result.push_str(&convert_to_hiragana(&part));
                    part.clear();
                }
                result.push(ch);
                continue;
            }
        };
        part.push(char::from_u32(mapped).unwrap_or(ch));
    }
    if !part.is_empty() {
        result.push_str(&convert_to_hiragana(&part));
    }
    result
}

/// JS `convertToHiragana`：按罗马字表顺序逐个整体替换，再补促音间隙。
fn convert_to_hiragana(text: &str) -> String {
    let mut s = text.to_lowercase();
    for (romaji, kana) in &TABLES.romaji {
        if s.contains(romaji.as_str()) {
            s = s.replace(romaji.as_str(), kana);
        }
    }
    fill_sokuon_gaps(&s)
}

/// `/っ[a-z](?=っ)/g → っっ`，再对片假名大写做一遍。前瞻的「っ」不被消费，可以作为下一次匹配的开头。
fn fill_sokuon_gaps(text: &str) -> String {
    fn pass(text: &str, tsu: char, letters: std::ops::RangeInclusive<char>) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == tsu
                && i + 2 < chars.len()
                && letters.contains(&chars[i + 1])
                && chars[i + 2] == tsu
            {
                out.push(tsu);
                out.push(tsu);
                i += 2;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }
    pass(&pass(text, 'っ', 'a'..='z'), 'ッ', 'A'..='Z')
}

fn dakuten_allowed(cp: u32) -> bool {
    (0x304b..=0x3068).contains(&cp)
        || (0x306f..=0x307b).contains(&cp)
        || (0x30ab..=0x30c8).contains(&cp)
        || (0x30cf..=0x30db).contains(&cp)
}

fn handakuten_allowed(cp: u32) -> bool {
    (0x306f..=0x307b).contains(&cp) || (0x30cf..=0x30db).contains(&cp)
}

/// `ト` + U+3099 → `ド`。从后往前扫，第一个字符不会和前面的东西合并。
pub fn normalize_combining_characters(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut reversed = Vec::with_capacity(chars.len());
    let mut i = chars.len() as isize - 1;
    while i > 0 {
        let c = chars[i as usize];
        let prev = chars[i as usize - 1] as u32;
        let combined = match c {
            '\u{3099}' if dakuten_allowed(prev) => char::from_u32(prev + 1),
            '\u{309a}' if handakuten_allowed(prev) => char::from_u32(prev + 2),
            _ => None,
        };
        if let Some(combined) = combined {
            reversed.push(combined);
            i -= 2;
        } else {
            reversed.push(c);
            i -= 1;
        }
    }
    if i == 0 {
        reversed.push(chars[0]);
    }
    reversed.into_iter().rev().collect()
}

fn map_through(text: &str, table: &HashMap<char, String>) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match table.get(&ch) {
            Some(s) => out.push_str(s),
            None => out.push(ch),
        }
    }
    out
}

/// `㌀` → `アパート`（U+3300–U+33FF 做 NFKD）。
pub fn normalize_cjk_compatibility_characters(text: &str) -> String {
    map_through(text, &TABLES.cjk_compatibility)
}

/// `⼀` → `一`（康熙部首、部首补充、笔画做 NFKD）。
pub fn normalize_radicals(text: &str) -> String {
    map_through(text, &TABLES.radicals)
}

fn is_emphatic(c: char) -> bool {
    matches!(c, 'っ' | 'ッ' | 'ー')
}

/// `すっっごーーい` → `すっごーい`（`full_collapse` 为真时 → `すごい`）。首尾的强调符号保留。
pub fn collapse_emphatic_sequences(text: &str, full_collapse: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let left = chars.iter().take_while(|&&c| is_emphatic(c)).count();
    let right_trim = chars.iter().rev().take_while(|&&c| is_emphatic(c)).count();
    if left + right_trim >= chars.len() {
        return text.to_owned();
    }
    let right = chars.len() - right_trim; // 不含
    let mut out = String::with_capacity(text.len());
    out.extend(&chars[..left]);
    let mut current = None;
    for &c in &chars[left..right] {
        if is_emphatic(c) {
            if current != Some(c) {
                current = Some(c);
                if !full_collapse {
                    out.push(c);
                }
            }
        } else {
            current = None;
            out.push(c);
        }
    }
    out.extend(&chars[right..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // 例子取自 Yomitan 各个文本处理器自己的 description
    #[test]
    fn the_examples_yomitan_uses_to_describe_its_processors() {
        assert_eq!(convert_half_width_kana_to_full_width("ﾖﾐﾁｬﾝ"), "ヨミチャン");
        assert_eq!(convert_alphabetic_to_kana("yomichan"), "よみちゃん");
        assert_eq!(convert_full_width_alphanumeric_to_normal("ｙｏｍｉｔａｎ"), "yomitan");
        assert_eq!(convert_hiragana_to_katakana("よみちゃん"), "ヨミチャン");
        assert_eq!(collapse_emphatic_sequences("すっっごーーい", false), "すっごーい");
        assert_eq!(collapse_emphatic_sequences("すっっごーーい", true), "すごい");
        assert_eq!(normalize_combining_characters("ト\u{3099}"), "ド");
        // Yomitan 的描述写的是「㌀ → アパート」，但实际做的是 NFKD，「パ」会拆成「ハ」+ 合成用半浊点。
        // 这里按实际行为断言（整条管线已和 Yomitan 逐行对账一致）。
        assert_eq!(normalize_cjk_compatibility_characters("㌀"), "アハ\u{309a}ート");
        assert_eq!(normalize_radicals("⼀"), "一");
    }

    #[test]
    fn prolonged_sound_marks_follow_the_previous_vowel() {
        assert_eq!(convert_katakana_to_hiragana("ラーメン", false), "らあめん");
        assert_eq!(convert_katakana_to_hiragana("コーヒー", false), "こうひい");
        assert_eq!(convert_katakana_to_hiragana("ラーメン", true), "らーめん");
        // 开头的长音没有前一个字符，保留
        assert_eq!(convert_katakana_to_hiragana("ーア", false), "ーあ");
        // ヵ ヶ 不转
        assert_eq!(convert_katakana_to_hiragana("ヵヶ", false), "ヵヶ");
    }

    #[test]
    fn a_voicing_mark_that_cannot_combine_is_left_in_place() {
        assert_eq!(convert_half_width_kana_to_full_width("ﾅﾞ"), "ナﾞ");
        assert_eq!(convert_half_width_kana_to_full_width("ﾊﾟﾋﾞ"), "パビ");
        assert_eq!(normalize_combining_characters("\u{3099}か"), "\u{3099}か");
        assert_eq!(normalize_combining_characters(""), "");
    }

    #[test]
    fn double_consonants_become_sokuon_even_in_long_runs() {
        assert_eq!(convert_alphabetic_to_kana("kitte"), "きって");
        assert_eq!(convert_alphabetic_to_kana("ttttttttttsu"), "っっっっっっっっっつ");
    }

    #[test]
    fn lookup_text_stops_at_the_first_non_cjk_character() {
        assert_eq!(japanese_chinese_korean_only_prefix("君の名は English"), "君の名は");
        assert_eq!(japanese_chinese_korean_only_prefix("「夜に駆ける」"), "「夜に駆ける」");
        assert_eq!(japanese_chinese_korean_only_prefix("abc"), "");
    }

    #[test]
    fn an_all_emphatic_string_is_left_alone() {
        assert_eq!(collapse_emphatic_sequences("ーっー", true), "ーっー");
        assert_eq!(collapse_emphatic_sequences("", true), "");
    }
}
