# Anki 导出（Rust 侧）

把语料里的词做成 Anki 卡片，例句取自用户自己听的歌。
对应 `rust/crates/jp-anki`，Tauri 入口是 `anki_*` 一族 command，界面在「Anki」页。

架构图上那个 Anki 盒子。两个方向：

| 方向 | 途径 | 模块 |
|---|---|---|
| 语料 → Anki | AnkiConnect HTTP（`127.0.0.1:8765`） | `connect` `model` `card` `export` |
| Anki → 语料 | 直接读 `collection.anki2` | `learning` |

**除了 AnkiConnect 本身，全程不联网。** 释义、音高、词频、JLPT 都来自
`corpus.db` 里现成的词典表，例句来自语料本身。

## 分层

| 模块 | 职责 | 能离线测试 |
|---|---|---|
| `connect` | AnkiConnect v6 客户端、错误分类与处置建议 | ✅（`FakeAnki`） |
| `model` | 「JPOP Corpus」笔记类型：字段、正反面模板、CSS | ✅ |
| `dict` | 本地词典查询（释义 / 读音 / 音高 / 词频 / JLPT） | ✅ |
| `card` | 组卡：例句挑选、高亮、HTML 生成 | ✅ |
| `export` | 选词、新建 / 追加、结果分类 | ✅ |
| `learning` | 读 Anki collection，得到「已加入 / 已复习」 | ✅ |
| `mining_report` | 挖词报告：按歌手、歌曲、JLPT 统计已挖的词，写成 HTML | ✅ |

## 笔记类型

`NOTE_TYPE = "JPOP Corpus"`，10 个字段，**顺序固定**——Anki 用第一个字段做查重，
所以 `Expression` 必须排第一。正反面模板和 CSS 从 PyQt 版逐字搬过来，
这样两边导出的卡片在同一个牌组里长得一样。

`model::ensure()` 是**建或补**：类型不存在就建；存在但少字段就加字段。
**从不删字段**——用户可能自己加过东西。

## 例句

`card::examples()` 从语料里挑句子。和 Python 版的差别只有一处：

- Python：`ORDER BY RANDOM()`
- Rust：确定性排序，优先选来自**不同歌**的句子

改这个是因为随机顺序没法测试，而且同一首歌连出两句对记忆没有帮助。
默认每张卡 2 句（界面可调 1–8）。

## 释义

`dict_terms.defs_json` 里混着两种形状：

```json
{"html": "<div class='ym-item'>…</div>"}    // 已经渲染好，不能再转义
"跑，快跑，奔跑。"                            // 纯文本，必须转义
```

`Definition { source, text, is_html }` 把这两种分开记，`meaning_html()`
按 `is_html` 决定要不要 `escape`。混为一谈的后果是标签被转义成字面文字，
或者纯文本里的 `<` 把卡片打烂。

**默认不限制取几部词典**（和 PyQt 一致）。但真实数据实测：全上的话，
一个常用词的释义能到 16,932 字符，卡片背面根本没法看。所以界面默认取 3 部，
顺序就是用户在词典管理里排的优先级。

## 学习状态

`learning::load()` **不打开用户正在用的 collection**。先 `snapshot()`：
把 `collection.anki2` 连同 `-wal` / `-shm` 复制到临时目录，再从副本读。
Anki 开着的时候直接读原文件会读到不一致的快照，甚至可能损坏。

collation 那边 Anki 用的是 `unicase`，rusqlite 需要开 `collation` feature
自己注册一个，否则查询直接报错。

**JPOP Corpus 和 Lyrics 两种笔记都算。** 起初只认 JPOP Corpus；2026-09-17 再看用户真实的 collection，
JPOP Corpus 是 0 张，卡都是一键制卡做的 Lyrics（两种卡第一个字段都是 Expression）。
只认一种的话「已进 Anki 的词」是 0，选词时「藏起已学过的」也不起作用。
Lapis 笔记（用户用 Yomitan 挖的一般词汇）不算。

「已加入」和「已复习」是两回事：加进去没看过不算学过。界面上的
「漏掉已学过的」勾的是后者。

牌组名在库里存的是 `\x1f` 分隔（Anki 2.1.28 起），不是 `::`。AnkiConnect 和
Anki 界面给的都是 `::`，所以读出来要转一下——**Python 版没转**，这是拿真实
collection 对账时才发现的：`\x1f` 打印出来是隐形的，「JPOP::ヨルシカ」看着
就是「JPOPヨルシカ」，而且 `deck_filter` 传用户照界面抄的 `::` 形式时永远匹配不上。
这是刻意与 Python 不一致的第二处。

## 和 Python 的对账

当时的真实 collection（233 MB，1046 张 JPOP Corpus 笔记；现在这些卡已经不在了）：

| 项 | Python | Rust |
|---|---|---|
| 已加入 | 1045 词 | 1045 词 |
| 已复习 | 921 词 | 921 词 |
| 逐词四个字段（`note_count` / `card_count` / `max_reps` / `studied`） | — | 1045/1045 相同 |
| 牌组名 | 带 `\x1f` | 转成 `::`（**有意不同**） |
| 读取耗时 | — | 174 ms（含复制快照） |

`note_count` 合计 1046，和 AnkiConnect 报的笔记数对上。比 1045 多一个是因为
「木漏れ日」有两张笔记（分在两个牌组），不是查重漏了。

## 导出

`export_word()` 四种结果：

| 结果 | 含义 |
|---|---|
| `Added` | 新建了一张卡 |
| `Updated { new_sentences }` | 卡已存在，**追加**了例句 |
| `AlreadyComplete` | 卡已存在，没有新例句可加 |
| `Skipped` | 卡已存在，设置为「跳过」 |
| `NoExamples` | 语料里没有这个词的例句，不做卡 |
| `Failed { error }` | 这个词失败了，继续下一个 |

`merge_into_existing()` **只追加，从不替换**。用户可能已经手工改过卡面，
覆盖掉是不可接受的。

Anki 中途关掉是另一回事：那不是「这个词失败了」，是整批做不下去了。
`spawn_export` 遇到 `AnkiError` 会**中断整批**并把 `aborted` 填上处置建议，
而不是让 500 个词各报一次同样的错。

## 连不上不是错误

`anki_status` 在 Anki 没开的时候照样返回，`connected: false` 加一句处置建议。
界面显示指引而不是红色报错，而且**选词和预览照常可用**——那两件事只读本地库。

## Tauri command

| command | 作用 |
|---|---|
| `anki_status` | 连接情况、牌组列表、笔记类型在不在、学习状态 |
| `anki_words` | 候选词表，按语料频次排，标出已有 / 已学 |
| `anki_preview` | 组一张卡看看，**不碰 Anki** |
| `anki_ensure_note_type` | 建或补笔记类型 |
| `anki_export_start` | 后台批量导出，进度走 `anki://progress` |
| `anki_cancel` | 中断 |
| `anki_is_running` | 有没有作业在跑 |
| `anki_audio_available` | 本机有没有 ffmpeg（没有时「音频片段」禁用） |
| `anki_update_start` | 更新选中的词，只重查词典字段，后台跑 |
| `anki_refresh_preview` | 刷新范围内有几张旧卡。只读 |
| `anki_refresh_start` | 刷新旧牌组，后台跑 |
| `anki_mining_report` | 挖词报告：写 `output/corpus_report.html` 并用浏览器打开 |

导出作业和刮削作业**不共用标志**：刮削排的是外部服务的限流队列，
Anki 是本机，两件事同时做没有冲突。

## 释义渲染：和 Python 逐字一致

「刷新旧牌组」会重写用户已有卡片的 Meaning。Rust 渲染只要差一点，刷一次所有卡片的样子都会变，
所以拿用户 Anki 里现有的 1045 个词，分别用 Python `_build_meaning_html(_lookup_yomitan_zh(词))`
和 Rust `meaning_html(lookup_definitions(词))` 生成，逐字比。

第一次对：**逐字一致只有 2 个**。差异分类后逐项修掉：

| 词数 | 差在哪 | 修法 |
|---|---|---|
| 1043 | 词典标题颜色：Python 用 md5 散列，Rust 用 FNV | 手写 md5（只为取色，不为这一处引入 md-5 一串依赖），用标准测试向量和真实样本钉住 |
| 437 | 「１コップ…」这种全角数字开头的义项号 Python 会单独排，Rust 只认圆圈数字 | 照抄 Python 的 `_render_item` 正则：【词形】、义项号、〔标签〕、`[注]` |
| 245 | 长释义拆句：Rust 遇到任何「（」都切，Python 只在「（一）」「（１２）」这种带数字的括号前切 | 照抄 `_split_flattened_def_text`，包括 `re.split` 零宽前瞻在「12〔」处切两刀的行为（用 Python 实跑确认） |
| 44 | 【词形】前缀：Python 只加在切出来的第一段 | 按条目分组，只给第一段加 |
| 13 | 一条释义都没有时 Python 写 `<div class='defs'></div>`，Rust 写空串 | 照写 |
| 4 | 去重键：Python 空白只折叠，HTML 条目按它自带的 `text` 去重；Rust 删光空白、按带前缀的 HTML 去重 | 照 `_definition_dedupe_text` |

修完：**Meaning 1045/1045 逐字一致，读音 0 差异。** 对账工具是 `cargo run --release -p jp-anki --example dump_meaning`。

## 查词候选

词元查不到释义时换几个形再查，卡片的 Expression 不变。规则照抄 `gui._lookup_candidates`：
先加词元和语料里最常见的写法，再按词性生成「勉強し → 勉強する」「早く → 早い」「よく → 良い」等候选，最多 10 个；
挨个查，第一个查到释义**或读音**的就用，并在释义顶部注明「辞書形：…」。音高、词频在词元查不到时也用这个形再查一次。

真实语料 5,863 个（词元, 词性）逐项对账：最常见写法、候选列表、最终用的查词形，**三项都是 5863/5863**。
直接查到 5,315；靠候选救回 76，大多是大写英文（`YOU` → `you`）和可能形（`止まれる` → `止まれ`）。
Python 在本地词典全都查不到时会去问在线的 Jotoba / Jisho——**这里不联网**，如实返回空。

## 导出选项

| 选项 | 取值 | 说明 |
|---|---|---|
| 已有这个词时 | 追加新例句 / 跳过 | 追加时只加没有的句子，**从不替换**；音频不追加（和 Python 一致） |
| 查重范围 | 当前牌组 / 主牌组及子牌组 | 主牌组时 `addNote` 带 `duplicateScopeOptions`，找已有卡也在主牌组里找 |
| 音频片段 | 开 / 关 | 默认开（有 ffmpeg 时）。和 Python 的默认一致 |

导出前先检查牌组在不在，不在就整批不开始，并列出现有牌组。

**刻意比 Python 多做的一步：先问 `canAddNotes`。** Python 是先切音频、先传给 Anki，再加卡；
遇到重复的词，传上去的文件在 Anki 媒体文件夹里就没人引用了。这里先问能不能加，能加才切、才传。

### 音频片段

参数照抄 `gui._clip_audio`：开头提前 0.3 秒、结尾多留 0.5 秒、`libmp3lame -q:a 5`、30 秒超时。

**文件名和 Python 刻意不同。** Python 是 `jpop_{utterance_id}.mp3`；行号在重建库、换库、迁移之后会重新分配，
再给同一个行号制卡就会覆盖掉旧卡的音频（用户真实的 Anki 里 84 个被引用的旧片段，有 15 个的行号在新库里已经是另一句）。
现在按内容起名 `jpop_clip_{mp3 的 md5}.mp3`，存的时候传 `deleteExisting: false`，卡片里用 Anki 实际存下的名字
（同名不同内容时 Anki 会改名，不会覆盖）。旧的 `jpop_{数字}.mp3` 一个不动，引用它们的卡照常能放。
词典图片和封面同样不再覆盖。
一个容易看错的细节：Python 导出线程传的是 `end_sec or t_sec + 5.0`，所以**没有下一行时间戳时是 5 秒**，
切片函数里那个 6 秒只在下一行时间戳贴得太近时才用。

ffmpeg 找 PATH 和项目根目录；Windows 下启动时带 `CREATE_NO_WINDOW`，不闪黑框。
测试用本机 ffmpeg 现生成一段 4 秒的测试音再切，不碰真实曲目，也不碰真实 Anki。

用户现有 1046 张卡里 1045 张带音频，这是在用的功能。

## 更新已有卡 / 刷新旧牌组

两者都只重写 **Reading / Meaning / JLPT / Pitch / Freq / PartOfSpeech** 六个字段，
**不动 Expression、Sentence、SentenceAudio、Source**——例句和音频是用户复习过的语境。开始前顺带更新卡片模板和 CSS。

- **更新选中的词**：在（当前牌组 / 主牌组）里按 Expression 找卡。比 Python 多带一个 `note:"JPOP Corpus"` 条件，
  别的笔记类型碰巧也有 Expression 字段时不会被改。
- **刷新旧牌组**：按笔记类型、按标签 `jpop-corpus` 各查一遍取并集，再按卡片所在牌组过滤
  （仅当前牌组 / 当前牌组及子牌组 / 主牌组及子牌组；「JPOPX」不算「JPOP」的子牌组）。
  词性取这个词在语料里最常见的那个。界面上先「数一下有几张」，按钮再变成「刷新这 N 张」，两步确认，不弹窗。

释义**不按「取前几部词典」截断**：已有的卡是按全量做的，截断的话刷一次所有卡都会变短。

读 Expression 的纯文本时**先反转义、再去标签**，和 Python 一致。Rust 原来顺序反了，
`&lt;b&gt;` 这种转义过的尖括号会留下来；落单的 `<` 还会把后面的字全吞掉。

## 一键制卡（Lyrics 笔记类型）

曲库里点词，右栏查词结果每个词条上有「＋ 制卡」。和上面的「JPOP Corpus」批量导出是两条独立的路。

**笔记类型「Lyrics」**：照 [Lapis](https://github.com/donkuri/lapis)（GPL-3.0）做的。模板在 `jp-anki/data/lyrics/`：
`front.html` `back.html` `styling.css` 由 `jp-anki/tools/make_lyrics_templates.py` 从 Lapis 源码生成，只改两处：
根元素加 `class="lyrics"`、背面右侧的图换成「歌曲封面 + 歌名 / 歌手 / 专辑」；
歌曲信息的样式是手写的 `song-info.css`，程序把它接在 Lapis 样式表后面。
字段是 Lapis 的 22 个再加 `SongTitle` `Artist` `Album`，所以 Lapis 的用户设置变量照样能用。

排版：封面右边竖排，从右往左是歌名、歌手、专辑，顶端和封面对齐。Lapis 的脚本会把封面高度调成和左边词条框一样高，
每张卡都不同，所以用 CSS grid 让竖排的高度等于封面的实际高度，长歌名自动换列（flex 算宽度时拿不到这个高度，列会溢出）。
手机上放不下几列竖排，横排放在封面下面。没有封面时竖排高度上限 200px。

第一次制卡时自动建。已存在时**不整体刷新模板和样式**（Lapis 的用户习惯直接改样式表顶上的设置变量），只做两件事：

- 缺字段就补字段，只增不删。
- 样式表里歌曲信息那一块（`/* ---------- Lyrics：` 开头，到 `/* ---------- Lyrics 结束 ---------- */`；第 1 版没有结束标记，到末尾）
  如果和 `song-info-history/` 里以前发过的某一版逐字相同（不计换行符），换成当前版，块外一个字不动，换行符跟 Anki 里原来的一致；
  用户改过或删了就不动。**改 `song-info.css` 之前要先把旧版存进 `song-info-history/` 并加进 `SONG_INFO_HISTORY`**，否则已经建了 Lyrics 的用户拿不到新样式（有测试检查当前版不能和历史版相同）。
  `cargo run -p jp-anki --example lyrics_styling` 只读地看 Anki 里的属于哪种，加 `-- --apply` 就地升级。

| 版本 | 样子 |
|---|---|
| 1 | 信息在封面左边横排、右对齐（手机上字号算成 0，其实不显示） |
| 2 | 封面右边竖排，三列居中，上下错开 |
| 3 | 同上，顶端对齐（当前） |

设置里也能换成用户自己的 Lapis（那样歌曲信息只进 MiscInfo）。

**按歌手放进子牌组**（设置里的开关，默认关）：开了之后，从歌里制的卡放进「牌组::歌手」，比如「JPOP::ヨルシカ」。
这个子牌组有就直接用（Anki 的牌组名不分大小写，用 Anki 里原来的写法），没有就用 `createDeck` 建。

- 歌手取曲库里这首歌的歌手串。合作曲按第一位歌手：「ずっと真夜中でいいのに。/森カリオペ」放进「JPOP::ずっと真夜中でいいのに。」。
  真实曲库 14 个歌手串里有 6 个合作曲，都是主唱在前。
- 歌手名里如果有 `::`，换成单个冒号，免得多出一层牌组。
- 不是从歌里查的词（没有歌曲上下文），还放在上面选的牌组里。
- **查重仍然按父牌组问**，范围是整个 collection，和放进哪个牌组无关。另外，子牌组还没建时，AnkiConnect 的 `canAddNotes` 找不到牌组会直接回 false，会被误当成重复。
- **子牌组放到最后、加卡之前才建**，重复的词或中途失败都不会留下空牌组。
- 结果里带回实际放进的牌组和是不是新建的，按钮悬停能看到。
- 规则在 `jp_anki::mine::artist_subdeck`。前端 `dict/mine.ts` 有一份同样的，只用来在按钮提示里预先写出会放进哪；两边测试用同一组例子。

| 层 | 位置 |
|---|---|
| Yomitan 制卡标记（`{glossary}` `{frequencies}` …）→ 字段 HTML | `jp-dict/src/anki/`，和 Yomitan `anki-note-builder-test-results.json` 对账：77 个词条、2002 个字段逐字相同 |
| 注音分配（`{furigana-plain}`） | `jp-dict/src/furigana.rs`，和前端 TS 版用同一份 Yomitan 基准（3164 条） |
| 同一首歌里找含这个词的行 | `jp-dict/src/occurrence.rs` |
| 组字段、查重、存媒体、建笔记类型、加卡 | `jp-anki/src/mine.rs`、`lyrics_model.rs` |
| 取歌词、对齐分词和原文、Tauri 命令 | `jp-app/src/mine.rs`，`anki_mine` `anki_mine_check` `anki_browse_notes` `anki_model_names` |
| 按钮、设置 | `app/src/dict/MineButton.tsx`、`MineSettingsCard.tsx`（Anki 页）、`mine.ts` |

字段：

| 字段 | 内容 |
|---|---|
| Expression / ExpressionReading / ExpressionFurigana | `{expression}` / `{reading}` / `{furigana-plain}` |
| MainDefinition | 首选词典的 `{single-glossary-…}`；首选词典里没有这个词时退到排在最前、有释义的那本 |
| Sentence | 点的那一行，查的词加粗；**这首歌里其他含这个词的行接在后面**（`<br>` 分隔），文字相同（去掉空白后）的行只放一次 |
| SentenceAudio | 每一句各切一段（到下一行开始为止），顺序和 Sentence 一致 |
| Picture | 歌曲封面 |
| SongTitle / Artist / Album | 歌名、歌手、专辑 |
| Glossary | `{glossary}`，所有词典，带词典自带样式 |
| PitchPosition / PitchCategories / Frequency / FreqSort | 对应的 Yomitan 标记 |
| MiscInfo | 歌手「歌名」 时间 |
| IsWordAndSentenceCard 等 | 按设置里的卡片类型打 `x`，默认词+句卡 |
| ExpressionAudio / SentenceFurigana | 留空：单词读音要联网取；例句注音 Lapis 建议交给 AJT Japanese |

「含这个词」怎么判断（`occurrence.rs`）：从每个词的起点做活用还原（和查词同一套），还原结果等于词头、
活用条件和词条词性相容，**并且匹配的末尾落在词的边界上**——「今夜」里的「夜」不算。点的是假名写法时读音也算。
真实歌词实测（《ナイロンの糸》55 行）：「消える」4 行、副歌重复去掉后 3 句；「甘える」找到「甘えて」「甘えてる」，去重后 2 句；每首歌 0.15–0.17 秒（release 构建）。

几条定下的：

- **当前句的音频必须有**：切不出来（没有 ffmpeg、这一行没有时间轴、音频文件不在）时卡照样加，但结果里给出原因，
  按钮变黄并弹出提示，不悄悄留空。其他句子切不出来只报数量。
- **先查重，再切音频、传图片**。查重用 `canAddNotes`，范围是整个收藏（Yomitan 默认）；
  重复时按钮变成「已有卡」，点了用 `guiBrowse` 在 Anki 里打开，不加第二张。Lyrics 还没建时查重直接当作没有卡。
- 词典图片存进 Anki 媒体库的名字按（词典, 路径）的 md5 固定，封面按路径 md5，同一个文件不会存出一堆副本。
- 词条、例句都在后端重新取，前端只传查词文字、词头、歌/行/第几个词。
- 分词结果拼起来不一定等于原文（全角空格会被丢掉），按词逐个在原文里对齐；对不上就退回用词拼成的句子，不猜。
- 词典样式的作用域用 CSS 嵌套写法（Yomitan 自己测试里的输出），Anki 25.02 起的桌面版和较新的手机端都支持。

`examples/mine_dry_run.rs`：在真实词典库上渲染字段、只读地问 Anki 查重，不加卡（`FIELDS_JSON=路径` 可导出字段做模板预览）。
实测「大げさ」「スムーズ」判为重复、找到的正是用户已有那两张 Lapis 卡。
`jp-dict/examples/song_occurrences.rs`：只读打开 corpus.db，看某首歌里哪些行含某个词。

## 挖词报告

PyQt 版菜单「报告 → 生成挖词报告…」调的是 `generate_report.py`：读 collection 里的卡，按歌手、歌曲、JLPT
统计已挖的词和复习进度，写成一个自带搜索和排序的 HTML，用浏览器打开。Rust 在 `mining_report`，
应用层（查 JLPT、写文件、开浏览器）在 `jp-app/src/anki_report.rs`，界面是 Anki 页的「挖词报告」卡片。

页面骨架、CSS、JS **不是手抄的**：用 Python 从 `generate_report.py` 里原样抽成 `jp-anki/data/mining_report/`，
拼接逻辑逐行照搬。在合成的 collection 上（160 张卡：并列、超过 25 个歌手、实体、标签、非法 JLPT、重复词、字段不够长的旧笔记）
和 Python 写出的文件比：**除了「来源」「生成时间」两栏和「词频」表头，逐字节相同**，换行也一样是 CRLF
（Python 的 `write_text` 在 Windows 上文本模式写）。

有意不同的几处：

| 项 | Python | Rust |
|---|---|---|
| 笔记类型 | 只认 JPOP Corpus | JPOP Corpus 和 Lyrics。只认前者的话，用户真实的 collection 里报告永远是「找不到卡片」 |
| Lyrics 卡的歌手 / 歌名 | — | `Artist`、`SongTitle` 字段；都空时从 `MiscInfo`（「歌手「歌名」 时间」）解析 |
| Lyrics 卡的 JLPT | — | 没有这个字段，查本地 `jlpt_cache` |
| 词频列 | 表头「JPDB 词频」 | 「词频」：Lyrics 卡取 `FreqSort`，那是几部词频词典的调和平均 |
| 牌组名 | 原样写 `\x1f`（看不见） | 写成 `::` |
| 字段反转义 | 完整的 `html.unescape` | `plain_field`，只解常见的几个实体（真实 collection 里这几个字段一个实体都没有） |
| 选哪个 profile | 文件名排序第一个 | 最近改过的（`learning::find_collection`） |

注意 `clean_field` 是**先反转义再去标签**：出处「`A&amp;B「C&lt;D&gt;」`」解出来是歌手 A&B、歌名 C——
`<D>` 反转义之后被当成标签去掉了。Rust 照这个行为。

输出和 PyQt 版一样在项目根目录的 `output/corpus_report.html`，每次覆盖（生成物，不是用户数据）。
「生成时间」由界面按本地时间给，后端只收数字、`-`、`:`、空格。打开浏览器用 `explorer`（macOS `open`，Linux `xdg-open`），
路径里的 `/` 先换成 `\`——explorer 碰到正斜杠会打开「文档」文件夹。

```bash
# 和 Python 对账：同一个 collection 分别生成，再比
python generate_report.py --db <collection.anki2> --output py.html
cargo run -p jp-anki --example mining_report -- <collection.anki2> rs.html "<生成时间>"
```

## 还没做

- 一键制卡的单词读音（ExpressionAudio）、例句注音
- 重复词的「手动选择」（逐个弹窗勾选要追加的例句）。批量导出时逐个弹窗等于几百次点击，先没做
- 英文释义（Python 走在线的 Jisho）。按「不用远程服务替代本地数据」的原则不做
- 屏蔽词表、JLPT 筛选和排序
