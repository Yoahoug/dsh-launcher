// dsh-launcher · 应用状态与动作协调器(Rust 原生核心,无 HTTP/轮询)
// 唯一状态机:所有动作从这里发起;epoch 递增使进行中的旧流程放弃(不能回写状态);
// 状态变化直接 emit Tauri 事件。
use crate::config;
use crate::contract::{
    ActionAccepted, AppSnapshot, DesktopPreferences, DesktopSnapshot, EnvironmentSnapshot,
    ErrorSummary, LauncherMode, LauncherState, LogPage, RepoSnapshot, SettingsSnapshot,
    UpdateResult, EVENT_STATE_CHANGED,
};
use crate::log_hub::LogHub;
use crate::preferences;
use crate::services::repo::RepoService;
use crate::services::runtime::{self, Tools};
use crate::services::supervisor::Supervisor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct AppState {
    pub log_hub: Arc<LogHub>,
    pub supervisor: Arc<Supervisor>,
    pub tools: Mutex<Tools>,
    /// 流程纪元:stop/cancel 递增;旧流程检测到变化即放弃。
    pub epoch: AtomicU64,
    /// 是否有流程在执行(串行化破坏性动作)。
    pub flow_active: AtomicBool,
    pub snapshot: Mutex<AppSnapshot>,
    pub preferences: Mutex<DesktopPreferences>,
    pub boot_error: Mutex<Option<String>>,
    #[allow(dead_code)]
    pub quit_requested: AtomicBool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 更新快照并返回独立副本。调用方拿到结果时互斥锁必须已经释放，
/// 后续事件广播、托盘菜单构建等操作才可以安全地再次读取状态。
fn apply_snapshot_update(
    snapshot: &Mutex<AppSnapshot>,
    partial: impl FnOnce(&mut AppSnapshot),
) -> AppSnapshot {
    let mut snap = snapshot.lock().unwrap();
    partial(&mut snap);
    snap.clone()
}

impl AppState {
    pub fn new(log_hub: Arc<LogHub>, supervisor: Arc<Supervisor>, tools: Tools) -> Self {
        let prefs = preferences::load_and_migrate();
        Self {
            log_hub,
            supervisor,
            tools: Mutex::new(tools),
            epoch: AtomicU64::new(1),
            flow_active: AtomicBool::new(false),
            snapshot: Mutex::new(AppSnapshot::mock_idle()),
            preferences: Mutex::new(prefs),
            boot_error: Mutex::new(None),
            quit_requested: AtomicBool::new(false),
        }
    }

    // ── 快照 ─────────────────────────────────────────────

    /// 用部分字段更新快照并广播事件。
    pub fn set_snapshot(&self, app: &AppHandle, partial: impl FnOnce(&mut AppSnapshot)) {
        // 先完成状态更新并释放锁。托盘刷新会再次读取 snapshot，若持锁调用会自锁，
        // 表现为 UI 已收到 starting 事件、随后启动流程和设置 IPC 永久卡住。
        let snap = apply_snapshot_update(&self.snapshot, partial);
        let _ = app.emit(EVENT_STATE_CHANGED, &snap);
        crate::tray::refresh(app);
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let mut snap = self.snapshot.lock().unwrap().clone();
        if let Some(err) = self.boot_error.lock().unwrap().clone() {
            snap.error = Some(ErrorSummary {
                summary: "桌面核心启动失败".into(),
                detail: err,
            });
        }
        snap
    }

    /// 刷新仓库状态快照(动作前后调用)。
    pub fn refresh_repo(&self) {
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            let mut snap = self.snapshot.lock().unwrap();
            snap.repo = RepoSnapshot {
                branch: String::new(),
                head: String::new(),
                behind: -1,
                ahead: -1,
                dirty: false,
                dirty_files: 0,
                sync_at: None,
                remote_up_to_date: true,
            };
            return;
        }
        // Git/TCC 文件访问可能耗时，不能在外部命令执行期间占用 snapshot；
        // 否则启动/停止动作会在 set_snapshot 中被无关的仓库探测阻塞。
        let sync_at = self.snapshot.lock().unwrap().repo.sync_at;
        let repo = RepoService::new(self.log_hub.clone(), self.tools.lock().unwrap().clone());
        let repo_snapshot = repo.status(&settings.repo_path, sync_at);
        self.snapshot.lock().unwrap().repo = repo_snapshot;
    }

    pub fn refresh_repo_emit(&self, app: &AppHandle) {
        self.refresh_repo();
        let snap = self.snapshot.lock().unwrap().clone();
        let _ = app.emit(EVENT_STATE_CHANGED, &snap);
    }

    // ── 桌面信息 ─────────────────────────────────────────

    pub fn desktop_snapshot(&self) -> DesktopSnapshot {
        let settings = config::load();
        let first_run_done =
            !settings.repo_path.is_empty() && config::repo_usable(&settings.repo_path).ok;
        DesktopSnapshot {
            preferences: self.preferences.lock().unwrap().clone(),
            first_run_done,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn save_preferences(
        &self,
        prefs: &DesktopPreferences,
    ) -> Result<DesktopPreferences, String> {
        let saved = preferences::save_validated(prefs)?;
        *self.preferences.lock().unwrap() = saved.clone();
        Ok(saved)
    }

    // ── 日志 ─────────────────────────────────────────────

    pub fn logs(&self, since_id: u64) -> LogPage {
        self.log_hub.snapshot(since_id)
    }

    pub fn clear_logs(&self) {
        self.log_hub.clear();
    }

    // ── 设置 ─────────────────────────────────────────────

    pub fn settings(&self) -> SettingsSnapshot {
        config::load()
    }

    pub fn save_settings(&self, patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
        config::apply_patch(patch)
    }

    // ── 环境 ─────────────────────────────────────────────

    pub fn environment(&self) -> EnvironmentSnapshot {
        let settings = config::load();
        let tools = self.tools.lock().unwrap().clone();
        let mut warnings = Vec::new();
        if tools.pnpm.is_none() {
            warnings.push(
                "未找到 pnpm,请先安装(brew install pnpm,或 corepack enable);「启动/开发模式/更新并构建」需要它"
                    .into(),
            );
        }
        if tools.git.is_none() {
            warnings.push("未找到 git,「更新并构建」不可用".into());
        }
        let dsh_node = runtime::resolve_dsh_node();
        if dsh_node.is_none() {
            warnings.push(
                "未找到 dsh 要求版本范围(^22.19 || >=24)的 Node;开发模式与构建将不可用,可在设置中安装托管 Node"
                    .to_string(),
            );
        }
        EnvironmentSnapshot {
            repo_path: settings.repo_path.clone(),
            repo_usable: config::repo_usable(&settings.repo_path),
            dist_built: config::dist_built(&settings.repo_path),
            node: crate::contract::EnvironmentNode {
                current: dsh_node
                    .as_ref()
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
                in_range: dsh_node.is_some(),
                used: dsh_node.as_ref().map(|(p, _)| p.display().to_string()),
                used_version: dsh_node.as_ref().map(|(_, v)| v.clone()),
                used_source: dsh_node.as_ref().map(|_| "系统安装/托管".to_string()),
            },
            pnpm: tools.pnpm.map(|p| {
                p.parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| p.display().to_string())
            }),
            git: tools.git.map(|g| g.display().to_string()),
            warnings,
        }
    }

    // ── 动作协调 ─────────────────────────────────────────

    /// 动作入口:长流程在后台线程执行,立即返回接受。
    pub fn run_action(self: &Arc<Self>, app: &AppHandle, action: &str) -> ActionAccepted {
        let allowed = [
            "start",
            "dev",
            "update",
            "stop",
            "rebuild",
            "install-node",
            "clear",
            "check-update",
            "apply-update",
            "detach",
        ];
        if !allowed.contains(&action) {
            return ActionAccepted {
                ok: false,
                reason: Some(format!("未知动作 {action}")),
                aborted: None,
                already: None,
            };
        }
        if action == "clear" {
            self.clear_logs();
            return ActionAccepted {
                ok: true,
                reason: None,
                aborted: None,
                already: None,
            };
        }
        if action == "stop" {
            // stop 不需要流程锁:epoch++ 让旧流程放弃 + 停止全部进程树
            self.epoch.fetch_add(1, Ordering::SeqCst);
            let app = app.clone();
            let me = self.clone();
            std::thread::spawn(move || me.stop_flow(&app));
            return ActionAccepted {
                ok: true,
                reason: None,
                aborted: None,
                already: None,
            };
        }
        if action == "detach" {
            self.supervisor.detach();
            return ActionAccepted {
                ok: true,
                reason: None,
                aborted: None,
                already: None,
            };
        }
        if action == "check-update" || action == "apply-update" {
            return ActionAccepted {
                ok: false,
                reason: Some("自动更新由桌面版内置 updater 提供".into()),
                aborted: None,
                already: None,
            };
        }

        // 破坏性/长流程:串行化
        if self.flow_active.swap(true, Ordering::SeqCst) {
            return ActionAccepted {
                ok: false,
                reason: Some("busy".into()),
                aborted: None,
                already: None,
            };
        }
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        let app = app.clone();
        let me = self.clone();
        let action = action.to_string();
        std::thread::spawn(move || {
            let result = match action.as_str() {
                "start" => me.start_flow(&app, "normal"),
                "dev" => me.start_flow(&app, "dev"),
                "update" => me.update_flow(&app),
                "rebuild" => me.rebuild_flow(&app),
                "install-node" => me.install_node_flow(&app),
                _ => Ok(()),
            };
            let _ = (result, my_epoch);
            me.flow_active.store(false, Ordering::SeqCst);
        });
        ActionAccepted {
            ok: true,
            reason: None,
            aborted: None,
            already: None,
        }
    }

    // ── 流程实现 ─────────────────────────────────────────

    fn epoch_ok(&self, my_epoch: u64) -> bool {
        self.epoch.load(Ordering::SeqCst) == my_epoch
    }

    /// 打开浏览器。
    fn open_browser(&self, url: &str) {
        let settings = config::load();
        if !settings.open_browser {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Ok,
                &format!("(openBrowser 已关闭,跳过自动打开)→ {url}"),
            );
            return;
        }
        if let Err(e) = tauri_plugin_opener::open_url(url, None::<&str>) {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Warn,
                &format!("打开浏览器失败:{e}"),
            );
        } else {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Ok,
                &format!("自动打开浏览器 → {url}"),
            );
        }
    }

    /// 端口占用诊断。
    fn port_diag(&self, port: u16) -> String {
        match crate::services::supervisor::port_holder_pid(port) {
            Some(pid) => format!("占用进程 PID {pid}"),
            None => "占用进程未知".into(),
        }
    }

    fn fail(&self, app: &AppHandle, summary: &str, detail: &str) {
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Err,
            &format!("失败:{summary} — {detail}"),
        );
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Failed;
            s.error = Some(ErrorSummary {
                summary: summary.into(),
                detail: detail.into(),
            });
            s.busy = false;
            s.phase = String::new();
        });
    }

    /// 启动 dsh web(normal/dev),等待就绪/超时/早退。
    fn launch_web(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: &str,
        my_epoch: u64,
    ) -> Result<(), String> {
        let settings = config::load();
        let tools = self.tools.lock().unwrap().clone();
        let pid = self.supervisor.spawn_web(
            &tools,
            &settings.repo_path,
            settings.port,
            &settings.host,
            &settings.dsh_home,
            |_url| {},
        )?;
        self.set_snapshot(app, |s| {
            s.web_pid = Some(pid);
            s.started_at = Some(now_ms());
            s.mode = if mode == "dev" {
                LauncherMode::Dev
            } else {
                LauncherMode::Normal
            };
        });
        let ready_timeout = settings.ready_timeout_ms.max(10_000);
        match self
            .supervisor
            .wait_ready(pid, settings.port, ready_timeout)
        {
            Ok(url) => {
                if !self.epoch_ok(my_epoch) {
                    return Err("epoch-changed".into());
                }
                self.set_snapshot(app, |s| {
                    s.state = LauncherState::Running;
                    s.url = Some(url.clone());
                    s.web_pid = Some(pid);
                    s.started_at = Some(now_ms());
                    s.ready_at = Some(now_ms());
                    s.error = None;
                    s.busy = false;
                    s.phase = "就绪".into();
                    s.hmr_active = false;
                });
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    &format!("就绪行命中 → {url} · 状态 → running"),
                );
                if mode == "dev" {
                    self.log_hub.append(
                        "launcher",
                        crate::contract::LogLevel::Info,
                        "开发模式提示:客户端插件 / 前端改动免刷新热更;lib/ 产物改动需「重建并重启」",
                    );
                }
                self.supervisor.persist_running();
                self.open_browser(&url);
                Ok(())
            }
            Err(e) => {
                if !self.epoch_ok(my_epoch) {
                    return Err("epoch-changed".into());
                }
                self.supervisor.stop("dsh web");
                self.fail(app, "启动失败", &e);
                Err(e)
            }
        }
    }

    fn start_flow(self: &Arc<Self>, app: &AppHandle, mode: &str) -> Result<(), String> {
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err("repo-unusable".into());
        }
        // 已在运行 → 直接召回
        {
            let snap = self.snapshot.lock().unwrap();
            if snap.state == LauncherState::Running && snap.web_pid.is_some() {
                let url = snap
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("http://{}:{}/", settings.host, settings.port));
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Info,
                    "dsh web 已在运行,直接打开主界面",
                );
                self.open_browser(&url);
                return Ok(());
            }
        }
        if mode == "dev" && runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("开发模式需要 Node {}", runtime::NODE_RANGE_MSG),
                "当前未找到兼容 Node,tsx/tsdown 会崩溃。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err("node-unsupported".into());
        }
        if config::probe_port(&settings.host, settings.port) {
            self.fail(
                app,
                &format!("端口 {} 已被占用", settings.port),
                &format!(
                    "{}。请在「设置」中更换 dsh web 端口后重试,或先停止占用进程",
                    self.port_diag(settings.port)
                ),
            );
            return Err("port-busy".into());
        }
        if self.tools.lock().unwrap().pnpm.is_none() {
            self.fail(
                app,
                "未找到 pnpm",
                "请先安装 pnpm(brew install pnpm 或 corepack enable),然后重试",
            );
            return Err("no-pnpm".into());
        }
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        self.set_snapshot(app, |s| {
            s.busy = true;
            s.error = None;
            s.mode = if mode == "dev" {
                LauncherMode::Dev
            } else {
                LauncherMode::Normal
            };
            s.state = LauncherState::Starting;
            s.phase = if mode == "dev" {
                "启动开发模式…".into()
            } else {
                "启动 dsh web…".into()
            };
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Info,
            &format!(
                "状态 → starting · 拉起 dsh web(源码启动,端口 {})",
                settings.port
            ),
        );
        if mode == "dev" {
            let tools = self.tools.lock().unwrap().clone();
            match self.supervisor.spawn_dev(&tools, &settings.repo_path) {
                Ok(pid) => {
                    self.set_snapshot(app, |s| s.dev_pid = Some(pid));
                    self.log_hub.append(
                        "launcher",
                        crate::contract::LogLevel::Info,
                        "开发模式:dsh web + pnpm run dev:web 同跑(HMR watcher 后台初始化)",
                    );
                }
                Err(e) => {
                    if self.epoch_ok(my_epoch) {
                        self.fail(app, "开发模式启动失败", &e);
                    }
                    return Err(e);
                }
            }
        }
        let r = self.launch_web(app, mode, my_epoch);
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        r
    }

    fn update_flow(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err("repo-unusable".into());
        }
        if runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("更新并构建需要 Node {}", runtime::NODE_RANGE_MSG),
                "未找到兼容 Node,tsx/tsdown 会崩溃。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err("node-unsupported".into());
        }
        if self.tools.lock().unwrap().git.is_none() {
            self.fail(app, "未找到 git", "「更新并构建」需要 git,请先安装");
            return Err("no-git".into());
        }
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        let mode = self.snapshot.lock().unwrap().mode.clone();
        self.set_snapshot(app, |s| {
            s.busy = true;
            s.error = None;
            s.mode = mode.clone();
            s.state = LauncherState::Syncing;
            s.phase = "同步远端…".into();
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Info,
            "更新并构建:同步 → 安装 → 构建 → 重启",
        );

        let tools = self.tools.lock().unwrap().clone();
        let repo = RepoService::new(self.log_hub.clone(), tools.clone());
        // 1. 同步
        let before = repo.head_short(&settings.repo_path);
        let sync = repo.sync(&settings.repo_path);
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        if !sync.ok {
            let (summary, detail) = match sync.stage.as_str() {
                "conflict" => (
                    "git 冲突:已报告,未破坏工作区".to_string(),
                    format!(
                        "{}{}",
                        sync.error.unwrap_or_default(),
                        if sync.conflicts.is_empty() {
                            String::new()
                        } else {
                            format!("\n冲突文件:{}", sync.conflicts.join("、"))
                        }
                    ),
                ),
                "stash" => (
                    "自动暂存本地改动失败".to_string(),
                    sync.error.unwrap_or_default(),
                ),
                stage => (
                    format!("同步远端失败({stage})"),
                    sync.error.unwrap_or_default(),
                ),
            };
            self.fail(app, &summary, &detail);
            return Err("sync-failed".into());
        }
        self.set_snapshot(app, |s| {
            s.repo.sync_at = Some(now_ms());
        });
        self.refresh_repo();

        // 2. 依赖安装
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Installing;
        });
        let (needed, ok) = crate::services::build::install_if_needed(
            &self.log_hub,
            &tools,
            &repo,
            &settings.repo_path,
            &before,
            &|p| {
                self.set_snapshot(app, |s| s.phase = p.to_string());
            },
        )?;
        let _ = needed;
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        if !ok {
            self.fail(app, "依赖安装失败", "查看日志尾部(pnpm install 退出码非 0)");
            return Err("install-failed".into());
        }

        // 3. 构建
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Building;
            s.phase = "构建中…".into();
        });
        let ok = crate::services::build::run_build(
            &self.log_hub,
            &tools,
            &settings.repo_path,
            &settings.build_args,
            &|p| {
                self.set_snapshot(app, |s| s.phase = p.to_string());
            },
        )?;
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        if !ok {
            self.fail(
                app,
                "构建失败",
                "退出码非 0;查看日志尾部定位到阶段。修复后重试「更新并构建」",
            );
            return Err("build-failed".into());
        }

        // 4. 重启服务(同模式)
        self.supervisor.stop("dsh web");
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Starting;
            s.phase = "启动 dsh web…".into();
            s.url = None;
            s.web_pid = None;
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            &format!(
                "更新并构建完成 → 启动 dsh web(模式:{})",
                if mode == LauncherMode::Dev {
                    "开发"
                } else {
                    "标准"
                }
            ),
        );
        let r = self.launch_web(
            app,
            if mode == LauncherMode::Dev {
                "dev"
            } else {
                "normal"
            },
            my_epoch,
        );
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        r
    }

    fn rebuild_flow(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err("repo-unusable".into());
        }
        if runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("重建并重启需要 Node {}", runtime::NODE_RANGE_MSG),
                "未找到兼容 Node。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err("node-unsupported".into());
        }
        if self.tools.lock().unwrap().pnpm.is_none() {
            self.fail(app, "未找到 pnpm", "请先安装 pnpm,然后重试");
            return Err("no-pnpm".into());
        }
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        let mode = self.snapshot.lock().unwrap().mode.clone();
        self.set_snapshot(app, |s| {
            s.busy = true;
            s.error = None;
            s.mode = mode.clone();
            s.state = LauncherState::Stopping;
            s.phase = "停止中…".into();
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Info,
            "重建并重启:停止 → 构建 → 启动",
        );
        self.supervisor.stop_all();
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Building;
            s.phase = "构建中…".into();
            s.web_pid = None;
            s.dev_pid = None;
            s.url = None;
        });
        let tools = self.tools.lock().unwrap().clone();
        let ok = crate::services::build::run_build(
            &self.log_hub,
            &tools,
            &settings.repo_path,
            &settings.build_args,
            &|p| {
                self.set_snapshot(app, |s| s.phase = p.to_string());
            },
        )?;
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        if !ok {
            self.fail(
                app,
                "构建失败",
                "构建失败,服务已停止。修复后重试「重建并重启」",
            );
            return Err("build-failed".into());
        }
        self.refresh_repo();
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Starting;
            s.phase = "启动 dsh web…".into();
        });
        let r = self.launch_web(
            app,
            if mode == LauncherMode::Dev {
                "dev"
            } else {
                "normal"
            },
            my_epoch,
        );
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        r
    }

    /// 停止流程:epoch++ 已在入口完成;这里停全部进程树并复位状态。
    fn stop_flow(self: &Arc<Self>, app: &AppHandle) {
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Stopping;
            s.busy = true;
            s.phase = "停止中…".into();
            s.error = None;
        });
        self.supervisor.stop_all();
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Idle;
            s.busy = false;
            s.mode = LauncherMode::None;
            s.phase = String::new();
            s.web_pid = None;
            s.dev_pid = None;
            s.url = None;
            s.hmr_active = false;
            s.started_at = None;
            s.ready_at = None;
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            "已停止全部进程(dsh web / dev:web)",
        );
    }

    fn install_node_flow(self: &Arc<Self>, app: &AppHandle) -> Result<(), String> {
        if runtime::resolve_dsh_node().is_some() {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Info,
                "Node 运行时已就绪,无需安装",
            );
            return Ok(());
        }
        let my_epoch = self.epoch.load(Ordering::SeqCst);
        self.set_snapshot(app, |s| {
            s.busy = true;
            s.error = None;
            s.state = LauncherState::Starting;
            s.phase = "安装 Node 24 LTS…".into();
        });
        let result = runtime::install_node(&self.log_hub, &|p| {
            self.set_snapshot(app, |s| s.phase = p.to_string());
        });
        if !self.epoch_ok(my_epoch) {
            return Err("epoch-changed".into());
        }
        match result {
            Ok((path, version)) => {
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    &format!("Node {version} 安装完成并已自动选用 → {}", path.display()),
                );
                self.set_snapshot(app, |s| {
                    s.busy = false;
                    s.state = LauncherState::Idle;
                    s.phase = String::new();
                });
                Ok(())
            }
            Err(e) => {
                self.fail(app, "Node 安装失败", &e);
                Err(e)
            }
        }
    }

    // ── 其它命令 ─────────────────────────────────────────

    /// 更新快照的 update 字段并广播。
    fn set_update(
        &self,
        app: &AppHandle,
        partial: impl FnOnce(&mut crate::contract::UpdateSnapshot),
    ) {
        self.set_snapshot(app, |s| partial(&mut s.update));
    }

    /// 检查更新(Tauri updater,异步;结果同时写入 snapshot.update)。
    pub async fn check_for_update(&self, app: &AppHandle) -> UpdateResult {
        use tauri_plugin_updater::UpdaterExt;
        self.set_update(app, |u| {
            u.checking = true;
            u.error = None;
            u.message = None;
        });
        let updater = match app.updater() {
            Ok(u) => u,
            Err(e) => {
                let msg = format!("updater 初始化失败:{e}");
                self.set_update(app, |u| {
                    u.checking = false;
                    u.error = Some(msg.clone());
                });
                return UpdateResult {
                    ok: false,
                    reason: None,
                    version: None,
                    error: Some(msg),
                };
            }
        };
        match updater.check().await {
            Ok(Some(update)) => {
                self.set_update(app, |u| {
                    u.checking = false;
                    u.available = true;
                    u.version = Some(update.version.clone());
                    u.url = Some(update.download_url.to_string());
                    u.notes = update.body.clone();
                    u.message = Some(format!("发现新版本 v{}", update.version));
                });
                UpdateResult {
                    ok: true,
                    reason: Some(format!("发现新版本 v{}", update.version)),
                    version: Some(update.version),
                    error: None,
                }
            }
            Ok(None) => {
                self.set_update(app, |u| {
                    u.checking = false;
                    u.available = false;
                    u.message = Some("当前已是最新版本".into());
                });
                UpdateResult {
                    ok: true,
                    reason: Some("当前已是最新版本".into()),
                    version: None,
                    error: None,
                }
            }
            Err(e) => {
                let msg = e.to_string();
                self.set_update(app, |u| {
                    u.checking = false;
                    u.error = Some(msg.clone());
                });
                UpdateResult {
                    ok: false,
                    reason: None,
                    version: None,
                    error: Some(msg),
                }
            }
        }
    }

    /// 下载并安装更新,完成后重启应用。
    pub async fn apply_update(&self, app: &AppHandle) -> ActionAccepted {
        use tauri_plugin_updater::UpdaterExt;
        let updater = match app.updater() {
            Ok(u) => u,
            Err(e) => {
                return ActionAccepted {
                    ok: false,
                    reason: Some(format!("updater 初始化失败:{e}")),
                    aborted: None,
                    already: None,
                };
            }
        };
        let update = match updater.check().await {
            Ok(Some(u)) => u,
            Ok(None) => {
                return ActionAccepted {
                    ok: false,
                    reason: Some("没有可用的更新".into()),
                    aborted: None,
                    already: None,
                };
            }
            Err(e) => {
                return ActionAccepted {
                    ok: false,
                    reason: Some(format!("检查更新失败:{e}")),
                    aborted: None,
                    already: None,
                };
            }
        };
        self.set_update(app, |u| {
            u.installing = true;
            u.error = None;
        });
        match update
            .download_and_install(|_chunks, _total| {}, || {})
            .await
        {
            Ok(_) => {
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    "更新已安装,应用将自动重启",
                );
                // restart 返回 !,后续不可达
                app.restart()
            }
            Err(e) => {
                self.set_update(app, |u| {
                    u.installing = false;
                    u.error = Some(e.to_string());
                });
                ActionAccepted {
                    ok: false,
                    reason: Some(format!("下载安装失败:{e}")),
                    aborted: None,
                    already: None,
                }
            }
        }
    }

    pub fn open_dsh(&self) -> Result<(), String> {
        let settings = config::load();
        let url = format!("http://{}:{}/", settings.host, settings.port);
        tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| format!("打开 dsh 失败:{e}"))
    }

    pub fn open_repo_directory(&self) -> Result<(), String> {
        let settings = config::load();
        tauri_plugin_opener::open_path(std::path::Path::new(&settings.repo_path), None::<&str>)
            .map_err(|e| format!("打开仓库目录失败:{e}"))
    }

    pub fn open_log_directory(&self) -> Result<(), String> {
        let dir = crate::services::supervisor::log_file();
        if let Some(parent) = dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        tauri_plugin_opener::open_path(&dir, None::<&str>)
            .map_err(|e| format!("打开日志目录失败:{e}"))
    }

    /// 应用退出:detach(不停止 dsh)。
    pub fn on_app_exit(&self) {
        self.supervisor.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_update_releases_lock_before_side_effects() {
        let snapshot = Mutex::new(AppSnapshot::mock_idle());
        let updated = apply_snapshot_update(&snapshot, |s| {
            s.state = LauncherState::Starting;
            s.phase = "启动中".into();
        });

        assert_eq!(updated.state, LauncherState::Starting);
        assert_eq!(updated.phase, "启动中");
        assert!(snapshot.try_lock().is_ok(), "快照更新后不应继续持锁");
    }
}
