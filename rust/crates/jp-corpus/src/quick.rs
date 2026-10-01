//! 全局搜索（Cmd+K）。
//!
//! 一次查询横跨曲目 / 专辑 / 人物 / 词汇 / 歌词五种实体。
//!
//! ## 为什么分组返回而不是混排
//!
//! 跨类型的相关性分数没有可比性：曲名的 LIKE 匹配和歌词的 bm25
//! （SQLite 里是负数，越小越相关）不是一个量纲。硬凑出一个全局排序
//! 只会得到假的顺序。分组让用户按类型自己找，也让前端能按类型
//! 决定点击行为。
//!
//! ## 归一化
//!
//! 曲名和人名用 `normalized_*` 列匹配，这样「YOASOBI」能搜到
//! 「ＹＯＡＳＯＢＩ」、「山口一郎」能搜到「山口　一郎」——
//! 这些列正是当初建 `people` / `albums` 实体时存下来的。

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::models::{QuickHit, QuickSearchResults};
use crate::search;

/// 每一类最多返回几条。面板里每组显示几行就够，多了反而找不着。
const PER_KIND: i64 = 6;

/// LIKE 的转义。用户输入里的 `%` `_` 不该被当成通配符。
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// 归一化的查询键。和 `people.normalized_name` / `albums.normalized_title`
/// 用同一套规则（NFKC + 折大小写 + 去标点空白）。
fn normalized_key(text: &str) -> String {
    // 这里刻意不引 scraper 的 Python 实现，用等价的最小规则：
    // 全角→半角由 SQLite 那边的数据已经处理过，这里只做小写 + 去空白标点
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub(crate) fn quick_search(
    conn: &Connection,
    query: &str,
    per_kind: Option<i64>,
) -> Result<QuickSearchResults> {
    let trimmed = query.trim();
    // 单字符查询会命中几乎所有东西，没有意义
    if trimmed.chars().count() < 1 {
        return Ok(QuickSearchResults::default());
    }
    let limit = per_kind.unwrap_or(PER_KIND).clamp(1, 50);
    let pattern = like_pattern(trimmed);
    let key = normalized_key(trimmed);
    let key_pattern = if key.is_empty() {
        // 纯标点的查询（比如「&」）没有归一化键，退回原串匹配
        pattern.clone()
    } else {
        format!("%{key}%")
    };

    Ok(QuickSearchResults {
        tracks: tracks(conn, &pattern, limit)?,
        albums: albums(conn, &pattern, &key_pattern, limit)?,
        people: people(conn, &pattern, &key_pattern, limit)?,
        words: words(conn, &pattern, limit)?,
        lyrics: lyrics(conn, trimmed, limit)?,
    })
}

fn tracks(conn: &Connection, pattern: &str, limit: i64) -> Result<Vec<QuickHit>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, COALESCE(album,''), duration_sec \
         FROM songs \
         WHERE title LIKE ?1 ESCAPE '\\' OR artist LIKE ?1 ESCAPE '\\' \
         ORDER BY \
           CASE WHEN title LIKE ?1 ESCAPE '\\' THEN 0 ELSE 1 END, \
           length(title), title \
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![pattern, limit], |row| {
            Ok(QuickHit::Track {
                song_id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration_sec: row.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn albums(
    conn: &Connection,
    pattern: &str,
    key_pattern: &str,
    limit: i64,
) -> Result<Vec<QuickHit>> {
    let mut stmt = conn.prepare(
        "SELECT al.id, al.title, al.album_artist, COUNT(s.id) \
         FROM albums al LEFT JOIN songs s ON s.album_id = al.id \
         WHERE al.title LIKE ?1 ESCAPE '\\' \
            OR al.normalized_title LIKE ?2 ESCAPE '\\' \
            OR al.album_artist LIKE ?1 ESCAPE '\\' \
         GROUP BY al.id ORDER BY length(al.title), al.title LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![pattern, key_pattern, limit], |row| {
            Ok(QuickHit::Album {
                album_id: row.get(0)?,
                title: row.get(1)?,
                album_artist: row.get(2)?,
                track_count: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn people(conn: &Connection, pattern: &str, key_pattern: &str, limit: i64) -> Result<Vec<QuickHit>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, \
                COUNT(DISTINCT c.song_id), \
                GROUP_CONCAT(DISTINCT c.role) \
         FROM people p LEFT JOIN track_credits c ON c.person_id = p.id \
         WHERE p.name LIKE ?1 ESCAPE '\\' OR p.normalized_name LIKE ?2 ESCAPE '\\' \
         GROUP BY p.id \
         ORDER BY COUNT(DISTINCT c.song_id) DESC, length(p.name) \
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![pattern, key_pattern, limit], |row| {
            let roles: Option<String> = row.get(3)?;
            Ok(QuickHit::Person {
                person_id: row.get(0)?,
                name: row.get(1)?,
                track_count: row.get(2)?,
                roles: roles
                    .unwrap_or_default()
                    .split(',')
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn words(conn: &Connection, pattern: &str, limit: i64) -> Result<Vec<QuickHit>> {
    let mut stmt = conn.prepare(
        "SELECT t.lemma, t.pos, COUNT(*), COUNT(DISTINCT u.song_id) \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         WHERE t.lemma LIKE ?1 ESCAPE '\\' \
           AND t.pos NOT IN ('PUNCT','SYM','SPACE','X') \
         GROUP BY t.lemma, t.pos \
         ORDER BY \
           CASE WHEN t.lemma = ?2 THEN 0 ELSE 1 END, \
           COUNT(*) DESC \
         LIMIT ?3",
    )?;
    // 精确相等的词排最前——搜「夜」时想要的是「夜」本身，不是「夜空」
    let exact = pattern.trim_matches('%').replace("\\%", "%").replace("\\_", "_");
    let rows = stmt
        .query_map(params![pattern, exact, limit], |row| {
            Ok(QuickHit::Word {
                lemma: row.get(0)?,
                pos: row.get(1)?,
                freq: row.get(2)?,
                song_count: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn lyrics(conn: &Connection, text: &str, limit: i64) -> Result<Vec<QuickHit>> {
    // 复用已经调好的全文检索（FTS5 + 短查询的 LIKE 兜底），
    // 不在这里再写一套
    Ok(search::search_lyrics(conn, text, limit)?
        .into_iter()
        .map(|hit| QuickHit::Lyric {
            song_id: hit.song_id,
            title: hit.title,
            artist: hit.artist,
            utterance_id: hit.utterance_id,
            time_sec: hit.time_sec,
            text: hit.text,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_in_user_input_are_escaped() {
        // 用户搜「100%」不该变成「匹配任意内容」
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("a\\b"), "%a\\\\b%");
    }

    #[test]
    fn normalized_key_folds_case_and_drops_punctuation() {
        assert_eq!(normalized_key("YOASOBI"), "yoasobi");
        assert_eq!(normalized_key("Mrs. GREEN APPLE"), "mrsgreenapple");
        assert_eq!(normalized_key("山口　一郎"), "山口一郎");
    }

    #[test]
    fn normalized_key_of_pure_punctuation_is_empty() {
        // 「&」这种曲名归一化后是空的，调用方要退回原串匹配
        assert_eq!(normalized_key("&"), "");
        assert_eq!(normalized_key("!?"), "");
    }
}
