//! 刮削：逻辑在 `jp-scrape`（macOS 客户端经 `jp-ffi` 用同一份），这里只管后台线程、事件和应用状态。
//!
//! 进度通过 `scrape://progress` 事件推给前端，取消靠一个原子标志。

use std::sync::Arc;

use anyhow::Result;
use tauri::{AppHandle, Emitter, Runtime};

pub use jp_scrape::*;

/// 批量刮削作业的运行/取消状态。四个后台作业共用 `crate::job::Job`——
/// 原来这里自己写了一份，带着「第二次启动会抹掉取消」和
/// 「panic 之后界面永久卡住」两个毛病。
///
/// 和 Anki、词典、补齐歌词**各自独立**：刮削排的是外部服务的限流队列，
/// 两个批量刮削同时跑会把额度用光，别的作业和它没冲突。
pub type ScrapeJob = crate::job::Job;

/// 把 jp-scrape 的日志接到应用的日志文件
pub fn connect_log() {
    jp_scrape::set_logger(|message| crate::log::warn(message));
}

/// 刚导入的歌里，文件内嵌了封面的，取出来记进库。返回取到了几张。
///
/// 导入本身一首歌一个事务、已经提交了，封面放在后面单独做：
/// 一张图写不下去不该让这首歌算导入失败。
pub fn save_imported_covers(state: &crate::state::AppState, report: &jp_import::ImportReport) -> usize {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let mut saved = 0;
    for track in &report.tracks {
        if !matches!(track.outcome, jp_import::Outcome::Imported { .. }) {
            continue;
        }
        let Ok(artist) = conn.query_row(
            "SELECT artist FROM songs WHERE id=?1",
            rusqlite::params![track.song_id],
            |r| r.get::<_, String>(0),
        ) else {
            continue;
        };
        let audio = std::path::Path::new(&track.path);
        if let Some(path) = save_embedded_cover(&state.covers_dir, &track.song_id, &artist, audio) {
            match record_cover(conn, &track.song_id, &path) {
                Ok(()) => saved += 1,
                Err(err) => crate::log::warn(format!("封面记不进库 {}: {err:#}", track.song_id)),
            }
        }
    }
    saved
}

/// 在后台线程刮一批。立刻返回，进度走 `scrape://progress` 事件。
pub fn spawn_batch<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    db_path: std::path::PathBuf,
    covers_dir: std::path::PathBuf,
    song_ids: Vec<String>,
    sources: jp_scraper::timed_sources::SourceOptions,
) -> Result<()> {
    spawn(app, job, "track", move |cancelled, progress| {
        jp_scrape::run_batch(&db_path, &covers_dir, &song_ids, apple_source(&sources), cancelled, progress)
    }, "刮削作业中断")
}

/// 设置里开了 Apple Music 而且填了开发者令牌时，给 resolver 的（地区, 令牌）
pub fn apple_source(options: &jp_scraper::timed_sources::SourceOptions) -> Option<(&str, &str)> {
    (options.apple && !options.apple_developer_token.is_empty())
        .then_some((options.apple_storefront.as_str(), options.apple_developer_token.as_str()))
}

/// 后台给已经识别过的歌补专辑信息（发行类型、曲目数）。和刮削共用作业标志
pub fn spawn_album_fill<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    db_path: std::path::PathBuf,
    targets: Vec<(i64, String)>,
) -> Result<()> {
    spawn(app, job, "album", move |cancelled, progress| {
        jp_scrape::run_album_fill(&db_path, targets, cancelled, progress)
    }, "补专辑信息中断")
}

/// 后台刮一批歌手。和曲目共用同一个作业标志——两边都要排 MusicBrainz
/// 的队，同时跑只会互相拖慢。
pub fn spawn_artist_batch<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    db_path: std::path::PathBuf,
    artists_dir: std::path::PathBuf,
    names: Vec<String>,
) -> Result<()> {
    spawn(app, job, "artist", move |cancelled, progress| {
        jp_scrape::run_artist_batch(&db_path, &artists_dir, &names, cancelled, progress)
    }, "歌手刮削作业中断")
}

/// 两种批量作业的外壳：占住作业标志、起线程、把进度发成事件，最后发一条终态。
/// 整批断在半路时终态里带上原因，不能只是悄悄「完成」
fn spawn<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    kind: &'static str,
    work: impl FnOnce(&dyn Fn() -> bool, &mut dyn FnMut(ScrapeProgress)) -> Result<bool> + Send + 'static,
    failure: &'static str,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "scrape://progress",
                ScrapeProgress {
                    kind: kind.into(),
                    finished: true,
                    error: "刮削的后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个刮削作业在跑"))?;

    std::thread::spawn(move || {
        let mut total = 0;
        let outcome = work(&|| job.cancelled(), &mut |p: ScrapeProgress| {
            total = p.total;
            let _ = app.emit("scrape://progress", p);
        });
        let (cancelled, error) = match outcome {
            Ok(cancelled) => (cancelled, String::new()),
            Err(err) => {
                crate::log::error(format!("{failure}: {err:#}"));
                (false, format!("{err:#}"))
            }
        };
        drop(guard);
        let _ = app.emit(
            "scrape://progress",
            ScrapeProgress {
                kind: kind.into(),
                done: total,
                total,
                finished: true,
                cancelled,
                error,
                ..Default::default()
            },
        );
    });
    Ok(())
}
