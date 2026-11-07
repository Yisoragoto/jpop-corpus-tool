//! 音乐文件夹（来源）：哪些文件夹算曲库。
//!
//! 导入是「就地引用」，歌留在用户自己的文件夹里。以前「语料库文件夹」是从库里的歌所在的位置
//! 推出来的（`jp_import::watch`），用户管不了；现在它是一张表，用户亲手加、停用、移除。
//!
//! - **歌属于哪个文件夹按路径算**（[`assign`]）：`recursive` 的来源管它下面所有层，
//!   不递归的只管直接放在它里面的文件；几个来源都盖得住时归最贴近的那个（路径最长）。
//!   不在任何来源下的歌（手动导入的单曲）`source_id` 是 NULL。
//! - 这里只管表本身和归属；扫描、对账在 `jp_import::sync`，盘上的事在 `jp_library::sources`。
//! - 来源之间可以嵌套：外面那个递归的已经盖住里面那个时，加不进来（[`add`] 报出来）；
//!   新加的递归来源盖住了已有的，已有的那几个并进来（它们的歌改归新的）。

use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::audio_files::path_key;
use crate::lineage::NOW;

/// 一个音乐文件夹。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Source {
    pub id: i64,
    pub root_path: String,
    pub label: String,
    pub recursive: bool,
    pub enabled: bool,
    pub created_at: String,
    pub last_scanned_at: Option<String>,
    pub last_scan_error: Option<String>,
}

/// 界面上的一行：来源本身，加上它下面有几首歌。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SourceView {
    #[serde(flatten)]
    pub source: Source,
    /// 在库的歌
    pub songs: usize,
    /// 已移出（`deleted_at` 不空）的歌
    pub removed: usize,
    /// 文件夹现在在不在（盘没插、挪走了是 false）
    pub present: bool,
}

/// 比较用的文件夹键：和 [`path_key`] 同一套规矩，去掉末尾的 `/`（盘符根除外）。
pub fn folder_key(path: &str) -> String {
    let key = path_key(path);
    let trimmed = key.trim_end_matches('/');
    // `D:/` 去掉末尾会变成 `D:`，那是「D 盘的当前目录」，不是根
    if trimmed.is_empty() || trimmed.ends_with(':') { key } else { trimmed.to_string() }
}

/// `root` 这个来源管不管 `audio_path` 这个文件。
pub fn covers(root: &str, recursive: bool, audio_path: &str) -> bool {
    let root = folder_key(root);
    let file = path_key(audio_path);
    let prefix = if root.ends_with('/') { root.clone() } else { format!("{root}/") };
    let Some(rest) = file.strip_prefix(&prefix) else { return false };
    !rest.is_empty() && (recursive || !rest.contains('/'))
}

/// 管这个文件的来源里最贴近的那个（路径最长的）。没有就是 None。
pub fn owner<'a>(sources: &'a [Source], audio_path: &str) -> Option<&'a Source> {
    sources
        .iter()
        .filter(|s| covers(&s.root_path, s.recursive, audio_path))
        .max_by_key(|s| folder_key(&s.root_path).len())
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: r.get(0)?,
        root_path: r.get(1)?,
        label: r.get(2)?,
        recursive: r.get::<_, i64>(3)? != 0,
        enabled: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
        last_scanned_at: r.get(6)?,
        last_scan_error: r.get(7)?,
    })
}

const COLUMNS: &str = "id, root_path, label, recursive, enabled, created_at, last_scanned_at, last_scan_error";

/// 所有来源，按路径排。
pub fn all(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM library_sources ORDER BY root_path"))?;
    let rows = stmt.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Source>> {
    Ok(conn
        .query_row(&format!("SELECT {COLUMNS} FROM library_sources WHERE id = ?1"), [id], row)
        .optional()?)
}

/// 界面上的列表：每个来源下在库几首、移出几首、文件夹在不在。
pub fn list(conn: &Connection) -> Result<Vec<SourceView>> {
    let mut counts = conn.prepare(
        "SELECT COALESCE(SUM(deleted_at IS NULL), 0), COALESCE(SUM(deleted_at IS NOT NULL), 0)
         FROM songs WHERE source_id = ?1",
    )?;
    all(conn)?
        .into_iter()
        .map(|source| {
            let (songs, removed): (i64, i64) = counts.query_row([source.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let present = Path::new(&source.root_path).is_dir();
            Ok(SourceView { source, songs: songs as usize, removed: removed as usize, present })
        })
        .collect()
}

/// 加一个来源。
///
/// - 文件夹必须现在就在；
/// - 已经有同一个文件夹，或者被一个递归的来源整个盖住了：报错，不加；
/// - 新的是递归的、盖住了已有的来源：那几个并进来（删掉它们的行），歌改归新的。
///
/// 返回新来源的 id。之后要调 [`assign`] 给歌填归属——放在一个事务里由调用方做。
pub fn add(conn: &Connection, root_path: &str, label: &str, recursive: bool) -> Result<i64> {
    let root = root_path.trim();
    if root.is_empty() {
        bail!("文件夹路径是空的");
    }
    if !Path::new(root).is_dir() {
        bail!("找不到这个文件夹：{root}");
    }
    let key = folder_key(root);
    let existing = all(conn)?;
    if let Some(same) = existing.iter().find(|s| folder_key(&s.root_path) == key) {
        bail!("这个文件夹已经在曲库里了：{}", same.root_path);
    }
    // 被盖住：拿文件夹里的一个虚构文件去问「谁管它」
    let probe = format!("{key}/_");
    if let Some(outer) = existing.iter().find(|s| s.recursive && covers(&s.root_path, true, &probe)) {
        bail!("{root} 已经包含在 {} 里了", outer.root_path);
    }
    let absorbed: Vec<i64> = if recursive {
        existing
            .iter()
            .filter(|s| covers(root, true, &format!("{}/_", folder_key(&s.root_path))))
            .map(|s| s.id)
            .collect()
    } else {
        Vec::new()
    };
    for id in &absorbed {
        conn.execute("UPDATE songs SET source_id = NULL WHERE source_id = ?1", [id])?;
        conn.execute("DELETE FROM library_sources WHERE id = ?1", [id])?;
    }
    conn.execute(
        &format!("INSERT INTO library_sources (root_path, label, recursive, enabled, created_at) VALUES (?1, ?2, ?3, 1, {NOW})"),
        params![root, label.trim(), recursive as i64],
    )
    .with_context(|| format!("加不了文件夹 {root}"))?;
    Ok(conn.last_insert_rowid())
}

/// 停用 / 启用。停用的不扫、启动时不看，歌照旧在库里。
pub fn set_enabled(conn: &Connection, id: i64, enabled: bool) -> Result<()> {
    let n = conn.execute("UPDATE library_sources SET enabled = ?2 WHERE id = ?1", params![id, enabled as i64])?;
    if n == 0 {
        bail!("没有这个文件夹：{id}");
    }
    Ok(())
}

/// 删掉来源这一行，它的歌 `source_id` 置空。**不动歌**——移出、彻底删除由调用方先做。
pub fn forget(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE songs SET source_id = NULL WHERE source_id = ?1", [id])?;
    let n = conn.execute("DELETE FROM library_sources WHERE id = ?1", [id])?;
    if n == 0 {
        bail!("没有这个文件夹：{id}");
    }
    Ok(())
}

/// 记下一次扫描：成功时清掉上次的错误，失败时记原因（扫描时间照样更新）。
pub fn record_scan(conn: &Connection, id: i64, error: Option<&str>) -> Result<()> {
    conn.execute(
        &format!("UPDATE library_sources SET last_scanned_at = {NOW}, last_scan_error = ?2 WHERE id = ?1"),
        params![id, error],
    )?;
    Ok(())
}

/// 按路径重算每首歌（含已移出的）属于哪个来源。返回改了几首。
pub fn assign(conn: &Connection) -> Result<usize> {
    let sources = all(conn)?;
    let songs: Vec<(String, Option<String>, Option<i64>)> = conn
        .prepare("SELECT id, audio_path, source_id FROM songs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut update = conn.prepare("UPDATE songs SET source_id = ?2 WHERE id = ?1")?;
    let mut changed = 0;
    for (id, audio, current) in songs {
        let wanted = audio.as_deref().filter(|a| !a.is_empty()).and_then(|a| owner(&sources, a)).map(|s| s.id);
        if wanted != current {
            update.execute(params![id, wanted])?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 属于某个来源的一首（含已移出的）。对账用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub audio_path: String,
    pub title: String,
    pub artist: String,
    pub removed: bool,
}

/// 这个来源下的歌（含已移出的）。
pub fn members(conn: &Connection, id: i64) -> Result<Vec<Member>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(audio_path,''), title, artist, deleted_at IS NOT NULL FROM songs WHERE source_id = ?1 ORDER BY id",
    )?;
    let rows = stmt
        .query_map([id], |r| {
            Ok(Member { id: r.get(0)?, audio_path: r.get(1)?, title: r.get(2)?, artist: r.get(3)?, removed: r.get(4)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 把这些歌移出语料库（打 `deleted_at`）。已经移出的不动，原来的移出时间留着。返回移出了几首。
pub fn mark_removed(conn: &Connection, ids: &[String]) -> Result<usize> {
    let mut stmt = conn.prepare(&format!("UPDATE songs SET deleted_at = {NOW} WHERE id = ?1 AND deleted_at IS NULL"))?;
    let mut n = 0;
    for id in ids {
        n += stmt.execute([id])?;
    }
    Ok(n)
}

/// 恢复移出的歌。返回恢复了几首。
pub fn restore(conn: &Connection, ids: &[String]) -> Result<usize> {
    let mut stmt = conn.prepare("UPDATE songs SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL")?;
    let mut n = 0;
    for id in ids {
        n += stmt.execute([id])?;
    }
    Ok(n)
}

/// 已移出的一首。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RemovedSong {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub audio_path: String,
    pub removed_at: String,
    /// 它原来属于哪个文件夹（那个文件夹也移除了的话是空）
    pub source: String,
    /// 音频文件现在还在不在
    pub audio_present: bool,
}

/// 已移出的歌，最近移出的在前。
pub fn removed(conn: &Connection) -> Result<Vec<RemovedSong>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, COALESCE(s.audio_path,''), s.deleted_at, COALESCE(src.root_path,'')
         FROM songs s LEFT JOIN library_sources src ON src.id = s.source_id
         WHERE s.deleted_at IS NOT NULL ORDER BY s.deleted_at DESC, s.id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let audio_path: String = r.get(3)?;
            Ok(RemovedSong {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                audio_present: !audio_path.is_empty() && Path::new(&audio_path).is_file(),
                audio_path,
                removed_at: r.get(4)?,
                source: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("jp-sources-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn dir(&self, rel: &str) -> String {
            let path = self.0.join(rel);
            std::fs::create_dir_all(&path).unwrap();
            path.to_string_lossy().into_owned()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::create_tables_on(&conn).unwrap();
        conn
    }
    fn song(conn: &Connection, id: &str, path: &str) {
        conn.execute("INSERT INTO songs (id, title, artist, audio_path) VALUES (?1, 't', 'a', ?2)", params![id, path]).unwrap();
    }
    fn source_of(conn: &Connection, id: &str) -> Option<i64> {
        conn.query_row("SELECT source_id FROM songs WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn coverage_follows_recursion_and_ignores_case_and_separators_on_windows() {
        assert!(covers("D:/Music", true, "D:/Music/A/B/x.flac"));
        assert!(!covers("D:/Music", false, "D:/Music/A/x.flac"));
        assert!(covers("D:/Music/", false, "D:/Music/x.flac"));
        assert!(!covers("D:/Music", true, "D:/Music2/x.flac"), "前缀相同的兄弟文件夹不算");
        assert!(!covers("D:/Music", true, "D:/Music"));
        if cfg!(windows) {
            assert!(covers(r"d:\music", true, r"D:\Music\A\x.flac"));
        }
    }

    #[test]
    fn songs_belong_to_the_closest_source_and_loose_songs_to_none() {
        let scratch = Scratch::new("assign");
        let music = scratch.dir("music");
        let anime = scratch.dir("music/anime");
        let conn = db();
        let sep = std::path::MAIN_SEPARATOR;
        song(&conn, "001", &format!("{music}{sep}A{sep}x.flac"));
        song(&conn, "002", &format!("{anime}{sep}y.flac"));
        song(&conn, "003", &format!("{}{sep}loose.flac", scratch.0.display()));
        let outer = add(&conn, &music, "", true).unwrap();
        // 外面是递归的：里面那个加不进来
        let err = add(&conn, &anime, "", false).unwrap_err().to_string();
        assert!(err.contains("已经包含在"), "{err}");
        assert_eq!(assign(&conn).unwrap(), 2);
        assert_eq!(source_of(&conn, "001"), Some(outer));
        assert_eq!(source_of(&conn, "002"), Some(outer));
        assert_eq!(source_of(&conn, "003"), None);
        assert_eq!(assign(&conn).unwrap(), 0, "再算一遍不改");
    }

    #[test]
    fn a_recursive_parent_absorbs_the_folders_inside_it() {
        let scratch = Scratch::new("absorb");
        let a = scratch.dir("music/A");
        let b = scratch.dir("music/B");
        let music = scratch.dir("music");
        let conn = db();
        add(&conn, &a, "", false).unwrap();
        add(&conn, &b, "", false).unwrap();
        let parent = add(&conn, &music, "", true).unwrap();
        let roots: Vec<i64> = all(&conn).unwrap().iter().map(|s| s.id).collect();
        assert_eq!(roots, vec![parent]);
        // 不递归的上一级不吞里面的
        let other = Scratch::new("absorb-flat");
        let inner = other.dir("x/inner");
        let outer = other.dir("x");
        add(&conn, &inner, "", false).unwrap();
        add(&conn, &outer, "", false).unwrap();
        assert_eq!(all(&conn).unwrap().len(), 3);
    }

    #[test]
    fn duplicates_and_missing_folders_are_refused() {
        let scratch = Scratch::new("refuse");
        let music = scratch.dir("music");
        let conn = db();
        add(&conn, &music, "", true).unwrap();
        assert!(add(&conn, &format!("{music}{}", std::path::MAIN_SEPARATOR), "", true).unwrap_err().to_string().contains("已经在曲库里"));
        assert!(add(&conn, &scratch.0.join("nope").to_string_lossy(), "", true).unwrap_err().to_string().contains("找不到"));
    }

    #[test]
    fn list_counts_live_and_removed_songs_and_forget_leaves_songs_alone() {
        let scratch = Scratch::new("list");
        let music = scratch.dir("music");
        let conn = db();
        let sep = std::path::MAIN_SEPARATOR;
        song(&conn, "001", &format!("{music}{sep}x.flac"));
        song(&conn, "002", &format!("{music}{sep}y.flac"));
        let id = add(&conn, &music, "我的歌", true).unwrap();
        assign(&conn).unwrap();
        conn.execute("UPDATE songs SET deleted_at = '2026-11-06T00:00:00Z' WHERE id = '002'", []).unwrap();
        let view = &list(&conn).unwrap()[0];
        assert_eq!((view.songs, view.removed, view.present), (1, 1, true));
        assert_eq!(view.source.label, "我的歌");
        set_enabled(&conn, id, false).unwrap();
        assert!(!get(&conn, id).unwrap().unwrap().enabled);
        record_scan(&conn, id, Some("盘没插")).unwrap();
        assert_eq!(get(&conn, id).unwrap().unwrap().last_scan_error.as_deref(), Some("盘没插"));
        forget(&conn, id).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM songs WHERE source_id IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 2, "移除来源这一步本身不删歌");
    }

    #[test]
    fn removing_and_restoring_only_touches_what_it_should() {
        let conn = db();
        song(&conn, "001", "D:/m/a.flac");
        song(&conn, "002", "D:/m/b.flac");
        let ids = vec!["001".to_string(), "002".to_string()];
        assert_eq!(mark_removed(&conn, &ids[..1]).unwrap(), 1);
        let first: String = conn.query_row("SELECT deleted_at FROM songs WHERE id='001'", [], |r| r.get(0)).unwrap();
        conn.execute("UPDATE songs SET deleted_at = '2000-01-01T00:00:00Z' WHERE id = '001'", []).unwrap();
        assert_eq!(mark_removed(&conn, &ids).unwrap(), 1, "已经移出的不再算");
        let kept: String = conn.query_row("SELECT deleted_at FROM songs WHERE id='001'", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, "2000-01-01T00:00:00Z", "原来的移出时间留着");
        assert!(!first.is_empty());
        let listed = removed(&conn).unwrap();
        assert_eq!(listed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["002", "001"]);
        assert!(!listed[0].audio_present);
        let live: i64 = conn.query_row("SELECT COUNT(*) FROM live_songs", [], |r| r.get(0)).unwrap();
        assert_eq!(live, 0);
        assert_eq!(restore(&conn, &ids).unwrap(), 2);
        assert_eq!(restore(&conn, &ids).unwrap(), 0);
        let live: i64 = conn.query_row("SELECT COUNT(*) FROM live_songs", [], |r| r.get(0)).unwrap();
        assert_eq!(live, 2);
    }
}
