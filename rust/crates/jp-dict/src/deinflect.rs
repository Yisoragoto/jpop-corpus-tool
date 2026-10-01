//! 算法活用还原管线：Yomitan `translator.js` 的 `_getAlgorithmDeinflections`。
//!
//! 逐字缩短（从整段到一个字）→ 每段做预处理变体 → 每个变体做活用还原。
//! 移植自 Yomitan（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）。
//!
//! 和 JS 版有意不同的一处：JS 按 UTF-16 码元缩短，会把 BMP 以外的汉字（如「𠮷」）劈成半个代理对
//! 去查词典——那样的字符串必然查不到东西。这里按字符缩短。

use serde::Serialize;

use crate::transformer::LanguageTransformer;
use crate::variants::{JAPANESE_PREPROCESSORS, RuleChain, TextCache, text_variants};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InflectionSource {
    /// 活用规则推出来的
    Algorithm,
    /// 词典条目自带的「某词的变形」
    Dictionary,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InflectionRuleChainCandidate {
    pub source: InflectionSource,
    pub inflection_rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Deinflection {
    /// 原文从头截到当前长度（JS `originalText`）
    pub original_text: String,
    /// 预处理后的变体（JS `transformedText`）
    pub transformed_text: String,
    /// 还原后的文字，拿它去查词典（JS `deinflectedText`）
    pub deinflected_text: String,
    pub conditions: u32,
    pub text_processor_rule_chain_candidates: Vec<RuleChain>,
    pub inflection_rule_chain_candidates: Vec<InflectionRuleChainCandidate>,
}

impl Deinflection {
    /// 和 `tools/dump-yomitan-text-processing.mjs` 同格式的一行，对账用。
    #[doc(hidden)]
    pub fn parity_line(&self) -> String {
        let rules = self
            .inflection_rule_chain_candidates
            .first()
            .map(|c| c.inflection_rules.join(","))
            .unwrap_or_default();
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.original_text,
            self.transformed_text,
            self.deinflected_text,
            self.conditions,
            serde_json::to_string(&self.text_processor_rule_chain_candidates).unwrap_or_default(),
            rules
        )
    }
}

/// 去掉最后一个字符。
fn next_substring(text: &str) -> &str {
    match text.char_indices().next_back() {
        Some((i, _)) => &text[..i],
        None => "",
    }
}

/// 对应 JS `_getAlgorithmDeinflections`（`searchResolution = 'letter'`，无 textReplacements）。
/// 日语没有后处理器，后处理变体只有自身，所以预处理规则链原样带过去。
pub fn algorithm_deinflections(transformer: &LanguageTransformer, text: &str) -> Vec<Deinflection> {
    let mut out = Vec::new();
    let mut cache = TextCache::default();
    let mut raw = text;
    while !raw.is_empty() {
        for variant in text_variants(raw, JAPANESE_PREPROCESSORS, &mut cache) {
            for transformed in transformer.transform(&variant.text) {
                let inflection_rules =
                    transformer.trace_ids(&transformed.trace).into_iter().map(str::to_owned).collect();
                out.push(Deinflection {
                    original_text: raw.to_owned(),
                    transformed_text: variant.text.clone(),
                    deinflected_text: transformed.text,
                    conditions: transformed.conditions,
                    text_processor_rule_chain_candidates: variant.candidates.clone(),
                    inflection_rule_chain_candidates: vec![InflectionRuleChainCandidate {
                        source: InflectionSource::Algorithm,
                        inflection_rules,
                    }],
                });
            }
        }
        raw = next_substring(raw);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortening_goes_down_to_a_single_character() {
        let lt = LanguageTransformer::japanese();
        let originals: Vec<String> = {
            let mut seen = Vec::new();
            for d in algorithm_deinflections(&lt, "打ち込む") {
                if seen.last() != Some(&d.original_text) {
                    seen.push(d.original_text);
                }
            }
            seen
        };
        assert_eq!(originals, ["打ち込む", "打ち込", "打ち", "打"]);
    }

    #[test]
    fn astral_kanji_are_never_split_in_half() {
        let lt = LanguageTransformer::japanese();
        for d in algorithm_deinflections(&lt, "𠮷野家") {
            assert!(d.original_text.starts_with('𠮷') || d.original_text.is_empty());
        }
    }
}
