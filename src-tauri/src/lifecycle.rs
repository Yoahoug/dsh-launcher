// dsh-launcher · 生命周期:关闭隐藏、Dock/taskbar 策略、自启、退出语义。
// 三条退出路径互不干扰:
//   1. 托盘「退出」/「停止并退出」:先停止 dsh 进程树,再退出(完整退出,无残留进程);
//   2. 关窗(偏好 quit):detach dsh(继续后台运行) → exit;
//   3. updater restart:由用户确认后触发 app.restart(),不停止 dsh、不走普通退出清理。
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

/// ExitRequested:仅拦截「非显式退出」(如 macOS Cmd+Q 在托盘模式下应留在托盘)。
/// 显式退出(托盘「退出」/「停止并退出」)会先置位 quit_requested,这里放行,
/// 否则 app.exit() 被 prevent_exit 拦下,托盘图标消失但 exe 进程永远残留。
pub fn on_exit_requested(app: &AppHandle, api: &tauri::ExitRequestApi) {
    use std::sync::atomic::Ordering;
    let state = app.state::<Arc<AppState>>();
    if state.quit_requested.load(Ordering::SeqCst) {
        return; // 显式退出:允许真正退出
    }
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
#[cfg(target_os = "macos")]
pub fn on_reopen(app: &AppHandle) {
    crate::tray::show_main_window(app);
}

/// detach:让 dsh 继续后台运行(清空托管注册表)。幂等。
fn detach(app: &AppHandle) {
    app.state::<Arc<AppState>>().on_app_exit();
}

/// 普通退出:仅 detach(不停止 dsh),保存窗口状态由 window-state 插件完成。
pub fn quit_launcher(app: &AppHandle) {
    // 置位「显式退出」,on_exit_requested 才会放行,进程才能真正终止
    use std::sync::atomic::Ordering;
    app.state::<Arc<AppState>>()
        .quit_requested
        .store(true, Ordering::SeqCst);
    detach(app);
    remove_tray(app);
    app.exit(0);
}

/// 原生确认框(通用):回调通过异步 channel 返回，绝不阻塞 WebKit/AppKit。
pub async fn confirm(app: &AppHandle, title: &str, message: &str) -> bool {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog();
    let (tx, mut rx) = tauri::async_runtime::channel::<bool>(1);
    dialog
        .message(message)
        .title(title)
        .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel)
        .show(move |yes| {
            let _ = tx.try_send(yes);
        });
    rx.recv().await.unwrap_or(false)
}

/// 原生确认框:停止服务并退出。
pub async fn confirm_stop_and_quit(app: &AppHandle) -> bool {
    confirm(
        app,
        "停止服务并退出",
        "确定要停止 dsh 并退出 DSH Launcher 吗?",
    )
    .await
}

/// 已确认后停止服务并退出:stop → 等待空闲 → detach → 退出。
pub fn stop_and_quit(app: &AppHandle) {
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

/// 托盘入口:按偏好异步确认，避免在 AppKit 菜单回调中同步等待对话框。
pub fn request_stop_and_quit(app: &AppHandle) {
    let confirm_first = app
        .state::<Arc<AppState>>()
        .preferences
        .lock()
        .unwrap()
        .confirm_stop_and_quit;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if !confirm_first || confirm_stop_and_quit(&handle).await {
            stop_and_quit(&handle);
        }
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
