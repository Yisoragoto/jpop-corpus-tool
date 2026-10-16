# 语料质量、来源追踪与可复现分析

歌词会错：漏行、重复贴了一遍副歌、时间轴漂移、乱码、混进署名，甚至整份是另一首歌的中文翻译。
这些错会直接污染词频、共现和播放器同步，而一旦进了库，统计结果里是看不出来的。

这一层要回答四个问题：

1. **这份歌词哪来的、靠不靠得住？**（P0：质量与来源）
2. **分析结果是用哪一版歌词、哪个分词器、什么设置算出来的？**（P0：文本与词语数据模型）
3. **我统计的是哪一块语料？**（P1：子语料）
4. **三个月后再算一次，数字为什么不一样？**（P1：可复现分析）

代码在 `rust/crates/jp-corpus/src/` 的 `quality.rs`（校验）、`lineage.rs`（来源记录）、
`analysis.rs`（子语料与分析记录）；结构版本 `20261018`（`migrations.rs`）。

## 数据模型

```
songs ─┬─ lyric_versions（每挂一次歌词一版，原文 raw_text 一字不改）
       │      └─ utterances.version_id
       ├─ utterances（歌词行：time_confidence、token_status、tokenizer_run_id）
       │      └─ tokens（分词；校正只动这里，原文不动）
       ├─ field_provenance（每个元数据字段的来源和置信度）
       └─ corpus_revisions（修订历史，只追加；删歌也不删）

tokenizer_runs（引擎 + 词典 + 切分模式 + 配置 的一种组合一行）
subcorpora（存起来的筛选条件）
analysis_runs（一次分析的条件、范围、分词器、口径、结果）
```

**全部是加表、加列**，迁移不改不删任何已有数据（`steps_only_ever_add_things` 守着）。
老库升上来后，启动时 `lineage::backfill_legacy` 给已有歌词的歌各补登记一版
`legacy`：原文和分词器当时没有记录，照实留空，分析记录里会写「N 行不知道用的哪个分词器」。

### 歌词版本（`lyric_versions`）

| 列 | 说明 |
|---|---|
| `source_type` | `lrc_file`（导入时旁边的 .lrc）/ `embedded`（音频内嵌）/ `sibling`（后来出现在旁边的 .lrc）/ `online`（在线）/ `manual_file`（你选的文件）/ `restored`（从旧版恢复）/ `legacy`（老库补登记） |
| `source_name` / `source_ref` | provider 名或文件名 / 路径或在线条目「曲名 — 歌手」 |
| `raw_text` | 挂上时的**原文**（解析前）。重新分词、校正分词都不碰它；解析规则以后改了也能从它重来 |
| `content_hash` | 解析后「时间 + 文本」序列的指纹：同一份歌词重挂一次，指纹相同 |
| `completeness` / `validation_json` | 校验结果（见下） |
| `is_current` | 当前在用的那一版。换歌词 = 加一版、旧版置 0，**不删** |

旧版可以在「资料库 → 歌 → ⋯ → 来源与校验」里看原文、一键恢复。恢复也是挂一版新的（`restored`），
历史只追加，回到旧版不会抹掉中间那几版；校正过的分词按「歌手 + 曲名 + 原文」套回去。

### 每行的状态（`utterances` 新增的列）

| 列 | 取值 |
|---|---|
| `version_id` | 属于哪一版歌词 |
| `time_confidence` | 0~1，时间戳置信度；没时间轴是 NULL（规则见下） |
| `token_status` | `auto`（分词器给的）/ `corrected`（校正过，或换歌词后套回来的）/ `pending`（还没分词）/ `empty`（分过了，没有实词） |
| `tokenizer_run_id` | 用的哪个分词器（`tokenizer_runs`） |

### 分词器（`tokenizer_runs`）

`jp_tokenizer::Analyzer::info()` 在加载词典时读出：

- 引擎和版本（`sudachi.rs 0.6.11`，和 `Cargo.toml` 的 tag 由测试对齐）；
- `system.dic` 头里的描述和生成时间、文件大小（small / core / full 大小不同）；
- 切分模式（`C`）；
- `sudachi.json` / `char.def` / `unk.def` / `rewrite.def` 内容的指纹——`inputTextPlugin` 会改变分词结果；
- 词性映射表版本 `POS_MAP_VERSION`（**改了 `pos_map` 的映射就把它加一**）。

这些字段算出一个 `fingerprint`，一样的就是同一行。换了词典或配置，新分的行指向新的一行，
旧分词仍指着旧的——同一个库里新旧两套分词并存时，分析记录看得出来。

### 修订历史（`corpus_revisions`）

记的事：`lyrics_attached` / `lyrics_replaced` / `tokens_corrected` / `tokens_reverted` /
`corrections_restored` / `retokenized` / `metadata_edited` / `song_deleted` / `legacy_registered`。
`before_json` / `after_json` 存改动前后（分词校正存的是整行的分词），`actor` 是 `user` / `import` / `system`。
歌手、曲名另存一份：删了歌也看得出是哪首。

### 字段来源（`field_provenance`）

`(entity_type, entity_id, field)` → `source`、`source_ref`、`confidence`。

| 来源 | 谁写 | 置信度 |
|---|---|---|
| `tag` / `filename` / `folder` / `audio` | 导入（标签有就记 tag，没有是从文件名、目录猜的） | NULL |
| `itunes` / `musicbrainz` / … | 刮削补空字段时 | 自动采纳时的匹配分；你在复核里确认的是 NULL |
| `manual` | 你在「编辑信息」里改的、手动定的语言 | NULL |
| `heuristic` | 按歌词字符推断的语言 | 主导字符占比 |

**手改的不被覆盖**：语言推断看到 `manual` 就不动；刮削本来就只补空字段。

## 校验规则（`quality.rs`）

只检查、不改。结论写进版本的 `validation_json`，每行置信度写进 `utterances.time_confidence`。

| 代码 | 级别 | 含义 |
|---|---|---|
| `empty` | 错误 | 一行都没有 |
| `replacement_char` | 错误 | 有 U+FFFD 乱码（编码认错了） |
| `not_japanese` | 警告 | 主要语言不是日文（翻译或别的歌） |
| `too_few_lines` | 警告 | ≥ 120 秒的歌不到 8 行 |
| `ends_early` | 警告 | 最后一行在歌长一半之前 |
| `mixed_timing` | 警告 | 有的行有时间、有的没有 |
| `non_monotonic` | 警告 | 时间倒退（只报跳回去的那一行，后面的不连报） |
| `beyond_duration` | 警告 | 时间超过歌长 2 秒以上（漂移，或不是这一版录音） |
| `stacked_duplicate` | 警告 | 和上一行同时间同文本：重复贴了 |
| `leftover_markup` | 警告 | 正文里还留着 `[mm:ss]`、`<mm:ss>` 逐字时间或「作詞：」署名 |
| `untimed` | 提示 | 整份没时间轴（不影响语料完整度，只影响同步） |
| `duplicate_timestamp` | 提示 | 和上一行同时间、文本不同 |
| `long_gap` | 提示 | 和上一行隔了 30 秒以上（间奏，或漏行） |
| `very_long_line` | 提示 | 一行超过 80 字 |

**完整度**从 1 起扣：乱码 0.5、行数太少 0.4、提前结束 0.3、不是日文 0.2；
重复贴行 / 长空档 / 时间倒退每处 0.05，每类最多 0.2。它是估计，不是判决——
副歌连唱两遍不扣分（`repeated_line_ratio` 单独给出）。

**时间戳置信度**：没时间 NULL；有时间从 1.0 起，取最低的一条：倒退 0.2、超出歌长 0.1、
和上一行同时间 0.5、长空档之后 0.7、文件里有的行没时间再 × 0.8。

**语言**按字符种类粗分：假名占文字字符 ≥ 5% 就算日文（日文歌词夹英文很平常，
不含假名的「日文歌词」几乎不存在）；韩文字母过半是 ko；拉丁字母 ≥ 80% 是 en；
没有假名、汉字过半是 zh；其余 mixed。可以在「来源与校验」里手动改。

需要歌长的检查（`beyond_duration`、`ends_early`、`too_few_lines`）在补了时长之后才算得出来：
分析页「语料质量」里点「全部重新校验」。

## 子语料（`StatsFilter`）

统计页原来只有「演唱者」「仅日文」。现在同一个结构多了这些条件（都是「空 = 不限」，之间是「且」）：

| 条件 | 说明 |
|---|---|
| `yearFrom` / `yearTo` | 发行年份（`release_date` 优先，其次 `year` 的前四位）。**设了年份条件时，年份不明的歌不算进来** |
| `genres` | 流派，不分大小写 |
| `languages` | 歌词语言 |
| `albumIds` / `songIds` | 指定专辑、指定歌 |
| `minCompleteness` | 当前歌词版本的完整度下限；没有版本记录的歌在设了这个条件时不算进来 |
| `distinctLines` | 统计口径：同一首歌里文本相同的行只算第一次（副歌只算一遍） |

条件可以起名存成子语料（`subcorpora`），存的是整份条件（含演唱者）。
词频表、语料报告、分析存档都用这一个结构，所以「范围」在三处是同一个意思。

## 可复现分析（`analysis_runs`）

「分析 → 分析记录 → 存档报告 / 存档词频表」把当前这份统计连同下面这些一起存下来：

| 字段 | 内容 |
|---|---|
| `filter_json` / `params_json` | 筛选条件、分析种类和参数——重跑时原样用 |
| `scope_json` | 当时纳入的**每首歌和它的歌词版本 id**（及行数） |
| `tokenizers_json` | 这些歌词行用过的分词器，以及多少行不知道是哪个 |
| `method_json` | 统计口径，写成人能读的话：词元怎么取、排除哪些词性、TTR / STTR / 覆盖率怎么算 |
| `result_json` / `result_hash` | 结果和它的指纹 |
| `schema_version` / `app_version` / `created_at` | 库结构版本、程序版本、生成时间 |

范围、分词器、结果在**同一个读事务**里取，中间有写入也看不见，三者一定对得上。

**重跑**在现在的库上用同样的条件再算一次，另存一条（`rerun_of` 指回原来那条），并逐项对比：

- 条件、参数、口径、范围、分词器、库结构各自变没变；
- 范围上新纳入、少了、换了歌词版本的歌；
- 报告的每个数值指标前后（覆盖率按 N 展开）；
- 词频表上频次或名次变了的词（最多 200 条）；
- 一句句人话的原因。范围、分词器、口径都没变而结果变了，最可能是有人校正了分词——会直接这么说。

词频表同频次时按词元、词性排（以前只按频次，同频次的先后不保证），这样结果的指纹才稳定。

## 界面

- **分析页**：顶上「语料质量」卡片（平均完整度、需要看一眼的歌、时间戳不可靠的行、来源 / 语言 /
  分词状态 / 问题分布、用过的分词器）；筛选条里多了子语料、年份、流派、语言、完整度、「副歌只算一遍」；
  底部「分析记录」。
- **资料库 → 歌 → ⋯ → 来源与校验**：当前版的来源、完整度、问题清单、逐行状态汇总、分词器、语言（可改），
  全部版本（看原文、恢复），字段来源，修订历史。

## 已知缺口

- KWIC、词云、单词的「例句」页还不吃 `StatsFilter`，只有统计页和分析存档用子语料。
- 「从别的语料库目录迁移数据」（`jp-import/src/migrate.rs`）不搬来源表；搬过来的歌启动时补登记成 `legacy`。
- 歌词行的文本还不能在界面上直接改（只能换一整份歌词）；`corpus_revisions` 已经能记这类修订，
  将来加编辑时记成新的一版即可。
- 刮削还不写作词作曲等人物的来源置信度（`track_credits.source` 仍是原来那一套）。
