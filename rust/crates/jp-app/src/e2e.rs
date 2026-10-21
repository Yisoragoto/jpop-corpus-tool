//! 原生文件对话框不能脚本化，测试入口必须由进程环境明确开启，普通运行连命令都不注册。
use crate::{
    commands::{CommandError, ScanResult},
    state::AppState,
};
use tauri::State;

pub fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

#[tauri::command(async)]
pub fn e2e_state(state: State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    Ok(
        serde_json::json!({"enabled":true,"trackCount":state.corpus().overview()?.tracks,"libraryPath":state.db_path.parent(),"buildId":crate::update::BUILD_ID}),
    )
}

#[tauri::command(async)]
pub fn e2e_scan_folder(
    state: State<'_, AppState>,
    path: String,
) -> Result<ScanResult, CommandError> {
    crate::commands::scan_folder(state, path, None)
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_exact_e2e_flag_registers_hooks() {
        for value in [None, Some(""), Some("0"), Some("true"), Some("01")] {
            assert!(!super::enabled(value));
        }
        assert!(super::enabled(Some("1")));
    }
}
