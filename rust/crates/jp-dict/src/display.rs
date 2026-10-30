//! 原生查词面板复用既有结构化释义渲染器。
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use crate::{anki::structured::{MediaResolver, format_glossary}, store::DictionaryStore};

/// 有的词典（比如明鏡日汉双解）在纯文本释义里写了 `<br>`。Anki 卡片照 Yomitan 原样输出（不动），
/// 查词面板和 Windows 版一样把它当换行；只认 `<br>` 这一个标签，其余照旧按文本转义
fn br_to_newline(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find("<br") {
        let start = at + found;
        let after = &lower[start + 3..];
        let tail = after.trim_start_matches(' ');
        let tail = tail.strip_prefix('/').unwrap_or(tail);
        if tail.starts_with('>') {
            out.push_str(&text[at..start]);
            out.push('\n');
            at = text.len() - tail.len() + 1;
        } else {
            out.push_str(&text[at..start + 3]);
            at = start + 3;
        }
    }
    out.push_str(&text[at..]);
    out
}

fn plain_breaks(entry: &Value) -> Value {
    match entry {
        Value::String(text) => Value::String(br_to_newline(text)),
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("text") => {
            let mut map = map.clone();
            if let Some(Value::String(text)) = map.get("text") {
                let fixed = br_to_newline(text);
                map.insert("text".into(), Value::String(fixed));
            }
            Value::Object(map)
        }
        other => other.clone(),
    }
}

pub fn definition_html(store: &DictionaryStore, dictionary: &str, entries: &[Value]) -> Result<String> {
    let entries: Vec<Value> = entries.iter().map(plain_breaks).collect();
    let entries = entries.as_slice();
    let empty = HashMap::new();
    let mut media = MediaResolver { files: &empty, missing: Vec::new() };
    for entry in entries { format_glossary(dictionary, entry, &mut media); }
    let files = media.missing.iter().map(|key| {
        let encoded: String = serde_json::to_vec(key).unwrap().iter().map(|b| format!("{b:02x}")).collect();
        (key.clone(), format!("jpdict://media/{encoded}"))
    }).collect();
    let mut media = MediaResolver { files: &files, missing: Vec::new() };
    let body: String = entries.iter().map(|entry| format!("<div>{}</div>", format_glossary(dictionary, entry, &mut media))).collect();
    let styles = match store.dictionaries()?.into_iter().find(|d| d.title == dictionary && d.enabled && d.has_styles) {
        Some(d) => store.styles(d.id)?, None => String::new(),
    };
    Ok(format!(r#"<!doctype html><html><head><meta name="color-scheme" content="light dark"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src jpdict: data:; style-src 'unsafe-inline'"><style>html,body{{background:transparent}} body{{font:14px -apple-system; margin:0; overflow-wrap:anywhere; color:CanvasText}} img{{max-width:100%;height:auto}} {styles}</style></head><body>{body}</body></html>"#))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn br_tags_in_plain_text_become_line_breaks() {
        assert_eq!(br_to_newline("一<br>二<BR/>三<br />四"), "一\n二\n三\n四");
        assert_eq!(br_to_newline("a <bra> b"), "a <bra> b", "不是 br 标签的不动");
        assert_eq!(br_to_newline("末尾<br"), "末尾<br");
        let text = plain_breaks(&serde_json::json!({"type": "text", "text": "x<br>y"}));
        assert_eq!(text["text"], "x\ny");
    }
}
