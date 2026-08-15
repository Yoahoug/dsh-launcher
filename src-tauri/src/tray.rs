// dsh-launcher · 系统托盘
// 动态菜单:随 AppSnapshot 状态启用/禁用;所有动作与 UI 走同一个 ActionCoordinator 数据源。
use crate::contract::{LauncherState, EVENT_OPEN_PAGE};
use crate::state::AppState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main";

fn state_text(state: &LauncherState) -> &'static str {
    match state {
        LauncherState::Idle => "空闲",
        LauncherState::Syncing => "同步中",
        LauncherState::Installing => "安装中",
        LauncherState::Building => "构建中",
        LauncherState::Starting => "启动中",
        LauncherState::Running => "运行中",
        LauncherState::Stopping => "停止中",
        LauncherState::Failed => "失败",
    }
}

/// 打开主窗口并切回 Regular(仅 macOS)。
pub fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 通知 renderer 切换页面(日志/设置等)。
fn open_page(app: &AppHandle, page: &str) {
    let _ = app.emit(EVENT_OPEN_PAGE, page);
    show_main_window(app);
}

/// 动态构建托盘菜单。
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let snap = state.snapshot();
    let busy = snap.busy
        || matches!(
            snap.state,
            LauncherState::Syncing
                | LauncherState::Installing
                | LauncherState::Building
                | LauncherState::Starting
                | LauncherState::Stopping
        );
    let running = snap.state == LauncherState::Running;

    let status = MenuItem::with_id(
        app,
        "status",
        format!("DSH Launcher · {}", state_text(&snap.state)),
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", "打开主窗口", true, None::<&str>)?;
    let open_dsh = MenuItem::with_id(
        app,
        "open-dsh",
        "打开 DeepSeek Harness",
        running,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let start = MenuItem::with_id(app, "start", "普通启动", !busy, None::<&str>)?;
    let dev = MenuItem::with_id(app, "dev", "开发模式", !busy, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "更新并构建", !busy, None::<&str>)?;
    let rebuild = MenuItem::with_id(app, "rebuild", "重建并重启", !busy, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "停止", running && !busy, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let logs = MenuItem::with_id(app, "logs", "查看日志", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let check_update = MenuItem::with_id(app, "check-update", "检查更新", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Launcher", true, None::<&str>)?;
    let stop_quit = MenuItem::with_id(
        app,
        "stop-and-quit",
        "停止服务并退出…",
        running,
        None::<&str>,
    )?;

    Menu::with_items(
        app,
        &[
            &status,
            &show,
            &open_dsh,
            &sep1,
            &start,
            &dev,
            &update,
            &rebuild,
            &stop,
            &sep2,
            &logs,
            &settings,
            &check_update,
            &sep3,
            &quit,
            &stop_quit,
        ],
    )
}

/// 刷新托盘菜单(状态变化时调用;无托盘时静默返回)。
pub fn refresh(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

/// 创建托盘(偏好 showTrayIcon=false 时创建但隐藏,用户可在设置里恢复)。
pub fn setup(app: &AppHandle) -> tauri::Result<TrayIcon<tauri::Wry>> {
    let menu = build_menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .expect("应用图标必须存在");
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("DSH Launcher")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "status" => {}
            "show" => show_main_window(app),
            "open-dsh" => {
                let _ = app.state::<AppState>().open_dsh();
            }
            "start" => {
                let _ = app.state::<AppState>().run_action("start");
            }
            "dev" => {
                let _ = app.state::<AppState>().run_action("dev");
            }
            "update" => {
                let _ = app.state::<AppState>().run_action("update");
            }
            "rebuild" => {
                let _ = app.state::<AppState>().run_action("rebuild");
            }
            "stop" => {
                let _ = app.state::<AppState>().run_action("stop");
            }
            "logs" => open_page(app, "logs"),
            "settings" => open_page(app, "settings"),
            "check-update" => {
                let _ = app.state::<AppState>().run_action("check-update");
                show_main_window(app);
            }
            "quit" => crate::lifecycle::quit_launcher(app),
            "stop-and-quit" => crate::lifecycle::stop_and_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    let show_icon = app
        .state::<AppState>()
        .preferences
        .lock()
        .unwrap()
        .show_tray_icon;
    tray.set_visible(show_icon)?;
    Ok(tray)
}
