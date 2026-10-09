//! 刮削状态的持久化。**整个 crate 里唯一碰 SQLite 的模块。**
//!
//! 要解决的核心问题：刮削完的状态原来只在对话框里显示一次就丢了，
//! 关掉窗口就再也无法回答「我那 12 首为什么没刮出来」。
//!
//! 三张表，全部是新增，不动 `songs` / `utterances` / `tokens` 的任何一列：
//!
//! | 表 | 内容 |
//! |---|---|
//! | `scrape_state` | 每个文件的当前状态（一行一文件） |
//! | `scrape_attempts` | 每次尝试的历史（一行一次，用于排查反复失败） |
//! | `track_original_metadata` | 原始 + 归一化元数据（需求：永远不覆盖原值） |
//!
//! 写入一律幂等：同一个文件跑两遍不会产生第二行。

use anyhow::{Context, Result};
use jp_normalize::{normalize_album, normalize_artist, normalize_title};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;

use crate::models::{ResolvedTrack, TrackFile, utc_now};
use crate::status::{ErrorType, ScrapeStatus};

/// 表结构本身不带版本号：版本由 `legacy/scripts/migrate_db.py` 的
/// `PRAGMA user_version` 统一管理，避免两处各记一套、互相打架。
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS scrape_state (
    file_path       TEXT PRIMARY KEY,
    song_id         TEXT,
    status          TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 0,
    provider        TEXT NOT NULL DEFAULT '',
    provider_id     TEXT NOT NULL DEFAULT '',
    error_type      TEXT NOT NULL DEFAULT '',
    error_message   TEXT NOT NULL DEFAULT '',
    retry_count     INTEGER NOT NULL DEFAULT 0,
    first_seen_at   TEXT,
    last_attempt_at TEXT,
    resolved_at     TEXT,
    breakdown_json  TEXT,
    candidates_json TEXT,
    queries_json    TEXT
);

CREATE TABLE IF NOT EXISTS scrape_attempts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path     TEXT NOT NULL,
    provider      TEXT NOT NULL DEFAULT '',
    status        TEXT NOT NULL,
    confidence    REAL NOT NULL DEFAULT 0,
    error_type    TEXT NOT NULL DEFAULT '',
    error_message TEXT NOT NULL DEFAULT '',
    attempted_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS track_original_metadata (
    file_path         TEXT PRIMARY KEY,
    song_id           TEXT,
    title             TEXT NOT NULL DEFAULT '',
    artist            TEXT NOT NULL DEFAULT '',
    album             TEXT NOT NULL DEFAULT '',
    year              TEXT NOT NULL DEFAULT '',
    genre             TEXT NOT NULL DEFAULT '',
    track_number      INTEGER,
    disc_number       INTEGER,
    duration_sec      REAL,
    size              INTEGER,
    mtime             REAL,
    normalized_title  TEXT NOT NULL DEFAULT '',
    normalized_artist TEXT NOT NULL DEFAULT '',
    normalized_album  TEXT NOT NULL DEFAULT '',
    source_json       TEXT,
    captured_at       TEXT
);

CREATE INDEX IF NOT EXISTS idx_scrape_state_status  ON scrape_state(status);
CREATE INDEX IF NOT EXISTS idx_scrape_state_song    ON scrape_state(song_id);
CREATE INDEX IF NOT EXISTS idx_scrape_attempts_file ON scrape_attempts(file_path, attempted_at);
CREATE INDEX IF NOT EXISTS idx_tom_song             ON track_original_metadata(song_id);
"#;

/// `scrape_state` 的一行，读出来的形态。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeRecord {
    pub file_path: String,
    pub status: ScrapeStatus,
    pub song_id: String,
    pub confidence: f64,
    pub provider: String,
    pub provider_id: String,
    pub error_type: ErrorType,
    pub error_message: String,
    pub retry_count: i64,
    pub first_seen_at: String,
    pub last_attempt_at: String,
    pub resolved_at: String,
    pub breakdown: Option<serde_json::Value>,
    pub candidates: Option<serde_json::Value>,
    pub queries: Option<serde_json::Value>,
}

/// 一次尝试的历史记录。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeAttempt {
    pub id: i64,
    pub file_path: String,
    pub provider: String,
    pub status: ScrapeStatus,
    pub confidence: f64,
    pub error_type: ErrorType,
    pub error_message: String,
    pub attempted_at: String,
}

/// 各状态的数量。导入页顶部的「962 已匹配 / 43 需确认」就用它。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeSummary {
    pub pending: i64,
    pub running: i64,
    pub success: i64,
    pub low_confidence: i64,
    pub failed: i64,
    pub skipped: i64,
    pub total: i64,
}

/// 刮削状态的读写。借用外部连接，事务边界由调用方决定。
pub struct ScrapeStore<'a> {
    conn: &'a Connection,
}

impl<'a> ScrapeStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// 建表。可以反复调用，已存在就什么都不做。
    pub fn ensure_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA).context("建刮削状态表失败")
    }

    // ── 写 ──

    /// 登记一个文件：写入原始元数据，状态置 `Pending`。
    ///
    /// 幂等：重复登记只更新原始元数据（文件可能被重新打过 tag），
    /// **不会重置已有的状态，也不会清掉 `retry_count`**。
    pub fn register(&self, track: &TrackFile, song_id: &str) -> Result<()> {
        let now = utc_now();
        let title = normalize_title(track.title());
        let artist = normalize_artist(track.artist());
        let album = normalize_album(track.album());
        let source_json = serde_json::to_string(track).unwrap_or_default();

        self.conn.execute(
            "INSERT INTO track_original_metadata
                (file_path, song_id, title, artist, album, year, genre,
                 track_number, disc_number, duration_sec, size, mtime,
                 normalized_title, normalized_artist, normalized_album,
                 source_json, captured_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(file_path) DO UPDATE SET
                song_id           = COALESCE(NULLIF(excluded.song_id, ''), song_id),
                title             = excluded.title,
                artist            = excluded.artist,
                album             = excluded.album,
                year              = excluded.year,
                genre             = excluded.genre,
                track_number      = excluded.track_number,
                disc_number       = excluded.disc_number,
                duration_sec      = excluded.duration_sec,
                size              = excluded.size,
                mtime             = excluded.mtime,
                normalized_title  = excluded.normalized_title,
                normalized_artist = excluded.normalized_artist,
                normalized_album  = excluded.normalized_album,
                source_json       = excluded.source_json,
                captured_at       = excluded.captured_at",
            params![
                track.path,
                song_id,
                track.title(),
                track.artist(),
                track.album(),
                track.embedded_year,
                track.embedded_genre,
                track.track_number,
                track.disc_number,
                track.duration_sec,
                track.size,
                track.mtime,
                title.normalized,
                artist.normalized,
                album.normalized,
                source_json,
                now,
            ],
        )?;

        self.conn.execute(
            "INSERT INTO scrape_state (file_path, song_id, status, first_seen_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(file_path) DO UPDATE SET
                song_id = COALESCE(NULLIF(excluded.song_id, ''), song_id)",
            params![track.path, song_id, ScrapeStatus::Pending.as_str(), now],
        )?;
        Ok(())
    }

    pub fn mark_running(&self, file_path: &str) -> Result<()> {
        self.set_status(file_path, ScrapeStatus::Running, "", true)
    }

    /// 用户显式跳过。重跑时不会再自动处理它。
    pub fn mark_skipped(&self, file_path: &str, reason: &str) -> Result<()> {
        self.set_status(file_path, ScrapeStatus::Skipped, reason, false)
    }

    /// 落一次解析结果：更新当前状态 + 追加一条历史。
    pub fn record(&self, result: &ResolvedTrack, song_id: &str) -> Result<()> {
        let now = utc_now();
        let resolved_at = (result.status == ScrapeStatus::Success).then(|| now.clone());
        let breakdown_json = result
            .breakdown()
            .and_then(|b| serde_json::to_string(b).ok());
        let candidates_json = serde_json::to_string(&result.candidates).ok();
        let queries_json = serde_json::to_string(&result.queries_tried).ok();
        let provider_id = result
            .candidate
            .as_ref()
            .map(|c| c.provider_id.clone())
            .unwrap_or_default();

        self.conn.execute(
            "INSERT INTO scrape_state
                (file_path, song_id, status, confidence, provider, provider_id,
                 error_type, error_message, retry_count, first_seen_at,
                 last_attempt_at, resolved_at, breakdown_json, candidates_json,
                 queries_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(file_path) DO UPDATE SET
                song_id         = COALESCE(NULLIF(excluded.song_id, ''), song_id),
                status          = excluded.status,
                confidence      = excluded.confidence,
                provider        = excluded.provider,
                provider_id     = excluded.provider_id,
                error_type      = excluded.error_type,
                error_message   = excluded.error_message,
                -- 只有失败才累加重试计数，成功不该把历史失败次数清零
                retry_count     = scrape_state.retry_count
                                  + CASE WHEN excluded.status = ?15 THEN 1 ELSE 0 END,
                last_attempt_at = excluded.last_attempt_at,
                resolved_at     = COALESCE(excluded.resolved_at, scrape_state.resolved_at),
                breakdown_json  = excluded.breakdown_json,
                candidates_json = excluded.candidates_json,
                queries_json    = excluded.queries_json",
            params![
                result.file_path,
                song_id,
                result.status.as_str(),
                result.confidence,
                result.provider(),
                provider_id,
                result.error_type.as_str(),
                result.error_message,
                now,
                now,
                resolved_at,
                breakdown_json,
                candidates_json,
                queries_json,
                ScrapeStatus::Failed.as_str(),
            ],
        )?;

        self.conn.execute(
            "INSERT INTO scrape_attempts
                (file_path, provider, status, confidence, error_type, error_message, attempted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                result.file_path,
                result.provider(),
                result.status.as_str(),
                result.confidence,
                result.error_type.as_str(),
                result.error_message,
                now,
            ],
        )?;
        Ok(())
    }

    /// 把文件和 `songs.id` 关联起来。导入完成后调用。
    pub fn link_song(&self, file_path: &str, song_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scrape_state SET song_id=?1 WHERE file_path=?2",
            params![song_id, file_path],
        )?;
        self.conn.execute(
            "UPDATE track_original_metadata SET song_id=?1 WHERE file_path=?2",
            params![song_id, file_path],
        )?;
        Ok(())
    }

    /// 把状态改回 `Pending`，用于重刮。
    ///
    /// **保留 `retry_count` 和历史**——「这首歌之前失败过 5 次」是有价值的信息。
    pub fn reset(&self, file_paths: Option<&[String]>) -> Result<usize> {
        let changed = match file_paths {
            None => self.conn.execute(
                "UPDATE scrape_state SET status=?1, error_type='', error_message=''",
                params![ScrapeStatus::Pending.as_str()],
            )?,
            // 空列表和 None 的语义完全不同：一个是「什么都不动」，
            // 一个是「全部重置」。落到下面的 IN () 会拼出非法 SQL。
            Some([]) => 0,
            Some(paths) => {
                let marks = std::iter::repeat_n("?", paths.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "UPDATE scrape_state SET status=?, error_type='', error_message='' \
                     WHERE file_path IN ({marks})"
                );
                let mut values: Vec<String> = vec![ScrapeStatus::Pending.as_str().to_string()];
                values.extend_from_slice(paths);
                self.conn.execute(&sql, params_from_iter(values))?
            }
        };
        Ok(changed)
    }

    /// 文件被删除时清掉它的记录。历史一并删除。
    pub fn forget(&self, file_path: &str) -> Result<()> {
        for table in ["scrape_state", "scrape_attempts", "track_original_metadata"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE file_path=?1"),
                params![file_path],
            )?;
        }
        Ok(())
    }

    fn set_status(
        &self,
        file_path: &str,
        status: ScrapeStatus,
        error_message: &str,
        touch_attempt: bool,
    ) -> Result<()> {
        let now = utc_now();
        self.conn.execute(
            "INSERT INTO scrape_state
                (file_path, status, error_message, first_seen_at, last_attempt_at)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(file_path) DO UPDATE SET
                status          = excluded.status,
                error_message   = excluded.error_message,
                last_attempt_at = CASE WHEN ?6 THEN excluded.last_attempt_at
                                       ELSE scrape_state.last_attempt_at END",
            params![
                file_path,
                status.as_str(),
                error_message,
                now,
                now,
                touch_attempt
            ],
        )?;
        Ok(())
    }

    // ── 读 ──

    pub fn get(&self, file_path: &str) -> Result<Option<ScrapeRecord>> {
        Ok(self
            .conn
            .query_row(
                "SELECT file_path, song_id, status, confidence, provider, provider_id,
                        error_type, error_message, retry_count, first_seen_at,
                        last_attempt_at, resolved_at, breakdown_json, candidates_json,
                        queries_json
                 FROM scrape_state WHERE file_path=?1",
                params![file_path],
                row_to_record,
            )
            .optional()?)
    }

    /// 按状态取。复核页面取 `LowConfidence`，重试取 `Failed`。
    pub fn by_status(
        &self,
        statuses: &[ScrapeStatus],
        limit: Option<i64>,
    ) -> Result<Vec<ScrapeRecord>> {
        let statuses: Vec<&str> = if statuses.is_empty() {
            ScrapeStatus::ALL.iter().map(|s| s.as_str()).collect()
        } else {
            statuses.iter().map(|s| s.as_str()).collect()
        };
        let marks = std::iter::repeat_n("?", statuses.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut sql = format!(
            "SELECT file_path, song_id, status, confidence, provider, provider_id,
                    error_type, error_message, retry_count, first_seen_at,
                    last_attempt_at, resolved_at, breakdown_json, candidates_json,
                    queries_json
             FROM scrape_state WHERE status IN ({marks})
             ORDER BY confidence DESC, file_path"
        );
        let mut values: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?");
            values.push(limit.to_string());
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(values), row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn summary(&self) -> Result<ScrapeSummary> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM scrape_state GROUP BY status")?;
        let mut out = ScrapeSummary::default();
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (status, n) = row?;
            match ScrapeStatus::from_str_lossy(&status) {
                ScrapeStatus::Pending => out.pending = n,
                ScrapeStatus::Running => out.running = n,
                ScrapeStatus::Success => out.success = n,
                ScrapeStatus::LowConfidence => out.low_confidence = n,
                ScrapeStatus::Failed => out.failed = n,
                ScrapeStatus::Skipped => out.skipped = n,
            }
            out.total += n;
        }
        Ok(out)
    }

    /// 某个文件的尝试历史，最近的在前。
    pub fn attempts(&self, file_path: &str, limit: i64) -> Result<Vec<ScrapeAttempt>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_path, provider, status, confidence, error_type,
                    error_message, attempted_at
             FROM scrape_attempts WHERE file_path=?1
             ORDER BY attempted_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![file_path, limit], |row| {
                Ok(ScrapeAttempt {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    provider: row.get(2)?,
                    status: ScrapeStatus::from_str_lossy(&row.get::<_, String>(3)?),
                    confidence: row.get(4)?,
                    error_type: ErrorType::from_str_lossy(&row.get::<_, String>(5)?),
                    error_message: row.get(6)?,
                    attempted_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 取原始元数据。**重新匹配时要用它**，而不是用已经被覆盖过的 `songs` 行。
    pub fn original_metadata(&self, file_path: &str) -> Result<Option<TrackFile>> {
        let source: Option<String> = self
            .conn
            .query_row(
                "SELECT source_json FROM track_original_metadata WHERE file_path=?1",
                params![file_path],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(source.and_then(|s| serde_json::from_str(&s).ok()))
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScrapeRecord> {
    let parse = |text: Option<String>| text.and_then(|t| serde_json::from_str(&t).ok());
    Ok(ScrapeRecord {
        file_path: row.get(0)?,
        song_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        status: ScrapeStatus::from_str_lossy(&row.get::<_, String>(2)?),
        confidence: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0),
        provider: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        provider_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        error_type: ErrorType::from_str_lossy(
            &row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        ),
        error_message: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        retry_count: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
        first_seen_at: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
        last_attempt_at: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
        resolved_at: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        breakdown: parse(row.get(12)?),
        candidates: parse(row.get(13)?),
        queries: parse(row.get(14)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MatchBreakdown, ScrapeCandidate};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ScrapeStore::new(&conn).ensure_schema().unwrap();
        conn
    }

    fn track(path: &str) -> TrackFile {
        TrackFile {
            path: path.into(),
            embedded_title: "夜に駆ける".into(),
            embedded_artist: "ＹＯＡＳＯＢＩ".into(),
            embedded_album: "THE BOOK".into(),
            duration_sec: Some(261.0),
            track_number: Some(1),
            ..Default::default()
        }
    }

    fn success(path: &str) -> ResolvedTrack {
        ResolvedTrack {
            file_path: path.into(),
            status: ScrapeStatus::Success,
            confidence: 0.97,
            candidate: Some(ScrapeCandidate {
                provider: "itunes".into(),
                provider_id: "123".into(),
                title: "夜に駆ける".into(),
                breakdown: Some(MatchBreakdown {
                    final_score: 0.97,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            queries_tried: vec!["full:YOASOBI 夜に駆ける".into()],
            matched_at: utc_now(),
            ..Default::default()
        }
    }

    fn failure(path: &str) -> ResolvedTrack {
        ResolvedTrack {
            file_path: path.into(),
            status: ScrapeStatus::Failed,
            error_type: ErrorType::RateLimit,
            error_message: "被限流".into(),
            matched_at: utc_now(),
            ..Default::default()
        }
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let conn = db();
        // 反复建表不该报错
        ScrapeStore::new(&conn).ensure_schema().unwrap();
        ScrapeStore::new(&conn).ensure_schema().unwrap();
    }

    #[test]
    fn register_stores_the_original_and_the_normalized_form() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "001").unwrap();

        let (original, normalized): (String, String) = conn
            .query_row(
                "SELECT artist, normalized_artist FROM track_original_metadata",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // 原值一个字都不能改
        assert_eq!(original, "ＹＯＡＳＯＢＩ");
        // 归一化的另存一列
        assert_eq!(normalized, "yoasobi");
    }

    #[test]
    fn register_twice_does_not_reset_an_existing_status() {
        // 重新扫描不该把已经刮好的歌打回待刮
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "001").unwrap();
        store.record(&success("D:/a/1.flac"), "001").unwrap();
        store.register(&track("D:/a/1.flac"), "001").unwrap();

        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(got.status, ScrapeStatus::Success);
        assert_eq!(store.summary().unwrap().total, 1, "不该产生第二行");
    }

    #[test]
    fn retry_count_only_grows_on_failure() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        assert_eq!(store.get("D:/a/1.flac").unwrap().unwrap().retry_count, 2);

        // 成功了也不该把历史失败次数清零——那是有价值的信息
        store.record(&success("D:/a/1.flac"), "").unwrap();
        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(got.retry_count, 2);
        assert_eq!(got.status, ScrapeStatus::Success);
    }

    #[test]
    fn every_attempt_is_appended_to_the_history() {
        // 「我那 12 首为什么反复失败」要能回答
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&success("D:/a/1.flac"), "").unwrap();

        let history = store.attempts("D:/a/1.flac", 10).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].status, ScrapeStatus::Success, "最近的在前");
        assert_eq!(history[2].error_type, ErrorType::RateLimit);
    }

    #[test]
    fn a_success_records_when_it_resolved() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&success("D:/a/1.flac"), "001").unwrap();
        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert!(!got.resolved_at.is_empty());
        assert_eq!(got.provider, "itunes");
        assert_eq!(got.provider_id, "123");
        // 打分要能读回来解释
        assert!(got.breakdown.is_some());
        assert!(got.queries.is_some());
    }

    #[test]
    fn a_later_failure_keeps_the_original_resolved_at() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&success("D:/a/1.flac"), "").unwrap();
        let first = store.get("D:/a/1.flac").unwrap().unwrap().resolved_at;
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        let after = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(after.resolved_at, first, "曾经成功过这件事不该被抹掉");
        assert_eq!(after.status, ScrapeStatus::Failed);
    }

    #[test]
    fn by_status_picks_out_the_review_queue() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        for (i, status) in [
            ScrapeStatus::Success,
            ScrapeStatus::LowConfidence,
            ScrapeStatus::Failed,
            ScrapeStatus::LowConfidence,
        ]
        .into_iter()
        .enumerate()
        {
            store
                .record(
                    &ResolvedTrack {
                        file_path: format!("D:/a/{i}.flac"),
                        status,
                        confidence: 0.5 + i as f64 * 0.1,
                        matched_at: utc_now(),
                        ..Default::default()
                    },
                    "",
                )
                .unwrap();
        }
        let review = store
            .by_status(&[ScrapeStatus::LowConfidence], None)
            .unwrap();
        assert_eq!(review.len(), 2);
        // 置信度高的在前，用户先看最可能对的
        assert!(review[0].confidence > review[1].confidence);

        let summary = store.summary().unwrap();
        assert_eq!(summary.low_confidence, 2);
        assert_eq!(summary.success, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.total, 4);
    }

    #[test]
    fn a_fresh_row_starts_at_zero_retries() {
        // 首次 record 走的是 INSERT 分支，计数从 0 起；
        // 只有后续的 ON CONFLICT 才累加。和 Python 一致。
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        assert_eq!(store.get("D:/a/1.flac").unwrap().unwrap().retry_count, 0);
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        assert_eq!(store.get("D:/a/1.flac").unwrap().unwrap().retry_count, 1);
    }

    #[test]
    fn reset_keeps_the_retry_history() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        // 走真实流程：先登记，再失败两次
        store.register(&track("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&failure("D:/a/2.flac"), "").unwrap();

        let n = store.reset(Some(&["D:/a/1.flac".to_string()])).unwrap();
        assert_eq!(n, 1);
        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(got.status, ScrapeStatus::Pending);
        assert!(got.error_message.is_empty());
        assert_eq!(got.retry_count, 2, "「之前失败过」是有价值的信息");
        // 没点名的那条不受影响
        assert_eq!(
            store.get("D:/a/2.flac").unwrap().unwrap().status,
            ScrapeStatus::Failed
        );
    }

    #[test]
    fn reset_without_paths_resets_everything() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.record(&success("D:/a/2.flac"), "").unwrap();
        assert_eq!(store.reset(None).unwrap(), 2);
        assert_eq!(store.summary().unwrap().pending, 2);
    }

    #[test]
    fn an_empty_reset_list_touches_nothing() {
        // 空列表和 None 的语义完全不同：一个是「什么都不动」，
        // 一个是「全部重置」。写错这里会把整库状态清掉。
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.record(&success("D:/a/1.flac"), "").unwrap();
        assert_eq!(store.reset(Some(&[])).unwrap(), 0);
        assert_eq!(
            store.get("D:/a/1.flac").unwrap().unwrap().status,
            ScrapeStatus::Success
        );
    }

    #[test]
    fn skipped_tracks_stay_out_of_the_retry_queue() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "").unwrap();
        store.mark_skipped("D:/a/1.flac", "用户手动跳过").unwrap();
        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(got.status, ScrapeStatus::Skipped);
        assert!(got.status.is_terminal());
        assert!(!got.status.is_retryable());
    }

    #[test]
    fn mark_running_touches_the_attempt_time() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "").unwrap();
        assert!(
            store
                .get("D:/a/1.flac")
                .unwrap()
                .unwrap()
                .last_attempt_at
                .is_empty()
        );
        store.mark_running("D:/a/1.flac").unwrap();
        let got = store.get("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(got.status, ScrapeStatus::Running);
        assert!(!got.last_attempt_at.is_empty());
        // 崩溃后残留的 running 要能被重试捞回来
        assert!(got.status.is_retryable());
    }

    #[test]
    fn original_metadata_survives_for_rematching() {
        // 重新匹配要用原值，不能用已经被刮削覆盖过的 songs 行
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "001").unwrap();
        let back = store.original_metadata("D:/a/1.flac").unwrap().unwrap();
        assert_eq!(back.embedded_artist, "ＹＯＡＳＯＢＩ");
        assert_eq!(back.duration_sec, Some(261.0));
        assert_eq!(back.track_number, Some(1));
    }

    #[test]
    fn link_song_connects_both_tables() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "").unwrap();
        store.link_song("D:/a/1.flac", "210").unwrap();
        assert_eq!(store.get("D:/a/1.flac").unwrap().unwrap().song_id, "210");
        let linked: String = conn
            .query_row(
                "SELECT song_id FROM track_original_metadata WHERE file_path='D:/a/1.flac'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, "210");
    }

    #[test]
    fn register_does_not_wipe_an_existing_song_id_with_a_blank_one() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "001").unwrap();
        store.register(&track("D:/a/1.flac"), "").unwrap();
        assert_eq!(store.get("D:/a/1.flac").unwrap().unwrap().song_id, "001");
    }

    #[test]
    fn forget_removes_every_trace() {
        let conn = db();
        let store = ScrapeStore::new(&conn);
        store.register(&track("D:/a/1.flac"), "001").unwrap();
        store.record(&failure("D:/a/1.flac"), "").unwrap();
        store.forget("D:/a/1.flac").unwrap();
        assert!(store.get("D:/a/1.flac").unwrap().is_none());
        assert!(store.attempts("D:/a/1.flac", 10).unwrap().is_empty());
        assert!(store.original_metadata("D:/a/1.flac").unwrap().is_none());
    }
}
