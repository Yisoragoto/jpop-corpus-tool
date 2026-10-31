//! 音频文件记录（`audio_files`）：从扫描一路走到入库、重扫、文件变了之后刷新。
//!
//! 夹具同 `embedded.rs`：`jp-app/tests/data/formats/` 里 1 秒的正弦波，复制出来写上 tag。

use std::path::{Path, PathBuf};

use jp_import::{Action, LibraryIndex, execute, plan, scan_targets_known};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagExt};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jp-import-audio-files-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tag(path: &Path, title: &str, genre: &str) {
    let tagged = Probe::open(path).unwrap().read().unwrap();
    let mut tag = tagged.primary_tag().cloned().unwrap_or_else(|| Tag::new(tagged.primary_tag_type()));
    tag.insert_text(ItemKey::TrackTitle, title.into());
    tag.insert_text(ItemKey::TrackArtist, "誰か".into());
    tag.insert_text(ItemKey::Genre, genre.into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn rescan(corpus: &jp_corpus::Corpus, dir: &Path) -> jp_import::ImportPlan {
    let index = LibraryIndex::from_corpus(corpus).unwrap();
    let (tracks, _) = scan_targets_known(&[dir.display().to_string()], 4, Some(&index));
    plan(&tracks, &index)
}

#[test]
fn import_records_the_file_rescans_skip_it_and_a_changed_file_refreshes_only_file_fields() {
    let dir = scratch("flow");
    let audio = dir.join("song.flac");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../jp-app/tests/data/formats/tone.flac"), &audio).unwrap();
    tag(&audio, "夜明け", "J-Pop");

    let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
    corpus.ensure_schema().unwrap();

    // 第一次：新歌，文件信息记进 audio_files
    let first = rescan(&corpus, &dir);
    assert!(matches!(first.items[0].action, Action::New { .. }));
    execute(corpus.connection_mut(), &first, None).unwrap();
    let song_id = first.items[0].action.clone();
    let Action::New { song_id, .. } = song_id else { unreachable!() };
    let rows = jp_corpus::audio_files::for_song(corpus.connection(), &song_id).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.status, "present");
    assert_eq!(row.facts.format, "flac");
    assert!(row.facts.size.is_some() && row.facts.mtime.is_some());
    assert!(row.facts.sample_rate.is_some_and(|r| r > 0), "{row:?}");
    assert!(row.facts.duration_sec.is_some_and(|d| d > 0.5));
    assert!(row.fingerprint.is_some(), "指纹跟着记下");

    // 第二次：没变，不读标签就判「已在库中」
    let second = rescan(&corpus, &dir);
    assert!(second.items[0].track.unchanged, "没变的文件不该再读标签");
    assert_eq!(second.items[0].action, Action::already(song_id.clone()));

    // 你改了流派；然后文件里的曲名和流派都换了
    let edit = jp_import::maintain::SongEdit {
        title: "夜明け".into(),
        artist: "誰か".into(),
        genre: "アニソン".into(),
        ..Default::default()
    };
    jp_import::maintain::edit_song(corpus.connection(), &song_id, &edit).unwrap();
    // 修改时间至少差一秒以上，FAT 之类只精确到两秒
    std::thread::sleep(std::time::Duration::from_millis(50));
    tag(&audio, "夜明け (2024 Mix)", "Rock");
    let file = std::fs::File::options().write(true).open(&audio).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10)).unwrap();
    drop(file);

    let third = rescan(&corpus, &dir);
    assert!(!third.items[0].track.unchanged);
    assert_eq!(third.items[0].action, Action::AlreadyImported { song_id: song_id.clone(), file_changed: true });
    let report = execute(corpus.connection_mut(), &third, None).unwrap();
    assert_eq!(report.files_refreshed, 1);
    let (title, genre): (String, String) = corpus
        .connection()
        .query_row("SELECT title, genre FROM songs WHERE id=?1", [&song_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    // 只有你真改了的流派记成 manual；曲名你没动过，还是「来自文件」，跟着新标签走
    assert_eq!(title, "夜明け (2024 Mix)", "来自文件的字段跟着文件更新");
    assert_eq!(genre, "アニソン", "你改过的字段不被文件覆盖");
    let rows = jp_corpus::audio_files::for_song(corpus.connection(), &song_id).unwrap();
    assert_eq!(rows.len(), 1, "同一路径只是更新");
    assert_ne!(rows[0].facts.mtime, row.facts.mtime);
}

#[test]
fn fields_that_came_from_the_file_follow_the_file() {
    let dir = scratch("follow");
    let audio = dir.join("song.flac");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../jp-app/tests/data/formats/tone.flac"), &audio).unwrap();
    tag(&audio, "夜明け", "J-Pop");
    let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
    corpus.ensure_schema().unwrap();
    let first = rescan(&corpus, &dir);
    execute(corpus.connection_mut(), &first, None).unwrap();
    let Action::New { song_id, .. } = first.items[0].action.clone() else { unreachable!() };

    tag(&audio, "夜明け", "Rock");
    let file = std::fs::File::options().write(true).open(&audio).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10)).unwrap();
    drop(file);
    let again = rescan(&corpus, &dir);
    execute(corpus.connection_mut(), &again, None).unwrap();
    let genre: String = corpus.connection().query_row("SELECT genre FROM songs WHERE id=?1", [&song_id], |r| r.get(0)).unwrap();
    assert_eq!(genre, "Rock", "来源是 tag 的字段跟着文件更新");
    // 从不往音频文件里写：文件里还是我们自己写的那份
    let tagged = Probe::open(&audio).unwrap().read().unwrap();
    assert_eq!(tagged.primary_tag().unwrap().get_string(ItemKey::Genre), Some("Rock"));
}
