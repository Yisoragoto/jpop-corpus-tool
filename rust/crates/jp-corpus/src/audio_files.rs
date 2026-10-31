//! 音频文件记录：曲目（`songs`）和它的音频文件分开记。
//!
//! 曲目的身份是 `songs.id`，不跟路径走——收藏、播放记录、歌词、制卡都挂在它上面，
//! 文件挪了、换了一个音源，曲目还是那一首。这张表记的是**文件本身**：
//! 在哪、多大、什么时候改过、多长、什么格式、采样率声道码率、指纹、
//! 在文件里从哪儿开始（`track_start_ms`，给将来的 CUE 整轨留的；现在恒为 0，不解析 CUE）。
//!
//! - `songs.audio_path` 仍是「现在用的那个文件」，老版本程序照旧只读它；
//!   这里和它同路径的那一行 `status = 'present'`，换掉的旧路径记成 `replaced`，找不到的记 `missing`。
//! - 重扫时大小和修改时间都没变的文件**不再读标签**（[`stamps`]），变了的才重读。
//! - 只读文件，从不往音频文件里写东西。

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::lineage::NOW;

/// 一个音频文件的客观信息（读文件得来，和曲目元数据无关）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFacts {
    pub path: String,
    pub size: Option<u64>,
    /// 修改时间，Unix 秒（带小数）
    pub mtime: Option<f64>,
    pub duration_sec: Option<f64>,
    /// flac / mp3 / m4a / ogg / opus / wav …（小写）
    pub format: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    pub bitrate_kbps: Option<u32>,
}

/// 库里的一条音频文件记录。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFileRow {
    pub id: i64,
    pub song_id: String,
    #[serde(flatten)]
    pub facts: FileFacts,
    pub fingerprint: Option<String>,
    /// 这首在文件里从哪儿开始（毫秒）。整轨 + CUE 时不为 0；现在恒为 0
    pub track_start_ms: i64,
    /// present / missing / replaced
    pub status: String,
    pub first_seen: String,
    pub checked_at: String,
}

fn table_exists(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='audio_files'", [], |_| Ok(()))
        .optional()?
        .is_some())
}

/// 路径比较键：分隔符统一，Windows 上忽略大小写（和导入计划的判据一样）。
pub fn path_key(path: &str) -> String {
    let unified = path.replace('\\', "/");
    if cfg!(windows) { unified.to_lowercase() } else { unified }
}

/// 两次读到的文件是不是同一个状态：大小一样、修改时间差不到 1 秒（FAT/网络盘只精确到秒或两秒）。
pub fn same_stamp(a: (Option<u64>, Option<f64>), b: (Option<u64>, Option<f64>)) -> bool {
    match (a, b) {
        ((Some(sa), Some(ma)), (Some(sb), Some(mb))) => sa == sb && (ma - mb).abs() < 2.0,
        _ => false,
    }
}

/// 记下（或更新）一首歌现在用的文件。同一首歌别的 present 行改成 replaced。
/// `fingerprint` 为 None 时保留原来的。返回这一行的 id。
pub fn record(conn: &Connection, song_id: &str, facts: &FileFacts, fingerprint: Option<&str>) -> Result<i64> {
    if !table_exists(conn)? {
        return Ok(0);
    }
    conn.execute(
        &format!(
            "UPDATE audio_files SET status='replaced', checked_at={NOW} \
             WHERE song_id=?1 AND path<>?2 AND status='present'"
        ),
        params![song_id, facts.path],
    )?;
    conn.execute(
        &format!("INSERT INTO audio_files (song_id, path, size, mtime, duration_sec, format, sample_rate, channels, \
                                  bit_depth, bitrate_kbps, fingerprint, status, first_seen, checked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'present', {NOW}, {NOW}) \
         ON CONFLICT(song_id, path) DO UPDATE SET \
           size=excluded.size, mtime=excluded.mtime, \
           duration_sec=COALESCE(excluded.duration_sec, audio_files.duration_sec), \
           format=CASE WHEN excluded.format='' THEN audio_files.format ELSE excluded.format END, \
           sample_rate=COALESCE(excluded.sample_rate, audio_files.sample_rate), \
           channels=COALESCE(excluded.channels, audio_files.channels), \
           bit_depth=COALESCE(excluded.bit_depth, audio_files.bit_depth), \
           bitrate_kbps=COALESCE(excluded.bitrate_kbps, audio_files.bitrate_kbps), \
           fingerprint=COALESCE(excluded.fingerprint, audio_files.fingerprint), \
           status='present', checked_at=excluded.checked_at"),
        params![
            song_id,
            facts.path,
            facts.size.map(|s| s as i64),
            facts.mtime,
            facts.duration_sec,
            facts.format,
            facts.sample_rate,
            facts.channels,
            facts.bit_depth,
            facts.bitrate_kbps,
            fingerprint,
        ],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM audio_files WHERE song_id=?1 AND path=?2",
        params![song_id, facts.path],
        |r| r.get(0),
    )?)
}

/// 文件内容变了：旧指纹作废（等重算）。
pub fn clear_fingerprint(conn: &Connection, song_id: &str, path: &str) -> Result<()> {
    if table_exists(conn)? {
        conn.execute(
            "UPDATE audio_files SET fingerprint=NULL WHERE song_id=?1 AND path=?2",
            params![song_id, path],
        )?;
    }
    Ok(())
}

/// 文件找不到了（不删记录：路径、大小、指纹留着给「找回音频」比对）。
pub fn mark_missing(conn: &Connection, song_id: &str, path: &str) -> Result<()> {
    if table_exists(conn)? {
        conn.execute(
            &format!("UPDATE audio_files SET status='missing', checked_at={NOW} WHERE song_id=?1 AND path=?2 AND status<>'missing'"),
            params![song_id, path],
        )?;
    }
    Ok(())
}

/// 一首歌的全部文件记录，现在用的排最前。
pub fn for_song(conn: &Connection, song_id: &str) -> Result<Vec<AudioFileRow>> {
    if !table_exists(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, song_id, path, size, mtime, duration_sec, format, sample_rate, channels, bit_depth, bitrate_kbps, \
                fingerprint, track_start_ms, status, first_seen, checked_at \
         FROM audio_files WHERE song_id=?1 \
         ORDER BY CASE status WHEN 'present' THEN 0 WHEN 'missing' THEN 1 ELSE 2 END, checked_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([song_id], |r| {
            Ok(AudioFileRow {
                id: r.get(0)?,
                song_id: r.get(1)?,
                facts: FileFacts {
                    path: r.get(2)?,
                    size: r.get::<_, Option<i64>>(3)?.map(|s| s.max(0) as u64),
                    mtime: r.get(4)?,
                    duration_sec: r.get(5)?,
                    format: r.get(6)?,
                    sample_rate: r.get(7)?,
                    channels: r.get(8)?,
                    bit_depth: r.get(9)?,
                    bitrate_kbps: r.get(10)?,
                },
                fingerprint: r.get(11)?,
                track_start_ms: r.get(12)?,
                status: r.get(13)?,
                first_seen: r.get(14)?,
                checked_at: r.get(15)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// 文件大小和修改时间（秒）
pub type Stamp = (Option<u64>, Option<f64>);
/// 回填用：歌 id、路径、时长、指纹、加入时间
type BackfillRow = (String, String, Option<f64>, Option<String>, String);

/// 现在在用的文件：路径键 → (大小, 修改时间)。重扫时拿它判断「没变，不用再读标签」。
pub fn stamps(conn: &Connection) -> Result<HashMap<String, Stamp>> {
    if !table_exists(conn)? {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare("SELECT path, size, mtime FROM audio_files WHERE status='present'")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                path_key(&r.get::<_, String>(0)?),
                (r.get::<_, Option<i64>>(1)?.map(|s| s.max(0) as u64), r.get::<_, Option<f64>>(2)?),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// 这首歌在文件里的起点（毫秒）。没有记录就是 0。
pub fn track_start_ms(conn: &Connection, song_id: &str) -> Result<i64> {
    if !table_exists(conn)? {
        return Ok(0);
    }
    Ok(conn
        .query_row(
            "SELECT track_start_ms FROM audio_files WHERE song_id=?1 AND status='present' ORDER BY id DESC LIMIT 1",
            [song_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub fn forget(conn: &Connection, song_id: &str) -> Result<()> {
    if table_exists(conn)? {
        conn.execute("DELETE FROM audio_files WHERE song_id=?1", [song_id])?;
    }
    Ok(())
}

/// 升级前就在库里的歌：按 `songs.audio_path` 补一条文件记录。**不碰磁盘**——
/// 大小、修改时间留空，下次重扫时补齐（留空的那次照常读标签，之后就能跳过了）。
/// 时长取 songs 里的，指纹取 songs.audio_fingerprint。在一个事务里调；重复调是空操作。
pub fn backfill(conn: &Connection) -> Result<usize> {
    if !table_exists(conn)? {
        return Ok(0);
    }
    let rows: Vec<BackfillRow> = {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.audio_path, s.duration_sec, s.audio_fingerprint, COALESCE(s.added_at, '') FROM songs s \
             WHERE COALESCE(s.audio_path, '') <> '' AND NOT EXISTS (SELECT 1 FROM audio_files a WHERE a.song_id = s.id)",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (song_id, path, duration, fingerprint, added) in &rows {
        conn.execute(
            "INSERT INTO audio_files (song_id, path, duration_sec, format, fingerprint, status, first_seen, checked_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'present', ?6, '')",
            params![song_id, path, duration, format_of(path), fingerprint, added],
        )?;
    }
    Ok(rows.len())
}

/// 扩展名当格式（小写，不带点）
pub fn format_of(path: &str) -> String {
    std::path::Path::new(&path.replace('\\', "/"))
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> crate::Corpus {
        let corpus = crate::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        corpus.connection().execute_batch(
            "INSERT INTO songs (id, title, artist, audio_path, duration_sec, audio_fingerprint, added_at) \
             VALUES ('001', '夜行', 'ヨルシカ', 'D:\\music\\夜行.flac', 230.5, 'abc', '2026-01-01T00:00:00Z'), \
                    ('002', '無題', 'X', '', NULL, NULL, NULL);",
        )
        .unwrap();
        corpus
    }

    fn facts(path: &str, size: u64, mtime: f64) -> FileFacts {
        FileFacts { path: path.into(), size: Some(size), mtime: Some(mtime), duration_sec: Some(230.4), format: "flac".into(), sample_rate: Some(44100), channels: Some(2), bit_depth: Some(16), bitrate_kbps: Some(900) }
    }

    #[test]
    fn backfill_registers_existing_audio_without_touching_the_disk() {
        let corpus = db();
        let conn = corpus.connection();
        assert_eq!(backfill(conn).unwrap(), 1, "没有音频路径的不登记");
        assert_eq!(backfill(conn).unwrap(), 0, "重复调是空操作");
        let rows = for_song(conn, "001").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].facts.format, "flac");
        assert_eq!(rows[0].facts.duration_sec, Some(230.5));
        assert_eq!(rows[0].fingerprint.as_deref(), Some("abc"));
        assert_eq!(rows[0].facts.size, None, "大小留给下次重扫");
        assert_eq!(rows[0].track_start_ms, 0);
        // 没有大小和修改时间的不算「没变」，下次重扫照常读标签
        let stamp = stamps(conn).unwrap()[&path_key("D:\\music\\夜行.flac")];
        assert!(!same_stamp(stamp, (Some(1), Some(1.0))));
    }

    #[test]
    fn a_new_file_replaces_the_old_one_and_keeps_its_record() {
        let corpus = db();
        let conn = corpus.connection();
        backfill(conn).unwrap();
        record(conn, "001", &facts("D:\\music\\夜行.flac", 10, 100.0), None).unwrap();
        assert_eq!(for_song(conn, "001").unwrap()[0].fingerprint.as_deref(), Some("abc"), "指纹没给就保留");
        record(conn, "001", &facts("E:\\new\\夜行.mp3", 5, 200.0), Some("def")).unwrap();
        let rows = for_song(conn, "001").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].facts.path.as_str(), rows[0].status.as_str()), ("E:\\new\\夜行.mp3", "present"));
        assert_eq!(rows[1].status, "replaced");
        let stamps = stamps(conn).unwrap();
        assert_eq!(stamps.len(), 1, "只有在用的文件参与「没变就跳过」");
        assert!(same_stamp(stamps[&path_key("E:\\new\\夜行.mp3")], (Some(5), Some(200.9))));
        assert!(!same_stamp(stamps[&path_key("E:\\new\\夜行.mp3")], (Some(6), Some(200.0))));
        mark_missing(conn, "001", "E:\\new\\夜行.mp3").unwrap();
        assert_eq!(for_song(conn, "001").unwrap()[0].status, "missing");
        forget(conn, "001").unwrap();
        assert!(for_song(conn, "001").unwrap().is_empty());
    }
}
