//! 文本变体：Yomitan `translator.js` 的 `_getTextVariants` / `_getProcessedTexts`，
//! 以及日语的预处理器列表（`language-descriptors.js` 里 `ja.textPreprocessors`，顺序有意义）。
//!
//! 移植自 Yomitan（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）。

use std::collections::HashMap;

use crate::text::{self, TABLES};

/// 一条变体经过了哪些预处理器（按应用顺序）。
pub type RuleChain = Vec<&'static str>;

pub struct TextProcessor {
    pub id: &'static str,
    pub process: fn(&str) -> Vec<String>,
}

fn half_width(s: &str) -> Vec<String> {
    vec![s.to_owned(), text::convert_half_width_kana_to_full_width(s)]
}
fn alphabetic_to_hiragana(s: &str) -> Vec<String> {
    vec![s.to_owned(), text::convert_alphabetic_to_kana(s)]
}
fn combining(s: &str) -> Vec<String> {
    vec![s.to_owned(), text::normalize_combining_characters(s)]
}
fn cjk_compatibility(s: &str) -> Vec<String> {
    vec![s.to_owned(), text::normalize_cjk_compatibility_characters(s)]
}
fn radicals(s: &str) -> Vec<String> {
    vec![s.to_owned(), text::normalize_radicals(s)]
}
fn alphanumeric_width(s: &str) -> Vec<String> {
    vec![
        s.to_owned(),
        text::convert_full_width_alphanumeric_to_normal(s),
        text::convert_alphanumeric_to_full_width(s),
    ]
}
fn hiragana_katakana(s: &str) -> Vec<String> {
    vec![
        s.to_owned(),
        text::convert_hiragana_to_katakana(s),
        text::convert_katakana_to_hiragana(s, false),
    ]
}
fn emphatic(s: &str) -> Vec<String> {
    vec![
        s.to_owned(),
        text::collapse_emphatic_sequences(s, false),
        text::collapse_emphatic_sequences(s, true),
    ]
}
/// 旧字体 → 新字体。Yomitan 用的是 npm 包 `kanji-processor` 的异体字表，还没移植，
/// 暂时不产生新变体（对账基准里同样把它换成了恒等函数）。保留这一项是为了处理器 id 顺序和 Yomitan 一致。
fn standardize_kanji(s: &str) -> Vec<String> {
    vec![s.to_owned(), s.to_owned()]
}

pub static JAPANESE_PREPROCESSORS: &[TextProcessor] = &[
    TextProcessor { id: "convertHalfWidthCharacters", process: half_width },
    TextProcessor { id: "alphabeticToHiragana", process: alphabetic_to_hiragana },
    TextProcessor { id: "normalizeCombiningCharacters", process: combining },
    TextProcessor { id: "normalizeCJKCompatibilityCharacters", process: cjk_compatibility },
    TextProcessor { id: "normalizeRadicalCharacters", process: radicals },
    TextProcessor { id: "alphanumericWidthVariants", process: alphanumeric_width },
    TextProcessor { id: "convertHiraganaToKatakana", process: hiragana_katakana },
    TextProcessor { id: "collapseEmphaticSequences", process: emphatic },
    TextProcessor { id: "standardizeKanji", process: standardize_kanji },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub text: String,
    pub candidates: Vec<RuleChain>,
}

/// 同一次查词里逐字缩短时，同一段文字会被同一个处理器反复处理，缓存下来。
#[derive(Default)]
pub struct TextCache {
    map: HashMap<String, HashMap<&'static str, Vec<String>>>,
}

impl TextCache {
    fn processed(&mut self, text: &str, processor: &TextProcessor) -> Vec<String> {
        if let Some(results) = self.map.get(text).and_then(|m| m.get(processor.id)) {
            return results.clone();
        }
        let mut results = (processor.process)(text);
        results.truncate(TABLES.max_process_variants);
        self.map.entry(text.to_owned()).or_default().insert(processor.id, results.clone());
        results
    }
}

/// JS `_getTextVariants`（不含 textReplacements）。返回顺序等于 JS `Map` 的插入顺序。
///
/// 一个容易看漏的细节：处理结果等于输入本身、而这个字符串已经被前面的变体生成过时，
/// JS 保留已有的规则链、丢掉当前的——照做。
pub fn text_variants(text: &str, processors: &[TextProcessor], cache: &mut TextCache) -> Vec<Variant> {
    let mut variants = vec![Variant { text: text.to_owned(), candidates: vec![Vec::new()] }];
    for processor in processors {
        let mut next: Vec<Variant> = Vec::with_capacity(variants.len());
        let mut index: HashMap<String, usize> = HashMap::new();
        for variant in &variants {
            for processed in cache.processed(&variant.text, processor) {
                let existing = index.get(&processed).copied();
                if processed == variant.text {
                    if existing.is_none() {
                        index.insert(processed.clone(), next.len());
                        next.push(Variant { text: processed, candidates: variant.candidates.clone() });
                    }
                    continue;
                }
                let extended = variant.candidates.iter().map(|chain| {
                    let mut chain = chain.clone();
                    chain.push(processor.id);
                    chain
                });
                match existing {
                    Some(i) => next[i].candidates.extend(extended),
                    None => {
                        index.insert(processed.clone(), next.len());
                        next.push(Variant { text: processed, candidates: extended.collect() });
                    }
                }
            }
        }
        variants = next;
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessor_order_matches_yomitan() {
        let ids: Vec<&str> = JAPANESE_PREPROCESSORS.iter().map(|p| p.id).collect();
        assert_eq!(ids, TABLES.preprocessor_ids);
    }

    #[test]
    fn a_plain_kanji_word_has_only_itself_as_variant() {
        let v = text_variants("漢字", JAPANESE_PREPROCESSORS, &mut TextCache::default());
        assert_eq!(v, vec![Variant { text: "漢字".into(), candidates: vec![vec![]] }]);
    }

    #[test]
    fn katakana_gets_a_hiragana_variant_tagged_with_the_processor() {
        let v = text_variants("ウツ", JAPANESE_PREPROCESSORS, &mut TextCache::default());
        let hira = v.iter().find(|x| x.text == "うつ").expect("应有平假名变体");
        assert_eq!(hira.candidates, vec![vec!["convertHiraganaToKatakana"]]);
    }
}
