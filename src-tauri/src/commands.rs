// dsh-launcher · Tauri commands(M1 为 mock 实现,M2 替换为 bridge 数据源,M3 增加桌面偏好)
use crate::contract::{
    ActionAccepted, AppSnapshot, DesktopPreferences, DesktopSnapshot, EnvironmentSnapshot, LogPage,
    SettingsSnapshot, UpdateResult,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;
use std::sync::Arc;

const DSH_PLUGINS_REPO_URL: &str = "https://github.com/Yoahoug/dsh-plugins.git";

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

#[tauri::command]
pub fn restart_app(app: AppHandle, state: State<'_, Arc<AppState>>) {
    state.restart_app(&app);
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

// ── M5:插件管理子界面 ─────────────────────────────────────

/// 组装插件写操作上下文(设置 + 工具)。
fn plugin_write_ctx(state: &Arc<AppState>) -> (crate::services::plugins::WriteCtx, String) {
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    (
        crate::services::plugins::WriteCtx {
            tools,
            repo_path: settings.repo_path.clone(),
            dsh_home_setting: settings.dsh_home.clone(),
        },
        settings.profile_name,
    )
}

/// dsh-plugins 路径:设置项 → 自动探测 profile deps 的 file: 链接 → 空。
fn resolve_plugins_path(settings: &SettingsSnapshot) -> String {
    if !settings.dsh_plugins_path.is_empty() {
        return settings.dsh_plugins_path.clone();
    }
    let home = crate::services::plugins::dsh_home_dir(&settings.dsh_home);
    let profiles = crate::services::plugins::profiles(&home);
    crate::services::plugins::detect_plugins_path_from_home(&profiles, &home).unwrap_or_default()
}

/// 插件组合视图快照(profiles + 行 + dsh-plugins 包)。
/// async:内部跑 dump-config(~1-3s),避免阻塞主线程/其它 IPC。
#[tauri::command]
pub async fn plugins_get_snapshot(
    profile: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PluginsSnapshot, String> {
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    let profile_name = profile.unwrap_or(settings.profile_name.clone());
    let dsh_plugins_path = resolve_plugins_path(&settings);
    Ok(crate::services::plugins::snapshot(
        &tools,
        &settings.repo_path,
        &settings.dsh_home,
        &profile_name,
        &dsh_plugins_path,
    ))
}

fn nonempty_profile(requested: &str, default: &str) -> String {
    if requested.is_empty() {
        default.to_string()
    } else {
        requested.to_string()
    }
}

/// 插件启停(写 profile patch + 备份 + dump-config 校验 + 回滚)。
#[tauri::command]
pub async fn plugins_set_enabled(
    profile: String,
    id: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    crate::services::plugins::set_enabled(&state.log_hub, &ctx, &profile, &id, enabled)
}

/// 保存配置:config(整行全量键)或 raw_yaml(原始 YAML 块,含 !!js 行)。
#[tauri::command]
pub async fn plugins_save_config(
    profile: String,
    id: String,
    config: serde_json::Value,
    raw_yaml: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    crate::services::plugins::save_config(
        &state.log_hub,
        &ctx,
        &profile,
        &id,
        &config,
        raw_yaml.as_deref(),
    )
}

/// 重置行(删除 profile patch 中该 id 条目,回落 bundle/home 默认)。
#[tauri::command]
pub async fn plugins_reset_row(
    profile: String,
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    crate::services::plugins::reset_row(&state.log_hub, &ctx, &profile, &id)
}

/// 仅校验补丁(不写文件)。
#[tauri::command]
pub async fn plugins_validate_patch(
    profile: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    Ok(crate::services::plugins::validate_patch(&ctx, &profile))
}

/// 原始 dump-config 文本(预览补丁效果/校验)。
#[tauri::command]
pub async fn dshctl_dump_config(
    profile: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    let profile = profile.unwrap_or(settings.profile_name);
    crate::services::dshctl::run_capture(
        &tools,
        &settings.repo_path,
        &settings.dsh_home,
        &[
            "--profile".to_string(),
            profile,
            "--dump-config".to_string(),
        ],
        crate::services::dshctl::CAPTURE_TIMEOUT,
    )
}

/// 打开 dsh-plugins 包目录(系统文件管理器)。
#[tauri::command]
pub fn plugins_open_in_explorer(abs_dir: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(std::path::Path::new(&abs_dir), None::<&str>)
        .map_err(|e| format!("打开目录失败:{e}"))
}

/// 长任务 worker 收尾(与 run_action 相同模式)。
fn finish_plugin_op(
    state: &Arc<AppState>,
    app: &AppHandle,
    id: u64,
    result: Result<(), crate::ops::OperationError>,
) {
    match result {
        Ok(()) => state
            .ops
            .finish(id, crate::contract::OperationStatus::Success, None),
        Err(crate::ops::OperationError::Cancelled) => state.ops.finish(
            id,
            crate::contract::OperationStatus::Cancelled,
            Some("已取消".into()),
        ),
        Err(crate::ops::OperationError::Failed(e)) => {
            state
                .ops
                .finish(id, crate::contract::OperationStatus::Failed, Some(e))
        }
    }
    state.set_snapshot(app, |_| {});
}

/// 从 dsh-plugins 安装包:包目录 pnpm install + build → dsh plugin add file:<abs>。
#[tauri::command]
pub fn plugins_install_package(
    app: AppHandle,
    profile: String,
    abs_dir: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ActionAccepted, String> {
    let state = state.inner().clone();
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    let (id, token) = state.ops.begin(
        crate::contract::OperationKind::PluginInstall,
        true,
        "准备安装插件…",
    )?;
    state.set_snapshot(&app, |_| {});
    let app2 = app.clone();
    let state2 = state.clone();
    std::thread::spawn(move || {
        let result = install_package_flow(&state2, &profile, &abs_dir, &settings, &tools, &token);
        finish_plugin_op(&state2, &app2, id, result);
    });
    Ok(ActionAccepted {
        ok: true,
        reason: None,
        aborted: None,
        already: None,
    })
}

fn install_package_flow(
    state: &Arc<AppState>,
    profile: &str,
    abs_dir: &str,
    settings: &SettingsSnapshot,
    tools: &crate::services::runtime::Tools,
    token: &crate::ops::CancellationToken,
) -> Result<(), crate::ops::OperationError> {
    token.check()?;
    let pkg = std::path::Path::new(abs_dir);
    let pj = pkg.join("package.json");
    if !pj.is_file() {
        return Err(crate::ops::OperationError::Failed(format!(
            "不是有效的插件包目录(缺少 package.json):{abs_dir}"
        )));
    }
    state.log_hub.append(
        "launcher",
        crate::contract::LogLevel::Info,
        &format!("安装插件包:{abs_dir} → profile '{profile}'"),
    );
    let extra_env = crate::services::build::registry_env(tools);
    // 1. pnpm install(包目录)
    state.ops.set_stage(
        state.ops.current().map(|o| o.operation_id).unwrap_or(0),
        "安装包依赖(pnpm install)…",
        Some(20),
    );
    let (code, _tail) = crate::services::build::run_pnpm(
        &state.log_hub,
        tools,
        abs_dir,
        &["install"],
        "插件包 pnpm install",
        None,
        &extra_env,
        token,
    )
    .map_err(crate::ops::OperationError::Failed)?;
    token.check()?;
    if code != 0 {
        return Err(crate::ops::OperationError::Failed(
            "插件包依赖安装失败(pnpm install 退出码非 0)".into(),
        ));
    }
    // 2. pnpm run build(包声明了 build 脚本才执行;产物 lib/)
    let has_build = std::fs::read_to_string(&pj)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.pointer("/scripts/build")
                .and_then(|b| b.as_str())
                .map(|s| s.to_string())
        })
        .is_some();
    if has_build {
        state.ops.set_stage(
            state.ops.current().map(|o| o.operation_id).unwrap_or(0),
            "构建插件包(pnpm run build)…",
            Some(50),
        );
        let (code2, _tail2) = crate::services::build::run_pnpm(
            &state.log_hub,
            tools,
            abs_dir,
            &["run", "build"],
            "插件包 pnpm run build",
            None,
            &extra_env,
            token,
        )
        .map_err(crate::ops::OperationError::Failed)?;
        token.check()?;
        if code2 != 0 {
            return Err(crate::ops::OperationError::Failed(
                "插件包构建失败(pnpm run build 退出码非 0)".into(),
            ));
        }
    }
    // 3. dsh plugin --profile <p> add file:<abs>
    state.ops.set_stage(
        state.ops.current().map(|o| o.operation_id).unwrap_or(0),
        "dsh plugin add…",
        Some(80),
    );
    crate::services::dshctl::run_stream(
        &state.log_hub,
        tools,
        &settings.repo_path,
        &settings.dsh_home,
        &[
            "plugin".to_string(),
            "--profile".to_string(),
            profile.to_string(),
            "add".to_string(),
            format!("file:{abs_dir}"),
        ],
        token,
    )
    .map_err(crate::ops::OperationError::Failed)
}

/// 同步本地 dsh-plugins 仓库,必要时从 GitHub 克隆,然后安装全部 packages/*。
#[tauri::command]
pub fn plugins_install_all(
    app: AppHandle,
    profile: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ActionAccepted, String> {
    let state = state.inner().clone();
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    let (id, token) = state.ops.begin(
        crate::contract::OperationKind::PluginInstall,
        true,
        "准备同步 dsh-plugins…",
    )?;
    state.set_snapshot(&app, |_| {});
    let app2 = app.clone();
    let state2 = state.clone();
    std::thread::spawn(move || {
        let result = install_all_plugins_flow(&state2, &profile, &settings, &tools, &token);
        finish_plugin_op(&state2, &app2, id, result);
    });
    Ok(ActionAccepted {
        ok: true,
        reason: None,
        aborted: None,
        already: None,
    })
}

fn install_all_plugins_flow(
    state: &Arc<AppState>,
    profile: &str,
    settings: &SettingsSnapshot,
    tools: &crate::services::runtime::Tools,
    token: &crate::ops::CancellationToken,
) -> Result<(), crate::ops::OperationError> {
    token.check()?;
    let home = crate::services::plugins::dsh_home_dir(&settings.dsh_home);
    let profiles = crate::services::plugins::profiles(&home);
    let detected = crate::services::plugins::detect_plugins_path_from_home(&profiles, &home);
    let root = if !settings.dsh_plugins_path.is_empty() {
        std::path::PathBuf::from(&settings.dsh_plugins_path)
    } else if let Some(path) = detected {
        std::path::PathBuf::from(path)
    } else {
        home.join("dsh-plugins")
    };
    let git = tools.git.clone().ok_or_else(|| {
        crate::ops::OperationError::Failed("未找到 git,无法拉取 dsh-plugins".into())
    })?;
    let cancel = token.arc_flag();
    let mut git_env = tools.env();
    git_env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    git_env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
    let parent = root.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| crate::ops::OperationError::Failed(format!("创建插件仓库父目录失败:{e}")))?;

    state.ops.set_stage(
        state.ops.current().map(|o| o.operation_id).unwrap_or(0),
        "同步 dsh-plugins 仓库…",
        Some(10),
    );
    let (code, tail) = if root.join(".git").is_dir() {
        let root_s = root.to_string_lossy().to_string();
        crate::clone::run_cancellable(
            &state.log_hub,
            "git pull dsh-plugins",
            &git,
            &["-C", &root_s, "pull", "--ff-only"],
            parent,
            &git_env,
            cancel.as_ref(),
            None,
        )?
    } else {
        if root.exists()
            && std::fs::read_dir(&root)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
        {
            return Err(crate::ops::OperationError::Failed(format!(
                "插件仓库目录非空且不是 git 仓库:{}",
                root.display()
            )));
        }
        let root_s = root.to_string_lossy().to_string();
        crate::clone::run_cancellable(
            &state.log_hub,
            "git clone dsh-plugins",
            &git,
            &["clone", DSH_PLUGINS_REPO_URL, &root_s],
            parent,
            &git_env,
            cancel.as_ref(),
            None,
        )?
    };
    token.check()?;
    if code != 0 {
        return Err(crate::ops::OperationError::Failed(format!(
            "同步 dsh-plugins 失败:{}",
            tail.last()
                .cloned()
                .unwrap_or_else(|| "git 退出码非 0".into())
        )));
    }

    if settings.dsh_plugins_path.is_empty() {
        let _ = crate::config::apply_patch(&serde_json::json!({
            "dshPluginsPath": root.to_string_lossy().to_string()
        }));
    }
    let profile_summaries = crate::services::plugins::profiles(&home);
    let packages =
        crate::services::plugins::scan_packages(&root.to_string_lossy(), &profile_summaries);
    if packages.is_empty() {
        return Err(crate::ops::OperationError::Failed(format!(
            "仓库中没有可安装的 packages/*:{}",
            root.display()
        )));
    }
    for (index, package) in packages.iter().enumerate() {
        token.check()?;
        let progress = 20 + ((index as u8) * 70 / packages.len().max(1) as u8);
        state.ops.set_stage(
            state.ops.current().map(|o| o.operation_id).unwrap_or(0),
            &format!("安装插件 {}…", package.name),
            Some(progress),
        );
        install_package_flow(state, profile, &package.abs_dir, settings, tools, token)?;
    }
    Ok(())
}

/// 移除插件包(dsh plugin --profile <p> remove <name>)。
#[tauri::command]
pub fn plugins_remove_package(
    app: AppHandle,
    profile: String,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ActionAccepted, String> {
    let state = state.inner().clone();
    let settings = crate::config::load();
    let tools = state.tools.lock().unwrap().clone();
    let (id, token) = state.ops.begin(
        crate::contract::OperationKind::PluginRemove,
        true,
        "移除插件…",
    )?;
    state.set_snapshot(&app, |_| {});
    let app2 = app.clone();
    let state2 = state.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), crate::ops::OperationError> {
            token.check()?;
            crate::services::dshctl::run_stream(
                &state2.log_hub,
                &tools,
                &settings.repo_path,
                &settings.dsh_home,
                &[
                    "plugin".to_string(),
                    "--profile".to_string(),
                    profile.clone(),
                    "remove".to_string(),
                    name.clone(),
                ],
                &token,
            )
            .map_err(crate::ops::OperationError::Failed)
        })();
        finish_plugin_op(&state2, &app2, id, result);
    });
    Ok(ActionAccepted {
        ok: true,
        reason: None,
        aborted: None,
        already: None,
    })
}

// ── M5:技能管理子界面 ─────────────────────────────────────

fn skill_ctx() -> (crate::services::skills::ScanCtx, String) {
    let settings = crate::config::load();
    (
        crate::services::skills::ScanCtx {
            repo_path: settings.repo_path.clone(),
            dsh_home_setting: settings.dsh_home.clone(),
            skill_managed_root_setting: settings.skill_managed_root.clone(),
            external_skill_roots: settings.external_skill_roots.clone(),
        },
        settings.profile_name,
    )
}

fn emit_skills_changed(app: &AppHandle) {
    let _ = app.emit(crate::contract::EVENT_SKILLS_CHANGED, ());
}

/// 技能快照(全部根目录扫描 + 分组 + 一键启用状态)。
#[tauri::command]
pub fn skills_get_snapshot(state: State<'_, Arc<AppState>>) -> crate::contract::SkillsSnapshot {
    let (ctx, profile) = skill_ctx();
    crate::services::skills::snapshot(&ctx, &state.log_hub, &profile)
}

/// 新建技能(kebab + 唯一性校验;自动生成 frontmatter)。
#[tauri::command]
pub fn skills_create(
    app: AppHandle,
    name: String,
    description: String,
    when_to_use: Option<String>,
    body: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::SkillSummary, String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::create(
        &state.log_hub,
        &ctx,
        &name,
        &description,
        when_to_use.as_deref(),
        &body,
    );
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 更新技能(仅 managed 根)。
#[tauri::command]
pub fn skills_update(
    app: AppHandle,
    name: String,
    description: String,
    when_to_use: Option<String>,
    body: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::SkillSummary, String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::update(
        &state.log_hub,
        &ctx,
        &name,
        &description,
        when_to_use.as_deref(),
        &body,
    );
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 删除技能(路径围栏:仅 managed 根;外部路径一律拒绝)。
#[tauri::command]
pub fn skills_delete(
    app: AppHandle,
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::delete(&state.log_hub, &ctx, &name);
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 导入外部技能(递归拷贝 SKILL.md + scripts/references 等到 managed 根)。
#[tauri::command]
pub fn skills_import(
    app: AppHandle,
    source_path: String,
    name: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::SkillSummary, String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::import(&state.log_hub, &ctx, &source_path, name.as_deref());
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 预览技能正文(上限 256 KB)。
#[tauri::command]
pub fn skills_preview(source_path: String) -> Result<String, String> {
    crate::services::skills::preview(&source_path)
}

/// 一键启用技能根:把外部根写进 profile patch 的 skill-filesystem.customSkillDirs。
#[tauri::command]
pub async fn skills_enable_root(
    app: AppHandle,
    profile: String,
    root_path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    let r = crate::services::plugins::enable_skill_root(&state.log_hub, &ctx, &profile, &root_path);
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 已启动技能清单(读 skill-external-roots v0.2 回写的 skills-active.json)。
#[tauri::command]
pub fn skills_get_active() -> crate::contract::SkillsActiveSnapshot {
    let (ctx, profile) = skill_ctx();
    crate::services::skills::active_snapshot(&ctx, &profile)
}

/// 注入控制文件状态(当前各技能的注入开关)。
#[tauri::command]
pub fn skills_get_control() -> crate::contract::SkillsControlState {
    let (ctx, _profile) = skill_ctx();
    crate::services::skills::control_state(&ctx)
}

/// 技能注入开关:关闭 = 运行中 dsh 不再注入该技能(写控制文件,插件热更新)。
#[tauri::command]
pub fn skills_set_injected(
    app: AppHandle,
    name: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::SkillToggleResult, String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::set_injected(&state.log_hub, &ctx, &name, enabled);
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 按外部工具族根目录批量开关(Cursor/Codex/Claude/OpenCode)。
#[tauri::command]
pub fn skills_set_root_injected(
    app: AppHandle,
    root_key: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::SkillToggleResult, String> {
    let (ctx, _profile) = skill_ctx();
    let r = crate::services::skills::set_root_injected(&state.log_hub, &ctx, &root_key, enabled);
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}

/// 一键启用注入控制:把 skillControlFile/activeFile 写进 skill-external-roots 行
/// (整行重述 + dump-config 校验 + HMR),之后插件才读写控制/active 文件。
#[tauri::command]
pub async fn skills_enable_control(
    app: AppHandle,
    profile: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::contract::PatchWriteResult, String> {
    let (ctx, default_profile) = plugin_write_ctx(&state);
    let profile = nonempty_profile(&profile, &default_profile);
    let home = crate::services::plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let control_file = crate::services::skills::control_file(&home);
    let active_file = crate::services::skills::active_file(&home);
    let r = crate::services::plugins::enable_skill_control(
        &state.log_hub,
        &ctx,
        &profile,
        &control_file.to_string_lossy(),
        &active_file.to_string_lossy(),
    );
    if r.is_ok() {
        emit_skills_changed(&app);
    }
    r
}
