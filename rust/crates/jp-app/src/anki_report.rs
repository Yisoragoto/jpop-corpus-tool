//! 挖词报告的应用层：查本地 JLPT、写 `output/corpus_report.html`、用浏览器打开。
//!
//! 统计和页面都在 `jp_anki::mining_report`（和 Python 版 `legacy/generate_report.py` 逐字节对过）。
//! 输出位置和 PyQt 版一样是项目根目录下的 `output/corpus_report.html`，每次覆盖——这是生成物，不是用户数据。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiningReportResult {
    /// 写出的 HTML
    pub path: String,
    /// 读的哪个 collection
    pub collection_path: String,
    #[serde(flatten)]
    pub summary: jp_anki::mining_report::ReportSummary,
    /// 浏览器没打开时的原因。文件已经写好了，不算失败
    pub open_error: String,
}

/// 本地 `jlpt_cache`：词元 → 等级（统计页也读这张表）。没有这张表时是空的
pub fn jlpt_levels(conn: &rusqlite::Connection) -> Result<HashMap<String, String>> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='jlpt_cache'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !exists {
        return Ok(HashMap::new());
    }
    let mut stmt = conn.prepare("SELECT lemma, level FROM jlpt_cache WHERE level <> ''")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (lemma, level) = row?;
        out.insert(lemma, level.trim_start_matches("JLPT-").to_uppercase());
    }
    Ok(out)
}

/// 「生成时间」由界面按本地时间给（`YYYY-MM-DD HH:MM`），原样写进页面，所以只收这几种字符
pub fn valid_timestamp(now: &str) -> bool {
    !now.is_empty()
        && now.len() <= 32
        && now
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | ' '))
}

/// 读 collection、生成报告写到 `<root>/output/corpus_report.html`。不打开浏览器
pub fn generate(
    root: &Path,
    collection: &Path,
    levels: &HashMap<String, String>,
    now: &str,
    deck_contains: &str,
) -> Result<MiningReportResult> {
    anyhow::ensure!(valid_timestamp(now), "生成时间的格式不对：{now}");
    let lookup = |expression: &str| levels.get(expression).cloned();
    let mined = jp_anki::mining_report::load(collection, deck_contains, &lookup)?;
    if mined.cards.is_empty() {
        let scope = if deck_contains.trim().is_empty() {
            String::new()
        } else {
            format!("（牌组名含「{}」）", deck_contains.trim())
        };
        anyhow::bail!("Anki 里还没有 JPOP Corpus 或 Lyrics 卡片{scope}。先挖几个词再来生成报告。");
    }
    let html = jp_anki::mining_report::render_html(&mined, now);
    let path = report_path(root);
    jp_anki::mining_report::write_report(&path, &html)?;
    Ok(MiningReportResult {
        path: path.display().to_string(),
        collection_path: collection.display().to_string(),
        summary: jp_anki::mining_report::summarize(&mined),
        open_error: String::new(),
    })
}

pub fn report_path(root: &Path) -> PathBuf {
    root.join("output").join("corpus_report.html")
}

/// 用系统默认的浏览器打开（PyQt 版是 `os.startfile`）
pub fn open_in_browser(path: &Path) -> Result<()> {
    // explorer 碰到正斜杠会打开「文档」文件夹而不是这个文件；根目录可能来自写成 D:/… 的环境变量
    #[cfg(windows)]
    let (program, target) = (
        "explorer",
        PathBuf::from(path.to_string_lossy().replace('/', "\\")),
    );
    #[cfg(target_os = "macos")]
    let (program, target) = ("open", path.to_path_buf());
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, target) = ("xdg-open", path.to_path_buf());
    std::process::Command::new(program)
        .arg(&target)
        .spawn()
        .with_context(|| format!("打不开 {}", target.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_from_the_ui_are_the_only_thing_written_raw() {
        assert!(valid_timestamp("2026-09-17 20:05"));
        assert!(!valid_timestamp(""));
        assert!(!valid_timestamp("<script>"));
        assert!(!valid_timestamp("2026-09-17T20:05"));
    }

    #[test]
    fn generate_writes_the_page_with_local_jlpt_and_refuses_an_empty_collection() {
        let dir = std::env::temp_dir().join(format!("jp-app-mining-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let collection = dir.join("collection.anki2");
        {
            let conn = rusqlite::Connection::open(&collection).unwrap();
            conn.execute_batch(
                "CREATE TABLE notetypes (id INTEGER PRIMARY KEY, name TEXT);
                 CREATE TABLE fields (ntid INTEGER, ord INTEGER, name TEXT);
                 CREATE TABLE notes (id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
                 CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, reps INTEGER);
                 CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO notetypes VALUES (2, 'Lyrics');
                 INSERT INTO decks VALUES (10, 'JPOP');
                 INSERT INTO fields VALUES (2, 0, 'Expression'), (2, 1, 'SongTitle'), (2, 2, 'Artist'), (2, 3, 'FreqSort');",
            )
            .unwrap();
            let flds = ["思い出す", "ユリイカ", "サカナクション", "400"].join("\x1f");
            conn.execute("INSERT INTO notes VALUES (1, 2, ?1)", [flds])
                .unwrap();
            conn.execute("INSERT INTO cards VALUES (1, 1, 10, 2)", [])
                .unwrap();
        }
        let levels = HashMap::from([("思い出す".to_owned(), "N3".to_owned())]);
        let root = dir.join("root");

        let result = generate(&root, &collection, &levels, "2026-09-17 20:05", "").unwrap();
        assert_eq!(result.path, report_path(&root).display().to_string());
        assert_eq!(
            (
                result.summary.cards,
                result.summary.studied,
                result.summary.words
            ),
            (1, 1, 1)
        );
        let html = std::fs::read_to_string(&result.path).unwrap();
        assert!(
            html.contains("<span class=\"badge\" style=\"background:#ff9800\">N3</span>"),
            "本地 JLPT 没查到"
        );
        assert!(html.contains("生成时间：2026-09-17 20:05"));
        if cfg!(windows) {
            assert!(
                html.contains("<html lang=\"zh\">\r\n<head>"),
                "Windows 上和 Python 一样写 CRLF"
            );
        }

        assert!(generate(&root, &collection, &levels, "<b>", "").is_err());
        let err = generate(&root, &collection, &levels, "2026-09-17 20:05", "英語").unwrap_err();
        assert!(format!("{err:#}").contains("牌组名含「英語」"), "{err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn jlpt_levels_skip_unrated_words_and_tolerate_a_missing_table() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert!(jlpt_levels(&conn).unwrap().is_empty());
        conn.execute_batch(
            "CREATE TABLE jlpt_cache (lemma TEXT PRIMARY KEY, level TEXT);
             INSERT INTO jlpt_cache VALUES ('夜', 'N5'), ('業務', ''), ('思い出す', 'JLPT-N3');",
        )
        .unwrap();
        let levels = jlpt_levels(&conn).unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels["思い出す"], "N3");
    }
}
