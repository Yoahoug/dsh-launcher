// dsh-launcher · Tauri commands(M1 为 mock 实现,M2 替换为 bridge 数据源,M3 增加桌面偏好)
use crate::contract::{
    ActionAccepted, AppSnapshot, DesktopPreferences, DesktopSnapshot, EnvironmentSnapshot, LogPage,
    SettingsSnapshot, UpdateResult,
};
use tauri::{AppHandle, State};

use crate::state::AppState;
use std::sync::Arc;

/// 快照:状态机全量(M1 mock → M2 bridge)。
#[tauri::command]
pub fn get_app_snapshot(state: State<'_, Arc<AppState>>) -> AppSnapshot {
    state.snapshot()
}

/// 执行后端动作。
#[tauri::command]
pub fn run_action(
    action: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> ActionAccepted {
    state.run_action(&app, &action)
}

/// 增量日志(本地 ring)。
#[tauri::command]
pub fn get_logs(since_id: u64, state: State<'_, Arc<AppState>>) -> LogPage {
    state.logs(since_id)
}

/// 清空日志 ring(仅本地,daemon 侧历史不受影响)。
#[tauri::command]
pub fn clear_logs(state: State<'_, Arc<AppState>>) {
    state.clear_logs();
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> SettingsSnapshot {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    patch: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<SettingsSnapshot, String> {
    state.save_settings(&patch)
}

#[tauri::command]
pub fn inspect_environment(state: State<'_, Arc<AppState>>) -> EnvironmentSnapshot {
    state.environment()
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UpdateResult, String> {
    Ok(state.check_for_update(&app).await)
}

#[tauri::command]
pub async fn apply_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ActionAccepted, String> {
    Ok(state.apply_update(&app).await)
}

/// 桌面信息:偏好 + 首次运行状态。
#[tauri::command]
pub fn get_desktop_snapshot(state: State<'_, Arc<AppState>>) -> DesktopSnapshot {
    state.desktop_snapshot()
}

/// 保存桌面偏好(Rust 持久化),并应用 autostart/托盘/主题副作用。
#[tauri::command]
pub fn save_preferences(
    app: AppHandle,
    preferences: DesktopPreferences,
    state: State<'_, Arc<AppState>>,
) -> Result<DesktopPreferences, String> {
    let saved = state.save_preferences(&preferences)?;
    crate::lifecycle::apply_preferences(&app);
    Ok(saved)
}

/// 危险动作:先弹原生确认框,确认后执行。stop-and-quit 走完整退出流程。
#[tauri::command]
pub fn confirm_and_run(
    app: AppHandle,
    action: String,
    state: State<'_, Arc<AppState>>,
) -> ActionAccepted {
    if action == "stop-and-quit" {
        if crate::lifecycle::confirm_stop_and_quit_blocking(&app) {
            crate::lifecycle::stop_and_quit(&app);
            return ActionAccepted {
                ok: true,
                reason: Some("已停止并退出".into()),
                aborted: None,
                already: None,
            };
        }
        return ActionAccepted {
            ok: false,
            reason: Some("已取消".into()),
            aborted: Some(true),
            already: None,
        };
    }
    let (title, message) = match action.as_str() {
        "stop" => ("停止 dsh", "确定要停止 dsh web 吗?"),
        "rebuild" => ("重建并重启", "确定要重建并重启 dsh 吗?服务将短暂停止。"),
        _ => return state.run_action(&app, &action),
    };
    if !crate::lifecycle::confirm_blocking(&app, title, message) {
        return ActionAccepted {
            ok: false,
            reason: Some("已取消".into()),
            aborted: Some(true),
            already: None,
        };
    }
    state.run_action(&app, &action)
}

/// 普通退出:仅 detach,不停止 dsh。
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::lifecycle::quit_launcher(&app);
}

/// 原生目录选择器(First-run / Settings 选择仓库)。
#[tauri::command]
pub fn pick_directory(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog();
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    dialog
        .file()
        .set_title("选择仓库目录")
        .pick_folder(move |res| {
            let _ = tx.send(res.map(|p| p.to_string()));
        });
    rx.recv_timeout(std::time::Duration::from_secs(300))
        .ok()
        .flatten()
}

#[tauri::command]
pub fn open_dsh(_app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.open_dsh()
}

#[tauri::command]
pub fn open_repo_directory(_app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.open_repo_directory()
}

#[tauri::command]
pub fn open_log_directory(_app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.open_log_directory()
}
