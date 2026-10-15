//! 内嵌歌词、封面（#5）：拿真的音频文件写上 tag，从扫描一路走到入库。
//!
//! 夹具借 `jp-app/tests/data/formats/` 里那几个 1 秒的正弦波，复制到临时目录再用 lofty 写 tag，
//! 写法就是 Mp3tag / foobar2000 / iTunes 落盘的那几个字段（见 `jp_import::embedded` 的表）。

use std::path::{Path, PathBuf};

use jp_import::{LibraryIndex, LyricsPick, execute, plan, scan_file};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagExt};
use rusqlite::Connection;

const SYNCED: &str = "[ti:夜明け]\n[00:01.00]夜が明けるまで\n[00:05.50]君を待っている\n";
const PLAIN: &str = "夜が明けるまで\n君を待っている";
/// 封底：JPEG 文件头加几个字节
const BACK: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, b'b', b'a', b'c', b'k'];

/// 最小的一张 PNG：只要文件头对，`image_dimensions` 认得出宽高就够了
fn png() -> Vec<u8> {
    let mut data = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
    data.extend_from_slice(&600u32.to_be_bytes());
    data.extend_from_slice(&600u32.to_be_bytes());
    data.extend_from_slice(b"\x08\x02\x00\x00\x00");
    data
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jp-import-embedded-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 复制一个格式夹具，写上曲名、歌手，再按需写歌词和封面
fn fixture(dir: &Path, ext: &str, key: ItemKey, lyrics: Option<&str>, cover: bool) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../jp-app/tests/data/formats/tone.{ext}"));
    let dest = dir.join(format!("song.{ext}"));
    std::fs::copy(&src, &dest).unwrap();

    let tagged = Probe::open(&dest).unwrap().read().unwrap();
    let mut tag = tagged.primary_tag().cloned().unwrap_or_else(|| Tag::new(tagged.primary_tag_type()));
    tag.insert_text(ItemKey::TrackTitle, format!("夜明け {ext}"));
    tag.insert_text(ItemKey::TrackArtist, "誰か".into());
    if let Some(text) = lyrics {
        assert!(tag.insert_text(key, text.into()), "{ext} 写不进 {key:?}");
    }
    if cover {
        // 先放一张封底，确认取的是标成正面封面的那张，不是第一张
        tag.push_picture(
            Picture::unchecked(BACK.to_vec())
                .pic_type(PictureType::CoverBack)
                .mime_type(MimeType::Jpeg)
                .build(),
        );
        tag.push_picture(
            Picture::unchecked(png()).pic_type(PictureType::CoverFront).mime_type(MimeType::Png).build(),
        );
    }
    tag.save_to_path(&dest, WriteOptions::default()).unwrap();
    dest
}

/// 三种格式、各自的歌词字段都读得出来；有没有时间轴按内容判断，不按字段名
#[test]
fn lyrics_and_cover_are_read_from_flac_mp3_and_m4a() {
    let cases = [
        ("flac", ItemKey::Lyrics, SYNCED, true),
        // ID3v2 的 USLT 名义上是「不同步」，但网易云和 Mp3tag 往里写整份 LRC
        ("mp3", ItemKey::UnsyncLyrics, SYNCED, true),
        ("m4a", ItemKey::Lyrics, PLAIN, false),
    ];
    for (ext, key, text, synced) in cases {
        let dir = scratch(&format!("read-{ext}"));
        let audio = fixture(&dir, ext, key, Some(text), true);

        let lyrics = jp_import::embedded::read_lyrics(&audio).unwrap_or_else(|| panic!("{ext} 没读到歌词"));
        assert_eq!(lyrics.synced, synced, "{ext}");
        let lines: Vec<String> =
            jp_import::lrc::parse(&lyrics.text).lines.into_iter().map(|l| l.text).collect();
        assert_eq!(lines, ["夜が明けるまで", "君を待っている"], "{ext}");

        let cover = jp_import::embedded::read_cover(&audio).unwrap_or_else(|| panic!("{ext} 没读到封面"));
        if ext == "m4a" {
            // MP4 的 covr 不记图片类型，读回来都是 Other：按 iTunes 的规矩取第一张
            assert_eq!(cover.data, BACK, "{ext} 该取第一张");
            assert_eq!(cover.mime.as_deref(), Some("image/jpeg"), "{ext}");
        } else {
            assert_eq!(cover.data, png(), "{ext} 取的不是正面封面");
            assert_eq!(cover.mime.as_deref(), Some("image/png"), "{ext}");
        }

        // 扫描时就带上了，界面会看到「有歌词」
        let scanned = scan_file(&audio);
        assert_eq!(scanned.warning, None, "{ext}");
        assert!(matches!(scanned.lyrics_pick(), Some(LyricsPick::Embedded(_))), "{ext}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_file_without_embedded_data_reads_as_none() {
    let dir = scratch("none");
    let audio = fixture(&dir, "flac", ItemKey::Lyrics, None, false);
    assert_eq!(jp_import::embedded::read_lyrics(&audio), None);
    assert_eq!(jp_import::embedded::read_cover(&audio), None);
    assert_eq!(scan_file(&audio).lyrics_pick(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 内嵌的优先于旁边的 .lrc；只有内嵌的没时间轴时让给 .lrc
#[test]
fn embedded_lyrics_win_over_a_sibling_lrc_unless_they_lack_timestamps() {
    let sibling = "[00:02.00]旁边の歌詞\n";

    let dir = scratch("pick-synced");
    let audio = fixture(&dir, "flac", ItemKey::Lyrics, Some(SYNCED), false);
    std::fs::write(audio.with_extension("lrc"), sibling).unwrap();
    let scanned = scan_file(&audio);
    assert!(scanned.lyrics_path.is_some(), "前提：旁边的 .lrc 也找到了");
    assert!(matches!(scanned.lyrics_pick(), Some(LyricsPick::Embedded(t)) if t.contains("夜が明ける")));
    let _ = std::fs::remove_dir_all(&dir);

    let dir = scratch("pick-plain");
    let audio = fixture(&dir, "flac", ItemKey::Lyrics, Some(PLAIN), false);
    let lrc = audio.with_extension("lrc");
    std::fs::write(&lrc, sibling).unwrap();
    let scanned = scan_file(&audio);
    assert_eq!(scanned.lyrics_pick(), Some(LyricsPick::File(&lrc.display().to_string())));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 一路走到库里：歌词行带着时间轴进去，全文检索搜得到，source_file 记的是音频本身
#[test]
fn embedded_lyrics_are_imported_with_their_timestamps() {
    let dir = scratch("import");
    let audio = fixture(&dir, "mp3", ItemKey::UnsyncLyrics, Some(SYNCED), false);
    let tracks = vec![scan_file(&audio)];

    let mut conn = Connection::open_in_memory().unwrap();
    jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
    let report = execute(&mut conn, &plan(&tracks, &LibraryIndex::empty()), None).unwrap();
    assert_eq!(report.imported(), 1, "{:?}", report.tracks);

    let rows: Vec<(Option<f64>, String)> = conn
        .prepare("SELECT time_sec, text FROM utterances ORDER BY line_idx")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [(Some(1.0), "夜が明けるまで".to_string()), (Some(5.5), "君を待っている".to_string())]
    );
    let hits: i64 = conn
        .query_row("SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '\"君を待っている\"'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(hits, 1, "全文索引没写");
    let source: String = conn.query_row("SELECT source_file FROM songs", [], |r| r.get(0)).unwrap();
    assert_eq!(source, tracks[0].path);
    let _ = std::fs::remove_dir_all(&dir);
}
