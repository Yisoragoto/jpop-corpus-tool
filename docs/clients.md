# 一个核心，两个客户端

现在有两个界面：Windows 上的 Tauri + React（`rust/crates/jp-app` + `app/`），macOS 上的原生 SwiftUI（`macos/`）。
两边**不各写一套业务逻辑**：检索、分析、行区间、库结构和迁移全在 Rust 的 `jp-corpus` 里，
界面只负责「选范围、展示、播放」。

```text
                 jp-corpus（SQL、检索、分析、行区间、迁移）
                 jp-normalize
                 jp-import / jp-scrape / jp-anki / jp-lyrics（共享应用层）
                     │
        ┌────────────┴─────────────┐
   jp-app（Tauri command）      jp-ffi（C ABI，JSON 进出）
        │                          │  libjp_ffi.a
   app/（React）                macos/（SwiftUI + AVFoundation）
   jp-audio 播放（rodio）        AVPlayer 播放
```

## 为什么这样分

- `jp-corpus`、`jp-normalize` 本来就不依赖 Tauri（只有 `jp-app` 依赖），不需要大规模重构，
  加一个薄薄的 `jp-ffi` 就能给 Swift 用。
- **跨语言接口是「一个入口 + JSON」**（`jp_call(session, method, args_json)`），不是逐个函数导出 C 签名，
  也没用 UniFFI。方法名和参数名与 Tauri command 对齐，数据形状就是 serde 的 camelCase——
  Rust 结构体加一个字段，Tauri 前端和 Swift 自动都有，不会出现某一边的绑定忘了重新生成。
  C 层只有五个函数（`include/jp_ffi.h`），几乎不会变；新功能在 `jp-ffi` 的 dispatch 中分派（歌词入口在 `src/lyrics.rs`）。
  代价是每次调用多一次 JSON 编解码（微秒级），相对查询本身（毫秒到几十毫秒）可以忽略。
- **播放不走 Rust**。macOS 用 AVFoundation：原生解码 FLAC/MP3/AAC、系统输出设备、变速保持音高。
  但「从哪儿放到哪儿」由 Rust 决定（见下），两边定位行为一致。
- 库是同一种库：同一个 `corpus.db` 结构、同一套迁移（`jp_corpus::migrations`）。macOS 打开 Windows 拷来的库会补齐结构，
  比程序新的库会被拒绝，不会被改坏。默认目录是 `~/Library/Application Support/JPOP Corpus Tool`，
  对应 Windows 的 `%LOCALAPPDATA%\JPOP Corpus Tool`。

## 数据如何关联到音频

```text
songs(id, audio_path, duration_sec, year, album_id …)
  └ utterances(id, song_id, line_idx, time_sec, text)      一行歌词 = 一个 utterance
      └ tokens(utterance_id, token_idx, surface, lemma, pos)
```

每个检索、分析结果最终都落到 `utterance_id`（有它就有歌、行号、开始时间），再由 `jp_corpus::segments` 算出可播放的区间：

- 一行的结束 = 下一条**更晚**的时间戳；
- 离下一行太远（间奏）就按字数估：`3 + 0.25 × 字数` 秒，夹在 5–12 秒——阈值来自真实语料（211 首、8,377 对相邻行：
  行时长中位数 3.9 秒、p90 9 秒、p95 13.6 秒）；
- 最后一行用曲目时长兜底；起播提前 0.4 秒听得到起音。

**精度只到行**：LRC 只有行级时间戳，KWIC 定位到「这个词所在的那一行」，不假装能定位到词。
将来有词级时间（逐字 LRC、强制对齐）时，只需要在 `segments` 里加一层，接口不变。

两个客户端的「单句循环」「试听一句」「试听前后文」都用这套规则（Tauri 的单句循环原来在前端自己算，
间奏前的那一句会连着整段伴奏一起循环，已经改成调 Rust）。

## 分析的计数口径与出处

- **重复段落**：同一首歌里文本完全相同的行算「重复行」（和 KWIC 的「去重」同一规则）。
  真实语料里重复行占 23% 的行、20% 的词次。词频、搭配、对比都可以选「全部词次」或「重复行只算一次」，
  词频表两个数都给，外加「来自重复行的比例」。
- **子语料**（`jp_corpus::subcorpus`）：演唱者、年份、专辑、流派、指定歌曲；条件之间 AND、同一条件内 OR。
  KWIC、词频、搭配、对比共用这一个定义。
- **出处**（`analysis::Provenance`）：每个分析结果带库指纹（行数、词数、最大 id、校正数、元数据的哈希）、
  规则版本（`analysis-1`）、范围描述、计数单位和原样回显的参数。指纹与参数相同，结果就相同；
  导入新歌、校正分词后指纹会变。
- **存档与来源追踪**（`jp_corpus::runs`、`jp_corpus::lineage`，见 [provenance.md](provenance.md)）：
  出处回答「是不是现在的库算的」，存档回答「当时怎么来的、为什么变了」，可以把上面任何一种分析存档后原样重跑对比。
  子语料在 `Subcorpus` 上还多两个条件：歌词语言、歌词完整度下限。
  `jp-ffi` 有同名方法：`qualityOverview`、`songLineage`、`lyricVersionRaw`、`artistProvenance`、
  `subcorpusFacets` / `Preview` / `List` / `Save` / `Delete`、`analysisRun` / `List` / `Get` / `Rerun` / `Compare` / `Delete`；
  `wordInCorpus` 多一个 `scope`。会话打开时和 Tauri 一样给老歌补登记歌词版本。
- **首页与曲库**：`homeSummary`（同 Tauri 的 `home_summary`，多一个 `addedLimit`）、`albums`、`albumTracks`、
  `peopleDirectory`、`personById`、`personWorks`、`collaborators`、`isFavorite` / `toggleFavorite`（收藏的歌 `entityType` 是 `"song"`）、
  `quickSearch`。macOS 的首页、曲库三栏和详情页、⌘K 都只用这些，没有自己的查询。
- **词典管理与制卡**：`dictImport`（后台作业，`dictImportProgress` / `dictImportCancel` 轮询和停止）、`dictSetEnabled`、
  `dictSetOrder`、`dictDelete`；`ankiStatus`、`ankiMineCheck`、`ankiMine`。制卡的应用层（取例句、查词条）从
  `jp-app/src/mine.rs` 挪进了 `jp_anki::lyric_mine`，分 `prepare`（拿语料库的锁，很快）和 `send`（切音频、传 Anki，不拿锁）
  两步，两个客户端共用；macOS 打开 Windows 拷来的库时，例句音频按 `media::resolve` 找到本机的文件再切。
  链进 `jp-anki` 后静态库里多了 rodio（CoreAudio）、Rubber Band（C++）和 HTTP，Swift 包要链 libc++ 和几个系统框架（见 `macos/Package.swift`）。
- **刮削与振假名**：刮削落库的那一层（单首、批量、封面、歌手照片、补封面、复核采纳）从 `jp-app/src/scrape.rs` 挪成了新 crate `jp-scrape`，
  批量函数 `run_batch` / `run_artist_batch` 收「取消了没有」和「报进度」两个回调：Tauri 把进度发成事件，jp-ffi 存进作业等界面轮询。
  不放进 `jp-scraper`：那个 crate 不认识语料库、关掉 `network` 特性后不碰网络，保持这样。
  `jp-ffi` 的方法：`scrapeSummary`、`completeness`、`scrapeStart` / `scrapeArtistsStart` / `scrapeProgress` / `scrapeCancel`、
  `scrapeTrack`、`scrapeReviewQueue`、`scrapeAccept`、`scrapeSkip`、`scrapeRetryFailed`、`scrapeArtist`、`scrapeBackfillCovers`；
  联网的都单开一条连接，不占会话那条。振假名 `lyricsFurigana`（同 Tauri 的 `lyrics_furigana`，现场用 Sudachi 取读音、不入库）。
- 统计量：搭配有 logDice、MI、t、对数似然（2×2 表，期望值按实际窗口位置数算）；对比用带符号的对数似然和 log ratio（零频 0.5 平滑）。

## macOS 上建库：导入、分词词典、查词

macOS 客户端可以自己建库，不必从 Windows 拷：欢迎页「新建语料库」在 `~/Library/Application Support/JPOP Corpus Tool`
打开（不存在就新建）`corpus.db`，然后到「导入」页选音乐文件夹（音频 + 同名 `.lrc`，或内嵌歌词）→ 看计划 → 导入。

- **同一份代码**：扫描、计划、写库是 `jp-import`（`scan_targets` + `review` 原来写在 `jp-app` 里，挪进了 `jp-import`），
  分词是 `jp-tokenizer`，分词词典的定位 / 下载 / 校验 / 复制是新的 `jp-setup`（原 `jp-app/src/tokenizer.rs` 和 `net.rs`，
  `jp-app` 里仍叫 `crate::tokenizer`、`crate::net`），查词的默认值是 `jp_dict::lookup`（原 `jp-app/src/dict.rs`）。
  `jp-ffi` 的方法名和 Tauri command 一致：`scanFolder`、`scanFiles`、`runImport`、`cancelImport`、`tokenizerStatus`、
  `tokenizerDownload`、`tokenizerInstall`、`tokenize`、`tokenizeMissingList`、`tokenizeMissingRun`、`dictList`、`dictLookup`；
  另有 `importProgress`、`tokenizerProgress`（见下）和给不渲染 HTML 的界面用的 `dictLookupText`（释义转成纯文本）。
- **长作业不占会话的锁**：`runImport`、`tokenizerDownload` 启动一条线程立刻返回，线程**自己开一条 SQLite 连接**
  （库是 WAL，两条连接都设了 30 秒 busy_timeout），界面每 250ms 问一次进度（`jp_ffi::jobs::JobView`：
  `running / finished / cancelled / done / total / stage / result / error`）。选轮询不选 C 回调：
  不用管回调时 Swift 对象还在不在、在哪条线程上；代价是进度最多晚几百毫秒。
  导入、分词词典、查词、扫描这些方法在 Rust 里不拿会话的锁，Swift 侧也走另一条并发队列，导入跑着时检索照常。
- **分词词典**放在语料库目录的 `sudachi/`，和 Windows 版同一个下载地址、同一个哈希；装好后顺手给没分词的歌补上。
  没有词典也能导，计划上会说清楚「这批不会分词」，之后在导入页点「补上分词」。
- **查词**读语料库目录的 `dictionaries.db`（Yomitan 词典库）。macOS 词典页支持导入 Yomitan zip、启用、排序和删除；
  歌词面板右键一个词「查词」，可继续通过共享 `jp-anki` 一键制卡。

## 构建 macOS 客户端

```bash
./macos/scripts/build-rust.sh      # 构建 libjp_ffi.a（universal 参数出通用二进制）
cd macos && swift run JPCorpus      # 或 ./scripts/bundle.sh 打成 .app
swift test                          # 经真的静态库调 Rust，检查跨语言的数据形状和最小闭环
```

CI：`.github/workflows/macos.yml` 在 macOS runner 上跑 Rust 核心测试、`swift build`、`swift test`、回归探针、打 .app 与真窗口导入 smoke；失败日志与跳过的用例均可见。

## 验证与功能边界

原生客户端的完整能力矩阵和测试边界见 [macos-status.md](macos-status.md)。
应用、导入和播放器状态在可测试的 `JPCorpusModel` target，界面在 `JPCorpusApp`；
`JPCoreTests` 检查真 FFI，`JPCorpusModelTests` 检查模型与源码接线，SmokeRun 在真窗口里导入并刷新曲目表。
Swift 编译与运行由 CI 的 macOS job 验证，本机 Windows 不具备 Swift 验证环境。

## 原生功能与剩余边界

| 功能 | 状态 | 有测试覆盖 | 备注 |
|---|---|---|---|
| 首页、曲库详情 | 可用 | LibraryBridgeTests、smoke | 首页、专辑/人物/歌曲详情、收藏、快速搜索 |
| 刮削 | 可用 | ScrapeBridgeTests、Rust 离线记账测试、smoke | 单首/批量、封面、歌手、复核，共用 jp-scrape |
| Yomitan 词典导入 | 可用 | ImportTests、jp-ffi 词典管理测试 | 词典页导入 zip 并管理；Sudachi 安装是另一条链路 |
| Anki 一键制卡 | 可用 | MiningBridgeTests、jp-anki/jp-ffi 测试 | 查词面板与设置入口；发送需要本机 AnkiConnect |
| 资料库关系网、时间轴、词云 | 未实现 | 无原生界面覆盖 | 曲库与详情已有入口，这些可视化尚未接入 |
| 自动更新 | 未实现 | 无原生界面覆盖 | 从新源码重建 |
| 换歌词、逐行编辑、恢复旧版 | 可用 | LyricsModelTests、LyricsBridgeTests、jp-lyrics/jp-ffi 测试 | 详情、面板、全屏共用管理入口，支持 .lrc 导入、版本原文和恢复；Swift 新测试待 CI |
| 在线补齐、时间歌词搜索与升级 | 可用 | 同上，网络使用离线边界测试 | 单首/批量队列、进度和取消、来源开关、候选预览与采用 |
| 卡拉 OK 与打轴 | 部分 | LyricsModelTests、LyricsBridgeTests、Rust 时间轴测试 | 两行交替、倒数、真实时间高亮、按住/点按、起止编辑、拆分合并、撤销重做与版本保存；无波形拖拽和完整 Windows 打轴快捷键 |
| 分析存档、来源与语料质量 | 可用 | CoreBridgeTests、LibraryBridgeTests、smoke | 已有存档、重跑、比较、删除及来源/质量入口 |
| 完整子语料管理 | 部分 | CoreBridgeTests 基础筛选 | 保存/管理完整条件的原生界面仍未接入 |
| 导入内嵌封面、追加 metadata/songs.csv | 部分 | 导入主体有模型/真 FFI/smoke；这两项未覆盖 | 对应代码仍在 jp-app/src/scrape.rs 与 library_admin.rs |
| 词级定位、跨行/句法搭配 | 部分 | CoreBridgeTests、LyricTimelineTests | 歌词面板可定位真实时间单位；KWIC 仍按行，搭配仍按行内窗口，跨行/句法未接入 |

## 共享歌词业务层与 FFI（第 1 阶段）

`jp-lyrics` 承接原先在 `jp-app` 的歌词版本/逐行编辑、手动打轴、文件导入、来源设置、
候选搜索与采用、补齐/升级队列。它依赖 `jp-corpus`、`jp-import`、`jp-scraper`；
`jp-scraper` 继续只负责网络和格式，避免引入语料库依赖。
Tauri 原模块只做重导出和事件适配，`app/` 无需改动。

新增的 camelCase FFI 方法（参数与 Tauri command 对齐）：

- 编辑/版本：`lyricsSaveLines`、`lyricsRestoreVersion`、`lyricsImportFile`、`lyricsRevalidate`、`lyricsSourcesGet`、`lyricsSourcesSet`、`lyricsMissing`。
- 补齐：`lyricsFillStart`、`lyricsFillOne`、`lyricsFillCancel`、`lyricsFillRunning`。
- 时间歌词：`lyricsTimedTargets`、`lyricsTimedSearch`、`lyricsTimedApply`。
- 升级：`lyricsUpgradeStart`、`lyricsUpgradeCancel`、`lyricsUpgradeStatus`。
- 打轴/校准：`lyricsSaveTiming`、`wordTimings`、`wordTimeSet`、`wordTimeClear`。
- 作业轮询扩展：`lyricsFillProgress`、`lyricsUpgradeProgress`，返回同导入/刮削的 `JobView`，`result` 是详细升级统计。

补齐与升级共用一份作业，重复启动返回错误；取消不会清除已完成的歌曲。
FFI 的 Job 工作线程调用共享队列的同步 `run_batch`，独立连接开启 WAL、外键与 30 秒 busy timeout，
不占会话锁；Tauri 继续通过同一队列的异步 `enqueue` 发送原有两个进度事件，并保留追加导入目标的行为。
文件与数据库提交、版本归档和人工修改保护在共享核心中完成。
Swift 的新代码及测试需 macOS CI 编译执行；Windows 本地只验证 Rust 与前端回归。
