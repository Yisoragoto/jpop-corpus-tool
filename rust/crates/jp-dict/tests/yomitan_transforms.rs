//! Yomitan 自带的活用测试，原样跑一遍。
//!
//! 用例来自 Yomitan `test/language/japanese-transforms.test.js`（GPL-3.0-or-later），
//! 判定逻辑逐行照抄 `test/fixtures/language-transformer-test.js` 的 `hasTermReasons`。

use jp_dict::transformer::{LanguageTransformer, conditions_match};
use serde::Deserialize;

#[derive(Deserialize)]
struct Category {
    category: String,
    valid: bool,
    tests: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    term: String,
    source: String,
    rule: Option<String>,
    reasons: Option<Vec<String>>,
}

fn has_term_reasons(
    lt: &LanguageTransformer,
    source: &str,
    expected_term: &str,
    expected_condition: Option<&str>,
    expected_reasons: Option<&[String]>,
) -> bool {
    for result in lt.transform(source) {
        if result.text != expected_term {
            continue;
        }
        if let Some(name) = expected_condition {
            let expected = lt.condition_flags_from_condition_type(name);
            if !conditions_match(result.conditions, expected) {
                continue;
            }
        }
        if let Some(reasons) = expected_reasons {
            if result.trace.len() != reasons.len() {
                continue;
            }
            if lt.trace_ids(&result.trace) != reasons.iter().map(String::as_str).collect::<Vec<_>>() {
                continue;
            }
        }
        return true;
    }
    false
}

#[test]
fn every_yomitan_japanese_transform_case_passes() {
    let categories: Vec<Category> =
        serde_json::from_str(include_str!("fixtures/japanese-transforms-tests.json")).unwrap();
    let lt = LanguageTransformer::japanese();

    let mut total = 0;
    let mut failures = Vec::new();
    for category in &categories {
        for case in &category.tests {
            total += 1;
            let has = has_term_reasons(
                &lt,
                &case.source,
                &case.term,
                case.rule.as_deref(),
                case.reasons.as_deref(),
            );
            if has != category.valid {
                failures.push(format!(
                    "[{}] {} {} {:?} rule={:?} reasons={:?}",
                    category.category,
                    case.source,
                    if category.valid { "应有" } else { "不应有" },
                    case.term,
                    case.rule,
                    case.reasons
                ));
            }
        }
    }
    // 防止 fixture 被截断后「全部通过」
    assert_eq!(total, 1406, "Yomitan 用例数变了，重新导出过 fixture 吗？");
    assert!(
        failures.is_empty(),
        "{} / {total} 条不通过：\n{}",
        failures.len(),
        failures.iter().take(30).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn condition_flags_are_numbered_exactly_like_yomitan() {
    let expected: std::collections::BTreeMap<String, u32> =
        serde_json::from_str(include_str!("fixtures/japanese-condition-flags.json")).unwrap();
    let lt = LanguageTransformer::japanese();
    for (name, flags) in &expected {
        assert_eq!(lt.condition_flags_from_condition_type(name), *flags, "条件 {name}");
    }
    assert_eq!(expected.len(), 22);
}
