//! 词级时间：一行歌词里某个词从第几秒开始唱。
//!
//! 行级时间（`utterances.time_sec`）只够「从这一行开始放」。词云、例句要的是**从这个词开始放**，
//! 以后的逐字歌词（唱到哪亮到哪）也要同一份数据。来源两种，存在同一张表 `word_timings`：
//!
//! - `lrc`：导入的增强型 LRC 里的逐字时间 `<00:12.34>`，导入时写进来；
//! - `manual`：在界面里听着按一下校准的。**同一处两种都有时以 `manual` 为准**——
//!   校准就是因为原来的不准。
//!
//! 位置用这一行 `text` 里的**字符下标**（Unicode 标量，不是字节，也不是 UTF-16），
//! `[char_start, char_end)`。歌词行被删（换歌词、删歌）时这一行的词级时间跟着删。
//! 没有完整可信时间单位时，界面回退逐行/纯文本，不估计字符时间。

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

pub mod source {
    /// 增强型 LRC 的逐字时间
    pub const LRC: &str = "lrc";
    /// 手动校准
    pub const MANUAL: &str = "manual";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordTiming {
    pub utterance_id: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub source: String,
}

/// 一段逐字时间（导入时用）：这一行第 `start..end` 个字从 `time_sec` 开始
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub time_sec: f64,
    pub end_sec: Option<f64>,
}

/// 这几行的词级时间，按行、按位置排好。
pub fn for_utterances(conn: &Connection, utterance_ids: &[i64]) -> Result<Vec<WordTiming>> {
    if utterance_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    // SQLite 的参数上限是 999（老版本），分批查
    for chunk in utterance_ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT utterance_id, char_start, char_end, start_sec, end_sec, source FROM word_timings \
             WHERE utterance_id IN ({marks}) ORDER BY utterance_id, char_start, source"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            Ok(WordTiming {
                utterance_id: r.get(0)?,
                char_start: r.get(1)?,
                char_end: r.get(2)?,
                start_sec: r.get(3)?,
                end_sec: r.get(4)?,
                source: r.get(5)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// 一首歌（当前那一版歌词）的全部词级时间。
pub fn for_song(conn: &Connection, song_id: &str) -> Result<Vec<WordTiming>> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM utterances WHERE song_id = ?1 ORDER BY line_idx")?
        .query_map(params![song_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for_utterances(conn, &ids)
}

/// 记一处手动校准：这一行第 `char_start..char_end` 个字从 `start_sec` 开始。同一处再校准就覆盖。
pub fn set_manual(conn: &Connection, utterance_id: i64, char_start: i64, char_end: i64, start_sec: f64) -> Result<()> {
    anyhow::ensure!(start_sec.is_finite() && start_sec >= 0.0, "时间不对：{start_sec}");
    anyhow::ensure!(char_start >= 0 && char_end > char_start, "位置不对：{char_start}..{char_end}");
    let text: Option<String> = conn
        .query_row("SELECT text FROM utterances WHERE id = ?1", params![utterance_id], |r| r.get(0))
        .ok();
    let Some(text) = text else { anyhow::bail!("找不到这一行歌词（{utterance_id}），可能歌词刚换过") };
    anyhow::ensure!(char_end as usize <= text.chars().count(), "位置超出这一行：{char_end} > {}", text.chars().count());
    conn.execute(
        "INSERT OR REPLACE INTO word_timings (utterance_id, char_start, char_end, start_sec, end_sec, source) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
        params![utterance_id, char_start, char_end, start_sec, source::MANUAL],
    )?;
    Ok(())
}

/// 撤掉一处手动校准（回到来源时间或者整行定位）。
pub fn clear_manual(conn: &Connection, utterance_id: i64, char_start: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM word_timings WHERE utterance_id = ?1 AND char_start = ?2 AND source = ?3",
        params![utterance_id, char_start, source::MANUAL],
    )?;
    Ok(())
}

/// 导入时写一行的逐字时间（来源 `lrc`）。优先保留显式结束时间；没有才取下一段开始。
pub fn insert_lrc(conn: &Connection, utterance_id: i64, segments: &[Segment]) -> Result<usize> {
    let mut written = 0;
    for (i, seg) in segments.iter().enumerate() {
        if seg.end <= seg.start {
            continue;
        }
        let end_sec = seg.end_sec.or_else(|| segments.get(i + 1).map(|next| next.time_sec));
        conn.execute(
            "INSERT OR REPLACE INTO word_timings (utterance_id, char_start, char_end, start_sec, end_sec, source) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![utterance_id, seg.start as i64, seg.end as i64, seg.time_sec, end_sec, source::LRC],
        )?;
        written += 1;
    }
    Ok(written)
}

/// 删歌词行之前调：这首歌所有行的词级时间一起删（表不在就什么都不做，老库还没建）。
pub fn delete_for_song(conn: &Connection, song_id: &str) -> Result<usize> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'word_timings')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    Ok(conn.execute(
        "DELETE FROM word_timings WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id = ?1)",
        params![song_id],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::create_tables_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist) VALUES ('001', 't', 'a');
             INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES (1, '001', 0, 10.0, '雨が降る夜に'), (2, '001', 1, 14.0, 'ねえ');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn manual_calibration_overrides_and_can_be_cleared() {
        let conn = db();
        insert_lrc(&conn, 1, &[Segment { start: 0, end: 2, time_sec: 10.1, end_sec: None }, Segment { start: 4, end: 6, time_sec: 11.2, end_sec: None }]).unwrap();
        set_manual(&conn, 1, 4, 5, 11.45).unwrap();
        let rows = for_song(&conn, "001").unwrap();
        let at4: Vec<_> = rows.iter().filter(|r| r.char_start == 4).map(|r| (r.source.as_str(), r.start_sec)).collect();
        assert_eq!(at4, vec![("lrc", 11.2), ("manual", 11.45)]);
        assert_eq!(rows[0].end_sec, Some(11.2), "逐字时间的结束取下一段的开始");

        // 再校准一次覆盖，不会多一行
        set_manual(&conn, 1, 4, 5, 11.4).unwrap();
        assert_eq!(for_song(&conn, "001").unwrap().len(), 3);

        clear_manual(&conn, 1, 4).unwrap();
        assert!(for_song(&conn, "001").unwrap().iter().all(|r| r.source == "lrc"));
    }

    #[test]
    fn bad_positions_and_missing_lines_are_refused() {
        let conn = db();
        assert!(set_manual(&conn, 1, 5, 9, 11.0).is_err(), "超出这一行");
        assert!(set_manual(&conn, 1, 3, 3, 11.0).is_err(), "空区间");
        assert!(set_manual(&conn, 99, 0, 1, 1.0).is_err(), "没有这一行");
        assert!(set_manual(&conn, 2, 0, 2, f64::NAN).is_err());
    }

    #[test]
    fn deleting_a_song_takes_its_timings_along() {
        let conn = db();
        set_manual(&conn, 2, 0, 2, 14.1).unwrap();
        assert_eq!(delete_for_song(&conn, "001").unwrap(), 1);
        assert!(for_song(&conn, "001").unwrap().is_empty());
    }
}
