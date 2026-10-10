//! 会**往库里加歌、改歌**的流程，同样跑在合成库上，同样哪台机器都跑。
//!
//! 和 `fixture_commands.rs` 分开是因为那边有一批数曲目数、数「有歌词没分词」的断言；
//! 这里每导一首歌，那些数就变一次。集成测试一个文件一个进程，所以这里是另一个 app、另一个库，
//! 两边互不相干（一个进程里只能装一个 app，原因见 `support/mod.rs`）。
//!
//! 这个文件里的测试仍然共用一个库、排队跑。约定：
//!
//! * 每个测试用**自己的**下载目录和歌，不碰别人导进来的；
//! * 只断言自己那几首歌，或者前后的相对变化；
//! * 扫描完没导入的，走之前 `cancel_import`——待确认的那一批是 app 里的状态，会留给下一个测试。
//!
//! 导入的「音频」大多是几个字节的假文件：导入只看文件名和标签，不解码。
//! 真要解码的（读时长、播放）用 `support::write_tone` 现写一段正弦波。

mod support;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use jp_corpus::fixture;
use support::{Fixture, audio_or_skip, has_keys, invoke, len, ok, refused, write_tone};

/// 这个测试自己的下载目录，建在语料库目录下面（跟着库一起被下一次运行清掉）。
fn drop_dir(f: &Fixture, name: &str) -> PathBuf {
    let dir = f.dir().join("downloads").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 几个字节的假音频。导入不解码，认的是扩展名和文件名里的「歌手 - 曲名」。
fn fake_audio(dir: &Path, file_name: &str) -> PathBuf {
    let path = dir.join(file_name);
    std::fs::write(&path, b"not really audio").unwrap();
    path
}

/// 选中这些文件 → 导入，返回每首的 songId。不分词：合成库的目录里没有词典。
fn import_files(f: &Fixture, files: &[&Path]) -> Vec<String> {
    let paths: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let scan = ok(f.w(), "scan_files", json!({ "paths": paths }));
    assert_eq!(scan["summary"]["new"].as_u64(), Some(files.len() as u64), "没被判成新歌：{scan}");
    let report = ok(f.w(), "run_import", json!({ "tokenize": false }));
    let tracks = report["tracks"].as_array().expect("tracks 应当是数组");
    assert_eq!(tracks.len(), files.len(), "{report}");
    tracks
        .iter()
        .map(|t| {
            assert_eq!(t["outcome"]["kind"], "imported", "{report}");
            t["songId"].as_str().expect("要有 songId").to_string()
        })
        .collect()
}

fn track_count(f: &Fixture) -> i64 {
    ok(f.w(), "health", json!({}))["tracks"].as_i64().unwrap()
}

// ────────────────────────── 扫描与计划 ──────────────────────────

#[test]
fn a_folder_scan_honours_the_depth_argument_and_can_be_cancelled() {
    let f = Fixture::new("scan-depth");
    let w = f.w();
    let dir = drop_dir(&f, "depth");
    fake_audio(&dir, "浅い歌手 - 浅い曲.flac");
    let deep = dir.join("アルバム").join("ディスク1");
    std::fs::create_dir_all(&deep).unwrap();
    fake_audio(&deep, "深い歌手 - 深い曲.flac");

    let scan = |depth: Option<u32>| {
        let mut args = json!({ "path": dir.display().to_string() });
        if let Some(depth) = depth {
            args["maxDepth"] = json!(depth);
        }
        ok(w, "scan_folder", args)
    };
    // maxDepth=1 只看顶层目录；两层下面那个文件看不到
    let shallow = scan(Some(1));
    let all = scan(Some(6));
    assert_eq!(shallow["summary"]["total"].as_i64(), Some(1), "maxDepth 没起作用，说明 camelCase 参数没对上：{shallow}");
    assert_eq!(all["summary"]["total"].as_i64(), Some(2), "{all}");
    assert_eq!(all["summary"]["new"].as_i64(), Some(2));

    // 复核列表要给得出每条的处置
    let items = all["items"].as_array().expect("items 应当是数组");
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["action"]["kind"], "new", "{item}");
        assert!(item["title"].as_str().is_some_and(|t| !t.is_empty()), "{item}");
    }

    // 取消之后不该还能导入，库里一首都没多
    let before = track_count(&f);
    ok(w, "cancel_import", json!({}));
    refused(w, "run_import", json!({}));
    assert_eq!(track_count(&f), before);
}

/// 单曲导入：选中具体的文件，而不是整个目录。
#[test]
fn scanning_chosen_files_plans_exactly_those_files() {
    let f = Fixture::new("scan-files");
    let w = f.w();
    let dir = drop_dir(&f, "chosen");
    let first = fake_audio(&dir, "選択歌手 - ひとつめ.flac").display().to_string();
    let second = fake_audio(&dir, "選択歌手 - ふたつめ.mp3").display().to_string();
    fake_audio(&dir, "選択歌手 - 選ばれなかった.flac");

    let result = ok(w, "scan_files", json!({ "paths": [first, second] }));
    assert_eq!(result["summary"]["total"].as_i64(), Some(2), "目录里有三个，只选了两个：{result}");
    assert_eq!(result["summary"]["new"].as_i64(), Some(2));
    assert_eq!(len(&result["ignored"]), 0);

    // 同一个文件选两次只算一次，否则两条会互相判成「本批重复」
    let dupes = ok(w, "scan_files", json!({ "paths": [first, first] }));
    assert_eq!(dupes["summary"]["total"].as_i64(), Some(1));
    assert_eq!(dupes["summary"]["duplicatesInBatch"].as_i64(), Some(0));
    ok(w, "cancel_import", json!({}));
}

/// 选中的不是音频（.lrc、封面图）时要报出来，不能默不作声地丢掉——
/// 否则「选了 3 个只导了 2 个」看起来像 bug。
#[test]
fn a_chosen_file_that_is_not_audio_is_reported_not_swallowed() {
    let f = Fixture::new("scan-not-audio");
    let dir = drop_dir(&f, "not-audio");
    let lrc = dir.join("歌詞だけ.lrc");
    std::fs::write(&lrc, "[00:01.00]歌詞\n").unwrap();

    let result = ok(f.w(), "scan_files", json!({ "paths": [lrc.display().to_string()] }));
    assert_eq!(result["summary"]["total"].as_i64(), Some(0));
    assert_eq!(result["ignored"], json!(["歌詞だけ.lrc"]));
    ok(f.w(), "cancel_import", json!({}));
}

/// 导入不支持的格式（opus、wma）要在计划里写明原因，不入库。
#[test]
fn an_unsupported_format_is_planned_as_skipped_with_the_reason() {
    let f = Fixture::new("scan-unsupported");
    let dir = drop_dir(&f, "unsupported");
    let opus = fake_audio(&dir, "非対応歌手 - 非対応曲.opus");

    let result = ok(f.w(), "scan_files", json!({ "paths": [opus.display().to_string()] }));
    assert_eq!(result["summary"]["new"].as_i64(), Some(0), "{result}");
    assert_eq!(result["summary"]["skipped"].as_i64(), Some(1), "{result}");
    let action = &result["items"][0]["action"];
    assert_eq!(action["kind"], "skipped", "{result}");
    assert!(action["reason"].as_str().is_some_and(|r| r.contains("Opus")), "原因要说是哪种格式：{action}");
    ok(f.w(), "cancel_import", json!({}));
}

/// 重扫已经导入过的目录：计划里一条新的都没有，确认之后也什么都不写。
#[test]
fn rescanning_what_was_just_imported_plans_nothing_and_writes_nothing() {
    let f = Fixture::new("rescan");
    let w = f.w();
    let dir = drop_dir(&f, "rescan");
    let audio = fake_audio(&dir, "再スキャン歌手 - 再スキャン曲.flac");
    import_files(&f, &[&audio]);
    let before = track_count(&f);

    let result = ok(w, "scan_folder", json!({ "path": dir.display().to_string() }));
    let summary = &result["summary"];
    assert_eq!(summary["new"].as_i64(), Some(0), "重扫不该产生新条目：{result}");
    assert_eq!(summary["skipped"].as_i64(), Some(0));
    assert_eq!(summary["possibleDuplicates"].as_i64(), Some(0));
    assert_eq!(summary["alreadyImported"].as_i64(), Some(1));
    assert_eq!(summary["total"], summary["alreadyImported"]);
    assert_eq!(result["items"][0]["action"]["kind"], "alreadyImported");

    let report = ok(w, "run_import", json!({}));
    assert_eq!(len(&report["tracks"]), 0, "全是重复时不该写任何一条");
    assert_eq!(track_count(&f), before, "曲目数被改动了");
}

// ────────────────────────── 语料库文件夹里的新歌 ──────────────────────────
//
// 这一组用自己的目录（`watched/…`），不用上面的 `downloads/…`：那边哪些文件夹里有库里的歌
// 取决于别的测试跑没跑，「上一级过半就整个看」的判断会跟着变。

/// 这个测试自己的一片地方，同样建在语料库目录下面。
///
/// 多套一层（`watched/{name}/here`）：几个测试的文件夹要是并排放，其中两个里有了库里的歌，
/// 它们的上一级就会被整个看起来，各个测试就互相看得见了。
fn watched_dir(f: &Fixture, name: &str) -> PathBuf {
    let dir = f.dir().join("watched").join(name).join("here");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn new_audio(f: &Fixture, args: Value) -> Value {
    ok(f.w(), "library_new_audio", args)
}

/// 结果里落在 `dir` 下面的那些条目
fn items_under(result: &Value, dir: &Path) -> Vec<Value> {
    let prefix = dir.display().to_string();
    result["scan"]["items"]
        .as_array()
        .expect("scan.items 应当是数组")
        .iter()
        .filter(|item| item["path"].as_str().is_some_and(|p| p.starts_with(&prefix)))
        .cloned()
        .collect()
}

fn folder_entry(result: &Value, dir: &Path) -> Option<Value> {
    let wanted = dir.display().to_string();
    result["folders"]
        .as_array()
        .expect("folders 应当是数组")
        .iter()
        .find(|folder| folder["path"].as_str() == Some(wanted.as_str()))
        .cloned()
}

/// 整条链：往一个已经导入过的文件夹里放新文件 → 查得到、说得清在哪儿 →
/// 照常扫描导入 → 归到歌手名下 → 再查就不算新的了。
#[test]
fn a_file_dropped_into_a_library_folder_is_found_imported_and_filed_under_its_artist() {
    let f = Fixture::new("new-audio");
    let w = f.w();
    let dir = watched_dir(&f, "found");
    let existing = fake_audio(&dir, "見つかる歌手 - 入っている曲.flac");
    import_files(&f, &[&existing]);

    // 没放新文件时：这个文件夹在看，里面没有库外的东西
    let quiet = new_audio(&f, json!({}));
    has_keys(&quiet, "library_new_audio", &["folders", "scan"]);
    let folder = folder_entry(&quiet, &dir).unwrap_or_else(|| panic!("有库里的歌的文件夹应当在看：{quiet}"));
    has_keys(&folder, "folders[]", &["path", "reason", "depth", "songs", "unknown", "new"]);
    assert_eq!(folder["depth"].as_i64(), Some(1), "有歌的文件夹只看它自己这一层");
    assert_eq!(folder["reason"], "libraryAudio");
    assert_eq!(folder["songs"].as_i64(), Some(1));
    assert_eq!(folder["unknown"].as_i64(), Some(0));
    assert!(items_under(&quiet, &dir).is_empty(), "{quiet}");

    // 放进去：一首新歌、同一首歌的另一个文件、一个放不了的格式、一个不是音频的
    let fresh = fake_audio(&dir, "見つかる歌手 - 新しい曲.flac");
    fake_audio(&dir, "見つかる歌手 - 入っている曲.mp3");
    fake_audio(&dir, "見つかる歌手 - 放不了.opus");
    fake_audio(&dir, "メモ.txt");

    // 另有一批扫出来等着确认的：查新歌不能把它顶掉
    let pending = fake_audio(&drop_dir(&f, "new-audio-pending"), "待ち歌手 - 待っている曲.flac");
    ok(w, "scan_files", json!({ "paths": [pending.display().to_string()] }));
    let before = track_count(&f);

    let found = new_audio(&f, json!({}));
    let folder = folder_entry(&found, &dir).expect("文件夹还在看");
    assert_eq!(folder["unknown"].as_i64(), Some(3), "三个音频文件库里没有：{found}");
    assert_eq!(folder["new"].as_i64(), Some(1), "只有一个算新歌：{found}");
    let items = items_under(&found, &dir);
    let kind_of = |file: &str| -> String {
        let item = items
            .iter()
            .find(|item| item["fileName"] == file)
            .unwrap_or_else(|| panic!("结果里没有 {file}：{items:?}"));
        item["action"]["kind"].as_str().unwrap().to_string()
    };
    assert_eq!(kind_of("見つかる歌手 - 新しい曲.flac"), "new");
    assert_eq!(kind_of("見つかる歌手 - 入っている曲.mp3"), "possibleDuplicate", "曲名歌手对得上的不算新歌");
    assert_eq!(kind_of("見つかる歌手 - 放不了.opus"), "skipped");
    assert_eq!(items.len(), 3, "库里已有的那个和 .txt 都不该出现：{items:?}");
    let item = items.iter().find(|item| item["action"]["kind"] == "new").unwrap();
    has_keys(
        item,
        "scan.items[]",
        &["path", "fileName", "title", "artist", "artistSource", "album", "durationSec", "hasLyrics", "warning", "action"],
    );
    assert_eq!(item["title"], "新しい曲");
    assert_eq!(item["artist"], "見つかる歌手");
    assert_eq!(item["artistSource"], "fileName", "歌手是文件名里写的");
    assert_eq!(track_count(&f), before, "查新歌是只读的");

    // 等着确认的还是原来那一批，一首不多一首不少
    let report = ok(w, "run_import", json!({ "tokenize": false }));
    let imported: Vec<&str> =
        report["tracks"].as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
    assert_eq!(imported, ["待っている曲"], "查新歌把待确认的那一批换掉了：{report}");

    // 界面的「导入」：拿查到的路径走平常的扫描 → 导入
    let ids = import_files(&f, &[&fresh]);
    let track = ok(w, "get_track", json!({ "songId": ids[0] }));
    assert_eq!(track["title"], "新しい曲");
    assert_eq!(track["artist"], "見つかる歌手");
    let credits = ok(w, "track_credits", json!({ "songId": ids[0] }));
    assert!(
        credits.to_string().contains("見つかる歌手"),
        "导进来的歌要挂在歌手名下：{credits}"
    );

    // 导完就不是新的了；疑似重复和放不了的还在，但不算新歌
    let after = new_audio(&f, json!({}));
    let folder = folder_entry(&after, &dir).expect("文件夹还在看");
    assert_eq!(folder["songs"].as_i64(), Some(2));
    assert_eq!(folder["unknown"].as_i64(), Some(2), "{after}");
    assert_eq!(folder["new"].as_i64(), Some(0), "{after}");
}

/// 「别再提这个文件」「别再看这个文件夹」是界面记着、每次传回来的。
/// 参数名是 camelCase，传不到的话两样都不生效，而且不会报错。
#[test]
fn ignored_files_and_folders_are_left_out_of_the_check() {
    let f = Fixture::new("new-audio-ignored");
    let dir = watched_dir(&f, "ignored");
    let existing = fake_audio(&dir, "無視歌手 - 入っている曲.flac");
    import_files(&f, &[&existing]);
    let keep = fake_audio(&dir, "無視歌手 - 要る曲.flac");
    let skip = fake_audio(&dir, "無視歌手 - 要らない曲.flac");

    let all = new_audio(&f, json!({}));
    assert_eq!(items_under(&all, &dir).len(), 2, "{all}");

    let some = new_audio(&f, json!({ "ignoredFiles": [skip.display().to_string()] }));
    let left: Vec<Value> = items_under(&some, &dir);
    assert_eq!(left.len(), 1, "ignoredFiles 没传到：{some}");
    assert_eq!(left[0]["path"].as_str(), Some(keep.display().to_string().as_str()));
    assert_eq!(folder_entry(&some, &dir).expect("文件夹还在看")["new"].as_i64(), Some(1));

    let none = new_audio(&f, json!({ "ignoredFolders": [dir.display().to_string()] }));
    assert!(folder_entry(&none, &dir).is_none(), "ignoredFolders 没传到：{none}");
    assert!(items_under(&none, &dir).is_empty(), "{none}");
}

/// 上一级整个在看的时候排除它下面的一个文件夹：那个文件夹里的不再报，旁边的照报。
/// 排除清单要同时管「看哪些文件夹」和「往下翻的时候绕开谁」，只接上前一半的话这条会红。
#[test]
fn an_ignored_folder_stays_ignored_under_a_shelf_that_is_checked_as_a_whole() {
    let f = Fixture::new("new-audio-ignored-child");
    let shelf = watched_dir(&f, "ignored-child");
    let mut seeded = Vec::new();
    for artist in ["除外の歌手A", "除外の歌手B"] {
        let dir = shelf.join(artist);
        std::fs::create_dir_all(&dir).unwrap();
        seeded.push(fake_audio(&dir, &format!("{artist} - 入っている曲.flac")));
    }
    import_files(&f, &seeded.iter().map(PathBuf::as_path).collect::<Vec<_>>());
    let muted = shelf.join("除外の歌手A");
    fake_audio(&muted, "除外の歌手A - 要らない曲.flac");
    fake_audio(&shelf.join("除外の歌手B"), "除外の歌手B - 要る曲.flac");

    let all = new_audio(&f, json!({}));
    assert_eq!(folder_entry(&all, &shelf).expect("上一级整个在看")["new"].as_i64(), Some(2), "{all}");

    let some = new_audio(&f, json!({ "ignoredFolders": [muted.display().to_string()] }));
    let titles: Vec<&str> = some["scan"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["path"].as_str().is_some_and(|p| p.starts_with(&shelf.display().to_string())))
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["要る曲"], "{some}");
}

/// 一个歌手一个文件夹的那种目录：上一级整个在看，新建的歌手文件夹和直接丢在上一级的文件都找得到，
/// 文件夹名当歌手线索用。
#[test]
fn a_shelf_of_artist_folders_is_checked_as_a_whole() {
    let f = Fixture::new("new-audio-shelf");
    let shelf = watched_dir(&f, "shelf");
    let mut seeded = Vec::new();
    for artist in ["棚の歌手A", "棚の歌手B"] {
        let dir = shelf.join(artist);
        std::fs::create_dir_all(&dir).unwrap();
        seeded.push(fake_audio(&dir, &format!("{artist} - 入っている曲.flac")));
    }
    import_files(&f, &seeded.iter().map(PathBuf::as_path).collect::<Vec<_>>());

    let newcomer = shelf.join("棚の歌手C");
    std::fs::create_dir_all(&newcomer).unwrap();
    fake_audio(&newcomer, "フォルダだけが頼りの曲.flac");
    fake_audio(&shelf, "棚の歌手A - 散らかった曲.flac");

    let found = new_audio(&f, json!({}));
    let folder = folder_entry(&found, &shelf).unwrap_or_else(|| panic!("上一级应当整个在看：{found}"));
    assert_eq!(folder["reason"], "parent");
    assert_eq!(folder["songs"].as_i64(), Some(2));
    assert_eq!(folder["new"].as_i64(), Some(2), "{found}");
    assert!(folder_entry(&found, &shelf.join("棚の歌手A")).is_none(), "并上去之后不用再单列：{found}");

    let items = items_under(&found, &shelf);
    // 文件名里没有歌手的那个：歌手只能从文件夹名猜，要标出来——界面靠这个标记不让它直接导
    let by_folder = items
        .iter()
        .find(|item| item["title"] == "フォルダだけが頼りの曲")
        .expect("新歌手文件夹里的要找得到");
    assert_eq!(by_folder["artistSource"], "folder", "{by_folder}");
    let by_name = items.iter().find(|item| item["title"] == "散らかった曲").expect("丢在上一级的要找得到");
    assert_eq!(by_name["artist"], "棚の歌手A");
    assert_eq!(by_name["artistSource"], "fileName");
}

// ───────────────────────── 单曲导入 + 补歌词 ──────────────────────

/// 走完整条链：选一个文件 → 导入 → 给它导一份歌词 → 读回来 → 搜得到。
///
/// 这条链里每一步都是「编译通过但一点就报错」的高危处：命令有没有注册、camelCase 参数对不对、
/// 返回值的形状前端认不认、FTS 有没有跟着同步。
#[test]
fn importing_one_file_then_its_lyrics_works_end_to_end() {
    let f = Fixture::new("lyrics-flow");
    let w = f.w();
    let before = track_count(&f);

    // 造一个「音乐 App 下载目录」：只有音频，旁边没有 .lrc
    let dir = drop_dir(&f, "lyrics-flow");
    let audio = fake_audio(&dir, "テスト歌手 - テスト曲.flac");

    let scan = ok(w, "scan_files", json!({ "paths": [audio.display().to_string()] }));
    assert_eq!(scan["summary"]["total"].as_i64(), Some(1));
    assert_eq!(scan["summary"]["new"].as_i64(), Some(1), "单个文件没被判成新歌：{scan}");
    assert_eq!(scan["items"][0]["hasLyrics"].as_bool(), Some(false));

    let report = ok(w, "run_import", json!({ "tokenize": false }));
    let tracks = report["tracks"].as_array().expect("tracks 应当是数组");
    assert_eq!(tracks.len(), 1, "{report}");
    assert_eq!(tracks[0]["outcome"]["kind"], "imported");
    assert_eq!(tracks[0]["outcome"]["lyricLines"].as_i64(), Some(0));
    let song_id = tracks[0]["songId"].as_str().expect("要有 songId").to_string();
    assert_eq!(track_count(&f), before + 1);

    let in_missing_list = |song_id: &str| -> Option<Value> {
        ok(w, "lyrics_missing", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["songId"].as_str() == Some(song_id))
            .cloned()
    };
    let finds = |text: &str, song_id: &str| -> bool {
        ok(w, "search_lyrics", json!({ "text": text, "limit": 10 }))
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["songId"].as_str() == Some(song_id))
    };

    // 刚导进来的歌必然在「缺歌词」名单里
    let row = in_missing_list(&song_id).expect("缺歌词名单里没有刚导入的那首");
    assert_eq!(row["title"].as_str(), Some("テスト曲"));
    assert_eq!(row["siblingLrc"], Value::Null, "旁边没有 .lrc 却说有");

    // 用户自己给一份歌词
    let lrc = dir.join("hand.lrc");
    std::fs::write(&lrc, "作詞 : テスト作詞家\n[00:12.00]ひとつめの行\n[00:20.50]ふたつめの行\n").unwrap();
    let attached = ok(w, "lyrics_import_file", json!({ "songId": song_id, "path": lrc.display().to_string() }));
    assert_eq!(attached["source"], "manual");
    assert_eq!(attached["lyricLines"].as_i64(), Some(2));
    assert_eq!(attached["credits"].as_i64(), Some(1), "LRC 里的作词没进署名");

    // 落盘的那一份要在语料库目录下，文件名是 song_id
    let on_disk = f.dir().join("raw").join("lyrics_lrc").join(format!("{song_id}.lrc"));
    assert!(on_disk.is_file(), "没落盘：{}", on_disk.display());

    // 读回来
    let lines = ok(w, "lyrics", json!({ "songId": song_id }));
    assert_eq!(len(&lines), 2);
    assert_eq!(lines[0]["text"].as_str(), Some("ひとつめの行"));
    assert!((lines[1]["timeSec"].as_f64().unwrap() - 20.5).abs() < 1e-6);

    // 搜得到——外部内容的 FTS 表没有触发器，漏同步这里就是空
    assert!(finds("ふたつめ", &song_id), "新挂上的歌词搜不到");
    assert!(in_missing_list(&song_id).is_none(), "挂上歌词之后还算缺歌词");

    // 换一份：不能变成两份，旧的也不能留在全文索引里
    let lrc2 = dir.join("hand2.lrc");
    std::fs::write(&lrc2, "[00:05.00]あたらしい行\n").unwrap();
    let again = ok(w, "lyrics_import_file", json!({ "songId": song_id, "path": lrc2.display().to_string() }));
    assert_eq!(again["lyricLines"].as_i64(), Some(1));
    assert_eq!(len(&ok(w, "lyrics", json!({ "songId": song_id }))), 1, "换歌词变成了两份");
    assert!(!finds("ふたつめ", &song_id), "旧歌词还留在全文索引里");
    assert!(finds("あたらしい", &song_id));

    // 已经有歌词的歌不走在线补齐——绝不用网上搜到的悄悄覆盖用户手上那一份
    let message = refused(w, "lyrics_fill_one", json!({ "songId": song_id }));
    assert!(message.contains("已经有歌词"), "错误信息要说清为什么拒绝：{message}");
}

/// 音频旁边就有 .lrc 时，「补齐」不该上网——本地那一份必然是对的。
#[test]
fn a_sibling_lrc_is_used_without_going_online() {
    let f = Fixture::new("lyrics-sibling");
    let w = f.w();
    let dir = drop_dir(&f, "lyrics-sibling");
    let audio = fake_audio(&dir, "ローカル歌手 - ローカル曲.flac");
    let song_id = import_files(&f, &[&audio]).remove(0);

    // 导入之后才下的歌词：文件现在才出现在音频旁边
    std::fs::write(audio.with_extension("lrc"), "[00:01.00]あとから来た歌詞\n").unwrap();

    let missing = ok(w, "lyrics_missing", json!({}));
    let row = missing
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["songId"].as_str() == Some(song_id.as_str()))
        .expect("应当还在缺歌词名单里");
    assert!(row["siblingLrc"].as_str().is_some(), "没认出音频旁边的 .lrc：{row}");

    let attached = ok(w, "lyrics_fill_one", json!({ "songId": song_id }));
    assert_eq!(attached["source"], "sibling", "本地有 .lrc 却跑去上网：{attached}");
    assert_eq!(attached["lyricLines"].as_i64(), Some(1));
}

// ────────────────────────── 维护 ──────────────────────────

/// 回填时长：只处理还没有时长的，读得出来的写进去，读不出来的**按原因分开数**。
///
/// 以前这三条跑在真库上，而 `backfill_durations` 是会写库的——等于测试在改用户的库。
#[test]
fn duration_backfill_writes_what_it_can_and_counts_the_rest_by_reason() {
    let f = Fixture::new("backfill");
    let w = f.w();
    let dir = drop_dir(&f, "backfill");
    // 一段真的能解码的 2 秒正弦波，和一个解不开的假文件
    let tone = dir.join("時間歌手 - 二秒の曲.wav");
    write_tone(&tone, 2.0);
    let broken = fake_audio(&dir, "時間歌手 - 壊れた曲.flac");
    let ids = import_files(&f, &[&tone, &broken]);
    let (tone_id, broken_id) = (&ids[0], &ids[1]);

    // **前提自己造**：不管导入时有没有顺手读过时长，这里都清掉，让回填来读
    f.sql(&format!("UPDATE songs SET duration_sec = NULL WHERE id IN ('{tone_id}', '{broken_id}')"));
    let duration = |song_id: &str| ok(w, "get_track", json!({ "songId": song_id }))["durationSec"].clone();
    assert!(duration(tone_id).is_null());
    let total_before = ok(w, "overview", json!({}))["totalDurationSec"].as_f64().unwrap();

    let report = ok(w, "backfill_durations", json!({}));
    has_keys(&report, "回填报告", &["scanned", "written", "missing", "unknown", "failed", "samples"]);
    let count = |report: &Value, key: &str| report[key].as_i64().unwrap();
    assert!(count(&report, "written") >= 1, "{report}");
    // 每一种失败都要单独计数——笼统的「成功 N 个」用户无从修。
    // 合成库里 003 没有时长、音频路径是个不存在的文件；假文件是打不开或解不了
    assert!(count(&report, "missing") >= 1, "路径失效的要算进 missing：{report}");
    assert!(count(&report, "failed") + count(&report, "unknown") >= 1, "解不开的要算进 failed / unknown：{report}");
    assert_eq!(
        count(&report, "scanned"),
        count(&report, "written") + count(&report, "missing") + count(&report, "unknown") + count(&report, "failed"),
        "每一首待处理的都要落进某一个桶：{report}"
    );
    assert!(len(&report["samples"]) > 0, "失败的要给出几条详情：{report}");

    // 读得出来的写进去了，前端拿得到
    let seconds = duration(tone_id).as_f64().unwrap_or_else(|| panic!("回填后应当有时长：{report}"));
    assert!((seconds - 2.0).abs() < 0.05, "2 秒的音频读出来是 {seconds} 秒");
    assert!(duration(broken_id).is_null(), "读不出来的不该写一个假的数");
    // totalDurationSec 是 f64，用 as_i64 读会得到 None
    let total_after = ok(w, "overview", json!({}))["totalDurationSec"].as_f64().unwrap();
    assert!((total_after - total_before - seconds).abs() < 1e-6, "总时长该多出这 {seconds} 秒：{total_before} → {total_after}");

    // 再跑一次：已经填过的不再处理，读不出来的还是那几首
    let again = ok(w, "backfill_durations", json!({}));
    assert_eq!(count(&again, "written"), 0, "回填应当幂等：{again}");
    assert_eq!(count(&again, "scanned"), count(&report, "scanned") - count(&report, "written"), "{again}");
}

#[test]
fn filling_album_artwork_is_idempotent() {
    let f = Fixture::new("album-art");
    let w = f.w();
    // 合成库里 Rain Notes 自己没有封面，它的曲目有；别的专辑连曲目都没有封面
    let stored = || {
        let conn = rusqlite::Connection::open(f.dir().join("corpus.db")).unwrap();
        let mut stmt = conn.prepare("SELECT title, artwork_path FROM albums ORDER BY id").unwrap();
        let rows: Vec<(String, String)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        rows
    };
    let before = stored();
    assert!(before.iter().any(|(title, art)| title == "Rain Notes" && art.is_empty()), "{before:?}");

    assert_eq!(ok(w, "fill_album_artwork", json!({})), json!(1), "有曲目封面的专辑该被填上");
    assert_eq!(ok(w, "fill_album_artwork", json!({})), json!(0), "重跑该是幂等的");

    let after = stored();
    for ((title, old), (_, new)) in before.iter().zip(&after) {
        if title == "Rain Notes" {
            assert_eq!(new, "E:/music/covers/004.jpg");
        } else {
            assert_eq!(new, old, "「{title}」的曲目没有封面，不该被动");
        }
    }
}

// ────────────────────────── 播放（要输出设备） ──────────────────────────

/// 导一段真的能放的音频，返回它的 songId。
fn import_tone(f: &Fixture, name: &str, seconds: f64) -> String {
    let dir = drop_dir(f, name);
    let tone = dir.join(format!("再生歌手 - {name}.wav"));
    write_tone(&tone, seconds);
    import_files(f, &[&tone]).remove(0)
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    (0..150).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        done()
    })
}

#[test]
fn the_full_playback_loop_works_through_ipc() {
    let f = Fixture::new("playback");
    let w = f.w();
    if !audio_or_skip(w, "the_full_playback_loop_works_through_ipc") {
        return;
    }
    let song_id = import_tone(&f, "playback", 40.0);

    ok(w, "audio_set_volume", json!({ "volume": 0.0 })); // 测试不出声
    ok(w, "audio_load", json!({ "songId": &song_id }));

    let state = ok(w, "audio_state", json!({}));
    assert_eq!(state["songId"], json!(song_id));
    assert_eq!(state["playState"], json!("playing"));

    let position = || ok(w, "audio_state", json!({}))["positionSec"].as_f64().unwrap_or(0.0);
    assert!(wait_until(|| position() > 0.05), "播放后位置应当推进");
    // 音量是 0，频谱照样有能量：频谱取的是音量之前的信号
    let has_energy = wait_until(|| {
        ok(w, "audio_spectrum", json!({})).as_array().unwrap().iter().any(|v| v.as_f64().unwrap_or(0.0) > 0.05)
    });
    assert!(has_energy, "真实音频应当产生非零频谱");

    ok(w, "audio_seek", json!({ "positionSec": 30.0 }));
    assert!(wait_until(|| position() >= 30.0), "拖动之后位置应当在 30 秒之后，实际 {}", position());

    ok(w, "audio_stop", json!({}));
    assert_eq!(ok(w, "audio_state", json!({}))["playState"], json!("empty"));
    ok(w, "audio_set_volume", json!({ "volume": 1.0 }));
}

#[test]
fn a_brief_touch_does_not_pollute_the_history() {
    let f = Fixture::new("brief-touch");
    let w = f.w();
    if !audio_or_skip(w, "a_brief_touch_does_not_pollute_the_history") {
        return;
    }
    let song_id = import_tone(&f, "brief-touch", 20.0);
    let history = || len(&ok(w, "recently_played", json!({ "limit": 50 })));
    let before = history();

    // 点开就立刻停——不该进历史
    ok(w, "audio_set_volume", json!({ "volume": 0.0 }));
    ok(w, "audio_load", json!({ "songId": &song_id }));
    for _ in 0..5 {
        ok(w, "audio_tick", json!({}));
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    ok(w, "audio_stop", json!({}));
    ok(w, "audio_tick", json!({}));
    ok(w, "audio_set_volume", json!({ "volume": 1.0 }));

    assert_eq!(history(), before, "听不到 5 秒不该进历史");
}

/// 合成库本身的那四首歌不该被这个文件里的任何一条动到。放在这里是提醒：
/// 上面的测试只许加自己的歌、改自己的歌。
#[test]
fn the_seeded_songs_are_still_what_the_fixture_wrote() {
    let f = Fixture::new("seeded");
    let w = f.w();
    for (song_id, title, lines) in [("001", "街の灯", 4), ("002", "朝の窓", 3), ("003", "海の底", 2), ("004", "雨の歌", 4)] {
        assert_eq!(ok(w, "get_track", json!({ "songId": song_id }))["title"], title);
        assert_eq!(len(&ok(w, "lyrics", json!({ "songId": song_id }))), lines);
    }
    assert!(track_count(&f) >= fixture::TRACKS);
    let _ = invoke(w, "cancel_import", json!({}));
}
