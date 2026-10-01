//! 词典导入：Yomitan `dictionary-importer.js` 的移植（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）。
//!
//! 和 Yomitan 一致的地方：
//! - 只认根目录下的 `term_bank_N.json` 等文件，按归档里的顺序处理（决定条目 id 先后）；
//! - V1 / V3 两种条目格式，读音为空时用词形；
//! - 释义里的 `{type:"text"}` 变成纯字符串；图片（含结构化内容里的 `img`）只保留 Yomitan 认的属性，
//!   原来的 `width`/`height` 改名为 `preferredWidth`/`preferredHeight`，再补上图片实际宽高；
//!   只存被释义引用到的图片；
//! - 旧格式 `index.json` 里的 `tagMeta` 跟在每个 tag_bank 后面追加一遍（Yomitan 就是这么做的）。
//!
//! 有意不同的地方：
//! - 整本词典在一个事务里导入，失败什么都不留（Yomitan 出错时会留下半本）；
//! - 引用的图片找不到、类型不认识时记一条警告继续导入（Yomitan 整本报错）——
//!   用户手上的词典包是旧版导入过的，不能因为一张外字图片整本导不进来；
//! - 汉字词典（kanji_bank / kanji_meta_bank）暂不导入，只计数。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::write::DeflateEncoder;
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};

use crate::media;
use crate::store::{DictionaryStore, GLOSSARY_HAS_CONTENT, GLOSSARY_HAS_FORM_OF};
use crate::zip::ZipArchive;

/// 释义块攒到这么大就压缩落盘（见 `store` 模块的实测说明）。
pub const BLOCK_TARGET_BYTES: usize = 64 * 1024;

/// 量图片宽高：`(文件内容, 媒体类型) -> (宽, 高)`
pub type ImageSizeFn = fn(&[u8], &str) -> Option<(u32, u32)>;
const MAX_WARNINGS: usize = 50;

pub trait Archive {
    /// 归档里的文件名（不含目录项），按归档自身的顺序
    fn file_names(&self) -> Vec<String>;
    fn read(&mut self, name: &str) -> Result<Vec<u8>>;
}

impl Archive for ZipArchive {
    fn file_names(&self) -> Vec<String> {
        self.entries().iter().filter(|e| !e.name.ends_with('/')).map(|e| e.name.clone()).collect()
    }

    fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        ZipArchive::read(self, name)
    }
}

/// 解压开的词典目录（测试夹具就是这种形态）。文件名按名字排序，和 Yomitan 测试打包时的 readdir 顺序一致。
pub struct DirArchive {
    root: PathBuf,
    names: Vec<String>,
}

impl DirArchive {
    pub fn open(root: &Path) -> Result<Self> {
        fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
            let mut entries = std::fs::read_dir(dir)
                .with_context(|| format!("读不了目录 {}", dir.display()))?
                .collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                if entry.file_type()?.is_dir() {
                    walk(&entry.path(), &rel, out)?;
                } else {
                    out.push(rel);
                }
            }
            Ok(())
        }
        let mut names = Vec::new();
        walk(root, "", &mut names)?;
        Ok(Self { root: root.to_owned(), names })
    }
}

impl Archive for DirArchive {
    fn file_names(&self) -> Vec<String> {
        self.names.clone()
    }

    fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        std::fs::read(self.root.join(name)).with_context(|| format!("读不了 {name}"))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub file: String,
    pub done_files: usize,
    pub total_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub dictionary_id: i64,
    pub title: String,
    pub revision: String,
    pub version: i64,
    pub terms: usize,
    /// `total` 加各 mode（freq / pitch / ipa）的条数，和 Yomitan summary 的 counts.termMeta 同形
    pub term_meta: BTreeMap<String, usize>,
    pub tags: usize,
    pub media: usize,
    pub kanji_skipped: usize,
    pub glossary_bytes: usize,
    pub compressed_bytes: usize,
    pub warnings: Vec<String>,
    pub warning_count: usize,
    pub elapsed_ms: u64,
}

pub struct ImportOptions {
    pub source_path: String,
    /// 图片宽高怎么量。默认读文件头；Yomitan 的测试用固定 100×100，对账时传同样的函数。
    pub image_size: ImageSizeFn,
    /// 置位后在下一个文件或下一个条目处中止；整本在一个事务里，中止即回滚，库里不留半本。
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { source_path: String::new(), image_size: media::image_dimensions, cancel: None }
    }
}

pub const CANCELLED: &str = "导入已取消";

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// `^{prefix}(\d+)\.json$`（JS 不带 u 标志的 `\d` 只认 ASCII 数字）
fn bank_files(names: &[String], prefix: &str) -> Vec<String> {
    names
        .iter()
        .filter(|n| {
            n.strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(".json"))
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        })
        .cloned()
        .collect()
}

#[derive(Deserialize)]
struct TermV3<'a>(
    String,
    String,
    Option<String>,
    String,
    f64,
    #[serde(borrow)] &'a RawValue,
    i64,
    String,
);

#[derive(Deserialize)]
struct TermMetaV3<'a>(String, String, #[serde(borrow)] &'a RawValue);

struct PendingTerm {
    expression: String,
    reading: String,
    definition_tags: String,
    rules: String,
    score: f64,
    sequence: Option<i64>,
    term_tags: String,
    flags: i64,
    offset: usize,
    len: usize,
}

struct BlockWriter {
    dictionary_id: i64,
    buf: Vec<u8>,
    pending: Vec<PendingTerm>,
    terms: usize,
    raw_bytes: usize,
    compressed_bytes: usize,
}

impl BlockWriter {
    fn new(dictionary_id: i64) -> Self {
        Self {
            dictionary_id,
            buf: Vec::with_capacity(BLOCK_TARGET_BYTES * 2),
            pending: Vec::new(),
            terms: 0,
            raw_bytes: 0,
            compressed_bytes: 0,
        }
    }

    fn push(&mut self, tx: &Transaction<'_>, mut term: PendingTerm, glossary_json: &[u8]) -> Result<()> {
        term.offset = self.buf.len();
        term.len = glossary_json.len();
        self.buf.extend_from_slice(glossary_json);
        self.pending.push(term);
        self.terms += 1;
        if self.buf.len() >= BLOCK_TARGET_BYTES {
            self.flush(tx)?;
        }
        Ok(())
    }

    fn flush(&mut self, tx: &Transaction<'_>) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let mut encoder = DeflateEncoder::new(Vec::with_capacity(self.buf.len() / 6), Compression::default());
        encoder.write_all(&self.buf)?;
        let compressed = encoder.finish()?;
        tx.prepare_cached("INSERT INTO glossary_blocks(dictionary_id, data) VALUES (?1, ?2)")?
            .execute(params![self.dictionary_id, compressed])?;
        let block_id = tx.last_insert_rowid();
        let mut stmt = tx.prepare_cached(
            "INSERT INTO terms(dictionary_id, expression, reading, definition_tags, rules, score, sequence,
                               term_tags, glossary_flags, block_id, block_offset, block_len)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for t in self.pending.drain(..) {
            stmt.execute(params![
                self.dictionary_id,
                t.expression,
                t.reading,
                t.definition_tags,
                t.rules,
                t.score,
                t.sequence,
                t.term_tags,
                t.flags,
                block_id,
                t.offset as i64,
                t.len as i64
            ])?;
        }
        self.raw_bytes += self.buf.len();
        self.compressed_bytes += compressed.len();
        self.buf.clear();
        Ok(())
    }
}

struct Warnings {
    list: Vec<String>,
    count: usize,
}

impl Warnings {
    fn push(&mut self, message: String) {
        self.count += 1;
        if self.list.len() < MAX_WARNINGS {
            self.list.push(message);
        }
    }
}

/// 释义整理时需要的上下文：读图片、存图片、记警告。
struct GlossaryContext<'a, 't> {
    archive: &'a mut dyn Archive,
    tx: &'a Transaction<'t>,
    names: &'a HashSet<String>,
    dictionary_id: i64,
    image_size: ImageSizeFn,
    /// path → 宽高；存过或确认有问题的都记下，同一路径只处理一次
    media: HashMap<String, (u32, u32)>,
    media_count: usize,
    warnings: &'a mut Warnings,
    expression: String,
    reading: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageKind {
    Glossary,
    StructuredContent,
}

impl GlossaryContext<'_, '_> {
    fn image(&mut self, path: &str) -> Result<(u32, u32)> {
        if let Some(&dims) = self.media.get(path) {
            return Ok(dims);
        }
        let dims = self.load_image(path)?;
        self.media.insert(path.to_owned(), dims);
        Ok(dims)
    }

    fn load_image(&mut self, path: &str) -> Result<(u32, u32)> {
        let reading = if self.reading.is_empty() { String::new() } else { format!("（{}）", self.reading) };
        let Some(media_type) = media::image_media_type_from_file_name(path) else {
            self.warnings.push(format!("图片类型不认识：{path}，见「{}」{reading}", self.expression));
            return Ok((0, 0));
        };
        if !self.names.contains(path) {
            self.warnings.push(format!("找不到图片 {path}，见「{}」{reading}", self.expression));
            return Ok((0, 0));
        }
        let data = self.archive.read(path)?;
        // 读不出尺寸记 0：界面按图片自身大小显示
        let (width, height) = (self.image_size)(&data, media_type).unwrap_or((0, 0));
        self.tx
            .prepare_cached(
                "INSERT INTO media(dictionary_id, path, media_type, width, height, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![self.dictionary_id, path, media_type, width, height, data])?;
        self.media_count += 1;
        Ok((width, height))
    }

    /// JS `_createImageData` + `_resolveStructuredContentImage`
    fn format_image(&mut self, source: &Map<String, Value>, kind: ImageKind) -> Result<Value> {
        let path = source.get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
        let (width, height) = self.image(&path)?;
        let mut target = Map::new();
        match kind {
            ImageKind::Glossary => target.insert("type".into(), json!("image")),
            ImageKind::StructuredContent => target.insert("tag".into(), json!("img")),
        };
        target.insert("path".into(), json!(path));
        target.insert("width".into(), json!(width));
        target.insert("height".into(), json!(height));
        let mut copy = |from: &str, to: &str, ok: fn(&Value) -> bool| {
            if let Some(v) = source.get(from).filter(|v| ok(v)) {
                target.insert(to.into(), v.clone());
            }
        };
        copy("width", "preferredWidth", Value::is_number);
        copy("height", "preferredHeight", Value::is_number);
        copy("title", "title", Value::is_string);
        copy("alt", "alt", Value::is_string);
        copy("description", "description", Value::is_string);
        copy("pixelated", "pixelated", Value::is_boolean);
        copy("imageRendering", "imageRendering", Value::is_string);
        copy("appearance", "appearance", Value::is_string);
        copy("background", "background", Value::is_boolean);
        copy("collapsed", "collapsed", Value::is_boolean);
        copy("collapsible", "collapsible", Value::is_boolean);
        if kind == ImageKind::StructuredContent {
            copy("verticalAlign", "verticalAlign", Value::is_string);
            copy("border", "border", Value::is_string);
            copy("borderRadius", "borderRadius", Value::is_string);
            copy("sizeUnits", "sizeUnits", Value::is_string);
        }
        Ok(Value::Object(target))
    }

    /// JS `_prepareStructuredContent`
    fn prepare_structured_content(&mut self, content: Value) -> Result<Value> {
        match content {
            Value::Array(items) => Ok(Value::Array(
                items.into_iter().map(|c| self.prepare_structured_content(c)).collect::<Result<_>>()?,
            )),
            Value::Object(mut map) => {
                if map.get("tag").and_then(Value::as_str) == Some("img") {
                    return self.format_image(&map, ImageKind::StructuredContent);
                }
                if let Some(child) = map.remove("content") {
                    let prepared = self.prepare_structured_content(child)?;
                    map.insert("content".into(), prepared);
                }
                Ok(Value::Object(map))
            }
            other => Ok(other),
        }
    }

    /// JS `_formatDictionaryTermGlossaryObject`
    fn format_glossary_object(&mut self, map: Map<String, Value>) -> Result<Value> {
        match map.get("type").and_then(Value::as_str) {
            Some("text") => Ok(map.get("text").cloned().unwrap_or(Value::Null)),
            Some("image") => self.format_image(&map, ImageKind::Glossary),
            Some("structured-content") => {
                let content = map.get("content").cloned().unwrap_or(Value::Null);
                Ok(json!({"type": "structured-content", "content": self.prepare_structured_content(content)?}))
            }
            other => bail!("「{}」的释义里有不认识的类型 {other:?}", self.expression),
        }
    }

    /// 整理一条释义数组，返回整理后的 JSON 和 `glossary_flags`。
    fn format_glossary(&mut self, glossary: Vec<Value>) -> Result<(Vec<u8>, i64)> {
        let mut flags = 0;
        let mut out = Vec::with_capacity(glossary.len());
        for item in glossary {
            let item = match item {
                Value::Object(map) => self.format_glossary_object(map)?,
                other => other,
            };
            flags |= if item.is_array() { GLOSSARY_HAS_FORM_OF } else { GLOSSARY_HAS_CONTENT };
            out.push(item);
        }
        Ok((serde_json::to_vec(&out)?, flags))
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn insert_tag(
    tx: &Transaction<'_>,
    dictionary_id: i64,
    name: &str,
    category: Option<&Value>,
    order: Option<&Value>,
    notes: Option<&Value>,
    score: Option<&Value>,
) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO tags(dictionary_id, name, category, sort_order, notes, score) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        dictionary_id,
        name,
        category.and_then(Value::as_str),
        order.and_then(Value::as_f64),
        notes.and_then(Value::as_str),
        score.and_then(Value::as_f64)
    ])?;
    Ok(())
}

pub fn import_dictionary(
    store: &mut DictionaryStore,
    archive: &mut dyn Archive,
    options: &ImportOptions,
    progress: &mut dyn FnMut(&ImportProgress),
) -> Result<ImportSummary> {
    let started = Instant::now();
    let check_cancel = || -> Result<()> {
        if options.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            bail!(CANCELLED);
        }
        Ok(())
    };
    let names = archive.file_names();
    if !names.iter().any(|n| n == "index.json") {
        if let Some(nested) = names.iter().find(|n| n.ends_with("/index.json")) {
            bail!(
                "index.json 嵌在多余的目录「{}」里，它必须在压缩包根目录",
                &nested[..nested.len() - "index.json".len()]
            );
        }
        bail!("压缩包里没有 index.json，不是 Yomitan 词典");
    }

    let index_bytes = archive.read("index.json")?;
    let index: Value = serde_json::from_slice(strip_bom(&index_bytes)).context("index.json 不是合法的 JSON")?;
    let index_obj = index.as_object().context("index.json 应当是一个对象")?;
    let version = match index_obj.get("format") {
        Some(v) if v.is_number() => v.as_i64(),
        _ => index_obj.get("version").and_then(Value::as_i64),
    }
    .context("无法识别的词典格式（index.json 里没有 format / version）")?;
    let title = index_obj
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .context("index.json 里没有 title")?
        .to_owned();
    let revision = index_obj
        .get("revision")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .context("index.json 里没有 revision")?
        .to_owned();
    if store.title_exists(&title)? {
        bail!("「{title}」已经导入过了");
    }
    let sequenced = index_obj.get("sequenced").and_then(Value::as_bool).unwrap_or(false);
    let frequency_mode = index_obj.get("frequencyMode").and_then(Value::as_str).map(str::to_owned);

    let term_files = bank_files(&names, "term_bank_");
    let term_meta_files = bank_files(&names, "term_meta_bank_");
    let kanji_files = bank_files(&names, "kanji_bank_");
    let kanji_meta_files = bank_files(&names, "kanji_meta_bank_");
    let tag_files = bank_files(&names, "tag_bank_");
    let total_files = term_files.len() + term_meta_files.len() + tag_files.len() + kanji_files.len() + kanji_meta_files.len();
    let name_set: HashSet<String> = names.iter().cloned().collect();

    let styles = if name_set.contains("styles.css") {
        String::from_utf8_lossy(strip_bom(&archive.read("styles.css")?)).into_owned()
    } else {
        String::new()
    };

    let tx = store.conn.transaction()?;
    let priority: i64 = tx.query_row("SELECT COALESCE(MAX(priority) + 1, 0) FROM dictionaries", [], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO dictionaries(title, revision, version, sequenced, frequency_mode, index_json, styles, source_path, enabled, priority, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10)",
        params![title, revision, version, sequenced, frequency_mode, serde_json::to_string(&index)?, styles, options.source_path, priority, now_ms()],
    )?;
    let dictionary_id = tx.last_insert_rowid();

    let mut done_files = 0;
    let mut warnings = Warnings { list: Vec::new(), count: 0 };
    let mut writer = BlockWriter::new(dictionary_id);
    let mut media_cache = HashMap::new();
    let mut media_count = 0;

    for name in &term_files {
        check_cancel()?;
        progress(&ImportProgress { file: name.clone(), done_files, total_files });
        let bytes = archive.read(name)?;
        let mut ctx = GlossaryContext {
            archive: &mut *archive,
            tx: &tx,
            names: &name_set,
            dictionary_id,
            image_size: options.image_size,
            media: std::mem::take(&mut media_cache),
            media_count,
            warnings: &mut warnings,
            expression: String::new(),
            reading: String::new(),
        };
        let add = |ctx: &mut GlossaryContext<'_, '_>,
                       writer: &mut BlockWriter,
                       expression: String,
                       reading: String,
                       definition_tags: Option<String>,
                       rules: String,
                       score: f64,
                       glossary: Vec<Value>,
                       sequence: Option<i64>,
                       term_tags: String|
         -> Result<()> {
            let reading = if reading.is_empty() { expression.clone() } else { reading };
            ctx.expression.clone_from(&expression);
            ctx.reading.clone_from(&reading);
            let (json, flags) = ctx.format_glossary(glossary)?;
            writer.push(
                ctx.tx,
                PendingTerm {
                    expression,
                    reading,
                    definition_tags: definition_tags.unwrap_or_default(),
                    rules,
                    score,
                    sequence,
                    term_tags,
                    flags,
                    offset: 0,
                    len: 0,
                },
                &json,
            )
        };
        if version == 1 {
            let rows: Vec<Vec<Value>> =
                serde_json::from_slice(strip_bom(&bytes)).with_context(|| format!("{name} 不是合法的 JSON"))?;
            for row in rows {
                check_cancel()?;
                let mut it = row.into_iter();
                let mut text = |what: &str| -> Result<String> {
                    match it.next() {
                        Some(Value::String(s)) => Ok(s),
                        other => bail!("{name} 里的条目 {what} 应当是字符串，实际是 {other:?}"),
                    }
                };
                let expression = text("词形")?;
                let reading = text("读音")?;
                let definition_tags = Some(text("释义标签")?);
                let rules = text("活用规则")?;
                let score = it.next().and_then(|v| v.as_f64()).with_context(|| format!("{name} 里「{expression}」的 score 不是数字"))?;
                let glossary: Vec<Value> = it.collect();
                add(&mut ctx, &mut writer, expression, reading, definition_tags, rules, score, glossary, None, String::new())?;
            }
        } else {
            let rows: Vec<TermV3<'_>> =
                serde_json::from_slice(strip_bom(&bytes)).with_context(|| format!("{name} 格式不对（不是 V3 条目数组）"))?;
            for TermV3(expression, reading, definition_tags, rules, score, glossary, sequence, term_tags) in rows {
                check_cancel()?;
                let glossary: Vec<Value> = serde_json::from_str(glossary.get())
                    .with_context(|| format!("{name} 里「{expression}」的释义不是数组"))?;
                add(&mut ctx, &mut writer, expression, reading, definition_tags, rules, score, glossary, Some(sequence), term_tags)?;
            }
        }
        media_cache = std::mem::take(&mut ctx.media);
        media_count = ctx.media_count;
        writer.flush(&tx)?;
        done_files += 1;
    }

    let mut term_meta_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut term_meta_total = 0;
    for name in &term_meta_files {
        check_cancel()?;
        progress(&ImportProgress { file: name.clone(), done_files, total_files });
        let bytes = archive.read(name)?;
        let rows: Vec<TermMetaV3<'_>> =
            serde_json::from_slice(strip_bom(&bytes)).with_context(|| format!("{name} 格式不对（不是 term meta 数组）"))?;
        let mut stmt = tx.prepare_cached("INSERT INTO term_meta(dictionary_id, expression, mode, data) VALUES (?1, ?2, ?3, ?4)")?;
        for TermMetaV3(expression, mode, data) in rows {
            if !matches!(mode.as_str(), "freq" | "pitch" | "ipa") {
                warnings.push(format!("{name} 里「{expression}」的 mode「{mode}」不认识，已跳过"));
                continue;
            }
            stmt.execute(params![dictionary_id, expression, mode, data.get()])?;
            *term_meta_counts.entry(mode).or_default() += 1;
            term_meta_total += 1;
        }
        done_files += 1;
    }
    term_meta_counts.insert("total".into(), term_meta_total);

    let mut kanji_skipped = 0;
    for name in kanji_files.iter().chain(&kanji_meta_files) {
        progress(&ImportProgress { file: name.clone(), done_files, total_files });
        let bytes = archive.read(name)?;
        let rows: Vec<&RawValue> =
            serde_json::from_slice(strip_bom(&bytes)).with_context(|| format!("{name} 不是合法的 JSON 数组"))?;
        kanji_skipped += rows.len();
        done_files += 1;
    }

    let mut tag_count = 0;
    for name in &tag_files {
        progress(&ImportProgress { file: name.clone(), done_files, total_files });
        let rows: Vec<Vec<Value>> =
            serde_json::from_slice(strip_bom(&archive.read(name)?)).with_context(|| format!("{name} 格式不对（不是标签数组）"))?;
        for row in &rows {
            let Some(tag_name) = row.first().and_then(Value::as_str) else {
                warnings.push(format!("{name} 里有一个没有名字的标签，已跳过"));
                continue;
            };
            insert_tag(&tx, dictionary_id, tag_name, row.get(1), row.get(2), row.get(3), row.get(4))?;
            tag_count += 1;
        }
        // 旧格式 index.json 的 tagMeta：Yomitan 在每个 tag_bank 之后都追加一遍
        if let Some(Value::Object(tag_meta)) = index_obj.get("tagMeta") {
            for (tag_name, value) in tag_meta {
                insert_tag(&tx, dictionary_id, tag_name, value.get("category"), value.get("order"), value.get("notes"), value.get("score"))?;
                tag_count += 1;
            }
        }
        done_files += 1;
    }

    let counts = json!({
        "terms": writer.terms,
        "termMeta": term_meta_counts,
        "tagMeta": tag_count,
        "media": media_count,
        "kanjiSkipped": kanji_skipped,
        "glossaryBytes": writer.raw_bytes,
        "compressedBytes": writer.compressed_bytes,
    });
    tx.execute(
        "UPDATE dictionaries SET counts_json = ?1, warnings_json = ?2 WHERE id = ?3",
        params![counts.to_string(), serde_json::to_string(&warnings.list)?, dictionary_id],
    )?;
    check_cancel()?;
    tx.commit()?;
    // 大事务把 WAL 撑到几百 MB；提交后立刻写回主库并截断。有读者占着时截不了也没关系，下次再截。
    let _ = store.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    progress(&ImportProgress { file: String::new(), done_files, total_files });

    Ok(ImportSummary {
        dictionary_id,
        title,
        revision,
        version,
        terms: writer.terms,
        term_meta: term_meta_counts,
        tags: tag_count,
        media: media_count,
        kanji_skipped,
        glossary_bytes: writer.raw_bytes,
        compressed_bytes: writer.compressed_bytes,
        warnings: warnings.list,
        warning_count: warnings.count,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// 打开 `.zip` 或解压开的词典目录。
pub fn open_archive(path: &Path) -> Result<Box<dyn Archive>> {
    if path.is_dir() {
        Ok(Box::new(DirArchive::open(path)?))
    } else {
        Ok(Box::new(ZipArchive::open(path)?))
    }
}

/// 打开 `.zip` 或解压开的目录，导入。
pub fn import_path(
    store: &mut DictionaryStore,
    path: &Path,
    progress: &mut dyn FnMut(&ImportProgress),
) -> Result<ImportSummary> {
    let options = ImportOptions { source_path: path.display().to_string(), ..ImportOptions::default() };
    import_dictionary(store, open_archive(path)?.as_mut(), &options, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_file_names_need_ascii_digits_and_the_root_directory() {
        let names: Vec<String> = ["term_bank_1.json", "term_bank_.json", "term_bank_１.json", "smk8/term_bank_2.json", "term_meta_bank_1.json", "term_bank_10.json"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(bank_files(&names, "term_bank_"), ["term_bank_1.json", "term_bank_10.json"]);
        assert_eq!(bank_files(&names, "term_meta_bank_"), ["term_meta_bank_1.json"]);
    }
}
