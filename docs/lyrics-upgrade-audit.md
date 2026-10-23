# 歌词升级：代码审查与实施清单

2026-10-22，以实际源码为准；不改变目录结构，不引入 AI 模型。

## 原有数据流与缺陷

- `jp-import/src/lrc.rs::parse` → `import.rs::attach_lines` → `word_timings`：Enhanced LRC 保存字符区间；原先丢失显式片段结束时间，休止会被拉长。
- `app/src/wordTiming.ts::charTimeline` 在缺少真实时间时，使用 `beats`、行起止和字符数量生成时间，且在真实的多字符片段内继续插值。`LibraryPage.tsx` → `useKaraoke.ts` 将它作为扫描高亮。必须删除这条估算路径。
- `wordStart` 同样会估计关键词的播放起点，需要改为真实单位起点或整行起点。
- `macos/Sources/JPCorpusApp/LyricsPanel.swift` 当前只有整行高亮；`JPCore` 的歌词模型没有时间单位，需补充共享 Rust 时间轴数据。纯文本不得获得播放高亮。
- `jp-scraper` 已有 `MetadataProvider`、多源候选评分和去重；复用这套结构，不再另建一套 resolver。`jp-corpus::lineage` 已有歌词版本、字段来源与修订记录。
- `jp-app/src/lyrics.rs` 是歌词替换/版本恢复入口。必须同时保护手动时间校准、旧版本原文和文件/数据库提交的一致性。

## 文件级实施顺序

1. `wordTiming.ts`、`useKaraoke.ts`、`LibraryPage.tsx`、`SettingsPage.tsx`、对应测试：逐行/纯文本回退，真实单位不拆分成伪时间。
2. `jp-scraper/timed_lyrics.rs`、`jp-import/lrc.rs`、`jp-corpus/word_timing.rs`：统一时间单位、显式结束、Unicode 字符映射和验证；保持原文和 Sudachi 分词独立。
3. `jp-scraper/timed_sources.rs`、`jp-app/timed_lyrics.rs`、commands/API、`TimedLyricsPanel.tsx`：多源只读搜索、候选预览、人工采用、过期/并发防护。核实 LRCLIB 的真实字段，不能将逐行 LRC 宣称为逐字。
4. 现有 providers/matching/store：Apple 官方元数据可选接入，版本冲突阻止误绑定，保留字段来源；不使用 Apple 私有歌词端点。AI 对齐只预留数据身份，不加载模型。
5. Swift 模型/桥接/歌词视图、合成库测试、文档：跨客户端保持同一精度规则。当前机器为 Windows，macOS 编译需明确单列验证限制。

## 外部能力约束

- Apple Music 官方 API 提供目录、ISRC、封面及 `hasLyrics`；文档未提供歌词正文或 syllable-lyrics 端点。开发者 token 不能被描述为购买逐字歌词能力。
- LRCLIB 官方字段是 `plainLyrics`、`syncedLyrics` 等；若实际正文仅有行时间，只作为逐行来源，不做字符分配。
- 网易、QQ、酷狗的网络端点会变化；失败须明确提示。KRC/QRC 解码也可用于用户选择的本地文件。网络读取能力不代表再分发许可。
