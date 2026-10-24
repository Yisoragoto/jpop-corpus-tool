use super::*;
#[test]
fn management_roundtrip_and_flattened_shapes() {
    let (s, dir) = tests::session();
    s.corpus.lock().unwrap().connection().execute("INSERT INTO songs(id,title,artist,audio_path) VALUES ('901','旧标题','歌手','missing.flac')", []).unwrap();
    let edited = s.call("libraryEditSong", json!({"songId":"901","edit":{"title":"新标题","artist":"歌手","album":"专辑","year":"2026","genre":"Pop"}})).unwrap();
    assert_eq!(edited["csv"], "noFile");
    assert!(
        edited["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("title"))
    );
    assert_eq!(
        s.call("track", json!({"songId":"901"})).unwrap()["title"],
        "新标题"
    );
    assert!(
        s.call("libraryMissingAudio", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["songId"] == "901")
    );
    let bad = s
        .call(
            "libraryRelinkAudio",
            json!({"links":[{"songId":"901","path":"absent.flac"}]}),
        )
        .unwrap();
    assert!(!bad["results"][0]["error"].as_str().unwrap().is_empty());
    let deleted = s
        .call("libraryDeleteSong", json!({"songId":"901"}))
        .unwrap();
    assert_eq!(deleted["title"], "新标题");
    assert_eq!(deleted["coverRemoved"], false);
    assert!(s.call("track", json!({"songId":"901"})).unwrap().is_null());
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}
#[test]
fn maintenance_job_does_not_take_session_lock() {
    let (s, dir) = tests::session();
    let guard = s.corpus.lock().unwrap();
    s.call("backfillDurations", json!({})).unwrap();
    let mut finished = false;
    for _ in 0..500 {
        let v = s.call("libraryProgress", json!({})).unwrap();
        if v["finished"] == true {
            assert!(v["error"].is_null(), "{v}");
            assert_eq!(v["result"]["scanned"], 1);
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(finished);
    drop(guard);
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn members_corrections_and_queries_roundtrip() {
    let (s, dir) = tests::session();
    let people = s.call("peopleByRole", json!({"role":"performer"})).unwrap();
    let band = people[0]["id"].as_i64().unwrap();
    assert_eq!(
        s.call(
            "membershipPut",
            json!({"bandId":band,"memberName":"新成员","part":"吉他","joined":"2017","leftAt":""})
        )
        .unwrap(),
        "inserted"
    );
    let members = s.call("bandMembers", json!({"personId":band})).unwrap();
    let member = members[0]["memberId"].as_i64().unwrap();
    assert_eq!(members[0]["source"], "manual");
    assert_eq!(
        s.call(
            "membershipConfirm",
            json!({"bandId":band,"memberId":member})
        )
        .unwrap(),
        "keptExisting"
    );
    assert_eq!(
        s.call("bandsOf", json!({"personId":member})).unwrap()[0]["bandId"],
        band
    );
    s.call("membershipReject", json!({"bandId":band,"memberId":member}))
        .unwrap();
    assert!(
        s.call("bandMembers", json!({"personId":band}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let lines = s.call("lyrics", json!({"songId":"001"})).unwrap();
    let id = lines[0]["utteranceId"].as_i64().unwrap();
    let before = s
        .call("tokenCorrection", json!({"utteranceId":id}))
        .unwrap();
    s.call(
        "saveTokenCorrection",
        json!({"utteranceId":id,"tokens":[{"surface":"测试","lemma":"词元","pos":"NOUN"}]}),
    )
    .unwrap();
    assert_eq!(
        s.call("tokenCorrection", json!({"utteranceId":id}))
            .unwrap()["corrected"],
        true
    );
    assert_eq!(
        s.call("revertTokenCorrection", json!({"utteranceId":id}))
            .unwrap(),
        true
    );
    assert_eq!(
        s.call("tokenCorrection", json!({"utteranceId":id}))
            .unwrap()["tokens"],
        before["tokens"]
    );
    for method in [
        "memberships",
        "membershipSuggestions",
        "artistRoster",
        "recentlyPlayed",
        "mostPlayed",
    ] {
        assert!(s.call(method, json!({})).unwrap().is_array(), "{method}");
    }
    assert!(
        s.call("worksByPerson", json!({"personId":band}))
            .unwrap()
            .is_array()
    );
    assert_eq!(
        s.call("lemmaForms", json!({"lemmas":[]})).unwrap(),
        json!([])
    );
    assert!(s.call("migratePlan", json!({"path":dir})).is_err());
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}
#[test]
fn discovery_does_not_replace_pending_scan_and_ignores_are_applied() {
    let (s, dir) = tests::session();
    let audio = dir.join("新歌手 - 新曲.wav");
    // 0.1 秒单声道 WAV，不引入新依赖。
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&1636u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&1600u32.to_le_bytes());
    wav.resize(1644, 0);
    std::fs::write(&audio, wav).unwrap();
    s.call("scanFiles", json!({"paths":[audio]})).unwrap();
    let count = s.pending_scan.lock().unwrap().len();
    let found = s.call("libraryNewAudio", json!({})).unwrap();
    assert!(
        found["scan"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["title"] == "新曲")
    );
    assert_eq!(s.pending_scan.lock().unwrap().len(), count);
    let ignored = s
        .call("libraryNewAudio", json!({"ignoredFiles":[audio]}))
        .unwrap();
    assert!(
        !ignored["scan"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["title"] == "新曲")
    );
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}

fn finish_library_job(s: &Session) -> Value {
    for _ in 0..500 {
        let view = s.call("libraryProgress", json!({})).unwrap();
        if view["finished"] == true {
            assert!(view["error"].is_null(), "{view}");
            return view["result"].clone();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("资料库作业超时")
}

#[test]
fn migration_copies_files_remaps_ids_and_does_not_take_session_lock() {
    let (s, dir) = tests::session();
    let source = dir.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let db = jp_corpus::fixture::write_to(&source).unwrap();
    let source_corpus = Corpus::open_writable(&db).unwrap();
    source_corpus.connection().execute("INSERT INTO songs(id,title,artist,audio_path) VALUES ('901','新迁移曲','新歌手','/original/audio.wav')", []).unwrap();
    let cover = source.join("raw/covers/新歌手/901.jpg");
    let lrc = source.join("raw/lyrics_lrc/901.lrc");
    std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
    std::fs::create_dir_all(lrc.parent().unwrap()).unwrap();
    std::fs::write(&cover, b"cover bytes").unwrap();
    std::fs::write(&lrc, "[00:01.00]新迁移曲").unwrap();
    source_corpus
        .connection()
        .execute(
            "UPDATE songs SET cover_path=?1,source_file=?2 WHERE id='901'",
            [
                cover.to_string_lossy().as_ref(),
                lrc.to_string_lossy().as_ref(),
            ],
        )
        .unwrap();
    let plan = s.call("migratePlan", json!({"path": source})).unwrap();
    assert_eq!(plan["sourceSongs"], 5);
    assert_eq!(plan["newSongs"], 1);
    let guard = s.corpus.lock().unwrap();
    s.call(
        "migrateRun",
        json!({"path":source,"options":{"history":true,"dictionaries":false}}),
    )
    .unwrap();
    let result = finish_library_job(&s);
    assert_eq!(result["songs"], 1);
    assert_eq!(result["skipped"], 4);
    assert_eq!(result["coversCopied"], 1);
    assert_eq!(result["lyricsCopied"], 1);
    let id = result["idMap"][0][1].as_str().unwrap();
    assert_ne!(id, "901");
    drop(guard);
    let track = s.call("track", json!({"songId":id})).unwrap();
    assert_eq!(track["audioPath"], "/original/audio.wav");
    assert_eq!(
        std::fs::read(track["coverPath"].as_str().unwrap()).unwrap(),
        b"cover bytes"
    );
    assert_eq!(
        source_corpus
            .connection()
            .query_row("SELECT title FROM songs WHERE id='901'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "新迁移曲"
    );
    s.call(
        "migrateRun",
        json!({"path":source,"options":{"history":true,"dictionaries":false}}),
    )
    .unwrap();
    assert_eq!(finish_library_job(&s)["songs"], 0);
    drop(source_corpus);
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn editing_and_deleting_sync_existing_csv_and_only_remove_app_cover() {
    let (s, dir) = tests::session();
    let csv = dir.join("metadata/songs.csv");
    std::fs::create_dir_all(csv.parent().unwrap()).unwrap();
    std::fs::write(
        &csv,
        "id,title,artist,year,album,genre,audio_path\r\n901,旧曲,旧人,,,,absent.wav\r\n",
    )
    .unwrap();
    let cover = dir.join("raw/covers/旧人/901.jpg");
    std::fs::create_dir_all(cover.parent().unwrap()).unwrap();
    std::fs::write(&cover, b"old cover").unwrap();
    s.corpus.lock().unwrap().connection().execute("INSERT INTO songs(id,title,artist,audio_path,cover_path) VALUES ('901','旧曲','旧人','absent.wav',?1)", [cover.to_string_lossy().as_ref()]).unwrap();
    let report = s.call("libraryEditSong",json!({"songId":"901","edit":{"title":"新曲","artist":"旧人","year":"","album":"","genre":""}})).unwrap();
    assert_eq!(report["csv"], "updated");
    assert!(
        std::fs::read_to_string(&csv)
            .unwrap()
            .contains("901,新曲,旧人")
    );
    let report = s
        .call("libraryDeleteSong", json!({"songId":"901"}))
        .unwrap();
    assert_eq!(report["csv"], "updated");
    assert_eq!(report["coverRemoved"], true);
    assert!(!cover.exists());
    assert!(!std::fs::read_to_string(&csv).unwrap().contains("901,"));
    let outside = dir.join("user-cover.jpg");
    std::fs::write(&outside, b"user image").unwrap();
    s.corpus
        .lock()
        .unwrap()
        .connection()
        .execute(
            "UPDATE songs SET cover_path=?1 WHERE id='001'",
            [outside.to_string_lossy().as_ref()],
        )
        .unwrap();
    assert_eq!(
        s.call("libraryDeleteSong", json!({"songId":"001"}))
            .unwrap()["coverRemoved"],
        false
    );
    assert!(outside.exists());
    drop(s);
    std::fs::remove_dir_all(dir).ok();
}
