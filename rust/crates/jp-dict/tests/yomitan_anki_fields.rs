//! 卡片字段和 Yomitan 自己的制卡期望结果（`test/data/anki-note-builder-test-results.json`）逐字对账。
//!
//! Yomitan 的测试（`test/utilities/anki.js` 的 `getTemplateRenderResults`）：每个查词结果按标准标记
//! 各渲染一个字段；例句是 `cloze-prefix{原文}cloze-suffix`、标题 `title`、测试词典带自己的 styles.css、
//! 不提供媒体文件。这里照同样的条件渲染，比较已移植的标记。

mod common;

use std::collections::HashMap;

use jp_dict::anki::{EntryKind, MediaResolver, NoteContext, NoteRenderer};
use jp_dict::translator::{FindTermsMode, Translator};
use serde_json::Value;

const MARKERS: &[&str] = &[
    "expression",
    "reading",
    "furigana",
    "furigana-plain",
    "glossary",
    "glossary-brief",
    "glossary-no-dictionary",
    "glossary-first",
    "glossary-first-brief",
    "glossary-first-no-dictionary",
    "cloze-prefix",
    "cloze-body",
    "cloze-body-kana",
    "cloze-suffix",
    "sentence",
    "conjugation",
    "dictionary",
    "dictionary-alias",
    "document-title",
    "frequencies",
    "frequency-harmonic-rank",
    "frequency-harmonic-occurrence",
    "frequency-average-rank",
    "frequency-average-occurrence",
    "pitch-accent-positions",
    "pitch-accent-categories",
];

fn short(s: &str) -> String {
    if s.chars().count() > 300 { format!("{}…", s.chars().take(300).collect::<String>()) } else { s.to_owned() }
}

#[test]
fn note_fields_match_yomitans_expected_results() {
    let store = common::store();
    let inputs = common::inputs();
    let expected: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/yomitan-translator/anki-note-builder-test-results.json")).unwrap();
    let dictionary = store.dictionaries().unwrap().into_iter().find(|d| d.title == common::TITLE).unwrap();
    let styles = HashMap::from([(common::TITLE.to_owned(), store.styles(dictionary.id).unwrap())]);
    assert!(!styles[common::TITLE].is_empty(), "测试词典应当带 styles.css");
    let translator = Translator::new();
    let no_media = HashMap::new();

    let (mut entries, mut fields) = (0, 0);
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    for (i, test) in inputs["tests"].as_array().unwrap().iter().enumerate() {
        let case = match common::case(&inputs, test) {
            Ok(case) => case,
            Err(reason) => {
                skipped.push(reason);
                continue;
            }
        };
        assert_eq!(expected[i]["name"], test["name"], "输入和结果文件的顺序对不上");
        let kind = match case.mode {
            FindTermsMode::Simple => {
                assert!(expected[i]["results"].is_null());
                skipped.push(format!("{}（simple 模式不制卡）", case.name));
                continue;
            }
            FindTermsMode::Split => EntryKind::Term,
            FindTermsMode::Group | FindTermsMode::Term => EntryKind::TermGrouped,
        };
        let result = translator.find_terms(&store, case.mode, &case.text, &case.options).unwrap();
        let want = expected[i]["results"].as_array().unwrap();
        assert_eq!(want.len(), result.dictionary_entries.len(), "[{}] 词条数", case.name);

        for (j, entry) in result.dictionary_entries.iter().enumerate() {
            let source = entry.headwords.first().and_then(|h| h.sources.first()).map(|s| s.original_text.as_str()).unwrap_or("");
            let sentence = format!("cloze-prefix{source}cloze-suffix");
            let ctx = NoteContext {
                sentence: &sentence,
                offset: "cloze-prefix".chars().count(),
                document_title: "title",
                dictionary_styles: &styles,
            };
            let renderer = NoteRenderer::new(entry, kind, &ctx);
            entries += 1;
            for marker in MARKERS {
                let mut media = MediaResolver { files: &no_media, missing: Vec::new() };
                let got = renderer.render(marker, &mut media).expect("已移植的标记");
                let want = want[j][*marker].as_str().unwrap_or_else(|| panic!("[{}] 期望结果里没有 {marker}", case.name));
                fields += 1;
                if got != want {
                    failures.push(format!(
                        "[{} #{j}] {{{marker}}}\n  Yomitan: {}\n  Rust:    {}",
                        case.name,
                        short(want),
                        short(&got)
                    ));
                }
            }
        }
    }
    eprintln!("对账 {entries} 个词条、{fields} 个字段；跳过：{}", skipped.join("、"));
    assert!(failures.is_empty(), "{} / {fields} 个字段和 Yomitan 不一致：\n{}", failures.len(), failures.join("\n"));
    assert!(entries >= 70, "只对账了 {entries} 个词条");
}

#[test]
fn a_single_dictionary_glossary_only_keeps_that_dictionary() {
    let store = common::store();
    let inputs = common::inputs();
    let test = inputs["tests"].as_array().unwrap().iter().find(|t| t["name"] == "Search using different modes - group").unwrap();
    let case = common::case(&inputs, test).unwrap();
    let result = Translator::new().find_terms(&store, case.mode, &case.text, &case.options).unwrap();
    let styles = HashMap::new();
    let ctx = NoteContext { sentence: "", offset: 0, document_title: "", dictionary_styles: &styles };
    let files = HashMap::new();
    let mut media = MediaResolver { files: &files, missing: Vec::new() };
    let renderer = NoteRenderer::new(&result.dictionary_entries[0], EntryKind::TermGrouped, &ctx);
    let all = renderer.render("glossary", &mut media).unwrap();
    assert_eq!(renderer.single_glossary(common::TITLE, false, false, &mut media), all);
    assert_eq!(
        renderer.single_glossary("别的词典", false, false, &mut media),
        r#"<div style="text-align: left;" class="yomitan-glossary"><ol></ol></div>"#
    );
}
