# 工程改造 3–8 执行记录

执行依据：用户提供的 `agent-task-3-8.md`；D 按用户后续消息改为 stable/debug 双轨，无 beta。
各项按 A→F 独立提交；本报告随对应提交更新，提交哈希可用 `git log --oneline` 查到。

| 项目 | 提交 | 说明 |
|---|---|---|
| A | 30871fb | 四类守卫及真实失败路径 |
| B | f39a77e | 缺陷档案与编号工具 |
| C | ae8ff8b | CLI 和共享库目录解析 |
| D | 32acf8f | stable/debug 发布与安全更新 |
| E | ae1fed1 | 内容身份、指纹及字段来源 |
| F | 本报告最后一次更新所属提交 | 交互回归、显式 skip 与重活租约 |

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
真实 WebView 的关于页构建编号展示已在 F 的隔离冒烟中检查，见下文证据。

## E：内容身份、指纹及字段来源

文件：`jp-corpus/{Cargo.toml,schema.sql,src/{lib.rs,migrations.rs,enrichment.rs,identity.rs,fingerprint.rs,lineage.rs},tests/architecture.rs}`、
`jp-import/src/{import.rs,maintain.rs}`、`jp-app/src/scrape.rs`、`jp-cli/{src/main.rs,tests/cli.rs}`、
`jp-scraper/{Cargo.toml,src/{net.rs,resolver.rs,providers/{mod.rs,musicbrainz.rs,deezer.rs}},tests/{content_identity.rs,data/*-identity.json}}`、
`rust/Cargo.lock`、`app/src/locales/backend.ts`、`docs/{content-identity.md,cli.md}`。

- `cargo test -p jp-corpus schema_parity --offline`：通过；原歌曲表/专辑表升级后逐表列的类型、非空、默认值、主键与空库一致，新增索引存在，原专辑保留。
- 新增指纹回归：改名不变、不同音频不同、只取前 30 秒、固定重采样结果；导入/重扫的 CLI 回归也断言实际写入 64 位散列。
- `cargo test -p jp-scraper --test content_identity --offline`：2 项通过，夹具经 provider→详情→核心落库→查询，
  断言 ISRC、厂牌、曲数和对应来源/置信度；MB 总计一次搜索+一次详情，且不会误取同专辑另一首的 ISRC。
- 947 MB 真实库经 SQLite backup 建隔离副本，从 20261015 升至 20261020；所有原表行数保持，quick_check=ok。
  用户原库保持只读，新的外部身份表为空，没有为验收联网刮用户曲库。
- `cargo test --workspace --no-fail-fast --offline`、全量 clippy `-D warnings`、前端 typecheck、npm test（232 项）：通过。

与计划差异：原仓库没有 schema_parity 测试，已新增；MusicBrainz Search 没有 inc，改用已确认候选的一次发行详情请求。
Deezer 实际轨道/专辑详情分属两个实体，因此各请求一次拿齐相关字段；测试用夹具，不把离线通过称为在线 API 验收。
新增直接依赖仅复用已有 rodio（只解码、不启用设备）和 sha2；多格式解码和标准密码散列不应手写。
重扫通过显式 `jp fingerprint` 和音频重新链接执行，保持 `jp scan` 的只读契约。

## F：交互回归、显式 skip 与重活租约

文件：`.github/workflows/check.yml`、`scripts/{smoke.mjs,check-skips.ps1,heavy.ps1,tests/heavy.Tests.ps1}`、
`app/src/{App.tsx,e2e.ts,e2e.test.ts,useHotkeys.ts,pages/ImportPage.tsx}`、
`jp-app/src/{lib.rs,e2e.rs,fonts.rs}`、`jp-app/tests/{commands.rs,fixture_commands.rs,fixture_flows.rs,support/mod.rs}`、
`jp-audio/{src/engine.rs,tests/playback.rs,tests/duration.rs}`、`jp-corpus/tests/{real_corpus.rs,invariants/mod.rs}`、
`jp-tokenizer/tests/{baseline.rs,lemma_forms.rs}`、`docs/{testing.md,BUGS.md,bugs/0001-space-button.md}`、`README.md`。
Rust 路径均相对 `rust/crates/`；音频文件仅改测试属性及缺前置时的行为，没有改音频算法。

- `./scripts/heavy.ps1 -Command './scripts/check-skips.ps1'`：通过，日志逐项列出 88 个 `[skip]`，包含明确设备、作者曲库、词典、系统字体、联网等前置条件。
  计划提到的六套与当前代码不完全对应，实际还涵盖 engine 内设备测试、字体、tokenizer 基准等；合成库 51 项继续执行。
  缺前置的 ignored 测试被手动运行时会失败，不能再 `return` 假绿。随源码发布的字体资源改为必须存在。
- `cargo test --workspace --no-fail-fast --offline`：48 个结果汇总，962 passed、89 ignored、0 failed（其中一个 ignored 属其他既有套件，故比专门日志多 1）。
  首轮两个示例编译出现依赖 rlib 不可用；原命令重新执行后通过，没有修改断言或裁掉示例。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
  `npm run typecheck`、`npm test`：通过，34 文件、233 项。
- 环境门控：Rust 仅接受精确 `JPOP_E2E=1`；普通注册表测试断言两个命令不可调用，前端单测断言普通环境不挂入口。
  真实普通窗口同样确认全局入口不存在、两条命令均拒绝。测试命令以 async 注册，不占 UI 主线程。
- 真实 WebView2 正向：`node scripts/smoke.mjs --exe D:/jp_corpus/rust/target/debug/jp-app.exe --out output/engineering/F-smoke-green`：三个场景全部通过。
  每个场景全新临时库，scanning=true→计划出现→真实按钮导入 0→1→全文「テスト」命中 2 行；无真实用户库、无词典下载。
  同时检查关于页非空 buildId，并保存各阶段截图及 JSON；实际查看了检索和关于页截图。
- 键盘先红后绿：`F-keyboard-red-clean/report.json` 无语言弹窗且 audioReady=true，真实 Tab→Space 后仍为 0 首、退出 1。
  修正全局热键：尊重 defaultPrevented，可交互元素保留按键；之后相同键盘用例成功导入，播放按钮 paused→playing。
- 导入事件负例：临时删除导入按钮 onClick，TypeScript 已先报缺属性/unused；仅对变异夹具用 Vite 直接打包后再测。
  `F-mouse-no-onclick/report.json` 确认已出计划，鼠标按钮后 0→1 断言超时、退出 1；finally 按字节还原源码并正常重建。
- 租约：同时启动两个 `heavy.ps1 -Command 'cargo test -p jp-corpus --offline'`，观察排队且两者均通过。
  `scripts/tests/heavy.Tests.ps1` 再验证时间戳不重叠及杀死持有者后接手；两个断言均通过。

默认单命令 `node scripts/smoke.mjs` 最终通过：`output/smoke-1792595836713/report.json` 的 success=true、built=true，mouse/keyboard/normal 均通过，留下 13 张阶段截图。
默认命令会自动正常构建前端和 custom-protocol 二进制，因为 cargo test 会覆盖同名 debug 可执行文件为 devUrl 模式。
独立 `--exe` 路径保留给已经打包好的应用；原 `<port> [output.json]` 附着用法继续兼容 release-check.ps1。
脚本使用条件轮询、DOM 事件与真实 Tab/Space，不用坐标和固定等待；阻断与本例无关的启动更新查询，避免未来线上版本弹窗影响键盘焦点。
本地受限沙箱首次无法连接 WebView2 调试端口，后续通过允许的本地执行权限完成窗口验收；最初失败日志保留，未当成通过。

新发现但未顺手改：设置页使用启动时的 health 快照，导入后侧栏总览显示 1 首，但设置「语料库」仍可能显示 0 首，直到重新读取 health。
这是已有刷新策略问题，与本次钩子的计数分开；钩子读取会随导入刷新的 overview，并用后端 health 二次核实。
Issue #15 的 macOS 原生导入问题仍没有本机 macOS 验收，不能根据 Windows 回归就关闭该 issue。
