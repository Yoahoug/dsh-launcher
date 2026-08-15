// dsh-launcher · Tauri 应用入口
mod bridge;
mod commands;
mod contract;
mod lifecycle;
mod preferences;
mod state;
mod tray;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Manager, RunEvent, WindowEvent};

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
        .manage(state::AppState::new())
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
            let app_state = app.state::<state::AppState>();
            let app_handle = app.handle().clone();

            // 托盘:状态动态菜单 + 左键召回
            if let Err(e) = tray::setup(&app_handle) {
                log::error!("托盘初始化失败: {e}");
            }

            // 启动/接管 Node 核心 daemon
            let state_dir = bridge::state_dir();
            match bridge::start_or_takeover(&state_dir) {
                Ok((handle, client, child)) => {
                    log::info!(
                        "bridge 就绪:{} daemon(pid {})",
                        if handle.owned { "托管" } else { "接管" },
                        handle.pid
                    );
                    let sup = Arc::new(bridge::BridgeSupervisor {
                        stop: Arc::new(AtomicBool::new(false)),
                        client: client.clone(),
                        daemon: Arc::new(Mutex::new(child)),
                        poll_state: app_state.poll_state.clone(),
                    });
                    *app_state.client.lock().unwrap() = Some(client);
                    *app_state.supervisor.lock().unwrap() = Some(sup.clone());
                    // 首次拉取 + 启动轮询
                    let _ = sup.client.poll(&sup.poll_state, &app_handle);
                    bridge::start_poller(app_handle.clone(), sup);
                    // legacy 浏览器控制台:仅显式环境变量开启(不作为用户入口)
                    // 删除条件:legacy UI 移除后删除
                    if std::env::var("DSH_LAUNCHER_LEGACY_UI").as_deref() == Ok("1") {
                        log::warn!("DSH_LAUNCHER_LEGACY_UI=1:打开 legacy 浏览器控制台(仅回归用)");
                        let _ =
                            tauri_plugin_opener::open_url("http://127.0.0.1:3090/", None::<&str>);
                    }
                }
                Err(e) => {
                    log::error!("bridge 启动失败: {e}");
                    *app_state.boot_error.lock().unwrap() = Some(e);
                }
            }

            // 应用偏好副作用(autostart 同步、托盘可见性)
            lifecycle::apply_preferences(&app_handle);

            // 静默启动:偏好开启时首帧隐藏窗口(托盘可召回)
            let prefs = app_state.preferences.lock().unwrap().clone();
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
