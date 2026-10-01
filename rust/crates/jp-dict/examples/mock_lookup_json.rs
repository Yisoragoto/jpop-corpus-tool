//! 给前端 dev-mock.html 造假数据：查几个词，把界面会收到的 JSON 原样打出来。
//!
//! 默认用 Yomitan 自带的测试词典（GPL 测试数据，可以放进仓库）；
//! 传一个词典库路径则查真实词典（只用于本机调试，别把输出放进仓库——里面是商业词典的正文）。
//!
//! ```text
//! cargo run -p jp-dict --example mock_lookup_json -- [dictionaries.db] > mock.json
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jp_dict::import::{DirArchive, ImportOptions, import_dictionary};
use jp_dict::store::DictionaryStore;
use jp_dict::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions, Translator};

fn main() -> anyhow::Result<()> {
    let (store, texts): (DictionaryStore, Vec<&str>) = match std::env::args().nth(1) {
        Some(db) => (
            DictionaryStore::open(Path::new(&db))?,
            vec!["打ち込んでいませんでした", "夜に駆ける", "食べちゃった", "学校"],
        ),
        None => {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yomitan-translator/valid-dictionary1");
            let mut store = DictionaryStore::open_in_memory()?;
            import_dictionary(&mut store, &mut DirArchive::open(&dir)?, &ImportOptions::default(), &mut |_| {})?;
            (store, vec!["打ち込んでいませんでした", "画像", "構造", "お手前", "好き"])
        }
    };
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
    let translator = Translator::new();
    let mut lookups = BTreeMap::new();
    for text in texts {
        lookups.insert(text, translator.find_terms(&store, FindTermsMode::Group, text, &options)?);
    }
    println!("{}", serde_json::to_string(&serde_json::json!({ "dictionaries": dictionaries, "lookups": lookups }))?);
    Ok(())
}
