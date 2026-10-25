//! Anki 导出的后台作业。
//!
//! 和刮削那边一样的形状：主循环独占数据库连接，进度走事件，可中断。
//! 但**不共用作业标志**——刮削排的是外部服务的限流队列，
//! Anki 是本机，两件事同时做没有冲突。

use std::sync::Arc;

use anyhow::Result;
use jp_anki::{AnkiConnect, DupScope, ExportOptions, RefreshScope};
use tauri::{AppHandle, Emitter, Runtime};

/// 生产用的 AnkiConnect 客户端。
pub fn client() -> AnkiConnect {
    AnkiConnect::local()
}

pub use jp_anki::batch::{ExportItem, ExportProgress};
pub type ExportJob = crate::job::Job;

/// 后台导一批词。
pub fn spawn_export<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    items: Vec<ExportItem>,
    options: ExportOptions,
) -> Result<()> {
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    job: "export".into(),
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个导出作业在跑"))?;
    std::thread::spawn(move || {
        let progress = jp_anki::batch::run_export(
            &db_path,
            &client(),
            &items,
            &options,
            || job.cancelled(),
            |p| {
                let _ = app.emit("anki://progress", p);
            },
        );
        drop(guard);
        let _ = app.emit("anki://progress", progress);
    });
    Ok(())
}

/// 后台更新选中的词：只重查读音、释义、JLPT、音高、词频、词性，不动例句、音频和出处。
pub fn spawn_update<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    items: Vec<ExportItem>,
    deck: String,
    scope: DupScope,
) -> Result<()> {
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    job: "update".into(),
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个 Anki 作业在跑"))?;
    std::thread::spawn(move || {
        let progress = jp_anki::batch::run_update(
            &db_path,
            &client(),
            &items,
            &deck,
            scope,
            || job.cancelled(),
            |p| {
                let _ = app.emit("anki://progress", p);
            },
        );
        drop(guard);
        let _ = app.emit("anki://progress", progress);
    });
    Ok(())
}

/// 刷新之前先报个数：范围内有几张 JPOP Corpus 旧卡。只读。
pub fn refresh_preview(deck: &str, scope: RefreshScope) -> Result<usize> {
    let anki = client();
    jp_anki::refresh_targets(&anki, deck, scope)
        .map(|targets| targets.len())
        .map_err(|err| anyhow::anyhow!("{}", err.advice()))
}

/// 后台刷新旧牌组：范围内每张 JPOP Corpus 卡重查词典字段，不动例句、音频和出处。
pub fn spawn_refresh<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    deck: String,
    scope: RefreshScope,
) -> Result<()> {
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    job: "refresh".into(),
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个 Anki 作业在跑"))?;
    std::thread::spawn(move || {
        let progress = jp_anki::batch::run_refresh(
            &db_path,
            &client(),
            &deck,
            scope,
            || job.cancelled(),
            |p| {
                let _ = app.emit("anki://progress", p);
            },
        );
        drop(guard);
        let _ = app.emit("anki://progress", progress);
    });
    Ok(())
}

pub use jp_anki::service::{AnkiStatus, describe, options};
pub fn status() -> AnkiStatus {
    jp_anki::service::status(&client())
}
