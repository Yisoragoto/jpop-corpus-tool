//! 导入、分词词典、查词这几组方法：从一个空目录建库，导一个「音频 + .lrc」的文件夹，
//! 补上分词之后 KWIC 能查到。这就是 macOS 客户端「自己建库」的那条路。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::Session;

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jp-ffi-import-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 一秒钟的静音 WAV。lofty 读得出时长就够了
fn write_wav(path: &Path) {
    const RATE: u32 = 8_000;
    let data_len = RATE * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.resize(44 + data_len as usize, 0);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// 一个「歌手文件夹 / 歌手 - 曲名.wav + 同名 .lrc」的音乐目录，两首歌
fn music_folder() -> PathBuf {
    let music = temp_dir("music");
    let artist = music.join("テスト歌手");
    write_wav(&artist.join("テスト歌手 - 夜の歌.wav"));
    std::fs::write(
        artist.join("テスト歌手 - 夜の歌.lrc"),
        "[00:01.00]夜の風が吹いている\n[00:04.00]君を待っている\n[00:07.00]夜の風が吹いている\n",
    )
    .unwrap();
    write_wav(&artist.join("テスト歌手 - 朝の歌.wav"));
    std::fs::write(artist.join("テスト歌手 - 朝の歌.lrc"), "[00:01.00]朝の光が眩しい\n").unwrap();
    // 不是音频，扫目录时不计数
    std::fs::write(artist.join("cover.jpg"), b"not really").unwrap();
    music
}

/// 一个全新的库：目录和 corpus.db 都还不存在
fn fresh_library() -> (Session, PathBuf) {
    let root = temp_dir("lib").join("JPOP Corpus Tool");
    let session = Session::open(root.join("corpus.db").to_str().unwrap()).unwrap();
    (session, root)
}

/// 轮询一个作业直到跑完
fn wait(session: &Session, progress: &str) -> Value {
    for _ in 0..3000 {
        let view = session.call(progress, json!({})).unwrap();
        if view["finished"].as_bool() == Some(true) {
            return view;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{progress} 一直没跑完");
}

/// 本机或 CI 上的一份 Sudachi 词典（`JP_SUDACHI_SOURCE`，或开发树上 0.1.x 的 venv）。没有就跳过要分词的那部分
fn dictionary_source() -> Option<PathBuf> {
    let candidates = std::env::var_os("JP_SUDACHI_SOURCE")
        .map(PathBuf::from)
        .into_iter()
        .chain([PathBuf::from(r"D:\jp_corpus")]);
    candidates.into_iter().find(|p| jp_tokenizer::find_sudachipy_source(p).is_some())
}

#[test]
fn a_fresh_library_is_built_from_a_folder_of_audio_and_lrc() {
    let music = music_folder();
    let (s, root) = fresh_library();
    assert!(root.join("corpus.db").is_file(), "打开即建库");
    assert_eq!(s.call("tracks", json!({})).unwrap().as_array().unwrap().len(), 0);

    // 扫描：只读，给出计划
    let plan = s.call("scanFolder", json!({ "path": music.to_str().unwrap() })).unwrap();
    assert_eq!(plan["summary"]["total"], 2);
    assert_eq!(plan["summary"]["new"], 2);
    let items = plan["items"].as_array().unwrap();
    assert!(items.iter().all(|i| i["hasLyrics"] == true && i["action"]["kind"] == "new"), "{plan}");
    assert!(items.iter().any(|i| i["title"] == "夜の歌" && i["artist"] == "テスト歌手"), "{plan}");
    assert_eq!(s.call("tracks", json!({})).unwrap().as_array().unwrap().len(), 0, "扫描不写库");

    // 导入：后台跑，轮询进度
    s.call("runImport", json!({ "tokenize": false })).unwrap();
    let done = wait(&s, "importProgress");
    assert!(done["error"].is_null(), "{done}");
    assert_eq!(done["done"], 2);
    assert_eq!(done["total"], 2);
    assert_eq!(done["result"]["imported"], 2);
    assert_eq!(done["result"]["lyricLines"], 4);
    assert!(!done["cancelled"].as_bool().unwrap());

    // 会话那条连接看得到作业那条写进去的
    let tracks = s.call("tracks", json!({})).unwrap();
    assert_eq!(tracks.as_array().unwrap().len(), 2);
    let night = tracks.as_array().unwrap().iter().find(|t| t["title"] == "夜の歌").unwrap();
    let lyrics = s.call("lyrics", json!({ "songId": night["id"] })).unwrap();
    assert_eq!(lyrics.as_array().unwrap().len(), 3);

    // 一批只导一次；再扫一遍全是「已在库中」
    assert!(s.call("runImport", json!({})).unwrap_err().to_string().contains("没有待导入"));
    let again = s.call("scanFolder", json!({ "path": music.to_str().unwrap() })).unwrap();
    assert_eq!(again["summary"]["new"], 0, "{again}");

    // 没分词时：列出缺分词的歌，KWIC 查不到
    let missing = s.call("tokenizeMissingList", json!({})).unwrap();
    assert_eq!(missing.as_array().unwrap().len(), 2);

    // 装上词典 → 顺手补分词 → KWIC 查得到（要本机有一份词典）
    let Some(source) = dictionary_source() else {
        eprintln!("跳过分词那一半：没有 Sudachi 词典（设 JP_SUDACHI_SOURCE）");
        return;
    };
    let ready = s.call("tokenizerInstall", json!({ "path": source.to_str().unwrap() })).unwrap();
    assert_eq!(ready["tokenized"]["songs"], 2, "{ready}");
    assert_eq!(ready["tokenizeError"], "");
    let status = s.call("tokenizerStatus", json!({})).unwrap();
    assert!(status["ready"] == true && status["source"] == "library", "{status}");
    assert!(root.join("sudachi").join("system.dic").is_file());

    let hits = s.call("kwic", json!({ "query": { "keywords": ["風"], "field": "lemma" } })).unwrap();
    // 重复的那一行默认合成一条（repeatCount 2）
    assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
    assert_eq!(hits[0]["repeatCount"], 2);
    assert!(hits[0]["endSec"].as_f64().unwrap() > hits[0]["timeSec"].as_f64().unwrap());
    let tokens = s.call("tokenize", json!({ "text": "夜行列車" })).unwrap();
    assert!(!tokens.as_array().unwrap().is_empty());

    // 有词典时导入直接带分词
    let more = temp_dir("music-more");
    write_wav(&more.join("別の歌手").join("別の歌手 - 雨.wav"));
    std::fs::write(more.join("別の歌手").join("別の歌手 - 雨.lrc"), "[00:01.00]雨の夜に風が吹く\n").unwrap();
    let plan = s.call("scanFolder", json!({ "path": more.to_str().unwrap() })).unwrap();
    assert_eq!(plan["tokenizerReady"], true);
    assert_eq!(s.call("runImport", json!({})).unwrap()["tokenize"], true);
    let done = wait(&s, "importProgress");
    assert!(done["result"]["tokens"].as_u64().unwrap() > 0, "{done}");
    let hits = s.call("kwic", json!({ "query": { "keywords": ["風"], "field": "lemma" } })).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 2, "{hits}");

    drop(s);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
    let _ = std::fs::remove_dir_all(music);
    let _ = std::fs::remove_dir_all(more);
}

#[test]
fn scanning_specific_files_reports_what_was_not_audio() {
    let music = music_folder();
    let (s, root) = fresh_library();
    let artist = music.join("テスト歌手");
    let wav = artist.join("テスト歌手 - 夜の歌.wav");
    let plan = s
        .call(
            "scanFiles",
            json!({ "paths": [wav.to_str().unwrap(), wav.to_str().unwrap(), artist.join("cover.jpg").to_str().unwrap()] }),
        )
        .unwrap();
    assert_eq!(plan["summary"]["total"], 1, "同一个文件选两次只算一次");
    assert_eq!(plan["ignored"], json!(["cover.jpg"]));
    assert!(s.call("scanFiles", json!({ "paths": [] })).is_err());
    assert!(s.call("scanFolder", json!({ "path": music.join("没有").to_str().unwrap() })).is_err());
    drop(s);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
    let _ = std::fs::remove_dir_all(music);
}

#[test]
fn a_cancelled_import_keeps_what_it_finished_and_reports_cancelled() {
    let music = music_folder();
    let (s, root) = fresh_library();
    s.call("scanFolder", json!({ "path": music.to_str().unwrap() })).unwrap();
    // 先要求取消再启动：启动会清掉旧的取消标志，所以这里应当照常跑完
    s.call("cancelImport", json!({})).unwrap();
    s.call("runImport", json!({ "tokenize": false })).unwrap();
    // 紧接着取消：两首歌很快，可能已经导完了；不管停在哪，库里的数等于报告里的数
    s.call("cancelImport", json!({})).unwrap();
    let done = wait(&s, "importProgress");
    let imported = done["result"]["imported"].as_u64().unwrap();
    let in_library = s.call("tracks", json!({})).unwrap().as_array().unwrap().len() as u64;
    assert_eq!(imported, in_library, "{done}");
    assert_eq!(done["cancelled"].as_bool().unwrap(), imported < 2);
    drop(s);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
    let _ = std::fs::remove_dir_all(music);
}

#[test]
fn without_a_dictionary_the_tokenizer_says_so_and_lookups_are_empty() {
    let (s, root) = fresh_library();
    let status = s.call("tokenizerStatus", json!({})).unwrap();
    assert!(status["bundledDir"].as_str().unwrap().ends_with("sudachi"));
    if status["ready"] == false {
        let err = s.call("tokenize", json!({ "text": "夜" })).unwrap_err().to_string();
        assert!(err.contains(jp_setup::sudachi::NO_DICTIONARY), "{err}");
        assert!(s.call("tokenizeMissingRun", json!({})).is_err());
    }
    // 没导入过 Yomitan 词典：列表空、查词空，也不新建 dictionaries.db
    assert_eq!(s.call("dictList", json!({})).unwrap(), json!([]));
    let found = s.call("dictLookupText", json!({ "text": "夜の風" })).unwrap();
    assert_eq!(found["entries"], json!([]));
    assert_eq!(s.call("dictLookup", json!({ "text": "夜" })).unwrap()["dictionaryEntries"], json!([]));
    assert!(!root.join("dictionaries.db").exists());
    // 进度：没启动过的作业
    let idle = s.call("tokenizerProgress", json!({})).unwrap();
    assert!(idle["running"] == false && idle["finished"] == false);
    drop(s);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

#[test]
fn lookups_read_an_imported_yomitan_dictionary() {
    let (s, root) = fresh_library();
    // 和 Tauri 客户端同一个文件：语料库目录下的 dictionaries.db
    {
        let mut store = jp_dict::store::DictionaryStore::open(&root.join("dictionaries.db")).unwrap();
        jp_dict::import::import_path(&mut store, &yomitan_fixture(), &mut |_| {}).unwrap();
    }
    let list = s.call("dictList", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    let found = s.call("dictLookupText", json!({ "text": "夜の風" })).unwrap();
    let entry = &found["entries"][0];
    assert_eq!(entry["headwords"][0]["term"], "夜", "{found}");
    assert_eq!(entry["definitions"][0]["text"][0], "日が沈んでから昇るまでの間");
    assert_eq!(found["originalTextLength"], 1);
    drop(s);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// 一本只有一个词条的 Yomitan 词典（解开的目录，导入认这种形态）
fn yomitan_fixture() -> PathBuf {
    let dir = temp_dir("yomitan");
    let index = json!({ "title": "テスト辞書", "revision": "1", "format": 3 }).to_string();
    let terms = json!([["夜", "よる", "", "", 0, ["日が沈んでから昇るまでの間"], 1, ""]]).to_string();
    std::fs::write(dir.join("index.json"), index).unwrap();
    std::fs::write(dir.join("term_bank_1.json"), terms).unwrap();
    dir
}
