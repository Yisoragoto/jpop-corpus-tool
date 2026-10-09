//! 对着**真实语料库**跑的 command 集成测试——只留真的要真实数据的那几条。
//!
//! 「命令有没有注册上、camelCase 参数对不对、返回值的形状前端认不认」这些不依赖库里
//! 具体是哪些歌的断言，都在 `tests/fixture_commands.rs` 和 `tests/fixture_flows.rs` 里，
//! 跑在合成库上，哪台机器都跑。以前它们全在这个文件里，找不到那个 991MB 的 `corpus.db`
//! 就整组跳过**并算通过**，CI 上一行断言都没执行。
//!
//! 留在这里的每一条都说得出为什么非真库不可：
//!
//! * asset scope 要拿库里存的**真实封面、照片路径**去问（盘符、反斜杠、目录名里的假名）；
//! * 重扫整个曲库要有那两百多个真实文件；
//! * 振假名、装词典后补分词要有 Sudachi 词典；
//! * 曲库维护那条要真实的音频文件和 `metadata/songs.csv`；
//! * 刮削的端到端要联网（标了 `#[ignore]`）。
//!
//! **不写用户的库。** 会写库的都跑在副本上（[`scratch_app`]）；直接开真库的那几条只读。
//! 以前有几条不是这样（切换收藏再切回、回填时长、对真库调 `run_import`），已经搬到合成库上了。
//!
//! 用 `tauri::test::mock_builder()` 装配的是 `jp_app_lib::register()`——
//! 和生产路径同一份 command 清单，不是另抄一份。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tauri::WebviewWindowBuilder;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;

use jp_app_lib::state::AppState;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf()
}

/// 装配一个跑在 mock runtime 上的真实应用。
///
/// 库不存在或没迁移时返回 None，整组测试跳过——克隆仓库的人不该因为
/// 缺一个 991MB 的数据库看到一片红。
fn app() -> Option<tauri::WebviewWindow<MockRuntime>> {
    let root = project_root();
    if !root.join("corpus.db").exists() {
        eprintln!("[skip] 找不到 {}", root.join("corpus.db").display());
        return None;
    }
    let state = match AppState::new(&root) {
        Ok(state) => state,
        Err(err) => {
            eprintln!(
                "[skip] {err}\n  这组测试要作者本机那个真实的 corpus.db，\
                 克隆仓库的人没有它——不挑机器的那组在 tests/fixture_commands.rs"
            );
            return None;
        }
    };
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("装配应用失败");
    Some(
        WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("建 webview 失败"),
    )
}

/// 走完整的 IPC 通道调一次 command，和前端 `invoke()` 完全同一条路径。
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

/// 临时副本上的应用。离开作用域时先关库连接、再删临时目录。
///
/// **只 drop 窗口不够**：原来的 album-art 测试就是先 `drop(w)` 再删目录，
/// 结果一次都没删成功，TEMP 里攒了 9 份、8.4 GB 的库副本。窗口 drop 了
/// AppState 还活着，库文件一直开着，Windows 上删不掉，而 `let _ =` 把错误吞了。
/// 所以先把 AppState 里的连接换成内存库，文件句柄才真正关掉。
struct Scratch {
    w: Option<tauri::WebviewWindow<MockRuntime>>,
    dir: PathBuf,
}

impl Scratch {
    fn w(&self) -> &tauri::WebviewWindow<MockRuntime> {
        self.w.as_ref().expect("Scratch 已经拆掉了")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        use tauri::Manager;
        if let Some(w) = self.w.take()
            && let Ok(memory) = jp_corpus::Corpus::open_in_memory()
        {
            *w.state::<AppState>().corpus() = memory;
        } // 窗口在这里 drop
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            // 删不掉要说出来，不能再像以前那样悄悄攒下去
            Err(err) => eprintln!("临时副本没删掉（{}）：{err}", self.dir.display()),
        }
    }
}

/// 在**真库的副本**上装配一个应用。
///
/// 会写库的 command 必须走这条——测试不该改用户的库。
/// 返回一个守卫，离开作用域时自动关库、删掉临时目录（见 [`Scratch`]）。
fn scratch_app(tag: &str) -> Option<Scratch> {
    let root = project_root();
    if !root.join("corpus.db").exists() {
        eprintln!("[skip] 找不到 corpus.db");
        return None;
    }
    let scratch = std::env::temp_dir().join(format!("jp-scratch-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("raw").join("covers")).ok()?;
    std::fs::create_dir_all(scratch.join("raw").join("artists")).ok()?;
    // **连 -wal / -shm 一起复制。** 库开着 WAL，最近的提交可能还只在 wal 文件里；
    // 只复制 corpus.db 会拿到一个「旧了一截」的副本——不报错，只是数据对不上。
    std::fs::copy(root.join("corpus.db"), scratch.join("corpus.db")).ok()?;
    for suffix in ["-wal", "-shm"] {
        let from = root.join(format!("corpus.db{suffix}"));
        if from.exists() {
            std::fs::copy(&from, scratch.join(format!("corpus.db{suffix}"))).ok()?;
        }
    }

    let state = AppState::new(&scratch).ok()?;
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .ok()?;
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .ok()?;
    Some(Scratch {
        w: Some(w),
        dir: scratch,
    })
}

macro_rules! app_or_skip {
    () => {
        match app() {
            Some(w) => w,
            None => return,
        }
    };
}

// ────────────────────────── 注册与自检 ──────────────────────────

/// 封面能不能显示，最后卡在 asset 协议的 scope 上：webview 请求一个路径，
/// Tauri 拿 glob 去比。Windows 的反斜杠、盘符、目录名里的日文假名都可能让
/// 这一步悄悄失败——失败的表现是图不出来，没有任何报错。
///
/// 这里直接拿 `songs.cover_path` 里存的真实路径去问 scope。
#[test]
fn the_asset_scope_actually_matches_the_cover_paths_in_the_database() {
    use tauri::Manager;

    let Some(webview) = app() else {
        eprintln!("[skip] 没有可用的 corpus.db");
        return;
    };
    let handle = webview.app_handle();
    let covers_dir = project_root().join("raw").join("covers");
    if !covers_dir.exists() {
        eprintln!("[skip] 还没刮削过，raw/covers 不存在");
        return;
    }

    // 走和 setup 同一个函数，而不是在测试里自己授权——测的是程序真正做的事
    jp_app_lib::allow_media_dirs(handle, &jp_app_lib::media_dirs(&handle.state::<AppState>()));
    let scope = handle.asset_protocol_scope();

    // 目录本身之外的东西一律不许读
    assert!(
        !scope.is_allowed(project_root().join("corpus.db")),
        "corpus.db 不该在 asset scope 里"
    );

    let paths: Vec<String> = {
        let conn = rusqlite::Connection::open_with_flags(
            project_root().join("corpus.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("打不开语料库");
        let mut stmt = conn
            .prepare("SELECT cover_path FROM songs WHERE COALESCE(cover_path,'') <> '' LIMIT 50")
            .expect("查询失败");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("查询失败");
        rows.filter_map(Result::ok).collect()
    };
    if paths.is_empty() {
        eprintln!("[skip] 库里还没有封面");
        return;
    }

    for path in &paths {
        assert!(
            scope.is_allowed(path),
            "scope 不认这个封面路径，界面上就是不出图：{path}"
        );
        assert!(
            std::path::Path::new(path).exists(),
            "库里记着这张封面，磁盘上却没有：{path}"
        );
    }
    println!("{} 条封面路径全部通过 scope", paths.len());
}

/// 歌手照片在 `raw/artists`，和封面不是一个目录。只放行封面目录的时候，
/// 人物页照样一张照片都出不来，而且和封面一样不报错。
#[test]
fn the_asset_scope_also_covers_artist_photos() {
    use tauri::Manager;

    let Some(webview) = app() else {
        eprintln!("[skip] 没有可用的 corpus.db");
        return;
    };
    let handle = webview.app_handle();
    jp_app_lib::allow_media_dirs(handle, &jp_app_lib::media_dirs(&handle.state::<AppState>()));
    let scope = handle.asset_protocol_scope();

    let conn = rusqlite::Connection::open_with_flags(
        project_root().join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("打不开语料库");
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='artists')",
            [],
            |r| r.get(0),
        )
        .expect("查询失败");
    if !has_table {
        eprintln!("[skip] 还没刮削过歌手");
        return;
    }
    let paths: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT image_path FROM artists WHERE COALESCE(image_path,'') <> ''")
            .expect("查询失败");
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("查询失败");
        rows.filter_map(Result::ok).collect()
    };
    if paths.is_empty() {
        eprintln!("[skip] 库里还没有歌手照片");
        return;
    }

    for path in &paths {
        assert!(
            scope.is_allowed(path),
            "scope 不认这张歌手照片，人物页就是不出图：{path}"
        );
        assert!(
            std::path::Path::new(path).exists(),
            "库里记着这张照片，磁盘上却没有：{path}"
        );
    }
    // 放行的是两个图片目录，不是整个 raw/
    assert!(!scope.is_allowed(project_root().join("raw").join("stray.txt")));
    println!("{} 张歌手照片全部通过 scope", paths.len());
}

#[test]
fn health_reports_the_real_state() {
    let w = app_or_skip!();
    let health = ok(&w, "health", json!({}));
    assert!(health["tracks"].as_i64().unwrap() > 0, "应当有曲目");
    assert!(health["lyricLines"].as_i64().unwrap() > 0, "应当有歌词行");
    // audioReady 反映的是「有没有可用输出设备」，不是「功能做没做」。
    // 无声卡环境（CI、远程桌面）应当是 false 且其余功能照常。
    assert!(health["audioReady"].is_boolean(), "audioReady 应当是布尔值");
    assert!(health["dbPath"].as_str().unwrap().ends_with("corpus.db"));
}

// ────────────────────────── 要分词词典的 ──────────────────────────

/// 振假名要真的分词才出得来，所以要 Sudachi 词典；合成库的目录里没有。
#[test]
fn furigana_offsets_line_up_with_the_lyric_text() {
    let w = app_or_skip!();
    if ok(&w, "health", json!({}))["tokenizerReady"] != json!(true) {
        return;
    }
    let tracks = ok(&w, "list_tracks", json!({ "limit": 5 }));
    let song_id = tracks[0]["id"].as_str().unwrap().to_string();
    let lines = ok(&w, "lyrics", json!({ "songId": &song_id }));
    for mode in ["kanji", "word"] {
        let furigana = ok(
            &w,
            "lyrics_furigana",
            json!({ "songId": &song_id, "mode": mode }),
        );
        let furigana = furigana.as_array().unwrap();
        assert_eq!(furigana.len(), lines.as_array().unwrap().len());
        let mut rubies = 0;
        for (line, entry) in lines.as_array().unwrap().iter().zip(furigana) {
            assert_eq!(line["utteranceId"], entry["utteranceId"]);
            let chars: Vec<char> = line["text"].as_str().unwrap().chars().collect();
            for ruby in entry["rubies"].as_array().unwrap() {
                let (start, end) = (
                    ruby["start"].as_u64().unwrap() as usize,
                    ruby["end"].as_u64().unwrap() as usize,
                );
                assert!(start < end && end <= chars.len());
                assert!(
                    chars[start..end]
                        .iter()
                        .any(|c| ('\u{3400}'..='\u{9fff}').contains(c) || *c == '々'),
                    "注音的那段里要有汉字"
                );
                assert!(!ruby["reading"].as_str().unwrap().is_empty());
                rubies += 1;
            }
        }
        assert!(rubies > 0, "{mode} 模式一段注音都没有");
    }
    let err = invoke(
        &w,
        "lyrics_furigana",
        json!({ "songId": &song_id, "mode": "romaji" }),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("romaji") || err.to_string().contains("variant"),
        "{err}"
    );
}

// ────────────────────────── 导入 ──────────────────────────
//
// 跑在**真库**上，所以只扫描、不确认：`run_import` 是会写库的命令，
// 「全是重复时确认了也不写」那条性质在 `fixture_flows.rs` 里对着合成库验。
// 这里要的是量：两百多个真实文件、带日文的路径，重扫一遍一条新的都不该有。

#[test]
fn scanning_an_already_imported_library_plans_nothing() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        eprintln!("[skip] 找不到 {}", audio.display());
        return;
    }
    let result = ok(
        &w,
        "scan_folder",
        json!({ "path": audio.display().to_string() }),
    );
    let summary = &result["summary"];
    assert_eq!(summary["new"].as_i64(), Some(0), "重扫不该产生新条目");
    assert_eq!(summary["skipped"].as_i64(), Some(0));
    assert_eq!(summary["possibleDuplicates"].as_i64(), Some(0));
    assert!(summary["alreadyImported"].as_i64().unwrap() > 0);
    assert_eq!(
        summary["total"].as_i64(),
        summary["alreadyImported"].as_i64()
    );
    // 复核列表要给得出每条的处置
    let items = result["items"].as_array().expect("items 应当是数组");
    assert_eq!(items.len(), summary["total"].as_i64().unwrap() as usize);
    assert_eq!(items[0]["action"]["kind"], "alreadyImported");
    assert!(items[0]["title"].as_str().is_some_and(|t| !t.is_empty()));
}

// ────────────────────────── 刮削 ──────────────────────────
//
// 只读的几条在 `fixture_commands.rs` 里。这里是会写库的端到端：复制一份 corpus.db
// 到临时目录，而且标了 #[ignore]，因为它们**真的发网络请求**——测试套件不该依赖外网。
//
//     cargo test -p jp-app --test commands -- --ignored --nocapture

/// 端到端：真的发网络请求，真的写库。
///
/// 跑在**真库的副本**上——刮削会改 songs 行和 scrape_state，
/// 不能碰用户的库。
#[test]
#[ignore = "要联网，而且要复制一份数据库"]
fn scraping_one_track_end_to_end() {
    let root = project_root();
    if !root.join("corpus.db").exists() {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("jp-scrape-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("raw").join("covers")).unwrap();
    std::fs::copy(root.join("corpus.db"), scratch.join("corpus.db")).unwrap();

    let state = AppState::new(&scratch).expect("装配失败");
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("装配应用失败");
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let before = ok(&w, "scrape_summary", json!({}))["total"]
        .as_i64()
        .unwrap();
    let after = ok(&w, "scrape_track", json!({ "songId": "001" }));
    println!("刮削 001 后：{after}");
    assert!(
        after["total"].as_i64().unwrap() > before,
        "刮完之后 scrape_state 该多一行"
    );

    // 状态要落进去，而且能读回来解释
    let all = ok(
        &w,
        "scrape_review_queue",
        json!({ "statuses": ["success", "low_confidence", "failed"], "limit": 10 }),
    );
    let rows = all.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{all}");
    let row = &rows[0];
    println!(
        "  状态={} 置信度={} 解释={}",
        row["status"], row["confidence"], row["explain"]
    );
    assert!(
        !row["localTitle"].as_str().unwrap().is_empty(),
        "要能显示本地写的是什么"
    );
    assert!(
        !row["explain"].as_str().unwrap().is_empty() || row["status"] == "failed",
        "有候选就要有打分解释：{row}"
    );

    // 尝试历史要留痕
    let history = ok(
        &w,
        "scrape_attempts",
        json!({ "filePath": row["filePath"] }),
    );
    assert_eq!(history.as_array().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// 批量作业端到端：真的发网络请求、真的写库、真的走后台线程。
///
/// 这是用户会按的那个按钮（「刮未匹配的」），之前只测了单首。
#[test]
#[ignore = "要联网，而且要复制一份数据库"]
fn a_batch_job_runs_in_the_background_and_reports_progress() {
    let root = project_root();
    if !root.join("corpus.db").exists() {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("jp-batch-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("raw").join("covers")).unwrap();
    std::fs::copy(root.join("corpus.db"), scratch.join("corpus.db")).unwrap();

    let state = AppState::new(&scratch).expect("装配失败");
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("装配应用失败");
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // 默认 3 首。想量整库速度就 JP_SCRAPE_E2E_LIMIT=12 跑一次——
    // 只有几首的时候流水线填不满，看不出并发的真实收益。
    let limit: i64 = std::env::var("JP_SCRAPE_E2E_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let started = std::time::Instant::now();
    let total = ok(
        &w,
        "scrape_start",
        json!({ "onlyMissing": true, "limit": limit }),
    );
    println!("scrape_start 返回：{total}");
    assert_eq!(total.as_i64(), Some(limit), "该排队 {limit} 首");

    // 立刻返回，作业在后台
    assert_eq!(ok(&w, "scrape_is_running", json!({})), json!(true));

    // 等它跑完。封面是大头：一张 600x600 在这条链路上 13~27 秒，
    // 4 路并发之后仍要一段时间。给足余量。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while std::time::Instant::now() < deadline {
        if ok(&w, "scrape_is_running", json!({})) == json!(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert_eq!(
        ok(&w, "scrape_is_running", json!({})),
        json!(false),
        "作业没在期限内跑完"
    );

    let elapsed = started.elapsed();
    let summary = ok(&w, "scrape_summary", json!({}));
    println!(
        "汇总：{summary}
{limit} 首用时 {elapsed:?}（{:.2} 秒/首，整库 209 首约 {:.0} 分钟）",
        elapsed.as_secs_f64() / limit as f64,
        elapsed.as_secs_f64() / limit as f64 * 209.0 / 60.0
    );
    assert_eq!(summary["total"].as_i64(), Some(limit), "都该落库");

    // 封面要真的落盘，而且路径记进了 songs。
    // 这一条钉住的是一个真实事故：图片和 JSON 共用 10 秒超时，
    // 结果元数据全对上、封面全军覆没，而汇总看起来一切正常。
    let conn = rusqlite::Connection::open(scratch.join("corpus.db")).unwrap();
    let with_cover: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM songs WHERE COALESCE(cover_path,'')<>''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let files = std::fs::read_dir(scratch.join("raw").join("covers"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
                .count()
        })
        .unwrap_or(0);
    // with_cover 里含库副本自带的存量（Python 早先刮的），
    // 真正能证明这一轮下成功的是**盘上的文件数**——临时目录是空的开始。
    println!("封面：库里共 {with_cover} 条（含存量），这一轮落盘 {files} 个文件");
    assert!(with_cover > 0);
    assert!(files > 0, "这一轮一张封面都没下下来");
    drop(conn);

    // 已经成功的不该再排队。库里 209 首，刮好 3 首之后
    // 「未匹配的」应当正好剩 206——不限 limit 才验得到这一点。
    let songs: i64 = {
        let conn = rusqlite::Connection::open(scratch.join("corpus.db")).unwrap();
        conn.query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))
            .unwrap()
    };
    let success = summary["success"].as_i64().unwrap();
    let again = ok(
        &w,
        "scrape_start",
        json!({ "onlyMissing": true, "limit": 100000 }),
    );
    println!("第二次 scrape_start 返回：{again}（库里 {songs} 首，已成功 {success}）");
    let _ = limit;
    ok(&w, "scrape_cancel", json!({}));
    assert_eq!(
        again.as_i64().unwrap(),
        songs - success,
        "已成功的该被跳过；汇总={summary}"
    );

    // 等第二个作业停下来再删目录，否则后台线程还握着数据库文件
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline
        && ok(&w, "scrape_is_running", json!({})) == json!(true)
    {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// 端到端：真的问 MusicBrainz 和 Deezer，真的写库和落图。
#[test]
#[ignore = "要联网，而且要复制一份数据库"]
fn scraping_an_artist_end_to_end() {
    let root = project_root();
    if !root.join("corpus.db").exists() {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("jp-artist-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("raw").join("artists")).unwrap();
    std::fs::copy(root.join("corpus.db"), scratch.join("corpus.db")).unwrap();

    let state = AppState::new(&scratch).expect("装配失败");
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("装配应用失败");
    let w = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    // ずっと真夜中でいいのに。在 Deezer 上叫 ZUTOMAYO——
    // 不先从 MusicBrainz 拿别名就找不到照片，这一条正是验那个链路。
    for name in ["ヨルシカ", "ずっと真夜中でいいのに。"] {
        let got = ok(&w, "scrape_artist", json!({ "name": name }));
        println!("{name} → {got}");
        assert!(!got["notFound"].as_bool().unwrap(), "{name} 该查得到");
        assert!(
            !got["artistType"].as_str().unwrap().is_empty()
                || !got["imagePath"].as_str().unwrap().is_empty(),
            "资料和照片总得有一样：{got}"
        );
        if let Some(path) = got["imagePath"].as_str().filter(|p| !p.is_empty()) {
            let meta = std::fs::metadata(path).expect("照片该落盘了");
            assert!(
                meta.len() > 1024,
                "照片太小，多半是占位图：{} 字节",
                meta.len()
            );
        }
    }

    // 写进 artists 表了，而且重跑不该把已有值抹掉
    let roster = ok(&w, "artist_roster", json!({}));
    let yorushika = roster
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "ヨルシカ")
        .expect("名册里该有ヨルシカ");
    println!("名册里的ヨルシカ：{yorushika}");
    assert!(!yorushika["artistType"].as_str().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(&scratch);
}

// ────────────────────────── 曲库维护 ──────────────────────────

/// 走真实 IPC、在库的**临时副本**上（连同 metadata/songs.csv 的副本）：
/// 编辑 → 清单跟着改；换音频 → 时长读出来；弄丢一首的音频 → 扫目录能找回；删歌 → 库里、检索、清单里都没了，
/// 真项目里的封面文件不受影响。
#[test]
fn library_maintenance_round_trips_through_ipc_on_a_scratch_copy() {
    let Some(scratch) = scratch_app("library-maintenance") else {
        eprintln!("[skip] 没有可用的 corpus.db");
        return;
    };
    let w = scratch.w();
    let real_csv = project_root().join("metadata/songs.csv");
    let csv = scratch.dir.join("metadata/songs.csv");
    let has_csv = real_csv.is_file();
    if has_csv {
        std::fs::create_dir_all(csv.parent().unwrap()).unwrap();
        std::fs::copy(&real_csv, &csv).unwrap();
    }
    let csv_row = |id: &str| -> Option<String> {
        let text = std::fs::read_to_string(&csv).ok()?;
        text.lines()
            .find(|l| l.starts_with(&format!("{id},")))
            .map(str::to_owned)
    };

    assert_eq!(
        ok(w, "library_missing_audio", json!({})),
        json!([]),
        "真实曲库的音频都在"
    );

    // 挑两首音频真实存在、有歌词的歌
    let tracks = ok(w, "list_tracks", json!({ "limit": 1000 }));
    let usable: Vec<Value> = tracks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            t["lineCount"].as_i64().unwrap_or(0) > 3
                && Path::new(t["audioPath"].as_str().unwrap_or("")).is_file()
        })
        .take(2)
        .cloned()
        .collect();
    assert_eq!(usable.len(), 2);
    let (first, second) = (&usable[0], &usable[1]);
    let id = first["id"].as_str().unwrap();

    // 编辑
    let edited = ok(
        w,
        "library_edit_song",
        json!({ "songId": id, "edit": {
            "title": format!("{}（改）", first["title"].as_str().unwrap()),
            "artist": first["artist"], "year": "1999", "album": first["album"], "genre": "テスト",
        }}),
    );
    assert!(
        edited["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "title"),
        "{edited}"
    );
    let track = ok(w, "get_track", json!({ "songId": id }));
    assert!(track["title"].as_str().unwrap().ends_with("（改）"));
    if has_csv {
        assert_eq!(edited["csv"], "updated");
        let row = csv_row(id).unwrap();
        assert!(
            row.contains("（改）") && row.contains(",1999,") && row.contains("テスト"),
            "{row}"
        );
    }

    // 换音频：复制一份到临时目录再指过去
    let copy = scratch.dir.join("moved.flac");
    std::fs::copy(first["audioPath"].as_str().unwrap(), &copy).unwrap();
    let relinked = ok(
        w,
        "library_relink_audio",
        json!({ "links": [
            { "songId": id, "path": copy.display().to_string() },
            { "songId": id, "path": scratch.dir.join("不存在.flac").display().to_string() },
        ]}),
    );
    let results = relinked["results"].as_array().unwrap();
    assert_eq!(results[0]["error"], "", "{relinked}");
    assert!(
        results[1]["error"].as_str().unwrap().contains("不存在"),
        "{relinked}"
    );
    if has_csv {
        assert_eq!(relinked["csv"], "updated");
        assert!(csv_row(id).unwrap().contains("moved.flac"));
    }
    let track = ok(w, "get_track", json!({ "songId": id }));
    assert_eq!(track["audioPath"], copy.display().to_string());
    assert!(track["durationSec"].as_f64().unwrap_or(0.0) > 10.0);

    // 弄丢第二首的音频，再扫它原来所在的目录找回来
    let second_id = second["id"].as_str().unwrap();
    let original = second["audioPath"].as_str().unwrap().to_owned();
    {
        use tauri::Manager;
        let state = w.state::<AppState>();
        state
            .corpus()
            .connection()
            .execute(
                "UPDATE songs SET audio_path='Z:/丢了/x.flac' WHERE id=?1",
                [second_id],
            )
            .unwrap();
    }
    let missing = ok(w, "library_missing_audio", json!({}));
    assert_eq!(missing.as_array().unwrap().len(), 1);
    let folder = Path::new(&original).parent().unwrap().display().to_string();
    let suggestions = ok(w, "library_suggest_relinks", json!({ "folder": folder }));
    let suggestion = suggestions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["songId"] == second_id)
        .unwrap_or_else(|| panic!("{suggestions}"));
    assert_eq!(
        suggestion["path"].as_str().unwrap().to_lowercase(),
        original.to_lowercase()
    );

    // 删除
    let cover = track["coverPath"].as_str().unwrap_or("").to_owned();
    let line = ok(w, "lyrics", json!({ "songId": id }))[0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let deleted = ok(w, "library_delete_song", json!({ "songId": id }));
    assert!(deleted["lyricLines"].as_i64().unwrap() > 3, "{deleted}");
    assert_eq!(
        deleted["coverRemoved"], false,
        "副本的封面路径指向真项目，不能删"
    );
    if !cover.is_empty() {
        assert!(Path::new(&cover).is_file(), "真项目里的封面文件还在");
    }
    assert!(
        invoke(w, "get_track", json!({ "songId": id }))
            .map(|v| v.is_null())
            .unwrap_or(true)
    );
    let hits = ok(w, "search_lyrics", json!({ "text": line, "limit": 50 }));
    assert!(
        hits.as_array().unwrap().iter().all(|h| h["songId"] != id),
        "删掉的歌检索不到"
    );
    if has_csv {
        assert_eq!(deleted["csv"], "updated");
        assert!(csv_row(id).is_none());
        assert!(csv_row(second_id).is_some(), "别的行不动");
    }
    assert!(
        invoke(w, "library_delete_song", json!({ "songId": id })).is_err(),
        "删过了再删要报错"
    );
}

/// 词典一装上，「有歌词、没分词」的歌要顺手补好——不该等用户自己去「曲库维护」里找。
///
/// 真实发生过：新库在没词典时导了 16 首，后来迁移旧库，旧库里有分词的同名歌被判成
/// 「已在库中」跳过，结果 16 首查不了词、没有振假名，修复按钮藏在设置深处。
///
/// 副本目录里没有 venv，一开始就是「没词典」；然后从项目目录复制一份装上。
/// 只该写 `tokens`：歌词原文一个字都不能变。
#[test]
fn installing_the_dictionary_tokenizes_the_songs_left_without_tokens() {
    use tauri::Manager;
    let Some(scratch) = scratch_app("dict-backlog") else {
        return;
    };
    let w = scratch.w();
    let count = |value: &Value| value.as_array().map_or(0, Vec::len);
    if jp_tokenizer::locate_sudachipy(&project_root()).is_none() {
        eprintln!("[skip] 项目目录里没有 Sudachi 词典，装不了");
        return;
    }
    assert_eq!(
        ok(w, "health", json!({}))["tokenizerReady"],
        json!(false),
        "副本目录里本不该有词典"
    );

    // 造前提：挑两首有分词的歌，把分词删掉
    let lyrics_of = |w: &tauri::WebviewWindow<MockRuntime>, ids: &[String]| -> Vec<String> {
        let state = w.state::<AppState>();
        let corpus = state.corpus();
        let mut stmt = corpus
            .connection()
            .prepare("SELECT text FROM utterances WHERE song_id = ?1 ORDER BY line_idx, id")
            .unwrap();
        ids.iter()
            .flat_map(|id| {
                stmt.query_map([id], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
            .collect()
    };
    let stripped: Vec<String> = {
        let state = w.state::<AppState>();
        let corpus = state.corpus();
        let conn = corpus.connection();
        let ids: Vec<String> = conn
            .prepare(
                "SELECT DISTINCT u.song_id FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
                 ORDER BY u.song_id LIMIT 2",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for id in &ids {
            conn.execute(
                "DELETE FROM tokens WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id = ?1)",
                [id],
            )
            .unwrap();
        }
        ids
    };
    assert_eq!(stripped.len(), 2);
    let before_install = count(&ok(w, "tokenize_missing_list", json!({})));
    assert!(before_install >= 2, "删掉分词的歌没进名单");
    let text_before = lyrics_of(w, &stripped);

    let done = ok(
        w,
        "tokenizer_install",
        json!({ "path": project_root().display().to_string() }),
    );
    assert_eq!(done["tokenizeError"], json!(""), "{done}");
    assert_eq!(done["tokenized"]["songs"].as_u64(), Some(before_install as u64), "{done}");
    assert!(done["tokenized"]["tokens"].as_u64().unwrap() > 0, "{done}");
    // 原来的字段还在（前端的 InstalledDict 读它们）
    assert!(done["bytes"].as_u64().unwrap() > 0 && done["dir"].is_string(), "{done}");

    assert_eq!(count(&ok(w, "tokenize_missing_list", json!({}))), 0, "补完了名单还不空");
    for id in &stripped {
        let lines = ok(w, "lyrics", json!({ "songId": id }));
        let lines = lines.as_array().unwrap();
        assert!(
            lines.iter().any(|l| count(&l["tokens"]) > 0),
            "{id} 补完还是没有分词"
        );
    }
    assert_eq!(lyrics_of(w, &stripped), text_before, "补分词改动了歌词原文");

    // 再装一次：没有要补的，什么都不写
    let again = ok(
        w,
        "tokenizer_install",
        json!({ "path": project_root().display().to_string() }),
    );
    assert_eq!(again["tokenized"]["songs"].as_u64(), Some(0), "{again}");
}
