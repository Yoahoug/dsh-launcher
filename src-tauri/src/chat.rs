// dsh-launcher · 独立 chat WebView(M3,回退路径)
//
// - 顶层 WebviewWindow 打开 http://127.0.0.1:<port>(本机 DeepSeek Harness Web UI,
//   不是官方 chat.deepseek.com);不使用 iframe,不把主窗口导航到 DSH;
// - M4.1 起普通用户路径改用主窗口内子 WebView(dsh_view),本模块仅保留为
//   可回退能力,不被 UI/托盘直接调用(不会弹出第二个 chat 窗口);
// - 安全逻辑(健康检查/loopback 导航/错误页/urlencoding/下载不覆盖)统一收敛在
//   dsh_view,这里只复用,不复制第二套。
use crate::config::{self, state_dir};
use crate::dsh_view::{dsh_health_check, error_page_url, loopback_allowed};
use crate::services::supervisor;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder};

pub const CHAT_LABEL: &str = "chat";
pub const EVENT_CHAT_STATE: &str = "app://chat-state";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatStatus {
    Closed,
    Starting,
    Checking,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatStateSnapshot {
    pub status: ChatStatus,
    pub url: Option<String>,
    pub error: Option<String>,
}

pub struct ChatManager {
    pub window: Mutex<Option<WebviewWindow>>,
    pub state: Mutex<ChatStateSnapshot>,
    /// 健康观察线程是否在运行。
    pub watcher_alive: AtomicBool,
}

impl Default for ChatManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatManager {
    pub fn new() -> Self {
        Self {
            window: Mutex::new(None),
            state: Mutex::new(ChatStateSnapshot {
                status: ChatStatus::Closed,
                url: None,
                error: None,
            }),
            watcher_alive: AtomicBool::new(false),
        }
    }

    fn set_state(&self, app: &AppHandle, snap: ChatStateSnapshot) {
        *self.state.lock().unwrap() = snap.clone();
        let _ = app.emit(EVENT_CHAT_STATE, &snap);
        log::info!("chat 状态 → {snap:?}");
    }

    pub fn current_state(&self) -> ChatStateSnapshot {
        self.state.lock().unwrap().clone()
    }
}

/// 健康检查:复用 dsh_view 的统一实现(内容标记 + 端口持有者)。
fn chat_url() -> String {
    crate::dsh_view::dsh_url()
}

/// 打开(或召回)chat 窗口。返回 (已显示, 是否需要等待服务)。
/// M4.1:仅作为回退路径;普通用户路径走 dsh_view::open_dsh_workspace。
pub fn open_chat(app: &AppHandle) -> Result<ChatStateSnapshot, String> {
    let state = app.state::<Arc<AppState>>();
    let chat = app.state::<Arc<ChatManager>>();

    // 已存在 → show + focus(复用)
    if let Some(w) = chat.window.lock().unwrap().as_ref() {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        chat.set_state(app, chat.current_state());
        return Ok(chat.current_state());
    }

    let settings = config::load();
    let expected_pid = state.supervisor.web_pid();
    let running = state.snapshot().state == crate::contract::LauncherState::Running
        || supervisor::port_holder_pid(settings.port).is_some();

    if !running || !dsh_health_check(settings.port, expected_pid) {
        // 服务未运行或健康检查不通过:进入启动流程(异步,UI 观察 chat-state)
        chat.set_state(
            app,
            ChatStateSnapshot {
                status: ChatStatus::Starting,
                url: Some(chat_url()),
                error: Some("DSH 服务未就绪,正在启动…".into()),
            },
        );
        let app2 = app.clone();
        let state2 = Arc::clone(&*state);
        let chat2 = Arc::clone(&*chat);
        std::thread::spawn(move || {
            chat2.set_state(
                &app2,
                ChatStateSnapshot {
                    status: ChatStatus::Starting,
                    url: Some(chat_url()),
                    error: None,
                },
            );
            let res = state2.run_action(&app2, "start");
            if !res.ok {
                chat2.set_state(
                    &app2,
                    ChatStateSnapshot {
                        status: ChatStatus::Error,
                        url: Some(chat_url()),
                        error: Some(res.reason.unwrap_or_else(|| "启动失败".into())),
                    },
                );
                return;
            }
            // 等待健康检查确认是预期 DSH 实例(最多 40s,期间轮询)
            let port = config::load().port;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
            loop {
                if dsh_health_check(port, state2.supervisor.web_pid()) {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    chat2.set_state(
                        &app2,
                        ChatStateSnapshot {
                            status: ChatStatus::Error,
                            url: Some(chat_url()),
                            error: Some("DSH 服务启动超时,健康检查未通过".into()),
                        },
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if let Err(e) = create_chat_window(&app2) {
                chat2.set_state(
                    &app2,
                    ChatStateSnapshot {
                        status: ChatStatus::Error,
                        url: Some(chat_url()),
                        error: Some(e),
                    },
                );
            }
        });
        return Ok(chat.current_state());
    }

    create_chat_window(app)
}

/// 创建 chat WebviewWindow(仅当健康检查通过后调用)。
#[allow(clippy::field_reassign_with_default)]
fn create_chat_window(app: &AppHandle) -> Result<ChatStateSnapshot, String> {
    let chat = app.state::<Arc<ChatManager>>();
    if let Some(w) = chat.window.lock().unwrap().as_ref() {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(chat.current_state());
    }
    let url = chat_url();
    let port = config::load().port;
    chat.set_state(
        app,
        ChatStateSnapshot {
            status: ChatStatus::Loading,
            url: Some(url.clone()),
            error: None,
        },
    );

    // 固定本地 UDF(WebView2 User Data Folder):state_dir/chat-webview2
    let udf: PathBuf = state_dir().join("chat-webview2");
    let _ = std::fs::create_dir_all(&udf);

    // WindowConfig 方式创建:chat 窗口需要 drag_drop_enabled=false(HTML5 拖放交给页面),
    // builder 无该接口,故从 WindowConfig 构建(逐字段覆盖默认值)。
    let mut cfg = tauri::utils::config::WindowConfig::default();
    cfg.label = CHAT_LABEL.into();
    cfg.url = tauri::utils::config::WebviewUrl::External(
        url.parse().map_err(|e| format!("URL 非法:{e}"))?,
    );
    cfg.title = "DeepSeek Harness".into();
    cfg.width = 1080.0;
    cfg.height = 760.0;
    cfg.min_width = Some(760.0);
    cfg.min_height = Some(560.0);
    cfg.resizable = true;
    cfg.center = true;
    cfg.drag_drop_enabled = false;
    cfg.visible = false;
    // Windows 第一版保留原生标题栏(decorations 默认 true)

    let mut builder = WebviewWindowBuilder::from_config(app, &cfg)
        .map_err(|e| format!("chat 窗口配置失败:{e}"))?
        .data_directory(udf)
        .devtools(cfg!(debug_assertions));

    // 导航策略:只允许精确 loopback origin;外链 → 系统浏览器;未知协议拦截
    builder = builder.on_navigation(move |url: &url::Url| {
        let ok = loopback_allowed(url, port);
        if !ok {
            // 外链(http/https)交给系统浏览器;其它协议直接拦截
            if matches!(url.scheme(), "http" | "https") {
                let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            }
        }
        ok
    });
    builder = builder.on_new_window(move |url, _features| {
        // 新窗口一律走系统浏览器(拦截任意 window.open 在 webview 内打开)
        if matches!(url.scheme(), "http" | "https") {
            let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
        }
        tauri::webview::NewWindowResponse::Deny
    });

    // 下载:默认下载目录 + 不覆盖已有文件(文件名自动加 (1),(2)…)
    let app4 = app.clone();
    builder = builder.on_download(move |_webview, event| {
        if let tauri::webview::DownloadEvent::Requested { destination, .. } = event {
            let target = crate::dsh_view::download_target_shared(&app4, destination);
            log::info!("chat 下载 → {}", target.display());
            *destination = target;
        }
        true // Requested:允许下载(仅修改目标路径);Finished:忽略返回值
    });

    let window = builder
        .build()
        .map_err(|e| format!("创建 chat 窗口失败:{e}"))?;

    // 关闭 = 隐藏(服务继续运行;托盘/主窗口可再次召回)
    let chat2 = Arc::clone(&*chat);
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = chat2.window.lock().unwrap().as_ref() {
                let _ = w.hide();
            }
        }
    });

    let _ = window.show();
    let _ = window.set_focus();
    *chat.window.lock().unwrap() = Some(window);
    chat.set_state(
        app,
        ChatStateSnapshot {
            status: ChatStatus::Ready,
            url: Some(url),
            error: None,
        },
    );
    app.state::<Arc<AppState>>()
        .timings
        .mark("chat_load_finished");
    app.state::<Arc<AppState>>().emit_perf(app);
    start_health_watcher(app);
    Ok(chat.current_state())
}

/// 健康观察:窗口打开期间每 5s 探测;DSH 死亡 → 错误页(自动重连)。
fn start_health_watcher(app: &AppHandle) {
    let chat = app.state::<Arc<ChatManager>>();
    if chat
        .watcher_alive
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    let app2 = app.clone();
    let chat2 = Arc::clone(&*chat);
    std::thread::spawn(move || {
        let port = config::load().port;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let window_open = chat2.window.lock().unwrap().is_some();
            if !window_open {
                chat2
                    .watcher_alive
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
            let ok = dsh_health_check(port, app2.state::<Arc<AppState>>().supervisor.web_pid());
            let cur = chat2.current_state();
            if ok && cur.status != ChatStatus::Ready {
                // 恢复:重新导航到 DSH
                if let Some(w) = chat2.window.lock().unwrap().as_ref() {
                    let url = chat_url();
                    let _ = w.navigate(url.parse().unwrap());
                }
                chat2.set_state(
                    &app2,
                    ChatStateSnapshot {
                        status: ChatStatus::Ready,
                        url: Some(chat_url()),
                        error: None,
                    },
                );
            } else if !ok && cur.status == ChatStatus::Ready {
                // 服务死亡 → 错误页(数据 URL,无任何 IPC)
                chat2.set_state(
                    &app2,
                    ChatStateSnapshot {
                        status: ChatStatus::Error,
                        url: Some(chat_url()),
                        error: Some("DSH 服务已停止或失去响应".into()),
                    },
                );
                if let Some(w) = chat2.window.lock().unwrap().as_ref() {
                    let _ = w.navigate(error_page_url(&chat_url()).parse().unwrap());
                }
            }
        }
    });
}

/// 关闭 chat 窗口(销毁,释放资源)。
pub fn close_chat(app: &AppHandle) {
    let chat = app.state::<Arc<ChatManager>>();
    if let Some(w) = chat.window.lock().unwrap().take() {
        let _ = w.destroy();
    }
    chat.set_state(
        app,
        ChatStateSnapshot {
            status: ChatStatus::Closed,
            url: None,
            error: None,
        },
    );
}

/// chat 窗口是否打开(主窗口退出前清理)。
pub fn chat_open(app: &AppHandle) -> bool {
    app.state::<Arc<ChatManager>>()
        .window
        .lock()
        .unwrap()
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_state_serde() {
        let s = ChatStateSnapshot {
            status: ChatStatus::Ready,
            url: Some("http://127.0.0.1:3080/".into()),
            error: None,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"status\":\"ready\""), "{j}");
        let back: ChatStateSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}
