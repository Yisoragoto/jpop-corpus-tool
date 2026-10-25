//! 歌词写入与版本管理由共享核心实现。
pub use jp_lyrics::lyrics::*;

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

/// 保留旧作业入口；这里只把共享进度转成事件。
pub fn spawn_fill<R: Runtime>(
    app: AppHandle<R>, job: Arc<LyricsJob>, db_path: PathBuf,
    lyrics_dir: PathBuf, song_ids: Option<Vec<String>>,
) -> anyhow::Result<()> {
    jp_lyrics::lyrics::spawn_fill(move |p| { let _ = app.emit("lyrics://progress", p); }, job, db_path, lyrics_dir, song_ids)
}
