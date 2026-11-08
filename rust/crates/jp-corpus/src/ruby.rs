//! 注音（#63）：源文件自带的注音和用户改过的读音。
//!
//! 振假名本来全是 Sudachi 现场分词给的读音。歌词里最难的是当て字和義訓——「他人」唱ひと、
//! 「運命」唱さだめ、「本気」唱マジ——这是作词人的选择，只看文字谁也猜不出。所以分两层盖在自动注音上：
//!
//! 1. **用户改的读音**（`reading_rules`）：按写法记，这首歌或全库里分出来的词正好是这个写法时用它。
//!    按写法而不是按行记，换歌词、重新导入后照样生效；勾了全库的，新歌进来也自动用上。
//! 2. **源文件注音**（`line_ruby`，origin = `source`）：TTML 的 `tts:ruby`，导入那份歌词时写入，跟着行走。
//!
//! 优先级：用户改的 > 源文件的 > 自动的。只管显示的读音，**不改分词、原形、词频和任何统计**。

use std::collections::HashMap;

use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub mod origin {
    /// 源文件自带的注音
    pub const SOURCE: &str = "source";
}

/// 某一行第 `char_start..char_end` 个字（Unicode 字符）的注音
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRuby {
    pub utterance_id: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub reading: String,
    pub origin: String,
}

/// 一条改过的读音。`song_id` 是空串时对全库生效
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReadingRule {
    pub song_id: String,
    pub surface: String,
    pub reading: String,
    /// 对全库生效（`song_id` 为空）
    pub whole_library: bool,
}

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |r| r.get(0),
    )?)
}

// ── 源文件注音 ──

/// 写一行的源文件注音：(起, 止, 读音)，按字符。同一处已有的覆盖
pub fn insert_source(conn: &Connection, utterance_id: i64, rubies: &[(usize, usize, &str)]) -> Result<usize> {
    let mut stmt = conn.prepare(&format!(
        "INSERT OR REPLACE INTO line_ruby (utterance_id, char_start, char_end, reading, origin, created_at) \
         VALUES (?1, ?2, ?3, ?4, '{}', {NOW})",
        origin::SOURCE
    ))?;
    let mut written = 0;
    for &(start, end, reading) in rubies {
        let reading = reading.trim();
        if start >= end || reading.is_empty() {
            continue;
        }
        written += stmt.execute(params![utterance_id, start as i64, end as i64, reading])?;
    }
    Ok(written)
}

/// 一首歌（当前那一版歌词）的全部注音，按行、按位置排好
pub fn for_song(conn: &Connection, song_id: &str) -> Result<Vec<LineRuby>> {
    if !table_exists(conn, "line_ruby")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT r.utterance_id, r.char_start, r.char_end, r.reading, r.origin FROM line_ruby r \
         JOIN utterances u ON u.id = r.utterance_id WHERE u.song_id = ?1 \
         ORDER BY u.line_idx, r.char_start",
    )?;
    let rows = stmt.query_map(params![song_id], |r| {
        Ok(LineRuby {
            utterance_id: r.get(0)?,
            char_start: r.get(1)?,
            char_end: r.get(2)?,
            reading: r.get(3)?,
            origin: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 一首歌的行级注音全删掉。换歌词、删歌时调用（规则按写法记，不删）
pub fn delete_for_song(conn: &Connection, song_id: &str) -> Result<usize> {
    if !table_exists(conn, "line_ruby")? {
        return Ok(0);
    }
    Ok(conn.execute(
        "DELETE FROM line_ruby WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id = ?1)",
        params![song_id],
    )?)
}

// ── 用户改的读音 ──

/// 这首歌用得上的读音规则：写法 → 读音。本首歌的规则盖过全库的
pub fn rules_for_song(conn: &Connection, song_id: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    if !table_exists(conn, "reading_rules")? {
        return Ok(out);
    }
    // 全库的先放、本首歌的后放，后放的覆盖
    let mut stmt = conn.prepare(
        "SELECT surface, reading FROM reading_rules WHERE song_id = '' OR song_id = ?1 \
         ORDER BY song_id = ?1",
    )?;
    let rows = stmt.query_map(params![song_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (surface, reading) = row?;
        out.insert(surface, reading);
    }
    Ok(out)
}

/// 这首歌里这个写法现在用的是哪条规则（本首歌的优先）。没改过是 None
pub fn rule_for(conn: &Connection, song_id: &str, surface: &str) -> Result<Option<ReadingRule>> {
    if !table_exists(conn, "reading_rules")? {
        return Ok(None);
    }
    Ok(conn
        .query_row(
            "SELECT song_id, surface, reading FROM reading_rules WHERE surface = ?2 AND (song_id = ?1 OR song_id = '') \
             ORDER BY song_id = ?1 DESC LIMIT 1",
            params![song_id, surface],
            |r| {
                let song_id: String = r.get(0)?;
                Ok(ReadingRule { whole_library: song_id.is_empty(), song_id, surface: r.get(1)?, reading: r.get(2)? })
            },
        )
        .optional()?)
}

/// 改读音。`whole_library` 时记成全库的，并去掉这首歌自己那条同写法的（不然它会盖住全库的）。
pub fn set_rule(conn: &Connection, song_id: &str, surface: &str, reading: &str, whole_library: bool) -> Result<ReadingRule> {
    let surface = surface.trim();
    let reading = reading.trim();
    ensure!(!surface.is_empty(), "没有要改读音的词");
    ensure!(!reading.is_empty(), "读音不能是空的；不想改了点「恢复自动」");
    ensure!(reading.chars().count() <= 64, "读音太长了");
    let scope = if whole_library { "" } else { song_id };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        &format!(
            "INSERT INTO reading_rules (song_id, surface, reading, updated_at) VALUES (?1, ?2, ?3, {NOW}) \
             ON CONFLICT(song_id, surface) DO UPDATE SET reading = excluded.reading, updated_at = excluded.updated_at"
        ),
        params![scope, surface, reading],
    )?;
    if whole_library && !song_id.is_empty() {
        tx.execute("DELETE FROM reading_rules WHERE song_id = ?1 AND surface = ?2", params![song_id, surface])?;
    }
    tx.commit()?;
    Ok(ReadingRule {
        song_id: scope.to_string(),
        surface: surface.to_string(),
        reading: reading.to_string(),
        whole_library,
    })
}

/// 恢复自动：去掉这首歌里这个写法现在用的那条规则（本首歌的；没有就是全库的）。
/// 返回去掉的那条；什么都没改过是 None
pub fn clear_rule(conn: &Connection, song_id: &str, surface: &str) -> Result<Option<ReadingRule>> {
    let Some(rule) = rule_for(conn, song_id, surface)? else { return Ok(None) };
    conn.execute(
        "DELETE FROM reading_rules WHERE song_id = ?1 AND surface = ?2",
        params![rule.song_id, rule.surface],
    )?;
    Ok(Some(rule))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_rules_beat_library_rules_and_clearing_falls_back() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        set_rule(conn, "", "他人", "ひと", true).unwrap();
        set_rule(conn, "001", "他人", "たにん", false).unwrap();
        assert_eq!(rules_for_song(conn, "001").unwrap()["他人"], "たにん");
        assert_eq!(rules_for_song(conn, "002").unwrap()["他人"], "ひと");

        // 恢复自动先去掉本首歌的，这时全库的生效
        let cleared = clear_rule(conn, "001", "他人").unwrap().unwrap();
        assert!(!cleared.whole_library);
        assert_eq!(rule_for(conn, "001", "他人").unwrap().unwrap().reading, "ひと");

        // 在这首歌里勾全库：记成全库的，本首歌自己那条去掉
        set_rule(conn, "001", "運命", "さだめ", false).unwrap();
        set_rule(conn, "001", "運命", "うんめい", true).unwrap();
        let rule = rule_for(conn, "001", "運命").unwrap().unwrap();
        assert!(rule.whole_library && rule.reading == "うんめい");

        assert!(set_rule(conn, "001", "他人", "  ", false).is_err());
    }

    #[test]
    fn source_rubies_go_with_the_lines() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let id: i64 = conn
            .query_row("SELECT id FROM utterances WHERE song_id='001' ORDER BY line_idx LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(insert_source(conn, id, &[(0, 1, "よ"), (2, 2, "x"), (3, 4, " ")]).unwrap(), 1);
        let rubies = for_song(conn, "001").unwrap();
        assert_eq!(rubies.len(), 1);
        assert_eq!((rubies[0].char_start, rubies[0].char_end, rubies[0].origin.as_str()), (0, 1, origin::SOURCE));
        crate::word_timing::delete_for_song(conn, "001").unwrap();
        assert!(for_song(conn, "001").unwrap().is_empty());
    }
}
