# Tauri 壳

迁移的第三步：Rust core 骨架 + Tauri commands + React 竖切。
分词器基准（`docs/tokenizer.md`）通过之后才做这一步。

## 目录

```
app/                              前端（React 19 + TS strict + Vite 7）
  src/App.tsx                     外壳：导航 + 页面切换 + 常驻播放条
  src/api.ts                      command 的类型化封装
  src/useLibrary.ts               曲目/歌词/语料的共享状态（跨页跳转靠它）
  src/usePlayback.ts              状态订阅 + 当前行推导 + 单句循环
  src/components/Player.tsx       播放条 + 频谱（Spotify 三栏 + 进度条整宽一行）
  src/components/NavIcons.tsx     左侧导航的图标
  src/components/CommandButton.tsx  命令栏按钮（图标 + 文字）
  src/components/Flyout.tsx       弹出面板（速度、调）
  src/coverColor.ts               从封面取主色，给全屏歌词做底色
  src/useHotkeys.ts               全局键盘交互
  src/virtual.ts                  定高虚拟化的窗口计算（18 个单测）
  src/components/VirtualList.tsx  定高虚拟列表
  src/components/CommandPalette.tsx  Cmd+K 搜索 / Cmd+P 命令
  src/pages/HomePage.tsx          语料概览 · 收听记录 · 收藏 · 分面
  src/pages/KwicPage.tsx          KWIC 语境检索 + 歌词全文
  src/pages/LibraryPage.tsx       曲库 → 歌词 → 语料 三栏
  src/pages/ExplorerPage.tsx      人物维度（演唱/作曲/作词/编曲 + 合作图谱）
  src/pages/AnalyticsPage.tsx     总览 + 时间线 + 词频统计 + 语料报告
  src/pages/SettingsPage.tsx      设置（外观 / 歌词 / 词典 / 制卡 / 曲库维护 / 关于）
  src/components/SettingCard.tsx  设置行：图标 + 标题说明 + 右侧控件、On/Off 开关
  src/components/Section.tsx      区块与封面架（Section / Shelf）——页面的基本版面单位
  src/components/Avatar.tsx       人的头像；没照片就画名字的第一个字
  src/dict/DictionaryManager.tsx  词典管理（导入 / 启用 / 拖动排序 / 删除），住在设置页
  src/settings.ts                 应用级偏好（全屏背景、歌手墙），存 localStorage
  src/components/PerformerPicker.tsx  演唱者多选（检索页、分析页共用）
  src/pyFormat.ts                 按 Python 的规则写数字和 CSV（导出要和 PyQt 版逐字节一样）
  src/styles.css
rust/
  Cargo.toml                      workspace
  crates/
    jp-tokenizer/                 分词 + UPOS 映射（已验收 1.0000）
    jp-corpus/                    数据层：SQL 全在这里
    jp-audio/                     播放引擎：WSOLA 变速 + 采样 tap + 频谱
    jp-app/                       Tauri 壳
      src/lib.rs                  register() + run()
      src/commands.rs             38 个 command，每个都是薄封装
      src/tracker.rs              收听会话统计（纯逻辑，14 个单测）
      src/maintenance.rs          时长回填等一次性数据修补
      src/lyrics.rs               补齐歌词：音频旁边的 .lrc / 在线搜 / 用户自己的文件
      src/state.rs                Mutex<Corpus> + 可选 Analyzer / AudioEngine
      tauri.conf.json
      tests/commands.rs           走真实 IPC 通道的集成测试
```

前端和后端分处两个目录，是因为 Rust 侧是一个 workspace（`jp-app` 要按路径
依赖 `jp-corpus` / `jp-tokenizer`），塞进 `src-tauri/` 会让 workspace 结构变别扭。
`tauri.conf.json` 里用相对路径把两边接起来。

## 跑起来

```bash
# 一次性：装前端依赖
cd app && npm install

# 开发（前端热更新 + Rust 自动重编）
npm run app:dev          # 或双击 开发.bat

# 打包（产出可独立运行的 exe）
npm run app:build        # 之后双击 启动-新版.bat

# 前端类型检查与测试
npm run typecheck
npm test
```

后端单独跑：

```bash
cd rust
cargo test          # 167 个测试
cargo clippy --all-targets
```

## 分层

```
React (api.ts)
   │  invoke("get_track", { songId })
   ▼
commands.rs        ← 薄封装，取状态 → 调一次 → 返回
   │
   ▼
jp-corpus          ← 所有 SQL 和业务逻辑在这里，能离线测试
jp-tokenizer       ← 分词，能离线测试
   │
   ▼
SQLite (corpus.db) + Sudachi 词典
```

**能测的东西不放进测不了的层。** command 层需要 Tauri 运行时才能测，
所以它只做转发；真正的逻辑在下面三层（corpus / tokenizer / audio），
用真库和真实音频文件跑 84 个测试。

## `cargo build` 出来的 exe 不能独立运行

直接跑 `rust/target/debug/jp-app.exe` 会看到
**「localhost 拒绝连接 ERR_CONNECTION_REFUSED」**。

原因是 Tauri 用 cargo 特性（不是 `debug_assertions`）区分开发和打包：

```rust
pub const fn is_dev() -> bool {
  !cfg!(feature = "custom-protocol")
}
```

没有 `custom-protocol` 时，二进制会去加载 `tauri.conf.json` 里的 `devUrl`
（vite dev server），而不是编译进来的前端资源。`tauri build` 会自动带上
这个特性，`cargo build` 不会。

所以：

| 想干什么 | 用什么 |
|---|---|
| 开发（热更新） | `npm run app:dev` / `开发.bat` |
| 独立运行的 exe | `npm run app:build`，或 `cargo build -p jp-app --release --features custom-protocol` |
| 跑测试 | `cargo test`（不受影响） |

`启动-新版.bat` 只认 release 产物，找不到时会打印上面这条构建命令，
而不是启动一个连不上 dev server 的窗口。

## .bat 文件必须是纯 ASCII

cmd.exe 用 OEM 代码页（简中系统是 GBK）读 .bat，写 UTF-8 中文进去会变成乱码。
两个启动脚本里都留了注释说明这一点。

（另外：**从 Git Bash 里调 `cmd.exe` 跑中文名的 .bat 会失败**，
因为文件名的 UTF-8 字节被按 GBK 解释了。资源管理器双击没这个问题。）

## 几个踩过的坑

**`crate-type` 不要写 `["staticlib", "cdylib", "rlib"]`。** 那是移动端才需要的。
桌面上加了它们，集成测试会去链接生成的 DLL，加载时
`STATUS_ENTRYPOINT_NOT_FOUND`。只留 `rlib`。

**`common-controls-v6` 要关掉。** 它要求可执行文件的清单里声明 ComCtl32 v6
程序集。主 exe 由 `tauri-build` 嵌入了清单，集成测试的 exe 没有，于是同样
加载失败。我们没有原生菜单/托盘，关掉零损失。

注意**依赖和 dev-dependency 两处都要关** —— Cargo 的特性在整个依赖图里合并，
只关一处等于没关。

**参数名是 camelCase。** Tauri 的默认是 `ArgumentCase::Camel`：Rust 形参
`song_id`，前端要传 `songId`。这条有测试钉着
（`camel_case_arguments_reach_snake_case_parameters`），
而且反向验证了 `song_id` 传不进去。

**状态是 `Mutex<Corpus>`。** `rusqlite::Connection` 是 `Send` 不是 `Sync`
（内部有语句缓存的 `RefCell`），而 Tauri 的 managed state 要求 `Send + Sync`。
代价是查询串行化；实测单条 0.3~3.5ms，桌面端够用。真要并发时换连接池，
command 层签名不用动。

**复制数据库要连 `-wal` / `-shm` 一起复制。** 库是 WAL 模式，最近的提交可能还只在 wal 文件里。
只复制 `corpus.db` 会拿到一个「旧了一截」的副本，不报错、只是数据对不上——
测试用的副本（`scratch_app`）和手工建的隔离实例都踩过：界面上 209 首都有封面，副本里只有 200 首。

**测试不要把「用户库现在长什么样」当前提。** `filling_album_artwork_is_idempotent`
原来依赖「真库里 `albums.artwork_path` 全是空的」，用户在界面上点一次「填专辑封面」它就红了，
而代码没有任何问题。现在测试自己先把副本里的清空，再验填充和幂等。

## 集成测试测什么

`tests/commands.rs` 用 `tauri::test::mock_builder()` 装配**同一份
`register()`**（不是另抄一份 command 清单），走真实 IPC 通道：

| 测的东西 | 为什么只能在这一层测 |
|---|---|
| command 有没有注册上 | 名字写错编译期不报错，运行时才 404 |
| camelCase → snake_case | 序列化层的行为，静态类型看不出来 |
| 返回值形状 | `rename_all` 漏了会让 TS 接口变成假的 |
| 竖切回路 | 曲库→歌词→点词→语料→跳转，任一环断了就不闭合 |

库不存在时整组跳过并提示先跑迁移脚本——克隆仓库的人不该因为缺一个
991MB 的数据库看到一片红。

## 五个页面

| 页面 | 内容 |
|---|---|
| **首页**（默认） | 语料概览 · 高频词入口 · 收听记录 · 收藏 · 年代/流派 · 专辑 |
| **检索** | KWIC 关键词居中对齐 + 歌词全文（FTS5）。表层/词元、词性、歌手、跨行、去重、仅日文，导出 CSV |
| **曲库** | 曲目 → 歌词（跟随播放、实词可点）→ 该词在全语料的样子 |
| **人物** | 按演唱/作曲/作词/编曲看作品与合作图谱 |
| **分析** | Corpus Overview + 年份时间线 + 词频统计（JLPT 列）+ 语料报告（导出 TXT） |

**Corpus First 体现在首页的版面顺序上**：先语料概览、再高频词入口，
收听记录排在后面。打开软件第一眼看到的是「这个库里有什么」，
不是「你最近在听什么」。首页每个板块都是入口，不是仪表盘。

### 跨页连续性

要求书第十八条那句「不应该感觉自己在几个独立的软件之间切换」，
靠的是架构而不是 UI 技巧：

* **播放状态在 Rust 引擎里**，切页不会中断播放，播放条常驻
* **曲目/歌词/语料状态提到 `useLibrary`**，检索页点一条命中能跳到曲库页
  的那一行并定位到时间点；分析页点一个词能跳到曲库页看它的例句
* 有测试钉着这条链路（`cross_page_navigation_targets_resolve`）：
  KWIC 命中的 `songId` 能被 `get_track` 解析，`utteranceId` 真的在那首歌的歌词里

### 图表不引库

三种图（年份柱、词频条、频谱）都用内联 CSS 画。为它们引一个图表库
不划算，也违反要求书第十五条第 3 点。

分析页也**不是孤立的 dashboard**（第十五条第 8 点）：词频表每个词都能点开
跳到曲库页看例句。数据是入口，不是终点。

## 分析页：词频统计与语料报告

对应 PyQt 版的「统计」「报告」两页，合到分析页里，**共用一组筛选**（歌手多选、仅日文），
筛选一改两边都重算。计算在 `jp-corpus/src/stats.rs`，command 是 `stats_frequency`、`stats_report`，
都放进阻塞线程池（报告要扫整张 tokens 表，一两百毫秒，同步 command 会卡住主线程）。

**词频统计**：词元 × 词性按出现次数排前 300，不含标点和符号，列出出现曲数、前 5 个表层形和 JLPT。
JLPT 只查本地的 `jlpt_cache`（10,090 个词元）；Python 查不到的会去网上抓，这里不抓。
点词元看例句（曲库页），点「检索」带着词元跳到检索页。

**语料报告**：TTR、STTR、Hapax、平均每曲词汇量、平均每行 token、词性分布、Top-N 覆盖率、高频词元 Top 20。
「导出 TXT」写的内容由后端和报告一起生成，所以导出的就是界面上看到的那份。

和 Python 对账（`jp-corpus/examples/stats_report.rs` 和 gui.py 里原样抽出来的 worker，真实 corpus.db）：

| 项 | 结果 |
|---|---|
| 报告（不筛选） | 所有数字相同；导出的 TXT 和 `_export_report_txt` 写出的**逐字节相同**（2,542 字节） |
| 报告（按ヨルシカ） | 数字相同；TXT 只有「フィルター」一行不同，见下 |
| 词频表 | 完整的频次档内逐条相同（最后一档被 300 截断，先后不定） |
| 仅日文 | **有意不同**，见下 |

有意不同的两处：

- **Python 导出的 TXT 永远写「全歌手 / 日本語のみ：なし」**：报告字典里根本没有 `artist_filter` / `jp_only`。这里写实际的筛选。
- **仅日文**：Python 报告只过滤了词种，词次、词性分布、覆盖率的分母还是全部词次；词频表是先取前 300 再过滤，常常不到 300 条。
  这里在词次这一层就过滤，词频表先过滤再取前 300。

数字的写法照 Python：千分位，小数**正好一半时取偶**（`f"{0.125:.2f}"` 是 0.12，JS 的 `toFixed` 是 0.13）。
后端用 Rust 的 `{:.N}`——拿 42.2 万个值（含整半的）和 Python 的 `:.Nf` 比过，0 处不同；
前端显示用 `pyFormat.ts` 的 `pyFixed`，`testdata/python_formats.json` 是 Python 真实输出的 2,175 条。

### 检索页的补充

- **仅日文**：只留关键词含假名或汉字的命中（Python `_JP_RE.search(match)`）。要看切出来的关键词才知道，
  所以 SQL 里不截断，建好命中再筛、再截到 5000。四组检索和 Python `SearchWorker` 条数、集合、顺序全部相同。
- **按歌手**：`personIds`，和分析页同一个多选组件。
- **多个关键词**：空格分隔，也认 PyQt 版的 `A|B`、`(A|B)` 写法。
- **导出 CSV**：列和编码照 `export_csv`——utf-8-sig（开头一个 BOM）、CRLF、`csv.writer` 的最少引号、
  时刻按 Python `str(float)` 写（`60.0` 不是 `60`）。`pyFormat.test.ts` 拿 `csv.writer` 真实写出的内容逐字符比。
  左右语境和界面一致，**保留原句里的空格**（Python 是把 token 拼起来，空格丢了；见 `jp-corpus/src/search.rs`）。

## 外观：Windows 11 的质感

2026-09-20 按用户要求整体向 Spotify + Apple Music 的版面、Windows 11 的质感靠拢。

**窗口是真的云母（Mica），不是 CSS 模拟。** `tauri.conf.json` 的窗口加了
`"transparent": true` 和 `"windowEffects": { "effects": ["mica"], "state": "active" }`。
验证方式是查窗口属性而不是看截图——CDP 截图只拍网页内容，拍不到系统的背景：

```powershell
# class 为 Tauri Window 的那个窗口，DWMWA_SYSTEMBACKDROP_TYPE(38) == 2 就是 Mica
[Dwm]::DwmGetWindowAttribute($hwnd, 38, [ref]$backdrop, 4)
```

**外壳半透明、内容不透明。** 导航栏、页头、播放条用 `--shell`（55% 暗色 + `backdrop-filter`），
桌面的颜色透得上来；内容区 `--content` 是 72%，保证文字可读。强调色是 Windows 深色主题的浅蓝 `#4cc2ff`。

**两个踩过的坑**（都是把 `--bg` 改成透明之后才暴露的）：

- **浮层必须自己有不透明背景**。`--panel` 这些是半透明的层填充，给浮层用就会透。
  全屏歌词原来写的是 `background: var(--bg)`，`--bg` 一透明整层跟着透，底下的页面全透上来；
  歌词的「显示」面板、命令面板、歌手多选也一样透了。现在浮层统一用 `--surface-pop`（97% 不透明 + 模糊）。
- **有最大高度的竖向 flex 面板，子项要写 `flex: 0 0 auto`**。默认会收缩：「显示」面板里的字体预览框被压扁，
  里面的字直接画到下面的滑块上。
- **别用全局已有的类名当表格列样式**。`.center` 是启动页用的（`height: 100vh`），
  词频表的 JLPT 列用了同一个名字，每行被撑成一屏高；DOM 里看不出来，只有截图能看出。

### 版面

| 位置 | 做法 |
|---|---|
| 外壳 | 左侧导航（WinUI 的 NavigationView：图标 + 文字，可收起成图标条，选中项左侧一条强调色）+ 页头（标题、一句说明、语料数字） |
| 播放条 | 进度条单独一行贯穿整宽（拖动面积大），下面一行左是这首歌、中间走带（±5 秒、播放、单句循环）、右边速度 / 调 / 音量；**整条是单色玻璃**，盖在全屏歌词上面，底下那张虚化封面透上来 |
| 速度、调 | `Flyout` 弹出面板，不用原生 `<select>`：系统下拉在窗口最底下会往屏幕外弹，样式也和这套控件对不上 |
| 曲库 | 列表视图按歌手分组、标题可收起（记在 localStorage）；封面网格**先是一墙歌手头像**，点进去才是他的歌（设置里可关），网格时左栏加宽到 420px |
| 各页操作 | `CommandButton`：图标 + 文字，主操作填强调色 |
| 全屏歌词 | 照 Spotify：**背景是整张封面虚化铺满**（设置里可换回取色纯色，强度可调）、歌词左对齐大字、当前行纯白，右边常驻一栏放封面和这首歌的信息（查词时换成查词结果） |

**封面取色**在 `coverColor.ts`：封面缩到 24×24，挑饱和度最高的像素做主色，再压到能当背景的亮度；
纯黑边、白边不参与，整张灰的退回平均色，取不到就返回 null 用中性深色（4 个单测）。不引取色库。

> **取色曾经在打包后的程序里一直是失灵的。** 它用 `fetch` 取图（要 CORS 头，
> 否则 canvas 被污染读不出像素），而 CSP 的 `connect-src` 里没有 asset 协议——
> **图照常显示**（那走的是 `img-src`），只有取色静默失败，底色一直是那个中性深灰。
> 开发时 vite 不加 CSP，所以只在打包后出现。现在 `connect-src` 也放行 asset，
> 并有测试钉着三个 directive（`the_csp_lets_the_front_end_fetch_assets_not_just_display_them`）。

### 右栏：词典 / 例句两页，记住你看的是哪一页

点歌词里的词，右栏原来是「统计 → 词典 → 例句」一路堆下来。词典的释义可以很长，
想看例句每次都得滚到底。现在词头（词、出现次数、词性）下面是一个两页的切换栏：

- **词典**：这次查到的释义（长按住的还是 `LookupResults` 那一份，曲库右栏和全屏查词栏共用）；
- **例句**：这个词在全语料里的句子，点一条跳到那首歌的那一行。

**选了哪一页记住**（`jp.library.corpusTab`）：下次查词直接停在你上次看的那一页。
原来那个「折叠词典」的开关没有了——切换栏已经把「现在不想看词典」这件事表达清楚了。

歌词上那个查词记号也改了：**只变色，不铺底色**。原来是 `rgba(76,194,255,.18)` 的蓝块压在字上，
盖住歌词还很显眼。另外**关掉全屏的查词栏时会把记号一起清掉**——面板都关了，
记号留在歌词上没有意义（`onCloseSide` 里连 `lookup` 和 `activeLemma` 一起清）。

### 「显示」面板为什么会卡住两秒

点开歌词右上角的「显示」（设置页的字体下拉同理），界面会整个冻住。
量出来的分布：

| 步骤 | 冷 | 热 |
|---|---|---|
| `fonts_catalog`（Rust 枚举 DirectWrite） | 119–135ms | 缓存 |
| canvas 探测 401 个字体名 | **1,779ms** | 4ms |

探测贵在**字体引擎第一次解析每个字族**：125 个名字单个超过 5ms，最慢 41.8ms；
同一批再跑一遍只要 4ms。所以不是算法问题，省不掉，只能别让它卡在主线程上。

三处改动（实测点开「显示」从 1,931ms / 最长卡顿 1,802ms → **31ms / 0**）：

1. 样本串去掉 CJK。带「あいう漢字」要 2,730ms，去掉之后 1,779ms，
   而且**判定一个不差**（348 个可用，逐名比对 0 处分歧）——慢的是 CJK 兜底字体的首次解析，
   判定本身靠拉丁部分的宽度差就够了。
2. 探测分片跑（6 个一片，片与片之间 `setTimeout(0)` 把主线程还回去），
   结果按字体清单存进 localStorage，装了新字体才重算。
3. 启动后空闲时就预热（`warmFontOptions`），等用户点开面板时通常已经好了；
   没好也只是先显示「读取字体…」，界面不冻。

**`document.fonts.check()` 替代不了这个探测**：在 WebView2 里它对任何不存在的字体名
都返回 true（实测 `"NotAFontXYZ123"`、`"____"` 全是 true），只能继续用 canvas 量宽度。

### 进全屏的动画

`.stage` 进场 220ms 淡入 + 上移 14px，歌词区再迟 60ms 浮上来 320ms——
**只动 `opacity` 和 `transform`**，这两样走合成器，不会被那一屏歌词的布局拖慢。
系统里关了动画效果（`prefers-reduced-motion`）就不动。

左下角那张歌曲卡片也能点进全屏：请求由外壳发出（`stageWanted`），曲库页收到后打开再清掉。
**不能用「计数变了才算」**——点的时候人往往在别的页面，曲库页这时才挂载，
挂载那一下读到的就是新值，看着像没变过，请求就丢了。

### 版面规则：区块，不是方框

2026-09-23 用户的话是「你这个 UI 很多页面都是一个个长短不一的方块组成」。
确实是：每一块都套一个带边框的圆角盒子，而且几个工具页的容器用了 `.maintenance`
（`align-items: flex-start`），块宽还跟着内容走，于是一页下来十几个长短不一的方块。

现在全项目一套规则：

| 规则 | 做法 |
|---|---|
| 一块 = 一行标题 + 内容 | `components/Section.tsx`；`.card` 也改成了同样的样子（只剩间距），真要圈起来的加 `.boxed` |
| 块宽一致 | 内容页 `.page` 限宽 1600px **并居中**，留白给 padding（`max(20px, (100% − 1600px) / 2)`）而不是给 `max-width`——限在滚动容器上会让滚动条缩到内容边上，窗口一最大化右边就是一大片空白加一根飘着的滚动条。自己限了宽的页（设置 980px）要把这份 padding 换回 20px：**百分比 padding 按包含块算**，照搬会把内容挤扁。工具页的容器类从 `.maintenance` 换成 `.tools`（后者不改布局，只管按钮样式） |
| 页名不重复 | 命令栏已经有「标题 + 一句说明」，页面里不再写一遍 `<h1>`，只留 `.page-lead` 那一句 |
| 一排封面 | `Shelf`：`grid-auto-flow: column`，放不下横向滚 |
| 空状态给入口 | `.empty-state`：图标 + 一句话 + 主操作。词典页一本都没有时给的是「导入 .zip」，检索页没查过时给几个高频词 |
| 没照片的人画首字 | `components/Avatar.tsx`。一列灰圆看着像坏了，而作词作曲本来就没有照片（刮削只查演唱者） |

**首页重做了。** 原来是七个方框（概览、高频词、最近播放、听得最多、收藏、年代、流派、专辑），
而且专辑那一格点不动——「点不进去就是摆设」。现在是：一行概览数字 → 继续听 → 专辑 → 收藏 →
从这些词开始 → 听得最多，**每一块都能点进去**（点专辑把曲库限定到这张专辑，正好补上
Artist → Album → Track 的中间一级）。年代 / 流派这两块没有去处，移回分析页。

### 设置页

2026-09-21 加的，钉在左下角（和 Windows 11 设置、Niratan 一样不排进导航列表）。
版面是 WinUI 的 SettingsCard：一组一组的行，左图标、中文字、右控件，**改了立刻生效、自动记住，没有「确定」**。

**这一页不自己存状态。** 每一行读写的都是原来那处的存储，所以在歌词右上角的「显示」面板里改字号，
设置页跟着变，反过来也一样：

| 组 | 内容 | 存在哪 |
|---|---|---|
| 外观 | 全屏背景用封面虚化 + 强度、曲库网格先显示歌手 | `settings.ts` |
| 歌词 | 振假名默认开关、注音方式、主/备用字体、导入字体、字号、振假名字号、行距、字距 | `lyricsDisplay.ts` |
| 词典 | **完整的词典管理**：导入、从旧版迁移、启用 / 停用、拖动排序、删除 | `dictionaries.db` |
| 查词 | 清空「折叠记忆」 | `dict/collapse.ts` |
| 制卡 | 牌组、按歌手放子牌组、笔记类型、首选释义、卡片类型 | `dict/mine.ts` |
| 曲库维护 | 补齐缺失封面、回填时长、去刮削页 | 后端命令 |
| 关于 | 库路径与条数、分词器 / 音频 / 变调状态 | `health` |

制卡那一组是 `dict/MineSettingsCard.tsx`，Anki 页用的是同一个组件（那边多给一个组标题），
所以两处永远一致。`components/SettingCard.tsx` 里是 `SettingGroup` / `SettingCard` / `Switch` /
`SettingSlider` / `SettingSelect`，词典页的「已安装」列表也用它们。

**播放条为什么盖在全屏歌词上面。** 原来全屏那一层的下边界停在播放条上沿（`bottom: playerHeight`），
于是上面是封面、下面是一条独立的深色板，两块明显不是一个东西。现在全屏铺到窗口底边、
播放条 `z-index: 60` 盖在它（50）上面：播放条是 55% 半透明 + `backdrop-filter`，
背后正是那张虚化封面，颜色透上来，两块就是一个面了。歌词内容按 `--player-h`（实测的播放条高度）让开，
不会被压在条底下。

**播放条里不用强调色。** 频谱、进度、音量、播放键原来都是浅蓝，盖在封面上很跳；
现在全是白色半透明，靠亮度分主次——播放键是亮一档的毛玻璃圆钮，不是一个蓝圈。
强调色留给导航选中、开关这些「状态」。

**词典管理搬到了设置页。** 原来占着词典页右边那一栏——查词的时候没人在管词典，
管词典的时候也不是在查词，两件事挤在一屏里只会互相打扰。现在词典页整页就是查词（居中一栏），
**一本都没有时**给的是导入入口而不是一个空输入框。排序可以直接拖（`dict-row` 上的 HTML5 拖放，
落点画一条线），也能用键盘：抓手上按 ↑ / ↓。拖完立刻写库，没有「保存」。

**歌手墙**（曲库的封面网格）：头像来自 `artists.image_path`，和人物页共用 `PerformerPicker` 里那份缓存，
不为了头像再查一遍库。名字**精确匹配**、别名不猜；合作署名（`A/B`）退到第一位演唱者——
信用表本来就是按 `/` 拆的，第一位是主唱。点进去是这位歌手的歌（封面卡片），左上角返回；
点开哪位只活在这次会话里，下次打开还是从歌手墙开始。

## 曲库歌词：振假名、字体、显示设置、全屏

歌词区右上角「振假名」开关、「显示」面板、「全屏」按钮。「显示」面板：主字体、备用字体、导入字体、预览句，
字号 12–30、振假名字号 6–18、行距 0–36、字距 0–12、只注汉字 / 整词注音。
范围照 Python 版 `_song_display_settings`，默认值是新版原来的样子（字体默认是界面字体），改了立刻生效、存 localStorage。

### 字体

对应 Python 版的 `_font_options` / `_import_font`。可选字体三类，下拉框里按这个顺序排：

1. **字体文件**：项目自带的 `assets/fonts`（Klee One、霞鹜文楷）和用户导入的 ttf/otf/ttc。没装进系统，
   界面用 `FontFace` 按文件加载；`fonts_catalog` 命令把这些文件**逐个**放行进 asset 协议，CSP 加了 `font-src asset:`。
   导入只记文件位置（localStorage），不复制；文件没了会提示「清除 N 个读不到的」。一个文件只列一项。
   族名自己读 name 表（`jp-app/src/fonts.rs`，不引入字体库依赖，只读表目录和 name 表）。
2. Python 版排在前面的常用字体（装了才列）；
3. **本机全部字体**：问 **DirectWrite**（`windows` crate 早在依赖树里，只多开 `Win32_Graphics_DirectWrite` 特性）。
   WebView2 按名字找字体走的就是它。字体族名 + 每个字重的 GDI 兼容名（「Yu Gothic UI Semibold」，挑它等于挑字重），
   中日文名作为别名显示在选项里。

对账过程（都在 Edge 里用 canvas 量宽度判断名字是否真的用上了字体）：

| 做法 | 列出 | Edge 认 | 漏掉 Qt 列表里 Edge 认的 |
|---|---|---|---|
| 自己读 name 表，只取 nameID 16 | 236 | 205 | 117 |
| 读 name 表，nameID 16 + 1 | 273 | 233 | 89（可变字体具名实例、DirectWrite 归并名、中文名） |
| DirectWrite 族名 + GDI 兼容名 | 348 | 295 | 只剩 `Courier`、`MS Sans Serif`、`MS Serif` 三个点阵字体 |

DirectWrite 列出、Edge 不认的 53 个是图标字体（Marlett、Segoe MDL2 Assets…）和「Yu Gothic UI Semilight」这类
Chromium 匹配不上的字重名，选了也不生效，所以前端**进下拉框前在 WebView2 里再量一次**，只留能用的（`fonts.ts` 的 `createFontProbe`）。
真实应用里实测：299 个选项；Klee One（自带）、DejaVu Sans（导入，在 assets 外）都按文件加载成功。

### 全屏歌词

对应 Python 版的展开歌词播放器（`LyricsPlayerView`）。`components/LyricsStage.tsx`：

- 盖住整个窗口，只留底部播放条——倍速、音调、进度都在那里，不再做一份（Python 版把倍速、音调挪到了顶部）；
- 当前行放大加粗、停在可视高度 45% 处，换行时平滑滚过去；滚轮可以自己翻，下一次换行回到当前行（和 Python 一样）；
- 点一行跳到那一句；点词在右侧查词，内容就是曲库页右栏那一份（语料统计、词典、制卡、例句）；
- 顶部同样有振假名开关、「显示」面板；Esc 退出（「显示」面板开着时先收面板）。

行的内容由曲库页的 `renderLineText` 渲染后传进去，和普通视图同一份代码。第一版滚动区上下用 45vh 的 padding 留空，
padding 压不扁，把滚动区撑到 810px、超出容器 732px，最后一行压到了播放条上——改成 `::before`/`::after` 占位，
真实应用里复测滚动区底边正好停在播放条上沿。

振假名照 Python 版 `_furigana_tokens` / `_kanji_only_furigana` 逐行移植到 `jp-tokenizer/src/furigana.rs`，
命令 `lyrics_furigana` 现场用 Sudachi（SplitMode C）分词取读音，不入库。**全部 8443 行、两种模式逐段和 Python 一致**
（`jp-app/examples/dump_furigana.rs` 对 venv 里用 `ast` 抽出来的原函数），一行约 0.02 ms。

注音套在可点的词里面（`lyricSegments.ts`）：实测库里的分词全部能对齐回原文、21000 段注音没有一段横跨两个词，
所以每个词按钮里直接放 `<ruby>`，点词查词照旧。对不上时那一行不注音、不猜位置。顺带把词与词之间原文里的空格也显示回来了
（原来按词拼接会丢掉）。

## 语料库在哪：新装的程序第一次启动

0.2.0 装好之后**双击桌面图标毫无反应**。原因是一条链：

1. `locate_project_root` 只看 `corpus.db` 这个文件在不在；
2. 安装目录下没有，于是退到当前工作目录，`Corpus::open_writable` 在那里
   **凭空生成了一个 0 表的空文件**（SQLite 打开不存在的路径就是这个行为）；
3. `check_schema` 发现没有 `songs` 表，`run()` 打印一行 stderr 然后 `exit(1)`——
   GUI 程序没有控制台，用户什么都看不到；
4. 更糟的是那个空文件留在了安装目录里，**之后每次启动都会认准它**，每次都这样退出。

现在（`library_root.rs`）：

| 改动 | 为什么 |
|---|---|
| `looks_like_library()` 真的去查有没有 `songs` 表 | 空壳文件不能算库，否则会一直卡在上面那个循环里 |
| 找不到库就用 `%LOCALAPPDATA%\JPOP Corpus Tool` | 安装目录会被卸载程序整个删掉，用户数据不能放那儿 |
| `Corpus::ensure_schema()` 建表 | 新装的程序第一次启动就能有一个能用的空库，从「导入」开始加歌 |
| 设置页「切换目录…」写 `settings.json` | 已经有库的人不该被逼去配环境变量；重启生效，因为连接和 asset 放行范围都是启动时定的 |
| 启动失败弹 `MessageBoxW` | 再有别的致命错误时，至少要说得出话，而不是「点了没反应」 |

`schema.sql` 是**从迁移完成的真实库里导出来的**（`scratchpad/gen_schema.py`），
不是手写的副本——schema 的事实来源是库本身。FTS5 的影子表和 `sqlite_sequence` 不在里面，
那两样由 SQLite 自己建。

实测（把 exe 单独放进一个空目录、LOCALAPPDATA 指到临时目录、不设环境变量）：
程序正常打开，`%LOCALAPPDATA%\JPOP Corpus Tool\corpus.db` 建出 22 张表，
各页都能开；在设置里选 `D:\jp_corpus` 再重启，读到的就是 209 首那个库。

## 音频引擎（jp-audio）

**位置是「歌里的秒数」，自己数，不用 rodio 的 `get_pos`。** 用户报「改了倍速歌词对不上」：
rodio 数的是送进声卡的样本（墙上时间），而 WSOLA 是时间伸缩——1.5 倍速放 3 秒，歌里过去的是 4.5 秒。
实测旧实现在 0.75× / 1× / 1.5× 下位置都按 1.0 倍往前走（位置增量 ÷ 墙上秒数 = 1.00 / 1.00 / 1.00），
歌词自然越走越偏。现在 `RateControlled` 每吐一个样本按当时的倍速往前记一点，记的是歌里的秒数；
修完实测同样三档是 **1.00 / 1.50 / 0.75**。

**跳转的参数也是歌里的时刻。** `rodio_wsola::Wsola::try_seek` 把传进来的时刻当「输出时间」、
会自己乘一次倍速，所以我们先除回去——不除的话 1.5 倍速拖到 3:20 会去找 5:00，
超过结尾直接报 `Symphonia decoder returned an error`，播放停在原地不动，
用户看到的就是**「拖一下卡住」**。

**结尾坏掉的文件要兜住。** 库里有一批 FLAC 结尾是坏的（ffmpeg 报 `invalid residual`，
见 [[project-audio-pitch-furigana]] 里那 179 个），实测那首 254 秒的歌**倒数 2 秒内怎么跳都失败**，
退到倒数 5 秒就正常。所以跳转失败时依次往回退 0.5 / 2 / 5 秒再试（`seek_candidates`），
退完还不行**而且目标就在结尾那 10 秒里**，就当这首放完了（清空队列、位置摆到结尾），不弹报错；
中间位置跳不动才是真有问题，照常报错。修完实测：拖到最末尾（甚至超过时长）落在 250 秒继续放，不再报错。

**`audio_seek` 改成异步命令**（`spawn_blocking`）：`try_seek` 要等音频线程真的跳完才返回，
同步命令跑在主线程上，拖进度条时整个界面跟着顿。


**放完之后还能拖回去重播。** rodio 的队列播完就空了，`play` / `seek` 作用在空队列上什么都不会发生
（`try_seek` 还会报「跳转失败」）。引擎记着当前这首的文件和 id，队列空时按原文件重新装一次、跳到目标位置接着放；
`stop()` 之后不会自己复活。界面那边进度条原来只在鼠标松开时**恰好还在滑块上**才提交，拖出去松手就白拖了，
看着像拖不动——改成监听窗口级的 `pointerup`。

```
Decoder ─→ RateControlled(WSOLA) ─→ Tapped ─→ 换算成声卡格式 ─→ Player ─→ 声卡
                  ↑                    │
             倍速（原子变量）        采样副本 → FFT → 频谱
```

**每首歌进 Player 之前先换算成声卡的采样率和声道数。** 这是修过的 bug（2026-09-17，用户报告「有些歌没动就升调或降调了」）：
Player 是一条一直开着的队列，rodio 只在「段」（span）的边界重新读当前歌的采样率，而 WSOLA 报的是「没有分段」，
于是本次启动第一首歌的采样率被一直用下去。曲库里 44.1 kHz（143 首）和 48 kHz（65 首）混着，
后面采样率不同的歌整首变调变速约 1.5 个半音、8.8%——先放哪首决定之后哪些歌出错，所以「有些歌」。

- 复现：`engine::tests::songs_with_different_sample_rates_keep_their_pitch`，不开声卡，Player 接 48 kHz 的 mixer 手动拉样本，
  440 Hz 的测试音修之前第二首量出来 404.0 Hz（正好是 440 × 44.1/48）；
- 真实文件对账：`real_songs_in_a_row_play_at_the_right_speed`（`JP_AUDIO_PAIR` 指两首采样率不同的歌，ignored），
  后播那首单独播和接在后面播，对齐后比波形：旧链路相关系数 0.02–0.10，新链路 1.0000。

**变速不变调。** rodio 自带的 `set_speed` 实现是「提高采样率」，
也就是重采样——0.75 倍速会连音高一起降下去。对语言学习工具这是致命的：
慢放是为了听清词，不是为了把人变成低音炮。所以走 `rodio-wsola` 做时间伸缩，
和 Python 版 `QMediaPlayer.setPlaybackRate()` 的行为对齐。

选型时试过 `signalsmith-stretch`（质量更好，还能实时变调），
但它是 C++ 封装、需要 libclang（bindgen），对开源项目的贡献者是额外门槛。
纯 Rust 的 WSOLA 对语音够用。

**变调**照 Python 版：ffmpeg 的 `rubberband` 滤镜把整首离线渲染成 FLAC，缓存到 `output/pitch_cache/`，再播缓存文件
（`jp-audio/src/pitch.rs` 渲染和缓存，`jp-app/src/pitch.rs` 决定什么时候渲染、换哪个文件）。

- −6…+6 半音，全局生效，播放条上的选择框一直显示当前调（不是原调时高亮）；不跨重启保存，和倍速一样；
- 缓存文件名和 Python 版逐字相同（`<文件名>_<±n>_<sha1("绝对路径|mtime_ns|字节数|n") 前 16 位>.flac`），两边互用；
  原曲一改缓存自动失效。SHA-1 手写，标准测试向量钉住；真实文件的名字和 venv 里 CPython 算的对过；
- 打开一首歌时没有这个调的缓存：先停、渲染好再开播（不先用原调放一段）；等待期间按的播放/暂停/拖动记下来，渲染好才生效，
  所以 `audio_load` 一次带上起点和 `autoplay`；
- 正在放时换调：原来的继续放，渲染好后接着**当时**的位置和播放状态换过去，单句循环保留（Python 版是跳回点击时的位置）；
- 渲染失败（没有 ffmpeg、报错、超时 180 秒）退回原调，原因放在 `pitchError`，前端只提示一次；
- 先写 `.part` 再改名（Python 版直接写目标文件，中途打断会留下半截文件被当成缓存）。

实测（release，真实曲库）：一首 4–5 分钟的歌渲染 7–9 秒；变调后时长和时间轴不变（合成信号上三处静音标记前后差 3 ms 以内），歌词跟随照用。

顺带查出：**曲库里 179 个 FLAC 结尾是截断的**。ffmpeg 和 symphonia 两个解码器都在同一处报错停下（中位数少 1.15 秒，最多 7.7 秒，
全库合计 5.2 分钟），解出的 PCM MD5 两边一致、和 STREAMINFO 里的 MD5 对不上；这 179 个文件的大小全是 4096 字节的整数倍
（103 个是 65536 的整数倍），是获取文件时被截在缓冲区边界的特征。导入用的是 `shutil.copy2`，不是导入弄坏的。
原调和变调版都在同一处结束，播放不受额外影响。

**分层**：只有 `engine` 碰音频设备，`state` / `tap` / `spectrum` 是纯逻辑，
无声卡环境也能完整测试。

| 模块 | 测试 |
|---|---|
| `state` `tap` `spectrum` | 29 个，纯逻辑 |
| `engine` | 无声卡自动跳过；采样率那条不需要声卡 |
| `pitch` `sha1` | 缓存名、参数、真 ffmpeg 渲染（没有 ffmpeg 跳过） |
| `tests/playback.rs` | 10 个，拿真实音频文件跑完整链路 |

播放测试期间音量置 0，不会真的出声——tap 挂在 Player 的音量控制**之前**，
所以静音不影响采样流。

### 几个实现决定

**循环看门狗是独立线程，不是塞进 Source。** 在 `next()` 里做 seek 会打断
WSOLA 的重叠窗，产生咔哒声。轮询间隔 20ms。

注意 rodio 会预缓冲，回跳时已进入输出缓冲的那一小段仍会播出来，
所以听感上的循环点比设定值略晚。这个限制 Python 版（150ms QTimer）也有，
而且更明显。

**tap 写入用 `try_lock`，拿不到就丢。** 频谱掉一帧没人看得出来，
音频卡一下所有人都听得出来。

**倍速每 512 个样本才读一次原子变量。** 每样本都读会让原子操作成为热点，
而倍速是人手操作，毫秒级延迟感知不到。

## 换歌之后位置没有归零

**症状**：播完一首点下一首，歌词整屏滚到最后一行，进度条顶在最右边。

根因在引擎，不在界面。`RateControlled::new` 原来把 `shared.position_sec()`
抄进新 source 当起点：

```rust
let position = shared.position_sec();   // ← 上一首停在 4:08，新歌从 4:08 开始数
```

`load_at` 里 `playback_chain(...)` 在 `set_position_sec(0.0)` **之前**就构造好了，
所以清零清了个寂寞——新 source 手里已经捏着旧值，第一个样本一吐就写回去，
然后一路往上加。界面拿到的位置是「上一首的位置 + 新歌已播的时间」：
进度条顶满，歌词跟随按这个位置二分新歌的时间轴，落在最后一行，于是整屏滚到底。

现在新 source 一律从 0 数起。要从中间开始放的两种情形——变调换版本、放完再拖回去——
都由 `load_at` 在 append 之后显式 seek 一次，`try_seek` 会把 source 和 shared 两边都设对。

实测（真库副本，209 首）：切歌前位置 248 秒、歌词栏 scrollTop = 1408（到底）；
切歌后**旧版**位置仍报 248 并继续往上数、scrollTop 滚到 934（新歌的底），
**修完**位置 0、scrollTop 0、当前行是第一行。回归测试
`switching_songs_restarts_the_position_at_zero` 盯的就是这条。

顺带两处：

* 当前行的推导（`useCurrentLine`）从 ref + effect 换成 `useMemo`，并多一个
  「这份歌词是不是正在放的那一首」。effect 在渲染**之后**才跑，换歌那一帧读到的
  还是上一首的时间轴；而歌词属于选中的那首、位置属于正在放的那首，两者不是同一首时
  本来就没有当前行。纯函数 `pickLine` 单独测。
* 播放条松手的监听改成常驻。原来挂在「正在拖」上，而 effect 在渲染之后才跑——
  主线程忙的时候，pointerup 可能早于那次 effect，这一下点击就只改了显示、没发 seek。
  另外 seek 改成**等引擎真的跳过去**再放开滑块显示：先放开的话，下一拍轮询回来的
  还是跳之前的位置，进度条会先弹回去再跳过来。实测 1.5 倍速下拖到 60 秒，
  旧版第一帧滑块还停在 182 秒，修完是 60。

## 当前行为什么在前端算

`PlaybackState` 里**没有** `currentLine`。需求书第十四条列了它，
但那是「位置 + 歌词时间轴」推导出来的——放进引擎会让音频层依赖歌词层，
正是要拆开的耦合。

推导在 `app/src/usePlayback.ts` 的 `useCurrentLine`，用二分查找
（歌词行数上千时线性扫描在 10Hz 轮询下是可观开销）。

## 状态订阅为什么是轮询

Tauri 的事件通道适合偶发事件；用它每秒推 10 次状态会在 IPC 上产生大量
小消息。轮询一个廉价 command（读几个原子变量）反而更省，
也不用管订阅生命周期。状态 10Hz，频谱 20Hz。

## 时长回填

`songs.duration_sec` 原来全库为空，导致进度条只能靠解码器给的时长、
分析页的总时长显示不出来、刮削也没法用时长区分原版 / TV size。

探测放在 `jp-audio::probe`，**和播放共用同一套解码器**——
两边读出来的时长必然一致，不会出现「进度条和库里对不上」。

**交叉校验**：光验证「读到了一个数」挡不住量纲错误（把采样数当毫秒照样
是个数）。所以拿 mutagen 的结果做基准逐条比对，容差 0.2 秒
（不同库对最后一个不完整帧的处理不同，FLAC 一帧 ≈ 93ms @ 44.1kHz）。

```bash
python scripts/export_durations.py          # 生成基准
cd rust && cargo test -p jp-audio --test duration
```

实测 **209/209 与 mutagen 一致，零偏差**，总时长 14.66 小时。

回填三种入口，共用 `maintenance::backfill_durations`（不依赖 Tauri，可离线测）：

```bash
cargo run -p jp-app --example backfill_durations   # 命令行
```

分析页的「回填时长」按钮，或 `backfill_durations` command。
**幂等**：只填为空的行，重跑扫描 0 条。

报告里**每种失败单独计数**（文件缺失 / 无时长 / 解码失败）——
笼统的「成功 N 个」用户无从判断该修什么。

## 导入后没有歌词：三条来路，一个出口

**症状**：从音乐 App 的下载目录导进来的歌，一行歌词都没有；0.1.x 导完就有。

**原因**不在导入，而在上一步。0.1.x 的歌词是 `scripts/01_fetch_lrc.py` 用
`syncedlyrics` **下回来的**，存成 `raw/lyrics_lrc/{song_id}.lrc`，
`02_parse_lrc_tokenize.py` 再入库。Tauri 版的导入只认**已经在磁盘上**的
.lrc（扫描时在音频旁边找），下载这一步整个没有移植过来。

现在三条来路（`jp-app/src/lyrics.rs`）：

| 来源 | 什么时候用 | 入口 |
|---|---|---|
| `Sibling` 音频旁边的同名 .lrc | 导入之后才下了歌词 | 补齐时自动优先 |
| `Netease` 在线搜 | 旁边什么都没有 | 设置 → 曲库维护 → 补齐缺失歌词；曲库里单首「在线补齐」 |
| `Manual` 用户自己选的文件 | 自动挑不出来，或者用户有更好的一份 | 曲库里「导入歌词文件…」/「换一份歌词…」 |

三条都走 `lyrics::apply_lrc_text`：**先落盘 `raw/lyrics_lrc/{song_id}.lrc`，
再在一个事务里清旧挂新**。顺序是刻意的——落盘在前，写文件失败就什么都没改；
歌词行、全文索引、分词、作词作曲必须同时成立，否则会得到一首「有歌词但搜不到」
或者「分了一半词」的歌。文件留着是为了可恢复，也和 0.1.x 的目录结构对得上。

入库那一步和导入**共用同一份代码**（`jp_import::attach_lyrics`，从
`import_one` 里抽出来的）。两条路写出来的库必须长得一样，所以只能有一份实现。
换歌词时先 `maintain::clear_lyrics`——外部内容的 FTS 表没有触发器，
漏掉 `'delete'` 那一步，旧歌词仍然搜得到、点进去是一条不存在的行。
用户校正过的分词不会丢：校正按「歌手 + 曲名 + 原文」匹配，挂完新歌词会套回去。

### 挑不准就不要

`jp-scraper/src/lyrics.rs` 里的 `pick_best` 是纯函数，单独测：

* 曲名相似度 < 0.78 不要（挡住 remix、长版本）；
* 歌手相似度 < 0.6 不要——**曲名一模一样也不放宽**。同名曲太多
  （「再会」Vaundy / 八代亜紀 / KIRINJI 都有），塞错一首的歌词比没有歌词糟得多；
* 时长差 > 8 秒不要（live、另一版剪辑）。库里没记时长时这一条不拦。

代价是网易云把歌手写成罗马字（ZUTOMAYO）时会漏——那种情况自己导一份。

**为什么是网易云**：实测 lrclib 对这批日文曲目几乎没有收录
（「ヨルシカ 夏、バス停、君を待つ」「ずっと真夜中でいいのに。 勘ぐれい」
「Vaundy 再会」搜出来都是 0 条）；网易云一搜就有，给的是带逐行时间轴的 LRC，
连「作词 : 山口　一郎」这样的信用行都带着，正好是 LRC 解析认得的那一套。

### 实测

用户那个新库（16 首、0 行歌词，音频在 `E:\cloud music\…`，旁边没有 .lrc）
的**副本**上跑 `cargo run -p jp-app --example fill_lyrics -- <目录>`：

```
16 首缺歌词（其中 0 首旁边就有 .lrc）
补上 16，挑不出来 0，失败 0
```

逐项对账：635 行歌词、3,890 个 token、**0 个空白 token**、0 行没有时间轴、
FTS 行数 = 歌词行数、每首歌随机取一行都搜得到、时间轴全部单调、
38 条来自 LRC 的作词作曲署名；008 的前 12 个 token 词性是 PRON/ADP/DET/NOUN…
（UPOS，和存量库一致）。之后在真实应用里点「补齐歌词」，界面报
「补齐完成：补上 16，挑不出来 0」，设置页那一行变成「每一首都有歌词了」。

### .lrc 的编码

`jp_import::lrc::parse_file` 的文档一直写着「UTF-8 → CP932 → GBK」，
**但代码只做了 `from_utf8_lossy`**。日文站点下回来的 .lrc 不少是 Shift-JIS，
整份会变成一串 U+FFFD——而且因为「有歌词」了，补齐流程再也不会来看它。
现在真的按那三种依次试（`encoding_rs`，已经在依赖树里）。CP932 排在 GBK 前面：
两者的双字节区大面积重叠，同一串字节往往两边都解得出字来，这是个日文语料库。
存量的 206 份 .lrc **全部是合法 UTF-8**，走的还是第一条路，结果逐字节不变。

## 单曲导入

以前只能选整个文件夹。「音乐 App 下载目录里新增的那一首」于是只能
把文件挪进临时目录，或者重扫整个目录再在几百条计划里找那一条。

`scan_dir` 本来就认单个文件（`root.is_file()` 那一支），缺的只是一个入口：
`scan_files(paths)` 收一批具体文件（也允许夹着目录），和 `scan_folder` 共用
`plan_targets`——扫描、比对、复核、确认导入全是同一条链，单曲导入不是
「另一条导入路径」。

两处细节：

* **按路径去重**。选了文件又选了它所在的目录时，同一个文件只算一次，
  否则计划里会出现两条一模一样的、还互相判成「本批重复」。
* **选错的文件要报出来**。用户可能顺手选中 .lrc 或封面图；默不作声地丢掉
  会让「选了 5 个只导了 3 个」看起来像 bug，所以放进 `ScanResult.ignored`，
  界面逐个列出来。

## 把另一个库的数据搬过来

0.1.x 的库在项目目录里，新装的程序默认用 `%LOCALAPPDATA%\JPOP Corpus Tool`，
而新库里往往已经导了几首歌。「切换目录」只能二选一，所以另做了**合并**
（`jp-import/src/migrate.rs` + `jp-app/src/migrate.rs`，设置 → 曲库维护）。

`ATTACH` 源库，**基本都在 SQL 里做**——250 万行词条逐行读进 Rust 再写回去没有意义。
只有两件事在 Rust 里：去重和分配新 id，那两步要用 `jp_normalize` 的归一化规则。

| 东西 | 怎么处理 |
|---|---|
| 歌 | 按「音频路径」或「歌手 + 曲名」去重（和导入同一套判据），剩下的分新 id 搬过来 |
| 歌词行 | id 按 `ORDER BY id` 连号重排，这样 `tokens`、`token_corrections` 在 SQL 里就能跟着改 |
| 全文索引 | 外部内容表没有触发器，`utterances_fts` 要手动插 `(rowid, text)` |
| 人、专辑、歌手 | 按归一化键 `INSERT OR IGNORE`，再 join 回来当映射 |
| 0.1.x 词典表 | `dict_terms` / `yomitan_*` / `jlpt_cache` 整表搬——制卡的释义和统计页的 JLPT 都读它们 |
| `dictionaries.db` | 另一个文件，整份复制；目标已经有就不覆盖 |
| 封面 / .lrc / 歌手照片 | 文件名里有 song_id，按新 id 改名复制，再把新路径写回库 |
| 音频 | **不复制**。`audio_path` 是绝对路径，源目录还在路径就还通；搬几十 GB 不是这个功能该做的事 |
| 分词词典 | `venv` 里那份 `system.dic`（207 MB）复制进目标库的 `sudachi/`；目标已经有就不动 |

整件事一个事务，中途出错什么都不会变；文件复制失败只少几张图，重跑会补上
（重跑时歌已经在库里，走「已存在」那条路，不会搬两份——这条有测试）。

**实测**（源 `D:\jp_corpus` 209 首 / 993 MB，目标是那个 16 首新库的副本）：

```
要搬 194，已经有了 15（サカナクション 那批两边都有）
歌 194、歌词 7,808 行、分词 54,918、署名 414、收听 116
封面 194、歌词文件 191、歌手照片 17、词典库 766 MB、词典词条 2,500,735
用时 27.8 秒
```

逐项对账：目标库 210 首 = 16 + 194；FTS 行数 == 歌词行数；重复 id 0；
`utterances`/`tokens`/`track_credits`/`songs.album_id`/`play_history` **没有一条外键落空**；
抽 40 首和源库比每首的歌词行数，0 处不一致；随机抽 5 行歌词按前 4 个字搜，5 条全中；
SQLite 自己的 `integrity-check` 通过。

### 分词词典从哪来

第一次迁完有个漏：歌、歌词、词典都在了，歌词上面却常驻一条
**「振假名不可用：找不到 Sudachi 词典」**。原因是 `locate_sudachipy` 只看
`<库>/venv/Lib/site-packages`——0.1.x 把 SudachiPy 装在项目目录里，开发树上一直都有，
而新装的程序把库建在 `%LOCALAPPDATA%`，那里没有 venv。

现在三条路，按顺序：

| 来路 | 在哪 |
|---|---|
| 0.1.x 的 venv | `<库>/venv/Lib/site-packages/…` |
| 库里自己的一份 | `<库>/sudachi/`——下回来的、迁移搬过来的，或者设置里从目录导入的 |
| 程序旁边 | `<exe 目录>/resources/sudachi/`，给「装完就有」的离线包留的口子，现在不发这种包 |

**用到振假名的时候程序自己去下**（`jp-app/src/tokenizer.rs` 的 `download`）：
从仓库的 `sudachi-dict-core` 这个 tag 取 `system.dic.xz`（45,128,544 字节，解开 217,203,456），
校验压缩包的 SHA-256 → 解压 → 再校验解开那个 `system.dic` 的 SHA-256 → 写配置 → 改名就位。

- **为什么不打进安装包**：量过——打进去安装包 6.8MB → 51.9MB（MSI 78MB），
  而且词典和版本无关，**每次更新都要重下这 50MB**；单独下的话一辈子只下一次。
- **为什么 xz 不是 zip**：同一份 `system.dic`，deflate 74.6MB、bzip2 64.6MB、
  **xz 43.0MB**。少下 30MB 值一个纯 Rust 的 `lzma-rs`（不要 C 工具链）。
- **为什么只下一个文件**：`sudachi.json` / `char.def` / `unk.def` / `rewrite.def`
  一共 15KB，`include_bytes!` 编进程序（`jp_tokenizer::EMBEDDED_RESOURCES`），
  于是不需要任何打包格式，也不会出现「配置和词典版本对不上」。
  这四个必须和 `system.dic` 配套——`sudachi.json` 里把 `〜` 归一成 `ー` 的那个
  `inputTextPlugin` 会改变分词结果。
- **两道校验**：压缩包那道是为了早点失败，解开那道才是要紧的——
  分词结果和 Python 侧逐 token 对过账，对的就是这一份字节。

界面上有两处入口：歌词上那条红条直接变成「下载词典（43 MB，只下这一次）」带进度，
设置 → 歌词 → 分词词典里也有一个。下好之后 `retryFurigana()` 推一把——
之前算失败的那首歌什么都没缓存，而 `key` 没变，effect 不会自己重跑。

- **复制而不是记路径**：记路径的话源目录一删一移，振假名第二天就没了，
  而用户不会想到是这个原因。手上已经有 0.1.x 的 venv 时，从那儿复制比下载快得多
  （实测 172ms vs 几分钟），所以「从目录导入」这条路留着。
- 先写 `.part` 再改名——207 MB 复制到一半断了，留下的是半截的 `.part`，
  不是一个「看起来齐了、加载时才炸」的 `system.dic`。
- 复制完当场 `Analyzer::from_sudachipy` 加载一次，装不上就把这份删掉，
  免得它顶掉下次的查找。
- 装上立刻生效：`AppState.analyzer` 改成 `RwLock<Option<Arc<Analyzer>>>`，
  导入词典和迁移跑完都会 `reload_analyzer()`，不用重启。

实测（库里只有 corpus.db、没有 venv）：装之前 `tokenizer_status.ready = false`、
`lyrics_furigana` 报错；指 `D:\jp_corpus` 导入，复制 217,218,859 字节用时 172 ms；
之后 `ready = true`、`health.tokenizerReady = true`、同一首歌 41 行全部注上假名。

### 两条路同时下，第二条报「改名失败」

0.2.4 发出去之后真遇上了：设置里点「下载并安装」，界面弹
**「改名失败：…\updates\JPOP.Corpus.Tool_0.2.4_x64-setup.exe」**，而那个目录是空的。

下载有两条路——启动时的自动更新（`App.tsx`，开了「下载后自动安装」才走）和设置里的按钮。
两条一起跑时，以前**共用同一个 `.part`**：

- 两边各写各的，内容交错；而各自的哈希只算**自己收到的字节**，
  所以**校验会「通过」，落盘的却是一团乱**——这比报错严重得多；
- 先改完名的那个把文件抽走，另一个的 `rename` 于是报「系统找不到指定的文件」，
  而错误里只有 `改名失败：<路径>`，操作系统说的那句被 `with_context` 盖掉了。

四处改动：

1. 一个进程同时只下一个（`DOWNLOADING` 锁）。后来的那个等前一个下完，
   然后走「已经下好而且校验得过就直接用」这条，不重下；
2. 临时文件名带进程号和时间戳，两个下载永远不共用一个文件；
3. 落盘失败重试 10 次（刚写完的 exe 可能还被杀软扫着），再不行就退回复制，
   **错误里带上操作系统原话**；
4. 安装程序一个进程只拉起一次（`INSTALL_LAUNCHED`）——两个 NSIS 同时跑会互相抢文件。

## 三栏的宽度可以拖

曲库页、人物页是「左 + 正文 + 右」，全屏歌词是「歌词 + 右栏」，宽度原来都写死在 CSS 里
（260 / 1fr / 320）。歌词长短、歌手名长短、查词栏里有没有音调图，各人各机器都不一样，
所以让它能拖（`components/ColumnSplitter.tsx`）。

- 拖的是 `grid-template-columns` 的第一条和最后一条轨道，**中间永远 `1fr`**：
  窗口变宽变窄，多出来少掉的都算在正文上，两边保持用户定的宽度；
  全屏那一栏是 flex，给的是 `flex-basis`，道理一样。
- **有边界**，而且边界是「正文还看得下去」而不是「别拖到 0」：

  | 版面 | 左 | 右 | 正文至少 |
  |---|---|---|---|
  | 曲库 / 人物 | 180–560 | 200–680 | 360 |
  | 全屏歌词 | — | 260–720 | 420 |

  窗口窄到两边一起把正文挤没时，**让步的是正在拖的那一侧**——否则拖左边会把右边也带着动，
  手感是错的。这几条是纯函数 `clampColumns` / `clampPane`，有 9 个用例。
- 记在 localStorage，按版面分开记；曲库的列表视图和网格视图各记各的
  （网格本来就该宽一些）。读写都包了 try/catch：隐私模式下 localStorage 会抛，
  抛了就当没存过，不能让整页跟着炸。
- 双击那道缝恢复默认；选中之后方向键一次 16px（`role="separator"`）。
- 指针用 `setPointerCapture`，`pointermove` 挂在把手上而不是 window——
  拖出窗口再松手也收得到，不会留下一个「还在拖」的状态。

实测（最大化 2560px）：曲库两道缝拖完 260→420、320→460，存进 localStorage；
一路往左停在下限 180；双击回到 260/320；重启程序之后还是用户定的那个宽度。
全屏右栏 340→540，往右到底停在 260，往左到底停在 720（歌词那边还剩 1730）。

## 检查更新

版本就发在仓库的 Releases 里，所以更新也从那儿来（`jp-app/src/update.rs`）：
问一次 `GET /repos/{owner}/{repo}/releases?per_page=5`，比版本号，需要的话把
`-setup.exe` 下回来、校验 SHA-256、再拉起安装程序并退出（NSIS 要替换正在运行的 exe）。

**没用 `tauri-plugin-updater`**：那套要自己签名、再维护一份 `latest.json`，
而这个项目的发布流程就是 `gh release create` 挂两个安装包。用现成的 Releases API
少一套要维护的东西；代价是自动更新只在 Windows 的 NSIS 包上成立——现在也只发这一个平台。

要跑的是一个 exe，所以有四条守得住的线，每条都有测试：

| 规则 | 为什么 |
|---|---|
| 下载地址必须以 `https://github.com/Yisoragoto/jpop-corpus-tool/releases/download/` 开头 | 接口返回里混进别的域名时直接丢掉那个资产，而不是「下下来再说」 |
| 装之前校验 SHA-256（GitHub 的资产带 `digest`） | 对不上就删掉重来。实测把期望哈希改成全 0，下完立刻报错、`tampered-setup.exe` 没留在磁盘上 |
| 没有 `digest` 的发布不自动装 | 校验不了的东西不执行，让用户去发布页手动下 |
| 版本号解析不出来就当没有新版 | 宁可不提示，也不要因为一个奇怪的 tag 名天天弹窗 |

「下载后自动安装」默认**关**着：装更新会关掉应用，什么时候更新该由人决定。
开关在「设置 → 系统」，那里还有「检查更新」和「查看更新日志」
（更新日志就是最近几次发布的 `body`，和检查更新共用同一次请求的数据）。

解析用的是**真实接口返回**存下来的 `tests/data/github-releases.json`，不是手捏的 JSON：
三次发布逐条对过版本号、发布时间、说明长度、安装包名字和 64 位十六进制的哈希。

实测：当前 0.2.2 / 最新 0.2.2 → 「已是最新」；下载 6,636,695 字节用 14 秒，
校验通过落到 `%LOCALAPPDATA%\com.github.yisoragoto.jpop-corpus-tool\updates\`。

## 收听统计

`play_history` 表和 `recently_played` / `most_played` 查询一直都在，
但**从来没有人往表里写**——首页要的数据链路缺的就是这一环。

判断「用户是否真的听了」是业务逻辑，所以放进可测的层：
`tracker::PlayTracker` 是纯逻辑（喂状态快照，吐该落库的事件），
不碰数据库、不碰 Tauri、不开线程，14 个单测。

### 「听了多久」按墙上时钟算，不按播放位置算

两者在三种情况下会分道扬镳：

| 情况 | 位置差 | 墙钟 | 哪个对 |
|---|---|---|---|
| 单句循环 | 原地打转，≈0 | 正常累加 | **墙钟** —— 人确实在反复听 |
| 2 倍速 | 2 秒 | 1 秒 | **墙钟** —— 注意力只花了 1 秒 |
| 拖进度条 | 瞬间跳几分钟 | ≈0 | **墙钟** —— 没人听那几分钟 |

墙钟衡量的是「花了多少注意力」，这正是收听统计想回答的问题。
另外采样间隔超过 2 秒不计入——系统休眠时把整段空档算成「在听」会严重虚高。

### 阈值

* 听满 **5 秒**才记一条。点开就切走不该淹没「最近播放」。
* 播到 **95%** 算听完。要求 100% 的话，结尾有淡出的歌几乎没有一首算得上。
* 用**见过的最大位置**判断有没有播到结尾，不是当前位置——
  播到尾又拖回去仍然算听完。

### 谁来驱动

前端本来就在 10Hz 轮询播放状态，那就是应用的心跳。轮询改调 `audio_tick`：

```
audio_state   无副作用的状态读取
audio_tick    状态读取 + 走一拍收听统计   ← 轮询用这个
```

**名字里带 tick 是因为它有副作用。** 让 `audio_state` 偷偷写库是不诚实的，
将来有人只想读一下状态就会莫名其妙多出记录。

墙钟间隔在 Rust 侧算，不信任前端报的数——窗口最小化时浏览器会节流定时器。

退出时通过 `RunEvent::Exit` 冲刷最后一段，否则关窗口那首歌就白听了。

端到端验证（跨引擎 → tracker → 库三层，单测各测各的，只有真跑才知道接没接上）：

```bash
cargo run -p jp-app --example verify_history
# 播放：サカナクション - さよならはエモーション
# ✓ 记录成功：さよならはエモーション 播放 1 次，累计 5.9s
```

## 全局搜索与命令面板

要求书第十四条的键盘交互：

| 键 | 行为 |
|---|---|
| Space | 播放 / 暂停 |
| ← → | 后退 / 前进 5 秒 |
| ↑ ↓ | 音量 ±5% |
| Ctrl/Cmd+K | 全局搜索 |
| Ctrl/Cmd+P | 命令面板 |

**输入框里一律不抢键**（空格是打字、方向键是移光标），
但 Cmd+K/P 例外——正在搜索时想切命令模式是合理的。

### 一个组件，两个入口

搜索和命令的交互完全一样（浮层 + 输入 + 键盘选择），差别只在候选来自哪里，
所以共用一个组件。输入开头打 `>` 在两种模式间切换，和多数编辑器一致。

搜索是**远端**的（跨全库五种实体），所以防抖 140ms 并丢弃过期响应——
慢的那次请求后到会覆盖新结果。命令是**本地**固定列表，即时过滤。

### 分组返回，不混排

`quick_search` 跨曲目 / 专辑 / 人物 / 词汇 / 歌词，**按类型分组返回**。

混排需要一个全局相关性排序，但跨类型的分数没有可比性：曲名的 LIKE 匹配
和歌词的 bm25（SQLite 里是负数，越小越相关）不是一个量纲。
硬凑出来的顺序是假的。分组让用户按类型自己找，也让前端能按类型
决定点击行为。

### 搜索不是死胡同

每种结果都有落点：曲目/歌词 → 曲库页并定位到那一行，词 → 语料面板，
人物 → 人物页。有测试钉着（`quick_search_targets_are_reachable`）：
每条结果的 id 都要真的能打开，歌词结果的 `utteranceId` 要真的在那首歌里。

人名和专辑名走 `normalized_*` 列匹配，所以「yoasobi」能搜到「ＹＯＡＳＯＢＩ」、
「山口一郎」能搜到「山口　一郎」——这些列正是当初建 people/albums 实体时存下的。

## 虚拟化列表

KWIC 检索一个常见词能命中上千行，全部渲染成 DOM 会让滚动明显卡顿。
上限因此从 500 提到 5000。

**没引第三方库**：行高是固定的（单行、超出省略），这种情况下的窗口计算
就是几行除法。`react-window` / `@tanstack/react-virtual` 是为了处理动态高度、
水平滚动、粘性表头这些用不到的场景。

计算在 `src/virtual.ts`，纯函数，**18 个单测**——虚拟化算错的表现是
「滚动时内容跳动」或「底部有一截空白」，靠肉眼很容易漏。测了这些边界：

* 负的滚动位置（macOS 橡皮筋）、滚过头
* 行高为 0 或负数（不能除以零）
* 视口高度为 0（首帧拿不到容器高度是常态，这时不该渲染成空白）
* 撑开的高度 + 渲染行数 == 总高（滚动条长度才正确）

**约束**：每行必须是固定高度且和 `itemHeight` 一致。CSS 里改了行高
就要同步 `KWIC_ROW_HEIGHT` / `TEXT_ROW_HEIGHT`，两处都有注释互指。

顺带给前端补了 vitest（之前是 0 测试）。只测纯逻辑，不引 jsdom——
组件测试是另一笔投入，现在的价值在于把「算错了没人会发现」的那部分钉住。

## 命令面板的定位

面板里点一条结果要能**落到具体的东西上**，不能只是「带你到那一页」。

**人物**：`person_by_id` 按 id 取人，并给出他作品最多的那个角色。
前端切到该角色（左侧列表才会包含他）再选中。
不去左侧列表里找——搜到的人可能根本不在当前角色的列表里
（按「作曲」列着，搜到的却是个只演唱过的人）。

有测试钉着：`person_by_id` 返回的主角色，必须真的能在
`people_by_role` 的结果里找到这个人——否则切过去会看到空列表。

**专辑**：没有独立页面，改成把曲库列表**限定到这张专辑**，
顶部显示一个可清除的专辑条。这也正好补上了 Artist → Album → Track
的中间一级（要求书第四条）。

**导入**：选目录 → 扫描 → 复核 → 确认。扫描只读，写库要再点一次。
计划里每一条都能说出理由（「已经是库里的 001」「曲名歌手对上了 016，
不会导入」）。管线本身在 `jp-import`，见 [import.md](import.md)。

**刮削**：识别曲目、补齐 metadata、下载封面。复核队列并排给出「本地写的
是什么」和候选，附打分解释；失败按原因分类并给出该怎么办；批量走后台
线程、可中断。管线在 `jp-scraper`，见 [scraper-rust.md](scraper-rust.md)。

选目录用 `tauri-plugin-dialog`。它经 `rfd` 打开了 `common-controls-v6`，
链进来的代码要求进程加载 ComCtl32 v6——真应用有 tauri-build 生成的清单，
但 `cargo test` 产出的 exe 没有，一启动就 `STATUS_ENTRYPOINT_NOT_FOUND`。
Cargo 的 feature 是并集关不掉，所以在 `build.rs` 里给测试 exe 补了
`/MANIFESTDEPENDENCY`。

## 还没做的

* 曲库列表本身还没虚拟化（209 首不需要；导入上千首时要）
* 词云（PyQt 版统计页有「词云图」按钮）
* 浅色主题（现在只有深色；云母在浅色下也有对应档位）
* 组件级测试（需要 jsdom + testing-library）
* Deezer 的艺人照片没接进 UI（provider 实现在，缺入口）
* 刮削没有并发，一次一首（MusicBrainz 本来就限每秒一次）
