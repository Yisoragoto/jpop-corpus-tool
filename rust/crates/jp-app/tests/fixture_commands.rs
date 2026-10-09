//! 不挑机器的 command 集成测试。
//!
//! `tests/commands.rs` 测的是**真库**：209 首歌、58,628 个 token，找不到那个 991MB 的
//! `corpus.db` 时整组跳过**并算通过**。这一组的数据来自 `jp_corpus::fixture`（四首歌，建在临时目录里），
//! 所以**在任何机器上都真的跑**。它测的不是数量，是「这条路通不通」：
//!
//! * command 有没有真的注册上（名字写错编译期不报错，运行时才 404）；
//! * 前端传的 camelCase 参数名对不对得上 Rust 的 snake_case 形参；
//! * 返回值能不能序列化成前端认识的形状。
//!
//! 凡是不依赖库里具体是哪些歌的断言都在这里；`tests/commands.rs` 只留真的要真实数据的
//! （真实封面路径过 asset scope、重扫整个曲库、要 Sudachi 词典的、要联网的）。
//!
//! 这个文件里的测试共用一个 app 和一个库（原因见 `support/mod.rs`），所以**不往库里加歌删歌**——
//! 那类流程在 `tests/fixture_flows.rs`，另一个进程、另一个库。

mod support;

use std::path::Path;

use serde_json::{Value, json};

use jp_app_lib::state::AppState;
use jp_corpus::fixture;
use support::{Fixture, audio_or_skip, has_keys, invoke, len, ok, refused};

// ────────────────────────────── 曲库 ──────────────────────────────

#[test]
fn the_library_commands_all_answer_on_a_fresh_fixture_library() {
    let f = Fixture::new("library");
    let w = f.w();

    let health = ok(w, "health", json!({}));
    assert_eq!(health["tracks"], json!(fixture::TRACKS));
    assert_eq!(health["lyricLines"], json!(fixture::LYRIC_LINES));
    // 库建在临时目录里，不是用户的真库——这一条防的是测试串到真库上去
    assert!(
        health["dbPath"]
            .as_str()
            .unwrap()
            .starts_with(&f.dir().display().to_string()),
        "库指到了别处：{}",
        health["dbPath"]
    );
    assert!(health["dbPath"].as_str().unwrap().ends_with("corpus.db"));

    assert_eq!(len(&ok(w, "list_tracks", json!({}))), fixture::TRACKS as usize);
    assert_eq!(ok(w, "get_track", json!({ "songId": "001" }))["title"], "街の灯");
    assert!(ok(w, "get_track", json!({ "songId": "没有这首" })).is_null());

    assert_eq!(len(&ok(w, "list_albums", json!({}))) as i64, fixture::ALBUMS);
    assert_eq!(len(&ok(w, "album_tracks", json!({ "albumId": 1 }))), 2);
    assert_eq!(len(&ok(w, "track_credits", json!({ "songId": "001" }))), 3);
    assert_eq!(len(&ok(w, "lyrics", json!({ "songId": "001" }))), 4);

    // 2 号在 001 和 002 上都挂着作曲，所以他和 1 号是合作者
    let composers = ok(w, "people_by_role", json!({ "role": "composer" }));
    assert_eq!(len(&composers), 2);
    let person_id = composers[0]["id"].as_i64().unwrap();
    assert!(len(&ok(w, "works_by_person", json!({ "personId": person_id }))) > 0);
    assert!(ok(w, "person_by_id", json!({ "personId": person_id }))["name"].is_string());
    let _ = ok(w, "collaborators", json!({ "personId": person_id }));
}

/// `lyrics` 的形状是歌词页的全部依据：有没有时间轴、有没有分词，
/// 决定了能不能跟随播放、能不能点词查词。
#[test]
fn the_lyrics_payload_carries_the_timeline_and_the_tokens() {
    let f = Fixture::new("lyrics");
    let lines = ok(f.w(), "lyrics", json!({ "songId": "001" }));

    let first = &lines[0];
    assert_eq!(first["lineIdx"], json!(0));
    assert!(first["timeSec"].as_f64().unwrap() > 0.0, "没有时间轴");
    assert_eq!(first["text"], "夜が明ける前に");
    assert_eq!(len(&first["tokens"]), 5);
    assert_eq!(first["tokens"][0]["surface"], "夜");
    assert_eq!(first["tokens"][0]["pos"], "NOUN");

    // 003 有歌词没分词：行要出得来，tokens 是空的——界面据此降级，而不是报错
    let bare = ok(f.w(), "lyrics", json!({ "songId": fixture::UNTOKENIZED_SONG }));
    assert_eq!(len(&bare), 2);
    assert_eq!(len(&bare[0]["tokens"]), 0);
}

// ────────────────────────────── 检索 ──────────────────────────────

#[test]
fn the_search_commands_find_what_is_in_the_fixture() {
    let f = Fixture::new("search");
    let w = f.w();

    // 「風」在 001 和 002 里各一次
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["風"], "field": "lemma" } }));
    assert_eq!(len(&hits), 2);

    // 全文检索走的是 FTS5 外部内容表——没同步的话这里是 0
    assert_eq!(len(&ok(w, "search_lyrics", json!({ "text": "灯り" }))), 1);
    assert_eq!(len(&ok(w, "search_lyrics", json!({ "text": "不存在的词" }))), 0);

    let quick = ok(w, "quick_search", json!({ "query": "街" }));
    assert!(quick.is_object(), "quick_search 的形状变了：{quick}");
}

// ────────────────────────────── 语料 ──────────────────────────────

#[test]
fn the_corpus_commands_count_what_is_actually_there() {
    let f = Fixture::new("corpus");
    let w = f.w();

    let overview = ok(w, "overview", json!({}));
    assert_eq!(overview["tracks"], json!(fixture::TRACKS));
    assert_eq!(overview["lyricLines"], json!(fixture::LYRIC_LINES));

    assert!(len(&ok(w, "timeline", json!({}))) > 0);
    assert!(len(&ok(w, "word_frequency", json!({ "limit": 10 }))) > 0);

    let word = ok(w, "word_in_corpus", json!({ "lemma": "風" }));
    assert_eq!(word["occurrences"], json!(2));
    assert_eq!(word["songCount"], json!(2));

    // 统计是 async command（走 spawn_blocking），IPC 上也要通
    assert!(len(&ok(w, "stats_frequency", json!({ "limit": 20 }))) > 0);
    assert!(ok(w, "stats_report", json!({}))["text"].is_string());
}

// ──────────────────────── 写：收藏 / 历史 / 校正 ────────────────────────

#[test]
fn toggling_a_favorite_goes_both_ways() {
    let f = Fixture::new("favorite");
    let w = f.w();

    // 类型是 `song`：前端发的就是它，收藏列表也只认它
    let song = |id: &str| json!({ "entityType": "song", "entityId": id });
    let favorites = || -> Vec<String> {
        ok(w, "home_summary", json!({}))["favorites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect()
    };
    // 001 在 fixture 里本来就是收藏的
    assert_eq!(ok(w, "is_favorite", song("001")), json!(true));
    assert_eq!(favorites(), vec!["001"], "收藏了就该在首页的收藏列表里");

    assert_eq!(ok(w, "toggle_favorite", song("002")), json!(true));
    assert_eq!(favorites(), vec!["002", "001"], "刚收藏的排最前");
    assert_eq!(ok(w, "toggle_favorite", song("002")), json!(false));
    assert_eq!(ok(w, "is_favorite", song("002")), json!(false));

    assert_eq!(ok(w, "toggle_favorite", song("001")), json!(false));
    assert!(favorites().is_empty());
    // 切回去，别的测试还要用
    assert_eq!(ok(w, "toggle_favorite", song("001")), json!(true));
}

#[test]
fn a_play_event_lands_in_the_history() {
    let f = Fixture::new("history");
    let w = f.w();

    let before = len(&ok(w, "recently_played", json!({})));
    ok(
        w,
        "record_play",
        json!({ "event": {
            "songId": "003", "listenedSec": 120.0, "positionSec": 130.0,
            "completed": false, "source": "library"
        }}),
    );
    assert_eq!(len(&ok(w, "recently_played", json!({}))), before + 1);
    // 听满 30 秒才算「常听」
    assert!(len(&ok(w, "most_played", json!({}))) > 0);

    let home = ok(w, "home_summary", json!({}));
    assert_eq!(home["overview"]["tracks"], json!(fixture::TRACKS));
    assert!(home["albums"].is_array() && home["favorites"].is_array());
}

#[test]
fn a_token_correction_can_be_saved_and_reverted() {
    let f = Fixture::new("correction");
    let w = f.w();

    let lines = ok(w, "lyrics", json!({ "songId": "001" }));
    let utterance_id = lines[0]["utteranceId"].as_i64().unwrap();

    // 行存在就一定返回一份视图，**不会是 null**；是否校正过看 `corrected`。
    // （一开始我按「没校正过就返回 null」写断言，实测不是——这是前端
    //   判断「这一行改过没有」的唯一依据，所以值得钉住。）
    let fresh = ok(w, "token_correction", json!({ "utteranceId": utterance_id }));
    assert_eq!(fresh["corrected"], json!(false));
    assert_eq!(len(&fresh["tokens"]), 5);
    assert_eq!(fresh["text"], "夜が明ける前に");

    ok(
        w,
        "save_token_correction",
        json!({ "utteranceId": utterance_id, "tokens": [
            { "surface": "夜が", "lemma": "夜", "pos": "NOUN" },
            { "surface": "明ける前に", "lemma": "明ける", "pos": "VERB" }
        ]}),
    );
    let view = ok(w, "token_correction", json!({ "utteranceId": utterance_id }));
    assert_eq!(view["corrected"], json!(true));
    assert_eq!(len(&view["tokens"]), 2, "校正没存上：{view}");
    // 原始分词要留着，撤销时才回得去
    assert_eq!(len(&view["original"]), 5);
    // 改完要立刻能查到新词，而且 KWIC 要标出这一行校正过（界面上的 ✏）
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["明ける"], "field": "lemma" } }));
    assert_eq!(len(&hits), 1);
    assert_eq!(hits[0]["keyword"], "明ける前に");
    assert_eq!(hits[0]["corrected"], json!(true));

    assert_eq!(
        ok(w, "revert_token_correction", json!({ "utteranceId": utterance_id })),
        json!(true)
    );
    let back = ok(w, "token_correction", json!({ "utteranceId": utterance_id }));
    assert_eq!(back["corrected"], json!(false));
    assert_eq!(back["tokens"], fresh["tokens"], "撤销没把分词还原回去");
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["明ける"], "field": "lemma" } }));
    assert_eq!(hits[0]["corrected"], json!(false));
    // 撤销第二次没东西可撤
    assert_eq!(
        ok(w, "revert_token_correction", json!({ "utteranceId": utterance_id })),
        json!(false)
    );
}

// ────────────────────────── 维护与能力 ──────────────────────────

/// 「有歌词但一个 token 都没有」要被找出来——这正是用户报的
/// 「有的歌开了振假名既无法查词也不显示振假名」。
#[test]
fn the_untokenized_song_shows_up_in_the_maintenance_list() {
    let f = Fixture::new("untokenized");
    let missing = ok(f.w(), "tokenize_missing_list", json!({}));
    assert_eq!(len(&missing), 1, "{missing}");
    assert_eq!(missing[0]["songId"], fixture::UNTOKENIZED_SONG);
    assert_eq!(missing[0]["lyricLines"], json!(2));
}

/// 临时目录里没有词典，分词器就该如实说「没有」，而不是报错或者假装有。
#[test]
fn a_library_without_a_dictionary_reports_itself_as_not_ready() {
    let f = Fixture::new("nodict");
    let w = f.w();

    let status = ok(w, "tokenizer_status", json!({}));
    assert_eq!(status["ready"], json!(false));
    assert_eq!(status["source"], "none");

    // 没有词典时补分词要给一句能照着做的话，而不是一个底层错误
    let err = invoke(w, "tokenize_missing_run", json!({})).expect_err("没有词典却成功了");
    let message = err["message"].as_str().unwrap_or_default();
    assert!(message.contains("词典"), "{message}");

    assert_eq!(ok(w, "health", json!({}))["tokenizerReady"], json!(false));
}

/// 「没有词典」那半句话是 Rust 和前端之间的**契约**：
/// `App.tsx` 用 `notice.includes(NO_DICTIONARY)` 决定把红条换成「下载词典」横幅。
/// 改一个字横幅就静默消失，没有任何东西会报错——所以在这儿钉住三处。
#[test]
fn the_no_dictionary_wording_is_the_same_on_both_sides() {
    let f = Fixture::new("wording");
    let w = f.w();
    let phrase = jp_app_lib::tokenizer::NO_DICTIONARY;

    // 1+2. 两条会撞上「没有词典」的命令，报错里都得有这半句
    for (cmd, args) in [
        ("lyrics_furigana", json!({ "songId": "001", "mode": "kanji" })),
        ("tokenize", json!({ "text": "夜が明ける" })),
    ] {
        let err = invoke(w, cmd, args)
            .err()
            .unwrap_or_else(|| panic!("{cmd} 在没有词典时居然成功了"));
        let message = err["message"].as_str().unwrap_or_default();
        assert!(message.contains(phrase), "{cmd} 的报错里没有这半句：{message}");
    }

    // 3. 前端那一份常量
    let api_ts = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .join("app/src/api.ts");
    let source = std::fs::read_to_string(&api_ts)
        .unwrap_or_else(|err| panic!("读不了 {}：{err}", api_ts.display()));
    assert!(
        source.contains(&format!("export const NO_DICTIONARY = \"{phrase}\"")),
        "app/src/api.ts 里的 NO_DICTIONARY 和 Rust 这边对不上（Rust 是「{phrase}」）"
    );
}

/// 诊断信息是出事时唯一的线索，所以它自己不能出事。
/// 变调缓存的上限：设置页改了之后立刻按新上限清一遍，从最久没用的删起；
/// 报回来的占用是清完之后的。缓存目录在语料库里，不是别处。
#[test]
fn lowering_the_pitch_cache_limit_evicts_the_least_recently_used() {
    let f = Fixture::new("pitch-cache");
    let w = f.w();
    let dir = f.dir().join("output").join("pitch_cache");
    std::fs::create_dir_all(&dir).unwrap();
    let mb = 1024 * 1024;
    // 三个 100 MB 的「缓存」（稀疏文件：只设长度，不真写 300 MB），最近使用时间一个比一个新
    for (name, hours_ago) in [("a_+1_r3_00000000000000a1.wav", 3u64), ("b_+1_r3_00000000000000b2.wav", 2), ("c_-2_r3_00000000000000c3.wav", 1)] {
        let file = std::fs::File::create(dir.join(name)).unwrap();
        file.set_len(100 * mb).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(hours_ago * 3600)).unwrap();
    }

    let status = ok(w, "pitch_cache_status", json!({}));
    assert_eq!(status["files"], json!(3), "{status}");
    assert_eq!(status["bytes"], json!(300 * mb), "{status}");
    assert_eq!(status["limitBytes"], json!(1536 * mb), "默认上限 1.5 GB：{status}");
    assert!(Path::new(status["dir"].as_str().unwrap()).starts_with(f.dir()), "{status}");

    // 上限最小只能设到 256 MB：300 MB 的缓存要清掉最旧的那一个
    let status = ok(w, "pitch_cache_set_limit", json!({ "limitMb": 1 }));
    assert_eq!(status["limitBytes"], json!(256 * mb), "低于下限的要夹到下限：{status}");
    assert_eq!(status["files"], json!(2), "{status}");
    assert_eq!(status["bytes"], json!(200 * mb), "{status}");
    assert!(!dir.join("a_+1_r3_00000000000000a1.wav").exists(), "最久没用的该被清掉");
    assert!(dir.join("b_+1_r3_00000000000000b2.wav").exists() && dir.join("c_-2_r3_00000000000000c3.wav").exists());

    // 别的测试还要用这个库：上限放回默认，缓存清干净
    ok(w, "pitch_cache_set_limit", json!({ "limitMb": 1536 }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_diagnostics_report_describes_this_library() {
    let f = Fixture::new("diagnostics");
    let text = ok(f.w(), "diagnostics_report", json!({}));
    let text = text.as_str().expect("不是字符串");

    // 这几个段落标题是契约：用户贴过来的那段文本靠它们定位，少一段就等于又要多问一轮
    for marker in [
        "JPOP Corpus Tool 诊断信息",
        "── 语料库 ──",
        "── 能力 ──",
        "── 日志",
        "corpus.db",
        "分词词典",
        "音频输出",
        "有歌词没分词 1 首",
    ] {
        assert!(text.contains(marker), "诊断信息里少了「{marker}」：\n{text}");
    }
    // 不能带歌名、歌手、歌词
    for secret in ["街の灯", "夜明バンド", "夜が明ける前に"] {
        assert!(!text.contains(secret), "诊断信息里出现了 {secret}");
    }

    // 存一份到文件，确认写得出来、内容一致
    let out = std::env::temp_dir().join(format!("jp-diag-{}.txt", std::process::id()));
    let saved = ok(f.w(), "diagnostics_save", json!({ "path": out.to_string_lossy() }));
    assert_eq!(saved.as_str().unwrap(), out.to_string_lossy());
    assert!(std::fs::read_to_string(&out).unwrap().contains("── 能力 ──"), "存出来的文件不对");
    let _ = std::fs::remove_file(&out);
}

// ────────────────────────────── 导入 ──────────────────────────────

/// 没扫描就导入、扫一个不存在的路径——两种错都要是**能看懂的一句话**，
/// 而不是一个底层错误或者一次 panic。
#[test]
fn the_import_commands_refuse_nonsense_with_a_readable_message() {
    let f = Fixture::new("import");
    let w = f.w();

    // 取消之后再导入，和从来没扫过是一回事
    ok(w, "cancel_import", json!({}));
    let message = refused(w, "run_import", json!({}));
    assert!(message.contains("scan_folder"), "错误信息要告诉调用方下一步做什么：{message}");
    let message = refused(w, "scan_folder", json!({ "path": "Z:/根本没有这个目录" }));
    assert!(message.contains("不存在"), "{message}");
    let message = refused(w, "scan_files", json!({ "paths": [] }));
    assert!(message.contains("一个文件都没选"), "{message}");

    // 取消是清掉待确认的那批，任何时候调都该成功
    ok(w, "cancel_import", json!({}));
}

/// 命令名写错了编译期不报错，运行时才 404。这里确认「不存在的命令」
/// 确实是失败的——否则上面所有 `ok(...)` 都证明不了命令真的注册上了。
#[test]
fn an_unregistered_command_is_rejected() {
    let f = Fixture::new("unknown");
    assert!(
        invoke(f.w(), "这个命令不存在", json!({})).is_err(),
        "不存在的命令居然成功了，说明这组测试证明不了任何事"
    );
}

// ────────────────────────────── 更新器 ──────────────────────────────

/// `update_install` 拉起的是一个 exe，**路径不能由前端决定**：
/// 前端（或者任何能往 webview 里注入脚本的东西）传一个路径进来就能让程序执行它。
/// 只该启动 `update_download` 这一轮亲手下好、校验过的那一个文件；没有就拒绝。
#[test]
fn update_install_never_runs_a_path_handed_in_by_the_frontend() {
    let f = Fixture::new("update-install");
    // 一个真实存在的文件：老代码会把它当安装包拉起来
    let bait = f.dir().join("bait-setup.exe");
    std::fs::write(&bait, b"not an installer").unwrap();

    let err = invoke(f.w(), "update_install", json!({ "path": bait.display().to_string() }))
        .expect_err("没下载过却拉起了安装程序");
    let message = err.to_string();
    assert!(message.contains("还没有下载好"), "拒绝的原因要说清楚：{message}");

    let err = invoke(f.w(), "update_install", json!({})).expect_err("没下载过却拉起了安装程序");
    assert!(err.to_string().contains("还没有下载好"), "{err}");
}

/// 前端传回来的资产（地址、文件名、哈希）一概不认：下什么只由后端那次检查决定。
/// 以前整条 `AssetView` 由前端回传——期望的哈希是调用方自己给的，地址可以指向旧版本，
/// 文件名可以带 `..\` 写出缓存目录。
#[test]
fn update_download_ignores_whatever_asset_the_frontend_sends() {
    use tauri::Manager;
    let f = Fixture::new("update-download");
    let forged = json!({
        "version": "99.0.0",
        "asset": {
            "name": r"..\..\evil-setup.exe",
            "size": 1,
            "url": "https://github.com/Yisoragoto/jpop-corpus-tool/releases/download/v0.1.0/x-setup.exe",
            "sha256": "00".repeat(32),
        }
    });
    let err = invoke(f.w(), "update_download", forged.clone()).expect_err("没检查过更新却开始下载了");
    assert!(err.to_string().contains("先检查更新"), "{err}");

    // 检查到的最新版比正在跑的还旧：不下
    let current = f.w().app_handle().package_info().version.to_string();
    let old = jp_app_lib::update::ReleaseView {
        version: "0.0.1".into(),
        tag: "v0.0.1".into(),
        name: "0.0.1".into(),
        notes: String::new(),
        published_at: String::new(),
        url: String::new(),
        prerelease: false,
        installer: Some(jp_app_lib::update::AssetView {
            name: "JPOP.Corpus.Tool_0.0.1_x64-setup.exe".into(),
            size: 1,
            url: format!("{}v0.0.1/JPOP.Corpus.Tool_0.0.1_x64-setup.exe", jp_app_lib::update::DOWNLOAD_PREFIX),
            sha256: Some("00".repeat(32)),
        }),
    };
    f.w().state::<AppState>().updates().remember_check(&jp_app_lib::update::UpdateStatus {
        current: current.clone(),
        latest: Some(old),
        update_available: false,
    });
    let err = invoke(f.w(), "update_download", json!({ "version": "0.0.1" })).expect_err("降级了");
    assert!(err.to_string().contains("降级"), "当前 {current}：{err}");
    // 版本号对不上检查到的那一次
    let err = invoke(f.w(), "update_download", forged).expect_err("下了没检查过的版本");
    assert!(err.to_string().contains("不是刚才检查到的"), "{err}");
}

// ──────────────────────────── 锁与网络 ────────────────────────────

/// 会联网的刮削命令不能占着共享的 `corpus()` 锁：识别要走 MusicBrainz、封面一张十几秒，
/// 这段时间里翻曲库、查词、播放统计全都排在后面。它们要像后台作业那样自己开连接。
///
/// 测法：测试线程自己攥住 `corpus()` 锁，再发命令。参数故意给成查不到的，
/// 命令在联网之前就会报错返回——前提是它不去等那把锁。老代码会一直卡在锁上，
/// 所以等 5 秒没回来就判红，而不是让整个测试挂死。
#[test]
fn network_scrape_commands_do_not_wait_on_the_shared_corpus_lock() {
    use tauri::Manager;
    let f = Fixture::new("scrape-lock");
    let w = f.w();
    let state = w.state::<AppState>();
    let held = state.corpus();

    for (cmd, args) in [
        ("scrape_track", json!({ "songId": "不存在的歌" })),
        ("scrape_accept", json!({ "filePath": "Z:/不存在.flac", "candidateIndex": 0 })),
    ] {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(invoke(w, cmd, args).is_err());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(failed) => assert!(failed, "{cmd} 用查不到的参数居然成功了"),
            Err(_) => panic!("{cmd} 在等 corpus() 锁：联网的命令不该占着共享连接"),
        }
    }
    drop(held);
}

// ═══════════════════════════════════════════════════════════════════════
// 下面这些原来在 `tests/commands.rs` 里，跑在作者本机的真实库上，CI 上整组跳过。
// 它们验的是「这条路通不通、形状对不对」，不依赖库里具体是哪些歌，所以搬到这里。
// 搬过来之后前提都是已知的：原来那些「库里没有就 return」的分支一律去掉了。
// ═══════════════════════════════════════════════════════════════════════

// ────────────────────────── 注册与参数名 ──────────────────────────

#[test]
fn every_registered_command_is_reachable() {
    let f = Fixture::new("reachable");
    // 每个 command 至少要能被调到。参数不全导致的业务错误可以接受，
    // 「命令不存在」不行——那说明 generate_handler! 里漏了。
    for (cmd, args) in [
        ("overview", json!({})),
        ("timeline", json!({})),
        ("list_tracks", json!({ "limit": 1 })),
        ("list_albums", json!({})),
        ("word_frequency", json!({ "limit": 1 })),
        ("recently_played", json!({ "limit": 1 })),
        ("most_played", json!({ "limit": 1 })),
        ("people_by_role", json!({ "role": "composer", "limit": 1 })),
        // 只放只读的；保存和撤销在上面单独测
        ("token_correction", json!({ "utteranceId": 1 })),
        // 只数一遍缓存目录，不写任何东西
        ("pitch_cache_status", json!({})),
    ] {
        ok(f.w(), cmd, args);
    }
}

#[test]
fn camel_case_arguments_reach_snake_case_parameters() {
    let f = Fixture::new("camel");
    let w = f.w();
    // 这是最容易翻车的一处：前端写 songId，Rust 形参是 song_id。
    // Tauri 默认按 camelCase 查找，所以 api.ts 必须用 camelCase。
    let track = ok(w, "get_track", json!({ "songId": "001" }));
    assert_eq!(track["id"], json!("001"));

    // snake_case 传参应当取不到值——用它反证上面那条不是巧合
    let missing = invoke(w, "get_track", json!({ "song_id": "001" }));
    assert!(
        missing.is_err() || missing.unwrap().is_null(),
        "snake_case 参数名不该被接受，否则说明约定和文档不符"
    );
}

#[test]
fn multi_word_arguments_convert_too() {
    let f = Fixture::new("multi-word");
    let w = f.w();
    // min_listened_sec -> minListenedSec，两段以上的名字也要对得上。
    // 光看「返回的是数组」证明不了参数传到了：门槛调到没人够得着，结果就该是空的
    assert!(len(&ok(w, "most_played", json!({ "minListenedSec": 0.0, "limit": 3 }))) > 0);
    assert_eq!(len(&ok(w, "most_played", json!({ "minListenedSec": 1.0e9, "limit": 3 }))), 0);
}

#[test]
fn optional_arguments_can_be_omitted() {
    let f = Fixture::new("optional");
    // Option<T> 形参不传时要走默认值，而不是报「缺参数」
    assert!(ok(f.w(), "list_tracks", json!({})).is_array());
    assert!(ok(f.w(), "word_frequency", json!({})).is_array());
}

// ────────────────────────── 序列化形状 ──────────────────────────

#[test]
fn track_is_camel_case_for_the_frontend() {
    let f = Fixture::new("track-shape");
    let track = &ok(f.w(), "list_tracks", json!({ "limit": 1 }))[0];
    has_keys(track, "Track", &["id", "title", "artist", "album", "albumId", "durationSec", "lineCount"]);
    // snake_case 泄漏出去就说明某个结构体漏了 rename_all
    assert!(track.get("album_id").is_none(), "不该出现 snake_case 字段");
}

#[test]
fn overview_matches_the_typescript_interface() {
    let f = Fixture::new("overview-shape");
    has_keys(
        &ok(f.w(), "overview", json!({})),
        "Overview",
        &[
            "tracks",
            "albums",
            "people",
            "performers",
            "composers",
            "lyricists",
            "arrangers",
            "lyricLines",
            "tokens",
            "vocabulary",
            "totalDurationSec",
        ],
    );
}

#[test]
fn nullable_fields_serialize_as_null_not_absent() {
    let f = Fixture::new("nullable");
    // 003 还没读过时长。它的 durationSec 必须是 null 而不是缺字段，
    // 否则 TS 那边 `durationSec: number | null` 的类型就是假的。
    let bare = ok(f.w(), "get_track", json!({ "songId": fixture::UNTOKENIZED_SONG }));
    assert!(bare.as_object().unwrap().contains_key("durationSec"), "{bare}");
    assert!(bare["durationSec"].is_null(), "{bare}");
    assert_eq!(ok(f.w(), "get_track", json!({ "songId": "001" }))["durationSec"], json!(262.0));
}

// ────────────────────────── 竖切回路 ──────────────────────────

#[test]
fn the_research_loop_works_end_to_end() {
    let f = Fixture::new("research-loop");
    let w = f.w();

    // 1. 曲库 → 2. 歌词（带分词）→ 3. 从歌词里挑一个实词。
    //    003 有歌词没分词，所以是「第一首挑得出实词的」，不是「第一首有歌词的」
    let tracks = ok(w, "list_tracks", json!({ "limit": 200 }));
    let (song_id, lemma) = tracks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["lineCount"].as_i64().unwrap_or(0) > 0)
        .find_map(|t| {
            let song_id = t["id"].as_str().unwrap().to_string();
            let lyrics = ok(w, "lyrics", json!({ "songId": &song_id }));
            let lemma = lyrics
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|l| l["tokens"].as_array().unwrap())
                .find(|t| matches!(t["pos"].as_str().unwrap_or(""), "NOUN" | "VERB" | "ADJ" | "ADV" | "PROPN"))
                .map(|t| t["lemma"].as_str().unwrap().to_string())?;
            Some((song_id, lemma))
        })
        .expect("应当至少有一首挑得出实词的歌");

    // 4. 点词 → 查全语料
    let word = ok(w, "word_in_corpus", json!({ "lemma": &lemma, "currentSongId": &song_id, "limit": 10 }));
    assert_eq!(word["lemma"], json!(lemma));
    assert!(word["occurrences"].as_i64().unwrap() > 0);

    // 5. 例句能跳回具体曲目——这是回路闭合的关键。正在看的那首排最前
    let examples = word["examples"].as_array().unwrap();
    assert!(!examples.is_empty(), "应当有例句，否则跳转无从谈起");
    assert_eq!(examples[0]["songId"], json!(song_id));
    let jumped = ok(w, "get_track", json!({ "songId": examples[0]["songId"] }));
    assert_eq!(jumped["id"], examples[0]["songId"]);
}

#[test]
fn credits_expose_the_new_dimensions() {
    let f = Fixture::new("credits");
    let w = f.w();
    // 作曲/作词是从 LRC 里捡回来的，command 层要能透出去
    let composers = ok(w, "people_by_role", json!({ "role": "composer", "limit": 5 }));
    has_keys(&composers[0], "PersonSummary", &["id", "name", "role", "trackCount", "imagePath"]);
    // 作品最多的排最前：2 号给三首歌作了曲
    assert_eq!(composers[0]["name"], "海野 透");
    assert_eq!(composers[0]["trackCount"], json!(3));

    let peers = ok(w, "collaborators", json!({ "personId": composers[0]["id"], "limit": 5 }));
    assert_eq!(len(&peers), 3, "{peers}");
}

#[test]
fn kwic_accepts_the_query_struct() {
    let f = Fixture::new("kwic-struct");
    let w = f.w();
    // KwicQuery 是嵌套结构体，序列化形状最容易和 TS 那边对不上
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 5 } }));
    assert_eq!(len(&hits), 3);
    has_keys(&hits[0], "KwicHit", &["songId", "left", "keyword", "right", "timeSec"]);

    // 只看日文：数字、符号的关键词筛掉，而且是筛完才截断
    let mixed = json!(["夜", "100", "%"]);
    let all = ok(w, "kwic", json!({ "query": { "keywords": mixed, "field": "surface" } }));
    assert!(all.as_array().unwrap().iter().any(|h| h["keyword"] == "100"), "{all}");
    let hits = ok(w, "kwic", json!({ "query": { "keywords": mixed, "field": "surface", "jpOnly": true, "limit": 2 } }));
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 2, "筛完还有三条，截到两条");
    assert!(hits.iter().all(|h| h["keyword"] == "夜"), "{hits:?}");
}

#[test]
fn stats_commands_filter_and_carry_the_report_text() {
    let f = Fixture::new("stats");
    let w = f.w();
    let rows = ok(w, "stats_frequency", json!({ "limit": 50 }));
    let rows = rows.as_array().unwrap();
    assert!(!rows.is_empty() && rows.len() <= 50);
    has_keys(&rows[0], "统计表", &["lemma", "pos", "freq", "songCount", "surfaces", "jlpt"]);
    // 词元 × 词性按频次降序，排除标点和符号
    assert!(rows.windows(2).all(|p| p[0]["freq"].as_i64() >= p[1]["freq"].as_i64()));
    assert!(rows.iter().all(|r| r["pos"] != "PUNCT" && r["pos"] != "SYM"));
    // JLPT 等级库里有带前缀和不带前缀两种写法，出来都是 N5 这种
    let jlpt = |lemma: &str| rows.iter().find(|r| r["lemma"] == lemma).unwrap_or_else(|| panic!("没有「{lemma}」"))["jlpt"].clone();
    assert_eq!(jlpt("夜"), "N5");
    assert_eq!(jlpt("風"), "N4");
    assert_eq!(jlpt("雨"), "", "没有等级的是空串，不是缺字段");

    // 限定到一位演唱者：夜明バンド 唱了 001 和 002
    let person = &ok(w, "people_by_role", json!({ "role": "performer", "limit": 1 }))[0];
    assert_eq!(person["name"], "夜明バンド");
    let filter = json!({ "performerIds": [person["id"]], "jpOnly": true });
    let nouns = ok(w, "stats_frequency", json!({ "pos": "NOUN", "filter": filter, "limit": 20 }));
    let nouns = nouns.as_array().unwrap();
    assert!(!nouns.is_empty());
    assert!(nouns.iter().all(|r| r["pos"] == "NOUN"));
    assert!(nouns.iter().all(|r| r["lemma"] != "雨"), "「雨」只在 Akari 的歌里：{nouns:?}");

    let everything = ok(w, "stats_report", json!({}));
    assert_eq!(everything["performers"], json!([]));
    assert!(everything["text"].as_str().unwrap().contains("フィルター：全歌手\r\n日本語のみ：なし\r\n"));

    let report = ok(w, "stats_report", json!({ "filter": filter }));
    assert_eq!(report["performers"], json!(["夜明バンド"]));
    assert_eq!(report["jpOnly"], true);
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("フィルター：夜明バンド\r\n日本語のみ：あり\r\n"), "{text}");
    let songs = report["report"]["songCount"].as_i64().unwrap();
    assert_eq!(songs, 2);
    assert!(songs < everything["report"]["songCount"].as_i64().unwrap());
    assert!(text.contains("  曲数                : 2"), "{text}");
    has_keys(
        &report["report"],
        "报告",
        &["ttr", "sttr", "sttrChunk", "hapaxRatio", "posDist", "coverage", "topWords"],
    );

    // 导出：只认 txt / csv / html
    let dir = std::env::temp_dir().join(format!("jp-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("レポート.txt");
    ok(w, "export_text", json!({ "path": target.display().to_string(), "content": text }));
    assert_eq!(std::fs::read(&target).unwrap(), text.as_bytes());
    let refused = invoke(w, "export_text", json!({ "path": dir.join("x.exe").display().to_string(), "content": "" }));
    assert!(refused.is_err() && !dir.join("x.exe").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ────────────────────────── 错误路径 ──────────────────────────

#[test]
fn errors_carry_a_readable_message() {
    let f = Fixture::new("errors");
    let w = f.w();
    // CommandError 序列化成 { message }——api.ts 的 call() 靠它转成 Error。
    // 原来这条用的是一个其实不会失败的调用，`if let Err` 里的断言从来没执行过
    for (cmd, args, expected) in [
        ("audio_load", json!({ "songId": "不存在" }), "不存在"),
        ("run_import", json!({}), "scan_folder"),
        ("scrape_accept", json!({ "filePath": "X:/nope.flac", "candidateIndex": 0 }), "刮削记录"),
        ("lyrics_fill_one", json!({ "songId": "001" }), "已经有歌词"),
    ] {
        let message = refused(w, cmd, args);
        assert!(message.contains(expected), "{cmd} 的报错没说清是什么问题：{message}");
    }
}

// ────────────────────────── 播放 ──────────────────────────
//
// 音频引擎在无声卡环境下是 None，这时播放类 command 应当**报错**而不是假装成功。
// 状态查询永远不该报错；要真的出声的那几条在没有设备时跳过并说一声。
//
// 共用一个 app，所以每条都把自己改的倍速、音量、循环区间放回去。

#[test]
fn health_reports_audio_capability_and_nothing_about_ffmpeg() {
    let f = Fixture::new("health-audio");
    let health = ok(f.w(), "health", json!({}));
    // audioReady 反映的是「有没有可用输出设备」，不是「功能做没做」。
    // 无声卡环境（CI、远程桌面）应当是 false 且其余功能照常。
    assert!(health["audioReady"].is_boolean(), "{health}");
    // 变调和 Anki 片段都在进程里做了：health 里不该再有「有没有 ffmpeg」这类字段，
    // 前端也就没有理由再因为缺 ffmpeg 把哪个功能禁掉
    for gone in ["pitchSupported", "ffmpegPath", "ffmpegExpected"] {
        assert!(health.get(gone).is_none(), "health 里还有 {gone}：{health}");
    }
}

#[test]
fn audio_state_and_tick_have_the_same_shape_even_without_a_device() {
    let f = Fixture::new("audio-state");
    let w = f.w();
    // 状态查询永远不该报错——UI 不该为了画一个空进度条去处理异常
    let state = ok(w, "audio_state", json!({}));
    has_keys(&state, "PlaybackState", &["songId", "playState", "positionSec", "rate", "volume", "loopRegion"]);
    // audio_tick 有副作用（累加收听时长），但返回结构必须一致，否则前端换用它的时候会静默出错
    let tick = ok(w, "audio_tick", json!({}));
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&tick), keys(&state), "两个命令的返回结构应当相同");
}

#[test]
fn spectrum_is_readable_even_without_a_device() {
    let f = Fixture::new("spectrum");
    let spectrum = ok(f.w(), "audio_spectrum", json!({}));
    let bands = spectrum.as_array().expect("应当返回数组");
    assert!(!bands.is_empty());
    for band in bands {
        let v = band.as_f64().unwrap();
        assert!((0.0..=1.0).contains(&v), "频段值越界: {v}");
    }
}

#[test]
fn loading_an_unknown_track_reports_which_one() {
    let f = Fixture::new("load-unknown");
    let message = refused(f.w(), "audio_load", json!({ "songId": "没有这首" }));
    assert!(message.contains("没有这首"), "错误信息应当指明是哪一首: {message}");
}

#[test]
fn setting_the_pitch_with_nothing_loaded_only_remembers_it() {
    let f = Fixture::new("pitch");
    let w = f.w();
    if !audio_or_skip(w, "setting_the_pitch_with_nothing_loaded_only_remembers_it") {
        return;
    }
    // 没加载歌时只记住半音数，不渲染
    ok(w, "audio_stop", json!({}));
    ok(w, "audio_set_pitch", json!({ "semitones": 9 }));
    let state = ok(w, "audio_state", json!({}));
    assert_eq!(state["pitchSemitones"], json!(6), "超出范围要钳到 +6");
    assert_eq!(state["pitchRendering"], json!(false));
    assert_eq!(state["pitchError"], Value::Null);
    ok(w, "audio_set_pitch", json!({ "semitones": 0 }));
    assert_eq!(ok(w, "audio_state", json!({}))["pitchSemitones"], json!(0));
}

#[test]
fn playback_controls_are_wired() {
    let f = Fixture::new("controls");
    let w = f.w();
    if !audio_or_skip(w, "playback_controls_are_wired") {
        return;
    }
    // 每个控制都要能调到。没加载音频时调它们不该 panic。
    for cmd in ["audio_play", "audio_pause", "audio_toggle", "audio_stop"] {
        ok(w, cmd, json!({}));
    }
}

#[test]
fn rate_and_volume_round_trip_through_ipc() {
    let f = Fixture::new("rate-volume");
    let w = f.w();
    if !audio_or_skip(w, "rate_and_volume_round_trip_through_ipc") {
        return;
    }
    ok(w, "audio_set_rate", json!({ "rate": 0.75 }));
    assert_eq!(ok(w, "audio_state", json!({}))["rate"], json!(0.75));
    // 越界值应当被钳到合法范围，而不是原样接受
    ok(w, "audio_set_rate", json!({ "rate": 99.0 }));
    let rate = ok(w, "audio_state", json!({}))["rate"].as_f64().unwrap();
    assert!(rate <= 2.0, "倍速应当被钳制: {rate}");

    ok(w, "audio_set_volume", json!({ "volume": 0.3 }));
    let volume = ok(w, "audio_state", json!({}))["volume"].as_f64().unwrap();
    assert!((volume - 0.3).abs() < 0.02);

    ok(w, "audio_set_rate", json!({ "rate": 1.0 }));
    ok(w, "audio_set_volume", json!({ "volume": 1.0 }));
}

#[test]
fn loop_region_round_trips_and_rejects_degenerate_spans() {
    let f = Fixture::new("loop");
    let w = f.w();
    if !audio_or_skip(w, "loop_region_round_trips_and_rejects_degenerate_spans") {
        return;
    }
    assert_eq!(ok(w, "audio_set_loop", json!({ "startSec": 10.0, "endSec": 20.0 })), json!(true));
    let region = &ok(w, "audio_state", json!({}))["loopRegion"];
    assert_eq!(region["startSec"], json!(10.0));
    assert_eq!(region["endSec"], json!(20.0));

    // 过短的区间要被拒绝并如实返回 false
    assert_eq!(ok(w, "audio_set_loop", json!({ "startSec": 10.0, "endSec": 10.01 })), json!(false));

    // 两个参数都省略 = 取消循环
    ok(w, "audio_set_loop", json!({}));
    assert!(ok(w, "audio_state", json!({}))["loopRegion"].is_null());
}

#[test]
fn font_catalog_lists_installed_and_bundled_fonts() {
    let f = Fixture::new("fonts");
    // 自带的字体是在语料库目录的 `assets/fonts` 下找的。把仓库里那份放一个进去
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/fonts/KleeOne-SemiBold.ttf");
    let fonts_dir = f.dir().join("assets").join("fonts");
    std::fs::create_dir_all(&fonts_dir).unwrap();
    std::fs::copy(&bundled, fonts_dir.join("KleeOne-SemiBold.ttf"))
        .unwrap_or_else(|err| panic!("仓库里应当有 {}：{err}", bundled.display()));

    let catalog = ok(f.w(), "fonts_catalog", json!({ "imported": ["不存在的字体.ttf"] }));
    let _ = std::fs::remove_dir_all(f.dir().join("assets"));

    let files = catalog["files"].as_array().unwrap();
    assert!(
        files.iter().any(|file| file["family"] == "Klee One" && file["bundled"] == json!(true)),
        "{files:?}"
    );
    let installed = catalog["installed"].as_array().unwrap();
    assert!(installed.iter().all(|font| font["name"].is_string() && font["aliases"].is_array()));
    // 原来断言的是「Windows 上应该有 Meiryo」。CI 的 runner 是不带日文字体的 Windows，
    // 所以改成：字体文件在这台机器上，名单里就得有它
    if Path::new(r"C:\Windows\Fonts\meiryo.ttc").is_file() {
        assert!(installed.iter().any(|font| font["name"] == "Meiryo"), "装着 Meiryo，名单里却没有");
    } else {
        eprintln!("[skip] 这台机器没有 Meiryo，只验了名单的形状");
    }
    // 导入过但文件已经不在的字体要报出来，不能悄悄少一个
    let problems = catalog["problems"].as_array().unwrap();
    assert!(
        problems.iter().any(|p| p.as_str().unwrap().contains("不存在的字体.ttf")),
        "{problems:?}"
    );
}

// ────────────────────────── 页面依赖的返回结构 ──────────────────────────
//
// jp-corpus 已经测过这些查询对不对；这里测的是**序列化后的形状**能不能
// 对上 app/src/api.ts 里的 TS 接口。字段名写错在 Rust 侧编译得过，
// 到前端才变成 undefined。

#[test]
fn kwic_hit_has_every_field_the_page_renders() {
    let f = Fixture::new("kwic-shape");
    let hits = ok(f.w(), "kwic", json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 20 } }));
    has_keys(
        &hits[0],
        "KwicHit",
        &["songId", "artist", "title", "utteranceId", "timeSec", "left", "keyword", "right", "repeatCount", "corrected"],
    );
    // 关键词必须非空——KWIC 视图靠它居中对齐
    assert_eq!(hits[0]["keyword"], "夜");
}

#[test]
fn kwic_options_are_all_wired_through_ipc() {
    let f = Fixture::new("kwic-options");
    let w = f.w();
    // 页面上的每个开关都要真的传到后端：光看「返回的是数组」证明不了，
    // 所以每个开关都挑一个打开和关上结果不一样的例子
    let count = |query: Value| len(&ok(w, "kwic", json!({ "query": query })));
    assert_eq!(count(json!({ "keywords": ["消える"], "field": "lemma" })), 1);
    assert_eq!(count(json!({ "keywords": ["消える"], "field": "surface" })), 0, "field 没传到");
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface", "pos": "NOUN" })), 3);
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface", "pos": "VERB" })), 0, "pos 没传到");
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface" })), 3, "副歌那两行默认折成一条");
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface", "dedup": false })), 4, "dedup 没传到");
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface", "personIds": [5] })), 2, "personIds 没传到");
    assert_eq!(count(json!({ "keywords": ["夜"], "field": "surface", "limit": 1 })), 1, "limit 没传到");

    let plain = ok(w, "kwic", json!({ "query": { "keywords": ["声"], "field": "surface" } }));
    let wide = ok(w, "kwic", json!({ "query": { "keywords": ["声"], "field": "surface", "crossLine": true } }));
    assert_eq!(plain[0]["left"], "君の");
    assert!(wide[0]["left"].as_str().unwrap().contains("街の灯りが消えて"), "crossLine 没传到：{wide}");
}

#[test]
fn kwic_query_omitting_optional_fields_uses_defaults() {
    let f = Fixture::new("kwic-defaults");
    // KwicQuery 有 #[serde(default)]，前端可以只传必填项
    let hits = ok(f.w(), "kwic", json!({ "query": { "keywords": ["夜"] } }));
    assert_eq!(len(&hits), 3, "默认按表层形找、折叠重复行");
    assert_eq!(len(&ok(f.w(), "kwic", json!({ "query": {} }))), 0, "一个关键词都没有就是空结果，不是报错");
}

#[test]
fn lyric_hit_has_every_field_the_page_renders() {
    let f = Fixture::new("lyric-shape");
    let hits = ok(f.w(), "search_lyrics", json!({ "text": "夜", "limit": 10 }));
    assert_eq!(len(&hits), 4);
    has_keys(&hits[0], "LyricHit", &["songId", "artist", "title", "utteranceId", "timeSec", "text", "rank"]);
    assert_eq!(len(&ok(f.w(), "search_lyrics", json!({ "text": "夜", "limit": 2 }))), 2);
}

#[test]
fn word_frequency_has_every_field_the_page_renders() {
    let f = Fixture::new("freq-shape");
    let w = f.w();
    let rows = ok(w, "word_frequency", json!({ "limit": 10 }));
    let list = rows.as_array().unwrap();
    assert_eq!(list.len(), 10);
    has_keys(&list[0], "WordFrequency", &["lemma", "pos", "freq", "songCount", "surfaces"]);
    assert!(list[0]["surfaces"].is_array());
    // 按频次降序——图表的条形宽度依赖这个顺序
    let freqs: Vec<i64> = list.iter().map(|r| r["freq"].as_i64().unwrap()).collect();
    assert!(freqs.windows(2).all(|p| p[0] >= p[1]), "词频应当降序：{freqs:?}");

    // 词性筛选
    let all = ok(w, "word_frequency", json!({ "limit": 50 }));
    let nouns = ok(w, "word_frequency", json!({ "pos": "NOUN", "limit": 50 }));
    assert!(nouns.as_array().unwrap().iter().all(|row| row["pos"] == "NOUN"), "筛选后不该出现别的词性");
    assert!(len(&nouns) > 0 && len(&nouns) < len(&all));
    assert_eq!(nouns[0]["lemma"], "夜", "出现最多的名词是「夜」");
}

#[test]
fn timeline_has_every_field_the_chart_renders() {
    let f = Fixture::new("timeline-shape");
    let rows = ok(f.w(), "timeline", json!({}));
    has_keys(&rows[0], "YearStats", &["year", "trackCount", "albumCount", "vocabulary"]);
    // 按年份升序——时间线是从左到右的
    let years: Vec<&str> = rows.as_array().unwrap().iter().map(|r| r["year"].as_str().unwrap()).collect();
    assert_eq!(years, vec!["2019", "2021", "2023"]);
}

#[test]
fn person_summary_and_collaborator_shapes_match_the_explorer() {
    let f = Fixture::new("person-shape");
    let w = f.w();
    // 合成库里四种角色都有人
    for role in ["performer", "composer", "lyricist", "arranger"] {
        let people = ok(w, "people_by_role", json!({ "role": role, "limit": 5 }));
        assert!(len(&people) > 0, "没有 {role}");
        has_keys(&people[0], role, &["id", "name", "role", "trackCount", "imagePath"]);
        assert_eq!(people[0]["role"], json!(role));

        let person_id = people[0]["id"].as_i64().unwrap();
        let works = ok(w, "works_by_person", json!({ "personId": person_id }));
        assert!(len(&works) > 0);
        let peers = ok(w, "collaborators", json!({ "personId": person_id, "limit": 5 }));
        for peer in peers.as_array().unwrap() {
            has_keys(peer, "Collaborator", &["personId", "name", "sharedTracks", "roles", "imagePath"]);
            assert!(peer["roles"].is_array());
        }
    }
    // 歌手照片是从 artists 表带出来的；没刮削过的是空串
    let performers = ok(w, "people_by_role", json!({ "role": "performer", "limit": 5 }));
    let photo = |name: &str| performers.as_array().unwrap().iter().find(|p| p["name"] == name).unwrap()["imagePath"].clone();
    assert_eq!(photo(fixture::ARTIST_WITH_PHOTO), "E:/music/artists/Akari.jpg");
    assert_eq!(photo("夜明バンド"), "");
}

#[test]
fn cross_page_navigation_targets_resolve() {
    let f = Fixture::new("cross-page");
    let w = f.w();
    // 检索页点一条命中 → 曲库页打开那首歌并定位到那一行。
    // 这条链路要求 KwicHit.songId 能被 get_track 解析，utteranceId 真的在那首歌的歌词里。
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 5 } }));
    for hit in hits.as_array().unwrap() {
        let song_id = hit["songId"].as_str().unwrap();
        assert!(!ok(w, "get_track", json!({ "songId": song_id })).is_null(), "KWIC 命中的 songId 应当能查到曲目");
        let lyrics = ok(w, "lyrics", json!({ "songId": song_id }));
        assert!(
            lyrics.as_array().unwrap().iter().any(|l| l["utteranceId"] == hit["utteranceId"]),
            "命中的行应当在该曲目的歌词里——否则跨页跳转会滚不到位置"
        );
    }
}

// ────────────────────────── 首页 ──────────────────────────

#[test]
fn home_summary_returns_every_section() {
    let f = Fixture::new("home");
    let home = ok(f.w(), "home_summary", json!({}));
    has_keys(
        &home,
        "HomeSummary",
        &["overview", "recentlyPlayed", "mostPlayed", "favorites", "albums", "genres", "decades", "topWords"],
    );
    assert_eq!(home["overview"]["tracks"], json!(fixture::TRACKS));
    assert_eq!(len(&home["albums"]) as i64, fixture::ALBUMS);
    assert!(len(&home["recentlyPlayed"]) > 0 && len(&home["mostPlayed"]) > 0 && len(&home["topWords"]) > 0);
}

#[test]
fn facets_cover_every_track() {
    let f = Fixture::new("facets");
    let home = ok(f.w(), "home_summary", json!({}));
    // 年代分面必须覆盖全部曲目——年份为空的归到「年代不详」而不是被丢掉，
    // 丢掉的话用户会发现各年代加起来对不上总数
    for facet in ["decades", "genres"] {
        let total: i64 = home[facet].as_array().unwrap().iter().map(|d| d["trackCount"].as_i64().unwrap()).sum();
        assert_eq!(total, fixture::TRACKS, "{facet} 分面应当覆盖全部曲目");
        for row in home[facet].as_array().unwrap() {
            has_keys(row, "FacetCount", &["label", "trackCount", "totalDurationSec"]);
        }
    }
}

// ────────────────────────── 全局搜索（Cmd+K） ──────────────────────────
//
// 「夜」在合成库里五个分组都搜得到：歌手和专辑名里有「夜明」，词和歌词里有「夜」。

fn quick(w: &tauri::WebviewWindow<tauri::test::MockRuntime>, query: &str, per_kind: i64) -> Value {
    ok(w, "quick_search", json!({ "query": query, "perKind": per_kind }))
}

const QUICK_GROUPS: [(&str, &str); 5] =
    [("tracks", "track"), ("albums", "album"), ("people", "person"), ("words", "word"), ("lyrics", "lyric")];

#[test]
fn quick_search_returns_every_group_with_its_kind_tag() {
    let f = Fixture::new("quick-groups");
    let results = quick(f.w(), "夜", 3);
    // 前端靠 kind 判别联合类型，缺了就没法渲染
    for (group, kind) in QUICK_GROUPS {
        let hits = results[group].as_array().unwrap_or_else(|| panic!("QuickSearchResults 缺分组 {group}"));
        assert!(!hits.is_empty(), "「夜」在 {group} 里应当搜得到：{results}");
        for hit in hits {
            assert_eq!(hit["kind"], json!(kind), "{group} 的 kind 标签不对");
        }
    }
}

#[test]
fn quick_hits_expose_what_the_palette_renders() {
    let f = Fixture::new("quick-fields");
    let results = quick(f.w(), "夜", 5);
    has_keys(&results["tracks"][0], "track hit", &["songId", "title", "artist", "album", "durationSec"]);
    has_keys(&results["albums"][0], "album hit", &["albumId", "title", "albumArtist", "trackCount"]);
    has_keys(&results["people"][0], "person hit", &["personId", "name", "roles", "trackCount"]);
    has_keys(&results["words"][0], "word hit", &["lemma", "pos", "freq", "songCount"]);
    has_keys(&results["lyrics"][0], "lyric hit", &["songId", "title", "artist", "utteranceId", "timeSec", "text"]);
    assert!(results["people"][0]["roles"].is_array(), "person hit 的 roles 应当是数组");
    // 精确相等的词排最前——搜「夜」想要的是「夜」本身
    assert_eq!(results["words"][0]["lemma"], "夜");
}

#[test]
fn quick_search_targets_are_reachable() {
    let f = Fixture::new("quick-targets");
    let w = f.w();
    // 面板里点一条就要能跳过去。跳不过去的搜索结果是死胡同。
    let results = quick(w, "夜", 3);
    for hit in results["tracks"].as_array().unwrap() {
        assert!(!ok(w, "get_track", json!({ "songId": hit["songId"] })).is_null(), "曲目结果应当能打开");
    }
    for hit in results["lyrics"].as_array().unwrap() {
        let lyrics = ok(w, "lyrics", json!({ "songId": hit["songId"] }));
        assert!(
            lyrics.as_array().unwrap().iter().any(|l| l["utteranceId"] == hit["utteranceId"]),
            "歌词结果的行应当在该曲目里——否则跳过去滚不到位置"
        );
    }
    for hit in results["words"].as_array().unwrap() {
        let word = ok(w, "word_in_corpus", json!({ "lemma": hit["lemma"], "limit": 1 }));
        assert!(word["occurrences"].as_i64().unwrap() > 0, "词结果应当能查到");
    }
    // 面板点专辑 → 曲库列表限定到它的曲目
    for hit in results["albums"].as_array().unwrap() {
        let tracks = ok(w, "album_tracks", json!({ "albumId": hit["albumId"] }));
        assert_eq!(len(&tracks) as i64, hit["trackCount"].as_i64().unwrap(), "专辑结果报的曲目数应当和实际取到的一致");
    }
    // 面板点人物 → 按 id 定位，再按主角色切左侧列表
    for hit in results["people"].as_array().unwrap() {
        let person = ok(w, "person_by_id", json!({ "personId": hit["personId"] }));
        has_keys(&person, "PersonSummary", &["id", "name", "role", "trackCount"]);
        assert_eq!(person["id"], hit["personId"]);
        // 主角色必须是这个人真的担任过的——前端靠它切左侧列表
        let listed = ok(w, "people_by_role", json!({ "role": person["role"], "limit": 500 }));
        assert!(
            listed.as_array().unwrap().iter().any(|p| p["id"] == hit["personId"]),
            "按主角色列出的人里应当包含他，否则切过去看不到"
        );
    }
    assert!(ok(w, "person_by_id", json!({ "personId": 999_999 })).is_null());
}

#[test]
fn quick_search_finds_people_whatever_the_case() {
    let f = Fixture::new("quick-case");
    for query in ["akari", "AKARI", "Akari"] {
        let results = quick(f.w(), query, 3);
        assert_eq!(results["people"][0]["name"], fixture::ARTIST_WITH_PHOTO, "搜 {query}");
        assert_eq!(results["tracks"][0]["songId"], fixture::RICH_SONG, "按歌手名也要找到她的歌");
    }
}

#[test]
fn quick_search_survives_hostile_input() {
    let f = Fixture::new("quick-hostile");
    let w = f.w();
    for query in ["", "   ", "'; DROP TABLE songs; --", "\\", "🎵"] {
        let results = invoke(w, "quick_search", json!({ "query": query }));
        assert!(results.is_ok(), "{query:?} 不该报错");
    }
    // 「%」「_」是 LIKE 的通配符：不转义的话会命中一切。合成库里只有一行歌词里有这两个字符
    for query in ["%", "_"] {
        let results = quick(w, query, 50);
        assert_eq!(len(&results["tracks"]), 0, "{query:?} 被当成通配符了：{results}");
        assert_eq!(len(&results["people"]), 0, "{query:?} 被当成通配符了：{results}");
        assert_eq!(len(&results["lyrics"]), 1, "{results}");
        assert_eq!(results["lyrics"][0]["text"], "100%の_夢を見た");
    }
    // 库还在
    assert_eq!(ok(w, "overview", json!({}))["tracks"], json!(fixture::TRACKS));
}

#[test]
fn quick_search_per_kind_limit_is_respected() {
    let f = Fixture::new("quick-limit");
    // 「夜」五个分组都不止……至少各有一条；歌词有四行
    let one = quick(f.w(), "夜", 1);
    for (group, _) in QUICK_GROUPS {
        assert_eq!(len(&one[group]), 1, "{group} 超过了每组限制");
    }
    assert_eq!(len(&quick(f.w(), "夜", 50)["lyrics"]), 4);
}

// ────────────────────────── 刮削（只读的那几条） ──────────────────────────
//
// 合成库没刮削过，状态表是空的——读不出东西也是有效结果。
// 真的发网络请求、真的写库的那几条留在 `tests/commands.rs`，标了 #[ignore]。

#[test]
fn scrape_summary_reports_every_status() {
    let f = Fixture::new("scrape-summary");
    let summary = ok(f.w(), "scrape_summary", json!({}));
    // 六个状态一个都不能少，UI 的统计块按名字取值
    for key in ["pending", "running", "success", "lowConfidence", "failed", "skipped", "total"] {
        assert!(summary[key].is_i64(), "缺字段 {key}: {summary}");
    }
}

#[test]
fn the_review_queue_takes_its_arguments() {
    let f = Fixture::new("scrape-queue");
    let w = f.w();
    // 不传 statuses 时默认取 low_confidence
    for row in ok(w, "scrape_review_queue", json!({})).as_array().expect("应当是数组") {
        assert_eq!(row["status"], "low_confidence");
    }
    // statuses / limit 两个参数都要能对上
    let rows = ok(w, "scrape_review_queue", json!({ "statuses": ["failed", "success"], "limit": 5 }));
    assert!(len(&rows) <= 5);
}

#[test]
fn scrape_commands_answer_for_a_file_nobody_has_scraped() {
    let f = Fixture::new("scrape-unknown");
    let w = f.w();
    assert_eq!(len(&ok(w, "scrape_attempts", json!({ "filePath": "X:/nope/nope.flac" }))), 0);
    let message = refused(w, "scrape_accept", json!({ "filePath": "X:/nope.flac", "candidateIndex": 0 }));
    assert!(message.contains("刮削记录"), "错误信息要说清是什么问题：{message}");
    // 没有批量作业在跑；取消一个没在跑的作业不该报错
    assert_eq!(ok(w, "scrape_is_running", json!({})), json!(false));
    ok(w, "scrape_cancel", json!({}));
}

#[test]
fn the_artist_roster_covers_every_artist_in_the_library() {
    let f = Fixture::new("roster");
    let rows = ok(f.w(), "artist_roster", json!({}));
    let mut seen: Vec<(String, i64, String, bool)> = rows
        .as_array()
        .expect("应当是数组")
        .iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap().to_string(),
                // 每个歌手都要能说出他有几首歌，否则「先刮谁」没法排序
                row["trackCount"].as_i64().unwrap(),
                row["imagePath"].as_str().unwrap().to_string(),
                row["hasImage"].as_bool().unwrap(),
            )
        })
        .collect();
    seen.sort();
    // Akari 在 artists 表里记着照片，但那个文件不在这台机器上：
    // `hasImage` 看的是**文件在不在**，不是库里有没有记——记着却没有文件的要能再刮一次
    assert_eq!(
        seen,
        vec![
            ("Akari".to_string(), 1, "E:/music/artists/Akari.jpg".to_string(), false),
            ("みなも".to_string(), 1, String::new(), false),
            ("夜明バンド".to_string(), 2, String::new(), false),
        ]
    );
}

// ────────────────────────── 不需要库的 ──────────────────────────

/// CSP 要**同时**放行 `img-src` 和 `connect-src` 的 asset 协议。
///
/// 为什么单独钉一条：全屏歌词的底色是前端把封面 `fetch` 下来画进 canvas 算的
/// （`coverColor.ts`，用 fetch 而不是 `<img>` 是为了拿到 CORS 头、canvas 不被污染）。
/// `connect-src` 少了 asset 的话，**图照样显示**——因为那走的是 `img-src`——
/// 只有取色悄悄失败，底色一直是那个中性深灰，界面上看不出哪里错了。
/// 开发时 vite 没有 CSP，所以这个坑只在打包后的程序里出现。
#[test]
fn the_csp_lets_the_front_end_fetch_assets_not_just_display_them() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json 不是合法 JSON");
    let csp = conf["app"]["security"]["csp"].as_str().expect("csp 不见了");
    let directive = |name: &str| {
        csp.split(';')
            .map(str::trim)
            .find(|part| part.starts_with(name))
            .unwrap_or_else(|| panic!("CSP 里没有 {name}"))
            .to_string()
    };
    for name in ["img-src", "connect-src", "media-src"] {
        let value = directive(name);
        assert!(
            value.contains("asset:") && value.contains("http://asset.localhost"),
            "{name} 要放行 asset 协议，现在是：{value}"
        );
    }
}
