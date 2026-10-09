//! 合成库上的 command 测试共用的装配。`fixture_commands.rs` 和 `fixture_flows.rs` 各编一份。
//!
//! 数据来自 `jp_corpus::fixture`，建在临时目录里，所以**在任何机器上都真的跑**。
//! 装配走的是 `jp_app_lib::register()`——和生产路径同一份命令清单。

// 每个测试文件是一个独立的 crate，用到的东西不一样
#![allow(dead_code)]

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
/// 所以：一个 app、一个库，全部测试共用，并且**排队跑**（见 `ONE_AT_A_TIME`）。
/// 代价是测试之间不再互相隔离，所以每个会改状态的测试都要**把自己改的改回去**，
/// 或者只断言相对变化。要往库里加歌、删歌的流程放在另一个测试文件里（另一个进程、另一个库），
/// 免得这边数曲目数的断言被它们带偏。
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
        // 固定名字 + 开跑前清一次：不留一堆带 pid 的垃圾目录。
        // 名字里带测试文件名（集成测试的 crate 名就是文件名），两个测试文件各用各的库
        let dir = std::env::temp_dir().join(concat!("jp-", env!("CARGO_CRATE_NAME")));
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
pub struct Fixture {
    _lock: MutexGuard<'static, ()>,
}

impl Fixture {
    pub fn new(_tag: &str) -> Self {
        // 上一个测试 panic 过的话锁会中毒，但锁本身没坏，照用
        let lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        SHARED.get_or_init(Shared::new);
        Self { _lock: lock }
    }

    pub fn w(&self) -> &'static tauri::WebviewWindow<MockRuntime> {
        &SHARED.get().expect("还没装起来").w
    }

    /// 语料库目录（临时目录里的那一个）
    pub fn dir(&self) -> &'static Path {
        &SHARED.get().expect("还没装起来").dir
    }

    /// 另开一个连接直接改库，给测试**自己造前提**用（比如把某首歌的时长清空）。
    /// 用完就关：应用自己的连接一直开着，两边都是 WAL。
    pub fn sql(&self, sql: &str) -> usize {
        let conn = rusqlite::Connection::open(self.dir().join("corpus.db")).expect("打不开合成库");
        conn.execute(sql, []).unwrap_or_else(|err| panic!("{sql} 失败：{err}"))
    }
}

/// 走完整的 IPC 通道调一次 command，和前端 `invoke()` 同一条路径。
pub fn invoke(
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

pub fn ok(webview: &tauri::WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Value {
    invoke(webview, cmd, args).unwrap_or_else(|e| panic!("{cmd} 失败: {e}"))
}

/// 这次调用应当失败，返回报错里的那句话。
///
/// 我们自己的错误序列化成 `{ message }`，`api.ts` 的 `call()` 靠它转成 `Error`——
/// 所以拿不到 `message` 本身就是一个该红的问题。
pub fn refused(webview: &tauri::WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> String {
    let err = invoke(webview, cmd, args).expect_err(&format!("{cmd} 应当报错"));
    err["message"]
        .as_str()
        .unwrap_or_else(|| panic!("{cmd} 的错误里没有 message：{err}"))
        .to_string()
}

pub fn len(value: &Value) -> usize {
    value.as_array().expect("不是数组").len()
}

/// `value` 这个对象里这些字段都得在。字段名写错在 Rust 侧编译得过，到前端才变成 undefined。
pub fn has_keys(value: &Value, what: &str, keys: &[&str]) {
    for key in keys {
        assert!(value.get(key).is_some(), "{what} 缺字段 {key}：{value}");
    }
}

/// 这台机器有没有音频输出设备。没有的话播放类的测试跳过——**并且说一声**：
/// 跳过也打 ok，不说的话分不清「跑过了」还是「没跑」。
pub fn audio_or_skip(webview: &tauri::WebviewWindow<MockRuntime>, test: &str) -> bool {
    let ready = ok(webview, "health", json!({}))["audioReady"].as_bool().unwrap_or(false);
    if !ready {
        eprintln!("[skip] {test}：没有音频输出设备");
    }
    ready
}

/// 写一段 440 Hz 的正弦波（16 位、单声道、22050 Hz 的 WAV）。
///
/// 仓库里的测试音只有 1 秒，播放回路的测试要「放一会儿、位置往前走、频谱有能量」，
/// 1 秒不够稳；现写一段长的，不往仓库里放大文件。
pub fn write_tone(path: &Path, seconds: f64) {
    const RATE: u32 = 22_050;
    let frames = (seconds * RATE as f64) as u32;
    let data_len = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * n as f64 / RATE as f64;
        bytes.extend_from_slice(&((phase.sin() * 0.5 * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap_or_else(|err| panic!("写不了 {}：{err}", path.display()));
}
