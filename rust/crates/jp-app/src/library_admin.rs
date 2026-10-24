//! 仅保留 Tauri 播放器与会话锁适配。
use crate::state::AppState;
use anyhow::Result;
use jp_import::maintain::{EditReport, MissingAudio, RelinkSuggestion, SongEdit};
pub use jp_library::admin::{DeleteResult, Maintained, RelinkBatch, RelinkRequest};
use std::path::Path;
fn with_library<T>(
    state: &AppState,
    f: impl FnOnce(&mut jp_library::admin::Library<'_>) -> Result<T>,
) -> Result<T> {
    let mut corpus = state.corpus();
    f(&mut jp_library::admin::Library {
        corpus: &mut corpus,
        db_path: &state.db_path,
        covers_dir: &state.covers_dir,
        pitch_cache: &state.pitch_cache().dir,
    })
}
pub fn edit_song(state: &AppState, id: &str, edit: &SongEdit) -> Result<Maintained<EditReport>> {
    with_library(state, |library| {
        jp_library::admin::edit_song(library, id, edit)
    })
}
pub fn delete_song(state: &AppState, id: &str) -> Result<Maintained<DeleteResult>> {
    if let Some(engine) = state.audio()
        && engine.state().song_id == id
    {
        state.pitch().stop();
        engine.stop();
    }
    state.flush_session();
    with_library(state, |library| jp_library::admin::delete_song(library, id))
}
pub fn missing_audio(state: &AppState) -> Result<Vec<MissingAudio>> {
    with_library(state, |library| jp_library::admin::missing_audio(library))
}
pub fn suggest_relinks(state: &AppState, folder: &Path) -> Result<Vec<RelinkSuggestion>> {
    let mut corpus = jp_corpus::Corpus::open(&state.db_path)?;
    jp_library::admin::suggest_relinks(
        &jp_library::admin::Library {
            corpus: &mut corpus,
            db_path: &state.db_path,
            covers_dir: &state.covers_dir,
            pitch_cache: &state.pitch_cache().dir,
        },
        folder,
    )
}
pub fn relink_audio(state: &AppState, links: &[RelinkRequest]) -> Result<RelinkBatch> {
    with_library(state, |library| {
        jp_library::admin::relink_audio(library, links)
    })
}
pub fn sync_imported(state: &AppState, report: &jp_import::ImportReport) -> String {
    with_library(state, |library| {
        Ok(jp_library::admin::sync_imported(library, report))
    })
    .unwrap_or_else(|e| e.to_string())
}
