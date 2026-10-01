//! 曲库维护：编辑曲目信息、删除曲目、音频文件丢了之后重新链接。
//!
//! 对应 Python 版的 `song_manager._edit` / `_delete` 和 `gui.py` 的 `RepairAudioDialog`。
//! Python 版只动 `songs` / `utterances` / `tokens` / 全文索引；这里连带把派生表一起维护对：
//!
//! - **删除**：新曲目的 id 是「现有最大 id + 1」（Python 和 `plan.rs` 都这样），删掉最后一首后下一首会**复用这个 id**。
//!   所以按 song_id 挂着的东西（署名、收藏、收听记录、刮削记录、章节）必须一起删，否则会挂到下一首新歌上；
//!   刮削时下载的封面文件名也是 id，同理删掉（由应用层做，这里只报路径）。
//!   **分词校正保留**：它按歌手、歌名、原文匹配，删歌再导入时要靠它恢复（`corrections::restore_for_song`）；
//!   歌词行 id 是自增、不会复用，旧行号挂不到别的歌上。删完没有歌了的专辑、没有署名了的人一并删掉，
//!   否则总览里的专辑数、人数会虚高（那几个统计没有经过 songs 表）。**不删音频文件**。
//! - **编辑**：歌手变了就按新歌手重写演唱署名（标成 manual，自动流程不再动它）；专辑或歌手变了就重新挂专辑；
//!   歌名或歌手变了，把这首歌现有分词校正里记的歌名歌手也改掉，不然删歌再导入时对不上。
//! - **重新链接**：刮削记录按文件路径存，跟着换到新路径，历史不丢。

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::import::{link_album, split_slash};
use crate::plan::{identity_key, path_key};
use crate::scan::ScannedTrack;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongEdit {
    pub title: String,
    pub artist: String,
    pub year: String,
    pub album: String,
    pub genre: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditReport {
    /// 实际变了的字段名（title / artist / year / album / genre）
    pub changed: Vec<String>,
    /// 重写的演唱署名条数（歌手没变时为 0）
    pub performers: usize,
    /// 专辑改挂到哪张（专辑名为空时是 None）
    pub album_id: Option<i64>,
    /// 改了歌名歌手的分词校正条数
    pub corrections_renamed: usize,
    pub albums_removed: usize,
    pub people_removed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    pub title: String,
    pub artist: String,
    pub audio_path: String,
    /// 封面文件路径（应用层判断是不是自己下载的再删）
    pub cover_path: String,
    pub lyric_lines: usize,
    pub tokens: usize,
    pub credits: usize,
    pub plays: usize,
    /// 留下来的分词校正（重导时恢复）
    pub corrections_kept: usize,
    pub albums_removed: usize,
    pub people_removed: usize,
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", params![name], |_| Ok(()))
        .optional()?
        .is_some())
}

fn clean(edit: &SongEdit) -> SongEdit {
    SongEdit {
        title: edit.title.trim().to_owned(),
        artist: edit.artist.trim().to_owned(),
        year: edit.year.trim().to_owned(),
        album: edit.album.trim().to_owned(),
        genre: edit.genre.trim().to_owned(),
    }
}

/// 删掉这些人里已经没有任何署名的
fn remove_orphan_people(conn: &Connection, ids: &BTreeSet<i64>) -> Result<usize> {
    let mut removed = 0;
    for id in ids {
        removed += conn.execute(
            "DELETE FROM people WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM track_credits WHERE person_id=?1)",
            params![id],
        )?;
    }
    Ok(removed)
}

/// 这张专辑没有歌了就删掉
fn remove_orphan_album(conn: &Connection, album_id: Option<i64>) -> Result<usize> {
    let Some(id) = album_id else { return Ok(0) };
    Ok(conn.execute(
        "DELETE FROM albums WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM songs WHERE album_id=?1)",
        params![id],
    )?)
}

/// 改曲目信息。歌名、歌手不能为空。**在一个事务里调**（`tx` 可以是 `Transaction`）。
pub fn edit_song(conn: &Connection, song_id: &str, edit: &SongEdit) -> Result<EditReport> {
    let edit = clean(edit);
    if edit.title.is_empty() || edit.artist.is_empty() {
        bail!("歌名和歌手不能为空");
    }
    let (old, album_id): (SongEdit, Option<i64>) = conn
        .query_row(
            "SELECT title, artist, COALESCE(year,''), COALESCE(album,''), COALESCE(genre,''), album_id FROM songs WHERE id=?1",
            params![song_id],
            |r| {
                Ok((
                    SongEdit { title: r.get(0)?, artist: r.get(1)?, year: r.get(2)?, album: r.get(3)?, genre: r.get(4)? },
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .with_context(|| format!("找不到曲目 {song_id}"))?;

    let mut report = EditReport { album_id, ..Default::default() };
    for (name, before, after) in [
        ("title", &old.title, &edit.title),
        ("artist", &old.artist, &edit.artist),
        ("year", &old.year, &edit.year),
        ("album", &old.album, &edit.album),
        ("genre", &old.genre, &edit.genre),
    ] {
        if before != after {
            report.changed.push(name.to_owned());
        }
    }
    if report.changed.is_empty() {
        return Ok(report);
    }
    let changed = |name: &str| report.changed.iter().any(|c| c == name);

    conn.execute(
        "UPDATE songs SET title=?1, artist=?2, year=?3, album=?4, genre=?5 WHERE id=?6",
        params![edit.title, edit.artist, edit.year, edit.album, edit.genre, song_id],
    )?;

    let mut touched_people = BTreeSet::new();
    if changed("artist") {
        let mut stmt = conn.prepare("SELECT person_id FROM track_credits WHERE song_id=?1 AND role='performer'")?;
        touched_people.extend(stmt.query_map(params![song_id], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?);
        conn.execute("DELETE FROM track_credits WHERE song_id=?1 AND role='performer'", params![song_id])?;
        for (position, name) in split_slash(&edit.artist).into_iter().enumerate() {
            if let Some(person_id) = crate::import::get_or_create_person(conn, &name)? {
                report.performers += crate::import::add_credit(conn, song_id, person_id, "performer", position, "manual")?;
            }
        }
    }

    if changed("album") || changed("artist") {
        if edit.album.is_empty() {
            conn.execute("UPDATE songs SET album_id=NULL WHERE id=?1", params![song_id])?;
        } else {
            link_album(conn, song_id, &edit.album, &edit.artist, &edit.year)?;
        }
        report.album_id = conn.query_row("SELECT album_id FROM songs WHERE id=?1", params![song_id], |r| r.get(0))?;
        if report.album_id != album_id {
            report.albums_removed = remove_orphan_album(conn, album_id)?;
        }
    }

    if (changed("title") || changed("artist")) && table_exists(conn, "token_corrections")? {
        report.corrections_renamed = conn.execute(
            "UPDATE token_corrections SET song_artist=?1, song_title=?2 \
             WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id=?3)",
            params![edit.artist, edit.title, song_id],
        )?;
    }
    report.people_removed = remove_orphan_people(conn, &touched_people)?;
    Ok(report)
}

/// 删一首歌和挂在它 id 上的一切（见模块说明），分词校正除外。**在一个事务里调**。
pub fn delete_song(conn: &Connection, song_id: &str) -> Result<DeleteReport> {
    let (title, artist, audio_path, cover_path, album_id): (String, String, String, String, Option<i64>) = conn
        .query_row(
            "SELECT title, artist, COALESCE(audio_path,''), COALESCE(cover_path,''), album_id FROM songs WHERE id=?1",
            params![song_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .with_context(|| format!("找不到曲目 {song_id}"))?;
    let mut report = DeleteReport { title, artist, audio_path: audio_path.clone(), cover_path, ..Default::default() };

    let lines: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, text FROM utterances WHERE song_id=?1")?;
        stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    report.lyric_lines = lines.len();
    for (id, text) in &lines {
        // 外部内容的全文索引：先按原文把词条删掉，再删歌词行
        conn.execute("INSERT INTO utterances_fts(utterances_fts, rowid, text) VALUES('delete', ?1, ?2)", params![id, text])?;
        report.tokens += conn.execute("DELETE FROM tokens WHERE utterance_id=?1", params![id])?;
    }
    if table_exists(conn, "token_corrections")? {
        report.corrections_kept = conn.query_row(
            "SELECT COUNT(*) FROM token_corrections WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id=?1)",
            params![song_id],
            |r| r.get::<_, i64>(0),
        )? as usize;
    }
    conn.execute("DELETE FROM utterances WHERE song_id=?1", params![song_id])?;
    if table_exists(conn, "chapters")? {
        conn.execute("DELETE FROM chapters WHERE source_id=?1", params![song_id])?;
    }

    let people: BTreeSet<i64> = {
        let mut stmt = conn.prepare("SELECT person_id FROM track_credits WHERE song_id=?1")?;
        stmt.query_map(params![song_id], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    report.credits = conn.execute("DELETE FROM track_credits WHERE song_id=?1", params![song_id])?;

    if table_exists(conn, "favorites")? {
        conn.execute("DELETE FROM favorites WHERE entity_type='song' AND entity_id=?1", params![song_id])?;
    }
    if table_exists(conn, "play_history")? {
        report.plays = conn.execute("DELETE FROM play_history WHERE song_id=?1", params![song_id])?;
    }
    for table in ["scrape_state", "track_original_metadata"] {
        if table_exists(conn, table)? {
            conn.execute(&format!("DELETE FROM {table} WHERE song_id=?1 OR file_path=?2"), params![song_id, audio_path])?;
        }
    }
    if table_exists(conn, "scrape_attempts")? {
        conn.execute("DELETE FROM scrape_attempts WHERE file_path=?1", params![audio_path])?;
    }

    conn.execute("DELETE FROM songs WHERE id=?1", params![song_id])?;
    report.albums_removed = remove_orphan_album(conn, album_id)?;
    report.people_removed = remove_orphan_people(conn, &people)?;
    Ok(report)
}

/// 清掉这首歌现有的歌词：歌词行、全文索引、分词。**分词校正保留**。
///
/// 给「换一份歌词」用：`clear_lyrics` 之后 `import::attach_lyrics` 重新挂一份。
/// 分开两个函数而不是让 attach 自己先删，是因为导入路径上这首歌必然是新的，
/// 多一次删除只会掩盖「重复挂歌词」这种 bug。
///
/// 校正按 `(歌手, 曲名, 原文)` 匹配，`attach_lyrics` 末尾会把它们套回新的行号上
/// （见 `jp_corpus::corrections::restore_for_song`），所以用户改过的分词不会因为
/// 换歌词而丢。**在一个事务里调**。
pub fn clear_lyrics(conn: &Connection, song_id: &str) -> Result<ClearedLyrics> {
    let lines: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, text FROM utterances WHERE song_id=?1")?;
        stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let mut report = ClearedLyrics {
        lyric_lines: lines.len(),
        tokens: 0,
    };
    for (id, text) in &lines {
        // 外部内容的全文索引：先按原文把词条删掉，再删歌词行。
        // 顺序反了的话 FTS 里会留下搜得到、点不开的幽灵行。
        conn.execute(
            "INSERT INTO utterances_fts(utterances_fts, rowid, text) VALUES('delete', ?1, ?2)",
            params![id, text],
        )?;
        report.tokens += conn.execute("DELETE FROM tokens WHERE utterance_id=?1", params![id])?;
    }
    conn.execute("DELETE FROM utterances WHERE song_id=?1", params![song_id])?;
    Ok(report)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedLyrics {
    pub lyric_lines: usize,
    pub tokens: usize,
}

/// 把这首歌的音频换成 `new_path`。新路径已经被别的歌用着时拒绝。**在一个事务里调**。返回旧路径
pub fn relink_audio(conn: &Connection, song_id: &str, new_path: &str) -> Result<String> {
    let new_path = new_path.trim();
    if new_path.is_empty() {
        bail!("新路径是空的");
    }
    let old: String = conn
        .query_row("SELECT COALESCE(audio_path,'') FROM songs WHERE id=?1", params![song_id], |r| r.get(0))
        .optional()?
        .with_context(|| format!("找不到曲目 {song_id}"))?;
    let wanted = path_key(new_path);
    let mut stmt = conn.prepare("SELECT id, title, COALESCE(audio_path,'') FROM songs WHERE id<>?1")?;
    let taken = stmt
        .query_map(params![song_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|(_, _, path)| path_key(path) == wanted);
    if let Some((id, title, _)) = taken {
        bail!("这个文件已经是 {id}《{title}》的音频了");
    }

    conn.execute("UPDATE songs SET audio_path=?1 WHERE id=?2", params![new_path, song_id])?;
    if !old.is_empty() && path_key(&old) != wanted {
        for table in ["scrape_state", "track_original_metadata"] {
            if table_exists(conn, table)? {
                // 新路径上已经有记录（比如以前扫到过这个文件）就保留那份，不覆盖
                conn.execute(
                    &format!(
                        "UPDATE {table} SET file_path=?1 WHERE file_path=?2 \
                         AND NOT EXISTS (SELECT 1 FROM {table} WHERE file_path=?1)"
                    ),
                    params![new_path, old],
                )?;
            }
        }
        if table_exists(conn, "scrape_attempts")? {
            conn.execute("UPDATE scrape_attempts SET file_path=?1 WHERE file_path=?2", params![new_path, old])?;
        }
    }
    Ok(old)
}

/// 音频文件找不到的曲目
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingAudio {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub audio_path: String,
}

pub fn missing_audio(conn: &Connection) -> Result<Vec<MissingAudio>> {
    let mut stmt = conn.prepare("SELECT id, title, artist, COALESCE(audio_path,'') FROM songs ORDER BY id")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MissingAudio { song_id: r.get(0)?, title: r.get(1)?, artist: r.get(2)?, audio_path: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().filter(|m| m.audio_path.trim().is_empty() || !Path::new(&m.audio_path).is_file()).collect())
}

/// 给一首丢了音频的歌建议的新文件
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkSuggestion {
    pub song_id: String,
    pub path: String,
    pub reason: String,
}

/// 在扫描到的文件里给丢了音频的歌找新文件。**只给唯一、有把握的匹配**：
///
/// 1. 归一化后的歌手 + 歌名对得上（和导入判重同一套键）；
/// 2. 文件名就是这首歌的编号，而且所在目录名是它的歌手（本项目自己的 `raw/audio/歌手/编号.flac` 布局）。
///
/// 一首歌对上多个文件、或一个文件对上多首歌，都不建议——Python 版是「文件名里含歌名就算」，
/// 「夜」「春」这种短歌名会配错。已经被库里别的歌用着的文件也不建议。
pub fn suggest_relinks(missing: &[MissingAudio], scanned: &[ScannedTrack], in_use: &[String]) -> Vec<RelinkSuggestion> {
    let used: BTreeSet<String> = in_use.iter().map(|p| path_key(p)).collect();
    let files: Vec<&ScannedTrack> = scanned.iter().filter(|t| !used.contains(&path_key(&t.path))).collect();

    let mut proposals: Vec<(usize, usize, &'static str)> = Vec::new();
    for (mi, song) in missing.iter().enumerate() {
        let wanted = identity_key(&song.artist, &song.title);
        let artist_key = crate::filename::matching_key(&song.artist);
        let mut hits: Vec<(usize, &'static str)> = Vec::new();
        for (fi, file) in files.iter().enumerate() {
            if wanted.is_some() && identity_key(file.artist(), file.title()) == wanted {
                hits.push((fi, "标签（或文件名）里的歌手和歌名对得上"));
                continue;
            }
            let path = Path::new(&file.path);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            let folder = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or_default();
            if stem == song.song_id && !artist_key.is_empty() && crate::filename::matching_key(folder) == artist_key {
                hits.push((fi, "文件名是这首歌的编号，放在歌手同名的目录里"));
            }
        }
        if let [(fi, reason)] = hits[..] {
            proposals.push((mi, fi, reason));
        }
    }
    // 一个文件被两首歌认领：都不建议
    let mut claims: HashMap<usize, usize> = HashMap::new();
    for (_, fi, _) in &proposals {
        *claims.entry(*fi).or_default() += 1;
    }
    proposals
        .into_iter()
        .filter(|(_, fi, _)| claims[fi] == 1)
        .map(|(mi, fi, reason)| RelinkSuggestion {
            song_id: missing[mi].song_id.clone(),
            path: files[fi].path.clone(),
            reason: reason.to_owned(),
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::import::NOW;

    /// 换一份歌词：清掉旧的再挂新的，全文索引必须跟着走。
    ///
    /// 外部内容表 `utterances_fts` 没有触发器，删行时漏掉 `'delete'` 那一步的话
    /// 旧歌词仍然搜得到，点进去是一条已经不存在的行——这个测试就是守那一步。
    #[test]
    fn replacing_the_lyrics_leaves_no_ghosts_in_the_full_text_index() {
        let conn = full_db();
        conn.execute(
            "INSERT INTO songs (id, title, artist, corpus_type) VALUES ('001','夜行','ヨルシカ','song')",
            [],
        )
        .unwrap();

        let dir = std::env::temp_dir().join(format!("jp-import-lyr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.lrc");
        let new = dir.join("new.lrc");
        std::fs::write(&old, "[00:10.00]夜が明けるまで\n[00:15.00]君を待っている\n").unwrap();
        std::fs::write(&new, "[00:12.00]月が綺麗ですね\n").unwrap();

        let first =
            crate::import::attach_lyrics(&conn, "001", &old, None, "ヨルシカ", "夜行").unwrap();
        assert_eq!(first.lyric_lines, 2);
        assert_eq!(fts_hits(&conn, "夜が明"), 1);

        let cleared = clear_lyrics(&conn, "001").unwrap();
        assert_eq!(cleared.lyric_lines, 2);
        assert_eq!(fts_hits(&conn, "夜が明"), 0, "清掉之后还搜得到旧歌词");

        let second =
            crate::import::attach_lyrics(&conn, "001", &new, None, "ヨルシカ", "夜行").unwrap();
        assert_eq!(second.lyric_lines, 1);
        assert_eq!(fts_hits(&conn, "月が綺"), 1, "新歌词搜不到");
        assert_eq!(fts_hits(&conn, "夜が明"), 0);

        // 行号从 0 重新排，不是接着旧的往后数
        let idx: Vec<i64> = conn
            .prepare("SELECT line_idx FROM utterances WHERE song_id='001' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(idx, vec![0]);
    }

    fn fts_hits(conn: &Connection, needle: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH ?1",
            params![needle],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// 和真库同构的库：导入测试用的那几张表，加上删除要维护的其他表（建表语句抄自真库 sqlite_master）
    pub(crate) fn full_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL,
                year TEXT, album TEXT, genre TEXT, audio_path TEXT,
                corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT,
                cover_path TEXT, duration_sec REAL, album_id INTEGER);
             CREATE TABLE utterances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
                line_idx INTEGER NOT NULL, time_sec REAL, text TEXT NOT NULL, chapter_id INTEGER);
             CREATE VIRTUAL TABLE utterances_fts USING fts5(text, content=utterances, content_rowid=id, tokenize='trigram');
             CREATE TABLE tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT, utterance_id INTEGER NOT NULL REFERENCES utterances(id),
                token_idx INTEGER NOT NULL, surface TEXT NOT NULL, lemma TEXT NOT NULL, pos TEXT, dep TEXT, head TEXT);
             CREATE TABLE people (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                normalized_name TEXT NOT NULL UNIQUE, sort_name TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT '');
             CREATE TABLE track_credits (
                song_id TEXT NOT NULL, person_id INTEGER NOT NULL, role TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0, source TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (song_id, person_id, role));
             CREATE TABLE albums (
                id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, normalized_title TEXT NOT NULL,
                album_artist TEXT NOT NULL DEFAULT '', normalized_album_artist TEXT NOT NULL DEFAULT '',
                year TEXT NOT NULL DEFAULT '', artwork_path TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT '',
                UNIQUE (normalized_title, normalized_album_artist));
             CREATE TABLE token_corrections (
                utterance_id INTEGER PRIMARY KEY, tokens_json TEXT NOT NULL, orig_json TEXT NOT NULL DEFAULT '[]',
                text TEXT NOT NULL DEFAULT '', song_artist TEXT NOT NULL DEFAULT '', song_title TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE chapters (
                id INTEGER PRIMARY KEY AUTOINCREMENT, source_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
                chapter_idx INTEGER NOT NULL, title TEXT DEFAULT '', utt_start INTEGER, utt_end INTEGER);
             CREATE TABLE favorites (
                entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (entity_type, entity_id));
             CREATE TABLE play_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT, song_id TEXT NOT NULL, played_at TEXT NOT NULL,
                listened_sec REAL NOT NULL DEFAULT 0, position_sec REAL NOT NULL DEFAULT 0,
                completed INTEGER NOT NULL DEFAULT 0, source TEXT NOT NULL DEFAULT '');
             CREATE TABLE scrape_state (
                file_path TEXT PRIMARY KEY, song_id TEXT, status TEXT NOT NULL, confidence REAL NOT NULL DEFAULT 0,
                provider TEXT NOT NULL DEFAULT '', provider_id TEXT NOT NULL DEFAULT '', error_type TEXT NOT NULL DEFAULT '',
                error_message TEXT NOT NULL DEFAULT '', retry_count INTEGER NOT NULL DEFAULT 0, first_seen_at TEXT,
                last_attempt_at TEXT, resolved_at TEXT, breakdown_json TEXT, candidates_json TEXT, queries_json TEXT);
             CREATE TABLE scrape_attempts (
                id INTEGER PRIMARY KEY AUTOINCREMENT, file_path TEXT NOT NULL, provider TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL, confidence REAL NOT NULL DEFAULT 0, error_type TEXT NOT NULL DEFAULT '',
                error_message TEXT NOT NULL DEFAULT '', attempted_at TEXT NOT NULL);
             CREATE TABLE track_original_metadata (
                file_path TEXT PRIMARY KEY, song_id TEXT, title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '',
                album TEXT NOT NULL DEFAULT '', year TEXT NOT NULL DEFAULT '', genre TEXT NOT NULL DEFAULT '',
                track_number INTEGER, disc_number INTEGER, duration_sec REAL, size INTEGER, mtime REAL,
                normalized_title TEXT NOT NULL DEFAULT '', normalized_artist TEXT NOT NULL DEFAULT '',
                normalized_album TEXT NOT NULL DEFAULT '', source_json TEXT, captured_at TEXT);",
        )
        .unwrap();
        conn
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// 两首歌：001 夜（歌手 A/B，专辑 X，两行歌词、署名、收藏、收听、刮削记录、分词校正），002 朝（歌手 A，专辑 X）
    fn library() -> Connection {
        let conn = full_db();
        conn.execute_batch(&format!(
            "INSERT INTO albums (id, title, normalized_title, album_artist, normalized_album_artist) VALUES (1, 'X', 'x', 'A', 'a');
             INSERT INTO albums (id, title, normalized_title, album_artist, normalized_album_artist) VALUES (2, 'Solo', 'solo', 'B', 'b');
             INSERT INTO songs (id, title, artist, album, audio_path, cover_path, album_id) VALUES
                ('001', '夜', 'A/B', 'X', 'D:/music/A/001.flac', 'D:/raw/covers/A/001.jpg', 1),
                ('002', '朝', 'A', 'X', 'D:/music/A/002.flac', '', 1);
             INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES
                (1, '001', 0, 1.0, '夜が明ける'), (2, '001', 1, 2.0, '朝が来る'), (3, '002', 0, 1.0, '朝が来る');
             INSERT INTO utterances_fts (rowid, text) SELECT id, text FROM utterances;
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES
                (1, 0, '夜', '夜', 'NOUN'), (1, 1, 'が', 'が', 'ADP'), (2, 0, '朝', '朝', 'NOUN'), (3, 0, '朝', '朝', 'NOUN');
             INSERT INTO people (id, name, normalized_name, created_at) VALUES
                (1, 'A', 'a', {NOW}), (2, 'B', 'b', {NOW}), (3, 'C', 'c', {NOW});
             INSERT INTO track_credits (song_id, person_id, role, position, source) VALUES
                ('001', 1, 'performer', 0, 'library'), ('001', 2, 'performer', 1, 'library'), ('001', 3, 'lyricist', 0, 'lrc'),
                ('002', 1, 'performer', 0, 'library');
             INSERT INTO favorites (entity_type, entity_id) VALUES ('song', '001'), ('song', '002');
             INSERT INTO play_history (song_id, played_at) VALUES ('001', 'now'), ('002', 'now');
             INSERT INTO scrape_state (file_path, song_id, status) VALUES ('D:/music/A/001.flac', '001', 'success');
             INSERT INTO scrape_attempts (file_path, status, attempted_at) VALUES ('D:/music/A/001.flac', 'success', 'now');
             INSERT INTO track_original_metadata (file_path, song_id, title) VALUES ('D:/music/A/001.flac', '001', '夜');
             INSERT INTO token_corrections (utterance_id, tokens_json, text, song_artist, song_title) VALUES
                (1, '[]', '夜が明ける', 'A/B', '夜');"
        ))
        .unwrap();
        conn
    }

    #[test]
    fn deleting_a_song_removes_everything_keyed_on_its_id_but_keeps_corrections() {
        let conn = library();
        let report = delete_song(&conn, "001").unwrap();
        assert_eq!((report.lyric_lines, report.tokens, report.credits, report.plays, report.corrections_kept), (2, 3, 3, 1, 1));
        assert_eq!(report.cover_path, "D:/raw/covers/A/001.jpg");

        assert_eq!(count(&conn, "SELECT COUNT(*) FROM songs"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM utterances WHERE song_id='001'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM tokens WHERE utterance_id IN (1,2)"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM track_credits WHERE song_id='001'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM favorites"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM play_history"), 1);
        for table in ["scrape_state", "scrape_attempts", "track_original_metadata"] {
            assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM {table}")), 0, "{table}");
        }
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM token_corrections"), 1, "分词校正要留着，重导时恢复");

        // B 和 C 只在 001 上有署名，删掉；A 还演唱 002，留着。专辑 X 还有 002，留着
        assert_eq!((report.people_removed, report.albums_removed), (2, 0));
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM people"), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM albums WHERE id=1"), 1);

        // 全文索引：删掉的行搜不到，留下的行照样搜得到，索引完整
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '明ける'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '朝が来'"), 1);
        conn.execute("INSERT INTO utterances_fts(utterances_fts) VALUES('integrity-check')", []).unwrap();

        // 最后一首也删了：专辑空了就删
        let report = delete_song(&conn, "002").unwrap();
        assert_eq!((report.albums_removed, report.people_removed), (1, 1));
        assert!(delete_song(&conn, "002").is_err());
    }

    #[test]
    fn editing_the_artist_rewrites_performers_and_moves_the_album() {
        let conn = library();
        let edit = SongEdit { title: " 夜明け ".into(), artist: "B".into(), year: "2020".into(), album: "Solo".into(), genre: String::new() };
        let report = edit_song(&conn, "001", &edit).unwrap();
        assert_eq!(report.changed, ["title", "artist", "year", "album"]);
        assert_eq!(report.performers, 1);

        let performers: Vec<(String, String)> = conn
            .prepare("SELECT p.name, c.source FROM track_credits c JOIN people p ON p.id=c.person_id WHERE c.song_id='001' AND c.role='performer'")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(performers, [("B".to_owned(), "manual".to_owned())]);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM track_credits WHERE song_id='001' AND role='lyricist'"), 1, "作词署名不动");

        // 「Solo」挂在 B 名下的专辑已经有了，直接挂上去；X 还有 002，留着
        assert_eq!(report.album_id, Some(2));
        assert_eq!(report.albums_removed, 0);
        assert_eq!(report.people_removed, 0, "A 还演唱 002");
        let (title, year): (String, String) = conn.query_row("SELECT title, year FROM songs WHERE id='001'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((title.as_str(), year.as_str()), ("夜明け", "2020"), "去掉首尾空白");

        assert_eq!(report.corrections_renamed, 1);
        let (artist, title): (String, String) =
            conn.query_row("SELECT song_artist, song_title FROM token_corrections", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((artist.as_str(), title.as_str()), ("B", "夜明け"));
    }

    #[test]
    fn clearing_the_album_unlinks_it_and_an_unchanged_edit_touches_nothing() {
        let conn = library();
        let same = SongEdit { title: "朝".into(), artist: "A".into(), year: String::new(), album: "X".into(), genre: String::new() };
        assert_eq!(edit_song(&conn, "002", &same).unwrap().changed, Vec::<String>::new());

        let cleared = SongEdit { album: String::new(), ..same };
        let report = edit_song(&conn, "002", &cleared).unwrap();
        assert_eq!(report.album_id, None);
        assert_eq!(report.albums_removed, 0, "X 还有 001");
        assert!(edit_song(&conn, "002", &SongEdit { title: " ".into(), ..cleared }).is_err(), "歌名不能为空");
    }

    #[test]
    fn relinking_moves_scrape_records_and_refuses_a_file_used_by_another_song() {
        let conn = library();
        assert_eq!(relink_audio(&conn, "001", "E:/new/夜.flac").unwrap(), "D:/music/A/001.flac");
        for table in ["scrape_state", "scrape_attempts", "track_original_metadata"] {
            assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM {table} WHERE file_path='E:/new/夜.flac'")), 1, "{table}");
        }
        let err = relink_audio(&conn, "001", "D:\\music\\A\\002.flac").unwrap_err().to_string();
        if cfg!(windows) {
            assert!(err.contains("002"), "{err}");
        }
    }

    fn scanned(path: &str, title: &str, artist: &str) -> ScannedTrack {
        ScannedTrack { path: path.into(), tag_title: title.into(), tag_artist: artist.into(), ..Default::default() }
    }

    fn missing(id: &str, title: &str, artist: &str) -> MissingAudio {
        MissingAudio { song_id: id.into(), title: title.into(), artist: artist.into(), audio_path: format!("D:/gone/{id}.flac") }
    }

    #[test]
    fn relink_suggestions_are_only_given_when_unambiguous() {
        let songs = [missing("001", "夜", "ヨルシカ"), missing("002", "春", "ヨルシカ"), missing("003", "朝", "YOASOBI"), missing("004", "雨", "Aimer")];
        let files = [
            scanned("F:/a/ヨルシカ - 夜.flac", "夜", "ヨルシカ"),
            // 「春」两个文件都对得上：不建议
            scanned("F:/a/春.flac", "春", "ヨルシカ"),
            scanned("F:/b/春 (live).flac", "春", "ヨルシカ"),
            // 标签不对，但文件名是编号、目录是歌手
            scanned("F:/raw/audio/YOASOBI/003.flac", "", ""),
            // 文件名里含「雨」，但歌手不对：Python 版会误配，这里不配
            scanned("F:/a/雨上がり.flac", "雨上がり", "someone"),
            // 已经被库里别的歌用着
            scanned("F:/used/ヨルシカ - 夜.flac", "夜", "ヨルシカ"),
        ];
        let used = ["F:/used/ヨルシカ - 夜.flac".to_owned()];
        let suggestions = suggest_relinks(&songs, &files, &used);
        let got: Vec<(&str, &str)> = suggestions.iter().map(|s| (s.song_id.as_str(), s.path.as_str())).collect();
        assert_eq!(got, [("001", "F:/a/ヨルシカ - 夜.flac"), ("003", "F:/raw/audio/YOASOBI/003.flac")]);
    }
}
