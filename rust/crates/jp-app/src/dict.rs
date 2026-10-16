//! 词典（Yomitan 格式）的应用层：后台导入作业、旧版词典清单、查词。
//!
//! 业务逻辑都在 `jp-dict` 里（能离线测，已和 Yomitan 逐项对账），这里只管线程、事件和状态。
//! 形状和 Anki 导出作业一样：作业自己开一条数据库连接，进度走事件，可中断。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use jp_dict::import::{
    ImportOptions, ImportProgress, ImportSummary, import_dictionary, open_archive,
};
use jp_dict::store::DictionaryStore;
use jp_dict::translator::FindTermsResult;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::state::AppState;

/// 查词的扫描长度、条数、选项、模式在 `jp-dict`，macOS 客户端用的是同一份
pub use jp_dict::lookup::{MAX_RESULTS, SCAN_LENGTH, lookup_options, parse_mode};

/// 词典导入作业的运行/取消状态。四个后台作业共用 `crate::job::Job`——
/// 「第二次启动不能抹掉第一个的取消」最早就是在这儿发现并修掉的，
/// 现在那条规则在 `job.rs` 里只有一份，另外三个作业也跟着对了。
pub type ImportJob = crate::job::Job;

/// `dict://progress`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictImportProgress {
    /// 第几本（从 0 起）
    pub index: usize,
    pub total: usize,
    pub path: String,
    pub file: String,
    pub done_files: usize,
    pub total_files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictImportResult {
    pub path: String,
    pub summary: Option<ImportSummary>,
    pub error: Option<String>,
}

/// `dict://done`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictImportDone {
    pub results: Vec<DictImportResult>,
    pub cancelled: bool,
    /// 作业整体失败（比如词典库打不开）。单本失败记在 results 里。
    pub error: Option<String>,
}

/// 后台逐本导入。一本失败不影响后面的；取消时正在导的那本整本回滚。
pub fn spawn_import<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ImportJob>,
    db_path: PathBuf,
    paths: Vec<String>,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "dict://done",
                DictImportDone {
                    results: Vec::new(),
                    cancelled: false,
                    error: Some("词典导入的后台线程崩了，详情见日志".into()),
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有词典在导入"))?;
    std::thread::spawn(move || {
        let total = paths.len();
        let mut results = Vec::new();
        let outcome = (|| -> Result<()> {
            let mut store = DictionaryStore::open(&db_path)?;
            for (index, path) in paths.iter().enumerate() {
                if job.cancelled() {
                    break;
                }
                let result = (|| -> Result<ImportSummary> {
                    let mut archive = open_archive(Path::new(path))?;
                    let options = ImportOptions {
                        source_path: path.clone(),
                        cancel: Some(job.cancel_flag()),
                        ..ImportOptions::default()
                    };
                    import_dictionary(
                        &mut store,
                        archive.as_mut(),
                        &options,
                        &mut |p: &ImportProgress| {
                            let _ = app.emit(
                                "dict://progress",
                                DictImportProgress {
                                    index,
                                    total,
                                    path: path.clone(),
                                    file: p.file.clone(),
                                    done_files: p.done_files,
                                    total_files: p.total_files,
                                },
                            );
                        },
                    )
                })();
                results.push(match result {
                    Ok(summary) => DictImportResult {
                        path: path.clone(),
                        summary: Some(summary),
                        error: None,
                    },
                    Err(err) => DictImportResult {
                        path: path.clone(),
                        summary: None,
                        error: Some(format!("{err:#}")),
                    },
                });
            }
            Ok(())
        })();
        let cancelled = job.cancelled();
        let _ = app.emit(
            "dict://done",
            DictImportDone {
                results,
                cancelled,
                error: outcome.err().map(|e| format!("{e:#}")),
            },
        );
        drop(guard);
    });
    Ok(())
}

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

/// 从 `text` 开头查词。还没导入过词典时返回空结果，不新建库文件。
pub fn lookup(state: &AppState, text: &str, mode: Option<&str>) -> Result<FindTermsResult> {
    let result = state.with_dictionaries(false, |store| {
        jp_dict::lookup::lookup(store, state.translator(), text, mode)
    })?;
    Ok(result.unwrap_or_default())
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
