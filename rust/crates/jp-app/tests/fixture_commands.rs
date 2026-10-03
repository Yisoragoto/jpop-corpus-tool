//! 不挑机器的 command 集成测试。
//!
//! `tests/commands.rs` 测的是**真库**：209 首歌、58,628 个 token，对的是数量。
//! 代价是它在找不到那个 991MB 的 `corpus.db` 时整组跳过**并算通过**——
//! CI 上 81 个测试里 78 个一行断言都没跑。
//!
//! 这一组补的是另一半：数据来自 `jp_corpus::fixture`（三首歌，建在临时目录里），
//! 所以**在任何机器上都真的跑**。它测的不是数量，是「这条路通不通」：
//!
//! * command 有没有真的注册上（名字写错编译期不报错，运行时才 404）；
//! * 前端传的 camelCase 参数名对不对得上 Rust 的 snake_case 形参；
//! * 返回值能不能序列化成前端认识的形状。
//!
//! 装配走的是 `jp_app_lib::register()`——和生产路径同一份命令清单。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde_json::{Value, json};
use tauri::WebviewWindowBuilder;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;

use jp_app_lib::state::AppState;
use jp_corpus::fixture;

/// **整个测试二进制只装一个 Tauri app。**
///
/// 一开始是一个测试一个 app。在 GitHub 的 Windows runner 上这会把测试进程打成
/// STATUS_ACCESS_VIOLATION：并行跑时第 5 个崩，加锁串行之后第 2 个就崩——
/// 也就是说问题不是并发，是**同一进程里装第二个 app**（或者拆第一个）
/// 在那台机器上根本不成立。本机跑得过，所以只有推上去才看得见。
///
/// 以前没人撞上是因为 `tests/commands.rs` 的那些在 CI 上找不到真库全都直接返回，
/// 一个 app 都没建过——这正是这组测试要解决的问题。
///
/// 所以：一个 app、一个库，全部测试共用，并且**排队跑**（见 `ONE_AT_A_TIME`）。
/// 代价是测试之间不再互相隔离，所以下面每个会写库的测试都要**把自己改的改回去**，
/// 或者只断言相对变化。
static SHARED: OnceLock<Shared> = OnceLock::new();

/// 排队。共用一个库，并发跑会互相看见对方写进去的东西。
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Shared {
    w: tauri::WebviewWindow<MockRuntime>,
    dir: PathBuf,
}

// `Shared` 放进 `OnceLock` 之后**永不 drop**：拆 app 正是上面说的崩溃点之一，
// 而且进程退出时库文件还开着、目录本来也删不掉。临时目录用固定名字，
// 下次跑之前先清掉（见 `Shared::new`）。

impl Shared {
    fn new() -> Self {
        // 固定名字 + 开跑前清一次：不留一堆带 pid 的垃圾目录
        let dir = std::env::temp_dir().join("jp-fixture-commands");
        let _ = std::fs::remove_dir_all(&dir);
        fixture::write_to(&dir).expect("建不出 fixture 库");

        let state = AppState::new(&dir).expect("装配状态失败");
        let app = jp_app_lib::register(mock_builder())
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("装配应用失败");
        let w = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("建 webview 失败");
        // app 本身故意泄掉：drop 它就是崩溃点
        std::mem::forget(app);
        Self { w, dir }
    }
}

/// 拿到共用的 app，并排到队里。守卫活着的时候只有这一个测试在动那个库。
struct Fixture {
    _lock: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(_tag: &str) -> Self {
        // 上一个测试 panic 过的话锁会中毒，但锁本身没坏，照用
        let lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        SHARED.get_or_init(Shared::new);
        Self { _lock: lock }
    }

    fn w(&self) -> &'static tauri::WebviewWindow<MockRuntime> {
        &SHARED.get().expect("还没装起来").w
    }

    fn dir(&self) -> &'static Path {
        &SHARED.get().expect("还没装起来").dir
    }
}

/// 走完整的 IPC 通道调一次 command，和前端 `invoke()` 同一条路径。
fn invoke(
    webview: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(webview, request)
        .map(|body| body.deserialize::<Value>().expect("返回值不是合法 JSON"))
}

fn ok(webview: &tauri::WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Value {
    invoke(webview, cmd, args).unwrap_or_else(|e| panic!("{cmd} 失败: {e}"))
}

fn len(value: &Value) -> usize {
    value.as_array().expect("不是数组").len()
}

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

    assert_eq!(len(&ok(w, "list_tracks", json!({}))), fixture::TRACKS as usize);
    assert_eq!(ok(w, "get_track", json!({ "songId": "001" }))["title"], "街の灯");
    assert!(ok(w, "get_track", json!({ "songId": "没有这首" })).is_null());

    assert_eq!(len(&ok(w, "list_albums", json!({}))), 2);
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

    // 001 在 fixture 里本来就是收藏的
    assert_eq!(ok(w, "is_favorite", json!({ "entityType": "track", "entityId": "001" })), json!(true));
    assert_eq!(ok(w, "toggle_favorite", json!({ "entityType": "track", "entityId": "001" })), json!(false));
    assert_eq!(ok(w, "is_favorite", json!({ "entityType": "track", "entityId": "001" })), json!(false));
    assert_eq!(ok(w, "toggle_favorite", json!({ "entityType": "track", "entityId": "001" })), json!(true));
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
    // 改完要立刻能查到新词
    let hits = ok(w, "kwic", json!({ "query": { "keywords": ["明ける"], "field": "lemma" } }));
    assert_eq!(len(&hits), 1);

    assert_eq!(
        ok(w, "revert_token_correction", json!({ "utteranceId": utterance_id })),
        json!(true)
    );
    let back = ok(w, "token_correction", json!({ "utteranceId": utterance_id }));
    assert_eq!(back["corrected"], json!(false));
    assert_eq!(back["tokens"], fresh["tokens"], "撤销没把分词还原回去");
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
#[test]
fn the_diagnostics_report_describes_this_library() {
    let f = Fixture::new("diagnostics");
    let text = ok(f.w(), "diagnostics_report", json!({}));
    let text = text.as_str().expect("不是字符串");

    assert!(text.contains("语料库"), "{text}");
    assert!(text.contains("有歌词没分词 1 首"), "{text}");
    // 不能带歌名、歌手、歌词
    for secret in ["街の灯", "夜明バンド", "夜が明ける前に"] {
        assert!(!text.contains(secret), "诊断信息里出现了 {secret}");
    }
}

// ────────────────────────────── 导入 ──────────────────────────────

/// 没扫描就导入、扫一个不存在的路径——两种错都要是**能看懂的一句话**，
/// 而不是一个底层错误或者一次 panic。
#[test]
fn the_import_commands_refuse_nonsense_with_a_readable_message() {
    let f = Fixture::new("import");
    let w = f.w();

    let err = invoke(w, "run_import", json!({})).expect_err("没有待导入的东西却成功了");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("scan"),
        "{err}"
    );

    let err = invoke(w, "scan_folder", json!({ "path": "Z:/根本没有这个目录" }))
        .expect_err("扫了个不存在的目录却成功了");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("不存在"),
        "{err}"
    );

    let err = invoke(w, "scan_files", json!({ "paths": [] })).expect_err("一个文件都没选却成功了");
    assert!(err["message"].is_string(), "{err}");

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
