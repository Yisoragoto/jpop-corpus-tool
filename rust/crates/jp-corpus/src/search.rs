//! KWIC 检索与全文检索。
//!
//! 两处相对旧实现的改动：
//!
//! **1. 干掉 N+1。** `gui.py` 的 SearchWorker 每条结果都要再发一次
//! `SELECT surface FROM tokens WHERE utterance_id=?` 来重建整行，
//! 开了 cross_line 还要再发两次。搜一个常见词（500 条结果）就是
//! 500~1500 次额外查询。这里改成：命中一次查、行内 token 一次查、
//! 相邻行一次查——**总共 3 次，与结果条数无关**。
//!
//! **2. 歌手筛选走实体。** 旧写法是
//! `'/' || s.artist || '/' LIKE '%/' || ? || '/%'`，无法走索引，
//! 合作曲靠拼字符串。现在按 `people.id` 过滤，走 `idx_credits_person`。

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter};

use crate::models::{KwicHit, KwicQuery, LyricHit};

/// 一条命中的骨架，token 文本还没填。
struct RawHit {
    song_id: String,
    artist: String,
    title: String,
    audio_path: String,
    utterance_id: i64,
    line_idx: i64,
    time_sec: Option<f64>,
    text: String,
    token_idx: i64,
    repeat_count: i64,
}

pub(crate) fn kwic(conn: &Connection, query: &KwicQuery) -> Result<Vec<KwicHit>> {
    if query.keywords.is_empty() {
        return Ok(Vec::new());
    }

    let raw = fetch_hits(conn, query)?;
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let utterance_ids: Vec<i64> = raw.iter().map(|h| h.utterance_id).collect();
    let tokens = fetch_tokens(conn, &utterance_ids)?;
    let neighbours = if query.cross_line {
        fetch_neighbours(conn, &raw)?
    } else {
        HashMap::new()
    };

    // 和 Python 一样一次批量查出来，不逐行查
    let corrected = crate::corrections::corrected_among(conn, &utterance_ids)?;

    let mut hits: Vec<KwicHit> = raw
        .into_iter()
        .map(|hit| {
            let mut out = build(hit, &tokens, &neighbours);
            out.corrected = corrected.contains(&out.utterance_id);
            out
        })
        .collect();
    if query.jp_only {
        // 要看切出来的关键词才知道算不算日文，所以 SQL 里没截断，这里筛完再截
        hits.retain(|hit| crate::stats::is_japanese(&hit.keyword));
        if let Some(n) = query.limit.filter(|n| *n > 0) {
            hits.truncate(n as usize);
        }
    }
    Ok(hits)
}

/// 第一次查询：命中的行 + 曲目元数据。
fn fetch_hits(conn: &Connection, query: &KwicQuery) -> Result<Vec<RawHit>> {
    let field = query.field.column();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let keyword_marks = placeholders(query.keywords.len());
    for kw in &query.keywords {
        params.push(Box::new(kw.clone()));
    }

    let mut wheres = format!("t.{field} IN ({keyword_marks})");
    if let Some(pos) = query.pos.as_deref().filter(|p| !p.is_empty()) {
        wheres.push_str(" AND t.pos = ?");
        params.push(Box::new(pos.to_string()));
    }
    if !query.person_ids.is_empty() {
        // 走 track_credits 的索引，而不是对 songs.artist 做 LIKE 扫描
        wheres.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM track_credits c \
               WHERE c.song_id = u.song_id AND c.person_id IN ({}))",
            placeholders(query.person_ids.len())
        ));
        for id in &query.person_ids {
            params.push(Box::new(*id));
        }
    }

    // dedup：同一首歌里文本相同的行折叠成一条，重复次数放进 repeat_count。
    // 取 MIN(token_idx) 是为了让左右语境切分点稳定，否则同一行的不同
    // 命中位置会让结果看起来在跳。
    let sql = if query.dedup {
        format!(
            "SELECT u.song_id, s.artist, s.title, COALESCE(s.audio_path,''), \
                    MIN(u.id), MIN(u.line_idx), MIN(u.time_sec), u.text, \
                    MIN(t.token_idx), COUNT(*) \
             FROM tokens t \
             JOIN utterances u ON u.id = t.utterance_id \
             JOIN songs s ON s.id = u.song_id \
             WHERE {wheres} \
             GROUP BY u.song_id, u.text \
             ORDER BY s.artist, s.title, MIN(u.time_sec)"
        )
    } else {
        format!(
            "SELECT u.song_id, s.artist, s.title, COALESCE(s.audio_path,''), \
                    u.id, u.line_idx, u.time_sec, u.text, t.token_idx, 1 \
             FROM tokens t \
             JOIN utterances u ON u.id = t.utterance_id \
             JOIN songs s ON s.id = u.song_id \
             WHERE {wheres} \
             ORDER BY s.artist, s.title, u.time_sec"
        )
    };
    let sql = match query.limit {
        Some(n) if n > 0 && !query.jp_only => format!("{sql} LIMIT {n}"),
        _ => sql,
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
        Ok(RawHit {
            song_id: row.get(0)?,
            artist: row.get(1)?,
            title: row.get(2)?,
            audio_path: row.get(3)?,
            utterance_id: row.get(4)?,
            line_idx: row.get(5)?,
            time_sec: row.get(6)?,
            text: row.get(7)?,
            token_idx: row.get(8)?,
            repeat_count: row.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 第二次查询：一次把所有命中行的 token 取回来。
fn fetch_tokens(conn: &Connection, utterance_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>> {
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    // SQLite 默认变量上限 999，分批取
    for chunk in utterance_ids.chunks(900) {
        let sql = format!(
            "SELECT utterance_id, surface FROM tokens \
             WHERE utterance_id IN ({}) ORDER BY utterance_id, token_idx",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, surface) = row?;
            out.entry(id).or_default().push(surface);
        }
    }
    Ok(out)
}

/// 第三次查询：cross_line 需要的相邻行，同样一次取完。
fn fetch_neighbours(conn: &Connection, hits: &[RawHit]) -> Result<HashMap<(String, i64), String>> {
    let mut wanted: Vec<(String, i64)> = Vec::with_capacity(hits.len() * 2);
    for hit in hits {
        wanted.push((hit.song_id.clone(), hit.line_idx - 1));
        wanted.push((hit.song_id.clone(), hit.line_idx + 1));
    }
    wanted.sort();
    wanted.dedup();

    let mut out = HashMap::new();
    for chunk in wanted.chunks(450) {
        let clause = std::iter::repeat_n("(u.song_id=? AND u.line_idx=?)", chunk.len())
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!("SELECT u.song_id, u.line_idx, u.text FROM utterances u WHERE {clause}");
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(chunk.len() * 2);
        for (song_id, line_idx) in chunk {
            params.push(Box::new(song_id.clone()));
            params.push(Box::new(*line_idx));
        }
        let rows = stmt.query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (song_id, line_idx, text) = row?;
            out.insert((song_id, line_idx), text);
        }
    }
    Ok(out)
}

fn build(
    hit: RawHit,
    tokens: &HashMap<i64, Vec<String>>,
    neighbours: &HashMap<(String, i64), String>,
) -> KwicHit {
    let empty = Vec::new();
    let surfaces = tokens.get(&hit.utterance_id).unwrap_or(&empty);
    let idx = hit.token_idx.max(0) as usize;

    // 优先按原文偏移切：`tokens` 表里没有空白 token（分词时被丢掉了），
    // 直接拼 surface 会把原句里的空格吃掉——
    // 「初めましての色が あることを」会变成「初めましての色があることを」。
    // 日语歌词里的空格是有意的断句，不能丢。
    let (mut left, keyword, mut right) = match locate(&hit.text, surfaces, idx) {
        Some((start, end)) => (
            hit.text[..start].to_string(),
            hit.text[start..end].to_string(),
            hit.text[end..].to_string(),
        ),
        // 对不上就退回拼接法（丢空格但仍可用），token 也缺失时整行当关键词——
        // 无论如何要让用户看到这一行，而不是一条空结果
        None if idx < surfaces.len() => (
            surfaces[..idx].concat(),
            surfaces[idx].clone(),
            surfaces[idx + 1..].concat(),
        ),
        None => (String::new(), hit.text.clone(), String::new()),
    };

    if !neighbours.is_empty() {
        if let Some(prev) = neighbours.get(&(hit.song_id.clone(), hit.line_idx - 1)) {
            left = format!("{prev} / {left}");
        }
        if let Some(next) = neighbours.get(&(hit.song_id.clone(), hit.line_idx + 1)) {
            right = format!("{right} / {next}");
        }
    }

    KwicHit {
        song_id: hit.song_id,
        artist: hit.artist,
        title: hit.title,
        audio_path: hit.audio_path,
        utterance_id: hit.utterance_id,
        line_idx: hit.line_idx,
        time_sec: hit.time_sec,
        text: hit.text,
        left,
        keyword,
        right,
        repeat_count: hit.repeat_count,
        corrected: false,
    }
}

// ────────────────────────────── 全文检索 ──────────────────────────────

/// trigram 索引的最短可查长度。少于 3 个字符无法构成一个 trigram，
/// FTS5 会静默返回 0 条。
const TRIGRAM_MIN_CHARS: usize = 3;

/// 歌词全文检索。
///
/// `utterances_fts` 这张 trigram 索引一直建着、一直维护着，
/// 但旧 UI 从来没查过它——KWIC 走的是 tokens 表的精确匹配。
/// trigram 能匹配任意子串，正好补上「只记得半句歌词」这种检索。
///
/// **短查询走 LIKE。** 日语里「夜」「恋」「君」这类单字检索非常常见，
/// 而 trigram 对它们一律返回 0 条——静默返回空结果比报错更糟，用户会
/// 以为语料里真的没有。8,443 行做一次 LIKE 全扫只要几毫秒，值得。
pub(crate) fn search_lyrics(conn: &Connection, text: &str, limit: i64) -> Result<Vec<LyricHit>> {
    let needle = text.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    if needle.chars().count() < TRIGRAM_MIN_CHARS {
        return search_lyrics_like(conn, needle, limit);
    }

    // FTS5 的查询语法里双引号是字符串定界符，用户输入要转义后整体当短语查，
    // 否则歌词里的 AND / OR / * 会被当成运算符。
    let phrase = format!("\"{}\"", needle.replace('"', "\"\""));
    let mut stmt = conn.prepare(
        "SELECT u.song_id, s.artist, s.title, u.id, u.time_sec, u.text, bm25(utterances_fts) \
         FROM utterances_fts f \
         JOIN utterances u ON u.id = f.rowid \
         JOIN songs s ON s.id = u.song_id \
         WHERE utterances_fts MATCH ?1 \
         ORDER BY bm25(utterances_fts) \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![phrase, limit], |row| {
        Ok(LyricHit {
            song_id: row.get(0)?,
            artist: row.get(1)?,
            title: row.get(2)?,
            utterance_id: row.get(3)?,
            time_sec: row.get(4)?,
            text: row.get(5)?,
            rank: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 单字/双字检索的兜底。按出现位置排序，越靠前越像是主题词。
fn search_lyrics_like(conn: &Connection, needle: &str, limit: i64) -> Result<Vec<LyricHit>> {
    // LIKE 的通配符要转义，否则用户搜「%」会匹配到所有行
    let escaped = needle
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");

    let mut stmt = conn.prepare(
        "SELECT u.song_id, s.artist, s.title, u.id, u.time_sec, u.text \
         FROM utterances u JOIN songs s ON s.id = u.song_id \
         WHERE u.text LIKE ?1 ESCAPE '\\' \
         ORDER BY instr(u.text, ?2), s.artist, s.title \
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, needle, limit], |row| {
        Ok(LyricHit {
            song_id: row.get(0)?,
            artist: row.get(1)?,
            title: row.get(2)?,
            utterance_id: row.get(3)?,
            time_sec: row.get(4)?,
            text: row.get(5)?,
            // LIKE 没有 bm25。用 0 而不是编一个假分数——
            // 前端据此知道这批结果没有相关性排序。
            rank: 0.0,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 在原文里定位第 `target` 个 token，返回它的字节区间。
///
/// 逐个 surface 沿原文推进，允许跳过原文里的空白——因为 `tokens` 表里
/// 没有空白 token。任何一个 surface 对不上就返回 None，让调用方退回拼接法：
/// 宁可丢空格，也不要给出错位的左右语境。
fn locate(text: &str, surfaces: &[String], target: usize) -> Option<(usize, usize)> {
    if target >= surfaces.len() {
        return None;
    }
    let mut cursor = 0usize;
    for (i, surface) in surfaces.iter().enumerate() {
        while cursor < text.len() {
            let rest = &text[cursor..];
            match rest.chars().next() {
                Some(ch) if ch.is_whitespace() && !surface.starts_with(ch) => {
                    cursor += ch.len_utf8();
                }
                _ => break,
            }
        }
        if !text[cursor..].starts_with(surface.as_str()) {
            return None;
        }
        if i == target {
            return Some((cursor, cursor + surface.len()));
        }
        cursor += surface.len();
    }
    None
}

pub(crate) fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",")
}
