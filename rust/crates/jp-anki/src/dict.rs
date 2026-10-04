//! 本地词典查询：释义、读音、音高、词频、JLPT。
//!
//! **全部在 `corpus.db` 里**，一个字节都不联网：
//!
//! | 表 | 内容 | 规模 |
//! |---|---|---|
//! | `dict_registry` | 装了哪些词典、启用哪些、什么顺序 | 25 |
//! | `dict_terms` | 词条释义 | 250 万 |
//! | `yomitan_zh` | 明鏡 / 小学館 的中文释义（旧格式） | 8 万 |
//! | `yomitan_pitch` | 音高型 | 33 万 |
//! | `yomitan_freq` | JPDB 词频 | 49 万 |
//! | `jlpt_cache` | JLPT 等级 | 1 万 |
//!
//! 查询顺序照搬 PyQt 版：按 `dict_registry.sort_order` 逐个词典查，
//! 每部最多取 8 条，跨词典去重。顺序就是用户在词典管理里排的优先级，
//! 换个顺序卡片上的释义就换了一批。

use std::collections::BTreeSet;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// `dict_registry.zip_path` 里这两个是哨兵值，指向旧的 `yomitan_zh` 表
/// 而不是真的 zip 文件。
const LEGACY_MEIKYO: &str = "#legacy_terms_明鏡";
const LEGACY_SHOGAKUKAN: &str = "#legacy_terms_小学館";

/// 每部词典最多取几条。取太多卡片背面会长到没法看。
const MAX_DEFS_PER_DICT: usize = 8;
/// 一个词最多看几个匹配条目（同形不同读音的）。
const MAX_ROWS_PER_DICT: usize = 8;

/// 一条释义及其出处。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    /// 词典名，显示在释义分组的标题上
    pub source: String,
    pub text: String,
    /// `text` 已经是渲染好的 HTML，**不能再转义**。
    ///
    /// `dict_terms.defs_json` 里两种形态混着：`{"html": "<div>…"}` 是
    /// 导入时就排好版的，纯字符串则要当文本处理。当成同一种的话，
    /// 要么标签被转义显示成字面文本，要么用户词典里的尖括号变成注入。
    pub is_html: bool,
}

/// 一个词查出来的全部信息。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordInfo {
    pub reading: String,
    pub definitions: Vec<Definition>,
    /// 形如 `よる[1]`，没有就是空串
    pub pitch: String,
    /// JPDB 词频排名
    pub freq: String,
    /// N1~N5
    pub jlpt: String,
}

impl WordInfo {
    pub fn is_empty(&self) -> bool {
        self.reading.is_empty() && self.definitions.is_empty()
    }
}

/// 查一个词。
pub fn lookup(conn: &Connection, word: &str) -> Result<WordInfo> {
    let (reading, definitions) = lookup_definitions(conn, word)?;
    Ok(WordInfo {
        pitch: lookup_pitch(conn, word, &reading)?,
        freq: lookup_freq(conn, word)?,
        jlpt: lookup_jlpt(conn, word)?,
        reading,
        definitions,
    })
}

/// 按 `dict_registry` 的顺序查释义。返回 (读音, 释义列表)。
pub fn lookup_definitions(conn: &Connection, word: &str) -> Result<(String, Vec<Definition>)> {
    let mut reading = String::new();
    let mut out: Vec<Definition> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

    let mut stmt = conn.prepare(
        "SELECT name, zip_path FROM dict_registry
         WHERE enabled=1 AND dict_type='terms' ORDER BY sort_order ASC",
    )?;
    let enabled: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))?
        .collect::<Result<Vec<_>, _>>()?;

    for (dict_name, zip_path) in enabled {
        if zip_path == LEGACY_MEIKYO || zip_path == LEGACY_SHOGAKUKAN {
            let label = if zip_path == LEGACY_MEIKYO {
                "明鏡"
            } else {
                "小学館"
            };
            legacy_defs(conn, word, label, &mut reading, &mut seen, &mut out)?;
        } else {
            modern_defs(conn, word, &dict_name, &mut reading, &mut seen, &mut out)?;
        }
    }
    Ok((reading, out))
}

/// 旧的 `yomitan_zh`：一行里存着多个来源的释义，按来源标签过滤。
fn legacy_defs(
    conn: &Connection,
    word: &str,
    label: &str,
    reading: &mut String,
    seen: &mut BTreeSet<(String, String)>,
    out: &mut Vec<Definition>,
) -> Result<()> {
    // 先按词形查，查不到再按读音查——用户可能输的是假名
    let rows = query_two_ways(
        conn,
        "SELECT term, reading, zh_defs FROM yomitan_zh WHERE term=?1",
        "SELECT term, reading, zh_defs FROM yomitan_zh WHERE reading=?1",
        word,
    )?;

    let mut added = 0usize;
    for (term, rd, json_text) in rows.into_iter().take(MAX_ROWS_PER_DICT) {
        if reading.is_empty() && !rd.is_empty() {
            *reading = rd;
        }
        // 查到的词形和输入不同（同形异读、活用形）时标出来
        let prefix = if term != word {
            format!("【{term}】")
        } else {
            String::new()
        };
        let senses: Vec<Vec<String>> = serde_json::from_str(&json_text).unwrap_or_default();
        for sense in senses {
            if sense.len() < 2 || sense[0] != label {
                continue;
            }
            let text = format!("{prefix}{}", sense[1]);
            let key = (label.to_string(), text.clone());
            if seen.insert(key) {
                out.push(Definition {
                    source: label.to_string(),
                    text,
                    // yomitan_zh 存的是纯文本
                    is_html: false,
                });
                added += 1;
                if added >= MAX_DEFS_PER_DICT {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// `dict_terms`：`defs_json` 是一个字符串数组。
fn modern_defs(
    conn: &Connection,
    word: &str,
    dict_name: &str,
    reading: &mut String,
    seen: &mut BTreeSet<(String, String)>,
    out: &mut Vec<Definition>,
) -> Result<()> {
    let mut exact = conn.prepare(
        "SELECT term, reading, defs_json FROM dict_terms WHERE dict_name=?1 AND term=?2",
    )?;
    let mut rows: Vec<(String, String, String)> = exact
        .query_map(params![dict_name, word], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        let mut by_reading = conn.prepare(
            "SELECT term, reading, defs_json FROM dict_terms WHERE dict_name=?1 AND reading=?2",
        )?;
        rows = by_reading
            .query_map(params![dict_name, word], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
    }

    let mut added = 0usize;
    for (term, rd, json_text) in rows.into_iter().take(MAX_ROWS_PER_DICT) {
        if reading.is_empty() && !rd.is_empty() {
            *reading = rd;
        }
        let prefix = if term != word {
            format!("【{term}】")
        } else {
            String::new()
        };
        for item in coerce_definitions(&json_text) {
            // (显示的内容, 是不是 HTML, 去重用的原文)
            let entries: Vec<(String, bool, String)> = match item {
                Coerced::Html { html, text } => {
                    // 前缀本身是纯文本，加到 HTML 前面时单独包一层
                    let shown = if prefix.is_empty() {
                        html
                    } else {
                        format!("<div class='ym-term-ref'>{}</div>{html}", jp_dict::html::escape(&prefix))
                    };
                    let key = if text.is_empty() { shown.clone() } else { text };
                    vec![(shown, true, key)]
                }
                // 和 Python 一样只给切出来的第一段加前缀
                Coerced::Plain(parts) => parts
                    .into_iter()
                    .enumerate()
                    .map(|(i, part)| {
                        let shown = if i == 0 && !prefix.is_empty() {
                            format!("{prefix}{part}")
                        } else {
                            part
                        };
                        (shown.clone(), false, shown)
                    })
                    .collect(),
            };
            for (text, is_html, key_source) in entries {
                let key = (dict_name.to_string(), dedupe_key(&key_source));
                if seen.insert(key) {
                    out.push(Definition {
                        source: dict_name.to_string(),
                        text,
                        is_html,
                    });
                    added += 1;
                    if added >= MAX_DEFS_PER_DICT {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

/// `defs_json` 里的一项，和 Python `_coerce_definition_items` 一致。
#[derive(Debug, PartialEq)]
enum Coerced {
    /// 排好版的。去重看它自带的 `text`（没有才看 html），**不含词形前缀**
    Html { html: String, text: String },
    /// 纯文本，太长的已经切成几段
    Plain(Vec<String>),
}

fn coerce_definitions(json_text: &str) -> Vec<Coerced> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return Vec::new();
    };
    let mut out: Vec<Coerced> = Vec::new();
    let mut push = |v: &serde_json::Value| match v {
        serde_json::Value::Object(map) => {
            let html = map.get("html").and_then(|h| h.as_str()).unwrap_or("");
            let text = map.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if !html.is_empty() {
                out.push(Coerced::Html {
                    html: html.to_string(),
                    text: text.to_string(),
                });
            } else if !text.is_empty() {
                push_plain(&mut out, split_flattened(text));
            }
        }
        serde_json::Value::String(s) => push_plain(&mut out, split_flattened(s)),
        // Yomitan 结构化内容：抽出纯文本再按文本处理（数字、null 抽出来是空的，什么都不加）
        other => push_plain(&mut out, split_flattened(&sc_to_text(other))),
    };
    match &value {
        serde_json::Value::Array(items) => items.iter().for_each(&mut push),
        other => push(other),
    }
    out
}

/// 和 Python 一样：切完一段都没有的条目什么都不加。
fn push_plain(out: &mut Vec<Coerced>, parts: Vec<String>) {
    if !parts.is_empty() {
        out.push(Coerced::Plain(parts));
    }
}

/// Yomitan 结构化内容抽纯文本。和 Python `_sc_to_text` 一致。
fn sc_to_text(node: &serde_json::Value) -> String {
    match node {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items.iter().map(sc_to_text).collect(),
        serde_json::Value::Object(map) => map
            .get("content")
            .or_else(|| map.get("text"))
            .map(sc_to_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// 早期导入的词条把整部词条压成了一长串。太长的按义项号切开，
/// 否则卡片背面会出现一整段没有断点的文字。
///
/// 和 Python `_split_flattened_def_text` 逐字一致：
/// - 先在「圆圈数字」或「1~2 位数字紧跟〔」之前切
/// - 切不开（只有一段）再在「（一）」「（１２）」这种带数字的括号之前切——
///   **不是**任何「（」都切，「覚える（憶える）」「（造）」不是义项边界
/// - `re.split` 的零宽前瞻在**每个**匹配位置都切，所以「12〔」会在「1」和「2」前各切一次
fn split_flattened(text: &str) -> Vec<String> {
    static SENSE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(?:[1-9][0-9]?〔|[①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳])").unwrap()
    });
    static NUMBERED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^（[一二三四五六七八九十０-９0-9]+）").unwrap()
    });

    let cleaned = clean_text(text);
    if cleaned.chars().count() < 260 {
        return if cleaned.is_empty() { Vec::new() } else { vec![cleaned] };
    }
    let mut parts = split_at_matches(&cleaned, &SENSE);
    if parts.len() <= 1 {
        parts = split_at_matches(&cleaned, &NUMBERED);
    }
    if parts.is_empty() {
        vec![cleaned]
    } else {
        parts.truncate(10);
        parts
    }
}

/// 在正则能从该处起匹配的**每个**字符边界之前切开，逐段清理，丢掉空段。
fn split_at_matches(text: &str, marker: &regex::Regex) -> Vec<String> {
    let mut pieces: Vec<&str> = Vec::new();
    let mut start = 0;
    for (index, _) in text.char_indices() {
        if index > 0 && marker.is_match(&text[index..]) {
            pieces.push(&text[start..index]);
            start = index;
        }
    }
    pieces.push(&text[start..]);
    pieces
        .into_iter()
        .map(clean_text)
        .filter(|p| !p.is_empty())
        .collect()
}

/// 全角空格换成半角，连续空白折成一个。和 Python 的 `_clean_def_text` 一致。
pub(crate) fn clean_text(text: &str) -> String {
    let replaced = text.replace('\u{3000}', " ");
    let mut out = String::with_capacity(replaced.len());
    let mut in_space = false;
    for ch in replaced.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out.trim().to_string()
}

/// 去重用的键。和 Python 的 `_definition_dedupe_text` 一致：空白折叠成一个，**不删除**。
/// 之前是把空白全删掉，比 Python 激进——「a b」和「ab」会被当成同一条。
fn dedupe_key(text: &str) -> String {
    clean_text(text)
}

fn query_two_ways(
    conn: &Connection,
    by_term: &str,
    by_reading: &str,
    word: &str,
) -> Result<Vec<(String, String, String)>> {
    let read = |sql: &str| -> Result<Vec<(String, String, String)>> {
        let mut stmt = conn.prepare(sql)?;
        Ok(stmt
            .query_map(params![word], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?)
    };
    let rows = read(by_term)?;
    if rows.is_empty() { read(by_reading) } else { Ok(rows) }
}

// ────────────────────────── 查词候选 ──────────────────────────

/// 用词元查不到时换几个形再查。**卡片的 Expression 不变**，只是换个形去找释义。
///
/// 和 Python `gui._lookup_candidates` 逐项一致，包括它那几处保守的守卫
/// （「し」结尾要三个字以上、而且含汉字才当サ変，免得把「探し」造成「探する」）。
///
/// 真实语料实测（5,863 个词条）：直接查到 5,315，靠候选救回 76——大多是大写英文
/// （`YOU` → `you`）和可能形（`止まれる` → `止まれ`），靠的是「最常见表层形」那一项。
pub fn lookup_candidates(lemma: &str, pos: &str, surface: &str) -> Vec<String> {
    fn push(out: &mut Vec<String>, value: &str) {
        let value = clean_text(value);
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    fn chars(s: &str) -> usize {
        s.chars().count()
    }
    /// 去掉末尾 n 个字（按字，不按字节）
    fn drop_last(s: &str, n: usize) -> &str {
        match s.char_indices().rev().nth(n.saturating_sub(1)) {
            Some((index, _)) if n > 0 => &s[..index],
            _ => s,
        }
    }
    /// Python 的 `base.endswith("し") and len(base) > 2 and re.search(r"[\u4e00-\u9fff]", base)`
    fn suru_stem(base: &str) -> bool {
        base.ends_with('し')
            && chars(base) > 2
            && base.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    let mut out: Vec<String> = Vec::new();
    push(&mut out, lemma);
    push(&mut out, surface);
    let bases: Vec<&str> = [lemma, surface].into_iter().filter(|b| !b.is_empty()).collect();

    if pos == "VERB" {
        for base in &bases {
            if base.ends_with("する") {
                continue;
            }
            if suru_stem(base) {
                push(&mut out, &format!("{}する", drop_last(base, 1)));
            }
        }
        if ["為る", "します", "して"].contains(&lemma) {
            push(&mut out, "する");
        }
        if ["来る", "きた", "来た"].contains(&lemma) {
            push(&mut out, "来る");
        }
    } else {
        for base in &bases {
            if suru_stem(base) {
                push(&mut out, &format!("{}する", drop_last(base, 1)));
            }
        }
    }

    if pos == "ADJ" || pos == "ADV" {
        for base in &bases {
            if base.ends_with('く') && chars(base) > 1 {
                push(&mut out, &format!("{}い", drop_last(base, 1)));
            }
            if base.ends_with("かった") && chars(base) > 3 {
                push(&mut out, &format!("{}い", drop_last(base, 3)));
            }
            if base.ends_with("くない") && chars(base) > 3 {
                push(&mut out, &format!("{}い", drop_last(base, 3)));
            }
            if base.ends_with('に') && chars(base) > 1 {
                push(&mut out, drop_last(base, 1));
            }
        }
        if ["いい", "よい", "よかった", "よく"].contains(&lemma)
            || ["いい", "よい", "よく", "よかった"].contains(&surface)
        {
            push(&mut out, "良い");
            push(&mut out, "いい");
        }
    } else {
        for base in &bases {
            if base.ends_with('く') && chars(base) > 1 {
                push(&mut out, &format!("{}い", drop_last(base, 1)));
            }
            if base.ends_with("かった") && chars(base) > 3 {
                push(&mut out, &format!("{}い", drop_last(base, 3)));
            }
        }
    }

    // 「勉強し」这类サ変连用形：让「勉強する」排在它前面
    let snapshot = out.clone();
    for base in &snapshot {
        if !suru_stem(base) {
            continue;
        }
        let suru = format!("{}する", drop_last(base, 1));
        if let Some(at) = out.iter().position(|c| *c == suru) {
            out.remove(at);
            let before = out.iter().position(|c| c == base).unwrap_or(out.len());
            out.insert(before, suru);
        }
    }
    out.truncate(10);
    out
}

/// 这个词元在语料里最常见的写法。和 Python `_common_surface_for_lemma` 同一条查询。
pub fn common_surface(conn: &Connection, lemma: &str, pos: &str) -> Result<String> {
    let first = |sql: &str, values: &[&dyn rusqlite::ToSql]| -> Result<String> {
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query_map(values, |r| r.get::<_, String>(0))?;
        Ok(rows.next().transpose()?.unwrap_or_default())
    };
    if pos.is_empty() {
        first(
            "SELECT surface FROM tokens \
             WHERE lemma=?1 AND surface IS NOT NULL AND surface <> '' \
             GROUP BY surface ORDER BY COUNT(*) DESC LIMIT 1",
            &[&lemma],
        )
    } else {
        first(
            "SELECT surface FROM tokens \
             WHERE lemma=?1 AND pos=?2 AND surface IS NOT NULL AND surface <> '' \
             GROUP BY surface ORDER BY COUNT(*) DESC LIMIT 1",
            &[&lemma, &pos],
        )
    }
}

/// 带候选的查词结果。
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// 实际查到的那个形。本地词典全都查不到时是词元本身。
    pub term: String,
    pub reading: String,
    pub definitions: Vec<Definition>,
}

/// 按候选挨个查，第一个查到释义**或读音**的就用。和 Python `_lookup_word_for_anki` 的中文分支一致。
///
/// `surface` 为空时用语料里最常见的写法。本地词典全都查不到时，Python 会去问在线的
/// Jotoba / Jisho——**这里不联网**，如实返回空结果，查词形记成词元本身。
pub fn lookup_with_candidates(conn: &Connection, lemma: &str, pos: &str, surface: &str) -> Result<Found> {
    let surface = if surface.is_empty() {
        common_surface(conn, lemma, pos)?
    } else {
        surface.to_string()
    };
    let mut terms = lookup_candidates(lemma, pos, &surface);
    if terms.is_empty() {
        terms.push(lemma.to_string());
    }
    for term in terms {
        let (reading, definitions) = lookup_definitions(conn, &term)?;
        if !definitions.is_empty() || !reading.is_empty() {
            return Ok(Found {
                term,
                reading,
                definitions,
            });
        }
    }
    Ok(Found {
        term: lemma.to_string(),
        ..Default::default()
    })
}

/// 音高型，形如 `よる[1]`。最多显示三个位置。
pub fn lookup_pitch(conn: &Connection, word: &str, reading: &str) -> Result<String> {
    let row: Option<(String, String)> = if reading.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT reading, positions FROM yomitan_pitch WHERE term=?1 AND reading=?2",
            params![word, reading],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
    };
    let row = match row {
        Some(row) => Some(row),
        None => conn
            .query_row(
                "SELECT reading, positions FROM yomitan_pitch WHERE term=?1 LIMIT 1",
                params![word],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
    };
    let Some((rd, positions_json)) = row else {
        return Ok(String::new());
    };
    let positions: Vec<i64> = serde_json::from_str(&positions_json).unwrap_or_default();
    if positions.is_empty() {
        return Ok(String::new());
    }
    let marks: String = positions.iter().take(3).map(|p| format!("[{p}]")).collect();
    Ok(format!("{rd}{marks}"))
}

pub fn lookup_freq(conn: &Connection, word: &str) -> Result<String> {
    let freq: Option<i64> = conn
        .query_row(
            "SELECT freq FROM yomitan_freq WHERE term=?1",
            params![word],
            |r| r.get(0),
        )
        .optional()?;
    Ok(freq.filter(|f| *f > 0).map(|f| f.to_string()).unwrap_or_default())
}

pub fn lookup_jlpt(conn: &Connection, word: &str) -> Result<String> {
    let level: Option<String> = conn
        .query_row(
            "SELECT level FROM jlpt_cache WHERE lemma=?1",
            params![word],
            |r| r.get(0),
        )
        .optional()?;
    Ok(level.unwrap_or_default().to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE dict_registry (name TEXT, zip_path TEXT, dict_type TEXT,
                enabled INTEGER, sort_order INTEGER);
             CREATE TABLE dict_terms (id INTEGER PRIMARY KEY, term TEXT, reading TEXT,
                dict_name TEXT, defs_json TEXT);
             CREATE TABLE yomitan_zh (term TEXT, reading TEXT, zh_defs TEXT);
             CREATE TABLE yomitan_pitch (term TEXT, reading TEXT, positions TEXT);
             CREATE TABLE yomitan_freq (term TEXT, freq INTEGER);
             CREATE TABLE jlpt_cache (lemma TEXT, level TEXT);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn dictionaries_are_consulted_in_registry_order() {
        // 顺序就是用户在词典管理里排的优先级，换个顺序释义就换一批
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('慢的', 'z2', 'terms', 1, 5),
                                              ('快的', 'z1', 'terms', 1, 1);
             INSERT INTO dict_terms (term, reading, dict_name, defs_json)
                VALUES ('夜', 'よる', '快的', '[\"先来的\"]'),
                       ('夜', 'よる', '慢的', '[\"后到的\"]');",
        )
        .unwrap();
        let (_, defs) = lookup_definitions(&conn, "夜").unwrap();
        assert_eq!(
            defs.iter().map(|d| d.source.as_str()).collect::<Vec<_>>(),
            vec!["快的", "慢的"]
        );
    }

    #[test]
    fn a_disabled_dictionary_is_skipped() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('关掉的', 'z1', 'terms', 0, 1);
             INSERT INTO dict_terms (term, reading, dict_name, defs_json)
                VALUES ('夜', 'よる', '关掉的', '[\"不该出现\"]');",
        )
        .unwrap();
        assert!(lookup_definitions(&conn, "夜").unwrap().1.is_empty());
    }

    #[test]
    fn the_legacy_table_is_filtered_by_source_label() {
        // yomitan_zh 一行里存着多个来源，明鏡那条不该把小学館的也带出来
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('明鏡', '#legacy_terms_明鏡', 'terms', 1, 1);
             INSERT INTO yomitan_zh VALUES ('夜', 'よる',
                '[[\"明鏡\",\"晚上。\"],[\"小学館\",\"夜晚\"]]');",
        )
        .unwrap();
        let (reading, defs) = lookup_definitions(&conn, "夜").unwrap();
        assert_eq!(reading, "よる");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].source, "明鏡");
        assert_eq!(defs[0].text, "晚上。");
    }

    #[test]
    fn a_different_headword_is_marked() {
        // 查「夜」命中的条目是「夜中」时要标出来，否则用户以为释义是「夜」的
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('D', 'z', 'terms', 1, 1);
             INSERT INTO dict_terms (term, reading, dict_name, defs_json)
                VALUES ('夜中', 'よなか', 'D', '[\"半夜\"]');",
        )
        .unwrap();
        // 按读音命中
        let (_, defs) = lookup_definitions(&conn, "よなか").unwrap();
        assert_eq!(defs[0].text, "【夜中】半夜");
    }

    #[test]
    fn a_reading_lookup_is_only_a_fallback() {
        // 词形能命中就不该再按读音查——那会带出一堆同音词
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('D', 'z', 'terms', 1, 1);
             INSERT INTO dict_terms (term, reading, dict_name, defs_json)
                VALUES ('夜', 'よる', 'D', '[\"正解\"]'),
                       ('世', '夜', 'D', '[\"同音词\"]');",
        )
        .unwrap();
        let (_, defs) = lookup_definitions(&conn, "夜").unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].text, "正解");
    }

    #[test]
    fn each_dictionary_is_capped() {
        // 不封顶的话卡片背面会长到没法看
        let conn = db();
        let many: Vec<String> = (0..30).map(|i| format!("释义{i}")).collect();
        conn.execute(
            "INSERT INTO dict_registry VALUES ('D', 'z', 'terms', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dict_terms (term, reading, dict_name, defs_json) VALUES ('夜','よる','D',?1)",
            params![serde_json::to_string(&many).unwrap()],
        )
        .unwrap();
        assert_eq!(lookup_definitions(&conn, "夜").unwrap().1.len(), MAX_DEFS_PER_DICT);
    }

    #[test]
    fn identical_definitions_are_not_repeated() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('D', 'z', 'terms', 1, 1);
             INSERT INTO dict_terms (term, reading, dict_name, defs_json)
                VALUES ('夜', 'よる', 'D', '[\"晚上\",\"晚 上\",\"晚上\"]');",
        )
        .unwrap();
        // 和 Python 的 `_definition_dedupe_text` 一致：完全相同的去掉，
        // 但「晚上」和「晚 上」算两条——空白只折叠，不删除。
        // （原来这里断言是 1 条，那是旧的、比 Python 激进的去重。）
        assert_eq!(lookup_definitions(&conn, "夜").unwrap().1.len(), 2);
    }

    #[test]
    fn html_definitions_are_kept_apart_from_plain_text() {
        // 两种形态在同一列里混着。当成同一种的话，要么标签被转义
        // 显示成字面文本，要么用户词典里的尖括号变成注入。
        assert_eq!(
            coerce_definitions(r#"["纯文本"]"#),
            vec![Coerced::Plain(vec!["纯文本".to_string()])]
        );
        assert_eq!(
            coerce_definitions(r#"[{"html":"<b>富文本</b>"}]"#),
            vec![Coerced::Html {
                html: "<b>富文本</b>".to_string(),
                text: String::new()
            }]
        );
        // 有 text 没 html 的按文本处理
        assert_eq!(
            coerce_definitions(r#"[{"text":"文本"}]"#),
            vec![Coerced::Plain(vec!["文本".to_string()])]
        );
        // 认不出的结构跳过，不该 panic
        assert!(coerce_definitions(r#"[123, null, {"weird": 1}]"#).is_empty());
        assert!(coerce_definitions("不是 JSON").is_empty());
    }

    #[test]
    fn a_short_definition_is_not_split() {
        assert_eq!(split_flattened("晚上。"), vec!["晚上。"]);
        // 全角空格和连续空白都要规整
        assert_eq!(split_flattened("晚\u{3000}上   。"), vec!["晚 上 。"]);
        assert!(split_flattened("   ").is_empty());
    }

    #[test]
    fn a_very_long_definition_is_split_on_sense_markers() {
        // 早期导入的词条把整部词条压成一长串，不切开的话卡片背面
        // 是一整段没有断点的文字
        let long: String = std::iter::repeat_n("あ", 100).collect();
        let text = format!("①{long}②{long}③{long}");
        let parts = split_flattened(&text);
        assert_eq!(parts.len(), 3, "{parts:?}");
        assert!(parts[0].starts_with('①'));
        assert!(parts[1].starts_with('②'));
    }

    #[test]
    fn pitch_prefers_the_matching_reading() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO yomitan_pitch VALUES ('夜', 'や', '[1]'),
                                              ('夜', 'よる', '[1,2,3,4]');",
        )
        .unwrap();
        assert_eq!(lookup_pitch(&conn, "夜", "よる").unwrap(), "よる[1][2][3]");
        // 没有读音时退到第一条
        assert_eq!(lookup_pitch(&conn, "夜", "").unwrap(), "や[1]");
        // 查不到不是错误
        assert_eq!(lookup_pitch(&conn, "不存在", "").unwrap(), "");
    }

    #[test]
    fn freq_and_jlpt_are_plain_lookups() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO yomitan_freq VALUES ('夜', 1234), ('零', 0);
             INSERT INTO jlpt_cache VALUES ('夜', 'n5');",
        )
        .unwrap();
        assert_eq!(lookup_freq(&conn, "夜").unwrap(), "1234");
        // 0 当成没有
        assert_eq!(lookup_freq(&conn, "零").unwrap(), "");
        assert_eq!(lookup_jlpt(&conn, "夜").unwrap(), "N5");
        assert_eq!(lookup_jlpt(&conn, "不存在").unwrap(), "");
    }

    #[test]
    fn a_word_with_nothing_known_is_empty_not_an_error() {
        let conn = db();
        let info = lookup(&conn, "查不到的词").unwrap();
        assert!(info.is_empty());
        assert_eq!(info.pitch, "");
    }
}

#[cfg(test)]
mod python_parity {
    use super::*;

    #[test]
    fn short_text_is_never_split() {
        assert_eq!(split_flattened("①a②b"), vec!["①a②b"]);
        assert!(split_flattened("  ").is_empty());
    }

    #[test]
    fn a_plain_parenthesis_is_not_a_sense_boundary() {
        // 对账里 245 个词差在这：「覚える（憶える）」「【年（▽歳）】」「（造）」被切开了
        let text = format!("思い（想い）覚える（憶える）（造）{}", "あ".repeat(260));
        assert_eq!(split_flattened(&text).len(), 1);
    }

    #[test]
    fn numbered_parentheses_split_when_there_are_no_sense_markers() {
        let text = format!("（一）{}（二）{}", "あ".repeat(140), "い".repeat(140));
        let parts = split_flattened(&text);
        assert_eq!(parts.len(), 2);
        assert!(parts[1].starts_with("（二）"));
    }

    #[test]
    fn a_digit_before_a_tag_bracket_splits_at_every_match_like_re_split() {
        // 期望值来自 Python 实跑 gui._split_flattened_def_text
        let text = format!("{}12〔名〕{}", "あ".repeat(150), "い".repeat(150));
        assert_eq!(
            split_flattened(&text),
            vec!["あ".repeat(150), "1".to_string(), format!("2〔名〕{}", "い".repeat(150))]
        );
    }

    #[test]
    fn dedupe_collapses_whitespace_but_keeps_it() {
        assert_eq!(dedupe_key("a \u{3000} b"), dedupe_key("a b"));
        assert_ne!(dedupe_key("a b"), dedupe_key("ab"));
    }

    #[test]
    fn structured_content_is_read_as_text() {
        let items = coerce_definitions(
            r#"[[{"content": "夜"}, "の意味"], {"html": "<b>x</b>", "text": "x"}]"#,
        );
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], Coerced::Plain(parts) if parts == &vec!["夜の意味".to_string()]));
        assert!(matches!(&items[1], Coerced::Html { html, text } if html == "<b>x</b>" && text == "x"));
    }
}
