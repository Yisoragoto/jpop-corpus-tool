//! 日语词性（UniDic / Sudachi 体系）→ UPOS。
//!
//! 这张表和 Python 侧的 `tokenizer/pos_map.py` 必须逐条一致——
//! 它是迁移后的分词器和库里现有 58,628 个 token 之间唯一的桥。
//! `tests/baseline.rs` 会拿 `benchmark/baseline.json` 校验，防止两边漂移。
//!
//! 推导过程和实测精度见 `docs/tokenizer.md`：
//! 前 2 级 91.40%，『是否实词』二分类 97.70%。

/// 认不出来时的兜底。GiNZA 对无法归类的 token 也用 X。
pub const DEFAULT_UPOS: &str = "X";

/// 实词类。Anki 导出的词类勾选、词频统计的筛选都只看这一组，
/// 所以「是否属于这一组」比「具体是哪个 UPOS」重要得多。
pub const CONTENT_POS: [&str; 5] = ["NOUN", "PROPN", "VERB", "ADJ", "ADV"];

/// 统计时一律排除的类别，和 `gui.py` 的 `StatsWorker` 规则一致。
pub const IGNORED_POS: [&str; 4] = ["PUNCT", "SYM", "SPACE", "X"];

/// 日语词性 → UPOS。
///
/// `pos` 是 Sudachi 的词性数组（六元组）。只看前两级：第 3 级只多 0.27 个
/// 百分点，不值得为它多维护 10 个条目。
pub fn to_upos(pos: &[String]) -> &'static str {
    let level0 = pos.first().map(String::as_str).unwrap_or("");
    let level1 = pos.get(1).map(String::as_str).unwrap_or("");

    match (level0, level1) {
        // ── 名词类 ──
        ("名詞", "普通名詞") => "NOUN",
        ("名詞", "固有名詞") => "PROPN",
        ("名詞", "数詞") => "NUM",
        ("名詞", "助動詞語幹") => "AUX",
        ("代名詞", _) => "PRON",
        // ── 用言 ──
        ("動詞", "一般") | ("動詞", "非自立可能") => "VERB",
        ("形容詞", "一般") | ("形容詞", "非自立可能") => "ADJ",
        ("形状詞", "一般") | ("形状詞", "タリ") => "ADJ",
        ("形状詞", "助動詞語幹") => "AUX",
        ("助動詞", _) => "AUX",
        // ── 副词 / 连体词 / 接续词 / 感叹词 ──
        ("副詞", _) => "ADV",
        ("連体詞", _) => "DET",
        ("接続詞", _) => "CCONJ",
        // フィラー 实测样本仅 5 个、众数是噪声，这里按语言学判定归 INTJ
        ("感動詞", _) => "INTJ",
        // ── 助词 ──
        ("助詞", "格助詞") | ("助詞", "係助詞") | ("助詞", "副助詞") => "ADP",
        ("助詞", "接続助詞") | ("助詞", "準体助詞") => "SCONJ",
        ("助詞", "終助詞") => "PART",
        // ── 词缀 ──
        ("接頭辞", _) => "NOUN",
        ("接尾辞", "名詞的") => "NOUN",
        ("接尾辞", "動詞的") | ("接尾辞", "形状詞的") => "PART",
        ("接尾辞", "形容詞的") => "AUX",
        // ── 符号 / 空白 ──
        ("補助記号", "読点") | ("補助記号", "句点") => "PUNCT",
        ("補助記号", "括弧開") | ("補助記号", "括弧閉") => "PUNCT",
        ("補助記号", _) => "SYM",
        // 記号,一般 实测样本仅 2 个、众数是噪声，按语言学判定归 SYM
        ("記号", _) | ("空白", _) => "SYM",
        // ── 大分类兜底：新词典版本引入新的中分类时不至于整个失效 ──
        ("名詞", _) => "NOUN",
        ("動詞", _) => "VERB",
        ("形容詞", _) | ("形状詞", _) => "ADJ",
        ("助詞", _) => "ADP",
        ("接尾辞", _) => "PART",
        _ => DEFAULT_UPOS,
    }
}

/// 是不是实词。Anki 导出和词频筛选用它。
pub fn is_content_word(upos: &str) -> bool {
    CONTENT_POS.contains(&upos)
}

/// 统计时是否应该排除。
pub fn is_ignored(upos: &str) -> bool {
    IGNORED_POS.contains(&upos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn core_mappings() {
        assert_eq!(to_upos(&pos(&["名詞", "普通名詞", "一般"])), "NOUN");
        assert_eq!(to_upos(&pos(&["名詞", "固有名詞", "人名"])), "PROPN");
        assert_eq!(to_upos(&pos(&["動詞", "一般"])), "VERB");
        assert_eq!(to_upos(&pos(&["形容詞", "一般"])), "ADJ");
        assert_eq!(to_upos(&pos(&["助動詞", "*"])), "AUX");
    }

    #[test]
    fn particles_split_by_subtype() {
        assert_eq!(to_upos(&pos(&["助詞", "格助詞"])), "ADP");
        assert_eq!(to_upos(&pos(&["助詞", "接続助詞"])), "SCONJ");
        assert_eq!(to_upos(&pos(&["助詞", "終助詞"])), "PART");
    }

    #[test]
    fn curated_overrides() {
        assert_eq!(to_upos(&pos(&["感動詞", "フィラー"])), "INTJ");
        assert_eq!(to_upos(&pos(&["記号", "一般"])), "SYM");
    }

    #[test]
    fn unknown_subtype_falls_back_to_top_level() {
        assert_eq!(to_upos(&pos(&["名詞", "全新的分类"])), "NOUN");
        assert_eq!(to_upos(&pos(&["動詞", "未来的分类"])), "VERB");
    }

    #[test]
    fn empty_and_unknown() {
        assert_eq!(to_upos(&[]), DEFAULT_UPOS);
        assert_eq!(to_upos(&pos(&["外星词性"])), DEFAULT_UPOS);
    }

    #[test]
    fn content_and_ignored_do_not_overlap() {
        for tag in CONTENT_POS {
            assert!(!is_ignored(tag), "{tag}");
        }
        for tag in IGNORED_POS {
            assert!(!is_content_word(tag), "{tag}");
        }
    }
}
