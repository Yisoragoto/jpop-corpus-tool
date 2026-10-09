# Rust Core

```
rust/
  Cargo.toml                    workspace
  crates/
    jp-tokenizer/               分词 + UniDic 词性 → UPOS
    jp-corpus/                  曲库 · 检索 · 语料 · 播放历史
```

两个 crate 都**不认识 UI、不认识 Tauri**——所有方法都是「进参数出结构体」，
可以离线测试。将来的 Tauri command 只是薄薄一层转发。

## 谁拥有 schema

**Python 侧拥有**（`legacy/scripts/migrate_db.py` + `library/schema.py` + `scraper/store.py`）。
`jp-corpus` **只读不建表**，缺表时 `check_schema()` 给出可操作的提示，
而不是让调用方撞上一个 `no such table`。

迁移期两边共用同一个 `corpus.db`，所以连接开了 WAL——Python 侧还会同时写它。

## 相对旧实现修掉的四件事

### 1. KWIC 的 N+1

`legacy/gui.py` 的 `SearchWorker` 每条结果都要再发一次
`SELECT surface FROM tokens WHERE utterance_id=?` 来重建整行，开了 cross_line
还要再发两次。搜一个常见词（128 条结果）就是 128~384 次额外查询。

`jp-corpus` 改成三次查询——命中一次、行内 token 一次、相邻行一次——
**与结果条数无关**。

实测同一查询（词元「夜」，128 条结果）：

```
Python SearchWorker   27.6 ms
Rust   Corpus::kwic    3.5 ms      8×
```

### 2. KWIC 丢空格

`tokens` 表里没有空白 token（分词时被丢掉了），所以拼接 surface 会把原句里的
空格吃掉：

```
原句   初めましての色が あることを
拼接   初めましての色があることを      ← 旧实现（Python）就是这样
```

日语歌词里的空格是有意的断句。新实现按原文偏移切，左右语境直接取原串子串，
空格原样保留。对不上时退回拼接法——宁可丢空格，也不要给出错位的语境。

### 3. 歌手筛选走实体

旧写法 `'/' || s.artist || '/' LIKE '%/' || ? || '/%'` 无法走索引，
合作曲靠拼字符串。现在按 `people.id` 过滤，走 `idx_credits_person`：

```sql
AND EXISTS (SELECT 1 FROM track_credits c
            WHERE c.song_id = u.song_id AND c.person_id IN (...))
```

### 4. FTS5 终于被用上

`utterances_fts`（trigram）一直建着、一直维护着，**旧 UI 从来没查过它**。
KWIC 走的是 tokens 表的精确匹配，补不上「只记得半句歌词」这种检索。

有一个坑：**trigram 对少于 3 个字符的查询一律返回 0 条**，而「夜」「恋」「君」
这类单字检索在日语里极其常见。静默返回空比报错更糟——用户会以为语料里
真的没有。所以短查询走 `LIKE` 兜底（8,443 行全扫只要几毫秒），
`rank` 返回 0 让前端知道这批结果没有相关性排序。

## 主要 API

```rust
let corpus = Corpus::open("corpus.db")?;
corpus.check_schema()?;

// 曲库：Artist → Album → Track
corpus.albums(None, 500)?;
corpus.album_tracks(album_id)?;
corpus.credits_for_track("001")?;          // 作词/作曲/编曲/演唱
corpus.collaborators(person_id, 50)?;      // Collaboration Graph 的边

// 检索
corpus.kwic(&KwicQuery { keywords: vec!["夜".into()], .. })?;
corpus.search_lyrics("深夜のコンビニ", 20)?;

// 语料 / Research Mode
corpus.overview()?;
corpus.word_in_corpus("夜", Some("001"), 20)?;   // 点词看全语料

// 歌词（一次两条 SQL 带回全部分词）
corpus.lyrics("001")?;
```

## 跑测试

```bash
cd rust && cargo test --release
```

`jp-corpus` 的集成测试**对着真实 `corpus.db` 跑**，不造假数据——这一层的价值
全在「SQL 对不对」上，内存里造几行能通过的查询在 209 首歌 / 58,628 个 token
上未必对（GROUP BY 漏字段、JOIN 放大、索引没走上，都只在真数据上暴露）。
库不存在或没迁移时整体跳过并提示要跑哪个命令。

看一眼实际效果：

```bash
cargo run -p jp-corpus --example demo --release -- 夜
```
