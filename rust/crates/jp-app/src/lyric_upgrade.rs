//! Tauri 只适配事件；队列、取消与写库保护共用 jp-lyrics。
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
pub use jp_lyrics::lyric_upgrade::{Batch, Progress, fetch_one};

fn notify<R: Runtime>(app: &AppHandle<R>, p: &Progress) {
    let _ = app.emit("lyrics-upgrade://progress", p);
    // Keep existing Settings/Scrape progress consumers on the same queue.
    let _ = app.emit(
        "lyrics://progress",
        crate::lyrics::LyricsProgress {
            done: p.done,
            total: p.total,
            title: p.title.clone(),
            filled: p.upgraded + p.line_filled,
            not_found: p.not_found,
            failed: p.failed,
            finished: !p.running,
            cancelled: p.cancelled,
            error: p.error.clone(),
            message: p.error.clone(),
            ..Default::default()
        },
    );
}

pub fn enqueue<R: Runtime>(
    app: AppHandle<R>, batch: Arc<Batch>, db_path: PathBuf,
    lyrics_dir: PathBuf, ids: Vec<String>, options: jp_scraper::timed_sources::SourceOptions,
) -> anyhow::Result<()> {
    jp_lyrics::lyric_upgrade::enqueue(move |p| notify(&app, p), batch, db_path, lyrics_dir, ids, options)
}
