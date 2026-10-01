//! 词典库：独立的 SQLite 文件（应用里是 `dictionaries.db`，和 corpus.db 分开）。
//!
//! 表对应 Yomitan IndexedDB 的对象仓库（`dictionary-database.js`）：dictionaries / terms /
//! termMeta / tagMeta / media（kanji、kanjiMeta 暂不导入）。
//!
//! 和 Yomitan 不同的一点：释义不逐条存 JSON，而是按导入顺序拼成约 64 KB 一块、raw deflate 压缩，
//! 条目行上记块号、偏移、长度。真实词典抽样实测：逐条压缩只能压到原大小的 28–38%，
//! 64 KB 块能到 7–8%，解一块约 0.1 ms。条目行另记 `glossary_flags`（有无正文释义、有无「某词的变形」），
//! 查词时只有真正要显示或要做词典活用还原的条目才解压释义。
//!
//! 查询顺序刻意和 IndexedDB 一致：先按查询词顺序，再按「词形索引 → 读音索引」，同键按主键升序。
//! Yomitan 最后的排序在各项都相等时保留输入顺序，顺序不同，结果就不同。

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use flate2::read::DeflateDecoder;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

pub const SCHEMA_VERSION: &str = "1";
pub const GLOSSARY_CODEC: &str = "deflate-raw; block≈65536; json array per term";

/// `glossary_flags` 位：释义里有正文（字符串 / 结构化内容 / 图片）
pub const GLOSSARY_HAS_CONTENT: i64 = 1;
/// `glossary_flags` 位：释义里有 `[原形, [活用规则…]]` 这种「某词的变形」
pub const GLOSSARY_HAS_FORM_OF: i64 = 2;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS dictionaries (
    id             INTEGER PRIMARY KEY,
    title          TEXT NOT NULL UNIQUE,
    revision       TEXT NOT NULL,
    version        INTEGER NOT NULL,
    sequenced      INTEGER NOT NULL,
    frequency_mode TEXT,
    index_json     TEXT NOT NULL,
    styles         TEXT NOT NULL DEFAULT '',
    counts_json    TEXT NOT NULL DEFAULT '{}',
    warnings_json  TEXT NOT NULL DEFAULT '[]',
    source_path    TEXT NOT NULL DEFAULT '',
    enabled        INTEGER NOT NULL DEFAULT 1,
    priority       INTEGER NOT NULL,
    imported_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS terms (
    id              INTEGER PRIMARY KEY,
    dictionary_id   INTEGER NOT NULL,
    expression      TEXT NOT NULL,
    reading         TEXT NOT NULL,
    definition_tags TEXT NOT NULL,
    rules           TEXT NOT NULL,
    score           REAL NOT NULL,
    sequence        INTEGER,
    term_tags       TEXT NOT NULL,
    glossary_flags  INTEGER NOT NULL,
    block_id        INTEGER NOT NULL,
    block_offset    INTEGER NOT NULL,
    block_len       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS terms_expression ON terms(expression);
CREATE INDEX IF NOT EXISTS terms_reading ON terms(reading);
CREATE TABLE IF NOT EXISTS glossary_blocks (
    id            INTEGER PRIMARY KEY,
    dictionary_id INTEGER NOT NULL,
    data          BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS term_meta (
    id            INTEGER PRIMARY KEY,
    dictionary_id INTEGER NOT NULL,
    expression    TEXT NOT NULL,
    mode          TEXT NOT NULL,
    data          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS term_meta_expression ON term_meta(expression);
CREATE TABLE IF NOT EXISTS tags (
    id            INTEGER PRIMARY KEY,
    dictionary_id INTEGER NOT NULL,
    name          TEXT NOT NULL,
    category      TEXT,
    sort_order    REAL,
    notes         TEXT,
    score         REAL
);
CREATE INDEX IF NOT EXISTS tags_name ON tags(name);
CREATE TABLE IF NOT EXISTS media (
    id            INTEGER PRIMARY KEY,
    dictionary_id INTEGER NOT NULL,
    path          TEXT NOT NULL,
    media_type    TEXT NOT NULL,
    width         INTEGER NOT NULL,
    height        INTEGER NOT NULL,
    data          BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS media_path ON media(path);
"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryInfo {
    pub id: i64,
    pub title: String,
    pub revision: String,
    pub version: i64,
    pub sequenced: bool,
    pub frequency_mode: Option<String>,
    pub author: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub attribution: Option<String>,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub counts: Value,
    pub warnings: Vec<String>,
    pub has_styles: bool,
    pub source_path: String,
    pub enabled: bool,
    pub priority: i64,
    pub imported_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchSource {
    Term,
    Reading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlossaryRef {
    pub block_id: i64,
    pub offset: usize,
    pub len: usize,
}

/// 对应 Yomitan `_createTerm` 产出的 `TermEntry`（释义延后读取）。
#[derive(Debug, Clone)]
pub struct TermRow {
    pub id: i64,
    /// 查询词在请求列表里的下标
    pub index: usize,
    pub match_source: MatchSource,
    pub term: String,
    pub reading: String,
    pub definition_tags: Vec<String>,
    pub term_tags: Vec<String>,
    pub rules: Vec<String>,
    pub score: f64,
    pub dictionary_id: i64,
    /// 没有时为 -1
    pub sequence: i64,
    pub glossary_flags: i64,
    pub glossary: GlossaryRef,
}

#[derive(Debug, Clone)]
pub struct TermMetaRow {
    pub index: usize,
    pub term: String,
    pub mode: String,
    pub data: Value,
    pub dictionary_id: i64,
}

#[derive(Debug, Clone, Default)]
pub struct TagRow {
    pub category: Option<String>,
    pub order: Option<f64>,
    pub notes: Option<String>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MediaRow {
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// JS `_splitField`：非空字符串按单个空格切（连续空格会切出空串，照做）。
pub(crate) fn split_field(field: &str) -> Vec<String> {
    if field.is_empty() { Vec::new() } else { field.split(' ').map(str::to_owned).collect() }
}

/// 解压后的释义块缓存多少个（64 KB 一块，最多约 16 MB）
const BLOCK_CACHE_SIZE: usize = 256;

/// 在几个线程上并行做 `f`，结果保持原顺序。量少时直接串行，省掉起线程的开销。
fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> Result<R> + Sync) -> Result<Vec<R>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    if threads <= 1 || items.len() < 8 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    let f = &f;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(f).collect::<Result<Vec<R>>>()))
            .collect();
        let mut out = Vec::with_capacity(items.len());
        for handle in handles {
            out.extend(handle.join().map_err(|_| anyhow::anyhow!("解压线程崩溃"))??);
        }
        Ok(out)
    })
}

pub struct DictionaryStore {
    pub(crate) conn: Connection,
    blocks: Mutex<VecDeque<(i64, Arc<Vec<u8>>)>>,
}

impl DictionaryStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("打不开词典库 {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // 导入一本大词典是一个几百 MB 的事务。WAL 下读者不被写者挡住，导入期间界面照常查词；
        // 导入作业和界面各开一条连接，偶尔撞锁时等一会儿而不是立刻报错。
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        // WAL 文件在检查点之后截到 64 MB 以内，不长期占着几百 MB 磁盘
        conn.query_row("PRAGMA journal_size_limit = 67108864", [], |_| Ok(()))?;
        // 导入时两个文本索引要插几十万行，默认 2 MB 页缓存会反复换页
        conn.execute_batch("PRAGMA cache_size = -65536;")?;
        conn.execute_batch(SCHEMA)?;
        let version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
            .optional()?;
        match version.as_deref() {
            None => {
                conn.execute(
                    "INSERT INTO meta(key, value) VALUES ('schema_version', ?1), ('glossary_codec', ?2)",
                    params![SCHEMA_VERSION, GLOSSARY_CODEC],
                )?;
            }
            Some(SCHEMA_VERSION) => {}
            Some(other) => bail!("词典库格式版本是 {other}，本程序只认识 {SCHEMA_VERSION}"),
        }
        Ok(Self { conn, blocks: Mutex::new(VecDeque::new()) })
    }

    pub fn title_exists(&self, title: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM dictionaries WHERE title = ?1", [title], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// 按优先级（界面上的先后）排列。
    pub fn dictionaries(&self) -> Result<Vec<DictionaryInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, revision, version, sequenced, frequency_mode, index_json, styles <> '',
                    counts_json, warnings_json, source_path, enabled, priority, imported_at
             FROM dictionaries ORDER BY priority, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, bool>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, bool>(11)?,
                r.get::<_, i64>(12)?,
                r.get::<_, i64>(13)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, title, revision, version, sequenced, frequency_mode, index_json, has_styles, counts, warnings, source_path, enabled, priority, imported_at) = row?;
            let index: Value = serde_json::from_str(&index_json).unwrap_or(Value::Null);
            let text = |key: &str| index.get(key).and_then(Value::as_str).map(str::to_owned);
            out.push(DictionaryInfo {
                id,
                author: text("author"),
                url: text("url"),
                description: text("description"),
                attribution: text("attribution"),
                source_language: text("sourceLanguage"),
                target_language: text("targetLanguage"),
                title,
                revision,
                version,
                sequenced,
                frequency_mode,
                counts: serde_json::from_str(&counts).unwrap_or(Value::Null),
                warnings: serde_json::from_str(&warnings).unwrap_or_default(),
                has_styles,
                source_path,
                enabled,
                priority,
                imported_at,
            });
        }
        Ok(out)
    }

    pub fn set_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let n = self.conn.execute("UPDATE dictionaries SET enabled = ?1 WHERE id = ?2", params![enabled, id])?;
        if n == 0 {
            bail!("没有 id 为 {id} 的词典");
        }
        Ok(())
    }

    /// `ids` 是界面上从上到下的新顺序；没列出的词典排在后面、保持原有先后。
    pub fn set_order(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        let existing: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT id FROM dictionaries ORDER BY priority, id")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let mut order: Vec<i64> = ids.iter().copied().filter(|id| existing.contains(id)).collect();
        for id in existing {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        for (priority, id) in order.iter().enumerate() {
            tx.execute("UPDATE dictionaries SET priority = ?1 WHERE id = ?2", params![priority as i64, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&mut self, id: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        for table in ["terms", "glossary_blocks", "term_meta", "tags", "media"] {
            tx.execute(&format!("DELETE FROM {table} WHERE dictionary_id = ?1"), [id])?;
        }
        if tx.execute("DELETE FROM dictionaries WHERE id = ?1", [id])? == 0 {
            bail!("没有 id 为 {id} 的词典");
        }
        tx.commit()?;
        self.blocks.lock().map_err(|_| anyhow::anyhow!("释义块缓存锁已损坏"))?.clear();
        Ok(())
    }

    pub fn styles(&self, id: i64) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT styles FROM dictionaries WHERE id = ?1", [id], |r| r.get(0))
            .optional()?
            .unwrap_or_default())
    }

    /// 对应 Yomitan `findTermsBulk(termList, dictionaries, 'exact')`。
    pub fn find_terms_bulk(&self, terms: &[String], enabled: &HashSet<i64>) -> Result<Vec<TermRow>> {
        const COLUMNS: &str = "id, dictionary_id, expression, reading, definition_tags, rules, score, sequence,
                               term_tags, glossary_flags, block_id, block_offset, block_len";
        let mut by_term = self.conn.prepare_cached(&format!("SELECT {COLUMNS} FROM terms WHERE expression = ?1 ORDER BY id"))?;
        let mut by_reading = self.conn.prepare_cached(&format!("SELECT {COLUMNS} FROM terms WHERE reading = ?1 ORDER BY id"))?;
        let mut visited = HashSet::new();
        let mut out = Vec::new();
        for (index, term) in terms.iter().enumerate() {
            for (stmt, match_source) in [(&mut by_term, MatchSource::Term), (&mut by_reading, MatchSource::Reading)] {
                let rows = stmt.query_map([term], |r| {
                    Ok(TermRow {
                        id: r.get(0)?,
                        dictionary_id: r.get(1)?,
                        term: r.get(2)?,
                        reading: r.get(3)?,
                        definition_tags: split_field(&r.get::<_, String>(4)?),
                        rules: split_field(&r.get::<_, String>(5)?),
                        score: r.get(6)?,
                        sequence: r.get::<_, Option<i64>>(7)?.unwrap_or(-1),
                        term_tags: split_field(&r.get::<_, String>(8)?),
                        glossary_flags: r.get(9)?,
                        glossary: GlossaryRef {
                            block_id: r.get(10)?,
                            offset: r.get::<_, i64>(11)? as usize,
                            len: r.get::<_, i64>(12)? as usize,
                        },
                        index,
                        match_source,
                    })
                })?;
                for row in rows {
                    let row = row?;
                    // 和 JS 的谓词顺序一样：先看词典是否启用，再记「见过」
                    if enabled.contains(&row.dictionary_id) && visited.insert(row.id) {
                        out.push(row);
                    }
                }
            }
        }
        Ok(out)
    }

    /// 对应 Yomitan `findTermMetaBulk`。
    pub fn find_term_meta_bulk(&self, terms: &[String], enabled: &HashSet<i64>) -> Result<Vec<TermMetaRow>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT dictionary_id, mode, data FROM term_meta WHERE expression = ?1 ORDER BY id")?;
        let mut out = Vec::new();
        for (index, term) in terms.iter().enumerate() {
            let rows = stmt.query_map([term], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            for row in rows {
                let (dictionary_id, mode, data) = row?;
                if !enabled.contains(&dictionary_id) {
                    continue;
                }
                let data = serde_json::from_str(&data).with_context(|| format!("「{term}」的 {mode} 数据损坏"))?;
                out.push(TermMetaRow { index, term: term.clone(), mode, data, dictionary_id });
            }
        }
        Ok(out)
    }

    /// 对应 Yomitan `findTagMetaBulk` 的单项：按名字找该词典里第一个插入的标签。
    pub fn find_tag(&self, dictionary_id: i64, name: &str) -> Result<Option<TagRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT category, sort_order, notes, score FROM tags WHERE name = ?1 AND dictionary_id = ?2 ORDER BY id LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![name, dictionary_id], |r| {
                Ok(TagRow { category: r.get(0)?, order: r.get(1)?, notes: r.get(2)?, score: r.get(3)? })
            })
            .optional()?)
    }

    fn block(&self, block_id: i64) -> Result<Arc<Vec<u8>>> {
        let mut cache = self.blocks.lock().map_err(|_| anyhow::anyhow!("释义块缓存锁已损坏"))?;
        if let Some((_, data)) = cache.iter().find(|(id, _)| *id == block_id) {
            return Ok(Arc::clone(data));
        }
        let compressed: Vec<u8> = self
            .conn
            .query_row("SELECT data FROM glossary_blocks WHERE id = ?1", [block_id], |r| r.get(0))
            .optional()?
            .with_context(|| format!("释义块 {block_id} 不存在，词典库可能已损坏"))?;
        let mut raw = Vec::with_capacity(compressed.len() * 8);
        DeflateDecoder::new(&compressed[..])
            .read_to_end(&mut raw)
            .with_context(|| format!("释义块 {block_id} 解压失败，词典库可能已损坏"))?;
        let data = Arc::new(raw);
        if cache.len() >= BLOCK_CACHE_SIZE {
            cache.pop_front();
        }
        cache.push_back((block_id, Arc::clone(&data)));
        Ok(data)
    }

    /// 批量取释义，结果和 `refs` 一一对应。
    ///
    /// 先按块把压缩数据读出来（SQLite 连接只能在一个线程上用），再多线程解压、解析。
    /// 一次查词要读上百条释义，逐条串行解压曾占查词九成的时间（`examples/lookup_bench.rs`）。
    pub fn glossaries(&self, refs: &[GlossaryRef]) -> Result<Vec<Vec<Value>>> {
        let mut block_ids: Vec<i64> = refs.iter().map(|r| r.block_id).collect();
        block_ids.sort_unstable();
        block_ids.dedup();

        let mut blocks: HashMap<i64, Arc<Vec<u8>>> = HashMap::new();
        {
            let cache = self.blocks.lock().map_err(|_| anyhow::anyhow!("释义块缓存锁已损坏"))?;
            for (id, data) in cache.iter() {
                if block_ids.binary_search(id).is_ok() {
                    blocks.insert(*id, Arc::clone(data));
                }
            }
        }

        let mut compressed: Vec<(i64, Vec<u8>)> = Vec::new();
        {
            let mut stmt = self.conn.prepare_cached("SELECT data FROM glossary_blocks WHERE id = ?1")?;
            for &id in block_ids.iter().filter(|id| !blocks.contains_key(id)) {
                let data: Vec<u8> = stmt
                    .query_row([id], |r| r.get(0))
                    .optional()?
                    .with_context(|| format!("释义块 {id} 不存在，词典库可能已损坏"))?;
                compressed.push((id, data));
            }
        }

        let decompressed = parallel_map(&compressed, |(id, data)| {
            let mut raw = Vec::with_capacity(data.len() * 8);
            DeflateDecoder::new(&data[..])
                .read_to_end(&mut raw)
                .with_context(|| format!("释义块 {id} 解压失败，词典库可能已损坏"))?;
            Ok((*id, Arc::new(raw)))
        })?;
        if !decompressed.is_empty() {
            let mut cache = self.blocks.lock().map_err(|_| anyhow::anyhow!("释义块缓存锁已损坏"))?;
            for (id, data) in decompressed {
                if cache.len() >= BLOCK_CACHE_SIZE {
                    cache.pop_front();
                }
                cache.push_back((id, Arc::clone(&data)));
                blocks.insert(id, data);
            }
        }

        parallel_map(refs, |r| {
            let block = blocks.get(&r.block_id).context("释义块缺失")?;
            let slice = block.get(r.offset..r.offset + r.len).context("释义偏移越界，词典库可能已损坏")?;
            serde_json::from_slice(slice).context("释义 JSON 损坏")
        })
    }

    /// 取一条的释义（导入时整理过的 Yomitan 释义数组）。
    pub fn glossary(&self, glossary: GlossaryRef) -> Result<Vec<Value>> {
        let block = self.block(glossary.block_id)?;
        let slice = block
            .get(glossary.offset..glossary.offset + glossary.len)
            .context("释义偏移越界，词典库可能已损坏")?;
        serde_json::from_slice(slice).context("释义 JSON 损坏")
    }

    pub fn media(&self, dictionary_id: i64, path: &str) -> Result<Option<MediaRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT media_type, width, height, data FROM media WHERE path = ?1 AND dictionary_id = ?2 ORDER BY id LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![path, dictionary_id], |r| {
                Ok(MediaRow { media_type: r.get(0)?, width: r.get(1)?, height: r.get(2)?, data: r.get(3)? })
            })
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_field_matches_js_split_on_a_single_space() {
        assert!(split_field("").is_empty());
        assert_eq!(split_field("n vt"), ["n", "vt"]);
        assert_eq!(split_field("a  b"), ["a", "", "b"]);
    }

    #[test]
    fn a_store_from_a_newer_version_is_refused() {
        let store = DictionaryStore::open_in_memory().unwrap();
        store.conn.execute("UPDATE meta SET value = '99' WHERE key = 'schema_version'", []).unwrap();
        let conn = std::mem::replace(&mut { store }.conn, Connection::open_in_memory().unwrap());
        let err = DictionaryStore::init(conn).err().expect("应当拒绝");
        assert!(err.to_string().contains("99"), "{err}");
    }
}
