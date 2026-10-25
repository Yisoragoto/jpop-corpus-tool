//! 旧版登记的词典包，两端共用。
use std::path::Path;
use anyhow::Result;
use serde::Serialize;

/// 旧版（PyQt）登记过的词典包。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacySource {
    pub name: String,
    pub path: String,
    /// terms / freq / pitch
    pub kind: String,
    pub exists: bool,
    /// 新词典库里已经有同名词典
    pub imported: bool,
}

/// 读 corpus.db 的 `dict_registry`（只读），列出旧版导入过、原始 zip 还登记着路径的词典。
/// `#legacy_*` 这类旧缓存没有原始文件，跳过。
pub fn legacy_sources(corpus_db: &Path, imported_titles: &[String]) -> Result<Vec<LegacySource>> {
    let conn = rusqlite::Connection::open_with_flags(
        corpus_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let has_table: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'dict_registry')",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT name, zip_path, dict_type FROM dict_registry WHERE zip_path NOT LIKE '#%' ORDER BY sort_order, name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (name, path, kind) = row?;
        out.push(LegacySource {
            exists: Path::new(&path).is_file(),
            // 旧版登记名是 index.json 的 title 去掉首尾空白
            imported: imported_titles.iter().any(|t| t.trim() == name),
            name,
            path,
            kind,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn legacy_sources_skip_cached_tables_and_mark_what_is_already_imported() {
        let path = std::env::temp_dir().join(format!("jp_app_legacy_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE dict_registry (name TEXT, zip_path TEXT, dict_type TEXT, sort_order INTEGER);",
            )
            .unwrap();
            let existing = env!("CARGO_MANIFEST_PATH");
            conn.execute(
                "INSERT INTO dict_registry VALUES ('JPDB（旧缓存）', '#legacy_freq', 'freq', 0), ('新明解国語辞典　第八版', ?1, 'terms', 2), ('JPDB', 'D:/nope/JPDB.zip', 'freq', 1)",
                [existing],
            )
            .unwrap();
        }
        let sources = legacy_sources(&path, &["新明解国語辞典　第八版 ".to_owned()]).unwrap();
        let _ = std::fs::remove_file(&path);
        let got: Vec<(&str, bool, bool)> = sources
            .iter()
            .map(|s| (s.name.as_str(), s.exists, s.imported))
            .collect();
        assert_eq!(
            got,
            [
                ("JPDB", false, false),
                ("新明解国語辞典　第八版", true, true)
            ]
        );
    }

    #[test]
    fn a_corpus_without_the_legacy_table_has_no_sources() {
        let path =
            std::env::temp_dir().join(format!("jp_app_legacy_empty_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (x);")
            .unwrap();
        let sources = legacy_sources(&path, &[]).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(sources.is_empty());
    }
}
