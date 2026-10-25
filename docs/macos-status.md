# macOS 客户端状态与验证

原生客户端在 `macos/`，需要 macOS 14 或更新版本。这里区分原生界面已经提供的功能与 Rust 核心已经提供的接口；
核心里有一个方法，不代表 Mac 界面已经接上它。

状态只有三个值：**可用**（已有完整入口）、**部分**（有入口但能力受限）、**未实现**（没有对应界面）。
下表是修复后的目标状态，验证证据以对应提交的 `macOS client` job 为准。

| 功能 | 状态 | 有测试覆盖 | 备注 |
|---|---|---|---|
| 建库、打开已有库 | 可用 | AppModelTests；ImportTests 真 FFI 建库；smoke 打开示例库 | 默认目录可直接建库；选其他无 corpus.db 的目录须先确认；openExisting 不创建库 |
| 导入音频与歌词 | 可用 | ImportModelTests 路由/错误；SourceGuardTests；ImportTests；smoke 真写库；jp-app fixture_flows | 文件夹、单文件、多文件、混选统一 scanFiles；显示计划后导入，可停止；共享内嵌封面保存与 songs.csv 同步 |
| Sudachi 分词 | 可用 | ImportTests 下载外的安装/补分词/KWIC 链路；ImportModelTests 词典选择路由 | 提供下载/从目录复制；真实网络中断和下载按钮交互没有自动化覆盖 |
| 歌词显示与管理（第 1 阶段） | 可用 | LyricEditTests（Windows 全部 6 例）、LyricsModelTests、LyricsBridgeTests、jp-ffi/jp-lyrics/jp-fonts | 时间戳解析、改动摘要、行后插入/删除、空行过滤；lrc/txt/yrc/qrc/krc/ttml 导入；校验问题、语言、分词器、字段来源、修订历史、版本原文与恢复；显示面板提供字体/备用字体、字体与 zip 导入、字号/注音字号/间距；Swift 待 macOS CI 验证 |
| 在线补歌词与时间歌词候选 | 可用 | LyricsModelTests、LyricsBridgeTests、Rust 离线队列与候选保护测试 | 单首在线补齐、批量缺失补齐、全库逐字升级、进度/取消/错误、全库待补名单；来源开关、候选精度与预览、采用失败清候选；Apple 地区与只写令牌（留空保留、显式清除、只显示已设置）；真实网络与 Swift 界面待另行验证 |
| 卡拉 OK 与打轴 | 可用 | KaraokeTests（Windows 全部 17 例）、LyricsModelTests、LyricsBridgeTests、Rust 波形/保存测试 | Karaoke.swift 逐函数移植；按结束时间换行、按有时间行序号交替、前奏歌名/署名与倒数；完整打轴快捷键、按住/点按共用撤销步骤、200 步历史、速度/提前量记忆、行状态、波形与边界拖动/选择/定位、拆并/清行、保存前检查与离开保护；Swift 编译与交互待 macOS CI 验证 |
| 播放 | 已实现，待 macOS CI 验证 | PlayerModelTests 注入引擎覆盖定位 / 音量 / 调音 / 循环 / 自动停 / 历史；smoke 增加 +2 半音、1.25× 位置推进 | AVAudioEngine 实时独立变速变调、48 条频谱、原生播放条与全屏歌词；详见 [macos-player.md](macos-player.md)；本机 Windows 仅能静态复查，CI 尚未运行 |
| 检索 | 可用 | CoreBridgeTests；smoke 检索截图 | 关键词、辞书形与写法、多词表达、附近词过滤 |
| KWIC 定位与试听 | 可用 | CoreBridgeTests；smoke 真窗口试听；LyricTimelineTests | 行级试听；歌词面板可按经过验证的真实单位起点播放，没有单位则回退到行 |
| 资料库管理（第 3 阶段） | 可用 | LibraryModelTests、MembersModelTests、LibraryAdminBridgeTests、TS 移植测试、jp-ffi、smoke 写库 | 代码已接入；新增 Swift 编译、测试和真窗口 smoke 待 macOS CI 验证，详见下节 |
| 首页、曲库与详情 | 可用 | LibraryBridgeTests、smoke 截图 | 首页各分区、三栏曲库、歌曲/专辑/人物详情、收藏和快速搜索已接入 |
| 分析、分析存档、来源与语料质量 | 可用 | CoreBridgeTests、LibraryBridgeTests、smoke 分析截图 | 词频/搭配/对比/画像、保存/重跑/比较/删除分析、来源与质量面板已有原生入口 |
| 子语料 | 可用 | SubcorpusScopeTests、VisualizationBridgeTests、Rust 完整条件往返 | 完整条件预览、同名覆盖、删除与跨检索/词云/例句的当前范围；新增 Swift 待 macOS CI |
| 查词 | 可用 | ImportTests、MiningBridgeTests、jp-dict/jp-ffi 查询测试 | 原生纯文本释义、原文选择；读取同目录 dictionaries.db；查词弹窗本身未做完整交互测试 |
| 刮削 | 可用 | ScrapeBridgeTests、jp-ffi 离线记账测试、smoke 刮削页 | 曲目/歌手批量与单首、复核、补封面、取消；共享 jp-scrape，真实网络另行验证 |
| Yomitan 词典导入 | 可用 | ImportTests、jp-ffi 词典导入/启用/排序/删除测试 | 原生词典页支持 zip 导入与管理；独立于 Sudachi |
| Anki 一键制卡 | 可用 | MiningBridgeTests、jp-anki、jp-ffi 制卡准备测试 | 查词面板与设置已有入口，共用 jp_anki::lyric_mine；发送需要本机 AnkiConnect |
| 资料库关系网、时间轴、词云 | 可用 | GraphTests、TimelineTests、WordCloudTests、SmokeRun 门禁 | 原生 Canvas/时间树入口与筛选、跳转已接；数值对账和真实窗口截图待 macOS CI |
| 自动更新 | 未实现 | 无原生界面覆盖 | 从新源码重建；Windows 更新器不适用于此客户端 |

## 可视化与其余界面

第 4 阶段差异核对：Windows `app/src` 为行为唯一依据；`app/` 不改。下表记录原基线与本次代码处理；新增 Swift 未在 Windows 编译或执行。

| Windows 组件/命令 | mac 基线 | 本次处理 |
|---|---|---|
| GraphView / RelationGraph / graph.ts；collaboration_edges | 无图页面与边 FFI | 移植建图、力导向、命中检测；Canvas、角色/合作数/局部筛选、拖动、缩放、人物跳转 |
| TimelineView / TimelineTree / timeline.ts / releaseAxis.ts；timeline_releases | 无时间轴与发行 FFI | 移植泳道、年份扩宽、封面排布、时间树与折叠空年；人物/歌曲跳转 |
| CloudView / WordCloud / wordCloud.ts；stats_frequency / lemma_forms | lemmaForms 已有，其余未接 | 移植合并、过滤、确定性螺线、命中分组；词性/年份/歌手/子语料、词例试听 |
| CommandPalette | QuickSearchView 仅搜索，无命令与上下选择 | 共用搜索浮层，支持 >、⌘P、禁用原因、键盘选择与过期响应保护 |
| CorpusCompare | 基础词频与年份对比 | 补副歌效应、参照歌手/保存范围、口径与来源、结果检索 |
| AnalyticsPage 的总览/年份统计/统计报告展示；timeline / stats_report | 只有分析词频/对比/存档/质量 | 补总览与年份图、报告正文和高频词检索；报告导出保留第 5 阶段 |
| PersonProfile / releaseAxis.ts / 发行分组纯函数 | 平铺作品与合作者，没有发行轴 | 补折叠空年/解散标记发行轴、三种可视化入口；移植发行分组、单曲/EP 类型与排序函数及全部 4 个原测试 |
| SubcorpusPicker / SubcorpusFilters / subcorpusScope.ts | 仅歌手与年份 | 完整条件保存/覆盖/删除/预览；检索、词云与例句共享当前范围 |
| PitchCacheCard / pitchCache.ts | AVAudioEngine 实时独立变调 | 无离线缓存生成/清理需求，按 macos-player.md 说明平台差异 |
| relativeTime.ts / listen.ts / backfill.ts | 原生调用链已有部分行为，纯函数未对齐 | 移植并接入显示、试听高亮与分词结果提示 |
| App.tsx 导航 | 首页、检索、曲库、分析、导入、刮削、词典；设置在系统菜单 | 曲库补关系网/时间轴/词云；侧栏补设置；Anki 浏览/更新按第 5 阶段保留缺口 |
| AnalysisRuns / QualityCard / SongProfile / SongLineagePanel | 已有存档、质量、画像、来源面板 | 补齐质量显示函数、分词器名单、指标名称及 corpusQuality.test.ts 全部断言；统计报告导出留第 5 阶段 |
| AudioRepair / NewSongsBanner / WatchedFolders / MigrateCard / MembersEditor / PerformerPicker / SongEditor / TokenCorrectionDialog | 第 3 阶段已有原生入口 | 编辑/删除统一边框与文字，反馈移至正文顶部；成员入口逐项照 PersonProfile 的 canEditMembers，另补 2 例规则回归 |
| LyricsEditor / LyricsTools / LyricsFill / LyricsUpgradePanel / TimedLyricsPanel / TimingEditor / KaraokeStage / LyricsStage / LanguagePicker / Player | 第 1–2 阶段已有原生入口 | 复核复用 |
| SudachiCard / SudachiBanner / TokenizeCard / SettingCard | 导入/设置/维护已有原生入口 | 复核复用，补 backfill 提示 |
| Avatar / Cover / Stat / Section / NavIcons / CommandButton / ColumnSplitter / VirtualList | SwiftUI 共用组件、原生分栏和惰性列表 | 沿用原生对应物 |
| MiningReportCard / Anki 浏览与更新 | 无完整原生入口 | 第 5 阶段，不在本次范围 |
| UpdateCard / UpdateDialog / DiagnosticsCard | 无更新与诊断页面 | 第 6 阶段，不在本次范围 |

Rust 新增 5 个 camelCase FFI 方法：`collaborationEdges`、`timelineReleases`、`statsFrequency`、`timeline`、`statsReport`。
前四个直接复用 jp-corpus；`StatsReport` 及其查询封装从 jp-app 移至 `jp_corpus::stats::stats_report`，
Tauri 重导出原类型、继续返回 `CmdResult<StatsReport>`，字段与文本不变。没有引入第二份 SQL 或业务计算。
子语料五个 FFI、lemmaForms、wordTimings/wordTimeSet/wordTimeClear 均复用已有接口。

移植到 JPCorpusModel 的 47 个 TS 函数/算法入口（含 Simulation 方法；seeded 对应 SeededRandom）：

| 来源 | 数量 | 入口 |
|---|---:|---|
| graph.ts | 6 | radiusFor、buildGraph、nodeAt、reheat、tick、settle；另保留 running、内部主角色/螺线初始化 |
| timeline.ts / releaseAxis.ts | 6 | yearValue、buildTimeline、warpYears、packRow、buildTree、buildAxisColumns；songsOf 内联复用 |
| wordCloud.ts | 10 | variantKey、mergeByLemma、isFunctionPos、isFunctionWord、filterCloudRows、seeded、fontSizeFor、layoutCloud、isSentenceStart、groupHitsBySong |
| subcorpusScope.ts / corpusQuality.ts | 7 | withScope、mergeSubcorpus、toSubcorpus（Swift 名 subcorpus）、normalizeFilter、sameFilter、isWholeCorpus、describeFilter |
| corpusQuality.ts 的质量与指标显示 | 5 | issueLabel、completenessLabel、percent、tokenizerLabel、metricLabel；baseMetricLabel 内联，问题代码表共用 |
| relativeTime.ts / listen.ts / backfill.ts | 3 | formatRelative、isPlayingSpan、backfillNote；audition/playFrom 复用已有 PlayerModel 调用链 |
| PersonProfile.tsx | 6 | albumKey、groupByAlbum、releaseTitle、releaseKind、splitReleases、allSongs |
| wordTiming.ts 的词首定位部分 | 4 | keywordSpan、validLyricTime、lyricEndBound、wordStart；完整单位时间轴继续使用 JPCore |

浏览器存储/订阅改为 UserDefaults 与 Observation，不复制 localStorage/React hook；当前范围删除、同名覆盖后刷新。
语言与完整度补入 JPCore.Subcorpus，保存整份 StatsFilter，包括歌手、年份、流派、语言、专辑、指定歌曲及两个计数口径。
画布只读取局部快照；关系网分桶力计算与词云螺线排布在 detached Task 中执行，取消/过期结果不覆盖当前视图。

新增 76 个 XCTest：Graph 7、Timeline/ReleaseAxis 12、WordCloud 22、InterfaceLogic 7、SubcorpusScope 7、CorpusQuality 4、
PersonReleases 5、WordStart 5、真实 FFI 4、PlayerModel 精确词首/行首提前量 1、人物成员入口 2。指定原 TS 测试的全部 46 例、人物发行 4 例和 corpusQuality 8 例（共 58 例输入/断言）已移植；
另有范围、边界及从 Windows TS 实际运行取出的图坐标/词云坐标对账断言。
同一字宽测量函数下断言同一布局；实际系统字体字宽由 AppKit 提供，像素与窗口交互仍需 Mac 验收。

本机验证：`cargo build -p jp-ffi -p jp-app` 成功；
`cargo test -p jp-ffi -p jp-app -p jp-corpus -p jp-library` 为 **368 passed、0 failed、52 ignored**（含 doc-test）。
其中 jp-app 147 passed/17 ignored、jp-ffi 32 passed、jp-corpus 185 passed/35 ignored、jp-library 3 passed，另 1 个 jp-corpus doc-test。
忽略项均为既有真实库、联网/设备用例。三条命令均设置 `CARGO_BUILD_JOBS=1`，本次没有并行启动大型构建。
`cargo clippy -p jp-ffi -p jp-app -p jp-corpus --all-targets` 成功，无警告；build/test 只有既有 MSVC linker_messages。
`app/` 未改，仍执行 `npm.cmd test -- --maxWorkers=1 --no-file-parallelism`：42 个文件、336 例全过（含上述 9 个文件、58 例）。
53 个改动 Swift 文件逐文件静态复查；tree-sitter 解析全部 123 个 Swift 文件无语法错误；SourceGuards 同规则静态检查（91 个源文件、98 个 View）与 git diff --check 通过。
这些检查不能替代 Swift 类型检查、XCTest 或截图；必须在 macos-14 / Xcode 15.4 / Swift 5.10 上确认。
重点编译风险为 SwiftUI 复杂 ViewBuilder/Chart、Canvas 文本测量、AppKit 后台字宽测量与 ImageRenderer 的 PNG 输出；
NSEvent 监视器通过 MainActor.assumeIsolated 进入主线程，离开页面时移除，没有使用 macOS 15 API。

本次续做复查修正：新页面改名 `ReleaseTimelineView`，避免遮蔽播放器/歌词舞台使用的 `SwiftUI.TimelineView`；
关系网与时间树使用数据修订号刷新，人数/发行数相同的换库或修改也会重算；过滤后的旧悬停索引先校验范围。
词云选词按完整合并结果刷新歌曲，取消的词首查询不覆盖新面板；无辞书也能看到语料例句，例句使用 Windows 的 1000 行上限与惰性布局。

逐文件复查清单（均检查 Swift 5.10 语法、跨模块 public/import、隔离闭包和 presentation 数量；不代表已做类型检查）：

| 文件（同目录省略路径前缀） | 静态复查重点 |
|---|---|
| JPCore：CorpusCore、LibraryModels、Models、VisualizationClient | 5 个新方法、既有范围接口；JSON 数值/元组/可选字段、角色数量及完整范围不丢失 |
| JPCorpusModel：AppModel、ImportModel、PlayerModel、LibraryIndex、VisualizationModel、SubcorpusScope | 偏好恢复、已有播放调用兼容、成员入口规则、后台任务取消/过期保护、同数量数据刷新 |
| JPCorpusModel：Graph、Timeline、WordCloud、InterfaceLogic、PersonReleases、WordStart、CorpusQuality | 逐函数对照 Windows 运算顺序、固定种子、Unicode scalar、排序、边界和全部对应测试输入 |
| JPCorpusApp：GraphView、TimelineView、CloudView、CloudSettings、CloudSongPanel、PersonReleaseAxis | Canvas 局部快照、后台布局、监视器卸载、类型名冲突、字体导入失败处理、词首/范围查询 |
| JPCorpusApp：AnalyticsOverview、RepeatEffectPane、SubcorpusViews、WordExamplesView、AnalysisView、SearchView | 全部筛选条件、保存/覆盖/删除、统计模型字段、惰性例句、过期请求与查询口径 |
| JPCorpusApp：Components、ContentView、JPCorpusApp、LibraryMaintenanceView、LibraryView、LookupPane、PersonAdminView、QualityLabels、QuickSearchView、SettingsView、SmokeRun、SongAdminView | 导航枚举穷尽、命令键盘选择、按钮/反馈位置、成员门槛、sheet/alert/fileImporter 每 View 各最多一个、三张截图与步骤日志 |
| Tests：VisualizationBridgeTests；CorpusQualityTests、GraphTests、InterfaceLogicTests、LibraryIndexTests、PersonReleasesTests、PlayerModelTests、SubcorpusScopeTests、TimelineTests、VisualizationFixtures、WordCloudTests、WordStartTests | 真实 FFI 解码、全部原 TS 断言与坐标对账、成员规则、测试 fixture 形状；仍待 XCTest 执行 |
| Rust：jp-corpus/stats.rs、jp-ffi/lib.rs、jp-app/commands.rs；macos.yml | 报告抽取只保留一份逻辑、Tauri 签名/JSON 不变；真实方法测试、三张图片与三条正数门禁 |

SmokeRun 从真实窗口进入三页，等待应用正在展示的 VisualizationModel 有内容后截图，macos.yml 强制检查：

```text
[step] graph nodes=N links=N status=pass
[step] timeline rows=N releases=N status=pass
[step] cloud words=N placed=N status=pass
```

对应 `04g-graph.png`、`04h-timeline.png`、`04i-cloud.png`；N 除 links 外必须大于零。
本机没有运行这三张截图。Anki 浏览/更新/MiningReportCard、报告导出留第 5 阶段；更新与诊断留第 6 阶段。
PitchCacheCard 的生成/清理与 pitchCache.ts 的磁盘任务状态不适用于实时变调，明确不新增无效入口，详见 [macos-player.md](macos-player.md#实现差异)。

## 资料库管理

第 3 阶段差异核对，基线 `a395fae`。Windows 源码是唯一行为权威，`app/` 不改。

| Windows command / 界面 | macOS 基线差异 | 本次完成与验证 |
|---|---|---|
| library_edit_song / SongEditor | 无入口 | 歌曲详情五字段编辑；共享事务、署名/专辑/CSV 同步；模型与真 FFI 读回、smoke |
| library_delete_song | 无入口 | 详情二次确认，停止当前播放并等待历史落库；保留音频/校正，清应用封面/缓存；FakeCorpus、PlayerModelTests、Rust 文件边界测试 |
| library_missing_audio / library_suggest_relinks / library_relink_audio / AudioRepair | 无入口 | 管理面板扫描、手选、路径预览后确认写入，手选不被建议覆盖，逐首保留失败；模型与真 FFI |
| library_new_audio / NewSongsBanner / WatchedFolders | 无入口 | 打开库自动扫描；曲库横幅及导入页/管理面板文件夹列表；20 首规则、复核、忽略/恢复、摘要与缺词补齐；18 个 TS 用例、持久化测试 |
| backfill_durations / fill_album_artwork | 无入口 | 维护页独立连接 jobs，分类报告/失败样本；只用库里已有曲目封面；Rust 锁内等待测试、真 FFI 报告解码 |
| artist_image_set / artist_image_reset / PersonProfile | jp-scrape 已有，界面未接 | 人物页本地替换/恢复，更新照片缓存；共享人工来源保护；Swift 真 FFI、jp-scrape 回归 |
| membership_suggestions / membership_confirm / membership_reject / membership_put / memberships / band_members / bands_of / MembersEditor | jp-corpus 已有，FFI/界面未接 | 人物页成员增改、建议确认/否定、移除二次确认、日期校验、结果提示；MembersModelTests 与真 FFI |
| token_correction / save_token_correction / revert_token_correction / TokenCorrectionDialog | jp-corpus 已有，FFI/界面未接 | 歌词行右键入口，三字段/UPOS、相邻合并/码点拆分、再次点击撤销，保存后重载歌词和检索；9 个 TS 用例、模型与真 FFI |
| artist_roster / people_by_role / works_by_person / PerformerPicker | 仅 performers（500）和 personWorks | 刮削页名单和单人入口；搜索/分析演唱者多选含照片，按 Windows 上限 5000；名单随资料库变化重拉；同名查询均可调用 |
| most_played / recently_played | 首页仅最近 12 首 | 管理面板收听页；独立参数接口和真 FFI 播放记录读回 |
| lemma_forms | 缺 FFI | 同名接口/模型已接，空输入与无词典测试；调用方词云仍属后续可视化 |
| song_set_language | songSetLanguage、LyricsModel 和语言菜单已有 | 已核对 LyricsManagementView → LyricsModel → songSetLanguage；真 FFI 写入来源及语言测试 |
| library_root / set_library_root | AppModel/Settings 用 UserDefaults 选库并打开 | 沿用原生选库/建库确认、Application Support 和 audioRoots 映射；不新增 Windows 安装路径设置 FFI |
| migrate_plan / migrate_run / MigrateCard | jp-import 仅搬数据库行 | 维护页预览/确认/两选项/jobs/报告；共享文件复制、Sudachi 和补分词；Rust 非空迁移、文件/id/源库/幂等与不占锁测试 |
| libraryAdmin.ts / newSongs.ts / corrections.ts / avatarIdentity.ts / libraryIndex.ts | 未移植 | 35 个公开纯函数及其内部辅助逻辑移植；原 38 个 TS 用例全部移植，另补辅助函数/状态测试；存储订阅改为 Observation/UserDefaults |
| LibraryBrowser / PersonProfile 作品归属、成员层级、幕后角色筛选 | 平铺目录及角色筛选 | 演唱署名归属，音乐人/幕后、成员命中自动展开、角色筛选和个人/乐队/合作分区；10 个 TS 用例及辅助用例 |
| 额外：内嵌封面、metadata/songs.csv | 导入未接 | jp-scrape::save_imported_covers 与 jp-library::admin::sync_imported 两端共用；Swift 显示 CSV 错误；Rust CSV/封面边界与现有导入回归 |
| 额外：关系网/时间轴/词云 | 第 3 阶段未实现 | 第 4 阶段已补，见上节；不宣称整个客户端等价 |

共享 crate `jp-library` 承接曲库编辑/删除/重新关联的事务与文件收尾、发现新歌、时长回填、迁移文件和成员输入/歌手名录适配。
这些是同时依赖语料库、导入器与本地文件的应用层，放进纯网络/格式层 `jp-scraper` 或通用查询层 `jp-corpus` 都不合适。
Tauri 保留播放停止、历史刷新、事件/锁适配；原 command 签名和返回 JSON 不变，`app/` 未改。
发现与配对扫描在 Tauri 也使用独立只读连接，避免抽取后把慢文件扫描包进会话锁。

新增 camelCase FFI：`libraryEditSong`、`libraryDeleteSong`、`libraryMissingAudio`、`libraryNewAudio`、
`librarySuggestRelinks`、`libraryRelinkAudio`、`backfillDurations`、`fillAlbumArtwork`、`libraryProgress`、
`artistImageSet`、`artistImageReset`、`artistRoster`、`membershipSuggestions`、`membershipConfirm`、
`membershipReject`、`membershipPut`、`memberships`、`bandMembers`、`bandsOf`、`tokenCorrection`、
`saveTokenCorrection`、`revertTokenCorrection`、`peopleByRole`、`worksByPerson`、`mostPlayed`、
`recentlyPlayed`、`lemmaForms`、`migratePlan`、`migrateRun`（29 个）。
`songSetLanguage` 和 `scrapeArtist` 是既有 FFI，本次分别复核接线、补 Swift 包装与名单入口。
长作业共享 library_job 槽，自己开 SQLite 连接，返回现有 JobView；不阻塞会话查询。

测试：新增 Swift 真 FFI 6 例、LibraryModel/MembersModel 14 例、播放器删除 1 例；
TS 移植为 corrections 9、newSongs 18、avatarIdentity 1、libraryIndex 10，共 38 例，另有辅助/描述测试。
本机已执行指定 Rust build/test/clippy；新增 Swift 逐文件复核 public/import/@MainActor、JSON 字段和 presentation 限制。
Rust 共 195 passed、0 failed、17 ignored；忽略项是既有的真实库/联网/音频输出设备用例。
各套件原文摘要：jp-app lib `61 passed; 0 failed`，fixture_commands `62 passed; 0 failed; 5 ignored`，
fixture_flows `16 passed; 0 failed; 2 ignored`，其余格式/主线程/schema 共 8 passed，真实资源套件 `0 passed; 0 failed; 10 ignored`；
jp-ffi `30 passed; 0 failed; 0 ignored`，jp-library `3 passed; 0 failed; 0 ignored`，jp-scrape `15 passed; 0 failed; 0 ignored`。
build/clippy 均 exit 0；build 仅有 MSVC 创建导入库的 linker_messages，clippy 无新增警告。
Windows 无法编译 macOS SwiftUI/AVFoundation，Swift 测试与窗口交互仍须 macos-14 / Xcode 15.4 CI 执行，不能把静态复查当成已运行。
SmokeRun 用显示在应用中的 LibraryModel 编辑标题后从真 FFI 读回，再恢复标题；CI 强制检查：

```text
[step] library-edit readback=true status=pass
```

## 歌词管理第 1 阶段

业务层从 `jp-app/src/lyrics.rs`、`timed_lyrics.rs`、`lyric_upgrade.rs` 抽到 `jp-lyrics`，
Tauri 的 command 签名、JSON 返回与事件名保持不变；版本/原文归档、文件与事务一致性、
人工修改保护、候选过期和并发校验均由同一份 Rust 代码负责。
新 crate 是依赖语料库与导入器的应用层，不能放进不认识语料库的 `jp-scraper`。

FFI 的批量补齐和升级共用一个 `jobs::Job`，在工作线程同步调用共享 `run_batch`，队列自己开 SQLite 连接；
进度仍是 `running / finished / cancelled / done / total / stage / result / error`。
`lyricsFillProgress` 与 `lyricsUpgradeProgress` 是该作业的两个轮询入口；`lyricsUpgradeStatus` 维持 Tauri 的详细进度形状。
单首在线补齐使用与 Windows 相同的 `lyricsFillOne`；批量任务离开面板继续运行，再次打开可恢复进度。

`LyricsModel` 负责可测试的草稿、选择完成/失败、候选选择、作业恢复、错误与打点状态，
JPCore 的 `LyricsClient.swift` 只封装方法和 Codable 形状。字符范围按 Unicode scalar（Rust `char`）计数。
来源提供的多字符单位不拆成伪时间；新切分的单位只有手工打点后才有时间。
字符单位已核对 `jp-corpus` 的 word_timings、`jp-lyrics::save_timed_lines` 和 Windows `karaoke.ts`：
Rust `chars()` 与 JS `[...text]` 都按码点计数；Swift 使用 Unicode scalar，同样把 `𠮷` 算一个，不使用 UTF-16 下标或字素簇下标。
`Karaoke.swift` 的 19 个公开函数及 MIN_UNIT_SEC / COUNTDOWN_GAP / INTRO_LEAD 常量逐项对应 Windows；
`LyricEdit.swift` 移植 formatStamp、parseStamp、summarizeEdit，全部 23 个原始用例的输入和断言值保持一致。
新增 `audioPeaks(songId, perSec)` 复用 jp_audio::decode_all / decode::peaks，每秒格数钳在 1–1000，Swift 请求 100。
FFI 此前已通过 jp-anki 引入同一份 jp-audio 和 CoreAudio/Rubber Band 链接配置，本轮没有引入新的设备依赖，未另拆 feature。
新增 `songSetLanguage`、`fontsCatalog`、`fontsImportArchive`；字体解析和压缩包导入原样抽到 jp-fonts，两端共用。
Apple SettingsInput 与 SettingsView 分开建模，不把只读的 `*TokenSet` 字段编码进输入；令牌不落盘、不回显，失败用固定文案。
jp-lyrics 的 warn/error 钩子接入 Tauri 日志，panic 日志恢复；FFI 默认 stderr。
来源配置统一从 session.library_root() 加载。
新增 Swift 代码已逐文件静态复查，**尚未在本机编译或执行测试**，以 macOS 14 CI 为最终证据。
表中的「可用」表示代码已有完整功能入口，不表示这次新增 Swift 已通过 CI；原生字体枚举使用 CoreText/AppKit，
字体选择结果与系统字体集合有关，窗口排版和系统文件选择面板的交互仍需 Mac 实机确认。

## 本次修复与任务书评审

issue [#15](https://github.com/Yisoragoto/jpop-corpus-tool/issues/15) 报告 v0.3.2 的导入入口无响应。
多个 `.fileImporter` 改为一个 presentation、用枚举记录意图；完成回调处理失败与未打开库，
设置里的音频目录明确用于路径映射，并提供「去导入」。选择无库目录时先确认，避免无意建出空库。

任务书中的几处推断需要保留边界：

- issue 将多个 importer 冲突列为假设。源码确认存在多个 presentation；源码守卫能阻止这种接法再次进入，
  **不会证明每个 macOS 版本的系统文件面板都能弹出**。本次按要求不引入 XCUITest 或坐标点击。
- Rust 的 `scanFolder` 实际也接受单文件。统一 `scanFiles` 消除数量分支、明确混选语义；
  方法选择由 FakeCorpus 断言，真 FFI smoke 证明单文件能生成计划，二者作用不同。
- 任务书示例在 `isPresented` 变为 false 时就清空 `pick`，可能早于 completion，导致意图丢失。
  `FilePickState` 将「是否展示」与「此次用途」分开，completion 的 `finish()` 才清空用途。
- 普通 `.sheet` 的内容闭包没有 `Result`。守卫按每个 View 限制 fileImporter/sheet/alert 各最多一个；
  显式 `.failure` 处理规则只用于 fileImporter。它是小型词法检查，不是 Swift 语义分析器。
- 模型留在独立 `JPCorpusModel` 库，视图跨模块实际需要的成员才导出。`CorpusClient` 的已有签名与
  UserDefaults 的 `libraryFolder`、`audioRoots` 键保持不变。范围按任务书文档要求包括 README 与 issue 模板。

## 三层验证

1. **真核心桥接**：保留 `JPCoreTests` 的既有断言，检查 JSON、建库、导入、补分词与 KWIC。
   `JP_SUDACHI_SOURCE` 未设置时的分词跳过，会在 CI 的 `Report skipped Swift tests` 步骤列出用例名。
2. **模型与源码守卫**：`JPCorpusModelTests` 使用 FakeCorpus 记录实际方法和参数，注入扫描与面板错误，
   验证无库提示、建库确认及面板关闭时意图不丢失。源码守卫按 View 的花括号范围检查 presentation 数量，
   并包含故意违规的字符串自测。
3. **真窗口 smoke**：使用显示在 ImportView 上的同一个 ImportModel，走单文件选择完成回调、文件夹扫描、
   真写库与曲目表刷新，保存计划和导入后窗口截图。系统选取面板本身不在 smoke 自动操作范围内。

`.github/workflows/macos.yml` 在 macOS runner 上执行 `swift build`、`swift test`、
`scripts/check-regressions.py` 和 `.app` smoke。**Windows 本地无法编译或运行 Swift，这些项目由 CI 的 macOS job 验证。**
回归探针临时恢复七种错误，要求出现指定 XCTest 的断言失败；编译错误、跳过或基础设施失败不算有效证据。
每次探针用 `finally` 还原源码，最后再跑通过模型测试。原始输出随 `macOS-test-evidence` artifact 保留。

smoke 在每一步即时写出 `smoke.txt`，即使崩溃或超时也留下诊断；失败会打印到 CI 日志。
CI 同时检查这些行和导入后的截图，任一缺失即失败：

```text
[step] single-file plan.total=1 status=pass
[step] folder plan.new=2 status=pass
[step] import tracks.before=4 tracks.after=6 added=2 status=pass
[step] complete status=pass
```

smoke 使用独立的 UserDefaults suite 和临时示例库，不读取用户上次的库或保存音频目录设置。
本地有 Mac 时，可先准备目录 `<audio>/import/`（至少一首唯一命名的 WAV 及同名 LRC），
播放用的 `001.flac`～`004.flac` 放在 `<audio>/`，再运行：

```bash
JPCORPUS_SMOKE=/tmp/jpcorpus-smoke JPCORPUS_SMOKE_AUDIO=/tmp/jpcorpus-audio \
  'macos/build/JPOP Corpus.app/Contents/MacOS/JPCorpus'
```

## 报告问题

使用仓库的 macOS bug 模板，提供应用版本、关于页 build id（没有时提供构建 commit SHA）、
macOS 版本和芯片，以及 smoke 或日志片段。源码构建可用 `git rev-parse HEAD` 取得 SHA。
日志分享前移除私人路径和不想公开的歌曲/歌词内容。
