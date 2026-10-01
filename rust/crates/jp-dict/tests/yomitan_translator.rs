//! 查词结果和 Yomitan 自己的期望结果（`test/data/translator-test-results.json`）逐字段对账。
//!
//! 输入、选项预设、测试词典都原样取自 Yomitan 仓库；组合预设的方式照抄
//! `test/utilities/translator.js` 的 `createFindTermsOptions`。
//! 暂未移植、因此跳过的：汉字查询（findKanji）、merge 模式、非日语、文本替换、按词切分。

mod common;

use std::collections::HashSet;

use jp_dict::translator::Translator;
use serde_json::{Value, json};

/// JSON 里 `0` 和 `0.0` 在 serde_json 里不相等，对账前统一成浮点。
fn normalize(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64()),
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), normalize(v))).collect()),
        other => other.clone(),
    }
}

fn short(v: Option<&Value>) -> String {
    let s = v.map_or_else(|| "（没有）".to_owned(), Value::to_string);
    if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s }
}

fn first_difference(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: HashSet<&String> = a.keys().chain(b.keys()).collect();
            let mut keys: Vec<_> = keys.into_iter().collect();
            keys.sort();
            for k in keys {
                let p = format!("{path}.{k}");
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => {
                        if let Some(d) = first_difference(&p, x, y) {
                            return Some(d);
                        }
                    }
                    (x, y) => return Some(format!("{p}: Yomitan {} / Rust {}", short(x), short(y))),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            for i in 0..a.len().max(b.len()) {
                let p = format!("{path}[{i}]");
                match (a.get(i), b.get(i)) {
                    (Some(x), Some(y)) => {
                        if let Some(d) = first_difference(&p, x, y) {
                            return Some(d);
                        }
                    }
                    (x, y) => {
                        return Some(format!("{p}: 长度 Yomitan {} / Rust {}，多出 {} / {}", a.len(), b.len(), short(x), short(y)));
                    }
                }
            }
            None
        }
        _ if want == got => None,
        _ => Some(format!("{path}: Yomitan {} / Rust {}", short(Some(want)), short(Some(got)))),
    }
}

#[test]
fn find_terms_matches_yomitans_expected_results() {
    let store = common::store();
    let inputs = common::inputs();
    let results: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/yomitan-translator/translator-test-results.json")).unwrap();
    let translator = Translator::new();

    let mut compared = Vec::new();
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    for (i, test) in inputs["tests"].as_array().unwrap().iter().enumerate() {
        let name = test["name"].as_str().unwrap();
        let expected = &results[i];
        assert_eq!(expected["name"], test["name"], "输入和结果文件的顺序对不上");
        let case = match common::case(&inputs, test) {
            Ok(case) => case,
            Err(reason) => {
                skipped.push(reason);
                continue;
            }
        };
        let (mode, options) = (case.mode, case.options);

        let got = translator.find_terms(&store, mode, test["text"].as_str().unwrap(), &options).unwrap();
        let got = normalize(&serde_json::to_value(&got).unwrap());
        let want = normalize(&json!({
            "dictionaryEntries": expected["dictionaryEntries"],
            "originalTextLength": expected["originalTextLength"],
        }));
        compared.push(name.to_owned());
        if let Some(difference) = first_difference("", &want, &got) {
            failures.push(format!("[{name}] {difference}"));
        }
    }

    eprintln!("对账 {} 条；跳过 {} 条：{}", compared.len(), skipped.len(), skipped.join("、"));
    assert!(
        failures.is_empty(),
        "{} / {} 条和 Yomitan 不一致：\n{}",
        failures.len(),
        compared.len(),
        failures.join("\n")
    );
    assert!(compared.len() >= 35, "只对账了 {} 条", compared.len());
}
