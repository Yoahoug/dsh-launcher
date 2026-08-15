// dsh-launcher · 主窗口内 DeepSeek 工作区(M4.1)
//
// 产品行为:
// - 主窗口分为两个顶级工作区:launcher(启动器)与 dsh(DeepSeek 完整工作区);
// - dsh 工作区 = 主窗口内的原生子 WebView(label=dsh-content),占据标题栏以下全部
//   可用区域,不是独立窗口、不是 iframe、不跳转浏览器;
// - 主 React WebView 负责窗口外壳/标题栏/启动器与工作区切换;子 WebView 位于
//   标题栏(64 logical px)以下,随窗口尺寸/DPI/显示器变化重新布局;
// - 启动成功(accepted ≠ success,需真实终态)且健康检查通过后自动进入 dsh 工作区;
//   失败/取消/超时不显示成功,也不进入空白页面;
// - 切换工作区只隐藏/显示子 WebView,不销毁、不刷新,会话与页面状态保持;
// - DSH 意外退出 → Disconnected(前端显示断线/重连状态),不静默打开系统浏览器。
//
// 安全边界:
// - dsh-content 不在任何 capability 的 windows/webviews 列表(零权限),且远程
//   loopback URL 不会注入 Tauri IPC(双重隔离);不向 DSH 页面注入本地状态;
// - 只允许精确 loopback origin 内部导航;外链交给系统浏览器;未知协议拦截;
// - 下载默认目录 + 不覆盖(自动 (1),(2)…);固定本地 UDF(WebView2)保持登录态;
// - 健康检查双重确认:HTTP 内容标记 + 端口持有者身份。
//
// Windows 线程约束:绝不在同步 command/同步事件处理器里创建子 WebView
// (add_child 内部会派发到主线程;调用方必须是后台线程/异步 command)。
use crate::config::{self, state_dir};
use crate::contract::{
    DshViewSnapshot, DshViewStatus, LauncherState, Workspace, EVENT_DSH_VIEW_STATE,
};
use crate::state::AppState;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalSize, Webview, WebviewUrl,
};

pub const DSH_VIEW_LABEL: &str = "dsh-content";
/// 标题栏高度(与前端 h-16 = 64px 对齐,logical 坐标)。
pub const TITLEBAR_HEIGHT: f64 = 64.0;

// ── 几何计算(纯函数,可单测) ─────────────────────────────

/// 子 WebView 的目标几何(逻辑坐标):
/// - 顶部栏可见(默认):标题栏以下全部区域;
/// - 顶部栏隐藏(全屏 + DeepSeek 工作区自动隐藏 chrome):占满整个窗口,DeepSeek 真全屏。
/// `inner` 是窗口内区物理尺寸,`scale` 是当前 DPI 缩放因子。
pub fn dsh_view_geometry(
    inner: PhysicalSize<u32>,
    scale: f64,
    topbar_hidden: bool,
) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let w = inner.width as f64 / scale;
    let top = if topbar_hidden { 0.0 } else { TITLEBAR_HEIGHT };
    let h = (inner.height as f64 / scale - top).max(0.0);
    (LogicalPosition::new(0.0, top), LogicalSize::new(w, h))
}

// ── 健康检查 / 导航策略 / 错误页(与 chat.rs 共享,单一实现) ──

/// 健康检查:HTTP 内容标记 + 端口持有者身份(双重确认是预期 DSH 实例)。
pub fn dsh_health_check(port: u16, expected_pid: Option<u32>) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(5))
        .build();
    let body_ok = agent
        .get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .is_some_and(|body| {
            body.contains("DeepSeek Harness")
                || body.contains("manifest.webmanifest")
                || body.contains("dsh-web")
        });
    if !body_ok {
        return false;
    }
    // 端口持有者身份:持有者必须属于 Launcher 托管的 dsh 进程树。
    if let Some(pid) = expected_pid {
        return crate::services::supervisor::port_holder_pid(port)
            .is_some_and(|holder| crate::services::supervisor::process_descends_from(holder, pid));
    }
    crate::services::supervisor::port_holder_pid(port).is_some_and(|pid| {
        crate::services::supervisor::process_cmdline(pid).is_some_and(|c| {
            c.contains("dsh web") || (c.contains("apps/cli/src/bin") && c.contains(" web "))
        })
    })
}

pub fn dsh_url() -> String {
    let s = config::load();
    // `host` 是服务监听地址；0.0.0.0 不能作为受信任的页面 origin 使用。
    // 桌面 WebView 始终通过精确 loopback 地址访问本机服务。
    format!("http://127.0.0.1:{}/", s.port)
}

/// 是否允许在子 WebView 内导航:精确 loopback origin + 预期端口。
pub fn loopback_allowed(url: &url::Url, port: u16) -> bool {
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port().map(|p| p == port).unwrap_or(false)
}

/// 本地错误页(data: URL,零权限;保留给回退路径使用)。
pub fn error_page_url(back_url: &str) -> String {
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8">
<title>DSH 连接中断</title>
<body style="font-family:system-ui,sans-serif;background:#0f172a;color:#e2e8f0;display:flex;align-items:center;justify-content:center;height:100vh;margin:0">
<div style="text-align:center;max-width:480px">
<h2 style="margin:0 0 8px">DeepSeek Harness 连接中断</h2>
<p style="color:#94a3b8;font-size:14px">服务可能已停止或正在重启。可点击下方重试;也可回到 DSH Launcher 控制台查看日志、重启服务或以系统浏览器打开。</p>
<button onclick="location.href='{back_url}'" style="margin-top:16px;padding:10px 20px;border-radius:8px;border:none;background:#3b82f6;color:#fff;cursor:pointer">重试连接</button>
<p style="margin-top:24px;font-size:12px;color:#64748b">返回 DSH Launcher 控制台 · 查看日志 · 在系统浏览器打开</p>
</div></body></html>"#
    );
    format!("data:text/html;charset=utf-8,{}", urlencoding(&html))
}

/// 简单 URL 编码(data URL 用)。
pub fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b' ' => {
                if *b == b' ' {
                    out.push('+');
                } else {
                    out.push(*b as char);
                }
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 下载目标:默认下载目录 + 不覆盖已有文件(自动 (1),(2)…)。
/// 供 dsh-content 子 WebView 与 chat 回退窗口共用(单一实现,防漂移)。
pub fn download_target_shared(
    app: &AppHandle,
    destination: &std::path::Path,
) -> std::path::PathBuf {
    let file_name = destination
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    let downloads = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let mut target = downloads.join(&file_name);
    let mut i = 1;
    while target.exists() {
        let stem = Path::new(&file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "download".to_string());
        let ext = Path::new(&file_name)
            .extension()
            .map(|s| format!(".{}", s.to_string_lossy()))
            .unwrap_or_default();
        target = downloads.join(format!("{stem} ({i}){ext}"));
        i += 1;
    }
    target
}

// ── DshViewManager:子 WebView 生命周期与工作区状态 ────────

pub struct DshViewManager {
    /// 子 WebView 句柄(创建后持有,隐藏/显示/布局复用)。
    view: Mutex<Option<Webview<tauri::Wry>>>,
    /// 状态快照(workspace/status/url/error)。
    state: Mutex<DshViewSnapshot>,
    /// 「成功后自动进入 DeepSeek」的 pending 意图(accepted ≠ success)。
    pub pending_enter: AtomicBool,
    /// 创建中的互斥守卫(防并发重复创建)。
    creating: AtomicBool,
    /// 健康观察线程是否在运行。
    watcher_alive: AtomicBool,
    /// 顶部栏是否隐藏(全屏 + DeepSeek 工作区自动隐藏 chrome;影响子 WebView 几何)。
    topbar_hidden: AtomicBool,
}

impl Default for DshViewManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DshViewManager {
    pub fn new() -> Self {
        Self {
            view: Mutex::new(None),
            state: Mutex::new(DshViewSnapshot {
                workspace: Workspace::Launcher,
                status: DshViewStatus::NotCreated,
                url: None,
                error: None,
                pending_enter: false,
                can_back_to_launcher: false,
                can_retry: false,
                can_reconnect: false,
            }),
            pending_enter: AtomicBool::new(false),
            creating: AtomicBool::new(false),
            watcher_alive: AtomicBool::new(false),
            topbar_hidden: AtomicBool::new(false),
        }
    }

    /// 顶部栏隐藏状态(renderer 在全屏 + DeepSeek 工作区自动隐藏 chrome 时设置)。
    pub fn set_topbar_hidden(&self, hidden: bool) {
        self.topbar_hidden.store(hidden, Ordering::SeqCst);
    }

    pub fn is_topbar_hidden(&self) -> bool {
        self.topbar_hidden.load(Ordering::SeqCst)
    }

    /// 直接写入状态(不广播;测试与内部流程用)。派生字段由 current_state 计算。
    pub fn apply(&self, snap: DshViewSnapshot) {
        *self.state.lock().unwrap() = snap;
    }

    /// 广播当前状态。
    pub fn emit(&self, app: &AppHandle) {
        let snap = self.current_state();
        let _ = app.emit(EVENT_DSH_VIEW_STATE, &snap);
    }

    pub fn current_state(&self) -> DshViewSnapshot {
        let mut s = self.state.lock().unwrap().clone();
        s.pending_enter = self.pending_enter.load(Ordering::SeqCst);
        s.can_back_to_launcher = s.workspace == Workspace::Dsh;
        s.can_retry = s.status.is_terminal_failure();
        s.can_reconnect = s.status == DshViewStatus::Disconnected;
        s
    }

    pub fn view(&self) -> Option<Webview<tauri::Wry>> {
        self.view.lock().unwrap().clone()
    }

    pub fn view_exists(&self) -> bool {
        self.view.lock().unwrap().is_some()
    }

    fn store_view(&self, view: Webview<tauri::Wry>) {
        *self.view.lock().unwrap() = Some(view);
    }

    /// 尝试进入「创建中」:成功返回 true(可创建),失败说明已有创建在进行。
    fn try_begin_create(&self) -> bool {
        !self.creating.swap(true, Ordering::SeqCst)
    }

    fn end_create(&self) {
        self.creating.store(false, Ordering::SeqCst);
    }
}

/// 状态迁移合法性(防止事件竞态把状态打乱;宽松规则,失败/断线可随时进入)。
pub fn can_transition(from: DshViewStatus, to: DshViewStatus) -> bool {
    use DshViewStatus::*;
    if from == to {
        return true;
    }
    match from {
        NotCreated => matches!(to, Creating | Failed),
        Creating => matches!(to, Loading | Ready | Failed | Disconnected | NotCreated),
        Loading => matches!(to, Ready | Failed | Disconnected | Creating),
        Ready => matches!(to, Disconnected | Failed | Loading | Creating | NotCreated),
        Disconnected => matches!(to, Ready | Loading | Creating | Failed | NotCreated),
        Failed => matches!(to, Creating | Loading | Disconnected | NotCreated),
    }
}

/// 是否需要在启动成功后自动创建并进入 DeepSeek 工作区。
pub fn needs_auto_create(state: LauncherState, op_active: bool, pending: bool) -> bool {
    pending && state == LauncherState::Running && !op_active
}

/// 子 WebView 已就绪但工作区还在启动器时,是否应翻转工作区。
pub fn needs_workspace_flip(workspace: Workspace, view_ready: bool, pending: bool) -> bool {
    pending && view_ready && workspace != Workspace::Dsh
}

/// 把新状态写入管理器(不合法迁移会被忽略并告警)。
fn transition(app: &AppHandle, to: DshViewStatus, error: Option<String>) {
    let mgr = app.state::<Arc<DshViewManager>>();
    let mut cur = mgr.current_state();
    if !can_transition(cur.status, to) {
        log::warn!("忽略非法的 dsh 视图状态迁移 {:?} → {to:?}", cur.status);
        return;
    }
    cur.status = to;
    cur.error = error;
    if to == DshViewStatus::Failed {
        mgr.pending_enter.store(false, Ordering::SeqCst);
    }
    mgr.apply(cur);
    mgr.emit(app);
}

// ── 创建 / 布局 / 进入 ────────────────────────────────────

/// 创建子 WebView(幂等)。调用方必须在后台线程/异步 command(Windows 线程约束)。
/// add_child 内部会把构建派发到主线程并等待结果,同步 command 中调用会死锁。
pub fn create_dsh_view(app: &AppHandle) -> Result<(), String> {
    let mgr = app.state::<Arc<DshViewManager>>();
    if mgr.view_exists() {
        return Ok(());
    }
    if !mgr.try_begin_create() {
        // 已有创建在进行:幂等返回,不重复创建。
        return Ok(());
    }

    let result = (|| -> Result<(), String> {
        let url = dsh_url();
        let port = config::load().port;
        // 固定本地 UDF(WebView2 User Data Folder),保持 origin 与登录态稳定。
        let udf = state_dir().join("dsh-webview2");
        std::fs::create_dir_all(&udf).map_err(|e| format!("创建 UDF 失败:{e}"))?;

        let window = app
            .get_window("main")
            .ok_or_else(|| "主窗口不存在,无法创建 DeepSeek 工作区".to_string())?;
        let scale = window.scale_factor().unwrap_or(1.0);
        let inner = window
            .inner_size()
            .map_err(|e| format!("读取主窗口尺寸失败:{e}"))?;
        let (pos, size) = dsh_view_geometry(inner, scale, mgr.is_topbar_hidden());

        let mut builder = WebviewBuilder::new(
            DSH_VIEW_LABEL,
            WebviewUrl::External(url.parse().map_err(|e| format!("URL 非法:{e}"))?),
        )
        .data_directory(udf)
        .devtools(cfg!(debug_assertions))
        // HTML5 拖放交给 DSH 页面,禁用 Tauri 原生拖放处理器
        .disable_drag_drop_handler();

        // 导航策略:只允许精确 loopback origin;外链 → 系统浏览器;未知协议拦截
        builder = builder.on_navigation(move |url: &url::Url| {
            let ok = loopback_allowed(url, port);
            if !ok && matches!(url.scheme(), "http" | "https") {
                let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            }
            ok
        });
        builder = builder.on_new_window(move |url, _features| {
            if matches!(url.scheme(), "http" | "https") {
                let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            }
            tauri::webview::NewWindowResponse::Deny
        });

        // 下载:默认下载目录 + 不覆盖已有文件
        let app4 = app.clone();
        builder = builder.on_download(move |_webview, event| {
            if let tauri::webview::DownloadEvent::Requested { destination, .. } = event {
                let target = download_target_shared(&app4, destination);
                log::info!("dsh 下载 → {}", target.display());
                *destination = target;
            }
            true
        });

        // 页面加载完成:健康确认后进入 Ready(后台线程,不阻塞 webview 事件线程)
        let app5 = app.clone();
        builder = builder.on_page_load(move |_view, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                app5.state::<Arc<AppState>>()
                    .timings
                    .mark("dsh_load_finished");
                app5.state::<Arc<AppState>>().emit_perf(&app5);
                confirm_and_mark_ready(&app5);
            }
        });

        let view = window
            .add_child(builder, pos, size)
            .map_err(|e| format!("创建 dsh-content 子 WebView 失败:{e}"))?;
        mgr.store_view(view.clone());
        transition(app, DshViewStatus::Loading, None);
        app.state::<Arc<AppState>>()
            .timings
            .mark("dsh_view_created");
        app.state::<Arc<AppState>>().emit_perf(app);
        // add_child 后先保持隐藏。只有页面加载完成且健康检查通过进入 Ready，
        // 才允许覆盖主 WebView；否则启动器/内嵌加载卡必须保持可见。
        let _ = view.hide();
        start_watcher(app);
        Ok(())
    })();

    mgr.end_create();
    if let Err(e) = &result {
        log::error!("创建 dsh-content 失败:{e}");
        transition(app, DshViewStatus::Failed, Some(e.clone()));
    }
    result
}

/// 页面加载完成后:健康确认(最多 10s)→ Ready + 进入 DeepSeek 工作区(若意图/已在 dsh)。
fn confirm_and_mark_ready(app: &AppHandle) {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let port = config::load().port;
        let expected = app2.state::<Arc<AppState>>().supervisor.web_pid();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if dsh_health_check(port, expected) {
                break;
            }
            if Instant::now() > deadline {
                // 交给健康观察线程兜底(可能标记断线/重连)
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let mgr = app2.state::<Arc<DshViewManager>>();
        let mut s = mgr.current_state();
        if s.status == DshViewStatus::Loading || s.status == DshViewStatus::Disconnected {
            s.status = DshViewStatus::Ready;
            s.error = None;
            let pending = mgr.pending_enter.load(Ordering::SeqCst);
            if pending || s.workspace == Workspace::Dsh {
                s.workspace = Workspace::Dsh;
                mgr.pending_enter.store(false, Ordering::SeqCst);
            }
            mgr.apply(s);
            mgr.emit(&app2);
            if let Some(v) = mgr.view() {
                if mgr.current_state().workspace == Workspace::Dsh {
                    let _ = v.show();
                    let _ = v.set_focus();
                }
            }
            app2.state::<Arc<AppState>>().timings.mark("dsh_view_ready");
            app2.state::<Arc<AppState>>().emit_perf(&app2);
        }
    });
}

/// 窗口尺寸/位置/DPI 变化时重排子 WebView(标题栏以下全部区域;顶部栏隐藏时全窗)。
pub fn relayout(app: &AppHandle) {
    let Some(mgr) = app.try_state::<Arc<DshViewManager>>() else {
        return;
    };
    if !mgr.view_exists() {
        return;
    }
    let Some(window) = app.get_window("main") else {
        return;
    };
    let Ok(inner) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let (pos, size) = dsh_view_geometry(inner, scale, mgr.is_topbar_hidden());
    if let Some(v) = mgr.view() {
        let _ = v.set_position(pos);
        let _ = v.set_size(size);
    }
}

/// 设置顶部栏隐藏状态并重排子 WebView(renderer 全屏自动隐藏 chrome 时调用)。
pub fn set_topbar_hidden(app: &AppHandle, hidden: bool) {
    let Some(mgr) = app.try_state::<Arc<DshViewManager>>() else {
        return;
    };
    mgr.set_topbar_hidden(hidden);
    relayout(app);
}

// ── 进入 / 返回 / 重试(工作区语义) ──────────────────────

/// 打开 DeepSeek 工作区(命令与托盘共用入口):
/// - 已在 dsh 且就绪 → 幂等返回;
/// - 服务未就绪 → 立即显示加载状态并异步启动服务,成功后自动切换;
/// - 失败/取消 → status Failed(前端显示错误与重试入口),绝不进入空白页面。
pub fn open_dsh_workspace(app: &AppHandle) -> DshViewSnapshot {
    let mgr = app.state::<Arc<DshViewManager>>();
    mgr.pending_enter.store(true, Ordering::SeqCst);
    let cur = mgr.current_state();
    // 已在 dsh 且就绪 → 幂等返回
    if cur.workspace == Workspace::Dsh && cur.status == DshViewStatus::Ready {
        if let Some(v) = mgr.view() {
            let _ = v.show();
            let _ = v.set_focus();
        }
        return mgr.current_state();
    }
    // 视图已就绪但工作区在启动器 → 直接切回(会话保持,不销毁、不刷新)
    if mgr.view_exists() && cur.status == DshViewStatus::Ready {
        mgr.pending_enter.store(false, Ordering::SeqCst);
        let mut s = cur;
        s.workspace = Workspace::Dsh;
        s.status = DshViewStatus::Ready;
        s.error = None;
        mgr.apply(s);
        mgr.emit(app);
        if let Some(v) = mgr.view() {
            let _ = v.show();
            let _ = v.set_focus();
        }
        return mgr.current_state();
    }
    let status = if cur.workspace == Workspace::Dsh {
        cur.status
    } else {
        DshViewStatus::Creating
    };
    let mut s = cur;
    s.workspace = Workspace::Dsh;
    s.status = status;
    s.url = Some(dsh_url());
    s.error = if status == DshViewStatus::Creating {
        Some("正在启动 DSH 服务并加载 DeepSeek 界面,就绪后自动进入…".into())
    } else {
        s.error
    };
    mgr.apply(s);
    mgr.emit(app);
    let app2 = app.clone();
    std::thread::spawn(move || ensure_and_enter(&app2));
    mgr.current_state()
}

/// 返回启动器工作区:隐藏子 WebView,但保留其页面状态/会话(不销毁、不刷新)。
pub fn back_to_launcher(app: &AppHandle) -> DshViewSnapshot {
    let mgr = app.state::<Arc<DshViewManager>>();
    // 用户明确返回即取消本轮自动进入意图，避免服务稍后就绪又把界面拉回。
    mgr.pending_enter.store(false, Ordering::SeqCst);
    let mut s = mgr.current_state();
    s.workspace = Workspace::Launcher;
    s.error = None;
    mgr.apply(s);
    mgr.emit(app);
    if let Some(v) = mgr.view() {
        let _ = v.hide();
    }
    mgr.current_state()
}

/// 重试/重连:清空失败状态,确保服务运行并重建(或恢复)子 WebView。
pub fn retry_dsh_view(app: &AppHandle) -> DshViewSnapshot {
    let mgr = app.state::<Arc<DshViewManager>>();
    mgr.pending_enter.store(true, Ordering::SeqCst);
    let mut s = mgr.current_state();
    s.workspace = Workspace::Dsh;
    s.status = DshViewStatus::Creating;
    s.url = Some(dsh_url());
    s.error = Some("正在重新连接 DeepSeek…".into());
    mgr.apply(s);
    mgr.emit(app);
    let app2 = app.clone();
    std::thread::spawn(move || ensure_and_enter(&app2));
    mgr.current_state()
}

/// 已有子 WebView 但不在 Ready(断线/失败/创建中残留):重新导航回 DSH。
/// 返回是否执行了重连(视图存在且需要恢复)。
fn reconnect_existing_view(app: &AppHandle) -> bool {
    let mgr = app.state::<Arc<DshViewManager>>();
    let cur = mgr.current_state();
    if !mgr.view_exists() || cur.status == DshViewStatus::Ready {
        return false;
    }
    if let Some(v) = mgr.view() {
        let url = dsh_url();
        let _ = v.navigate(url.parse().unwrap());
    }
    let mut s = cur;
    s.status = DshViewStatus::Loading;
    s.error = None;
    mgr.apply(s);
    mgr.emit(app);
    true
}

/// 确保服务就绪并进入 dsh 工作区:
/// - 已 running + 健康 → 复用/重建子 WebView(断线则重连);
/// - 否则发起 start 流程;成功后由 maybe_auto_enter(挂在 set_snapshot)接管。
fn ensure_and_enter(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();
    let snap = state.snapshot();
    let port = config::load().port;
    let expected = state.supervisor.web_pid();
    if snap.state == LauncherState::Running && dsh_health_check(port, expected) {
        if !reconnect_existing_view(app) {
            if let Err(e) = create_dsh_view(app) {
                transition(app, DshViewStatus::Failed, Some(e));
            }
        }
        return;
    }
    let res = state.run_action(app, "start");
    if !res.ok {
        let reason = res.reason.unwrap_or_else(|| "DSH 服务启动失败".to_string());
        transition(app, DshViewStatus::Failed, Some(reason));
    }
    // accepted ≠ success:真正成功由操作终态 + maybe_auto_enter 决定。
}

/// 启动成功后自动进入 DeepSeek 工作区(在每次 set_snapshot 尾部调用)。
/// 触发条件(全部满足才动作):
/// 1. 存在 pending 意图;2. 状态机到达 Running(真实成功终态);
/// 3. 无进行中操作;4. 健康检查通过;5. 子 WebView 已加载/就绪。
///
/// 不满足时保持启动器状态,绝不提前显示成功。
pub fn maybe_auto_enter(app: &AppHandle) {
    let Some(mgr) = app.try_state::<Arc<DshViewManager>>() else {
        return;
    };
    if !mgr.pending_enter.load(Ordering::SeqCst) {
        return;
    }
    let state = app.state::<Arc<AppState>>();
    let snap = state.snapshot();
    if !needs_auto_create(snap.state, snap.operation.is_some(), true) {
        return;
    }
    let cur = mgr.current_state();
    let view_ready = cur.status == DshViewStatus::Ready;
    if needs_workspace_flip(cur.workspace, view_ready, true) {
        // 视图已就绪:翻转到 dsh 工作区并清空意图。
        mgr.pending_enter.store(false, Ordering::SeqCst);
        let mut s = mgr.current_state();
        s.workspace = Workspace::Dsh;
        s.error = None;
        mgr.apply(s);
        mgr.emit(app);
        if let Some(v) = mgr.view() {
            let _ = v.show();
            let _ = v.set_focus();
        }
        return;
    }
    if view_ready {
        // 已在 dsh 且就绪:清空意图即可。
        mgr.pending_enter.store(false, Ordering::SeqCst);
        return;
    }
    // 需要创建/恢复视图:后台线程先健康确认(≤40s),再创建或重连。
    let app2 = app.clone();
    std::thread::spawn(move || {
        let port = config::load().port;
        let expected = app2.state::<Arc<AppState>>().supervisor.web_pid();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if dsh_health_check(port, expected) {
                break;
            }
            if Instant::now() > deadline {
                // 超时:启动器动作保持在启动器；若用户已主动打开工作区，
                // 则展示可重试的内嵌错误卡，不能永久卡在 Creating。
                transition(
                    &app2,
                    DshViewStatus::Failed,
                    Some("DSH 服务已运行，但 DeepSeek 工作区健康检查超时".into()),
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !reconnect_existing_view(&app2) {
            let _ = create_dsh_view(&app2);
        }
    });
}

// ── 健康观察(自动重连) ──────────────────────────────────

/// 健康观察:子 WebView 存在期间每 5s 探测;
/// DSH 死亡 → Disconnected + 隐藏子视图(前端显示断线状态);
/// 恢复 → 导航回 DSH + Ready + 显示(自动重连,不静默开浏览器)。
fn start_watcher(app: &AppHandle) {
    let mgr = app.state::<Arc<DshViewManager>>();
    if mgr.watcher_alive.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    let mgr2 = Arc::clone(&*mgr);
    std::thread::spawn(move || {
        let port = config::load().port;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            if !mgr2.view_exists() {
                mgr2.watcher_alive.store(false, Ordering::SeqCst);
                return;
            }
            let expected = app2.state::<Arc<AppState>>().supervisor.web_pid();
            let ok = dsh_health_check(port, expected);
            let cur = mgr2.current_state();
            if ok {
                if cur.status == DshViewStatus::Disconnected || cur.status == DshViewStatus::Failed
                {
                    // 恢复:保持隐藏并重新导航；必须等 on_page_load + 健康确认
                    // 后才能进入 Ready，不能仅凭服务健康就提前显示空白 WebView。
                    if let Some(v) = mgr2.view() {
                        let _ = v.hide();
                        let url = dsh_url();
                        let _ = v.navigate(url.parse().unwrap());
                    }
                    let mut s = mgr2.current_state();
                    s.status = DshViewStatus::Loading;
                    s.error = None;
                    mgr2.apply(s);
                    mgr2.emit(&app2);
                } else if cur.workspace == Workspace::Dsh {
                    // 就绪且在工作区:确保可见(窗口恢复/最小化还原后)。
                    if let Some(v) = mgr2.view() {
                        let _ = v.show();
                    }
                }
            } else if cur.status == DshViewStatus::Ready || cur.status == DshViewStatus::Loading {
                // 服务死亡 → 断线状态(不静默打开浏览器)。
                // 覆盖 Loading:页面加载失败但服务已死的场景,避免卡在加载态。
                let mut s = mgr2.current_state();
                s.status = DshViewStatus::Disconnected;
                s.error = Some("DSH 服务已停止或失去响应".into());
                mgr2.apply(s);
                mgr2.emit(&app2);
                if let Some(v) = mgr2.view() {
                    let _ = v.hide();
                }
            }
        }
    });
}

/// 程序启动时若发现受管 DSH 已正常运行,自动恢复并进入 DeepSeek 工作区。
pub fn maybe_enter_on_boot(app: &AppHandle) {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let state = app2.state::<Arc<AppState>>();
        let snap = state.snapshot();
        let port = config::load().port;
        let expected = state.supervisor.web_pid();
        if snap.state != LauncherState::Running {
            return;
        }
        if !dsh_health_check(port, expected) {
            return;
        }
        let mgr = app2.state::<Arc<DshViewManager>>();
        mgr.pending_enter.store(true, Ordering::SeqCst);
        let mut s = mgr.current_state();
        s.workspace = Workspace::Dsh;
        s.status = DshViewStatus::Creating;
        s.url = Some(dsh_url());
        s.error = Some("正在打开 DeepSeek 工作区…".into());
        mgr.apply(s);
        mgr.emit(&app2);
        let _ = create_dsh_view(&app2);
    });
}

/// 退出前清理(可选;窗口销毁时子 WebView 随窗口销毁)。
pub fn teardown(app: &AppHandle) {
    let Some(mgr) = app.try_state::<Arc<DshViewManager>>() else {
        return;
    };
    if let Some(v) = mgr.view.lock().unwrap().take() {
        let _ = v.close();
    }
    mgr.apply(DshViewSnapshot {
        workspace: Workspace::Launcher,
        status: DshViewStatus::NotCreated,
        url: None,
        error: None,
        pending_enter: false,
        can_back_to_launcher: false,
        can_retry: false,
        can_reconnect: false,
    });
}

/// WebviewBuilder 类型别名(与 tauri 运行时解耦的缩写)。
type WebviewBuilder = tauri::webview::WebviewBuilder<tauri::Wry>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_titlebar_offset_scale_1() {
        let (pos, size) = dsh_view_geometry(PhysicalSize::new(1000, 700), 1.0, false);
        assert_eq!(pos.x, 0.0);
        assert_eq!(pos.y, TITLEBAR_HEIGHT);
        assert_eq!(size.width, 1000.0);
        assert_eq!(size.height, 700.0 - TITLEBAR_HEIGHT);
    }

    #[test]
    fn geometry_scale_2_keeps_logical_math() {
        // 2x Retina:物理 2000x1400 → 逻辑 1000x700,标题栏 64 逻辑 px。
        let (pos, size) = dsh_view_geometry(PhysicalSize::new(2000, 1400), 2.0, false);
        assert_eq!(pos.y, TITLEBAR_HEIGHT);
        assert_eq!(size.width, 1000.0);
        assert_eq!(size.height, 700.0 - TITLEBAR_HEIGHT);
    }

    #[test]
    fn geometry_non_integer_scale_1_25() {
        let (pos, size) = dsh_view_geometry(PhysicalSize::new(1250, 780), 1.25, false);
        assert_eq!(pos.y, TITLEBAR_HEIGHT);
        assert_eq!(size.width, 1000.0);
        assert_eq!(size.height, 624.0 - TITLEBAR_HEIGHT);
    }

    #[test]
    fn geometry_clamps_negative_height_and_bad_scale() {
        let (_, size) = dsh_view_geometry(PhysicalSize::new(100, 40), 1.0, false);
        assert_eq!(size.height, 0.0);
        let (_, size2) = dsh_view_geometry(PhysicalSize::new(1000, 700), 0.0, false);
        assert_eq!(size2.height, 700.0 - TITLEBAR_HEIGHT);
    }

    #[test]
    fn geometry_topbar_hidden_fills_window() {
        // 顶部栏隐藏(全屏 DeepSeek 工作区):子 WebView 从 y=0 占满整个窗口。
        let (pos, size) = dsh_view_geometry(PhysicalSize::new(1000, 700), 1.0, true);
        assert_eq!(pos.y, 0.0);
        assert_eq!(size.width, 1000.0);
        assert_eq!(size.height, 700.0);
        // Retina 2x:物理 2000x1400 → 逻辑 1000x700 全窗。
        let (pos2, size2) = dsh_view_geometry(PhysicalSize::new(2000, 1400), 2.0, true);
        assert_eq!(pos2.y, 0.0);
        assert_eq!(size2.height, 700.0);
    }

    #[test]
    fn manager_topbar_hidden_flag() {
        let mgr = DshViewManager::new();
        assert!(!mgr.is_topbar_hidden(), "默认顶部栏可见");
        mgr.set_topbar_hidden(true);
        assert!(mgr.is_topbar_hidden());
        mgr.set_topbar_hidden(false);
        assert!(!mgr.is_topbar_hidden());
    }

    #[test]
    fn loopback_policy_only_exact_origin_port() {
        let port = 3080;
        let ok = |s: &str| loopback_allowed(&url::Url::parse(s).unwrap(), port);
        assert!(ok("http://127.0.0.1:3080/"));
        assert!(ok("http://localhost:3080/some/path?q=1"));
        assert!(!ok("http://127.0.0.1:3081/"), "端口必须一致");
        assert!(!ok("http://127.0.0.2:3080/"), "非 loopback 拒绝");
        assert!(!ok("https://127.0.0.1:3080/"), "https 拒绝(只允许 http)");
        assert!(!ok("http://example.com:3080/"), "外链拒绝");
    }

    #[test]
    fn dsh_url_always_uses_loopback_even_when_service_binds_all_interfaces() {
        let _guard = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-view-url-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", &base);
        std::fs::write(
            base.join("dsh-launcher.json"),
            r#"{"host":"0.0.0.0","port":43123}"#,
        )
        .unwrap();

        assert_eq!(dsh_url(), "http://127.0.0.1:43123/");

        std::env::remove_var("DSH_LAUNCHER_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn health_check_false_when_port_closed() {
        // 无监听端口 → 必须 false(不能只靠端口开放判定,这里端口都没开)
        assert!(!dsh_health_check(1, None));
    }

    #[test]
    fn error_page_is_data_url_with_retry() {
        let u = error_page_url("http://127.0.0.1:3080/");
        assert!(u.starts_with("data:text/html"), "{u}");
        assert!(u.contains("%E9%87%8D%E8%AF%95"), "错误页必须提供重试按钮");
        assert!(
            !u.contains("__TAURI_INTERNALS__"),
            "错误页不得依赖 Tauri IPC"
        );
        assert!(
            u.contains("http%3A%2F%2F127.0.0.1%3A3080"),
            "重试应指向 DSH 回环地址"
        );
    }

    #[test]
    fn urlencoding_basic() {
        assert_eq!(urlencoding("a b/c"), "a+b%2Fc");
    }

    #[test]
    fn state_transitions_legal_and_illegal() {
        use DshViewStatus::*;
        assert!(can_transition(NotCreated, Creating));
        assert!(can_transition(Creating, Loading));
        assert!(can_transition(Creating, Failed));
        assert!(can_transition(Loading, Ready));
        assert!(can_transition(Loading, Disconnected));
        assert!(can_transition(Ready, Disconnected));
        assert!(can_transition(Disconnected, Ready));
        assert!(can_transition(Failed, Creating));
        // 非法:未创建直接 Ready / Ready 回 NotCreated 之外的越级
        assert!(!can_transition(NotCreated, Ready));
        assert!(!can_transition(NotCreated, Disconnected));
    }

    #[test]
    fn manager_state_mutations_and_derived_flags() {
        let mgr = DshViewManager::new();
        assert_eq!(mgr.current_state().workspace, Workspace::Launcher);
        assert_eq!(mgr.current_state().status, DshViewStatus::NotCreated);

        mgr.pending_enter.store(true, Ordering::SeqCst);
        let mut s = mgr.current_state();
        s.workspace = Workspace::Dsh;
        s.status = DshViewStatus::Creating;
        mgr.apply(s);

        let cur = mgr.current_state();
        assert_eq!(cur.workspace, Workspace::Dsh);
        assert!(cur.pending_enter, "派生:原子意图写入快照");
        assert!(cur.can_back_to_launcher, "dsh 工作区必须可返回启动器");
        assert!(!cur.can_retry);

        let mut s2 = mgr.current_state();
        s2.status = DshViewStatus::Failed;
        mgr.apply(s2);
        let cur2 = mgr.current_state();
        assert!(cur2.can_retry, "终态失败可重试");

        let mut s3 = mgr.current_state();
        s3.status = DshViewStatus::Disconnected;
        mgr.apply(s3);
        let cur3 = mgr.current_state();
        assert!(cur3.can_reconnect, "断线可重连");
    }

    #[test]
    fn auto_enter_predicates() {
        // accepted ≠ success:操作进行中 / 非 running → 不进入
        assert!(!needs_auto_create(LauncherState::Starting, false, true));
        assert!(!needs_auto_create(LauncherState::Running, true, true));
        assert!(!needs_auto_create(LauncherState::Running, false, false));
        // 失败/取消/中断终态后状态不会是 running
        assert!(!needs_auto_create(LauncherState::Failed, false, true));
        assert!(!needs_auto_create(LauncherState::Idle, false, true));
        // 真实成功:running + 无进行中操作 + pending
        assert!(needs_auto_create(LauncherState::Running, false, true));

        // 视图已就绪 → 只需翻转工作区
        assert!(needs_workspace_flip(Workspace::Launcher, true, true));
        assert!(!needs_workspace_flip(Workspace::Dsh, true, true));
        assert!(!needs_workspace_flip(Workspace::Launcher, false, true));
    }

    #[test]
    fn create_guard_is_idempotent() {
        let mgr = DshViewManager::new();
        assert!(mgr.try_begin_create(), "第一次创建成功占位");
        assert!(!mgr.try_begin_create(), "并发第二次必须被拒绝(幂等)");
        mgr.end_create();
        assert!(mgr.try_begin_create(), "结束后可再次创建");
        mgr.end_create();
    }

    /// capability 隔离检查:读取 capabilities/default.json,
    /// 断言 dsh-content 不在任何 windows/webviews 列表,且没有 remote 授权。
    #[test]
    fn capability_isolation_excludes_dsh_content() {
        let raw = std::fs::read_to_string("capabilities/default.json")
            .expect("capabilities/default.json 必须存在");
        let v: serde_json::Value =
            serde_json::from_str(&raw).expect("capabilities/default.json 必须是合法 JSON");

        // 单个 capability 对象
        let windows = v
            .get("windows")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let webviews = v
            .get("webviews")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();

        let listed: Vec<&str> = windows
            .iter()
            .chain(webviews.iter())
            .filter_map(|x| x.as_str())
            .collect();
        assert!(
            !listed.contains(&DSH_VIEW_LABEL),
            "dsh-content 不得出现在任何 capability 的 windows/webviews 列表:{listed:?}"
        );
        // 主窗口权限必须精确到 main webview(而不是整窗口,否则子 WebView 也会继承)
        assert!(
            webviews.iter().any(|w| w.as_str() == Some("main")),
            "capability 必须使用 webviews:[\"main\"] 精确匹配主 WebView:{listed:?}"
        );
        assert!(
            !windows.iter().any(|w| w.as_str() == Some("main")),
            "不得用 windows:[\"main\"] 匹配整窗口(会授权给 dsh-content)"
        );
        assert!(
            v.get("remote").is_none(),
            "不允许配置 remote 授权(远程 loopback 页面零权限)"
        );
    }
}
