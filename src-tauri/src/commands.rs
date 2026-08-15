// dsh-launcher · Tauri commands(M1 为 mock 实现,M2 替换为 bridge 数据源,M3 增加桌面偏好)
use crate::contract::{
    ActionAccepted, AppSnapshot, DesktopPreferences, DesktopSnapshot, EnvironmentSnapshot, LogPage,
    SettingsSnapshot, UpdateResult,
};
use tauri::{AppHandle, Manager, State};

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

/// 环境检测(带文件缓存)。force=true 强制重新探测(「重新检测」按钮);
/// 默认读缓存(成功即成功,秒开)。async:避免探测子进程阻塞主线程/其它 IPC。
#[tauri::command]
pub async fn inspect_environment(app: AppHandle, force: Option<bool>) -> EnvironmentSnapshot {
    let state = app.state::<Arc<AppState>>().inner().clone();
    state.environment(force.unwrap_or(false))
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

/// 完成/跳过首次运行引导(skip=true 跳过;提供 repo_path 时一并保存)。
/// 广播 state-changed 使 renderer 的 desktop snapshot 立即刷新并退出向导。
#[tauri::command]
pub fn complete_first_run(
    app: AppHandle,
    skip: bool,
    repo_path: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<DesktopSnapshot, String> {
    state.complete_first_run(&app, skip, repo_path)
}

/// 主窗口顶部栏(启动器 chrome)是否隐藏;隐藏时 DeepSeek 子 WebView 占满全窗。
/// 仅由 renderer 在全屏 + DeepSeek 工作区自动隐藏时调用。
#[tauri::command]
pub fn set_topbar_hidden(app: AppHandle, hidden: bool) {
    crate::dsh_view::set_topbar_hidden(&app, hidden);
}

/// 光标相对主窗口客户区的位置(逻辑坐标)。renderer 全屏自动隐藏顶部栏时轮询。
/// 返回 None 表示无法获取(窗口不存在等),调用方保持当前隐藏状态即可。
#[tauri::command]
pub fn get_cursor_position(app: AppHandle) -> Option<(f64, f64)> {
    use tauri::Manager;
    let window = app.get_window("main")?;
    let pos = window.cursor_position().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let scale = if scale > 0.0 { scale } else { 1.0 };
    Some((pos.x / scale, pos.y / scale))
}

/// 危险动作:先弹原生确认框,确认后执行。stop-and-quit 走完整退出流程。
#[tauri::command]
pub async fn confirm_and_run(app: AppHandle, action: String) -> Result<ActionAccepted, String> {
    // 异步 command 不能跨 await 持有带请求生命周期的 State<'_>，
    // 从 AppHandle 获取独立 Arc 后再打开原生对话框。
    let state = app.state::<Arc<AppState>>().inner().clone();
    if action == "stop-and-quit" {
        if crate::lifecycle::confirm_stop_and_quit(&app).await {
            crate::lifecycle::stop_and_quit(&app);
            return Ok(ActionAccepted {
                ok: true,
                reason: Some("已停止并退出".into()),
                aborted: None,
                already: None,
            });
        }
        return Ok(ActionAccepted {
            ok: false,
            reason: Some("已取消".into()),
            aborted: Some(true),
            already: None,
        });
    }
    let (title, message) = match action.as_str() {
        "stop" => ("停止 dsh", "确定要停止 dsh web 吗?"),
        "rebuild" => ("重建并重启", "确定要重建并重启 dsh 吗?服务将短暂停止。"),
        _ => return Ok(state.run_action(&app, &action)),
    };
    if !crate::lifecycle::confirm(&app, title, message).await {
        return Ok(ActionAccepted {
            ok: false,
            reason: Some("已取消".into()),
            aborted: Some(true),
            already: None,
        });
    }
    Ok(state.run_action(&app, &action))
}

/// 普通退出:仅 detach,不停止 dsh。
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::lifecycle::quit_launcher(&app);
}

/// 原生目录选择器(First-run / Settings 选择仓库)。
#[tauri::command]
pub async fn pick_directory(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog();
    let (tx, mut rx) = tauri::async_runtime::channel::<Option<String>>(1);
    dialog
        .file()
        .set_title("选择仓库目录")
        .pick_folder(move |res| {
            let _ = tx.try_send(res.map(|p| p.to_string()));
        });
    rx.recv().await.flatten()
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

// ── M0/M1:性能测量、Clone 弹窗、托管工具链快照 ──────────

/// renderer 上报测量点(如 react_interactive),记录后广播完整指标。
#[tauri::command]
pub fn perf_mark(name: String, app: AppHandle, state: State<'_, Arc<AppState>>) {
    state.timings.mark(&name);
    state.emit_perf(&app);
}

#[tauri::command]
pub fn get_perf_metrics(state: State<'_, Arc<AppState>>) -> Vec<crate::perf::PerfMark> {
    state.timings.snapshot()
}

/// Clone 弹窗初始数据:上次成功地址(默认填充)+ 默认目标目录(放置位置)。
/// async:保证弹窗数据秒开,不参与任何阻塞探测。
#[tauri::command]
pub async fn open_clone_dialog(app: AppHandle) -> crate::clone::CloneDialogData {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let settings = state.settings();
    crate::clone::CloneDialogData {
        last_good_url: crate::clone::last_good_url(),
        default_target: crate::clone::default_target_dir(&settings.repo_path),
        official_url: "https://github.com/deepseek-ai/deepseek-harness.git".to_string(),
    }
}

/// Clone 状态(上次成功地址等)。
#[tauri::command]
pub fn get_clone_state() -> crate::clone::CloneState {
    crate::clone::load_clone_state()
}

/// 提交克隆请求:校验 URL 后保存为 pending_clone,并启动 clone-repo / full-setup 操作。
#[tauri::command]
pub fn submit_clone_request(
    app: AppHandle,
    request: crate::clone::CloneRequest,
    full: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::ActionAccepted, String> {
    // 校验(不通过不进入流程;非法输入绝不覆盖 last-good)
    crate::clone::validate_url(&request.url).map_err(|e| format!("克隆地址无效:{e}"))?;
    if request.target_dir.trim().is_empty() {
        return Err("目标目录不能为空".into());
    }
    *state.pending_clone.lock().unwrap() = Some(request);
    let action = if full { "full-setup" } else { "clone-repo" };
    Ok(state.run_action(&app, action))
}

/// 托管工具链安装快照(设置页展示来源/版本)。
#[tauri::command]
pub fn get_installation_snapshot(
    state: State<'_, Arc<AppState>>,
) -> crate::ops::InstallationSnapshot {
    state.installation()
}

// ── M3:独立 chat WebView(零权限) ─────────────────────────

/// 打开(或召回)内嵌 DSH chat 窗口:服务未就绪时先启动并异步等待。
#[tauri::command]
pub fn open_chat(app: AppHandle) -> Result<crate::chat::ChatStateSnapshot, String> {
    crate::chat::open_chat(&app)
}

/// 关闭 chat 窗口(销毁;服务继续运行)。
#[tauri::command]
pub fn close_chat(app: AppHandle) {
    crate::chat::close_chat(&app);
}

/// 当前 chat 窗口状态(事件 app://chat-state 之外的一次性查询)。
#[tauri::command]
pub fn get_chat_state(app: AppHandle) -> crate::chat::ChatStateSnapshot {
    app.state::<Arc<crate::chat::ChatManager>>().current_state()
}

// ── M4.1:主窗口内 DeepSeek 工作区(dsh-content 子 WebView) ──

/// 打开 DeepSeek 工作区:服务未就绪时先启动并异步等待,就绪后在当前主窗口切换。
#[tauri::command]
pub fn open_dsh_workspace(app: AppHandle) -> crate::contract::DshViewSnapshot {
    crate::dsh_view::open_dsh_workspace(&app)
}

/// 返回启动器工作区(子 WebView 隐藏,会话保持,不销毁不刷新)。
#[tauri::command]
pub fn back_to_launcher(app: AppHandle) -> crate::contract::DshViewSnapshot {
    crate::dsh_view::back_to_launcher(&app)
}

/// 重试/重连 DeepSeek 工作区(断线/失败后)。
#[tauri::command]
pub fn retry_dsh_view(app: AppHandle) -> crate::contract::DshViewSnapshot {
    crate::dsh_view::retry_dsh_view(&app)
}

/// 工作区切换(launcher|dsh);幂等,连续点击不重复创建。
#[tauri::command]
pub fn set_workspace(
    app: AppHandle,
    workspace: crate::contract::Workspace,
) -> crate::contract::DshViewSnapshot {
    match workspace {
        crate::contract::Workspace::Launcher => crate::dsh_view::back_to_launcher(&app),
        crate::contract::Workspace::Dsh => crate::dsh_view::open_dsh_workspace(&app),
    }
}

/// 当前 DeepSeek 工作区/子 WebView 状态(事件 app://dsh-view-state 之外的一次性查询)。
#[tauri::command]
pub fn get_dsh_view_state(app: AppHandle) -> crate::contract::DshViewSnapshot {
    app.state::<Arc<crate::dsh_view::DshViewManager>>()
        .current_state()
}
