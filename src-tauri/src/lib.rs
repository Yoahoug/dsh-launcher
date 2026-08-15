// dsh-launcher · Tauri 应用入口
mod bridge;
mod commands;
mod contract;
mod state;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Manager, RunEvent};

/// 召回主窗口:第二实例触发时显示并聚焦。
fn recall_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
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
        ])
        .setup(|app| {
            let app_state = app.state::<state::AppState>();
            let app_handle = app.handle().clone();

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
                    bridge::start_poller(app_handle, sup);
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
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("dsh-launcher 初始化失败");

    app.run(|app_handle, event| {
        // macOS:隐藏窗口后点 Dock 图标召回
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            recall_main_window(app_handle);
        }
        // 应用退出:daemon detach(dsh web 继续运行,下次召回),停止轮询
        if let RunEvent::ExitRequested { .. } = event {
            if let Some(sup) = app_handle.state::<state::AppState>().supervisor() {
                bridge::shutdown(&sup);
            }
        }
    });
}
