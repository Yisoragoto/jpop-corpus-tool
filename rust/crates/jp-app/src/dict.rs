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

pub use jp_dict::legacy::{LegacySource, legacy_sources};

/// 从 `text` 开头查词。还没导入过词典时返回空结果，不新建库文件。
pub fn lookup(state: &AppState, text: &str, mode: Option<&str>) -> Result<FindTermsResult> {
    let result = state.with_dictionaries(false, |store| {
        jp_dict::lookup::lookup(store, state.translator(), text, mode)
    })?;
    Ok(result.unwrap_or_default())
}

