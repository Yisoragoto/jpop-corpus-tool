# 导入管线

把「一个装着音频的目录」变成「语料库里的歌 + 歌词 + 分词」。
对应 `rust/crates/jp-import`，Tauri 侧入口是 `scan_folder` / `run_import`。

在这之前，Tauri 版是**只读的**——加歌只能回去用 PyQt 版。

## 分层

| 模块 | 职责 | 能离线测试 |
|---|---|---|
| `filename` | 文件名 / 目录结构 → artist·title·音轨号 | ✅ |
| `lrc` | LRC 解析（歌词行 + 作词作曲编曲） | ✅ |
| `scan` | 遍历目录、读 tag → `ScannedTrack` | ✅（构造数据） |
| `plan` | 和现有曲库比对 → 每个文件打算怎么处置 | ✅ |
| `import` | 执行计划，写库 | ✅（内存库） |

前四层**只读取和推断，不写库**，所以不需要数据库就能完整测试。
`import` 是唯一写库的一层。

## 扫描 → 复核 → 确认

扫描是只读的，写库要再点一次。理由是导入一次会往库里写几千行，
用户有权在写之前看清「哪些是新的、哪些被判成重复、哪些读不出曲名」。

`plan` 对每个文件给一个处置，每种都能说出理由：

| 处置 | 含义 | 写库 |
|---|---|---|
| `New` | 库里没有，分配 id 新建 | ✅ |
| `AlreadyImported` | 这个文件路径已在库里 | ✗ |
| `PossibleDuplicate` | 曲名歌手对得上，但指向另一个文件 | ✗ |
| `DuplicateInBatch` | 同一批扫描里出现了重复 | ✗ |
| `Skipped` | 连曲名都定不出 | ✗ |

`PossibleDuplicate` **不自动决定**。可能是换了音源（FLAC 换 MP3），
也可能真有两个版本（原版 / Live）——猜错就把用户的库弄脏了，交给人看。

路径相同优先于曲名歌手相同：路径是更硬的证据。

## 幂等

重扫同一个目录不该产生任何新条目。这条在真实曲库上验过：

```
扫描 209 个文件 → 已在库中 209，新增 0，疑似重复 0，跳过 0
```

见 `cargo run -p jp-import --example plan_import`。

## 一首歌一个事务

不是整批一个事务。第 150 首失败时，前 149 首留在库里，
重跑扫描会把它们判成「已在库中」，从第 150 首接着来。
整批一个事务的话一次失败全部回滚，两百首重来一遍。

失败的那首**一行都不会留下**，报告里逐条写明是哪首、为什么。

## 写哪些表

| 表 | 内容 |
|---|---|
| `songs` | 曲目。tag 原值直接写，不做任何「美化」 |
| `utterances` | 歌词行 |
| `utterances_fts` | 全文索引 |
| `tokens` | 分词 |
| `people` / `track_credits` | 演唱者 + 从 LRC 解析的作词作曲编曲 |
| `albums` | 专辑，并回填 `songs.album_id` |

三个容易踩的点：

1. **`utterances_fts` 是 `content=utterances` 的外部内容表，没有触发器。**
   往 `utterances` 插数据不会自动进索引，必须手动
   `INSERT INTO utterances_fts(rowid, text)`。漏掉的话歌能存进去但搜不到。
2. **`tokens.pos` 存的是 UPOS**（`NOUN` / `VERB` …），不是日语词性。
   写日语词性进去会让整个语料的词性统计对不上。
3. **`track_credits` 里 `source='manual'` 的行不能覆盖**——那是用户的修正。

## 归一化键必须和 Python 逐字一致

`matching_key` 决定「是不是同一首歌 / 同一个人」。迁移期两边共用一个
`corpus.db`，两边算得不一样就会 Python 认为重复、Rust 认为是新的，库会写脏。

Rust 侧原来用 `filter(char::is_alphanumeric)` 近似，实测拿 294 个真实
曲名 / 歌手 / 专辑 / 人名比过，**42 个对不上**（14%）。主因：

- 片假名长音符 `ー` 的 Unicode 类别是 Lm（修饰字母），`is_alphanumeric()`
  判真会留下它，而 Python 当标点去掉（「アルジャーノン」→「アルジャノン」）；
- 反过来 `♪` `→` `☆` 这类不在 Python 标点表里的符号，`is_alphanumeric`
  又会丢掉。

改成照搬 Python 的标点表 + feat 剥离之后：**294/294 完全一致**。
见 `cargo run -p jp-import --example compare_keys -- <keys.tsv>`。

## 和 Python 建的库对账

往一个空库里完整导入同样 209 个文件，和 Python 当初用 SudachiPy + GiNZA
建的 `corpus.db` 比：

```
songs          rust=209     python=209     =
utterances     rust=8494    python=8443    ≠  (+51)
tokens         rust=58599   python=58628   ≠  (-29)
people         rust=28      python=28      =
albums         rust=107     python=111     ≠  (-4)
track_credits  rust=442     python=442     =

切分一致的行 : 8421/8421 (100.00%)
连词性也一致 : 4834/8421 (57.40%)
```

见 `cargo run --release -p jp-import --example import_library -- <空库>`。

每一处差异都查清了：

### utterances +51：Python 那边的两个 bug

- **[122] Vaundy - 再会（+47）**：这个 LRC 用 `[00:19:05]` 这种把小数点
  写成冒号的变体。Python 的正则 `^\[(\d+):(\d+\.\d+)\]` 要求小数点，
  一行都没匹配上，**整首歌的歌词静默丢失**（库里是 0 行）。
- **[079] サカナクション - ワンダーランド（+4）**：这份 LRC 每行两个时间戳
  （一份重新对轴的版本）。Python 只剥了第一个，第二个连同方括号被当成
  歌词正文存了进去，库里能看到 `[02:02.788]卵の殻を破った雛` 这样的行，
  还有 4 行正文为空、只剩一个孤立时间戳。这些方括号进了分词器，
  产生 24 个垃圾 token。

  Rust 把一行里的每个时间戳都展开成独立的一行，空正文的丢掉：
  `10 单戳 + 8 双戳×2 = 26`，Python 是 `10 + 12 = 22`。

`[mm:ss:cc]` 的判别按**整份文件**来，三条判据从硬到软：

1. 末位带小数点 → 一定是 `h:mm:ss.xx`（冒号变体不会再带点）；
2. 末位 ≥ 60 → 一定是厘秒；
3. 按 `h:mm:ss` 解出的最大值超过一小时 → 当成厘秒。

单独一个 `[00:19:05]` 是真歧义，定不下来就保持 `h:mm:ss` 的原解释。

### tokens −29：空白 token

GiNZA 把空白也当词写进了库，全库 **275** 个：273 个全角空格 U+3000
（标成 SYM）、1 个半角空格（PUNCT）、1 个 `\xa0\xa0`（被标成 **NOUN**）。
Rust 侧不收空白。

> 注意：用 SQLite 的 `TRIM(surface)=''` 数只会数出 1 个——
> SQLite 默认只去 ASCII 空格，不去 U+3000 和 U+00A0。

账正好平：
`+256`（122 整首，Python 为 0）`− 275`（空白）`− 10`（079 的污染差）`= −29`。

除去 079 / 122 这两首，其余 207 首的 token 差是 **−269**，
恰好等于那 207 首里的 269 个空白 token，分毫不差。

### albums −4：不是差异，是数据来源不同

Python 库里的专辑名是 scraper 从 iTunes 补的（`アルジャーノン - Single`），
文件 tag 里写的是 `アルジャーノン`。导入照 tag 原值写才对——
「永远不覆盖用户文件里的原始 metadata」。几张单曲因此合并，107 vs 111。

刮削补全是另一步，不该由导入代劳。

## 已知缺口

### 词性只有 90.55% 和 GiNZA 一致

分词**逐词 100% 一致**，但 UPOS 只有 90.55%（token 级）。
最常见的分歧：

```
VERB → NOUN   606
 ADJ → NOUN   527
 ADV → NOUN   469
PART → ADP    360
 AUX → VERB   325
```

原因是本质性的：Sudachi 给的是**词典词性**，GiNZA 的 UPOS 是 spaCy
模型**按上下文**判的。「愛」在「愛する」里 GiNZA 判 VERB，查表只能得 NOUN。
一张映射表拿不到这部分。

这是去掉 Python NLP sidecar 的固有代价。影响面：按词性过滤的词频统计
（`word_frequency(pos, …)`）对新导入的歌会有约 9% 的偏差。

### `tokens.dep` / `tokens.head` 留空

那是 GiNZA 依存分析的产物，sudachi.rs 只做分词。
实测这两列全库 100% 填满但**只写不读**——除了
`scripts/04_export_processed_from_db.py` 那个导出脚本，
Rust 侧、前端、`gui.py` 的查询都没引用过。

### 存量语料里的脏数据

上面查出来的两处，**现有 `corpus.db` 里还是脏的**（本次没有改动真库）：

- [122] 整首歌的歌词缺失（47 行）
- [079] 12 行文本里嵌着字面时间戳，其中 4 行没有正文；24 个方括号 token
- 275 个空白 token

要清的话，把这两首删掉重新导入即可（`plan` 会把其余 207 首判成
「已在库中」，不会重复）。

## 曲库维护：编辑、删除、找回音频

导入之后的修修补补，Rust 在 `jp-import/src/maintain.rs`，应用层在 `jp-app/src/library_admin.rs`，
界面在曲库页（曲目右侧「编辑信息」「删除」，音频丢失时顶上一条横幅进「找回音频」）。

- **编辑**（`library_edit_song`）：标题、歌手、年份、专辑、流派。歌手改了就重写演唱署名（`source = 'manual'`），
  专辑重新挂接（没有作品了的专辑、人物一并清掉），分词校正里记的歌名 / 歌手跟着改，下次重新导入还能恢复。
- **删除**（`library_delete_song`）：两步确认，不弹窗。删歌词、分词（先删 `utterances_fts` 里的条目）、章节、署名、
  收藏、收听记录、刮削记录，以及没有作品了的专辑和人物。**不删**：音频文件、分词校正（重新导入后能恢复）。
  封面只在它就在封面目录里、文件名就是这首歌的 id 时才删；变调缓存一起删。正在播的话先停。
- **找回音频**（`library_suggest_relinks` → `library_relink_audio`）：选一个目录扫一遍，按「标签或文件名里的歌手 + 歌名」
  唯一对上、或者「文件名就是 id 且在歌手目录下」给建议，逐首确认。一首一个事务，逐首报结果。
  拿真实曲库挪位置试过，209 首建议全对。

`metadata/songs.csv` 按行同步（和 Python 一样整行改写）：文件不存在就不建（报「没有 songs.csv」），
先写 `.tmp` 再改名。UTF-8 BOM、CRLF、最少引号，原样读出再写回逐字节相同。
注意：存量的 songs.csv 里很多行的年份、专辑、流派是旧的——刮削改的是库，没回写 CSV；按行同步只会更新被编辑的那几行。

## 命令

```bash
# 扫描真实曲库，看读出来的东西对不对
cargo run -p jp-import --example scan_library

# 解析全部 LRC，和库里的 utterances 对账
cargo run -p jp-import --example parse_lyrics

# 重扫幂等性验证
cargo run -p jp-import --example plan_import

# 归一化键和 Python 对账
cargo run -p jp-import --example compare_keys -- <keys.tsv>

# 端到端：往空库完整导入并对账
cargo run --release -p jp-import --example import_library -- <空库路径>
```

## 线上格式：带标签枚举的字段名

`Action`（计划）和 `Outcome`（导入结果）是 `#[serde(tag = "kind")]` 的枚举。
**枚举上的 `rename_all = "camelCase"` 只改变体名**（也就是 `kind` 的取值），
结构体变体里的字段名不受影响，要在每个变体上再写一次。

之前漏了，实测序列化出来的是：

```json
{"kind":"imported","lyric_lines":3,"tokens":9,"credits":1}
{"kind":"possibleDuplicate","song_id":"001","existing_path":"D:/a.flac"}
```

而 `ImportPage.tsx` 读的是 `outcome.lyricLines`、`action.existingPath`——
导入结果里「歌词行」一栏一直是 NaN，疑似重复的提示里路径是 `undefined`。
TypeScript 查不出来：类型是手写的，和 Rust 实际发出去的 JSON 之间没有校验。

现在两个枚举的结构体变体都加了 `#[serde(rename_all = "camelCase")]`（和 `QuickHit` 的写法一致），
`plan.rs` / `import.rs` 里各有一个测试直接断言序列化出来的键名。
`jp-anki` 的 `ExportOutcome` 有同样的写法，但它不直接发给前端（`jp-app/src/anki.rs` 手工映射成字符串），没改。
