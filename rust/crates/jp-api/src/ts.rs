//! 由 Rust 类型生成 `app/src/generated/jp-api.ts`，并检查仓库里那份没过期。
//!
//! 只在测试里编译（ts-rs 是 dev-dependency）。从 [`crate::COMMANDS`] 那张表出发，
//! 把每条命令的 `Args`、`Output` 以及它们引用到的类型全收集起来——不另外维护一份类型清单。
//!
//! 改了类型之后重新生成：`JP_API_BLESS=1 cargo test -p jp-api`。

use std::collections::BTreeMap;
use std::path::PathBuf;

use ts_rs::{Config, TS, TypeVisitor};

use crate::Command;

const HEADER: &str = "\
// 由 rust/crates/jp-api 生成（ts-rs），不要手改。
// 改了 Rust 类型之后跑 `JP_API_BLESS=1 cargo test -p jp-api` 重新生成，连同这个文件一起提交。
// 不重新生成的话 `cargo test --workspace` 会失败。
";

pub(crate) struct Collector {
    cfg: Config,
    /// 类型名 → 声明。按名字排序输出，生成结果和登记顺序无关
    decls: BTreeMap<String, String>,
    /// `Commands` 接口的每一行，按登记顺序
    commands: Vec<String>,
}

impl Collector {
    fn new() -> Self {
        Self {
            // i64 在 JSON 里就是数字，前端一直按 number 用（id、计数都远小于 2^53）
            cfg: Config::default().with_large_int("number"),
            decls: BTreeMap::new(),
            commands: Vec::new(),
        }
    }

    pub(crate) fn command<C: Command>(&mut self)
    where
        C::Args: TS + 'static,
        C::Output: TS + 'static,
    {
        self.visit::<C::Args>();
        self.visit::<C::Output>();
        self.commands.push(format!(
            "  {}: {{ args: {}; output: {} }};",
            C::TAURI,
            <C::Args as TS>::name(&self.cfg),
            <C::Output as TS>::name(&self.cfg),
        ));
    }

    fn render(&self) -> String {
        let mut out = String::from(HEADER);
        for decl in self.decls.values() {
            out.push('\n');
            out.push_str(decl);
            out.push('\n');
        }
        out.push_str("\n/** Tauri 命令名 → 参数和返回值。`api.ts` 的 `command()` 按它做类型检查 */\n");
        out.push_str("export interface Commands {\n");
        for line in &self.commands {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("}\n");
        out
    }
}

impl TypeVisitor for Collector {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        // 没有自己声明的类型（Option、Vec、基本类型）：只看它的类型参数
        if T::output_path().is_none() {
            T::visit_generics(self);
            return;
        }
        let name = T::ident(&self.cfg);
        if self.decls.contains_key(&name) {
            return;
        }
        let mut decl = T::docs().unwrap_or_default();
        decl.push_str("export ");
        decl.push_str(&T::decl(&self.cfg));
        self.decls.insert(name, decl);
        T::visit_dependencies(self);
    }
}

fn generated_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../app/src/generated/jp-api.ts")
}

pub(crate) fn render() -> String {
    let mut collector = Collector::new();
    crate::visit_commands(&mut collector);
    collector.render()
}

#[test]
fn generated_typescript_is_up_to_date() {
    let fresh = render();
    let path = generated_path();
    if std::env::var_os("JP_API_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &fresh).unwrap();
        return;
    }
    // 工作区按 .gitattributes 是 LF；万一被签成 CRLF 也别误报
    let current = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        current == fresh,
        "{} 和 Rust 类型对不上了。跑 `JP_API_BLESS=1 cargo test -p jp-api` 重新生成，连同改动一起提交",
        path.display()
    );
}

#[test]
fn every_declared_type_name_is_unique_and_referenced_types_are_declared() {
    // 两个模块里同名的类型会在 TS 里互相覆盖；BTreeMap 按名字去重会把这种事藏起来，这里查一次
    let text = render();
    let declared: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("export type ").and_then(|r| r.split([' ', '<']).next()))
        .collect();
    let mut sorted = declared.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), declared.len(), "有重名的类型：{declared:?}");
}
