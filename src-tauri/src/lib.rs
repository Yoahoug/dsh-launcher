// dsh-launcher · Tauri 应用入口
// M4 原生核心:LogHub + Supervisor + ActionCoordinator,无 Node daemon、无 3090。
#[cfg(test)]
pub mod test_lock {
    /// 所有修改进程级 env 覆盖(DSH_LAUNCHER_*)的测试必须共用此锁串行执行。
    pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
mod commands;
pub mod config;
pub mod contract;
mod lifecycle;
pub mod log_hub;
mod migration;
mod preferences;
pub mod services;
mod state;
mod tray;

use crate::contract::{LogEntry, LogLevel, EVENT_LOG_APPENDED};
use crate::log_hub::LogSink;
use crate::services::runtime::{self, Tools};
use crate::services::supervisor::Supervisor;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

/// 召回主窗口:第二实例触发时显示并聚焦。
fn recall_main_window(app: &tauri::AppHandle) {
    crate::tray::show_main_window(app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            commands::open_dsh,
            commands::open_repo_directory,
            commands::open_log_directory,
            commands::get_desktop_snapshot,
            commands::save_preferences,
            commands::confirm_and_run,
            commands::quit_app,
            commands::pick_directory,
            commands::clear_logs,
        ])
        .setup(|app| {
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
                    "DSH Launcher {} 启动 · 原生核心(v0.3.0,无 Node daemon)",
                    env!("CARGO_PKG_VERSION")
                ),
            );

            let supervisor = Arc::new(Supervisor::new(log_hub.clone()));

            // 工具解析(可能为 None,命令层给出可读诊断)
            let dsh_node_dir: Option<PathBuf> =
                runtime::resolve_dsh_node().and_then(|(bin, _)| bin.parent().map(PathBuf::from));
            let tools = Tools {
                pnpm: runtime::resolve_executable("pnpm"),
                git: runtime::resolve_executable("git"),
                dsh_node_dir,
            };
            if tools.pnpm.is_none() {
                log_hub.append(
                    "launcher",
                    LogLevel::Warn,
                    "未找到 pnpm:启动/开发模式/更新并构建不可用,请在设置页查看诊断",
                );
            }

            let state = Arc::new(state::AppState::new(log_hub.clone(), supervisor, tools));
            app.manage(state.clone());

            // 迁移旧 Node daemon(幂等):终止旧 daemon、清理 token、记录版本
            let report = migration::run(&state.log_hub);
            if report.old_daemon_terminated.is_some() {
                log::info!("迁移:已终止旧 Node daemon,由桌面核心接管");
            }

            // 召回上次 detach 保留的 dsh web(三重校验:pid 存活 + 命令行 + 端口)
            if let Some(m) = state.supervisor.recall() {
                let cmd = crate::services::supervisor::process_cmdline(m.pid);
                let is_dsh = cmd.is_some_and(|c| c.contains("dsh web") || c.contains("dsh"));
                let on_port = state
                    .supervisor
                    .web_pid()
                    .is_some_and(|_| config::probe_port("127.0.0.1", config::load().port));
                if is_dsh && on_port {
                    log_hub.append(
                        "launcher",
                        LogLevel::Ok,
                        &format!("召回上次运行的 dsh web(PID {})", m.pid),
                    );
                    state.set_snapshot(&app_handle, |s| {
                        s.state = crate::contract::LauncherState::Running;
                        s.web_pid = Some(m.pid);
                        s.url = m.url.clone();
                        s.started_at = m.started_at;
                        s.ready_at = m.ready_at;
                        s.mode = crate::contract::LauncherMode::Normal;
                    });
                } else {
                    // 校验不通过:仅清理记录,不杀进程
                    log_hub.append(
                        "launcher",
                        LogLevel::Warn,
                        "上次运行的 dsh web 记录失效(进程已退出或端口变化),忽略",
                    );
                    state.supervisor.detach();
                }
            }

            // 托盘:状态动态菜单 + 左键召回
            if let Err(e) = tray::setup(&app_handle) {
                log::error!("托盘初始化失败: {e}");
            }

            // 仓库状态快照(首帧)
            state.refresh_repo_emit(&app_handle);

            // 应用偏好副作用(autostart 同步、托盘可见性)
            lifecycle::apply_preferences(&app_handle);

            // 静默启动:偏好开启时首帧隐藏窗口(托盘可召回)
            let prefs = state.preferences.lock().unwrap().clone();
            if prefs.silent_startup {
                if let Some(w) = app_handle.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                lifecycle::on_close_requested(window.app_handle(), window, api);
            }
        })
        .build(tauri::generate_context!())
        .expect("dsh-launcher 初始化失败");

    app.run(|app_handle, event| {
        match event {
            // macOS:隐藏窗口后点 Dock 图标召回
            RunEvent::Reopen { .. } => lifecycle::on_reopen(app_handle),
            // 保持托盘:阻止运行时自动退出请求;偏好 quit 时允许(执行 detach)
            RunEvent::ExitRequested { api, .. } => {
                lifecycle::on_exit_requested(app_handle, &api);
            }
            _ => {}
        }
    });
}
