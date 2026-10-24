# macOS 客户端状态与验证

原生客户端在 `macos/`，需要 macOS 14 或更新版本。这里区分原生界面已经提供的功能与 Rust 核心已经提供的接口；
核心里有一个方法，不代表 Mac 界面已经接上它。

状态只有三个值：**可用**（已有完整入口）、**部分**（有入口但能力受限）、**未实现**（没有对应界面）。
下表是修复后的目标状态，验证证据以对应提交的 `macOS client` job 为准。

| 功能 | 状态 | 有测试覆盖 | 备注 |
|---|---|---|---|
| 建库、打开已有库 | 可用 | AppModelTests；ImportTests 真 FFI 建库；smoke 打开示例库 | 默认目录可直接建库；选其他无 corpus.db 的目录须先确认；openExisting 不创建库 |
| 导入音频与歌词 | 可用 | ImportModelTests 路由/错误；SourceGuardTests；ImportTests；smoke 真写库 | 文件夹、单文件、多文件、混选统一 scanFiles；显示计划后导入，可停止；内嵌封面与 songs.csv 尚未接入 |
| Sudachi 分词 | 可用 | ImportTests 下载外的安装/补分词/KWIC 链路；ImportModelTests 词典选择路由 | 提供下载/从目录复制；真实网络中断和下载按钮交互没有自动化覆盖 |
| 歌词显示与管理（第 1 阶段） | 可用 | LyricEditTests（Windows 全部 6 例）、LyricsModelTests、LyricsBridgeTests、jp-ffi/jp-lyrics/jp-fonts | 时间戳解析、改动摘要、行后插入/删除、空行过滤；lrc/txt/yrc/qrc/krc/ttml 导入；校验问题、语言、分词器、字段来源、修订历史、版本原文与恢复；显示面板提供字体/备用字体、字体与 zip 导入、字号/注音字号/间距；Swift 待 macOS CI 验证 |
| 在线补歌词与时间歌词候选 | 可用 | LyricsModelTests、LyricsBridgeTests、Rust 离线队列与候选保护测试 | 单首在线补齐、批量缺失补齐、全库逐字升级、进度/取消/错误、全库待补名单；来源开关、候选精度与预览、采用失败清候选；Apple 地区与只写令牌（留空保留、显式清除、只显示已设置）；真实网络与 Swift 界面待另行验证 |
| 卡拉 OK 与打轴 | 可用 | KaraokeTests（Windows 全部 17 例）、LyricsModelTests、LyricsBridgeTests、Rust 波形/保存测试 | Karaoke.swift 逐函数移植；按结束时间换行、按有时间行序号交替、前奏歌名/署名与倒数；完整打轴快捷键、按住/点按共用撤销步骤、200 步历史、速度/提前量记忆、行状态、波形与边界拖动/选择/定位、拆并/清行、保存前检查与离开保护；Swift 编译与交互待 macOS CI 验证 |
| 播放 | 已实现，待 macOS CI 验证 | PlayerModelTests 注入引擎覆盖定位 / 音量 / 调音 / 循环 / 自动停 / 历史；smoke 增加 +2 半音、1.25× 位置推进 | AVAudioEngine 实时独立变速变调、48 条频谱、原生播放条与全屏歌词；详见 [macos-player.md](macos-player.md)；本机 Windows 仅能静态复查，CI 尚未运行 |
| 检索 | 可用 | CoreBridgeTests；smoke 检索截图 | 关键词、辞书形与写法、多词表达、附近词过滤 |
| KWIC 定位与试听 | 可用 | CoreBridgeTests；smoke 真窗口试听；LyricTimelineTests | 行级试听；歌词面板可按经过验证的真实单位起点播放，没有单位则回退到行 |
| 首页、曲库与详情 | 可用 | LibraryBridgeTests、smoke 截图 | 首页各分区、三栏曲库、歌曲/专辑/人物详情、收藏和快速搜索已接入 |
| 分析、分析存档、来源与语料质量 | 可用 | CoreBridgeTests、LibraryBridgeTests、smoke 分析截图 | 词频/搭配/对比/画像、保存/重跑/比较/删除分析、来源与质量面板已有原生入口 |
| 子语料 | 部分 | CoreBridgeTests 指定歌曲筛选 | 原生界面主要按歌手、年份选择；保存/管理完整条件的界面未接入 |
| 查词 | 可用 | ImportTests、MiningBridgeTests、jp-dict/jp-ffi 查询测试 | 原生纯文本释义、原文选择；读取同目录 dictionaries.db；查词弹窗本身未做完整交互测试 |
| 刮削 | 可用 | ScrapeBridgeTests、jp-ffi 离线记账测试、smoke 刮削页 | 曲目/歌手批量与单首、复核、补封面、取消；共享 jp-scrape，真实网络另行验证 |
| Yomitan 词典导入 | 可用 | ImportTests、jp-ffi 词典导入/启用/排序/删除测试 | 原生词典页支持 zip 导入与管理；独立于 Sudachi |
| Anki 一键制卡 | 可用 | MiningBridgeTests、jp-anki、jp-ffi 制卡准备测试 | 查词面板与设置已有入口，共用 jp_anki::lyric_mine；发送需要本机 AnkiConnect |
| 资料库关系网、时间轴、词云 | 未实现 | 无原生界面覆盖 | 现有首页和曲库详情已实现；这些可视化仍未接入 |
| 自动更新 | 未实现 | 无原生界面覆盖 | 从新源码重建；Windows 更新器不适用于此客户端 |

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
