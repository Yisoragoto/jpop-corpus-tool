# 一个核心，两个客户端

现在有两个界面：Windows 上的 Tauri + React（`rust/crates/jp-app` + `app/`），macOS 上的原生 SwiftUI（`macos/`）。
两边**不各写一套业务逻辑**：检索、分析、行区间、库结构和迁移全在 Rust 的 `jp-corpus` 里，
界面只负责「选范围、展示、播放」。

```text
                 jp-corpus（SQL、检索、分析、行区间、迁移）
                 jp-normalize
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
  C 层只有五个函数（`include/jp_ffi.h`），几乎不会变；新功能只在 `jp-ffi/src/lib.rs` 的 `dispatch` 里加一个分支。
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
- 统计量：搭配有 logDice、MI、t、对数似然（2×2 表，期望值按实际窗口位置数算）；对比用带符号的对数似然和 log ratio（零频 0.5 平滑）。

## 构建 macOS 客户端

```bash
./macos/scripts/build-rust.sh      # 构建 libjp_ffi.a（universal 参数出通用二进制）
cd macos && swift run JPCorpus      # 或 ./scripts/bundle.sh 打成 .app
swift test                          # 经真的静态库调 Rust，检查跨语言的数据形状和最小闭环
```

CI：`.github/workflows/macos.yml` 在 macOS runner 上跑 Rust 核心测试、`swift build`、`swift test`、打 .app。

## 还没做的

- macOS 上的导入、分词、刮削、查词、Anki 制卡（核心在 `jp-import` / `jp-tokenizer` / `jp-dict` / `jp-anki`，
  都不依赖 Tauri，下一步按需要加进 `jp-ffi`）。现在 macOS 客户端打开已有的库。
- 词级时间戳。
- 搭配目前只在行内开窗；跨行、按句法依存的搭配没做。
