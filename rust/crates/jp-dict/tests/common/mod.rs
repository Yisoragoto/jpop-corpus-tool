//! 和 Yomitan 对账的测试共用：导入 Yomitan 的测试词典，按它的测试输入组合查词选项。
//!
//! 输入、选项预设、测试词典都原样取自 Yomitan 仓库；组合预设的方式照抄
//! `test/utilities/translator.js` 的 `createFindTermsOptions`。

#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::Result;
use jp_dict::import::{Archive, DirArchive, ImportOptions, import_dictionary};
use jp_dict::store::DictionaryStore;
use jp_dict::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions};
use serde_json::{Map, Value, json};

pub const TITLE: &str = "Test Dictionary 2";
const PLACEHOLDER: &str = "${title}";

struct Renamed(DirArchive);

impl Archive for Renamed {
    fn file_names(&self) -> Vec<String> {
        self.0.file_names()
    }

    fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let bytes = self.0.read(name)?;
        if name != "index.json" {
            return Ok(bytes);
        }
        let mut index: Value = serde_json::from_slice(&bytes)?;
        index["title"] = json!(TITLE);
        Ok(serde_json::to_vec(&index)?)
    }
}

/// Yomitan 测试里的 `createDictionaryArchiveData(dir, 'Test Dictionary 2')` + 图片尺寸一律 100×100
pub fn store() -> DictionaryStore {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yomitan-translator/valid-dictionary1");
    let mut store = DictionaryStore::open_in_memory().unwrap();
    let options = ImportOptions { image_size: |_, _| Some((100, 100)), ..ImportOptions::default() };
    import_dictionary(&mut store, &mut Renamed(DirArchive::open(&dir).unwrap()), &options, &mut |_| {}).unwrap();
    store
}

pub fn inputs() -> Value {
    serde_json::from_str(include_str!("../fixtures/yomitan-translator/translator-test-inputs.json")).unwrap()
}

/// `getCompositePreset`：字符串引用预设，对象原样，按顺序浅合并。
fn composite_preset(presets: &Value, options: &Value) -> Map<String, Value> {
    let list = match options {
        Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let mut preset = Map::new();
    for item in list {
        let object = match item {
            Value::String(name) => presets[&name].clone(),
            other => other,
        };
        for (k, v) in object.as_object().unwrap() {
            preset.insert(k.clone(), v.clone());
        }
    }
    preset
}

pub struct Case {
    pub name: String,
    pub text: String,
    pub mode: FindTermsMode,
    pub options: FindTermsOptions,
}

/// 把一条 Yomitan 测试输入转成查词参数。暂未移植的功能返回 `Err(跳过原因)`。
pub fn case(inputs: &Value, test: &Value) -> Result<Case, String> {
    let name = test["name"].as_str().unwrap().to_owned();
    if test["func"] != "findTerms" {
        return Err(format!("{name}（{}）", test["func"]));
    }
    let preset = composite_preset(&inputs["optionsPresets"], &test["options"]);
    let mode = match test["mode"].as_str().unwrap() {
        "simple" => FindTermsMode::Simple,
        "split" => FindTermsMode::Split,
        "group" => FindTermsMode::Group,
        "term" => FindTermsMode::Term,
        other => return Err(format!("{name}（{other} 模式）")),
    };
    let text_of = |key: &str| preset.get(key).and_then(Value::as_str);
    if text_of("language").unwrap_or("ja") != "ja" {
        return Err(format!("{name}（语言 {}）", text_of("language").unwrap_or_default()));
    }
    if preset.get("textReplacements").and_then(Value::as_array).is_some_and(|r| r.iter().any(|x| !x.is_null())) {
        return Err(format!("{name}（文本替换）"));
    }
    if text_of("searchResolution").is_some_and(|s| s != "letter") {
        return Err(format!("{name}（按词切分）"));
    }

    let dictionaries = preset["enabledDictionaryMap"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            let title = pair[0].as_str().unwrap();
            let details = &pair[1];
            EnabledDictionary {
                title: if title == PLACEHOLDER { TITLE.to_owned() } else { title.to_owned() },
                index: details["index"].as_u64().unwrap() as usize,
                alias: details["alias"].as_str().unwrap_or_default().to_owned(),
                parts_of_speech_filter: details["partsOfSpeechFilter"].as_bool().unwrap_or(false),
                use_deinflections: details["useDeinflections"].as_bool().unwrap_or(true),
            }
        })
        .collect();
    let options = FindTermsOptions {
        dictionaries,
        deinflect: preset.get("deinflect").and_then(Value::as_bool).unwrap_or(true),
        remove_non_japanese_characters: preset.get("removeNonJapaneseCharacters").and_then(Value::as_bool).unwrap_or(false),
        primary_reading: text_of("primaryReading").unwrap_or_default().to_owned(),
        sort_frequency_dictionary: text_of("sortFrequencyDictionary").map(str::to_owned),
        sort_frequency_ascending: text_of("sortFrequencyDictionaryOrder").unwrap_or("ascending") == "ascending",
        use_all_frequency_dictionaries: false,
        max_results: None,
    };
    Ok(Case { name, text: test["text"].as_str().unwrap().to_owned(), mode, options })
}
