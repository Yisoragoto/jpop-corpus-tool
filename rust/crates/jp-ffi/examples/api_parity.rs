//! 对账：命令搬进 `jp-api` 前后，macOS 客户端拿到的 JSON 要逐字节相同。
//!
//! 只对着库的**副本**跑（打开会话会迁移库）：
//!
//! ```text
//! cargo run -p jp-ffi --example api_parity -- <corpus.db 副本> <输出文件>
//! ```
//!
//! 输出每行一条调用：`方法<TAB>参数<TAB>结果`，结果就是 `jp_call` 包进 `{"ok": …}` 之前的那段 JSON。
//! 搬之前、搬之后各跑一次，两份直接 diff。Tauri 那一侧在 `jp-app/tests/api_parity.rs`。

use std::fmt::Write as _;

use anyhow::{Context, Result};
use jp_ffi::Session;
use serde_json::{Value, json};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (db, out) = (args.first().context("缺库路径")?, args.get(1).context("缺输出路径")?);
    let session = Session::open(db)?;
    let calls: Vec<(&str, Value)> = vec![
        ("overview", json!({})),
        ("timeline", json!({})),
        ("track", json!({ "songId": "001" })),
        ("track", json!({ "songId": "no-such-song" })),
        ("track", json!({})),
        ("quickSearch", json!({ "query": "愛" })),
        ("quickSearch", json!({ "query": "yui", "perKind": 50 })),
        ("quickSearch", json!({ "query": "", "perKind": null })),
        ("homeSummary", json!({})),
        ("homeSummary", json!({ "utcOffsetMinutes": 480 })),
        ("homeSummary", json!({ "utcOffsetMinutes": 480, "addedLimit": 1000 })),
    ];
    let mut text = String::new();
    for (method, args) in calls {
        let body = match session.call(method, args.clone()) {
            Ok(v) => format!("ok {}", serde_json::to_string(&v)?),
            Err(e) => format!("err {e:#}"),
        };
        writeln!(text, "{method}\t{args}\t{body}")?;
    }
    std::fs::write(out, text)?;
    Ok(())
}
