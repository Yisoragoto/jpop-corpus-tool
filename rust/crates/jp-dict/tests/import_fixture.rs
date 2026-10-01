//! 用 Yomitan 自带的测试词典（`valid-dictionary1`）导入，和 Yomitan 自己跑出来的结果核对：
//! 条数不丢、条目 id 的先后与 Yomitan 相同、释义整理（图片、结构化内容）逐项相同。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jp_dict::import::{Archive, DirArchive, ImportOptions, import_dictionary};
use jp_dict::store::DictionaryStore;
use serde_json::Value;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yomitan-translator/valid-dictionary1")
}

/// Yomitan 的测试框架打包时把标题改成 `Test Dictionary 2`，这里照做。
struct Renamed {
    inner: DirArchive,
    title: String,
}

impl Archive for Renamed {
    fn file_names(&self) -> Vec<String> {
        self.inner.file_names()
    }

    fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let bytes = self.inner.read(name)?;
        if name != "index.json" {
            return Ok(bytes);
        }
        let mut index: Value = serde_json::from_slice(&bytes)?;
        index["title"] = Value::String(self.title.clone());
        Ok(serde_json::to_vec(&index)?)
    }
}

fn yomitan_test_options() -> ImportOptions {
    // Yomitan 测试里的图片加载器固定返回 100×100
    ImportOptions { image_size: |_, _| Some((100, 100)), ..ImportOptions::default() }
}

#[test]
fn a_cancelled_import_leaves_nothing_behind() {
    let mut store = DictionaryStore::open_in_memory().unwrap();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let options = ImportOptions { cancel: Some(cancel), ..yomitan_test_options() };
    let err = import_dictionary(&mut store, &mut DirArchive::open(&fixture_dir()).unwrap(), &options, &mut |_| {})
        .unwrap_err();
    assert_eq!(err.to_string(), jp_dict::import::CANCELLED);
    assert!(store.dictionaries().unwrap().is_empty());
    // 取消后照常能导入同一本
    import_dictionary(&mut store, &mut DirArchive::open(&fixture_dir()).unwrap(), &yomitan_test_options(), &mut |_| {})
        .unwrap();
    assert_eq!(store.dictionaries().unwrap().len(), 1);
}

fn count_rows(dir: &Path, prefix: &str) -> usize {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        let is_bank = name
            .strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(".json"))
            .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
        if is_bank {
            let rows: Vec<Value> = serde_json::from_slice(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
            total += rows.len();
        }
    }
    total
}

fn imported() -> DictionaryStore {
    let mut store = DictionaryStore::open_in_memory().unwrap();
    let mut archive = Renamed { inner: DirArchive::open(&fixture_dir()).unwrap(), title: "Test Dictionary 2".into() };
    import_dictionary(&mut store, &mut archive, &yomitan_test_options(), &mut |_| {}).unwrap();
    store
}

#[test]
fn nothing_is_lost_and_a_second_import_is_refused() {
    let dir = fixture_dir();
    let mut store = DictionaryStore::open_in_memory().unwrap();
    let summary =
        import_dictionary(&mut store, &mut DirArchive::open(&dir).unwrap(), &yomitan_test_options(), &mut |_| {}).unwrap();

    assert_eq!(summary.title, "Test Dictionary");
    assert_eq!(summary.terms, count_rows(&dir, "term_bank_"));
    assert_eq!(summary.term_meta["total"], count_rows(&dir, "term_meta_bank_"));
    assert_eq!(summary.tags, count_rows(&dir, "tag_bank_"));
    assert_eq!(summary.kanji_skipped, count_rows(&dir, "kanji_bank_") + count_rows(&dir, "kanji_meta_bank_"));
    assert!(summary.media > 0, "测试词典里有被释义引用的图片");
    assert_eq!(summary.warning_count, 0, "{:?}", summary.warnings);

    let infos = store.dictionaries().unwrap();
    assert_eq!(infos.len(), 1);
    assert!(infos[0].has_styles);
    assert_eq!(infos[0].counts["terms"], summary.terms);

    let err = import_dictionary(&mut store, &mut DirArchive::open(&dir).unwrap(), &yomitan_test_options(), &mut |_| {})
        .unwrap_err();
    assert!(err.to_string().contains("已经导入"), "{err}");
    assert_eq!(store.dictionaries().unwrap().len(), 1);
}

#[test]
fn every_definition_in_yomitans_expected_results_matches_the_stored_row_by_id() {
    let store = imported();
    let enabled: HashSet<i64> = store.dictionaries().unwrap().iter().map(|d| d.id).collect();
    let results: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/yomitan-translator/translator-test-results.json")).unwrap();

    let mut checked = 0;
    for result in &results {
        let Some(entries) = result.get("dictionaryEntries").and_then(Value::as_array) else { continue };
        for entry in entries.iter().filter(|e| e["type"] == "term") {
            let terms: Vec<String> = entry["headwords"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h["term"].as_str().unwrap().to_owned())
                .collect();
            let rows = store.find_terms_bulk(&terms, &enabled).unwrap();
            for definition in entry["definitions"].as_array().unwrap() {
                let id = definition["id"].as_i64().unwrap();
                let row = rows
                    .iter()
                    .find(|r| r.id == id)
                    .unwrap_or_else(|| panic!("[{}] id {id} 找不到（{terms:?}）", result["name"]));
                // Yomitan 查词时会把「某词的变形」这种数组释义滤掉
                let content: Vec<Value> = store.glossary(row.glossary).unwrap().into_iter().filter(|g| !g.is_array()).collect();
                assert_eq!(Value::Array(content), definition["entries"], "[{}] id {id}", result["name"]);
                assert_eq!(row.score, definition["score"].as_f64().unwrap(), "[{}] id {id} score", result["name"]);
                assert!(
                    definition["sequences"].as_array().unwrap().iter().any(|s| s.as_i64() == Some(row.sequence)),
                    "[{}] id {id} sequence",
                    result["name"]
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "只核对了 {checked} 条，结果文件不对？");
}

#[test]
fn stored_images_can_be_fetched_back_by_path() {
    let store = imported();
    let id = store.dictionaries().unwrap()[0].id;
    let media = store.media(id, "image.gif").unwrap().expect("image.gif 被释义引用，应当入库");
    assert_eq!(media.media_type, "image/gif");
    assert_eq!((media.width, media.height), (100, 100));
    assert_eq!(media.data, std::fs::read(fixture_dir().join("image.gif")).unwrap());
    assert!(store.media(id, "not-there.png").unwrap().is_none());
}

#[test]
fn deleting_a_dictionary_removes_all_of_its_rows() {
    let mut store = imported();
    let id = store.dictionaries().unwrap()[0].id;
    store.delete(id).unwrap();
    assert!(store.dictionaries().unwrap().is_empty());
    let enabled = HashSet::from([id]);
    assert!(store.find_terms_bulk(&["打".to_owned()], &enabled).unwrap().is_empty());
    assert!(store.media(id, "image.gif").unwrap().is_none());
}
