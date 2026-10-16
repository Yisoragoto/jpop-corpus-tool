//! 「从这段文字开头查词」的默认做法：扫多长、返回几条、启用的词典怎么排。
//!
//! 两个客户端（Tauri 的查词弹窗、macOS 的歌词面板）查出来的要一样，所以这些默认值放在这里，
//! 原来写在 `jp-app/src/dict.rs` 里。

use anyhow::{Result, bail};

use crate::store::{DictionaryInfo, DictionaryStore};
use crate::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions, FindTermsResult, Translator};

/// 一次查词最多往后看多少个字（Yomitan 的默认扫描长度）。
pub const SCAN_LENGTH: usize = 16;

/// 一次最多返回多少条词条。
pub const MAX_RESULTS: usize = 32;

/// 界面上启用的词典，按界面顺序编号。其余取 Yomitan 的默认值。
pub fn lookup_options(dictionaries: &[DictionaryInfo]) -> FindTermsOptions {
    FindTermsOptions {
        dictionaries: dictionaries
            .iter()
            .filter(|d| d.enabled)
            .enumerate()
            .map(|(index, d)| EnabledDictionary {
                title: d.title.clone(),
                index,
                alias: d.title.clone(),
                parts_of_speech_filter: true,
                use_deinflections: true,
            })
            .collect(),
        deinflect: true,
        remove_non_japanese_characters: true,
        primary_reading: String::new(),
        sort_frequency_dictionary: None,
        sort_frequency_ascending: true,
        use_all_frequency_dictionaries: false,
        max_results: Some(MAX_RESULTS),
    }
}

pub fn parse_mode(mode: Option<&str>) -> Result<FindTermsMode> {
    Ok(match mode.unwrap_or("group") {
        "group" => FindTermsMode::Group,
        "split" => FindTermsMode::Split,
        "term" => FindTermsMode::Term,
        "simple" => FindTermsMode::Simple,
        other => bail!("不认识的查词模式「{other}」"),
    })
}

/// 从 `text` 开头查词：截前 [`SCAN_LENGTH`] 个字，用库里启用的词典按界面顺序查。
pub fn lookup(
    store: &DictionaryStore,
    translator: &Translator,
    text: &str,
    mode: Option<&str>,
) -> Result<FindTermsResult> {
    let mode = parse_mode(mode)?;
    let text: String = text.chars().take(SCAN_LENGTH).collect();
    let options = lookup_options(&store.dictionaries()?);
    translator.find_terms(store, mode, &text, &options)
}

/// 一条释义（Yomitan 的 glossary 项）的纯文本，给不渲染 HTML 的界面（macOS 歌词面板）用。
///
/// glossary 项可以是字符串、`{type:"text"}`、`{type:"image"}`，或者 `{type:"structured-content"}`
/// 里一棵 `{tag, content}` 树。这里只取文字：块级元素（div、li、tr、br…）换行，图片丢掉，
/// 连续空行压成一行。样式、链接、注音的排版都不管——要原样排版的地方走 Tauri 那边的 HTML。
pub fn plain_text(entry: &serde_json::Value) -> String {
    let mut out = String::new();
    collect_text(entry, &mut out);
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

fn collect_text(node: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match node {
        Value::String(s) => out.push_str(s),
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, out)),
        Value::Object(map) => {
            match map.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = map.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                    return;
                }
                Some("image") => return,
                _ => {}
            }
            let tag = map.get("tag").and_then(Value::as_str).unwrap_or("");
            // 注音的读音（<rt>）不放进正文，不然「漢字かんじ」连在一起读不了
            if tag == "rt" || tag == "rp" || tag == "img" {
                return;
            }
            if tag == "br" {
                out.push('\n');
                return;
            }
            let block = matches!(tag, "div" | "li" | "ol" | "ul" | "tr" | "table" | "thead" | "tbody" | "details" | "summary");
            if block {
                out.push('\n');
            }
            if let Some(content) = map.get("content") {
                collect_text(content, out);
            }
            if block {
                out.push('\n');
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: i64, title: &str, enabled: bool) -> DictionaryInfo {
        DictionaryInfo {
            id,
            title: title.into(),
            revision: "r".into(),
            version: 3,
            sequenced: false,
            frequency_mode: None,
            author: None,
            url: None,
            description: None,
            attribution: None,
            source_language: None,
            target_language: None,
            counts: serde_json::Value::Null,
            warnings: Vec::new(),
            has_styles: false,
            source_path: String::new(),
            enabled,
            priority: id,
            imported_at: 0,
        }
    }

    #[test]
    fn disabled_dictionaries_are_left_out_and_the_rest_are_numbered_in_order() {
        let options = lookup_options(&[
            info(1, "明鏡", true),
            info(2, "JMnedict", false),
            info(3, "大辞泉", true),
        ]);
        let got: Vec<(&str, usize)> = options
            .dictionaries
            .iter()
            .map(|d| (d.title.as_str(), d.index))
            .collect();
        assert_eq!(got, [("明鏡", 0), ("大辞泉", 1)]);
        assert!(
            options
                .dictionaries
                .iter()
                .all(|d| d.parts_of_speech_filter && d.use_deinflections)
        );
    }

    #[test]
    fn unknown_modes_are_refused_by_name() {
        assert!(matches!(parse_mode(None).unwrap(), FindTermsMode::Group));
        assert!(parse_mode(Some("nope")).unwrap_err().to_string().contains("nope"));
    }

    #[test]
    fn structured_content_flattens_to_lines_without_readings_or_images() {
        let entry = serde_json::json!({
            "type": "structured-content",
            "content": [
                {"tag": "div", "content": ["夜。", {"tag": "ruby", "content": ["日", {"tag": "rt", "content": "ひ"}]}, "の暮れ"]},
                {"tag": "ul", "content": [{"tag": "li", "content": "一"}, {"tag": "li", "content": "二"}]},
                {"tag": "img", "path": "x.png"}
            ]
        });
        assert_eq!(plain_text(&entry), "夜。日の暮れ\n一\n二");
        assert_eq!(plain_text(&serde_json::json!("ただの文字列")), "ただの文字列");
        assert_eq!(plain_text(&serde_json::json!({"type": "text", "text": "t"})), "t");
        assert_eq!(plain_text(&serde_json::json!({"type": "image", "path": "x"})), "");
    }
}
