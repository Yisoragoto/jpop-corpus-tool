//! KWIC 检索与全文检索。
//!
//! 两处相对旧实现的改动：
//!
//! **1. 干掉 N+1。** `legacy/gui.py` 的 SearchWorker 每条结果都要再发一次
//! `SELECT surface FROM tokens WHERE utterance_id=?` 来重建整行，
//! 开了 cross_line 还要再发两次。搜一个常见词（500 条结果）就是
//! 500~1500 次额外查询。这里改成：命中一次查、行内 token 一次查、
//! 相邻行一次查——**总共 3 次，与结果条数无关**。
//!
//! **2. 歌手筛选走实体。** 旧写法是
//! `'/' || s.artist || '/' LIKE '%/' || ? || '/%'`，无法走索引，
//! 合作曲靠拼字符串。现在按 `people.id` 过滤，走 `idx_credits_person`。

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};

use crate::models::{KwicHit, KwicQuery, LyricHit, MatchField};

/// 一条命中的骨架，token 文本还没填。
#[derive(Clone)]
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
    match_len: usize,
}

/// 行内的一个 token
struct Tok {
    surface: String,
    lemma: String,
    pos: String,
}

impl Tok {
    fn field(&self, field: MatchField) -> &str {
        match field {
            MatchField::Surface => &self.surface,
            MatchField::Lemma => &self.lemma,
        }
    }
}

/// 关键词拆成模式：`A B` / `A|B` 是多个候选（在前端拆好），`A+B` 是**连续的多个词**——
/// 「て+しまう」「なんて+ない」这类表达。`+` 两边按同一个字段（表层 / 词元）匹配。
fn patterns(keywords: &[String]) -> Vec<Vec<String>> {
    keywords
        .iter()
        .map(|k| k.split('+').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect::<Vec<_>>())
        .filter(|p| !p.is_empty())
        .collect()
}

pub(crate) fn kwic(conn: &Connection, query: &KwicQuery) -> Result<Vec<KwicHit>> {
    let patterns = patterns(&query.keywords);
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    // 要看整行 token 才能判断的条件（多词表达、附近有某词）：
    // SQL 只按第一个词粗筛，不折叠不截断，在 Rust 里筛完再折叠、截断
    let multi = patterns.iter().any(|p| p.len() > 1);
    let near = query.near.as_ref().filter(|n| !n.word.trim().is_empty());
    let post = multi || near.is_some();

    let mut raw = fetch_hits(conn, query, &patterns, post)?;
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let mut utterance_ids: Vec<i64> = raw.iter().map(|h| h.utterance_id).collect::<HashSet<_>>().into_iter().collect();
    utterance_ids.sort_unstable();
    let tokens = fetch_tokens(conn, &utterance_ids)?;

    if post {
        raw.retain_mut(|hit| {
            let Some(line) = tokens.get(&hit.utterance_id) else { return false };
            let idx = hit.token_idx.max(0) as usize;
            let Some(len) = match_at(line, idx, &patterns, query.field) else { return false };
            hit.match_len = len;
            near.is_none_or(|n| near_ok(line, idx, len, n))
        });
        if query.dedup {
            raw = fold_repeats(raw);
        }
        if !query.jp_only
            && let Some(n) = query.limit.filter(|n| *n > 0)
        {
            raw.truncate(n as usize);
        }
    }

    let neighbours = if query.cross_line {
        fetch_neighbours(conn, &raw)?
    } else {
        HashMap::new()
    };

    // 和 Python 一样一次批量查出来，不逐行查
    let corrected = crate::corrections::corrected_among(conn, &utterance_ids)?;
    let (spans, positions) = spans_for(conn, &raw)?;

    let mut hits: Vec<KwicHit> = raw
        .into_iter()
        .map(|hit| {
            let mut out = build(hit, &tokens, &neighbours);
            out.corrected = corrected.contains(&out.utterance_id);
            if let Some(span) = spans.get(&out.utterance_id) {
                out.end_sec = Some(span.end_sec);
                out.end_kind = Some(span.end_kind);
            }
            if let Some(&(pos, lines)) = positions.get(&out.utterance_id) {
                out.line_pos = pos;
                out.song_lines = lines;
            }
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

/// 从 `idx` 开始能匹配上哪个模式，返回匹配的 token 数（取最长的）
fn match_at(line: &[Tok], idx: usize, patterns: &[Vec<String>], field: MatchField) -> Option<usize> {
    patterns
        .iter()
        .filter(|p| idx + p.len() <= line.len() && p.iter().enumerate().all(|(k, w)| line[idx + k].field(field) == w))
        .map(Vec::len)
        .max()
}

/// 关键词左右 `window` 个词（不算标点、符号）之内有没有那个词
fn near_ok(line: &[Tok], idx: usize, len: usize, near: &crate::models::NearFilter) -> bool {
    let word = near.word.trim();
    let content = |t: &&Tok| t.pos != "PUNCT" && t.pos != "SYM";
    let left = line[..idx.min(line.len())].iter().rev().filter(content).take(near.window);
    let right = line[(idx + len).min(line.len())..].iter().filter(content).take(near.window);
    left.chain(right).any(|t| t.field(near.field) == word)
}

/// 同一首歌里文本相同的行折叠成一条（保留最早的那次），和 SQL 那条路的 GROUP BY 同一个规则
fn fold_repeats(raw: Vec<RawHit>) -> Vec<RawHit> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: HashMap<(String, String), RawHit> = HashMap::new();
    for hit in raw {
        let key = (hit.song_id.clone(), hit.text.clone());
        match groups.get_mut(&key) {
            Some(first) => first.repeat_count += 1,
            None => {
                order.push(key.clone());
                groups.insert(key, hit);
            }
        }
    }
    order.into_iter().filter_map(|k| groups.remove(&k)).collect()
}

/// 行在歌里的位置：（第几行，一共几行）
type LinePositions = HashMap<i64, (i64, i64)>;

/// 每条命中那一行的音频区间，以及这一行在歌里的位置。一次查完所有涉及的歌
fn spans_for(conn: &Connection, hits: &[RawHit]) -> Result<(HashMap<i64, crate::segments::LineSpan>, LinePositions)> {
    let mut songs: Vec<String> = hits.iter().map(|h| h.song_id.clone()).collect::<HashSet<_>>().into_iter().collect();
    songs.sort();
    let mut out = HashMap::new();
    let mut positions = LinePositions::new();
    for chunk in songs.chunks(400) {
        let marks = placeholders(chunk.len());
        let mut durations: HashMap<String, Option<f64>> = HashMap::new();
        {
            let mut stmt = conn.prepare(&format!("SELECT id, duration_sec FROM songs WHERE id IN ({marks})"))?;
            let rows = stmt.query_map(params_from_iter(chunk.iter()), |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
            for row in rows {
                let (id, d) = row?;
                durations.insert(id, d);
            }
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT song_id, id, line_idx, time_sec, text FROM utterances \
             WHERE song_id IN ({marks}) ORDER BY song_id, line_idx, id"
        ))?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })?;
        let mut by_song: HashMap<String, Vec<crate::segments::TimedLine>> = HashMap::new();
        for row in rows {
            let (song, line) = row?;
            by_song.entry(song).or_default().push(line);
        }
        for (song, lines) in by_song {
            for (i, line) in lines.iter().enumerate() {
                positions.insert(line.0, (i as i64, lines.len() as i64));
            }
            let duration = durations.get(&song).copied().flatten();
            for span in crate::segments::spans_from_lines(&lines, duration).into_iter().flatten() {
                out.insert(span.utterance_id, span);
            }
        }
    }
    Ok((out, positions))
}

/// 第一次查询：命中的行 + 曲目元数据。`raw_rows` 为真时不折叠、不截断（后面还要在 Rust 里筛）
fn fetch_hits(conn: &Connection, query: &KwicQuery, patterns: &[Vec<String>], raw_rows: bool) -> Result<Vec<RawHit>> {
    let field = query.field.column();
    let mut params: Vec<Value> = Vec::new();

    let mut firsts: Vec<String> = patterns.iter().map(|p| p[0].clone()).collect::<HashSet<_>>().into_iter().collect();
    firsts.sort();
    let keyword_marks = placeholders(firsts.len());
    params.extend(firsts.into_iter().map(Value::from));

    let mut wheres = format!("t.{field} IN ({keyword_marks})");
    // 多词表达的词性过滤对第一个词没有意义（「て+しまう」的「て」是助词），只在单词检索时用
    let single = patterns.iter().all(|p| p.len() == 1);
    if let Some(pos) = query.pos.as_deref().filter(|p| !p.is_empty() && single) {
        wheres.push_str(" AND t.pos = ?");
        params.push(Value::from(pos.to_string()));
    }
    if !query.person_ids.is_empty() {
        // 走 track_credits 的索引，而不是对 songs.artist 做 LIKE 扫描
        wheres.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM track_credits c \
               WHERE c.song_id = u.song_id AND c.person_id IN ({}))",
            placeholders(query.person_ids.len())
        ));
        params.extend(query.person_ids.iter().map(|id| Value::from(*id)));
    }
    let (sub, sub_params) = query.subcorpus.and_clause("u.song_id");
    wheres.push_str(&sub);
    params.extend(sub_params);

    // dedup：同一首歌里文本相同的行折叠成一条，重复次数放进 repeat_count。
    // 取 MIN(token_idx) 是为了让左右语境切分点稳定，否则同一行的不同
    // 命中位置会让结果看起来在跳。
    let sql = if query.dedup && !raw_rows {
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
             ORDER BY s.artist, s.title, u.time_sec, u.line_idx, t.token_idx"
        )
    };
    let sql = match query.limit {
        Some(n) if n > 0 && !query.jp_only && !raw_rows => format!("{sql} LIMIT {n}"),
        _ => sql,
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
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
            match_len: 1,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 第二次查询：一次把所有命中行的 token 取回来。
fn fetch_tokens(conn: &Connection, utterance_ids: &[i64]) -> Result<HashMap<i64, Vec<Tok>>> {
    let mut out: HashMap<i64, Vec<Tok>> = HashMap::new();
    // SQLite 默认变量上限 999，分批取
    for chunk in utterance_ids.chunks(900) {
        let sql = format!(
            "SELECT utterance_id, surface, lemma, COALESCE(pos,'') FROM tokens \
             WHERE utterance_id IN ({}) ORDER BY utterance_id, token_idx",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, Tok { surface: row.get(1)?, lemma: row.get(2)?, pos: row.get(3)? }))
        })?;
        for row in rows {
            let (id, tok) = row?;
            out.entry(id).or_default().push(tok);
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
    tokens: &HashMap<i64, Vec<Tok>>,
    neighbours: &HashMap<(String, i64), String>,
) -> KwicHit {
    let surfaces: Vec<String> = tokens
        .get(&hit.utterance_id)
        .map(|line| line.iter().map(|t| t.surface.clone()).collect())
        .unwrap_or_default();
    let idx = hit.token_idx.max(0) as usize;
    let last = idx + hit.match_len.max(1) - 1;
    let (left_words, right_words) = tokens
        .get(&hit.utterance_id)
        .map(|line| context_words(line, idx, last))
        .unwrap_or_default();

    // 优先按原文偏移切：`tokens` 表里没有空白 token（分词时被丢掉了），
    // 直接拼 surface 会把原句里的空格吃掉——
    // 「初めましての色が あることを」会变成「初めましての色があることを」。
    // 日语歌词里的空格是有意的断句，不能丢。
    let located = locate(&hit.text, &surfaces, idx)
        .zip(locate(&hit.text, &surfaces, last))
        .map(|((start, _), (_, end))| (start, end));
    let (mut left, keyword, mut right) = match located {
        Some((start, end)) => (
            hit.text[..start].to_string(),
            hit.text[start..end].to_string(),
            hit.text[end..].to_string(),
        ),
        // 对不上就退回拼接法（丢空格但仍可用），token 也缺失时整行当关键词——
        // 无论如何要让用户看到这一行，而不是一条空结果
        None if last < surfaces.len() => (
            surfaces[..idx].concat(),
            surfaces[idx..=last].concat(),
            surfaces[last + 1..].concat(),
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
        end_sec: None,
        end_kind: None,
        match_len: hit.match_len as i64,
        left_words,
        right_words,
        line_pos: 0,
        song_lines: 0,
    }
}

/// 排序用的左右语境：关键词两边各最多 [`CONTEXT_WORDS`] 个词，跳过标点、符号，由近到远
fn context_words(line: &[Tok], idx: usize, last: usize) -> (Vec<String>, Vec<String>) {
    let content = |t: &&Tok| t.pos != "PUNCT" && t.pos != "SYM";
    let left = line[..idx.min(line.len())].iter().rev().filter(content).take(CONTEXT_WORDS).map(|t| t.surface.clone()).collect();
    let right = line[(last + 1).min(line.len())..].iter().filter(content).take(CONTEXT_WORDS).map(|t| t.surface.clone()).collect();
    (left, right)
}

/// KWIC 每边给几个词用来排序（AntConc 默认排到 3，这里多给两个做次级键）
const CONTEXT_WORDS: usize = 5;

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
