//! 升级结构之前给整库留一份备份。
//!
//! **什么时候备份**：只在 [`crate::migrations::apply`] 真要把一个老库往上升的时候。
//! 每次启动都备份不行——用户的库将近 1 GB，`VACUUM INTO` 一次要十几秒。
//! 一个版本只升一次，所以一次升级只多等这一回。
//!
//! **为什么是 `VACUUM INTO`**：它在一个读事务里把库原样写进新文件，
//! WAL 里还没 checkpoint 的内容也在里面，得到的是一致的快照，而且自带整理碎片。
//! 直接复制文件的话，要处理 `-wal` 和复制途中被写的问题。
//!
//! **放哪、留几份**：库旁边的 `backups/`（Python 旧版也用这个目录），
//! 文件名 `corpus_v<旧版本>_<UTC 时间>.db`。只留最近 [`KEEP`] 份，
//! 并且只清理这个前缀的文件；Python 旧版留下的 `corpus_<时间>.db` 和用户自己放的东西一律不碰。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// 最多留几份升级前的备份
pub const KEEP: usize = 3;

/// 本模块生成的文件都以它开头，清理时只认这个前缀
const PREFIX: &str = "corpus_v";

/// 给 `conn` 打开的库做一份备份，返回备份文件的路径。
///
/// 内存库、临时库没有文件路径，返回 `None`，什么都不做。
/// 写到一半失败时，会删掉写了一半的文件再报错。
pub fn before_upgrade(conn: &Connection, from_version: i64) -> Result<Option<PathBuf>> {
    let Some(db_path) = conn.path().filter(|p| !p.is_empty()).map(PathBuf::from) else {
        return Ok(None);
    };
    let dir = db_path
        .parent()
        .map(|p| p.join("backups"))
        .unwrap_or_else(|| PathBuf::from("backups"));
    std::fs::create_dir_all(&dir).with_context(|| format!("建不了备份目录 {}", dir.display()))?;

    let target = unique_target(&dir, from_version, now_unix());
    let target_str = target
        .to_str()
        .with_context(|| format!("备份路径不是合法的 UTF-8：{}", target.display()))?;
    if let Err(err) = conn.execute("VACUUM INTO ?1", [target_str]) {
        let _ = std::fs::remove_file(&target);
        return Err(err).with_context(|| {
            format!(
                "升级前备份失败，没有升级。请确认 {} 所在的磁盘有不少于语料库大小的剩余空间",
                dir.display()
            )
        });
    }
    prune(&dir, KEEP)?;
    Ok(Some(target))
}

/// `corpus_v<版本>_<UTC 时间>.db`；同一秒里已经有了就加序号
fn unique_target(dir: &Path, from_version: i64, unix: u64) -> PathBuf {
    let stamp = utc_stamp(unix);
    let mut path = dir.join(format!("{PREFIX}{from_version}_{stamp}.db"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{PREFIX}{from_version}_{stamp}-{n}.db"));
        n += 1;
    }
    path
}

/// 只留最新的 `keep` 份。新旧按文件名里的时间排，不看修改时间：
/// 用户复制、同步备份目录时修改时间会变，文件名不会。
fn prune(dir: &Path, keep: usize) -> Result<()> {
    let mut ours: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .with_context(|| format!("读不了备份目录 {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stamp = parse_name(&name)?;
            Some((stamp, entry.path()))
        })
        .collect();
    ours.sort();
    let excess = ours.len().saturating_sub(keep);
    for (_, path) in ours.into_iter().take(excess) {
        std::fs::remove_file(&path).with_context(|| format!("删不掉旧备份 {}", path.display()))?;
    }
    Ok(())
}

/// 认出本模块生成的文件名，返回可以按字典序排先后的时间部分（含同秒序号）。
fn parse_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix(PREFIX)?.strip_suffix(".db")?;
    let (version, stamp) = rest.split_once('_')?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // 20261106T115301Z，或者后面带 -2、-3
    let (time, seq) = match stamp.split_once('-') {
        Some((time, seq)) => (time, seq.parse::<u32>().ok()?),
        None => (stamp, 1),
    };
    let valid = time.len() == 16
        && time.as_bytes()[8] == b'T'
        && time.ends_with('Z')
        && time[..8].bytes().all(|b| b.is_ascii_digit())
        && time[9..15].bytes().all(|b| b.is_ascii_digit());
    valid.then(|| format!("{time}-{seq:04}"))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Unix 秒 → `YYYYMMDDTHHMMSSZ`（UTC）。标准库不带日历，这里用 Howard Hinnant 的 civil_from_days。
fn utc_stamp(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试一个空目录，测完删掉
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "jp-corpus-backup-{tag}-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn utc_stamps_match_known_dates() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        // 2026-11-06 11:53:01 UTC
        assert_eq!(utc_stamp(1_793_965_981), "20261106T115301Z");
    }

    #[test]
    fn only_our_own_file_names_are_recognised() {
        assert!(parse_name("corpus_v20261101_20261106T115301Z.db").is_some());
        assert!(parse_name("corpus_v20261101_20261106T115301Z-2.db").is_some());
        // Python 旧版的备份、用户自己的文件
        assert!(parse_name("corpus_20260525_104800.db").is_none());
        assert!(parse_name("corpus_vX_20261106T115301Z.db").is_none());
        assert!(parse_name("corpus_v20261101_20261106T115301Z.db.bak").is_none());
        assert!(parse_name("notes.txt").is_none());
    }

    #[test]
    fn a_memory_database_is_not_backed_up() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(before_upgrade(&conn, 20_261_101).unwrap(), None);
    }

    #[test]
    fn the_backup_holds_rows_that_were_still_in_the_wal() {
        let scratch = Scratch::new("wal");
        let db = scratch.0.join("corpus.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
             CREATE TABLE songs (id TEXT PRIMARY KEY, title TEXT);
             INSERT INTO songs VALUES ('001', '夜に駆ける');",
        )
        .unwrap();
        let path = before_upgrade(&conn, 20_261_101).unwrap().unwrap();
        // macOS 的临时目录是符号链接（/var → /private/var），比较前都换成真实路径
        let backups = std::fs::canonicalize(scratch.0.join("backups")).unwrap();
        assert!(std::fs::canonicalize(&path).unwrap().starts_with(backups));

        let copy = Connection::open(&path).unwrap();
        let title: String = copy.query_row("SELECT title FROM songs", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "夜に駆ける");
    }

    #[test]
    fn only_the_newest_three_of_ours_are_kept_and_nothing_else_is_touched() {
        let scratch = Scratch::new("prune");
        let dir = &scratch.0;
        for name in [
            "corpus_v20261029_20261101T000000Z.db",
            "corpus_v20261031_20261102T000000Z.db",
            "corpus_v20261031_20261102T000000Z-2.db",
            "corpus_v20261101_20261103T000000Z.db",
            "corpus_v20261101_20261104T000000Z.db",
            "corpus_20260525_104800.db",
            ".gitkeep",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        prune(dir, KEEP).unwrap();
        assert_eq!(
            names(dir),
            [
                ".gitkeep",
                "corpus_20260525_104800.db",
                "corpus_v20261031_20261102T000000Z-2.db",
                "corpus_v20261101_20261103T000000Z.db",
                "corpus_v20261101_20261104T000000Z.db",
            ]
        );
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_overwrite_each_other() {
        let scratch = Scratch::new("same-second");
        let first = unique_target(&scratch.0, 20_261_101, 1_793_965_981);
        std::fs::write(&first, b"").unwrap();
        let second = unique_target(&scratch.0, 20_261_101, 1_793_965_981);
        assert_ne!(first, second);
        assert!(parse_name(second.file_name().unwrap().to_str().unwrap()).is_some());
    }
}
