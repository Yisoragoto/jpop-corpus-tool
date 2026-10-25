//! 原生查词面板复用既有结构化释义渲染器。
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use crate::{anki::structured::{MediaResolver, format_glossary}, store::DictionaryStore};

pub fn definition_html(store: &DictionaryStore, dictionary: &str, entries: &[Value]) -> Result<String> {
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
    Ok(format!(r#"<!doctype html><html><head><meta name="color-scheme" content="light dark"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src jpdict: data:; style-src 'unsafe-inline'"><style>body{{font:14px -apple-system; margin:0; overflow-wrap:anywhere}} img{{max-width:100%;height:auto}} {styles}</style></head><body>{body}</body></html>"#))
}
