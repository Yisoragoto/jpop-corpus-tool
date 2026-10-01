//! 在一行分好词的歌词里找某个词的出现（含活用形）。制卡时把同一首歌里含这个词的其他句子一起放上卡。
//!
//! 判断方法和查词一致：从每个词的起点做活用还原（`algorithm_deinflections`），还原结果等于词头，
//! 活用条件和词条的词性相容（Yomitan 的 `partsOfSpeechFilter`）。另加一条查词没有的限制：
//! 匹配的末尾必须落在词的边界上——「夜空」分成一个词时，里面的「夜」不算「夜」这个词出现了。

use crate::deinflect::algorithm_deinflections;
use crate::transformer::{LanguageTransformer, conditions_match};

/// 和查词一样，从起点最多往后看这么多字
const SCAN_LENGTH: usize = 16;

/// 一处出现：覆盖第 `start` 个到第 `end` 个（不含）词
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub start: usize,
    pub end: usize,
}

/// `targets`：词头（点的是假名写法时也算上读音）。`conditions`：词条词性对应的条件位
/// （`LanguageTransformer::condition_flags_from_parts_of_speech`），没有词性规则时为 0——
/// 这时和 Yomitan 一样只认不活用的原形。
pub fn find_occurrences<S: AsRef<str>>(
    transformer: &LanguageTransformer,
    surfaces: &[S],
    targets: &[&str],
    conditions: u32,
) -> Vec<Occurrence> {
    let lengths: Vec<usize> = surfaces.iter().map(|s| s.as_ref().chars().count()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < surfaces.len() {
        if lengths[i] == 0 {
            i += 1;
            continue;
        }
        let text: String = surfaces[i..].iter().flat_map(|s| s.as_ref().chars()).take(SCAN_LENGTH).collect();
        let mut best: Option<usize> = None;
        for d in algorithm_deinflections(transformer, &text) {
            if !targets.contains(&d.deinflected_text.as_str()) || !conditions_match(d.conditions, conditions) {
                continue;
            }
            let length = d.original_text.chars().count();
            // 末尾要正好落在某个词的结尾
            let mut covered = 0;
            for (j, l) in lengths[i..].iter().enumerate() {
                covered += l;
                if covered >= length {
                    if covered == length {
                        best = Some(best.map_or(i + j + 1, |b| b.max(i + j + 1)));
                    }
                    break;
                }
            }
        }
        match best {
            Some(end) => {
                out.push(Occurrence { start: i, end });
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(surfaces: &[&str], target: &str, pos: &[&str]) -> Vec<(usize, usize)> {
        let lt = LanguageTransformer::japanese();
        let flags = lt.condition_flags_from_parts_of_speech(pos);
        find_occurrences(&lt, surfaces, &[target], flags).into_iter().map(|o| (o.start, o.end)).collect()
    }

    #[test]
    fn inflected_forms_are_found_and_cover_the_whole_inflection() {
        assert_eq!(find(&["甘え", "て", "もう", "一", "歩"], "甘える", &["v1"]), [(0, 2)]);
        assert_eq!(find(&["甘え", "てる", "様"], "甘える", &["v1"]), [(0, 2)]);
        assert_eq!(find(&["打ち込ん", "で", "い", "ませ", "ん", "でし", "た"], "打ち込む", &["v5"]), [(0, 7)]);
    }

    #[test]
    fn a_word_inside_a_longer_token_does_not_count() {
        assert!(find(&["今夜", "は"], "夜", &[]).is_empty());
        assert_eq!(find(&["夜", "が", "明ける"], "夜", &[]), [(0, 1)]);
        assert_eq!(find(&["夜", "と", "夜"], "夜", &[]), [(0, 1), (2, 3)]);
    }

    #[test]
    fn a_noun_does_not_match_verb_inflections_that_happen_to_deinflect_to_it() {
        // 没有词性规则的词条只认原形：「駆けて」还原成「駆ける」，但词条没有动词规则就不算
        assert!(find(&["駆け", "て"], "駆ける", &[]).is_empty());
        assert_eq!(find(&["駆け", "て"], "駆ける", &["v1"]), [(0, 2)]);
    }
}
