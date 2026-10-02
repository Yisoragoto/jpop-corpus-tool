//! Tauri command 层的集成测试。
//!
//! 业务逻辑在 `jp-corpus` 里已经测过了，这里测的是**只有跑起来才会暴露的东西**：
//!
//! * command 有没有真的注册上（名字写错了编译期不报错，运行时才 404）
//! * 前端传的 camelCase 参数名能不能对上 Rust 的 snake_case 形参
//! * 返回值能不能序列化成前端认识的形状
//!
//! 这三样正是「编译通过但一点就报错」的来源。
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
        eprintln!("跳过：找不到 {}", root.join("corpus.db").display());
        return None;
    }
    let state = match AppState::new(&root) {
        Ok(state) => state,
        Err(err) => {
            eprintln!(
                "跳过：{err}\n  先跑 python scripts/migrate_db.py \
                 && python scripts/backfill_library.py"
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

    /// 副本所在目录。测试要自己造前提（改副本里的库）时用
    fn dir(&self) -> &Path {
        &self.dir
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
        eprintln!("跳过：找不到 corpus.db");
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
        eprintln!("跳过：没有可用的 corpus.db");
        return;
    };
    let handle = webview.app_handle();
    let covers_dir = project_root().join("raw").join("covers");
    if !covers_dir.exists() {
        eprintln!("跳过：还没刮削过，raw/covers 不存在");
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
        eprintln!("跳过：库里还没有封面");
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
        eprintln!("跳过：没有可用的 corpus.db");
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
        eprintln!("跳过：还没刮削过歌手");
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
        eprintln!("跳过：库里还没有歌手照片");
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

#[test]
fn unknown_command_is_rejected() {
    let w = app_or_skip!();
    // 名字写错时应当报错而不是静默返回 null——否则前端拿到 undefined 很难查
    assert!(invoke(&w, "no_such_command", json!({})).is_err());
}

#[test]
fn every_registered_command_is_reachable() {
    let w = app_or_skip!();
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
        // 只放只读的；保存和撤销会写库，在下面的临时副本上测
        ("token_correction", json!({ "utteranceId": 1 })),
        // 只看本机有没有 ffmpeg，不连 Anki
        ("anki_audio_available", json!({})),
    ] {
        let result = invoke(&w, cmd, args);
        if let Err(err) = &result {
            let message = err.to_string();
            assert!(
                !message.contains("not found") && !message.contains("Command"),
                "{cmd} 没有注册上: {message}"
            );
        }
    }
}

// ────────────────────────── 参数名转换 ──────────────────────────

#[test]
fn camel_case_arguments_reach_snake_case_parameters() {
    let w = app_or_skip!();
    // 这是最容易翻车的一处：前端写 songId，Rust 形参是 song_id。
    // Tauri 默认按 camelCase 查找，所以 api.ts 必须用 camelCase。
    let first = &ok(&w, "list_tracks", json!({ "limit": 1 }))[0];
    let song_id = first["id"].as_str().unwrap().to_string();

    let track = ok(&w, "get_track", json!({ "songId": song_id }));
    assert_eq!(track["id"], json!(song_id));

    // snake_case 传参应当取不到值 —— 用它反证上面那条不是巧合
    let missing = invoke(&w, "get_track", json!({ "song_id": song_id }));
    assert!(
        missing.is_err() || missing.unwrap().is_null(),
        "snake_case 参数名不该被接受，否则说明约定和文档不符"
    );
}

#[test]
fn multi_word_arguments_convert_too() {
    let w = app_or_skip!();
    // min_listened_sec -> minListenedSec，两段以上的名字也要对得上
    let rows = ok(
        &w,
        "most_played",
        json!({ "minListenedSec": 0.0, "limit": 3 }),
    );
    assert!(rows.is_array());
}

#[test]
fn optional_arguments_can_be_omitted() {
    let w = app_or_skip!();
    // Option<T> 形参不传时要走默认值，而不是报「缺参数」
    assert!(ok(&w, "list_tracks", json!({})).is_array());
    assert!(ok(&w, "word_frequency", json!({})).is_array());
}

// ────────────────────────── 序列化形状 ──────────────────────────

#[test]
fn track_is_camel_case_for_the_frontend() {
    let w = app_or_skip!();
    let track = &ok(&w, "list_tracks", json!({ "limit": 1 }))[0];
    for key in [
        "id",
        "title",
        "artist",
        "album",
        "albumId",
        "durationSec",
        "lineCount",
    ] {
        assert!(track.get(key).is_some(), "Track 缺字段 {key}");
    }
    // snake_case 泄漏出去就说明某个结构体漏了 rename_all
    assert!(track.get("album_id").is_none(), "不该出现 snake_case 字段");
}

#[test]
fn overview_matches_the_typescript_interface() {
    let w = app_or_skip!();
    let overview = ok(&w, "overview", json!({}));
    for key in [
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
    ] {
        assert!(overview.get(key).is_some(), "Overview 缺字段 {key}");
    }
}

#[test]
fn nullable_fields_serialize_as_null_not_absent() {
    let w = app_or_skip!();
    // durationSec 现在全库都是空的。它必须是 null 而不是缺字段，
    // 否则 TS 那边 `durationSec: number | null` 的类型就是假的。
    let track = &ok(&w, "list_tracks", json!({ "limit": 1 }))[0];
    assert!(track["durationSec"].is_null() || track["durationSec"].is_number());
    assert!(track.as_object().unwrap().contains_key("durationSec"));
}

// ────────────────────────── 竖切回路 ──────────────────────────

#[test]
fn the_research_loop_works_end_to_end() {
    let w = app_or_skip!();

    // 1. 曲库 → 找一首有歌词的
    let tracks = ok(&w, "list_tracks", json!({ "limit": 200 }));
    let track = tracks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["lineCount"].as_i64().unwrap_or(0) > 0)
        .expect("应当至少有一首带歌词的曲目");
    let song_id = track["id"].as_str().unwrap().to_string();

    // 2. 歌词（带分词）
    let lyrics = ok(&w, "lyrics", json!({ "songId": &song_id }));
    let lines = lyrics.as_array().unwrap();
    assert!(!lines.is_empty());

    // 3. 从歌词里挑一个实词
    let lemma = lines
        .iter()
        .flat_map(|l| l["tokens"].as_array().unwrap())
        .find(|t| {
            matches!(
                t["pos"].as_str().unwrap_or(""),
                "NOUN" | "VERB" | "ADJ" | "ADV" | "PROPN"
            )
        })
        .map(|t| t["lemma"].as_str().unwrap().to_string())
        .expect("应当至少有一个实词");

    // 4. 点词 → 查全语料
    let word = ok(
        &w,
        "word_in_corpus",
        json!({ "lemma": &lemma, "currentSongId": &song_id, "limit": 10 }),
    );
    assert_eq!(word["lemma"], json!(lemma));
    assert!(word["occurrences"].as_i64().unwrap() > 0);

    // 5. 例句能跳回具体曲目——这是回路闭合的关键
    let examples = word["examples"].as_array().unwrap();
    assert!(!examples.is_empty(), "应当有例句，否则跳转无从谈起");
    let target = examples[0]["songId"].as_str().unwrap();
    let jumped = ok(&w, "get_track", json!({ "songId": target }));
    assert_eq!(jumped["id"], json!(target));
}

#[test]
fn credits_expose_the_new_dimensions() {
    let w = app_or_skip!();
    // 作曲/作词是这一轮从 LRC 里捡回来的，command 层要能透出去
    let composers = ok(
        &w,
        "people_by_role",
        json!({ "role": "composer", "limit": 5 }),
    );
    let list = composers.as_array().unwrap();
    assert!(!list.is_empty(), "应当有作曲家");
    for key in ["id", "name", "role", "trackCount"] {
        assert!(list[0].get(key).is_some(), "PersonSummary 缺字段 {key}");
    }

    // 合作图谱：一次自连接就能查
    let person_id = list[0]["id"].as_i64().unwrap();
    let peers = ok(
        &w,
        "collaborators",
        json!({ "personId": person_id, "limit": 5 }),
    );
    assert!(peers.is_array());
}

#[test]
fn kwic_accepts_the_query_struct() {
    let w = app_or_skip!();
    // KwicQuery 是嵌套结构体，序列化形状最容易和 TS 那边对不上
    let hits = ok(
        &w,
        "kwic",
        json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 5 } }),
    );
    assert!(hits.is_array());
    if let Some(first) = hits.as_array().unwrap().first() {
        for key in ["songId", "left", "keyword", "right", "timeSec"] {
            assert!(first.get(key).is_some(), "KwicHit 缺字段 {key}");
        }
    }

    // 只看日文：数字、字母的关键词筛掉，而且是筛完才截断
    let hits = ok(
        &w,
        "kwic",
        json!({ "query": { "keywords": ["夜", "1", "I"], "field": "surface", "jpOnly": true, "limit": 5 } }),
    );
    let hits = hits.as_array().unwrap();
    assert!(!hits.is_empty() && hits.len() <= 5);
    assert!(hits.iter().all(|h| h["keyword"] == "夜"), "{hits:?}");
}

#[test]
fn stats_commands_filter_and_carry_the_report_text() {
    let w = app_or_skip!();
    let rows = ok(&w, "stats_frequency", json!({ "limit": 50 }));
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 50);
    for key in ["lemma", "pos", "freq", "songCount", "surfaces", "jlpt"] {
        assert!(rows[0].get(key).is_some(), "统计表缺字段 {key}");
    }
    // 词元 × 词性按频次降序，排除标点和符号
    assert!(
        rows.windows(2)
            .all(|p| p[0]["freq"].as_i64() >= p[1]["freq"].as_i64())
    );
    assert!(
        rows.iter()
            .all(|r| r["pos"] != "PUNCT" && r["pos"] != "SYM")
    );
    assert!(
        rows.iter()
            .any(|r| r["jlpt"].as_str().unwrap().starts_with('N')),
        "JLPT 列一个都没查到"
    );

    let performers = ok(
        &w,
        "people_by_role",
        json!({ "role": "performer", "limit": 1 }),
    );
    let person = &performers[0];
    let filter = json!({ "performerIds": [person["id"]], "jpOnly": true });
    let nouns = ok(
        &w,
        "stats_frequency",
        json!({ "pos": "NOUN", "filter": filter, "limit": 20 }),
    );
    assert!(!nouns.as_array().unwrap().is_empty());
    assert!(nouns.as_array().unwrap().iter().all(|r| r["pos"] == "NOUN"));

    let everything = ok(&w, "stats_report", json!({}));
    assert_eq!(everything["performers"], json!([]));
    assert!(
        everything["text"]
            .as_str()
            .unwrap()
            .contains("フィルター：全歌手\r\n日本語のみ：なし\r\n")
    );

    let report = ok(&w, "stats_report", json!({ "filter": filter }));
    assert_eq!(report["performers"], json!([person["name"]]));
    assert_eq!(report["jpOnly"], true);
    let name = person["name"].as_str().unwrap();
    let text = report["text"].as_str().unwrap();
    assert!(
        text.contains(&format!("フィルター：{name}\r\n日本語のみ：あり\r\n")),
        "{text}"
    );
    let songs = report["report"]["songCount"].as_i64().unwrap();
    assert!(songs > 0 && songs < everything["report"]["songCount"].as_i64().unwrap());
    assert!(text.contains(&format!("  曲数                : {songs}")) || songs >= 1000);
    for key in [
        "ttr",
        "sttr",
        "sttrChunk",
        "hapaxRatio",
        "posDist",
        "coverage",
        "topWords",
    ] {
        assert!(report["report"].get(key).is_some(), "报告缺字段 {key}");
    }

    // 导出：只认 txt / csv / html
    let dir = std::env::temp_dir().join(format!("jp-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("レポート.txt");
    ok(
        &w,
        "export_text",
        json!({ "path": target.display().to_string(), "content": text }),
    );
    assert_eq!(std::fs::read(&target).unwrap(), text.as_bytes());
    let refused = invoke(
        &w,
        "export_text",
        json!({ "path": dir.join("x.exe").display().to_string(), "content": "" }),
    );
    assert!(refused.is_err() && !dir.join("x.exe").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ────────────────────────── 错误路径 ──────────────────────────

#[test]
fn missing_track_returns_null_not_an_error() {
    let w = app_or_skip!();
    // 查不到应当是 null（Option<Track> → null），不是抛错
    assert!(ok(&w, "get_track", json!({ "songId": "不存在的ID" })).is_null());
}

#[test]
fn errors_carry_a_readable_message() {
    let w = app_or_skip!();
    // CommandError 序列化成 { message } —— api.ts 的 call() 靠它转成 Error
    if let Err(err) = invoke(&w, "kwic", json!({ "query": { "field": "surface" } })) {
        assert!(
            err.get("message").is_some() || err.is_string(),
            "错误应当带可读消息，实际是 {err}"
        );
    }
}

// ────────────────────────── 播放 ──────────────────────────

/// 音频引擎在无声卡环境下是 None，这时播放类 command 应当**报错**而不是
/// 假装成功。所以这些测试分两种断言：有引擎时验行为，没引擎时验错误信息。
fn audio_ready(w: &tauri::WebviewWindow<MockRuntime>) -> bool {
    ok(w, "health", json!({}))["audioReady"]
        .as_bool()
        .unwrap_or(false)
}

#[test]
fn health_reports_audio_and_pitch_capability() {
    let w = app_or_skip!();
    let health = ok(&w, "health", json!({}));
    assert!(health.get("audioReady").is_some());
    // 变调要 ffmpeg：有引擎、项目根目录或 PATH 里有 ffmpeg 才报能用
    let has_ffmpeg = jp_anki::audio::find_ffmpeg(&project_root()).is_some();
    assert_eq!(
        health["pitchSupported"],
        json!(health["audioReady"] == json!(true) && has_ffmpeg)
    );
}

#[test]
fn setting_the_pitch_with_nothing_loaded_only_remembers_it() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        return;
    }
    // 没加载歌时只记住半音数，不渲染（测试不往项目的缓存目录里写东西）
    ok(&w, "audio_stop", json!({}));
    ok(&w, "audio_set_pitch", json!({ "semitones": 9 }));
    let state = ok(&w, "audio_state", json!({}));
    assert_eq!(state["pitchSemitones"], json!(6), "超出范围要钳到 +6");
    assert_eq!(state["pitchRendering"], json!(false));
    assert_eq!(state["pitchError"], Value::Null);
    ok(&w, "audio_set_pitch", json!({ "semitones": 0 }));
    assert_eq!(ok(&w, "audio_state", json!({}))["pitchSemitones"], json!(0));
}

#[test]
fn font_catalog_lists_installed_and_bundled_fonts() {
    let w = app_or_skip!();
    let catalog = ok(
        &w,
        "fonts_catalog",
        json!({ "imported": ["不存在的字体.ttf"] }),
    );
    let installed = catalog["installed"].as_array().unwrap();
    assert!(
        installed
            .iter()
            .all(|f| f["name"].is_string() && f["aliases"].is_array())
    );
    if cfg!(windows) {
        assert!(
            installed.iter().any(|f| f["name"] == "Meiryo"),
            "Windows 上应该有 Meiryo"
        );
    }
    if project_root()
        .join("assets/fonts/KleeOne-SemiBold.ttf")
        .is_file()
    {
        let files = catalog["files"].as_array().unwrap();
        assert!(
            files
                .iter()
                .any(|f| f["family"] == "Klee One" && f["bundled"] == json!(true)),
            "{files:?}"
        );
    }
    let problems = catalog["problems"].as_array().unwrap();
    assert!(
        problems
            .iter()
            .any(|p| p.as_str().unwrap().contains("不存在的字体.ttf")),
        "{problems:?}"
    );
}

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

#[test]
fn audio_state_is_readable_even_without_a_device() {
    let w = app_or_skip!();
    // 状态查询永远不该报错——UI 不该为了画一个空进度条去处理异常
    let state = ok(&w, "audio_state", json!({}));
    for key in [
        "songId",
        "playState",
        "positionSec",
        "rate",
        "volume",
        "loopRegion",
    ] {
        assert!(state.get(key).is_some(), "PlaybackState 缺字段 {key}");
    }
}

#[test]
fn spectrum_is_readable_even_without_a_device() {
    let w = app_or_skip!();
    let spectrum = ok(&w, "audio_spectrum", json!({}));
    let bands = spectrum.as_array().expect("应当返回数组");
    assert!(!bands.is_empty());
    for band in bands {
        let v = band.as_f64().unwrap();
        assert!((0.0..=1.0).contains(&v), "频段值越界: {v}");
    }
}

#[test]
fn loading_an_unknown_track_reports_which_one() {
    let w = app_or_skip!();
    let err = invoke(&w, "audio_load", json!({ "songId": "不存在" })).expect_err("应当报错");
    let message = err["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("不存在"),
        "错误信息应当指明是哪一首: {message}"
    );
}

#[test]
fn playback_controls_are_wired() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        eprintln!("跳过：没有可用的输出设备");
        return;
    }
    // 每个控制都要能调到。没加载音频时调它们不该 panic。
    for cmd in ["audio_play", "audio_pause", "audio_toggle", "audio_stop"] {
        ok(&w, cmd, json!({}));
    }
}

#[test]
fn rate_and_volume_round_trip_through_ipc() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        return;
    }
    ok(&w, "audio_set_rate", json!({ "rate": 0.75 }));
    assert_eq!(ok(&w, "audio_state", json!({}))["rate"], json!(0.75));

    // 越界值应当被钳到合法范围，而不是原样接受
    ok(&w, "audio_set_rate", json!({ "rate": 99.0 }));
    let rate = ok(&w, "audio_state", json!({}))["rate"].as_f64().unwrap();
    assert!(rate <= 2.0, "倍速应当被钳制: {rate}");

    ok(&w, "audio_set_volume", json!({ "volume": 0.3 }));
    let volume = ok(&w, "audio_state", json!({}))["volume"].as_f64().unwrap();
    assert!((volume - 0.3).abs() < 0.02);
}

#[test]
fn loop_region_round_trips_and_rejects_degenerate_spans() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        return;
    }
    assert_eq!(
        ok(
            &w,
            "audio_set_loop",
            json!({ "startSec": 10.0, "endSec": 20.0 })
        ),
        json!(true)
    );
    let region = &ok(&w, "audio_state", json!({}))["loopRegion"];
    assert_eq!(region["startSec"], json!(10.0));
    assert_eq!(region["endSec"], json!(20.0));

    // 过短的区间要被拒绝并如实返回 false
    assert_eq!(
        ok(
            &w,
            "audio_set_loop",
            json!({ "startSec": 10.0, "endSec": 10.01 })
        ),
        json!(false)
    );

    // 两个参数都省略 = 取消循环
    ok(&w, "audio_set_loop", json!({}));
    assert!(ok(&w, "audio_state", json!({}))["loopRegion"].is_null());
}

#[test]
fn the_full_playback_loop_works_through_ipc() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        return;
    }
    // 找一首音频文件真的存在的曲目
    let tracks = ok(&w, "list_tracks", json!({ "limit": 50 }));
    let Some(song_id) = tracks.as_array().unwrap().iter().find_map(|t| {
        let path = t["audioPath"].as_str().unwrap_or_default();
        (!path.is_empty() && std::path::Path::new(path).exists())
            .then(|| t["id"].as_str().unwrap().to_string())
    }) else {
        eprintln!("跳过：曲库里没有音频文件真实存在的曲目");
        return;
    };

    ok(&w, "audio_set_volume", json!({ "volume": 0.0 })); // 测试不出声
    ok(&w, "audio_load", json!({ "songId": &song_id }));
    let state = ok(&w, "audio_state", json!({}));
    assert_eq!(state["songId"], json!(song_id));
    assert_eq!(state["playState"], json!("playing"));

    // 位置应当推进
    let advanced = (0..150).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        ok(&w, "audio_state", json!({}))["positionSec"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.05
    });
    assert!(advanced, "播放后位置应当推进");

    // 频谱应当有能量
    let has_energy = (0..150).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        ok(&w, "audio_spectrum", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_f64().unwrap_or(0.0) > 0.05)
    });
    assert!(has_energy, "真实音频应当产生非零频谱");

    ok(&w, "audio_seek", json!({ "positionSec": 30.0 }));
    ok(&w, "audio_stop", json!({}));
    assert_eq!(
        ok(&w, "audio_state", json!({}))["playState"],
        json!("empty")
    );
}

// ────────────────────────── 页面依赖的返回结构 ──────────────────────────
//
// jp-corpus 已经测过这些查询对不对；这里测的是**序列化后的形状**能不能
// 对上 app/src/api.ts 里的 TS 接口。字段名写错在 Rust 侧编译得过，
// 到前端才变成 undefined。

#[test]
fn kwic_hit_has_every_field_the_page_renders() {
    let w = app_or_skip!();
    let hits = ok(
        &w,
        "kwic",
        json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 20 } }),
    );
    let list = hits.as_array().unwrap();
    if list.is_empty() {
        eprintln!("跳过：语料里没有「夜」");
        return;
    }
    for key in [
        "songId",
        "artist",
        "title",
        "utteranceId",
        "timeSec",
        "left",
        "keyword",
        "right",
        "repeatCount",
    ] {
        assert!(list[0].get(key).is_some(), "KwicHit 缺字段 {key}");
    }
    // 关键词必须非空——KWIC 视图靠它居中对齐
    assert!(!list[0]["keyword"].as_str().unwrap_or("").is_empty());
}

#[test]
fn kwic_options_are_all_wired_through_ipc() {
    let w = app_or_skip!();
    // 页面上的每个开关都要真的传到后端
    for query in [
        json!({ "keywords": ["夜"], "field": "lemma", "limit": 5 }),
        json!({ "keywords": ["夜"], "field": "surface", "pos": "NOUN", "limit": 5 }),
        json!({ "keywords": ["夜"], "field": "surface", "crossLine": true, "limit": 5 }),
        json!({ "keywords": ["夜"], "field": "surface", "dedup": false, "limit": 5 }),
    ] {
        let hits = ok(&w, "kwic", json!({ "query": query.clone() }));
        assert!(hits.is_array(), "查询 {query} 应当返回数组");
    }
}

#[test]
fn kwic_query_omitting_optional_fields_uses_defaults() {
    let w = app_or_skip!();
    // KwicQuery 有 #[serde(default)]，前端可以只传必填项
    let hits = ok(
        &w,
        "kwic",
        json!({ "query": { "keywords": ["夜"], "field": "surface" } }),
    );
    assert!(hits.is_array());
}

#[test]
fn lyric_hit_has_every_field_the_page_renders() {
    let w = app_or_skip!();
    let hits = ok(&w, "search_lyrics", json!({ "text": "夜", "limit": 10 }));
    let list = hits.as_array().unwrap();
    if list.is_empty() {
        return;
    }
    for key in [
        "songId",
        "artist",
        "title",
        "utteranceId",
        "timeSec",
        "text",
        "rank",
    ] {
        assert!(list[0].get(key).is_some(), "LyricHit 缺字段 {key}");
    }
}

#[test]
fn word_frequency_has_every_field_the_page_renders() {
    let w = app_or_skip!();
    let rows = ok(&w, "word_frequency", json!({ "limit": 10 }));
    let list = rows.as_array().unwrap();
    assert!(!list.is_empty(), "应当有词频数据");
    for key in ["lemma", "pos", "freq", "songCount", "surfaces"] {
        assert!(list[0].get(key).is_some(), "WordFrequency 缺字段 {key}");
    }
    assert!(list[0]["surfaces"].is_array());
    // 按频次降序——图表的条形宽度依赖这个顺序
    let freqs: Vec<i64> = list.iter().map(|r| r["freq"].as_i64().unwrap()).collect();
    let mut sorted = freqs.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(freqs, sorted, "词频应当降序");
}

#[test]
fn word_frequency_pos_filter_narrows_the_result() {
    let w = app_or_skip!();
    let all = ok(&w, "word_frequency", json!({ "limit": 50 }));
    let nouns = ok(&w, "word_frequency", json!({ "pos": "NOUN", "limit": 50 }));
    for row in nouns.as_array().unwrap() {
        assert_eq!(row["pos"], json!("NOUN"), "筛选后不该出现别的词性");
    }
    assert!(all.as_array().unwrap().len() >= nouns.as_array().unwrap().len());
}

#[test]
fn timeline_has_every_field_the_chart_renders() {
    let w = app_or_skip!();
    let rows = ok(&w, "timeline", json!({}));
    let list = rows.as_array().unwrap();
    if list.is_empty() {
        return;
    }
    for key in ["year", "trackCount", "albumCount", "vocabulary"] {
        assert!(list[0].get(key).is_some(), "YearStats 缺字段 {key}");
    }
    // 按年份升序——时间线是从左到右的
    let years: Vec<&str> = list.iter().map(|r| r["year"].as_str().unwrap()).collect();
    let mut sorted = years.clone();
    sorted.sort_unstable();
    assert_eq!(years, sorted, "时间线应当按年份升序");
}

#[test]
fn person_summary_and_collaborator_shapes_match_the_explorer() {
    let w = app_or_skip!();
    for role in ["performer", "composer", "lyricist", "arranger"] {
        let people = ok(&w, "people_by_role", json!({ "role": role, "limit": 5 }));
        let list = people.as_array().unwrap();
        if list.is_empty() {
            continue;
        }
        for key in ["id", "name", "role", "trackCount"] {
            assert!(
                list[0].get(key).is_some(),
                "{role}: PersonSummary 缺字段 {key}"
            );
        }

        let person_id = list[0]["id"].as_i64().unwrap();
        let works = ok(&w, "works_by_person", json!({ "personId": person_id }));
        assert!(works.is_array());

        let peers = ok(
            &w,
            "collaborators",
            json!({ "personId": person_id, "limit": 5 }),
        );
        for peer in peers.as_array().unwrap() {
            for key in ["personId", "name", "sharedTracks", "roles"] {
                assert!(peer.get(key).is_some(), "Collaborator 缺字段 {key}");
            }
            assert!(peer["roles"].is_array());
        }
    }
}

#[test]
fn cross_page_navigation_targets_resolve() {
    let w = app_or_skip!();
    // 检索页点一条命中 → 曲库页打开那首歌并定位到那一行。
    // 这条链路要求 KwicHit.songId 能被 get_track 解析。
    let hits = ok(
        &w,
        "kwic",
        json!({ "query": { "keywords": ["夜"], "field": "surface", "limit": 5 } }),
    );
    let Some(hit) = hits.as_array().unwrap().first() else {
        return;
    };
    let song_id = hit["songId"].as_str().unwrap();
    let track = ok(&w, "get_track", json!({ "songId": song_id }));
    assert!(!track.is_null(), "KWIC 命中的 songId 应当能查到曲目");

    // 命中的 utteranceId 应当真的在那首歌的歌词里
    let lyrics = ok(&w, "lyrics", json!({ "songId": song_id }));
    let target = hit["utteranceId"].as_i64().unwrap();
    assert!(
        lyrics
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["utteranceId"].as_i64() == Some(target)),
        "命中的行应当在该曲目的歌词里——否则跨页跳转会滚不到位置"
    );
}

// ────────────────────────── 维护 ──────────────────────────

#[test]
fn duration_backfill_is_idempotent_and_reports_every_bucket() {
    let w = app_or_skip!();
    let report = ok(&w, "backfill_durations", json!({}));
    for key in [
        "scanned", "written", "missing", "unknown", "failed", "samples",
    ] {
        assert!(report.get(key).is_some(), "报告缺字段 {key}");
    }
    // 每一种失败都要单独计数——笼统的「成功 N 个」用户无从修
    assert!(report["samples"].is_array());

    // 再跑一次：已经填过的行不该被重新处理
    let again = ok(&w, "backfill_durations", json!({}));
    assert_eq!(
        again["scanned"],
        json!(0),
        "回填应当幂等，第二次不该再扫到待处理的行（除非上一次有失败）"
    );
    assert_eq!(again["written"], json!(0));
}

#[test]
fn tracks_expose_duration_after_backfill() {
    let w = app_or_skip!();
    ok(&w, "backfill_durations", json!({}));
    let tracks = ok(&w, "list_tracks", json!({ "limit": 20 }));
    let with_audio: Vec<_> = tracks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| {
            let p = t["audioPath"].as_str().unwrap_or_default();
            !p.is_empty() && std::path::Path::new(p).exists()
        })
        .collect();
    if with_audio.is_empty() {
        return;
    }
    for track in &with_audio {
        let duration = track["durationSec"].as_f64();
        assert!(
            duration.is_some_and(|d| d > 0.0),
            "[{}] 回填后应当有时长，实际 {:?}",
            track["id"],
            track["durationSec"]
        );
    }
}

#[test]
fn overview_total_duration_is_no_longer_zero() {
    let w = app_or_skip!();
    ok(&w, "backfill_durations", json!({}));
    // totalDurationSec 是 f64，用 as_i64 读会得到 None
    let total = ok(&w, "overview", json!({}))["totalDurationSec"]
        .as_f64()
        .unwrap_or(0.0);
    assert!(total > 0.0, "回填后总时长不该还是 0——分析页靠它显示");
}

// ────────────────────────── 首页 / 收藏 / 收听统计 ──────────────────────────

#[test]
fn home_summary_returns_every_section() {
    let w = app_or_skip!();
    let home = ok(&w, "home_summary", json!({}));
    for key in [
        "overview",
        "recentlyPlayed",
        "mostPlayed",
        "favorites",
        "albums",
        "genres",
        "decades",
        "topWords",
    ] {
        assert!(home.get(key).is_some(), "HomeSummary 缺字段 {key}");
    }
    assert!(home["overview"]["tracks"].as_i64().unwrap() > 0);
    assert!(!home["albums"].as_array().unwrap().is_empty(), "应当有专辑");
}

#[test]
fn facets_cover_every_track() {
    let w = app_or_skip!();
    let home = ok(&w, "home_summary", json!({}));
    let total = home["overview"]["tracks"].as_i64().unwrap();

    // 年代分面必须覆盖全部曲目——年份为空的归到「年代不详」而不是被丢掉，
    // 丢掉的话用户会发现各年代加起来对不上总数
    let decades: i64 = home["decades"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["trackCount"].as_i64().unwrap())
        .sum();
    assert_eq!(decades, total, "年代分面应当覆盖全部曲目");

    for facet in home["decades"].as_array().unwrap() {
        for key in ["label", "trackCount", "totalDurationSec"] {
            assert!(facet.get(key).is_some(), "FacetCount 缺字段 {key}");
        }
    }
}

#[test]
fn favorites_toggle_round_trips() {
    let w = app_or_skip!();
    let song_id = ok(&w, "list_tracks", json!({ "limit": 1 }))[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let args = json!({ "entityType": "song", "entityId": &song_id });

    let before = ok(&w, "is_favorite", args.clone()).as_bool().unwrap();
    let after = ok(&w, "toggle_favorite", args.clone()).as_bool().unwrap();
    assert_ne!(before, after, "切换应当改变状态");
    assert_eq!(ok(&w, "is_favorite", args.clone()), json!(after));

    // 收藏后应当出现在列表里
    if after {
        let home = ok(&w, "home_summary", json!({}));
        let favorites = &home["favorites"];
        assert!(
            favorites
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["id"] == json!(song_id)),
            "收藏的曲目应当出现在 favorites 里"
        );
    }

    // 切回去，不留痕迹
    ok(&w, "toggle_favorite", args.clone());
    assert_eq!(ok(&w, "is_favorite", args), json!(before));
}

#[test]
fn audio_tick_returns_the_same_shape_as_audio_state() {
    let w = app_or_skip!();
    // audio_tick 有副作用（累加收听时长），但返回结构必须一致，
    // 否则前端换用它的时候会静默出错
    let tick = ok(&w, "audio_tick", json!({}));
    let state = ok(&w, "audio_state", json!({}));
    let tick_keys: Vec<_> = tick.as_object().unwrap().keys().collect();
    let state_keys: Vec<_> = state.as_object().unwrap().keys().collect();
    assert_eq!(tick_keys, state_keys, "两个命令的返回结构应当相同");
}

#[test]
fn a_brief_touch_does_not_pollute_the_history() {
    let w = app_or_skip!();
    if !audio_ready(&w) {
        return;
    }
    let before = ok(&w, "home_summary", json!({}))["recentlyPlayed"]
        .as_array()
        .unwrap()
        .len();

    // 找一首能播的，点开就立刻停——不该进历史
    let tracks = ok(&w, "list_tracks", json!({ "limit": 50 }));
    let Some(song_id) = tracks.as_array().unwrap().iter().find_map(|t| {
        let p = t["audioPath"].as_str().unwrap_or_default();
        (!p.is_empty() && std::path::Path::new(p).exists())
            .then(|| t["id"].as_str().unwrap().to_string())
    }) else {
        return;
    };

    ok(&w, "audio_set_volume", json!({ "volume": 0.0 }));
    ok(&w, "audio_load", json!({ "songId": &song_id }));
    for _ in 0..5 {
        ok(&w, "audio_tick", json!({}));
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    ok(&w, "audio_stop", json!({}));
    ok(&w, "audio_tick", json!({}));

    let after = ok(&w, "home_summary", json!({}))["recentlyPlayed"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before, "听不到 5 秒不该进历史");
}

// ────────────────────────── 全局搜索（Cmd+K） ──────────────────────────

#[test]
fn quick_search_returns_every_group() {
    let w = app_or_skip!();
    let results = ok(&w, "quick_search", json!({ "query": "夜", "perKind": 3 }));
    for key in ["tracks", "albums", "people", "words", "lyrics"] {
        assert!(
            results.get(key).is_some(),
            "QuickSearchResults 缺分组 {key}"
        );
        assert!(results[key].is_array());
    }
}

#[test]
fn quick_hits_carry_their_kind_tag() {
    let w = app_or_skip!();
    let results = ok(&w, "quick_search", json!({ "query": "夜", "perKind": 3 }));
    // 前端靠 kind 判别联合类型，缺了就没法渲染
    for (group, expected) in [
        ("tracks", "track"),
        ("albums", "album"),
        ("people", "person"),
        ("words", "word"),
        ("lyrics", "lyric"),
    ] {
        for hit in results[group].as_array().unwrap() {
            assert_eq!(hit["kind"], json!(expected), "{group} 的 kind 标签不对");
        }
    }
}

#[test]
fn quick_hits_expose_what_the_palette_renders() {
    let w = app_or_skip!();
    let results = ok(&w, "quick_search", json!({ "query": "夜", "perKind": 5 }));
    for hit in results["tracks"].as_array().unwrap() {
        for key in ["songId", "title", "artist", "album", "durationSec"] {
            assert!(hit.get(key).is_some(), "track hit 缺字段 {key}");
        }
    }
    for hit in results["words"].as_array().unwrap() {
        for key in ["lemma", "pos", "freq", "songCount"] {
            assert!(hit.get(key).is_some(), "word hit 缺字段 {key}");
        }
    }
    for hit in results["people"].as_array().unwrap() {
        assert!(hit["roles"].is_array(), "person hit 的 roles 应当是数组");
    }
}

#[test]
fn quick_search_targets_are_reachable() {
    let w = app_or_skip!();
    // 面板里点一条就要能跳过去。跳不过去的搜索结果是死胡同。
    let results = ok(&w, "quick_search", json!({ "query": "夜", "perKind": 3 }));

    for hit in results["tracks"].as_array().unwrap() {
        let song_id = hit["songId"].as_str().unwrap();
        assert!(
            !ok(&w, "get_track", json!({ "songId": song_id })).is_null(),
            "曲目结果应当能打开"
        );
    }
    for hit in results["lyrics"].as_array().unwrap() {
        let song_id = hit["songId"].as_str().unwrap();
        let target = hit["utteranceId"].as_i64().unwrap();
        let lyrics = ok(&w, "lyrics", json!({ "songId": song_id }));
        assert!(
            lyrics
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l["utteranceId"].as_i64() == Some(target)),
            "歌词结果的行应当在该曲目里——否则跳过去滚不到位置"
        );
    }
    for hit in results["words"].as_array().unwrap() {
        let lemma = hit["lemma"].as_str().unwrap();
        let word = ok(&w, "word_in_corpus", json!({ "lemma": lemma, "limit": 1 }));
        assert!(
            word["occurrences"].as_i64().unwrap() > 0,
            "词结果应当能查到"
        );
    }
}

#[test]
fn quick_search_survives_hostile_input() {
    let w = app_or_skip!();
    for query in ["", "   ", "%", "_", "'; DROP TABLE songs; --", "\\", "🎵"] {
        let results = invoke(&w, "quick_search", json!({ "query": query }));
        assert!(results.is_ok(), "{query:?} 不该报错");
    }
    // 库还在
    assert!(ok(&w, "overview", json!({}))["tracks"].as_i64().unwrap() > 0);
}

#[test]
fn quick_search_per_kind_limit_is_respected() {
    let w = app_or_skip!();
    let results = ok(&w, "quick_search", json!({ "query": "a", "perKind": 2 }));
    for key in ["tracks", "albums", "people", "words", "lyrics"] {
        assert!(
            results[key].as_array().unwrap().len() <= 2,
            "{key} 超过了每组限制"
        );
    }
}

#[test]
fn person_by_id_resolves_palette_targets() {
    let w = app_or_skip!();
    // 命令面板点一条人物结果 → 按 id 定位。拿不到就是死胡同。
    let results = ok(
        &w,
        "quick_search",
        json!({ "query": "ヨルシカ", "perKind": 3 }),
    );
    for hit in results["people"].as_array().unwrap() {
        let person_id = hit["personId"].as_i64().unwrap();
        let person = ok(&w, "person_by_id", json!({ "personId": person_id }));
        assert!(!person.is_null(), "人物结果应当能按 id 取到");
        assert_eq!(person["id"], json!(person_id));
        for key in ["id", "name", "role", "trackCount"] {
            assert!(person.get(key).is_some(), "PersonSummary 缺字段 {key}");
        }
        // 主角色必须是这个人真的担任过的——前端靠它切左侧列表
        let role = person["role"].as_str().unwrap();
        if !role.is_empty() {
            let listed = ok(&w, "people_by_role", json!({ "role": role, "limit": 500 }));
            assert!(
                listed
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"].as_i64() == Some(person_id)),
                "按主角色列出的人里应当包含他，否则切过去看不到"
            );
        }
    }
}

#[test]
fn person_by_id_returns_null_for_unknown() {
    let w = app_or_skip!();
    assert!(ok(&w, "person_by_id", json!({ "personId": 999_999 })).is_null());
}

#[test]
fn album_targets_resolve_to_tracks() {
    let w = app_or_skip!();
    // 面板点专辑 → 曲库列表限定到它的曲目
    let results = ok(&w, "quick_search", json!({ "query": "a", "perKind": 3 }));
    for hit in results["albums"].as_array().unwrap() {
        let album_id = hit["albumId"].as_i64().unwrap();
        let tracks = ok(&w, "album_tracks", json!({ "albumId": album_id }));
        assert_eq!(
            tracks.as_array().unwrap().len() as i64,
            hit["trackCount"].as_i64().unwrap(),
            "专辑结果报的曲目数应当和实际取到的一致"
        );
    }
}

// ────────────────────────── 导入 ──────────────────────────
//
// 这几个测试跑在**真库**上，所以每一条都必须是不写库的。
// 唯一调用 `run_import` 的那条，计划里刚好一条 New 都没有——
// 这本身就是要验的性质：重扫已导入的曲库，确认之后也什么都不该写。

#[test]
fn scanning_an_already_imported_library_plans_nothing() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        eprintln!("跳过：找不到 {}", audio.display());
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

#[test]
fn confirming_an_all_duplicate_scan_writes_nothing() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        return;
    }
    let before = ok(&w, "health", json!({}))["tracks"].as_i64().unwrap();
    ok(
        &w,
        "scan_folder",
        json!({ "path": audio.display().to_string() }),
    );

    let report = ok(&w, "run_import", json!({}));
    let tracks = report["tracks"].as_array().expect("tracks 应当是数组");
    assert!(tracks.is_empty(), "全是重复时不该写任何一条");

    let after = ok(&w, "health", json!({}))["tracks"].as_i64().unwrap();
    assert_eq!(before, after, "曲目数被改动了");
}

#[test]
fn scanning_a_missing_folder_is_an_error_not_a_panic() {
    let w = app_or_skip!();
    let err = invoke(&w, "scan_folder", json!({ "path": "X:/nope/nope" }))
        .expect_err("不存在的目录应当报错");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("不存在"),
        "错误信息要说清是什么问题：{err}"
    );
}

#[test]
fn importing_without_scanning_first_is_an_error() {
    let w = app_or_skip!();
    ok(&w, "cancel_import", json!({}));
    let err = invoke(&w, "run_import", json!({})).expect_err("没扫描就导入应当报错");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("scan_folder"),
        "错误信息要告诉调用方下一步做什么：{err}"
    );
}

#[test]
fn cancelling_discards_the_pending_scan() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        return;
    }
    ok(
        &w,
        "scan_folder",
        json!({ "path": audio.display().to_string() }),
    );
    ok(&w, "cancel_import", json!({}));
    invoke(&w, "run_import", json!({})).expect_err("取消之后不该还能导入");
}

#[test]
fn scan_folder_accepts_the_camel_case_depth_argument() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        return;
    }
    // maxDepth=1 只看顶层目录，那一层没有音频文件
    let shallow = ok(
        &w,
        "scan_folder",
        json!({ "path": audio.display().to_string(), "maxDepth": 1 }),
    );
    let deep = ok(
        &w,
        "scan_folder",
        json!({ "path": audio.display().to_string(), "maxDepth": 6 }),
    );
    assert!(
        shallow["summary"]["total"].as_i64() < deep["summary"]["total"].as_i64(),
        "maxDepth 没起作用，说明 camelCase 参数没对上"
    );
    ok(&w, "cancel_import", json!({}));
}

/// 单曲导入：选中具体的文件，而不是整个目录。
///
/// 跑在真库上，所以挑的是**已经导入过的**两首歌——计划必然全是
/// alreadyImported，一行都不会写。要验的是「按文件选也能扫、也能对上」。
#[test]
fn scanning_two_files_plans_exactly_those_two() {
    let w = app_or_skip!();
    let audio = project_root().join("raw/audio");
    if !audio.is_dir() {
        eprintln!("跳过：找不到 {}", audio.display());
        return;
    }
    let mut files: Vec<String> = Vec::new();
    for entry in walkdir(&audio).into_iter() {
        if jp_import::is_audio_file(&entry) {
            files.push(entry.display().to_string());
        }
        if files.len() == 2 {
            break;
        }
    }
    if files.len() < 2 {
        eprintln!("跳过：raw/audio 里音频不足两个");
        return;
    }

    let result = ok(&w, "scan_files", json!({ "paths": files.clone() }));
    assert_eq!(result["summary"]["total"].as_i64(), Some(2));
    assert_eq!(result["summary"]["alreadyImported"].as_i64(), Some(2));
    assert_eq!(result["ignored"].as_array().map(|a| a.len()), Some(0));

    // 同一个文件选两次只算一次，否则两条会互相判成「本批重复」
    let dupes = ok(
        &w,
        "scan_files",
        json!({ "paths": vec![files[0].clone(), files[0].clone()] }),
    );
    assert_eq!(dupes["summary"]["total"].as_i64(), Some(1));
    assert_eq!(dupes["summary"]["duplicatesInBatch"].as_i64(), Some(0));

    ok(&w, "cancel_import", json!({}));
}

/// 选中的不是音频（.lrc、封面图）时要报出来，不能默不作声地丢掉——
/// 否则「选了 3 个只导了 2 个」看起来像 bug。
#[test]
fn a_chosen_file_that_is_not_audio_is_reported_not_swallowed() {
    let w = app_or_skip!();
    let lrc = project_root().join("raw/lyrics_lrc/001.lrc");
    if !lrc.is_file() {
        eprintln!("跳过：找不到 {}", lrc.display());
        return;
    }
    let result = ok(
        &w,
        "scan_files",
        json!({ "paths": vec![lrc.display().to_string()] }),
    );
    assert_eq!(result["summary"]["total"].as_i64(), Some(0));
    let ignored = result["ignored"].as_array().expect("ignored 应当是数组");
    assert_eq!(ignored.len(), 1);
    assert_eq!(ignored[0].as_str(), Some("001.lrc"));
    ok(&w, "cancel_import", json!({}));
}

#[test]
fn scanning_an_empty_file_list_is_an_error() {
    let w = app_or_skip!();
    let err = invoke(&w, "scan_files", json!({ "paths": Vec::<String>::new() }))
        .expect_err("空列表应当报错");
    assert!(
        err["message"].as_str().unwrap_or_default().contains("一个文件都没选"),
        "错误信息要说清问题：{err}"
    );
}

/// 浅遍历，只为在 raw/audio 里找头两个音频文件
fn walkdir(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

// ───────────────────────── 单曲导入 + 补歌词 ─────────────────────────

/// 走完整条链：选一个文件 → 导入 → 给它导一份歌词 → 读回来 → 搜得到。
///
/// 全程走真实 IPC，跑在**真库的副本**上。这条链里每一步都是
/// 「编译通过但一点就报错」的高危处：命令有没有注册、camelCase 参数对不对、
/// 返回值的形状前端认不认、FTS 有没有跟着同步。
#[test]
fn importing_one_file_then_its_lyrics_works_end_to_end() {
    let Some(scratch) = scratch_app("lyrics-flow") else {
        return;
    };
    let w = scratch.w();

    // 造一个「音乐 App 下载目录」：只有音频，旁边没有 .lrc
    let drop_dir = scratch.dir().join("downloads");
    std::fs::create_dir_all(&drop_dir).unwrap();
    let audio = drop_dir.join("テスト歌手 - テスト曲.flac");
    std::fs::write(&audio, b"not really audio").unwrap();

    let scan = ok(
        w,
        "scan_files",
        json!({ "paths": vec![audio.display().to_string()] }),
    );
    assert_eq!(scan["summary"]["total"].as_i64(), Some(1));
    assert_eq!(scan["summary"]["new"].as_i64(), Some(1), "单个文件没被判成新歌：{scan}");
    assert_eq!(scan["items"][0]["hasLyrics"].as_bool(), Some(false));

    // tokenize=false：副本目录里没有 venv，分词器本来就不可用
    let report = ok(w, "run_import", json!({ "tokenize": false }));
    let tracks = report["tracks"].as_array().expect("tracks 应当是数组");
    assert_eq!(tracks.len(), 1, "{report}");
    assert_eq!(tracks[0]["outcome"]["kind"], "imported");
    assert_eq!(tracks[0]["outcome"]["lyricLines"].as_i64(), Some(0));
    let song_id = tracks[0]["songId"].as_str().expect("要有 songId").to_string();

    // 刚导进来的歌必然在「缺歌词」名单里
    let missing = ok(w, "lyrics_missing", json!({}));
    let row = missing
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["songId"].as_str() == Some(song_id.as_str()))
        .unwrap_or_else(|| panic!("缺歌词名单里没有刚导入的那首：{missing}"));
    assert_eq!(row["title"].as_str(), Some("テスト曲"));
    assert_eq!(row["siblingLrc"], Value::Null, "旁边没有 .lrc 却说有");

    // 用户自己给一份歌词
    let lrc = scratch.dir().join("hand.lrc");
    std::fs::write(
        &lrc,
        "作詞 : テスト作詞家\n[00:12.00]ひとつめの行\n[00:20.50]ふたつめの行\n",
    )
    .unwrap();
    let attached = ok(
        w,
        "lyrics_import_file",
        json!({ "songId": song_id, "path": lrc.display().to_string() }),
    );
    assert_eq!(attached["source"], "manual");
    assert_eq!(attached["lyricLines"].as_i64(), Some(2));
    assert_eq!(attached["credits"].as_i64(), Some(1), "LRC 里的作词没进署名");
    // 落盘的那一份要在语料库目录下，文件名是 song_id
    let on_disk = scratch
        .dir()
        .join("raw")
        .join("lyrics_lrc")
        .join(format!("{song_id}.lrc"));
    assert!(on_disk.is_file(), "没落盘：{}", on_disk.display());

    // 读回来
    let lines = ok(w, "lyrics", json!({ "songId": song_id }));
    let lines = lines.as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["text"].as_str(), Some("ひとつめの行"));
    assert!((lines[1]["timeSec"].as_f64().unwrap() - 20.5).abs() < 1e-6);

    // 搜得到——外部内容的 FTS 表没有触发器，漏同步这里就是空
    let hits = ok(w, "search_lyrics", json!({ "text": "ふたつめ", "limit": 10 }));
    assert!(
        hits.as_array()
            .unwrap()
            .iter()
            .any(|h| h["songId"].as_str() == Some(song_id.as_str())),
        "新挂上的歌词搜不到：{hits}"
    );

    // 名单里不该还有它
    let missing = ok(w, "lyrics_missing", json!({}));
    assert!(
        !missing
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["songId"].as_str() == Some(song_id.as_str())),
        "挂上歌词之后还算缺歌词"
    );

    // 换一份：不能变成两份，旧的也不能留在全文索引里
    let lrc2 = scratch.dir().join("hand2.lrc");
    std::fs::write(&lrc2, "[00:05.00]あたらしい行\n").unwrap();
    let again = ok(
        w,
        "lyrics_import_file",
        json!({ "songId": song_id, "path": lrc2.display().to_string() }),
    );
    assert_eq!(again["lyricLines"].as_i64(), Some(1));
    let lines = ok(w, "lyrics", json!({ "songId": song_id }));
    assert_eq!(lines.as_array().unwrap().len(), 1, "换歌词变成了两份");
    let ghosts = ok(w, "search_lyrics", json!({ "text": "ふたつめ", "limit": 10 }));
    assert!(
        !ghosts
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["songId"].as_str() == Some(song_id.as_str())),
        "旧歌词还留在全文索引里：{ghosts}"
    );
}

/// 音频旁边就有 .lrc 时，「补齐」不该上网——本地那一份必然是对的。
#[test]
fn a_sibling_lrc_is_used_without_going_online() {
    let Some(scratch) = scratch_app("lyrics-sibling") else {
        return;
    };
    let w = scratch.w();

    let drop_dir = scratch.dir().join("downloads");
    std::fs::create_dir_all(&drop_dir).unwrap();
    let audio = drop_dir.join("ローカル歌手 - ローカル曲.flac");
    std::fs::write(&audio, b"not really audio").unwrap();

    let scan = ok(
        w,
        "scan_files",
        json!({ "paths": vec![audio.display().to_string()] }),
    );
    assert_eq!(scan["summary"]["new"].as_i64(), Some(1));
    let report = ok(w, "run_import", json!({ "tokenize": false }));
    let song_id = report["tracks"][0]["songId"].as_str().unwrap().to_string();

    // 导入之后才下的歌词：文件现在才出现在音频旁边
    std::fs::write(audio.with_extension("lrc"), "[00:01.00]あとから来た歌詞\n").unwrap();
    let missing = ok(w, "lyrics_missing", json!({}));
    let row = missing
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["songId"].as_str() == Some(song_id.as_str()))
        .expect("应当还在缺歌词名单里");
    assert!(
        row["siblingLrc"].as_str().is_some(),
        "没认出音频旁边的 .lrc：{row}"
    );

    let attached = ok(w, "lyrics_fill_one", json!({ "songId": song_id }));
    assert_eq!(attached["source"], "sibling", "本地有 .lrc 却跑去上网：{attached}");
    assert_eq!(attached["lyricLines"].as_i64(), Some(1));
}

/// 已经有歌词的歌不走在线补齐——绝不用网上搜到的悄悄覆盖用户手上那一份。
#[test]
fn filling_online_is_refused_for_a_song_that_already_has_lyrics() {
    let w = app_or_skip!();
    let tracks = ok(&w, "list_tracks", json!({ "limit": 400 }));
    let with_lyrics = tracks.as_array().unwrap().iter().find(|t| {
        let id = t["id"].as_str().unwrap_or_default();
        !ok(&w, "lyrics", json!({ "songId": id }))
            .as_array()
            .unwrap()
            .is_empty()
    });
    let Some(track) = with_lyrics else {
        eprintln!("跳过：库里没有带歌词的歌");
        return;
    };
    let err = invoke(
        &w,
        "lyrics_fill_one",
        json!({ "songId": track["id"].as_str().unwrap() }),
    )
    .expect_err("已经有歌词时应当拒绝");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("已经有歌词"),
        "错误信息要说清为什么拒绝：{err}"
    );
}

// ────────────────────────── 刮削 ──────────────────────────
//
// 只读的几条跑在真库上（刮削状态表是空的，读不出东西也是有效结果）。
// 会写库的那条单独隔离：复制一份 corpus.db 到临时目录，而且标了
// #[ignore]，因为它**真的发网络请求**——测试套件不该依赖外网。
//
//     cargo test -p jp-app --test commands -- --ignored --nocapture

#[test]
fn scrape_summary_reports_every_status() {
    let w = app_or_skip!();
    let summary = ok(&w, "scrape_summary", json!({}));
    // 六个状态一个都不能少，UI 的统计块按名字取值
    for key in [
        "pending",
        "running",
        "success",
        "lowConfidence",
        "failed",
        "skipped",
        "total",
    ] {
        assert!(summary[key].is_i64(), "缺字段 {key}: {summary}");
    }
}

#[test]
fn the_review_queue_defaults_to_what_needs_confirming() {
    let w = app_or_skip!();
    // 不传 statuses 时默认取 low_confidence
    let rows = ok(&w, "scrape_review_queue", json!({}));
    let rows = rows.as_array().expect("应当是数组");
    for row in rows {
        assert_eq!(row["status"], "low_confidence");
    }
}

#[test]
fn the_review_queue_accepts_camel_case_arguments() {
    let w = app_or_skip!();
    // statuses / limit 两个参数都要能对上
    let rows = ok(
        &w,
        "scrape_review_queue",
        json!({ "statuses": ["failed", "success"], "limit": 5 }),
    );
    assert!(rows.as_array().unwrap().len() <= 5);
}

#[test]
fn attempts_of_an_unknown_file_is_empty_not_an_error() {
    let w = app_or_skip!();
    let rows = ok(
        &w,
        "scrape_attempts",
        json!({ "filePath": "X:/nope/nope.flac" }),
    );
    assert!(rows.as_array().unwrap().is_empty());
}

#[test]
fn accepting_a_candidate_for_an_unknown_file_is_an_error() {
    let w = app_or_skip!();
    let err = invoke(
        &w,
        "scrape_accept",
        json!({ "filePath": "X:/nope.flac", "candidateIndex": 0 }),
    )
    .expect_err("没有记录时应当报错");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("刮削记录"),
        "错误信息要说清是什么问题：{err}"
    );
}

#[test]
fn a_batch_job_is_not_running_at_rest() {
    let w = app_or_skip!();
    assert_eq!(ok(&w, "scrape_is_running", json!({})), json!(false));
    // 取消一个没在跑的作业不该报错
    ok(&w, "scrape_cancel", json!({}));
}

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

// ────────────────────────── 歌手 / 专辑封面 ──────────────────────────

#[test]
fn the_artist_roster_covers_every_artist_in_the_library() {
    let w = app_or_skip!();
    let rows = ok(&w, "artist_roster", json!({}));
    let rows = rows.as_array().expect("应当是数组");
    assert!(!rows.is_empty(), "库里有歌手，名册不该是空的");
    for row in rows {
        assert!(!row["name"].as_str().unwrap().is_empty());
        // 每个歌手都要能说出他有几首歌，否则「先刮谁」没法排序
        assert!(row["trackCount"].as_i64().unwrap() > 0, "{row}");
        assert!(row["hasImage"].is_boolean());
    }
    // 合作曲「A/B」要拆开成两个人
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert!(
        names.iter().all(|n| !n.contains('/')),
        "斜杠没拆开：{names:?}"
    );
}

#[test]
fn filling_album_artwork_is_idempotent() {
    // **跑在副本上**：这个 command 会写 albums.artwork_path，
    // 测试不该改用户的库。不需要网络，所以能留在常规测试套件里。
    let Some(scratch) = scratch_app("album-art") else {
        return;
    };
    // **前提自己造**：先把副本里的专辑封面清空。
    // 原来这里靠的是「真库里 albums.artwork_path 全是空的」——
    // 用户在界面上点一次「填专辑封面」，这条测试就红了，而代码没有任何问题。
    let cleared = {
        let conn = rusqlite::Connection::open(scratch.dir().join("corpus.db")).unwrap();
        conn.execute("UPDATE albums SET artwork_path=''", []).unwrap()
    };
    assert!(cleared > 0, "副本里该有专辑");

    let w = scratch.w();
    let first = ok(w, "fill_album_artwork", json!({}));
    let second = ok(w, "fill_album_artwork", json!({}));
    println!("清空 {cleared} 张后填了 {first} 张，第二次 {second} 张");
    assert!(first.as_i64().unwrap() > 0, "有曲目封面的专辑该被填上");
    assert_eq!(second.as_i64(), Some(0), "重跑该是幂等的");
    // 关库、删临时目录由 Scratch 的 Drop 负责
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

// ────────────────────────── 分词校正 ──────────────────────────

/// 走真实 IPC、在库的**临时副本**上：打开一行 → 合并前两个词 → 保存 →
/// 立刻能按新词检索到并带 ✏ → 撤销 → 回到原样。
#[test]
fn a_token_correction_round_trips_through_ipc_on_a_scratch_copy() {
    let Some(scratch) = scratch_app("token-correction") else {
        eprintln!("跳过：没有可用的 corpus.db");
        return;
    };
    let w = scratch.w();

    // 找一行至少有两个词的真实歌词
    let hits = ok(
        w,
        "kwic",
        json!({ "query": { "keywords": ["夜"], "limit": 50 } }),
    );
    let mut picked = None;
    for hit in hits.as_array().expect("kwic 返回的不是数组") {
        let utt = hit["utteranceId"].as_i64().unwrap();
        let view = ok(w, "token_correction", json!({ "utteranceId": utt }));
        if view["tokens"].as_array().map_or(0, Vec::len) >= 2 && view["corrected"] == false {
            picked = Some((utt, view));
            break;
        }
    }
    let Some((utt, before)) = picked else {
        eprintln!("跳过：没找到合适的行");
        return;
    };
    let tokens = before["tokens"].as_array().unwrap().clone();
    let first = tokens[0]["surface"].as_str().unwrap();
    let second = tokens[1]["surface"].as_str().unwrap();
    let joined = format!("{first}{second}");

    let mut edited = vec![json!({
        "surface": joined,
        "lemma": tokens[0]["lemma"],
        "pos": tokens[0]["pos"],
    })];
    edited.extend(tokens[2..].iter().cloned());
    ok(
        w,
        "save_token_correction",
        json!({ "utteranceId": utt, "tokens": edited }),
    );

    let after = ok(w, "token_correction", json!({ "utteranceId": utt }));
    assert_eq!(after["corrected"], true);
    assert_eq!(after["tokens"].as_array().unwrap().len(), tokens.len() - 1);
    assert_eq!(after["original"], before["tokens"], "原始分词要原样记下");

    let found = ok(
        w,
        "kwic",
        json!({ "query": { "keywords": [joined], "limit": 5000 } }),
    );
    let row = found
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["utteranceId"].as_i64() == Some(utt))
        .unwrap_or_else(|| panic!("合并出来的「{joined}」检索不到"));
    assert_eq!(row["corrected"], true, "KWIC 要标出校正过的行");

    assert_eq!(
        ok(w, "revert_token_correction", json!({ "utteranceId": utt })),
        true
    );
    let reverted = ok(w, "token_correction", json!({ "utteranceId": utt }));
    assert_eq!(reverted["corrected"], false);
    assert_eq!(reverted["tokens"], before["tokens"], "撤销要回到原样");
}

// ────────────────────────── 曲库维护 ──────────────────────────

/// 走真实 IPC、在库的**临时副本**上（连同 metadata/songs.csv 的副本）：
/// 编辑 → 清单跟着改；换音频 → 时长读出来；弄丢一首的音频 → 扫目录能找回；删歌 → 库里、检索、清单里都没了，
/// 真项目里的封面文件不受影响。
#[test]
fn library_maintenance_round_trips_through_ipc_on_a_scratch_copy() {
    let Some(scratch) = scratch_app("library-maintenance") else {
        eprintln!("跳过：没有可用的 corpus.db");
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
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .expect("csp 不见了");
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

/// 诊断信息要**一次说完排查一个问题要问的那几样**，而且不能带歌名歌词。
///
/// 这几个段落标题是契约：用户贴过来的那段文本靠它们定位，少一段就等于
/// 又要多问一轮。
#[test]
fn the_diagnostics_report_answers_the_questions_i_always_have_to_ask() {
    let w = app_or_skip!();
    let text = ok(&w, "diagnostics_report", json!({}))
        .as_str()
        .expect("诊断信息应该是一段文本")
        .to_string();

    for marker in [
        "JPOP Corpus Tool 诊断信息",
        "── 语料库 ──",
        "── 能力 ──",
        "── 日志",
        "corpus.db",
        "分词词典",
        "音频输出",
        "有歌词没分词",
    ] {
        assert!(text.contains(marker), "诊断信息里少了「{marker}」：\n{text}");
    }

    // 存一份到临时文件，确认写得出来、内容一致
    let out = std::env::temp_dir().join(format!("jp-diag-{}.txt", std::process::id()));
    let saved = ok(&w, "diagnostics_save", json!({ "path": out.to_string_lossy() }));
    assert_eq!(saved.as_str().unwrap(), out.to_string_lossy());
    let on_disk = std::fs::read_to_string(&out).unwrap();
    assert!(on_disk.contains("── 能力 ──"), "存出来的文件不对");
    let _ = std::fs::remove_file(&out);
}
