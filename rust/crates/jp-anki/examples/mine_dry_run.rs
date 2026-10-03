//! 制卡空跑：在真实词典库上查一个词，按 Lapis 字段渲染出来看看，**不往 Anki 里加卡**。
//!
//! 对 Anki 只做只读查询（笔记类型的字段、canAddNotes 查重、findNotes），用来确认字段对得上、查重判断对。
//!
//! ```text
//! cargo run -p jp-anki --release --example mine_dry_run -- <dictionaries.db> <歌词行> <查词起点(字)> [首选词典]
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Context;
use jp_anki::mine::{LAPIS, LapisCardKind, MineOptions, MineSentence, MineSource, existing_notes, lapis_fields};
use jp_dict::store::DictionaryStore;
use jp_dict::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions, Translator};
use jp_scraper::UreqTransport;
use jp_scraper::http::HttpClient;
use serde_json::json;

fn main() -> anyhow::Result<()> {
    const USAGE: &str = "用法: mine_dry_run <dictionaries.db> <歌词行> <查词起点(字)> [首选词典]";
    let mut args = std::env::args().skip(1);
    let db = args.next().context(USAGE)?;
    let line = args.next().context(USAGE)?;
    let offset: usize = args.next().context(USAGE)?.parse()?;
    let main_dictionary = args.next();

    let store = DictionaryStore::open(Path::new(&db))?;
    let dictionaries = store.dictionaries()?;
    let options = FindTermsOptions {
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
        max_results: Some(32),
    };
    let lookup_text: String = line.chars().skip(offset).filter(|c| !c.is_whitespace()).take(16).collect();
    let result = Translator::new().find_terms(&store, FindTermsMode::Group, &lookup_text, &options)?;
    let entry = result.dictionary_entries.first().context("没查到")?;
    println!("查「{lookup_text}」→ {}【{}】，释义 {} 条", entry.headwords[0].term, entry.headwords[0].reading, entry.definitions.len());

    let mut styles = HashMap::new();
    for d in dictionaries.iter().filter(|d| d.enabled && d.has_styles) {
        styles.insert(d.title.clone(), store.styles(d.id)?);
    }
    let mine_options = MineOptions {
        deck: "Lapis".into(),
        artist_subdeck: false,
        model: LAPIS.into(),
        main_dictionary,
        kind: LapisCardKind::WordAndSentence,
        tags: vec!["jpop-corpus".into()],
    };
    let sentence = MineSentence { text: &line, offset, utterance_id: 0, time_sec: Some(60.0), end_sec: None };
    let source = MineSource { artist: "サカナクション", title: "ナイロンの糸", album: "", audio_path: "", cover_path: "" };
    let started = Instant::now();
    let (fields, missing) = lapis_fields(entry, Some(&sentence), &[], Some(&source), &mine_options, &styles, &HashMap::new());
    println!("渲染 {:.1?}，释义里要存进 Anki 的词典图片 {} 张", started.elapsed(), missing.len());
    // FIELDS_JSON=路径：把字段存成 JSON，给模板预览用
    if let Some(path) = std::env::var_os("FIELDS_JSON") {
        let map: serde_json::Map<String, serde_json::Value> =
            fields.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
        std::fs::write(path, serde_json::to_vec_pretty(&map)?)?;
    }
    for (name, value) in &fields {
        let preview: String = value.chars().take(160).collect();
        println!("  {name:22} {:7} 字  {preview}", value.chars().count());
    }

    let anki = jp_anki::AnkiConnect::new(HttpClient::new(Box::new(UreqTransport)));
    match anki.model_field_names(LAPIS) {
        Ok(model_fields) => {
            let missing_fields: Vec<&String> =
                fields.iter().map(|(n, _)| n).filter(|n| !model_fields.contains(n)).collect();
            println!("\nAnki 的 Lapis 有 {} 个字段；我们填的字段里它没有的：{missing_fields:?}", model_fields.len());
            let expression = &fields[0].1;
            let note = json!({
                "deckName": "Lapis", "modelName": LAPIS,
                "fields": { "Expression": expression },
                "options": { "allowDuplicate": false, "duplicateScope": "collection" },
            });
            let can_add = anki.can_add_note(&note)?;
            let existing = existing_notes(&anki, LAPIS, &model_fields[0], expression)?;
            println!("canAddNotes: {can_add}；按首字段找到已有笔记 {existing:?}");
            if !existing.is_empty() {
                let info = anki.call("notesInfo", json!({ "notes": [existing[0]] }))?;
                let old = info[0]["fields"]["Glossary"]["value"].as_str().map(|s| s.chars().count());
                println!("已有那张卡的 Glossary {old:?} 字（Yomitan 做的），这次渲染出 {} 字", fields[11].1.chars().count());
            }
        }
        Err(err) => println!("\nAnki 连不上：{}", err.advice()),
    }
    Ok(())
}
