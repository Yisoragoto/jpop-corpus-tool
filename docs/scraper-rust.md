# 刮削（Rust 侧）

把「一个本地文件」变成「这首歌是哪一首」，并补齐 metadata 和封面。
对应 `rust/crates/jp-scraper`，Tauri 入口是 `scrape_*` 一族 command，
界面在「刮削」页。

Python 侧的 `scraper/` 包仍在，PyQt 版继续用它。两边共用一个
`corpus.db`，所以**归一化、打分、状态取值必须逐位一致**——见下面的对账。

## 分层

| 模块 | 职责 | 能离线测试 |
|---|---|---|
| `jp-normalize`（独立 crate） | 曲名 / 歌手 / 专辑归一化 | ✅ |
| `status` | 状态与失败原因枚举 | ✅ |
| `models` | 各层之间传递的数据结构 | ✅ |
| `similarity` | 字符串相似度（difflib / Jaro-Winkler） | ✅ |
| `matching` | 可解释的候选打分 | ✅ |
| `query` | 由准到宽的查询阶梯 | ✅ |
| `error` | 分类过的 provider 错误 | ✅ |
| `http` | 超时 / 重试 / 退避 / 限流 | ✅（注入传输层和时钟） |
| `providers` | iTunes / MusicBrainz / Deezer | ✅（注入响应） |
| `resolver` | 串起来：查询 → 候选 → 打分 → 判定 | ✅（假 provider） |
| `store` | 状态 / 历史 / 原始元数据落库 | ✅（内存库） |
| `artwork` | 封面下载 / 校验 / 落盘 | ✅（注入传输层） |
| `net` | 真实 HTTP 传输层（ureq） | ✗（要联网） |

除 `net` 外每一层都能离线测试。归一化抽成独立 crate 是因为
`jp-import` 的判重和刮削的匹配用的是同一个键，两处各写一套迟早分叉。

## 和 Python 的对账

三份基线在 `tests/fixtures/`，由真实曲库 + 人造变体生成：

```
归一化   title 1290 / artist 59 / album 138 条，逐字段全部一致
相似度   7140 个样本，最大偏差 5.55e-17（浮点噪声）
打分     4000 个配对 × 两套配置，final 最大偏差 0.000e0
查询阶梯 4183 个用例、19285 条查询，逐条同序一致
```

跑：`cargo test -p jp-normalize -p jp-scraper`。

对账过程里查出三件只有量化才会发现的事：

### 一、rapidfuzz 没装，Python 实际在用 difflib

`scraper/matching.py` 是这么写的：

```python
try:
    from rapidfuzz.distance import JaroWinkler as _JaroWinkler
    def _similar(a, b): return _JaroWinkler.normalized_similarity(a, b)
except ImportError:
    def _similar(a, b): return difflib.SequenceMatcher(None, a, b).ratio()
```

两条分支是**完全不同的算法**（Jaro-Winkler vs Ratcliff-Obershelp）。
`"202newmix"` vs `"aoi"`：difflib 给 0.167，Jaro-Winkler 给 0.0。

实测这台机器上 **rapidfuzz 没有安装**，所以现有行为、以及将来落进
`scrape_state.confidence` 的分数，全部出自 difflib。

Rust 侧两种都实现，**默认走 difflib**，迁移不改变行为。
换成 Jaro-Winkler 应该是一个明确的决定（改 `Similarity` 的默认值），
而不是取决于某台机器上装没装一个包。

> 顺带：Python 那边也该把这个不确定性去掉——要么把 rapidfuzz 写死成
> 硬依赖，要么删掉那个 try/except。现在这样，同一份代码在两台机器上
> 会给出不同的匹配结果，而且没有任何地方记录这件事。

### 二、CPython 3.12 的 `sum()` 是补偿求和

`0.42+0.08+0.16+0.03+0.03` 朴素左折叠是 `0.7200000000000001`，
而内置 `sum()` 给 `0.72`——3.12 起它对浮点走 Neumaier 补偿求和
（等价于 `math.fsum`）。

差这一位会让 base 落到 `0.9562499999…` 而不是 `0.95625`，
四舍五入后 0.9562 vs 0.9563。**4000 个配对里 70 个中招。**
分数要和 0.90 / 0.55 两个阈值比大小，差 1e-4 就可能把「自动采纳」
变成「需要确认」。

Rust 侧实现了同样的补偿求和，并且**按 Python 的字典插入顺序累加**
（不是字母序，浮点加法不满足结合律）。

### 三、`round(x, 4)` 不能先乘一万

`f64::round` 平局时远离零，Python 平局取偶；更要紧的是先乘 10000
本身会引入舍入误差，把并非平局的值判成平局。
`0.95625` 的二进制真值是 `0.95625000000000004…`，在平局之上，
Python 进位到 `0.9563`，而「乘一万 + 取偶」会舍到 `0.9562`。

正确做法是格式化到 4 位小数再解析回来，走对二进制真值的正确舍入。

## 真实 API 验证

离线测试用的是我自己造的响应，验的是「我以为接口长这样」。
下面两个坑只有真请求才会暴露，所以有两个联网的例子：

```bash
# 整条链路，默认 12 首
cargo run --release -p jp-scraper --example scrape_library -- 12

# 单独打 MusicBrainz，验最绕的那段解析
cargo run --release -p jp-scraper --example probe_mb
```

实测结果：12 首全部自动采纳，置信度 0.994~0.999，曲名无一对不上。
MusicBrainz 那边「ダンスホール」的首选发行版是《Unity》而不是
《Halloween Mix for Kids》——`pick_release` 的排序在真数据上成立。

耗时约 4.4 秒/首，主要花在 MusicBrainz（每秒一次的限流 + 它自己的响应
时间）。整库 209 首约 15 分钟，所以批量走后台线程。

## 已知的坑（都有测试钉着）

| 坑 | 后果 | 现在怎么处理 |
|---|---|---|
| iTunes 限流返回 **403 + 空 body** | 归成不可重试的话，209 首里 70 首被判永久失败 | 403 当限流，可重试 |
| `Retry-After` 被丢掉 | 只能用自己的退避节奏，继续被拒 | 分类时把响应头传下去 |
| 限流器用 0 当「还没发过」的哨兵 | 刚开机时第一次请求白等一个间隔 | 用 `None` |
| MusicBrainz 退避起点 < 限流间隔 | 重试本身就是违规请求，偶发失败变必然失败 | 退避起点 = 1.1s |
| MusicBrainz 的 `score` | 查不存在的名字也给 100 分 | 只认名字/别名归一化后相等 |
| `releases[0]` 是任意顺序 | 「ダンスホール」匹配到杂锦碟 | 按 secondary-type / primary-type / 日期排序 |
| Deezer 对无照片的歌手也返回 `picture_xl` | 下下来是灰色人形占位图 | URL 里 `/images/artist//` 的跳过 |
| 曲名无法比较时 provider 先验独自撑起分数 | 字段全空的候选拿 0.715 进复核队列 | 曲名不可比时直接 0 分 |

**图片和 JSON 不能用同一套超时。** 搜索接口返回几十 KB 的 JSON，10 秒
绰绰有余；一张 600x600 封面是 290 KB，实测这条链路上要 **27 秒**。
两者共用 10 秒的结果是**一张封面都下不下来**，而日志里只有一句
`timeout: global`——元数据全部匹配成功，封面全军覆没，看统计还以为都好了。

现在分两段：`connect` 10 秒（连不上的主机快点失败）、`total` 90 秒
（下得完一张大图）。再加一层**尺寸回退**：600×600 下不完就退到
300×300、100×100——iTunes 的 CDN 把尺寸写在路径里，换个数字就行，
不用重新查询。拿到一张小图也比一张都没有强。

### 快慢是量出来的，不是猜的

一开始整库 209 首要**近两个小时**。压到约 1 分钟，靠的是四处改动，
每一处都先测再改：

| 改动 | 依据 | 效果 |
|---|---|---|
| iTunes 够好就不问 MusicBrainz | A/B 交替 24 首 | **30.7×**，采纳结果 0/24 变化 |
| 封面 600×600 → 300×300 | 数据量 128 KB → 31 KB | **4× 少下**，与网速无关（后来改回 600，见下） |
| 元数据 4 路并发 | 串行 5.60 → 并发 1.88 秒/首 | 3.0× |
| 封面 4 路并发 | 串行 8.0 → 并发 24.2 KB/s | 3.0× |

**最大的一处是第一条，而它一开始不在我的怀疑名单上。**
原来的逻辑是一条查询把所有 provider 都问一遍再判断够不够好。
iTunes 通常一问就给 0.99，但仍然会去问 MusicBrainz——而 MB 限每秒
一次、响应还慢，实测每首要花掉 8~10 秒，对结论毫无贡献。

改成「问完一个就判断」之后，iTunes 不够确定时**仍然**会问
MusicBrainz——那正是它结构化数据有价值的场合。所以这不只是快，
逻辑上也更对。

第二条同样是「白花的力气」：当时全项目显示封面的最大尺寸是 132px
（PyQt 的人物页；Tauri 前端一处都没渲染封面），2× HiDPI 也才
264px。600×600 多下的那 4 倍数据全是没人看的像素。

> **这一条后来撤回了。** 新界面有网格视图（一格最宽 220px）和全屏歌词的大图，
> 300 的图在 2× 下看得出糊——省的那 4 倍数据这下有人看了。`ARTWORK_SIZES`
> 第一档改回 600，阶梯（600→300→200→100）照旧；库里 43 张 300px 的旧图重取了一遍。
> **依据变了结论就得跟着变**，把当时的测量留在上表里，不假装它一直是对的。

> **网络波动很大，绝对时间不可比。** 同一批封面两次测差 9 倍
> （24 KB/s vs 348 KB/s）。所以上表里能当依据的是 A/B 交替测出来的
> 比值和数据量的比值，不是某一次的墙钟时间。
> 「17 分钟 → 1 分钟」这两个绝对数分别测于网络很差和很好的时段。

**数据库只有主循环碰。** `resolver.resolve` 和图片下载都是纯网络、
不需要连接；工作线程把结果交回来，主循环再写库。这样既拿到了并发，
又不用处理多连接写同一个库的问题。

```bash
# 各处的测量都能重跑
cargo run --release -p jp-scraper --example probe_early_stop -- 24
cargo run --release -p jp-scraper --example probe_resolve_parallel -- 8
cargo run --release -p jp-scraper --example probe_cover_parallel
JP_SCRAPE_E2E_LIMIT=16 cargo test -p jp-app --test commands --release   -- --ignored --nocapture a_batch_job
```

### 两级并发，都是量出来的

串行的话整库 209 首要**近两个小时**。两处都测过，结论一致：

| | 串行 | 4 路并发 | 提速 |
|---|---|---|---|
| 元数据查询 | 5.60 秒/首 | 1.88 秒/首 | **3.0×** |
| 封面下载 | 8.0 KB/s | 24.2 KB/s | **3.0×** |

我先前判断过「MusicBrainz 限每秒一次，元数据并发收益有限」——**那是错的**。
限流器限的是两次请求的**间隔**，不是同时在飞的数量：4 个线程排队时请求
仍每 1.1 秒发一个，但各自的响应时间是重叠的。所以吞吐上限是 1/1.1s，
不是 1/(单首总耗时)。

封面那边慢的是单连接被限速而不是总带宽，所以并发同样有效。

跑：

```bash
cargo run --release -p jp-scraper --example probe_resolve_parallel -- 8
cargo run --release -p jp-scraper --example probe_cover_parallel
```

**数据库只有主循环碰。** `resolver.resolve` 和图片下载都是纯网络、
不需要连接；工作线程把结果交回来，主循环再写库。这样既拿到了并发，
又不用处理多连接写同一个库的问题。

整条流水线跑起来实测 **12 首 59.5 秒 = 4.96 秒/首**，整库 209 首约
**17 分钟**（改造前串行约 90 分钟）。封面仍是瓶颈——单张 12~27 秒，
4 路摊下来每首约 3~5 秒，而元数据只要 1.88 秒。

```bash
JP_SCRAPE_E2E_LIMIT=12 cargo test -p jp-app --test commands --release   -- --ignored --nocapture a_batch_job
```

还有一条 Rust 侧新加的：**下载到的东西要校验是不是图片**。
Python 只要 HTTP 200 就写盘，provider 返回一个 HTML 错误页也会被存成
`.jpg`，之后每次渲染都失败而没人知道为什么。这里看文件头，
并且先写 `.part` 再原子改名，中断不会留下半截文件。

## 判定与落库

```
分数 ≥ 0.90            自动采纳，写 songs（只补空字段）+ 下封面
0.55 ≤ 分数 < 0.90     停在「需确认」，等人看，一个字都不写
分数 < 0.55            判定失败，连 candidate 都不给出，但候选列表留着
```

写 `songs` 时只补空字段，**不覆盖已有值**——需求「永远不要覆盖用户
本地文件里的原始 metadata」。原值另有一份完整存在
`track_original_metadata.source_json` 里，重新匹配时用的是它，
而不是可能已被上一轮刮削覆盖过的 `songs` 行。

三张表（`scrape_state` / `scrape_attempts` / `track_original_metadata`）
在库里早就建好了，但一行数据都没有——刮削状态从来没被持久化过。
这是这次补上的最大缺口：现在能回答「我那 12 首为什么没刮出来」。

## 界面

「刮削」页三件事：

1. **复核队列**：并排给出「本地写的是什么」和候选，不同的字段标黄，
   附打分解释（`0.612 = title 0.95×0.42, artist 0.62×0.28  候选多出版本标记 live -0.18`）。
2. **失败说清原因并说该怎么办**：「限流」等一会儿重试就好，
   「搜索无结果」重试一万次也一样——分类字段就是为这个存在的。
3. **批量可中断**：后台线程 + `scrape://progress` 事件，
   而且自己开一条数据库连接，不占着 UI 那条。

同一页还有**歌手照片**（谁有谁没有，能单个刮也能批量刮）、
**专辑封面**（一键用曲目封面填上）和**补齐缺失封面**（只补 `cover_path` 为空的那些，
用刮削时存下的候选，不重新搜索，顺带把专辑封面也填上）。歌手批量和曲目批量
共用一个作业标志——两边都要排 MusicBrainz 的队，同时跑只会互相拖慢。

## 歌手照片与专辑封面

**歌手**：MusicBrainz 拿资料和别名 → Deezer 拿照片。**顺序不能反**：
大量日本歌手在 Deezer 上按罗马字收录（ずっと真夜中でいいのに。→
ZUTOMAYO），不先拿到别名就基本找不到。结果写 `artists` 表
（`image_path` / `artist_type` / `country` / `formed`），照片落
`raw/artists/{歌手}.jpg`。**空值不覆盖已有值**——这一轮没查到照片，
不该把上一轮拿到的抹掉。

端到端验过：ヨルシカ 和 ずっと真夜中でいいのに。都拿到了资料和照片，
后者正是走的别名链路。

**专辑封面**：用曲目封面填 `albums.artwork_path`，**不额外发网络请求**。
同一张专辑的曲目封面就是这张专辑的封面（iTunes 给的本来就是专辑图），
取该专辑里最小 `song_id` 的那张，结果稳定、可重跑。

## 图怎么显示出来

刮削把图存到 `raw/covers/<歌手>/<曲目号>.jpg`，`songs.cover_path` 记路径。
**但存下来不等于看得见**——中间还有三道关，缺一道就是「刮削完一张图都没有」，
而且三道都不报错：

| 关 | 缺了会怎样 | 在哪 |
|---|---|---|
| Tauri 的 `protocol-asset` 特性 + `assetProtocol.enable` | webview 根本不认 `asset:` URL | `Cargo.toml` / `tauri.conf.json` |
| asset scope 里有 `raw/covers` **和** `raw/artists` | URL 认得，请求被静默拒绝 | `jp-app/src/lib.rs` 的 `allow_media_dirs` |
| 界面上真的有 `<img>` | 前面全对，但没人画 | `app/src/components/Cover.tsx` |

scope 是运行时授权的，不是写死在 `tauri.conf.json` 里——项目根可以用
`JPOP_CORPUS_HOME` 换，写死会不准。授权范围**只有 `raw/covers` 和 `raw/artists` 两个目录**，
`commands.rs` 的测试会验 `corpus.db` 不在 scope 里。

Windows 上最容易悄悄挂掉的是 scope 的 glob 匹配：反斜杠、盘符、目录名里的
日文假名。`tests/commands.rs` 里那个测试直接拿库里真实的 `cover_path` 去问
`scope.is_allowed()`，50 条全过才算数。

`Cover` 组件在没有封面或文件被删掉时退回占位块，**不显示裂图**：209 首里有
9 首本来就没匹配到封面，裂图会让人以为程序坏了。

### 专辑封面的回落

`albums.artwork_path` 要跑一次「填专辑封面」才有值，而曲目封面刮削完就有了。
所以查询在专辑没有自己的封面时回落到成员曲目的封面：

```sql
CASE WHEN COALESCE(al.artwork_path,'') <> '' THEN al.artwork_path
     ELSE COALESCE(MIN(NULLIF(s.cover_path,'')), '') END
```

**只影响显示，不写库**——真要落到 `albums` 表还是得走 `fill_album_artwork`。
实测：111 张专辑里 103 张靠回落就有图了（不回落是 0 张）。

### 歌手照片

照片在 `raw/artists/<歌手>.jpg`，和封面**不是一个目录**。第一次只放行了 `raw/covers`，
结果人物页照样一张照片都没有——和封面那次一模一样的三道关，一道没少：
scope 不含这个目录、`PersonSummary` 没有照片字段、人物页没有 `<img>`。

`setup` 和测试现在走同一个 `allow_media_dirs`，测的是程序真正做的事，
而不是测试里自己再授权一遍。

照片按 `people.name = artists.name` **精确匹配**。别名（`ZTMY` ↔ ずっと真夜中でいいのに。）
不去猜——猜错比没图糟。`artists` 表是刮削时才建的，`check_schema` 并不要求它，
所以查询先看 `sqlite_master`，表不在就给空串；没刮削过的库人物页照常能用
（`library.rs` 里有专门的测试）。

实测（28 个人）：

| 状态 | 人数 | 说明 |
|---|---|---|
| 有照片 | 15 | 刮削过的演唱者 |
| 有资料没照片 | 1 | 月村手毬：MB 登记为 Character、无 image 关系，Wikidata 无 P18，Deezer 没这个艺人 |
| 从没查过 | 12 | 作词、作曲、编曲——刮削只查演唱者；外加别名 `ZTMY` |

后两类显示占位块。点合作者切过去时，头像跟着 `Collaborator.image_path` 走，
不会因为是从右栏点进来的就变成占位块。

## 覆盖与一致性：后来补的几处

整库刮完之后仍然有 9 首在界面上是占位块。查下来**不是没匹配上**——
209 首全是 `success`，最低置信度 0.907，而且库里显示的曲名/歌手/专辑
**一个字都不是刮削写的**（和本地标签逐首比对：0 处差异，刮削只补了 36 个空年份）。
差的全在图上。

| 症状 | 原因 | 改动 |
|---|---|---|
| MusicBrainz 赢下匹配的 43 首里 9 首没有封面 | MB 自己不存图，图在 Cover Art Archive | `artwork_urls` 按 release → release-group 排出 CAA 地址 |
| 已经刮过的歌补不回来 | 补图要重新搜索一遍才有候选 | 「补齐缺失封面」直接用 `scrape_state.candidates_json`，不发搜索请求 |
| 同一张专辑两种写法 | iTunes 给单曲/EP 加「 - Single」「 - EP」后缀 | `strip_release_suffix`，只削结尾（`Fantasy EP` 不动） |
| 专辑名变成罗马字 | MB 同一张专辑常有两条（「潜潜話」/「Hisohiso Banashi」） | 同分时优先和本地写法同一种文字 |
| 一排方图里夹一张 16:9 | CAA 上有的 release 传的是带字幕的 MV 截图 | `save_cover_any`：**方的优先**，非方的只作兜底 |
| 300px 的图在网格里发糊 | 旧的 `ARTWORK_SIZES` 按「界面最大 132px」定的 | 第一档提回 600；CAA 归到最近档（600→500 而不是 1200） |

**后两条改动会不会改掉已有的匹配，是量出来的，不是想出来的。**
`cargo run -p jp-app --example replay_ranking -- <corpus.db>` 拿库里存下的
候选池重放一遍排序：209 首里 **207 首赢家不变**，变的 2 首分数都**升**了——
本地标签写着单曲名（`形`、`プラトー`），去掉后缀之后正是那张单曲对上了，
原来赢的是同名的精选集/专辑版。

**一致性那 15 首带后缀的没有动。** 查了一下它们的后缀来自**用户自己文件的标签**
（`track_original_metadata.album` 就是 `怪獣 - Single`），不是刮削塞进去的，
而且去掉后缀也不会和任何已有专辑重名。改它等于改用户的原始 metadata——不改。
刮削这一端不再产生新的后缀就够了。

现在的覆盖（`examples/backfill_covers.rs` 跑完后实测）：

| | 覆盖 | 备注 |
|---|---|---|
| 曲目封面 | 209 / 209 | 全部 ≥ 500px，没有一张宽高比出格 |
| 专辑封面 | 111 / 111 | `fill_album_artwork` 用曲目封面填，不发网络请求 |
| 歌手照片 | 17 / 18 | 差的是月村手毬 |

月村手毬那张查得到为什么：MusicBrainz 把它登记成 **Character**（学园偶像大师的角色），
没有别名、没有 image 关系；它的 Wikidata 条目（Q134570629）没有 P18 图片；
Deezer 上根本没有这个艺人。三个源都没有，不是查法不对——留占位块。

## 人工确认

用户在复核界面选定一条候选，会做四件事：

1. 把候选的 metadata 补进 `songs`——**只补空字段**，不覆盖已有值
2. 把这首歌的信用标成 `source='manual'`
3. 候选里的演唱者如果还没有信用记录，补一条 `manual` 的
4. 下封面

第 2 条的含义：`source='manual'` 在这套 schema 里就是「人工版本，
自动流程不许动」（`add_credit` 和 Python 的回填都认这个）。用户是看着
本地值和候选并排比过才选的，这就是人工版本。

> 这一步之前是空的：只把 `scrape_state` 置成 success，`songs` 一个字
> 没写、封面也没下——点「采用」等于什么都没发生。界面现在会回报实际
> 写了什么（「补了 2 个字段，1 条信用标为人工，封面已下载」），
> 免得再出现「点了但看不出有没有生效」。

`fields_filled` 数的是**真的填上的字段数**，不是 `execute` 的返回值——
后者是受影响的行数，UPDATE 永远命中那一行，恒为 1。

## 还没做的

* 歌手的**合作曲拆分**只按斜杠。顿号和 × 会误伤乐队名，和 Python 侧
  的保守策略一致。
* 并发度写死 4。再高的收益要重新量——限流器的间隔是硬上限，
  而且也不该把对方的服务当自己的。
* 歌手照片只有 Deezer 一个源。MusicBrainz 的 image 关系、Wikidata 的 P18
  都能当第二源，但库里现在唯一缺照片的那位三个源都没有——真有第二位缺的时候再接。
