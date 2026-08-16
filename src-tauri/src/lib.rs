// dsh-launcher · Tauri 应用入口
// M4 原生核心:LogHub + Supervisor + ActionCoordinator,无 Node daemon、无 3090。
// 后续阶段:统一 Operation Coordinator(journal/取消)、签名 catalog、托管工具链、
// clone 事务、chat WebView。
#[cfg(test)]
pub mod test_lock {
    /// 所有修改进程级 env 覆盖(DSH_LAUNCHER_*)的测试必须共用此锁串行执行。
    pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
pub mod archive;
pub mod catalog;
pub mod chat;
pub mod clone;
mod commands;
pub mod config;
pub mod contract;
pub mod download;
pub mod dsh_view;
mod lifecycle;
pub mod log_hub;
mod migration;
pub mod ops;
pub mod perf;
mod preferences;
pub mod services;
mod state;
pub mod toolchain;
mod tray;

use crate::contract::{LogEntry, LogLevel, EVENT_LOG_APPENDED};
use crate::log_hub::LogSink;
use crate::services::runtime::{self, Tools};
use crate::services::supervisor::Supervisor;
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

/// 召回主窗口:第二实例触发时显示并聚焦。
fn recall_main_window(app: &tauri::AppHandle) {
    crate::tray::show_main_window(app);
}

/// setup 后的后台初始化:安全校验、工具解析、迁移、已有 Host 召回和 repo 快照都不阻塞首帧。
fn bootstrap_async(app: tauri::AppHandle, state: Arc<state::AppState>) {
    let log_hub = state.log_hub.clone();
    if let Err(e) = catalog::verify_embedded() {
        log_hub.append("launcher", LogLevel::Err, &format!("catalog 自检失败:{e}"));
        state.set_bootstrap_error(&app, format!("catalog 自检失败(安全失败,拒绝启动):{e}"));
        return;
    }
    let cat = match catalog::load_catalog() {
        Ok(cat) => cat,
        Err(e) => {
            log_hub.append("launcher", LogLevel::Err, &format!("catalog 加载失败:{e}"));
            state.set_bootstrap_error(&app, format!("catalog 加载失败(安全失败,拒绝启动):{e}"));
            return;
        }
    };
    log_hub.append(
        "launcher",
        LogLevel::Info,
        &format!(
            "runtime catalog v{} 验签通过({} 个组件,全部国内镜像)",
            cat.schema,
            cat.components.len()
        ),
    );

    let recovered = ops::recover_stale(&log_hub);
    if !recovered.is_empty() {
        log_hub.append(
            "launcher",
            LogLevel::Warn,
            &format!(
                "检测到 {} 个上次中断的操作,已标记为 interrupted(请检查后重试)",
                recovered.len()
            ),
        );
    }

    // 有效 packaged manifest 走快速路径，不做重复工具扫描；首次预配或开发模式再按需扫描。
    if runtime::packaged_runtime_fast_path_available(&app) {
        log_hub.append(
            "launcher",
            LogLevel::Info,
            "检测到有效 packaged 运行时 manifest，跳过重复工具扫描",
        );
    } else {
        state.refresh_tools();
        if state.tools.lock().unwrap().pnpm.is_none() {
            log_hub.append(
                "launcher",
                LogLevel::Warn,
                "未找到 pnpm:开发模式/更新并构建不可用,普通模式首次启动会尝试安装",
            );
        }
        if state.tools.lock().unwrap().git.is_none() {
            log_hub.append(
                "launcher",
                LogLevel::Warn,
                if cfg!(windows) {
                    "未找到系统 git:开发模式/克隆/更新并构建不可用,可「安装托管工具链」安装托管 MinGit"
                } else {
                    "未找到系统 git:开发模式/克隆/更新并构建不可用,请安装 Xcode Command Line Tools 或 Homebrew git"
                },
            );
        }
    }

    let report = migration::run(&state.log_hub);
    if report.old_daemon_terminated.is_some() {
        log::info!("迁移:已终止旧 Node daemon,由桌面核心接管");
    }

    // 召回上次 detach 保留的 dsh web(三重校验:pid 存活 + 命令行 + 端口)。
    if let Some(m) = state.supervisor.recall() {
        let cmd = crate::services::supervisor::process_cmdline(m.pid);
        let is_dsh = cmd.is_some_and(|c| {
            c.contains("dsh web") || c.contains("apps/cli/lib/bin.js") || c.contains("bin.js web")
        });
        let on_port = crate::services::supervisor::port_holder_pid(config::load().port)
            .is_some_and(|holder| {
                crate::services::supervisor::process_descends_from(holder, m.pid)
            });
        if is_dsh && on_port {
            log_hub.append(
                "launcher",
                LogLevel::Ok,
                &format!("召回上次运行的 dsh web(PID {})", m.pid),
            );
            state.set_snapshot(&app, |s| {
                s.state = crate::contract::LauncherState::Running;
                s.web_pid = Some(m.pid);
                s.url = m.url.clone();
                s.started_at = m.started_at;
                s.ready_at = m.ready_at;
                s.mode = crate::contract::LauncherMode::Normal;
            });
        } else {
            log_hub.append(
                "launcher",
                LogLevel::Warn,
                "上次运行的 dsh web 记录失效(进程已退出或端口变化),忽略",
            );
            state.supervisor.detach();
        }
    }

    // catalog 已安全通过后即可允许启动；repo 快照仍独立后台刷新，不成为普通启动前置条件。
    state.mark_bootstrap_ready();
    let repo_state = state.clone();
    let repo_app = app.clone();
    std::thread::spawn(move || repo_state.refresh_repo_emit(&repo_app));

    if crate::config::take_update_restart_pending() {
        let restart_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            restart_handle.restart();
        });
    }
    crate::lifecycle::apply_preferences(&app);
    crate::dsh_view::maybe_enter_on_boot(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 性能测量点:process_start(近似,main 入口即 run 首行)
    let timings = Arc::new(crate::perf::BootTimings::new(std::time::Instant::now()));
    timings.mark("process_start");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!("收到第二实例请求,召回主窗口");
            recall_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::run_action,
            commands::get_logs,
            commands::get_settings,
            commands::save_settings,
            commands::inspect_environment,
            commands::check_for_update,
            commands::apply_update,
            commands::restart_app,
            commands::open_dsh,
            commands::open_repo_directory,
            commands::open_log_directory,
            commands::get_desktop_snapshot,
            commands::archives_get_snapshot,
            commands::archives_restore,
            commands::archives_delete,
            commands::archives_delete_all,
            commands::save_preferences,
            commands::complete_first_run,
            commands::set_topbar_hidden,
            commands::get_cursor_position,
            commands::confirm_and_run,
            commands::quit_app,
            commands::pick_directory,
            commands::clear_logs,
            commands::perf_mark,
            commands::get_perf_metrics,
            commands::get_clone_state,
            commands::submit_clone_request,
            commands::open_clone_dialog,
            commands::get_installation_snapshot,
            commands::open_chat,
            commands::close_chat,
            commands::get_chat_state,
            commands::open_dsh_workspace,
            commands::back_to_launcher,
            commands::retry_dsh_view,
            commands::set_workspace,
            commands::get_dsh_view_state,
            // M5:插件管理子界面
            commands::plugins_get_snapshot,
            commands::plugins_set_enabled,
            commands::plugins_save_config,
            commands::plugins_reset_row,
            commands::plugins_validate_patch,
            commands::dshctl_dump_config,
            commands::plugins_open_in_explorer,
            commands::plugins_install_package,
            commands::plugins_install_all,
            commands::plugins_remove_package,
            // M5:技能管理子界面
            commands::skills_get_snapshot,
            commands::skills_create,
            commands::skills_update,
            commands::skills_delete,
            commands::skills_import,
            commands::skills_preview,
            commands::skills_enable_root,
            commands::skills_get_active,
            commands::skills_get_control,
            commands::skills_set_injected,
            commands::skills_set_root_injected,
            commands::skills_enable_control,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // 原生核心:LogHub(文件 + 事件广播)
            let log_path = crate::services::supervisor::log_file();
            let sink: Arc<LogSink> = {
                let h = app_handle.clone();
                Arc::new(move |entry: &LogEntry| {
                    let _ = h.emit(EVENT_LOG_APPENDED, entry);
                })
            };
            let log_hub = Arc::new(log_hub::LogHub::new(log_path, sink, true));
            log_hub.append(
                "launcher",
                LogLevel::Ok,
                &format!(
                    "DSH Launcher {} 启动 · 原生核心(纯 Rust,无 Node daemon)",
                    env!("CARGO_PKG_VERSION")
                ),
            );

            let supervisor = Arc::new(Supervisor::new(log_hub.clone()));
            let ops = ops::OperationCoordinator::new(log_hub.clone(), Arc::new(|_| {}));
            let state = Arc::new(state::AppState::new(
                log_hub.clone(),
                supervisor,
                Tools::empty(),
                ops,
                timings.clone(),
            ));
            app.manage(state.clone());
            app.manage(Arc::new(chat::ChatManager::new()));
            app.manage(Arc::new(dsh_view::DshViewManager::new()));

            // 托盘:状态动态菜单 + 左键召回
            if let Err(e) = tray::setup(&app_handle) {
                log::error!("托盘初始化失败: {e}");
            }
            // 性能测量点:主窗口已创建并可见(首帧不等待网络/Git/更新/完整环境检查)
            timings.mark("main_window_visible");
            state.emit_perf(&app_handle);

            // 静默启动:偏好开启时首帧隐藏窗口(托盘可召回)
            let prefs = state.preferences.lock().unwrap().clone();
            if prefs.silent_startup {
                if let Some(w) = app_handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            timings.mark("tauri_ready");
            state.emit_perf(&app_handle);

            // 深度校验、工具扫描、迁移、repo 刷新和 Host 召回均异步执行，窗口先可见。
            let bootstrap_app = app_handle.clone();
            let bootstrap_state = state.clone();
            std::thread::spawn(move || bootstrap_async(bootstrap_app, bootstrap_state));

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    lifecycle::on_close_requested(window.app_handle(), window, api);
                }
                // 窗口尺寸/位置/DPI 变化 → 重排 dsh-content 子 WebView(标题栏以下)
                WindowEvent::Resized(_)
                | WindowEvent::Moved(_)
                | WindowEvent::ScaleFactorChanged { .. } => {
                    dsh_view::relayout(window.app_handle());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("dsh-launcher 初始化失败");

    app.run(|app_handle, event| {
        match event {
            // macOS:隐藏窗口后点 Dock 图标召回(Reopen 为 macOS 专属变体)
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => lifecycle::on_reopen(app_handle),
            // 保持托盘:阻止运行时自动退出请求;偏好 quit 时允许(执行 detach)
            RunEvent::ExitRequested { api, .. } => {
                lifecycle::on_exit_requested(app_handle, &api);
            }
            _ => {}
        }
    });
}
