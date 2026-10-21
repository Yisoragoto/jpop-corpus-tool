# 工程改造 3–8 执行记录

执行依据：用户提供的 `agent-task-3-8.md`；D 按用户后续消息改为 stable/debug 双轨，无 beta。
各项按 A→F 独立提交；本报告随对应提交更新，提交哈希可用 `git log --oneline` 查到。

## 与计划描述的差异

- 当前应用是 0.3.2，结构版本为 20261019；`Corpus::tune()` 没有 `BUSY_TIMEOUT` 常量。
- `rust-core.md` 中“Python 拥有 schema、Rust 只读”的旧说明已过时，Rust migrations 实际负责建库和升级。
- SQL 存量还分布在 jp-import、jp-scraper、jp-anki 和 jp-app；本次新增 SQL 放在 jp-corpus，不借机搬动全部存量。
- 更新器目前查 releases 列表筛正式版，并非 `/latest`；现有发布 workflow 仅构建、上传 artifacts，没有发布 Release。
- 英文字典是 en.ts 汇总 en1–en5，backend.ts 是独立后端字典。重复键守卫检查这六份前端源文件（包括对象内重复）。
- tauri.conf.json 没有自定义安装包命名模板，版本测试校验三处一致及合法 semver 文件名片段。

## A：四类守卫

文件：`rust/crates/jp-corpus/tests/architecture.rs`、`app/src/i18n.test.ts`、
`app/src/locales/en{1,3,4,5}.ts`、`app/src/styles.tokens{.test.ts,.baseline.txt}`、
`app/src/version.test.ts`、`app/vitest.config.ts`。

- `cargo test --workspace --no-fail-fast --offline`：通过（详见本地 `output/engineering/A-rust.log`）。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `npm run typecheck`：通过；`npm test`：33 文件，232 测试通过。
- 四次临时变异并恢复：添加可选 tauri 依赖 → architecture **断言失败**（101）；
  en3 英文加入中文、CSS 加 `#ff0000`、单改 package 版本 → 对应 Vitest **失败**（1）。
- CSS 存量基线 243 处，按原行内容和出现次数计数；行号只供定位，前面插行不会制造假新增。
- 新守卫发现并修正“语言”重复键和 10 条英文中的中日文示例；改为英文或罗马字。
- 变异验证发现 Vitest 默认将 CSS mock 为空串，已开启真实 CSS，并断言输入长度，防止守卫假绿。

限制与范围外记录：基线只许缩减，增加存量例外必须在评审时拒绝；不改视觉样式。
存量 `jp-setup::net` 有一项测试会连 example.invalid，导致离线全量测试等待网络超时；本项不改下载逻辑。
