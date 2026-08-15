// dsh-launcher · Tauri commands(M1 为 mock 实现,M2 替换为 bridge 数据源)
use crate::contract::{
    ActionAccepted, AppSnapshot, EnvironmentSnapshot, LogPage, SettingsSnapshot, UpdateResult,
};
use tauri::{AppHandle, State};

use crate::state::AppState;

/// 快照:状态机全量(M1 mock → M2 bridge)。
#[tauri::command]
pub fn get_app_snapshot(state: State<AppState>) -> AppSnapshot {
    state.snapshot()
}

/// 执行后端动作。
#[tauri::command]
pub fn run_action(action: String, state: State<AppState>) -> ActionAccepted {
    state.run_action(&action)
}

/// 增量日志。
#[tauri::command]
pub fn get_logs(since_id: u64, state: State<AppState>) -> LogPage {
    state.logs(since_id)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> SettingsSnapshot {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    patch: serde_json::Value,
    state: State<AppState>,
) -> Result<SettingsSnapshot, String> {
    state.save_settings(&patch)
}

#[tauri::command]
pub fn inspect_environment(state: State<AppState>) -> EnvironmentSnapshot {
    state.environment()
}

#[tauri::command]
pub fn check_for_update(state: State<AppState>) -> UpdateResult {
    state.check_for_update()
}

#[tauri::command]
pub fn apply_update(state: State<AppState>) -> ActionAccepted {
    state.apply_update()
}

#[tauri::command]
pub fn open_dsh(_app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.open_dsh()
}

#[tauri::command]
pub fn open_repo_directory(_app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.open_repo_directory()
}

#[tauri::command]
pub fn open_log_directory(_app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.open_log_directory()
}
