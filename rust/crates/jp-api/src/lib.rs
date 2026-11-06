//! # jp-api：两个客户端共用的一张命令表
//!
//! Windows 端（Tauri + React）和 macOS 端（SwiftUI，经 `jp-ffi`）说的是同一套命令。
//! 以前一条命令要写三遍——`jp-app/src/commands.rs` 的 `#[tauri::command]`、
//! `jp-ffi` 里按方法名分支的 `dispatch`、`app/src/api.ts` 的手写类型——三边经常对不齐，
//! 而 TS 类型写错了编译器也查不出来。
//!
//! 这里每条命令只定义一次（见 [`Command`]）：
//!
//! - **名字**：Tauri 命令名（snake_case）和 FFI 方法名（camelCase）。两边历史上叫法不全一样
//!   （`get_track` ↔ `track`），这里如实记下，客户端一个字都不用改。
//! - **参数**：一个 `Args` 结构体，`#[serde(rename_all = "camelCase")]`，字段名就是前端、
//!   Swift 传的那个键。可选参数用 `Option`，默认值在 [`Command::run`] 里补。
//! - **返回**：`Output`，serde 序列化成什么，两个客户端就拿到什么。
//! - **实现**：[`Command::run`]，只依赖 `jp-corpus` 这类业务 crate，拿一条 [`Corpus`] 连接。
//!   不碰 Tauri、不碰 C ABI，所以能离线测。
//!
//! 两个客户端怎么接：
//!
//! - **Tauri**（`jp-app`）：`#[tauri::command]` 只剩一行，把形参装进 `Args` 调 `run`。
//!   形参名不变，前端 `invoke` 的参数不用改。
//! - **FFI**（`jp-ffi`）：`dispatch` 先问 [`ffi::dispatch`]，按 FFI 方法名在 [`COMMANDS`] 里找，
//!   找到就把 JSON 参数解析成 `Args`、调 `run`、把结果转回 JSON。Swift 一行不改。
//! - **TS 类型**：`app/src/generated/jp-api.ts` 由这里的 Rust 类型生成（ts-rs），
//!   里面的 `Commands` 把 Tauri 命令名映射到参数和返回类型，`api.ts` 的 `command()` 按它做类型检查。
//!   生成结果提交在仓库里；`cargo test -p jp-api` 发现它和 Rust 类型对不上就失败。
//!
//! ## 加一条新命令
//!
//! 1. **实现**：在合适的模块里（或者新开一个）写一个单元结构体，实现 [`Command`]：
//!    两个名字、`Args`、`Output`、`run`。`run` 里只调业务 crate 的函数，不写 SQL，也不碰 Tauri / C ABI。
//! 2. **类型可导出**：`Args`、`Output` 以及它们用到的每个结构体、枚举都加
//!    `#[cfg_attr(test, derive(ts_rs::TS))]`（本 crate 的类型）或
//!    `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]`（`jp-corpus` 的类型；那个特性只有本 crate
//!    的测试打开，发布构建不带 ts-rs）。serde 的 `rename_all`、`tag`、`Option` ts-rs 都认；
//!    `Args` 写成 `#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]`，可选参数在 TS 里才是 `x?: T`。
//! 3. **登记**：把它加进本文件里 `commands![…]` 那张表。FFI 路由和 TS 的 `Commands`
//!    都从这张表来，漏登记两边都没有。
//! 4. **Tauri**：在 `jp-app/src/commands.rs` 写一行转发，并在 `jp-app/src/lib.rs` 的
//!    `generate_handler!` 里注册。`jp-app/tests/fixture_commands.rs` 会逐条检查表里的
//!    Tauri 名字都注册了。
//! 5. **重新生成 TS**：`JP_API_BLESS=1 cargo test -p jp-api`，把 `app/src/generated/jp-api.ts`
//!    的改动一起提交。前端用 `command("名字", { … })` 调，参数、返回值都有类型。
//! 6. **从旧地方搬过来的命令**：删掉 `jp-ffi` 里对应的分支、`api.ts` 里对应的手写类型，
//!    并用 `jp-ffi/examples/api_parity.rs` 和 `jp-app/tests/api_parity.rs` 在真实库的副本上
//!    对一遍搬之前、搬之后的输出（先把新命令加进两边的调用清单）——要逐字节相同。

use anyhow::{Context, Result};
use jp_corpus::Corpus;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub mod corpus;
pub mod home;
pub mod library;
pub mod search;

#[cfg(test)]
mod ts;

/// 一条命令。实现者是一个单元结构体，只当类型级别的名字用。
pub trait Command {
    /// Tauri 命令名（`#[tauri::command]` 的函数名，snake_case）
    const TAURI: &'static str;
    /// macOS FFI 的方法名（`jp_call` 的 `method`，camelCase）
    const FFI: &'static str;
    /// 参数。字段名 camelCase，和前端、Swift 传的键一致
    type Args: DeserializeOwned;
    /// 返回值。序列化出来的 JSON 就是两个客户端看到的形状
    type Output: Serialize;
    /// 实现。只依赖业务 crate
    fn run(corpus: &Corpus, args: Self::Args) -> Result<Self::Output>;
}

/// 没有参数的命令用它。TS 里是 `Record<symbol, never>`，传 `{}` 或不传都行
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NoArgs {}

/// 表里的一行：名字 + 按 JSON 调用的入口
pub struct Entry {
    pub tauri: &'static str,
    pub ffi: &'static str,
    call: fn(&Corpus, &Value) -> Result<Value>,
}

impl Entry {
    /// 按 JSON 调一次：解析参数 → [`Command::run`] → 结果转 JSON
    pub fn call(&self, corpus: &Corpus, args: &Value) -> Result<Value> {
        (self.call)(corpus, args)
    }
}

fn call_json<C: Command>(corpus: &Corpus, args: &Value) -> Result<Value> {
    let args = parse_args::<C::Args>(args)?;
    Ok(serde_json::to_value(C::run(corpus, args)?)?)
}

/// JSON 参数 → `Args`。没传参数（`null`）当作 `{}`
pub fn parse_args<T: DeserializeOwned>(args: &Value) -> Result<T> {
    let args = match args {
        Value::Null => Value::Object(Default::default()),
        other => other.clone(),
    };
    serde_json::from_value(args).context("参数不对")
}

/// FFI 那一侧的入口
pub mod ffi {
    use super::*;

    /// 按 FFI 方法名找命令并调用。不是表里的方法返回 `None`，由 `jp-ffi` 自己的分支接着处理
    pub fn dispatch(corpus: &Corpus, method: &str, args: &Value) -> Option<Result<Value>> {
        crate::COMMANDS.iter().find(|e| e.ffi == method).map(|e| e.call(corpus, args))
    }
}

/// 登记命令。一处登记，生成 [`COMMANDS`]（FFI 路由、Tauri 注册检查用）和 TS 类型的遍历。
macro_rules! commands {
    ($($cmd:ty),* $(,)?) => {
        /// 所有已经搬进来的命令
        pub const COMMANDS: &[Entry] = &[
            $(Entry { tauri: <$cmd as Command>::TAURI, ffi: <$cmd as Command>::FFI, call: call_json::<$cmd> },)*
        ];

        #[cfg(test)]
        fn visit_commands(v: &mut ts::Collector) {
            $(v.command::<$cmd>();)*
        }
    };
}

commands![
    corpus::GetOverview,
    corpus::GetTimeline,
    library::GetTrack,
    search::QuickSearch,
    home::GetHomeSummary,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique_on_both_sides() {
        let tauri: HashSet<_> = COMMANDS.iter().map(|e| e.tauri).collect();
        let ffi: HashSet<_> = COMMANDS.iter().map(|e| e.ffi).collect();
        assert_eq!(tauri.len(), COMMANDS.len(), "Tauri 名字重了");
        assert_eq!(ffi.len(), COMMANDS.len(), "FFI 名字重了");
    }

    #[test]
    fn ffi_dispatch_routes_by_method_name_and_passes_on_the_rest() {
        let dir = std::env::temp_dir().join(format!("jp-api-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = jp_corpus::fixture::write_to(&dir).unwrap();
        let corpus = Corpus::open(&db).unwrap();

        let overview = ffi::dispatch(&corpus, "overview", &Value::Null).unwrap().unwrap();
        assert_eq!(overview, serde_json::to_value(corpus.overview().unwrap()).unwrap());
        let track = ffi::dispatch(&corpus, "track", &serde_json::json!({ "songId": "001" })).unwrap().unwrap();
        assert_eq!(track["id"], "001");
        assert!(ffi::dispatch(&corpus, "track", &serde_json::json!({})).unwrap().is_err());
        assert!(ffi::dispatch(&corpus, "noSuchMethod", &Value::Null).is_none());
        // Tauri 名字不是 FFI 名字
        assert!(ffi::dispatch(&corpus, "get_track", &Value::Null).is_none());

        drop(corpus);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
