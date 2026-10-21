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

## B：Bug 建档与索引

文件：`docs/BUGS.md`、`docs/bugs/.gitkeep`、`scripts/bug.ps1`、
`scripts/tests/bug.Tests.ps1`、`README.md`。

- `./scripts/tests/bug.Tests.ps1`：通过；独立临时仓库中连续 new 得到 0001/0002，
  手工制造重号后 renumber 修复，settle 再跑索引不变。
- `npm run typecheck` 和 `npm test`：通过，33 文件、232 测试。
- `cargo test --workspace --no-fail-fast --offline`：通过；Rust 源码未改，clippy 沿用 A 的通过结果。
- README 的文档表已链接 BUGS.md；“两个勾没打满不算修完”写入仓库规则。

未做：没有把 Issue #15 标成修复；本计划不包含它的 macOS 根因修复。

## C：CLI 与共享目录解析

文件：`rust/crates/jp-cli/{Cargo.toml,src/main.rs,tests/cli.rs}`、`rust/Cargo.lock`、
`jp-corpus/src/{lib.rs,library_root.rs,enrichment.rs}`、`jp-app/src/{library_root.rs,scrape.rs}`、
`app/src/rustMessages.test.ts`、`docs/cli.md`、`README.md`。

- `cargo test -p jp-cli --offline`：5 个集成测试通过，覆盖空库 info、空目录只读 JSON 扫描、
  单文件导入后重扫去重、真实卡片字段与 HTML 例句导出、0/1/2 退出码。
- `cargo test --workspace --no-fail-fast --offline`、全 workspace/all-targets clippy `-D warnings`：通过。
- `npm run typecheck`、`npm test`：通过；CLI 文案不会送到前端，因此后端翻译守卫明确排除该壳。
- `JPOP_CORPUS_HOME=<真实库隔离副本> jp info`：结构 20261019，210 首 / 8443 行 / 58628 词次。
  副本由 SQLite backup 从 993214464 字节、结构 20261015 的用户库取得；仅副本执行升级。
- 目录解析实现在 `jp_corpus::library_root::locate()`；jp-app 原模块只 re-export。
- CLI 直接依赖均为已有 workspace crate 和标准库，未引入外部依赖。

实现选择：`export-anki` 输出 Anki 可导入的 TSV（使用 jp-anki 的笔记字段和 HTML 卡片构造），
不连接 AnkiConnect、不导出 apkg 或音频。刮削沿用现有 provider 限流，结果为 JSON Lines，
已导入歌曲的空元数据和来源使用新抽到核心的共用函数；未导入文件只保存刮削状态，不凭空建歌。

## D：stable/debug 双轨

文件：`.github/workflows/release.yml`、`scripts/check-release-policy.ps1`、`scripts/tests/release-policy.Tests.ps1`、
`jp-app/{build.rs,src/update.rs,src/update/channels.rs,src/commands.rs,src/lib.rs,tests/fixture_commands.rs}`、
`jp-corpus/src/{lib.rs,library_root.rs,update_settings.rs}`、`app/src/{api.ts,App.tsx,components/UpdateCard.tsx,components/UpdateDialog.tsx,pages/SettingsPage.tsx,locales/en1.ts,locales/backend.ts}`、`docs/updates.md`。

- 策略自检 10 用例通过：五条规则各有通过/拒绝或阻断用例，main stable、PR 发包、v tag、debug Latest 均拒绝。
- 更新器 22 个离线单元测试通过，覆盖 stable 版本、debug 构建身份、缺清单/哈希等必要字段、404 回落、
  两种轨道的降级阻断、缓存 debug 大小不符即删除、后端资产身份及安装前再校验。
- 核心偏好测试证明跳过记录隔离、切目录保留轨道及未知字段；buildId 已由本地 build.rs 编入二进制，关于页读取同一个常量。
- `cargo test --workspace --no-fail-fast --offline`、clippy `--workspace --all-targets -- -D warnings`、
  `npm run typecheck`、`npm test`（33 文件 / 232 测试）全部通过。
- 既有 main_thread 守卫发现新增设置命令会在 UI 主线程读写文件；已改为线程池命令，未放宽白名单。

未执行的远端验收：没有推 main、创建/删除 GitHub Release，因此“线上最终恰好一个 debug 且 sha 对应”的运行结果
尚未实测。工作流实现了固定 tag 删除重建和两资产约束，待代码进入 main 后由 Actions 实际运行；不把静态检查冒充线上发布成功。
真实 WebView 的关于页构建编号展示会在 F 的隔离冒烟中一并检查。
