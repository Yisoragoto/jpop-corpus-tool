use super::*;

fn fixture() -> (Session, PathBuf) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jp-lyrics-ffi-{}-{n}", std::process::id()));
    let db = jp_corpus::fixture::write_to(&dir).unwrap();
    (Session::open(db.to_str().unwrap()).unwrap(), dir)
}

#[test]
fn save_lines_read_back_and_restore_an_old_version() {
    let (s, dir) = fixture();
    s.call("lyricsSaveLines", json!({"songId":"001","lines":[{"timeSec":1.0,"text":"星の空"},{"timeSec":3.0,"text":"夜の声"}]})).unwrap();
    let original = s.call("lyrics", json!({"songId":"001"})).unwrap();
    let lineage = s.call("songLineage", json!({"songId":"001"})).unwrap();
    let version = lineage["versions"][0]["id"].clone();
    let saved = s.call("lyricsSaveLines", json!({"songId":"001","lines":[{"timeSec":1.25,"text":"あの空"},{"timeSec":4.0,"text":"君の声"}],"note":"修正"})).unwrap();
    assert_eq!(saved["lyricLines"], 2);
    let rows = s.call("lyrics", json!({"songId":"001"})).unwrap();
    assert_eq!(rows[0]["text"], "あの空");
    assert_eq!(rows[0]["timeSec"], 1.25);
    assert!(std::fs::read_to_string(dir.join("raw/lyrics_lrc/001.lrc")).unwrap().contains("あの空"));
    s.call("lyricsRestoreVersion", json!({"songId":"001","versionId":version})).unwrap();
    let restored = s.call("lyrics", json!({"songId":"001"})).unwrap();
    let texts = |rows: &Value| rows.as_array().unwrap().iter().map(|r| r["text"].clone()).collect::<Vec<_>>();
    assert_eq!(texts(&restored), texts(&original));
    let versions = s.call("songLineage", json!({"songId":"001"})).unwrap();
    assert!(versions["versions"].as_array().unwrap().len() >= 3);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn manual_word_start_is_set_read_and_cleared() {
    let (s, dir) = fixture();
    let row = s.call("lyrics", json!({"songId":"001"})).unwrap()[0]["utteranceId"].clone();
    s.call("wordTimeSet", json!({"utteranceId":row,"charStart":0,"charEnd":1,"startSec":1.125})).unwrap();
    let marks = s.call("wordTimings", json!({"utteranceIds":[row]})).unwrap();
    assert!(marks.as_array().unwrap().iter().any(|m| m["source"] == "manual" && m["startSec"] == 1.125));
    s.call("wordTimeClear", json!({"utteranceId":row,"charStart":0})).unwrap();
    let marks = s.call("wordTimings", json!({"utteranceIds":[row]})).unwrap();
    assert!(marks.as_array().unwrap().iter().all(|m| m["source"] != "manual"));
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn jobs_have_the_polling_shape_and_do_not_take_the_session_lock() {
    let (s, dir) = fixture();
    s.call("lyricsSourcesSet", json!({"config":{"netease":false,"qq":false,"kugou":false,"amll":false,"lrclib":false}})).unwrap();
    // 空目标也必须有 finished 终态，不能让界面永久等待。
    let guard = s.corpus.lock().unwrap();
    s.call("lyricsFillStart", json!({"songIds":[]})).unwrap();
    let mut progress = Value::Null;
    for _ in 0..500 {
        progress = s.call("lyricsFillProgress", json!({})).unwrap();
        if progress["finished"] == true { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(progress["running"], false);
    assert_eq!(progress["finished"], true);
    for key in ["cancelled", "done", "total", "stage", "result", "error"] {
        assert!(progress.get(key).is_some(), "缺字段 {key}");
    }
    assert_eq!(progress["result"]["upgraded"], 0);
    assert!(s.call("lyricsUpgradeStatus", json!({})).unwrap()["results"].is_array());
    s.call("lyricsFillCancel", json!({})).unwrap();
    s.call("lyricsUpgradeCancel", json!({})).unwrap();
    drop(guard);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_nonempty_job_writes_on_its_own_connection_while_the_session_is_locked() {
    let (s, dir) = fixture();
    s.call("lyricsSourcesSet", json!({"config":{"netease":false,"qq":false,"kugou":false,"amll":false,"lrclib":false}})).unwrap();
    let audio = dir.join("offline.wav");
    std::fs::write(audio.with_extension("lrc"), "[00:01.000]あの空\n").unwrap();
    let guard = s.corpus.lock().unwrap();
    guard.connection().execute("INSERT INTO songs(id,title,artist,duration_sec,audio_path) VALUES('offline','試験曲','テスト歌手',30,?1)", [audio.to_str().unwrap()]).unwrap();
    s.call("lyricsFillStart", json!({"songIds":["offline"]})).unwrap();
    let mut progress = Value::Null;
    for _ in 0..500 {
        progress = s.call("lyricsFillProgress", json!({})).unwrap();
        if progress["finished"] == true { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(progress["finished"], true);
    assert_eq!(progress["error"], Value::Null);
    assert_eq!(progress["done"], 1);
    assert_eq!(progress["result"]["lineFilled"], 1);
    assert_eq!(progress["result"]["results"][0]["source"], "sibling");
    drop(guard);
    assert_eq!(s.call("lyrics", json!({"songId":"offline"})).unwrap()[0]["text"], "あの空");
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn timing_save_and_local_import_use_the_shared_version_path() {
    let (s, dir) = fixture();
    s.call("lyricsSaveLines", json!({"songId":"001","lines":[{"timeSec":1.0,"text":"あの空"}]})).unwrap();
    s.call("lyricsSaveTiming", json!({"songId":"001","lines":[{"timeSec":1.0,"text":"あの空","units":[{"charStart":0,"charEnd":1,"startSec":1.0,"endSec":1.4},{"charStart":1,"charEnd":3,"startSec":1.5,"endSec":2.0}]}]})).unwrap();
    let timeline = s.call("lyricTimeline", json!({"songId":"001"})).unwrap();
    assert_eq!(timeline[0]["mode"], "karaoke");
    let file = dir.join("import.lrc");
    std::fs::write(&file, "[00:03.000]君の声\n").unwrap();
    s.call("lyricsImportFile", json!({"songId":"001","path":file})).unwrap();
    assert_eq!(s.call("lyrics", json!({"songId":"001"})).unwrap()[0]["text"], "君の声");
    assert_eq!(s.call("lyricsRevalidate", json!({"songId":"001"})).unwrap(), 1);
    drop(s);
    let _ = std::fs::remove_dir_all(dir);
}
