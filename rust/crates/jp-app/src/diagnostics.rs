//! Tauri 只提供本机引擎能力，报告生成由共享 crate 负责。
use crate::state::AppState;
pub fn report(state: &AppState, version: &str, log_lines: usize) -> String {
    let root = state.project_root();
    let cache = state.pitch_cache();
    let usage = cache.usage();
    let cache_text = format!("{} 个文件，{} MB（上限 {} MB）", usage.files, usage.bytes / (1024 * 1024), cache.limit_bytes() / (1024 * 1024));
    jp_diagnostics::report(&state.corpus(), &root, &state.db_path, version, jp_diagnostics::Capabilities {
        system: std::env::consts::OS, audio: state.audio().is_some(),
        pitch: &jp_audio::pitch::engine_tag(jp_audio::pitch::ENGINE), pitch_cache: &cache_text,
    }, log_lines)
}
