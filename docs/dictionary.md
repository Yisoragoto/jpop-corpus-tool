# 词典与查词（jp-dict）

Yomitan 格式词典的导入、存储、查词，以及界面上的词条显示。算法、规则表、显示逻辑移植自
[Yomitan](https://github.com/yomidevs/yomitan)（commit `d34832d7`，GPL-3.0-or-later），
存储思路参考 [hoshidicts](https://github.com/Manhhao/hoshidicts)（GPL-3.0-or-later，只借鉴设计，没有抄代码）。
项目因此改为 GPL-3.0-or-later。

## 为什么是移植 Yomitan，而不是链接 hoshidicts

hoshidicts 是 C++23 加一串子模块（zstd、glaze、utf8proc…），`hoshidicts-rs` 只是绑定。
接进来要多一套 C++ 工具链，而本项目的约束是「不为此引入额外工具链和大量依赖」。
Yomitan 的查词核心是纯逻辑（活用规则表 + 广度优先还原 + 排序），用 Rust 重写后
可以拿 Yomitan 自己的测试和期望结果逐项对账——这比「链接一个黑盒」更可测。

## 模块

| 模块 | 对应 Yomitan | 说明 |
|---|---|---|
| `transformer.rs` | `language-transformer.js` | 活用还原。规则数据 `data/japanese-transforms.json` 由脚本从源码导出，不手抄 |
| `text.rs` / `variants.rs` | `japanese.js`、`japanese-wanakana.js`、`CJK-util.js`、`translator._getTextVariants` | 半角、罗马字、合成浊点、兼容字符、部首、平片假名、强调符号。码表从源码和 Node 的 Unicode 数据导出 |
| `deinflect.rs` | `translator._getAlgorithmDeinflections` | 逐字缩短 → 文本变体 → 活用还原 |
| `translator.rs` | `translator.findTerms` | simple / split / group / term 四种模式、词频、音高、标签、排序 |
| `import.rs` + `zip.rs` + `media.rs` | `dictionary-importer.js` | zip 读取（自写，只支持不压缩/deflate，逐文件校验 CRC）、释义整理、图片 |
| `store.rs` | `dictionary-database.js` | 独立的 `dictionaries.db`，查询顺序照 IndexedDB |
| `app/src/dict/*` | `structured-content-generator.js`、`pronunciation-generator.js`、`japanese.js` 注音部分、相应 CSS | React 显示 |

应用层：`jp-app/src/dict.rs`（导入作业、旧版词典清单、查词选项）和 `commands.rs` 里的 `dict_*`。

## 和 Yomitan 对账

能拿 Yomitan 源码跑出基准的地方都拿来逐项比，基准由 `rust/crates/jp-dict/tools/*.mjs` 用 Node 直接运行 Yomitan 源码生成。

| 对象 | 基准 | 结果 |
|---|---|---|
| 活用还原 | Yomitan `japanese-transforms.test.js` 全部用例 | 1406 / 1406 |
| 活用还原全量输出（文字、条件位、trace、顺序） | 测试输入 + 6310 条真实歌词行前缀 | 7696 / 7696 条输入逐项一致 |
| 截断 + 文本变体 + 活用还原整条管线 | 同上 + 57 个特殊输入 | 7751 / 7751 条输入、219,134 行逐行一致 |
| `findTerms` 输出（逐字段） | Yomitan `translator-test-results.json` | 35 / 35（跳过 15 条：汉字查询 3、merge 模式 2、文本替换 5、非日语 5） |
| 导入后的条目 | 同上期望结果里每个定义的 id、释义、score、sequence | 全部一致（id 的先后也和 Yomitan 相同） |
| 注音分配（TS） | Yomitan 用例 + 1500 个真实词头 | 1558 / 1558；活用形注音 1606 / 1606 |

## 刻意和 Yomitan 不同的地方

- **长度按 Unicode 字符计**，JS 是 UTF-16 码元。只有「𠮷」这类 BMP 以外的字不同；JS 会把它劈成半个代理对去查词典，那种字符串必然查不到。`originalTextLength` 因此是字符数，界面截取时按字符。
- **名字排序没搬 ICU 排序表**：ASCII 不分大小写、同字母小写在前，其余按码位。只在标签 order 相同、或词条其余排序键全部相等时才用到。对账用例里没有撞上。
- **整本词典一个事务导入**，失败或取消什么都不留（Yomitan 出错会留下半本）。
- **引用的图片缺失、类型不认识时记警告继续导入**（Yomitan 整本报错）。用户的词典包是旧版导入过的，不能因为一张外字图整本导不进来。
- **图片尺寸读文件头**（PNG/GIF/JPEG/WebP/BMP/SVG），读不出记 0，界面按图片加载后的实际大小算宽高比（Yomitan 在浏览器里解码，默认 100×100）。
- **结果截断在读释义之前**：Yomitan 在 backend 里 `findTerms` 之后截到 `maxResults`（默认 32），这里放进选项，截断后才解压释义，输出相同。
- 暂未移植：merge 模式（依赖主词典 + 序号合并）、汉字查询、文本替换、按词切分、`standardizeKanji`（旧字体→新字体，Yomitan 用 npm 包 `kanji-processor` 的异体字表，基准里同样换成了恒等函数）。

## 存储：dictionaries.db

和 `corpus.db` 放在同一目录、分开存。**懒创建**：没用到词典功能不会在项目目录里新建文件（集成测试也用真实项目目录装配状态）。WAL 模式：导入一本大词典是几十秒的大事务，WAL 下界面照常查词；导入期间的写操作（启用、排序、删除）直接提示「等导入完成」，不让用户等锁超时。

释义不逐条存 JSON，按导入顺序拼成约 64 KB 一块、raw deflate 压缩；条目行记块号、偏移、长度，另记 `glossary_flags`（有无正文、有无「某词的变形」），查词时只解压真正要用的。

实测（`examples/block_size_study.rs`，真实词典各取 80 MB 释义）：

| 块大小 | 大辞泉 压缩率 / 解一块 | 明鏡日汉（短条目） |
|---|---|---|
| 4 KB | 16.7% / 17 µs | 36.9% / 27 µs |
| 16 KB | 10.7% / 28 µs | 31.5% / 65 µs |
| 64 KB | 8.1% / 74 µs | 28.4% / 226 µs |

用户手上 21 本词典全部导入：**130 秒，0 条警告，库 804 MB**（旧版 `dict_terms` 连索引 868 MB，且丢了结构化释义、词性规则、标签、图片、按词典和读音区分的词频）。最大的大辞泉 63.6 万条 45 秒。

## 查词性能

`examples/lookup_bench.rs`：150 行真实歌词，从每个字起查一次（1542 次，和点击查词一样取 16 个字），21 本词典全开。

| | 平均 | p50 | p90 | p99 |
|---|---|---|---|---|
| 最初（逐条串行解压释义，不截断） | 42.8 ms | 26 ms | 100 ms | 207 ms |
| 并行解压 + 截到 32 条（应用里的设置） | **17.3 ms** | **13.6 ms** | **38 ms** | **66 ms** |

各阶段计时显示最初 91% 的时间花在解压释义上（平均每次 117 条），活用还原、查条目、分组、词频音调、标签、排序合计约 4 ms。所以只改了这一处：按块批量读出后多线程解压、解析。再往下还可以换 16 KB 块（上表，库大约 +30%），目前没做。

## 旧版词频数据的问题

旧版的 `yomitan_freq` 表是「一个词一个数」：导入每本词频词典时 `INSERT OR REPLACE`，后导入的覆盖先导入的；带「㋕」（假名写法）的条目直接跳过；读音信息丢掉。于是 Anki 卡片上标着「JPDB」的词频大多其实是后导入的 Jiten 的值：

「夜【よる】」旧表是 376（Jiten 综合），JPDB 实际是 392；新库里按词典、按读音分别保留：JPDB 392、Jiten 376、Jiten (Anime) 508，另有各自的假名写法排名。旧 Anki 卡片的 Freq 字段还没有改用新库。

## 界面

- **词典页**：整页就是查词（活用形、片假名、罗马字都能查），一本词典都没有时给的是导入入口。管理在**设置页**（`dict/DictionaryManager.tsx`）：导入 zip、「从旧版迁移 N 本」（读 `corpus.db` 的 `dict_registry`，原始 zip 还在的直接重新导入）、启用 / 停用、**拖动排序**（也能用键盘：抓手上按 ↑ / ↓）、删除（两步确认，只删库里的数据）。每本一行，和设置页其它行同一套版面（`components/SettingCard.tsx`）。
- **曲库页**：点歌词里的词，右栏按「词 → 语料统计 → 词典 → 例句」排。从点的词起到行尾交给词典做最长匹配，所以「打ち込んでいませんでした」点第一个词就能还原到「打ち込む」。词典这一段可整体折叠（记住），窄栏默认只显示前 3 条。
- **折叠**：点词典名折叠这一本（记住，之后所有查词里这本都折叠，只留一行预览）；结果顶上一键折叠 / 展开这次出现的全部词典；单本释义过长先截断（词典页约 420 px、曲库右栏约 200 px），点「展开全文」。
- **暗色主题**：词典自带样式大多按 Yomitan 的主题变量写、带浅色回退值（明鏡的粉框 `var(--meikyo-pink, var(--danger-color-lightest, #FFCCCC))`），所以照 Yomitan 暗色主题（`material.css`、`display.css` 的 `:root[data-theme=dark]`）把变量补齐。另外两处是 Yomitan 暗色主题里本来也难看的：例解学習国語的 `--rgko12-*` 浅色框给了深色调（作者留的覆盖入口）；透明底、没标 `monochrome` 的 SVG（明鏡、三省堂的外字和音调图，黑线）反色显示，栅格插图不动。
- 纯文本释义里的 `<br>`（明鏡日汉双解）当换行，其余尖括号仍按文本显示。
- 词典自带的 `styles.css` 按 Yomitan 的做法包进 `[data-dictionary="词典名"] { … }` 注入，互不污染。
- 图片走 `dict_media` 原始字节建 Blob URL。CSP 要同时有 `img-src blob:` 和 `connect-src ipc: http://ipc.localhost`。后者缺了时，Tauri 的 IPC 整体退回 postMessage 通道，控制台只有一条警告，二进制返回值却变成数字数组，塞进 Blob 就是乱码、图片全部加载失败——真实应用里出现过。前端的 `toBytes` 两种都认。

## 重新生成基准

在 Yomitan 仓库（commit `d34832d7`）上：

```text
node tools/dump-yomitan-transforms.mjs      <yomitan> <输出目录>
node tools/dump-yomitan-text-processing.mjs <yomitan> <输出目录> data tests/fixtures <额外输入.txt>
node tools/dump-yomitan-japanese-util.mjs   <yomitan> <词形\t读音列表> app/src/dict/__fixtures__/yomitan-japanese-util.json
```

大规模对账（不入库）：`examples/parity_transforms.rs`、`examples/parity_text_pipeline.rs`。
