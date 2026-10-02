//! `corpus.db` 的结构版本和前向迁移。
//!
//! **为什么需要**：发出去的程序原来一点 schema 演进能力都没有。
//! `ensure_schema` 只跑 `CREATE TABLE IF NOT EXISTS`——它能补上缺的**表**，
//! 但对已经存在的表**加不了一列**。加列的逻辑只存在于 Python 侧
//! （`library/schema.py` 的 `ADDED_COLUMNS`、`scripts/migrate_db.py`），
//! 而安装目录里只有 `jp-app.exe`，用户手上没有 Python。
//! 于是下一次任何功能需要一个新列，装了的用户就会在某个查询上撞见 `no such column`。
//!
//! 另外版本号两边各记一套：Python 写 `PRAGMA user_version = 20260908`，
//! Rust 从不读也从不写，所以新装用户的库戳是 `0`——和「远古库」分不出来，
//! 将来连「从哪一版升」都判断不了。
//!
//! 这里补上三件事，都很小：
//!
//! 1. **戳版本**。`user_version` 由这个模块统一维护，沿用 Python 的那个数字。
//! 2. **按序补差**。库戳着 V，就跑所有 `version > V` 的步骤。
//! 3. **挡降级**。库比我新就直接报错，而不是等某个查询炸出 `no such column`。
//!    `jp-dict` 的 `dictionaries.db` 早就这么做了，这里只是把 `corpus.db` 补齐。
//!
//! **步骤只许加东西**（加列、加表、加索引），不许删、不许改数据。
//! 有这条约束才敢不做备份——991MB 的库 `VACUUM INTO` 一次要很久，
//! 而每次启动都备份是不可接受的。约束由 `steps_only_ever_add_things` 守着。

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

/// 当前结构版本。`schema.sql` 建出来的就是这一版。
///
/// 这个数字沿用 Python 侧 `scripts/migrate_db.py` 的 `SCHEMA_USER_VERSION`，
/// 因为两边建出来的东西**逐表逐列逐索引完全一致**（26 表 26 索引，对过）。
/// 共用一个编号，Python 迁移过的库和本程序新建的库才不会互相看成「另一版」。
///
/// 格式是 `YYYYMMDD`：单调递增，一眼看得出先后。
pub const CURRENT: i64 = 20_260_908;

/// 一步迁移里的一个改动。
enum Change {
    /// `ALTER TABLE <table> ADD COLUMN <column> <ddl>`。
    ///
    /// 列已经在就跳过，所以重复跑是安全的（没戳过版本的库会把每一步都重跑一遍）。
    /// **表还不存在也跳过**：那说明这个库老到连这张表都没有，
    /// 稍后 `schema.sql` 会把它整张建出来、列本来就是全的。
    AddColumn {
        table: &'static str,
        column: &'static str,
        ddl: &'static str,
    },
    /// 其他**只加不减**的 DDL（建表、建索引）。自己写成 `IF NOT EXISTS`。
    #[allow(dead_code)] // 现在还没有用到，但枚举要留着——下一次加表就是它
    Exec(&'static str),
}

struct Step {
    version: i64,
    /// 一句人话，进日志和诊断信息
    what: &'static str,
    changes: &'static [Change],
}

/// 全部迁移步骤，**按 version 升序**。
///
/// 已经存在的库戳着 V，就跑所有 `version > V` 的步骤。空库不跑任何一步——
/// `schema.sql` 本来就是最新的。
const STEPS: &[Step] = &[Step {
    version: 20_260_908,
    what: "songs.album_id（专辑归属）",
    // 20260907 那一版的库没有这一列。`schema.sql` 的
    // `CREATE TABLE IF NOT EXISTS songs` 对已有的表不会加列，只能显式 ALTER。
    // 对应 Python 的 `library/schema.py::ADDED_COLUMNS`。
    changes: &[Change::AddColumn {
        table: "songs",
        column: "album_id",
        ddl: "INTEGER",
    }],
}];

/// 这次打开做了什么。调用方拿去写日志。
#[derive(Debug, Clone)]
pub struct Applied {
    /// 打开之前库戳着几版。空库和从没戳过的库都是 0
    pub from: i64,
    /// 现在戳着几版，一定等于 [`CURRENT`]
    pub to: i64,
    /// 本来是个空库（一张表都没有），这次才建起来
    pub created: bool,
    /// 实际跑过的步骤，各一句人话
    pub steps: Vec<&'static str>,
}

impl Applied {
    /// 一句话总结，给日志用
    pub fn summary(&self) -> String {
        if self.created {
            return format!("新建语料库，结构版本 {}", self.to);
        }
        if self.steps.is_empty() {
            return format!("语料库结构版本 {}，无需迁移", self.to);
        }
        format!(
            "语料库结构 {} → {}，跑了 {} 步：{}",
            self.from,
            self.to,
            self.steps.len(),
            self.steps.join("、")
        )
    }
}

/// 把库升到 [`CURRENT`]。可反复调用，什么都不用改时是空操作。
///
/// **顺序是「先补列、再建表」**，反过来会炸：`schema.sql` 里有
/// `CREATE INDEX idx_songs_album ON songs(album_id)`，而 `album_id` 是
/// 20260908 这一步加的——先跑 `schema.sql` 的话，老库上这条索引会报
/// `no such column: album_id`，整个建表批次失败，程序起不来。
/// （这不是推演，是 `an_old_database_gets_the_missing_column_and_the_missing_tables`
/// 当场抓到的。）
///
/// 所以分工是：步骤负责**老表上缺的列**，`schema.sql` 负责**缺的表和索引**，
/// 并且索引可以依赖步骤刚加上的列。
pub fn apply(conn: &Connection) -> Result<Applied> {
    let from = version(conn)?;
    if from > CURRENT {
        // 旧程序打开新库。不挡的话表现是过一会儿某个查询炸一句 `no such column`，
        // 用户完全看不出发生了什么——更糟的是他可能以为库坏了。
        bail!(
            "这个语料库的结构版本是 {from}，本程序只认识到 {CURRENT}。\
             它是更新版本的 JPOP Corpus Tool 建的，请先升级程序再打开。"
        );
    }

    // 空库（新装的程序第一次启动）和老库要分开：空库跑完 `schema.sql` 就已经是
    // 最新结构了，一步都不用补；老库才要逐步补列。
    let created = !table_exists(conn, "songs")?;

    let mut steps = Vec::new();
    if !created {
        for step in STEPS.iter().filter(|step| step.version > from) {
            run(conn, step).with_context(|| format!("迁移到 {} 失败", step.version))?;
            steps.push(step.what);
        }
    }

    crate::Corpus::create_tables_on(conn)?;
    stamp(conn, CURRENT)?;
    Ok(Applied {
        from,
        to: CURRENT,
        created,
        steps,
    })
}

/// 库现在戳着几版。从没戳过的是 0。
pub fn version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// 跑一步，**改动和版本戳在同一个事务里**提交。
///
/// 中途断电的话这一步整个回滚，戳也不会前进；下次打开重跑一遍即可。
/// （`PRAGMA user_version` 写的是数据库头，和普通写入一样走 journal，是事务性的。）
fn run(conn: &Connection, step: &Step) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for change in step.changes {
        match change {
            Change::AddColumn { table, column, ddl } => {
                if table_exists(&tx, table)? && !column_exists(&tx, table, column)? {
                    tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))
                        .with_context(|| format!("给 {table} 加列 {column} 失败"))?;
                }
            }
            Change::Exec(sql) => tx.execute_batch(sql)?,
        }
    }
    stamp(&tx, step.version)?;
    tx.commit()?;
    Ok(())
}

fn stamp(conn: &Connection, version: i64) -> Result<()> {
    // user_version 不能用占位符，只能拼。version 来自本文件的常量，不是外部输入。
    conn.execute_batch(&format!("PRAGMA user_version = {version}"))
        .with_context(|| format!("写不进版本戳 {version}"))?;
    Ok(())
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 20260907 那一版：有 songs 但没有 album_id，也没有 20260908 加的那几张表。
    fn old_database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL,
                year TEXT, album TEXT, genre TEXT, audio_path TEXT,
                corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT,
                cover_path TEXT, duration_sec REAL);
             INSERT INTO songs (id, title, artist) VALUES ('001', '夜に駆ける', 'YOASOBI');
             PRAGMA user_version = 20260907;",
        )
        .unwrap();
        conn
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    #[test]
    fn a_brand_new_database_lands_on_the_current_version_without_running_any_step() {
        let conn = Connection::open_in_memory().unwrap();
        let applied = apply(&conn).unwrap();
        assert!(applied.created, "空库应当标成新建");
        assert_eq!(applied.from, 0);
        assert_eq!(applied.to, CURRENT);
        assert!(applied.steps.is_empty(), "空库不该跑任何一步：schema.sql 就是最新的");
        assert_eq!(version(&conn).unwrap(), CURRENT);
        // schema.sql 里本来就有这一列
        assert!(columns(&conn, "songs").contains(&"album_id".to_string()));
    }

    #[test]
    fn an_old_database_gets_the_missing_column_and_the_missing_tables() {
        let conn = old_database();
        assert!(!columns(&conn, "songs").contains(&"album_id".to_string()));

        let applied = apply(&conn).unwrap();
        assert!(!applied.created, "有 songs 的库不是新建");
        assert_eq!(applied.from, 20_260_907);
        assert_eq!(applied.to, CURRENT);
        assert_eq!(applied.steps, vec!["songs.album_id（专辑归属）"]);

        // 加列（步骤干的）
        assert!(columns(&conn, "songs").contains(&"album_id".to_string()));
        // 加表（schema.sql 干的）
        assert!(table_exists(&conn, "albums").unwrap());
        assert!(table_exists(&conn, "track_credits").unwrap());
        // 原来的数据一行不少
        let title: String = conn
            .query_row("SELECT title FROM songs WHERE id='001'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "夜に駆ける");
    }

    /// Python 迁移过的真库就是这个状态：戳着 20260908，结构已经齐了。
    /// 再打开不该做任何事——否则每次启动都在动用户的库。
    #[test]
    fn a_database_already_at_the_current_version_is_left_alone() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        conn.execute_batch("INSERT INTO songs (id, title, artist) VALUES ('001','あ','い')")
            .unwrap();

        let again = apply(&conn).unwrap();
        assert_eq!(again.from, CURRENT);
        assert!(!again.created);
        assert!(again.steps.is_empty(), "已经是最新的就不该跑步骤");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "数据不能被动");
    }

    /// 从没戳过版本、但结构其实已经齐了的库（本程序早先版本建的）。
    /// 步骤必须幂等：它们会被当成「从 0 升」全部重跑一遍。
    #[test]
    fn an_unstamped_but_complete_database_survives_rerunning_every_step() {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::create_tables_on(&conn).unwrap();
        assert_eq!(version(&conn).unwrap(), 0, "还没戳过");

        let applied = apply(&conn).unwrap();
        assert_eq!(applied.from, 0);
        assert_eq!(applied.to, CURRENT);
        assert_eq!(
            applied.steps,
            vec!["songs.album_id（专辑归属）"],
            "没戳过就得把每一步都重跑一遍"
        );
        // 列本来就在，重跑没把它加成两列
        let album_id = columns(&conn, "songs")
            .into_iter()
            .filter(|c| c == "album_id")
            .count();
        assert_eq!(album_id, 1);
    }

    /// 旧程序打开新库：要当场说清楚，而不是过一会儿炸一句 `no such column`。
    #[test]
    fn a_database_from_a_newer_build_is_refused_with_a_readable_message() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn).unwrap();
        conn.execute_batch("PRAGMA user_version = 99999999").unwrap();

        let err = apply(&conn).unwrap_err().to_string();
        assert!(err.contains("99999999"), "{err}");
        assert!(err.contains("升级程序"), "{err}");
    }

    /// 步骤表本身的约束。
    #[test]
    fn steps_are_sorted_and_the_last_one_is_the_current_version() {
        assert!(
            STEPS.windows(2).all(|w| w[0].version < w[1].version),
            "STEPS 要按 version 严格升序"
        );
        if let Some(last) = STEPS.last() {
            assert_eq!(
                last.version, CURRENT,
                "最后一步的版本号就是 CURRENT，否则升完还差一截"
            );
        }
        assert!(STEPS.iter().all(|s| !s.what.is_empty()), "每一步都要有一句人话");
    }

    /// **不做备份的前提**：步骤只许加东西。
    ///
    /// 哪天真需要删列、改数据、重建表，就得先把备份补上，再放开这条——
    /// 这个断言是让那件事必须被想一次，而不是顺手写进去。
    #[test]
    fn steps_only_ever_add_things() {
        const FORBIDDEN: [&str; 6] = ["DROP", "DELETE", "UPDATE", "RENAME", "REPLACE", "TRUNCATE"];
        for step in STEPS {
            for change in step.changes {
                let Change::Exec(sql) = change else { continue };
                let upper = sql.to_uppercase();
                for word in FORBIDDEN {
                    assert!(
                        !upper.contains(word),
                        "{} 这一步里有 {word}；破坏性迁移要先有备份",
                        step.version
                    );
                }
            }
        }
    }
}
