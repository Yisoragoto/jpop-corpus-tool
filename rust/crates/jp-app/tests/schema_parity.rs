//! 两份建表语句的对账。
//!
//! `scrape_state` / `scrape_attempts` / `track_original_metadata` 这三张表的
//! DDL 在仓库里**各有两份**：
//!
//! * `jp-corpus/schema.sql`——新库是它建的（`AppState::new` 里走 `ensure_schema`）；
//! * `jp-scraper/src/store.rs` 的 `SCHEMA`——刮削前 `ScrapeStore::ensure_schema` 建的。
//!
//! 两份都是 `CREATE TABLE IF NOT EXISTS`，**谁先跑谁说了算，漂了不报错**：
//! 改了 `schema.sql` 而忘了 `store.rs`，新库和刮削过的老库就会长得不一样，
//! 表现是某台机器上某个查询查不到列——最难查的那一类。
//!
//! 这里不去统一两份代码（jp-scraper 不依赖 jp-corpus，要让它依赖就是倒置分层），
//! 只把它们**逐列对一遍**：漂了，这个测试立刻红。
//!
//! 放在 jp-app 是因为只有它同时依赖这两个 crate。

use rusqlite::Connection;

/// 两份 DDL 共有的表。jp-scraper 只建这三张。
const SHARED: [&str; 3] = ["scrape_state", "scrape_attempts", "track_original_metadata"];

/// 一张表的列定义：(序号, 列名, 类型, 非空, 默认值, 主键序号)。
type Column = (i64, String, String, i64, Option<String>, i64);

fn columns(conn: &Connection, table: &str) -> Vec<Column> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("查不了表结构");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("查不了表结构");
    rows.map(Result::unwrap).collect()
}

fn indexes(conn: &Connection, table: &str) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT name, COALESCE(sql, '') FROM sqlite_master \
             WHERE type='index' AND tbl_name=?1 AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("查不了索引");
    let rows = stmt
        .query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("查不了索引");
    rows.map(Result::unwrap).collect()
}

/// 用 `schema.sql` 建的库
fn from_corpus_schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
    conn
}

/// 用 jp-scraper 自己那份建的库
fn from_scraper_schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    jp_scraper::ScrapeStore::new(&conn).ensure_schema().unwrap();
    conn
}

#[test]
fn the_scraper_tables_are_defined_the_same_way_in_both_places() {
    let corpus = from_corpus_schema();
    let scraper = from_scraper_schema();

    for table in SHARED {
        let a = columns(&corpus, table);
        let b = columns(&scraper, table);
        assert!(!a.is_empty(), "schema.sql 里没有 {table}");
        assert!(!b.is_empty(), "jp-scraper 的 SCHEMA 里没有 {table}");
        assert_eq!(
            a, b,
            "{table} 的列对不上。\n  schema.sql: {a:#?}\n  jp-scraper: {b:#?}\n\
             两份 DDL 必须同时改——谁先建表谁说了算，漂了不会报错。"
        );
    }
}

#[test]
fn the_scraper_indexes_are_defined_the_same_way_in_both_places() {
    let corpus = from_corpus_schema();
    let scraper = from_scraper_schema();

    for table in SHARED {
        // 把 SQL 里的空白压平再比：两份的缩进和对齐本来就不一样，
        // 比的是索引**建在哪些列上**，不是排版。
        let flat = |rows: Vec<(String, String)>| -> Vec<(String, String)> {
            rows.into_iter()
                .map(|(name, sql)| (name, sql.split_whitespace().collect::<Vec<_>>().join(" ")))
                .collect()
        };
        assert_eq!(
            flat(indexes(&corpus, table)),
            flat(indexes(&scraper, table)),
            "{table} 的索引对不上"
        );
    }
}

/// 反过来也要成立：刮削那份跑在一个已经建好的新库上时，不能把表改成别的样子。
/// （`IF NOT EXISTS` 保证了这一点，但这是**被依赖的行为**，值得钉住。）
#[test]
fn running_the_scraper_schema_on_a_fresh_corpus_changes_nothing() {
    let conn = from_corpus_schema();
    let before: Vec<Vec<Column>> = SHARED.iter().map(|t| columns(&conn, t)).collect();

    jp_scraper::ScrapeStore::new(&conn).ensure_schema().unwrap();

    let after: Vec<Vec<Column>> = SHARED.iter().map(|t| columns(&conn, t)).collect();
    assert_eq!(before, after, "刮削的建表语句动了新库的结构");
}
