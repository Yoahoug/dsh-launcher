// dsh-launcher · 生命周期:关闭隐藏、Dock/taskbar 策略、自启、退出语义。
// 三条退出路径互不干扰:
//   1. 普通退出(退出 Launcher):detach dsh(继续后台运行) → exit;
//   2. 停止并退出:确认 → stop → 等待空闲 → detach → exit;
//   3. updater restart:由 updater 插件触发 app.exit(),不停止 dsh、不走普通退出清理。
use crate::contract::{CloseBehavior, EVENT_PREFERENCES_CHANGED};
use crate::state::AppState;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// CloseRequested:默认隐藏到托盘;偏好 quit 时直接退出(普通退出语义,不停止 dsh)。
pub fn on_close_requested(app: &AppHandle, window: &tauri::Window, api: &tauri::CloseRequestApi) {
    let prefs = app
        .state::<Arc<AppState>>()
        .preferences
        .lock()
        .unwrap()
        .clone();
    match prefs.close_behavior {
        CloseBehavior::Tray => {
            api.prevent_close();
            let _ = window.hide();
            // macOS:隐藏后切 Accessory 隐藏 Dock;Windows:跳过任务栏
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            #[cfg(target_os = "windows")]
            {
                let _ = window.set_skip_taskbar(true);
            }
        }
        CloseBehavior::Quit => {
            // 关窗即退出:detach 后退出(不停止 dsh)
            quit_launcher(app);
        }
    }
}

/// ExitRequested:阻止运行时自动退出请求(保持托盘),除非偏好允许退出。
pub fn on_exit_requested(app: &AppHandle, api: &tauri::ExitRequestApi) {
    let prefs = app
        .state::<Arc<AppState>>()
        .preferences
        .lock()
        .unwrap()
        .clone();
    match prefs.close_behavior {
        CloseBehavior::Tray => {
            api.prevent_exit();
        }
        CloseBehavior::Quit => {
            detach(app);
        }
    }
}

/// macOS Dock 图标召回(Reopen):恢复 Regular 并显示窗口。
pub fn on_reopen(app: &AppHandle) {
    crate::tray::show_main_window(app);
}

/// detach:让 dsh 继续后台运行(清空托管注册表)。幂等。
fn detach(app: &AppHandle) {
    app.state::<Arc<AppState>>().on_app_exit();
}

/// 普通退出:仅 detach(不停止 dsh),保存窗口状态由 window-state 插件完成。
pub fn quit_launcher(app: &AppHandle) {
    detach(app);
    remove_tray(app);
    app.exit(0);
}

/// 原生确认框(通用):返回用户是否确认。
pub fn confirm_blocking(app: &AppHandle, title: &str, message: &str) -> bool {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog();
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    dialog
        .message(message)
        .title(title)
        .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel)
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.recv_timeout(Duration::from_secs(300)).unwrap_or(false)
}

/// 原生确认框:停止服务并退出。
pub fn confirm_stop_and_quit_blocking(app: &AppHandle) -> bool {
    confirm_blocking(
        app,
        "停止服务并退出",
        "确定要停止 dsh 并退出 DSH Launcher 吗?",
    )
}

/// 停止服务并退出:确认(偏好开关)→ stop → 等待空闲 → detach → 退出。
pub fn stop_and_quit(app: &AppHandle) {
    let prefs = app
        .state::<Arc<AppState>>()
        .preferences
        .lock()
        .unwrap()
        .clone();
    if prefs.confirm_stop_and_quit && !confirm_stop_and_quit_blocking(app) {
        return;
    }
    let state = app.state::<Arc<AppState>>();
    let res = state.run_action(app, "stop");
    if !res.ok {
        log::warn!("停止服务失败,仍退出: {:?}", res.reason);
        quit_launcher(app);
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        // 等待停止完成(≤15s),然后 detach 退出
        for _ in 0..150 {
            let snap = handle.state::<Arc<AppState>>().snapshot();
            if snap.state == crate::contract::LauncherState::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        quit_launcher(&handle);
    });
}

/// 移除托盘图标(退出前调用)。
fn remove_tray(app: &AppHandle) {
    app.remove_tray_by_id("main");
}

/// 偏好变更后应用副作用:autostart 同步、托盘可见性、通知 renderer 应用主题。
pub fn apply_preferences(app: &AppHandle) {
    let prefs = app
        .state::<Arc<AppState>>()
        .preferences
        .lock()
        .unwrap()
        .clone();

    // autostart 只由 Tauri 插件管理,不再调用旧 LaunchAgent
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let enabled = mgr.is_enabled().unwrap_or(false);
    if prefs.launch_on_startup && !enabled {
        let _ = mgr.enable();
    } else if !prefs.launch_on_startup && enabled {
        let _ = mgr.disable();
    }

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(prefs.show_tray_icon);
    }

    let _ = app.emit(EVENT_PREFERENCES_CHANGED, &prefs);
}
