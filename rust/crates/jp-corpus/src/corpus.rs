//! 语料统计与 Research Mode 的核心查询。

use anyhow::Result;
use rusqlite::{Connection, params};

use crate::models::{
    Overview, PlayEvent, PlayedTrack, PosCount, WordExample, WordFrequency, WordInCorpus, YearStats,
};

pub(crate) fn overview(conn: &Connection) -> Result<Overview> {
    let scalar = |sql: &str| -> Result<i64> {
        Ok(conn.query_row(sql, [], |row| row.get::<_, Option<i64>>(0))?.unwrap_or(0))
    };
    let by_role = |role: &str| -> Result<i64> {
        Ok(conn.query_row(
            "SELECT COUNT(DISTINCT person_id) FROM track_credits WHERE role = ?1",
            params![role],
            |row| row.get(0),
        )?)
    };
    Ok(Overview {
        tracks: scalar("SELECT COUNT(*) FROM songs")?,
        albums: scalar("SELECT COUNT(*) FROM albums")?,
        people: scalar("SELECT COUNT(*) FROM people")?,
        performers: by_role("performer")?,
        composers: by_role("composer")?,
        lyricists: by_role("lyricist")?,
        arrangers: by_role("arranger")?,
        lyric_lines: scalar("SELECT COUNT(*) FROM utterances")?,
        tokens: scalar("SELECT COUNT(*) FROM tokens")?,
        vocabulary: scalar("SELECT COUNT(DISTINCT lemma) FROM tokens")?,
        total_duration_sec: conn.query_row(
            "SELECT COALESCE(SUM(duration_sec), 0) FROM songs",
            [],
            |row| row.get(0),
        )?,
    })
}

pub(crate) fn timeline(conn: &Connection) -> Result<Vec<YearStats>> {
    let mut stmt = conn.prepare(
        "SELECT s.year, COUNT(DISTINCT s.id), COUNT(DISTINCT s.album_id), \
                COUNT(DISTINCT t.lemma) \
         FROM songs s \
         LEFT JOIN utterances u ON u.song_id = s.id \
         LEFT JOIN tokens t ON t.utterance_id = u.id \
         WHERE s.year IS NOT NULL AND trim(s.year) <> '' \
         GROUP BY s.year ORDER BY s.year",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(YearStats {
            year: row.get(0)?,
            track_count: row.get(1)?,
            album_count: row.get(2)?,
            vocabulary: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 词频表。排除标点/符号，和 `gui.py` 的 StatsWorker 规则一致。
pub(crate) fn word_frequency(
    conn: &Connection,
    pos: Option<&str>,
    limit: i64,
) -> Result<Vec<WordFrequency>> {
    let filter = if pos.is_some() { "AND t.pos = ?1" } else { "" };
    let sql = format!(
        "SELECT t.lemma, t.pos, COUNT(*) AS freq, COUNT(DISTINCT u.song_id), \
                GROUP_CONCAT(DISTINCT t.surface) \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         WHERE t.pos NOT IN ('PUNCT','SYM','SPACE','X') {filter} \
         GROUP BY t.lemma, t.pos ORDER BY freq DESC LIMIT ?{}",
        if pos.is_some() { 2 } else { 1 }
    );
    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &rusqlite::Row| -> rusqlite::Result<WordFrequency> {
        let surfaces: Option<String> = row.get(4)?;
        Ok(WordFrequency {
            lemma: row.get(0)?,
            pos: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            freq: row.get(2)?,
            song_count: row.get(3)?,
            surfaces: surfaces
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        })
    };
    let rows = match pos {
        Some(p) => stmt.query_map(params![p, limit], map)?,
        None => stmt.query_map(params![limit], map)?,
    };
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// **Research Mode 的核心。** 点歌词里一个词，看到它在整个语料里的样子。
///
/// `current_song_id` 只影响例句排序——当前歌曲的例句排最前，
/// 这样用户点词后第一眼看到的是「这首歌里它还出现在哪」。
pub(crate) fn word_in_corpus(
    conn: &Connection,
    lemma: &str,
    current_song_id: Option<&str>,
    example_limit: i64,
) -> Result<WordInCorpus> {
    let (occurrences, song_count, artist_count) = conn.query_row(
        "SELECT COUNT(*), COUNT(DISTINCT u.song_id), COUNT(DISTINCT s.artist) \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         JOIN songs s ON s.id = u.song_id WHERE t.lemma = ?1",
        params![lemma],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let (first_year, last_year) = conn.query_row(
        "SELECT MIN(NULLIF(trim(s.year),'')), MAX(NULLIF(trim(s.year),'')) \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         JOIN songs s ON s.id = u.song_id WHERE t.lemma = ?1",
        params![lemma],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut stmt = conn.prepare(
        "SELECT COALESCE(pos,''), COUNT(*) FROM tokens WHERE lemma = ?1 \
         GROUP BY pos ORDER BY COUNT(*) DESC",
    )?;
    let pos_distribution = stmt
        .query_map(params![lemma], |row| {
            Ok(PosCount {
                pos: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let current = current_song_id.unwrap_or("");
    let mut stmt = conn.prepare(
        "SELECT u.song_id, s.artist, s.title, u.id, u.time_sec, u.text \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         JOIN songs s ON s.id = u.song_id \
         WHERE t.lemma = ?1 GROUP BY u.id \
         ORDER BY CASE WHEN u.song_id = ?2 THEN 0 ELSE 1 END, s.artist, s.title, u.time_sec \
         LIMIT ?3",
    )?;
    let examples = stmt
        .query_map(params![lemma, current, example_limit], |row| {
            Ok(WordExample {
                song_id: row.get(0)?,
                artist: row.get(1)?,
                title: row.get(2)?,
                utterance_id: row.get(3)?,
                time_sec: row.get(4)?,
                text: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(WordInCorpus {
        lemma: lemma.to_string(),
        occurrences,
        song_count,
        artist_count,
        first_year,
        last_year,
        pos_distribution,
        examples,
    })
}

// ────────────────────────────── 播放历史 ──────────────────────────────

pub(crate) fn record_play(conn: &Connection, event: &PlayEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO play_history \
           (song_id, played_at, listened_sec, position_sec, completed, source) \
         VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2, ?3, ?4, ?5)",
        params![
            event.song_id,
            event.listened_sec,
            event.position_sec,
            event.completed as i64,
            event.source,
        ],
    )?;
    Ok(())
}

pub(crate) fn recently_played(conn: &Connection, limit: i64) -> Result<Vec<PlayedTrack>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, COALESCE(s.album,''), COALESCE(s.cover_path,''), \
                COUNT(*), MAX(h.played_at), COALESCE(SUM(h.listened_sec),0) \
         FROM play_history h JOIN songs s ON s.id = h.song_id \
         GROUP BY s.id ORDER BY MAX(h.played_at) DESC LIMIT ?1",
    )?;
    collect_played(stmt.query_map(params![limit], played_from_row)?)
}

/// 播放排行。只算真的听了一会儿的，避免「点开就切走」被计入。
pub(crate) fn most_played(
    conn: &Connection,
    min_listened_sec: f64,
    limit: i64,
) -> Result<Vec<PlayedTrack>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, COALESCE(s.album,''), COALESCE(s.cover_path,''), \
                COUNT(*), MAX(h.played_at), COALESCE(SUM(h.listened_sec),0) \
         FROM play_history h JOIN songs s ON s.id = h.song_id \
         WHERE h.listened_sec >= ?1 \
         GROUP BY s.id ORDER BY COUNT(*) DESC, SUM(h.listened_sec) DESC LIMIT ?2",
    )?;
    collect_played(stmt.query_map(params![min_listened_sec, limit], played_from_row)?)
}

fn played_from_row(row: &rusqlite::Row) -> rusqlite::Result<PlayedTrack> {
    Ok(PlayedTrack {
        song_id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        cover_path: row.get(4)?,
        play_count: row.get(5)?,
        last_played: row.get(6)?,
        total_listened_sec: row.get(7)?,
    })
}

fn collect_played<I>(rows: I) -> Result<Vec<PlayedTrack>>
where
    I: Iterator<Item = rusqlite::Result<PlayedTrack>>,
{
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ────────────────────────────── 时长回填 ──────────────────────────────

/// 需要回填时长的曲目：`duration_sec` 为空且记了音频路径。
///
/// 只挑空的，所以重跑是幂等的，也不会覆盖已经写好的值。
pub(crate) fn tracks_missing_duration(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, audio_path FROM songs          WHERE duration_sec IS NULL            AND audio_path IS NOT NULL AND trim(audio_path) <> ''          ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 批量写入时长。一个事务，要么全成要么全不成——
/// 中途失败留下半套数据比一点没写更难排查。
pub(crate) fn set_durations(conn: &mut Connection, rows: &[(String, f64)]) -> Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut written = 0usize;
    {
        let mut stmt = tx.prepare("UPDATE songs SET duration_sec = ?2 WHERE id = ?1")?;
        for (song_id, seconds) in rows {
            if !seconds.is_finite() || *seconds <= 0.0 {
                continue; // 写 0 进库比留空更糟
            }
            written += stmt.execute(params![song_id, seconds])?;
        }
    }
    tx.commit()?;
    Ok(written)
}
