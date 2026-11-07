//! 对账：命令搬进 `jp-api` 前后，Tauri 返回给前端的 JSON 要逐字节相同。
//!
//! 只在显式要求时跑，而且只对着库的**副本**跑（`AppState::new` 会迁移、补数据）：
//!
//! ```text
//! JP_PARITY_ROOT=<放着 corpus.db 副本的目录> JP_PARITY_OUT=<输出文件> \
//!   cargo test -p jp-app --test api_parity -- --ignored
//! ```
//!
//! 输出每行一条调用：`命令<TAB>参数<TAB>原样的响应体`。搬之前、搬之后各跑一次，两份直接 diff。
//! macOS 那一侧的同一组调用在 `jp-ffi/examples/api_parity.rs`。

use std::fmt::Write as _;

use serde_json::{Value, json};
use tauri::WebviewWindowBuilder;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

use jp_app_lib::state::AppState;

#[test]
#[ignore = "要一份真实库的副本，见文件头"]
fn dump_moved_commands() {
    let root = std::env::var("JP_PARITY_ROOT").expect("没设 JP_PARITY_ROOT");
    let out = std::env::var("JP_PARITY_OUT").expect("没设 JP_PARITY_OUT");
    let state = AppState::new(std::path::Path::new(&root)).expect("装配状态失败");
    let app = jp_app_lib::register(mock_builder())
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("装配应用失败");
    let w = WebviewWindowBuilder::new(&app, "main", Default::default()).build().expect("建 webview 失败");

    let calls: Vec<(&str, Value)> = vec![
        ("overview", json!({})),
        ("timeline", json!({})),
        ("get_track", json!({ "songId": "001" })),
        ("get_track", json!({ "songId": "no-such-song" })),
        ("quick_search", json!({ "query": "愛" })),
        ("quick_search", json!({ "query": "yui", "perKind": 50 })),
        ("quick_search", json!({ "query": "" })),
        ("home_summary", json!({})),
        ("home_summary", json!({ "utcOffsetMinutes": 480 })),
    ];
    let mut text = String::new();
    for (cmd, args) in calls {
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: InvokeBody::Json(args.clone()),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        let body = match get_ipc_response(&w, request) {
            Ok(InvokeResponseBody::Json(s)) => format!("ok {s}"),
            Ok(InvokeResponseBody::Raw(b)) => format!("raw {b:?}"),
            Err(e) => format!("err {e}"),
        };
        writeln!(text, "{cmd}\t{args}\t{body}").unwrap();
    }
    std::fs::write(&out, text).expect("写不出结果");
    std::mem::forget(app);
}
