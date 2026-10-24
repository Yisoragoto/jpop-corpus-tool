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
| 歌词显示与管理（第 1 阶段） | 可用 | LyricsModelTests（FakeCorpus）、LyricsBridgeTests（真 FFI）、jp-ffi/jp-lyrics 测试 | 详情页/歌词面板/全屏歌词共用管理面板：逐行编辑、增删行、导入 .lrc、版本原文与恢复、重新校验；Swift 新增测试待 macOS CI 执行 |
| 在线补歌词与时间歌词候选 | 可用 | LyricsModelTests、LyricsBridgeTests、Rust 离线队列与候选保护测试 | 单首/批量缺失补齐、全库逐字升级、进度与取消、来源开关、逐行/逐字搜索预览并采用；网络覆盖依赖来源，不以离线测试宣称网络可用 |
| 卡拉 OK 与打轴 | 部分 | LyricsModelTests、LyricsBridgeTests、共享核心时间轴/保存测试 | 全屏两行交替与倒数、真实单位高亮；选单位、空格按住/点按、直接填时间、拆分/合并、撤销/重做、起点校准及清除、保存为新版本；尚无 Windows 的波形拖拽和完整打轴快捷键 |
| 播放 | 部分 | CoreBridgeTests 行区间；smoke 试听及行尾自动停 | AVFoundation、单句循环、倍速；smoke 不判断声卡是否实际有声，不覆盖全部控制按钮 |
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

FFI 的补齐和升级共用一个 `jobs::Job`，在工作线程同步调用共享 `run_batch`，队列自己开 SQLite 连接；
进度仍是 `running / finished / cancelled / done / total / stage / result / error`。
`lyricsFillProgress` 与 `lyricsUpgradeProgress` 是该作业的两个轮询入口；`lyricsUpgradeStatus` 维持 Tauri 的详细进度形状。
单首按钮也进入同一后台队列，离开面板继续运行，再次打开可恢复进度。

`LyricsModel` 负责可测试的草稿、选择完成/失败、候选选择、作业恢复、错误与打点状态，
JPCore 的 `LyricsClient.swift` 只封装方法和 Codable 形状。字符范围按 Unicode scalar（Rust `char`）计数。
来源提供的多字符单位不拆成伪时间；新切分的单位只有手工打点后才有时间。
本阶段 UI 的文件选择只开放 .lrc/纯文本，其他原生格式仍可经共享核心/FFI 导入。
Apple 目录凭据管理、波形编辑和完整 Windows 打轴快捷键暂未提供原生界面。
新增 Swift 代码已逐文件静态复查，**尚未在本机编译或执行测试**，以 macOS 14 CI 为最终证据。

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
