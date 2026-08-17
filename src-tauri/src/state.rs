// dsh-launcher · 应用状态与动作协调器(Rust 原生核心,无 HTTP/轮询)
// 唯一状态机:所有动作从这里发起;长任务通过 OperationCoordinator 获得 operationId、
// 取消令牌与 journal;只有 terminal success 才表示成功。
// 动作矩阵:exclusive-write(安装/克隆/构建/更新)同一时间只能一个;start/dev 与
// exclusive-write 互斥;stop/cancel、日志、最小化保持可用;禁用按钮给出具体原因。
use crate::config;
use crate::contract::{
    ActionAccepted, AppSnapshot, DesktopPreferences, DesktopSnapshot, DisabledAction,
    EnvironmentSnapshot, ErrorSummary, LauncherMode, LauncherState, LogPage, OperationKind,
    OperationStatus, RepoSnapshot, SettingsSnapshot, ToolCheck, UpdateResult, EVENT_STATE_CHANGED,
};
use crate::log_hub::LogHub;
use crate::ops::{CancellationToken, InstallationSnapshot, OperationCoordinator, OperationError};
use crate::perf::BootTimings;
use crate::preferences;
use crate::services::repo::RepoService;
use crate::services::runtime::{self, Tools};
use crate::services::supervisor::Supervisor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub struct AppState {
    pub log_hub: Arc<LogHub>,
    pub supervisor: Arc<Supervisor>,
    pub tools: Mutex<Tools>,
    /// 统一操作协调器(operationId / journal / 取消令牌)。
    pub ops: OperationCoordinator,
    /// 启动/运行性能测量点。
    pub timings: Arc<BootTimings>,
    /// 待执行的克隆请求(Clone 弹窗提交;流程层取出执行)。
    pub pending_clone: Mutex<Option<crate::clone::CloneRequest>>,
    /// 流程纪元:stop/cancel 递增;旧流程检测到变化即放弃(与取消令牌双保险)。
    pub epoch: AtomicU64,
    /// 是否有流程在执行(串行化破坏性动作)。
    pub flow_active: AtomicBool,
    pub snapshot: Mutex<AppSnapshot>,
    pub preferences: Mutex<DesktopPreferences>,
    pub boot_error: Mutex<Option<String>>,
    /// setup 后的异步 bootstrap 是否完成。启动动作在此之前不会绕过 catalog 安全校验。
    pub bootstrap_ready: AtomicBool,
    #[allow(dead_code)]
    pub quit_requested: AtomicBool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartPlan {
    packaged: bool,
    requires_repo: bool,
    builds_on_start: bool,
    starts_watcher: bool,
}

fn start_plan(mode: &str) -> StartPlan {
    if mode == "normal" {
        StartPlan {
            packaged: true,
            requires_repo: false,
            builds_on_start: false,
            starts_watcher: false,
        }
    } else {
        StartPlan {
            packaged: false,
            requires_repo: true,
            builds_on_start: true,
            starts_watcher: true,
        }
    }
}

// ── 环境检测文件缓存 ─────────────────────────────────────
// 探测 node/pnpm/git 要拉起子进程,Windows 上可能耗时数秒;成功结果落盘缓存,
// 工具链安装/克隆/设置/构建等影响检测结果的动作显式失效,「重新检测」走 force。

/// 缓存有效期:24h(成功即成功;跨天或显式失效才重新探测)。
const ENV_CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const ENV_CACHE_SCHEMA: u32 = 2;

fn env_cache_file() -> PathBuf {
    crate::config::state_dir().join("env-cache.json")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EnvCacheFile {
    schema: u32,
    cached_at_ms: i64,
    snapshot: EnvironmentSnapshot,
}

fn load_env_cache() -> Option<EnvironmentSnapshot> {
    let raw = std::fs::read_to_string(env_cache_file()).ok()?;
    let c: EnvCacheFile = serde_json::from_str(&raw).ok()?;
    if c.schema != ENV_CACHE_SCHEMA {
        return None;
    }
    if now_ms() - c.cached_at_ms > ENV_CACHE_TTL_MS {
        return None;
    }
    Some(c.snapshot)
}

fn save_env_cache(snap: &EnvironmentSnapshot) {
    let _ = std::fs::create_dir_all(crate::config::state_dir());
    let c = EnvCacheFile {
        schema: ENV_CACHE_SCHEMA,
        cached_at_ms: now_ms(),
        snapshot: snap.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&c) {
        let _ = std::fs::write(env_cache_file(), json);
    }
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
    pub fn new(
        log_hub: Arc<LogHub>,
        supervisor: Arc<Supervisor>,
        tools: Tools,
        ops: OperationCoordinator,
        timings: Arc<BootTimings>,
    ) -> Self {
        let prefs = preferences::load_and_migrate();
        Self {
            log_hub,
            supervisor,
            tools: Mutex::new(tools),
            ops,
            timings,
            pending_clone: Mutex::new(None),
            epoch: AtomicU64::new(1),
            flow_active: AtomicBool::new(false),
            snapshot: Mutex::new(AppSnapshot::mock_idle()),
            preferences: Mutex::new(prefs),
            boot_error: Mutex::new(None),
            bootstrap_ready: AtomicBool::new(false),
            quit_requested: AtomicBool::new(false),
        }
    }

    // ── 快照 ─────────────────────────────────────────────

    /// 把 operation / disabled_actions / busy 注入快照(单一事实来源)。
    fn finalize(&self, mut s: AppSnapshot) -> AppSnapshot {
        s.operation = self.ops.current();
        s.disabled_actions = self.disabled_actions(&s);
        s.busy = self.ops.is_active()
            || matches!(
                s.state,
                LauncherState::Syncing
                    | LauncherState::Installing
                    | LauncherState::Building
                    | LauncherState::Starting
                    | LauncherState::Stopping
            );
        s
    }

    /// 动作矩阵:给定当前快照,返回被禁用动作及其原因。
    pub fn disabled_actions(&self, s: &AppSnapshot) -> Vec<DisabledAction> {
        const ACTIONS: [&str; 12] = [
            "install-node",
            "install-git",
            "install-pnpm",
            "install-toolchain",
            "clone-repo",
            "full-setup",
            "start",
            "dev",
            "update",
            "rebuild",
            "apply-update",
            "save-settings",
        ];
        let mut out = Vec::new();
        for a in ACTIONS {
            if let Err(reason) = self.can_run(a, s) {
                out.push(DisabledAction {
                    action: a.to_string(),
                    reason,
                });
            }
        }
        out
    }

    /// 动作矩阵判定:Err(原因) 表示该动作当前被禁用。
    pub fn can_run(&self, action: &str, s: &AppSnapshot) -> Result<(), String> {
        let op = s.operation.as_ref();
        let active = op.is_some();
        let transitioning = matches!(s.state, LauncherState::Starting | LauncherState::Stopping);
        match action {
            // 安全类:始终允许(日志/取消/停止/查看)
            "clear" | "detach" | "check-update" | "stop" | "cancel" => Ok(()),
            "start" | "dev" => {
                if active {
                    Err(format!(
                        "正在执行「{}」,完成后才能启动",
                        op.unwrap().kind.label()
                    ))
                } else if transitioning {
                    Err("服务正在启动/停止中".into())
                } else {
                    Ok(())
                }
            }
            // 工具链安装:不与任何 exclusive-write 并发;服务启停期间不允许
            "install-node" | "install-git" | "install-pnpm" | "install-toolchain" => {
                if active {
                    Err(format!("正在执行「{}」", op.unwrap().kind.label()))
                } else if transitioning {
                    Err("服务正在启动/停止中".into())
                } else {
                    Ok(())
                }
            }
            // 仓库/构建类:exclusive-write;运行中必须先停止
            "clone-repo" | "full-setup" | "update" | "rebuild" => {
                if active {
                    Err(format!("正在执行「{}」", op.unwrap().kind.label()))
                } else if transitioning {
                    Err("服务正在启动/停止中".into())
                } else if s.state == LauncherState::Running {
                    Err("请先停止 dsh 服务,再执行仓库/构建操作".into())
                } else {
                    Ok(())
                }
            }
            "apply-update" => {
                if active {
                    Err(format!("正在执行「{}」", op.unwrap().kind.label()))
                } else if !s.update.available {
                    Err("没有可用更新".into())
                } else {
                    Ok(())
                }
            }
            // 影响 repo/runtime/network 的设置:任务期间禁用
            "save-settings" => {
                if active {
                    Err(format!(
                        "任务「{}」执行期间不能修改仓库/端口等设置",
                        op.unwrap().kind.label()
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// 用部分字段更新快照并广播事件。
    pub fn set_snapshot(&self, app: &AppHandle, partial: impl FnOnce(&mut AppSnapshot)) {
        // 先完成状态更新并释放锁。托盘刷新会再次读取 snapshot，若持锁调用会自锁，
        // 表现为 UI 已收到 starting 事件、随后启动流程和设置 IPC 永久卡住。
        let snap = apply_snapshot_update(&self.snapshot, partial);
        let snap = self.finalize(snap);
        let _ = app.emit(EVENT_STATE_CHANGED, &snap);
        crate::tray::refresh(app);
        // 启动成功后自动进入 DeepSeek 工作区(真实终态 + 健康 + 子视图就绪才动作)
        crate::dsh_view::maybe_auto_enter(app);
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let mut snap = self.snapshot.lock().unwrap().clone();
        if let Some(err) = self.boot_error.lock().unwrap().clone() {
            snap.error = Some(ErrorSummary {
                summary: "桌面核心启动失败".into(),
                detail: err,
            });
        }
        self.finalize(snap)
    }

    pub fn mark_bootstrap_ready(&self) {
        self.bootstrap_ready.store(true, Ordering::SeqCst);
    }

    pub fn set_bootstrap_error(&self, app: &AppHandle, detail: String) {
        *self.boot_error.lock().unwrap() = Some(detail);
        self.bootstrap_ready.store(true, Ordering::SeqCst);
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Failed;
            s.phase = String::new();
        });
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
        self.timings.mark("repo_check_done");
        self.emit_perf(app);
        let snap = self.snapshot.lock().unwrap().clone();
        let _ = app.emit(EVENT_STATE_CHANGED, &snap);
    }

    // ── 桌面信息 ─────────────────────────────────────────

    pub fn desktop_snapshot(&self) -> DesktopSnapshot {
        let settings = config::load();
        // 首次运行判定:显式跳过(或完成过引导)即视为已处理;否则需要可用仓库。
        // 跳过不是死路:启动器主界面仍可克隆仓库/安装环境。
        let first_run_done = settings.first_run_skipped
            || (!settings.repo_path.is_empty() && config::repo_usable(&settings.repo_path).ok);
        DesktopSnapshot {
            preferences: self.preferences.lock().unwrap().clone(),
            first_run_done,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    // ── 归档会话 ───────────────────────────────────────────

    pub fn archives_get_snapshot(&self) -> Result<crate::contract::ArchivesSnapshot, String> {
        let settings = config::load();
        let running = self.snapshot().state == crate::contract::LauncherState::Running;
        crate::services::archives::get_snapshot(&settings, running)
    }

    pub fn archives_restore(
        &self,
        session_id: &str,
    ) -> Result<crate::contract::ArchiveRestoreResult, String> {
        let settings = config::load();
        let running = self.snapshot().state == crate::contract::LauncherState::Running;
        crate::services::archives::restore(&settings, running, session_id)
    }

    pub fn archives_delete(
        &self,
        session_id: &str,
    ) -> Result<crate::contract::ArchiveDeleteResult, String> {
        let settings = config::load();
        let running = self.snapshot().state == crate::contract::LauncherState::Running;
        crate::services::archives::delete(&settings, running, session_id)
    }

    pub fn archives_delete_all(&self) -> Result<crate::contract::ArchiveDeleteResult, String> {
        let settings = config::load();
        let running = self.snapshot().state == crate::contract::LauncherState::Running;
        crate::services::archives::delete_all(&settings, running)
    }

    /// 完成/跳过首次运行引导:
    /// - skip=true 仅标记 firstRunSkipped(不强制要求仓库可用,用户稍后配置);
    /// - repo_path 提供时一并保存(完成引导路径);
    /// - 广播 state-changed,让 renderer 的 desktop snapshot 立即刷新,退出向导。
    pub fn complete_first_run(
        &self,
        app: &AppHandle,
        skip: bool,
        repo_path: Option<String>,
    ) -> Result<DesktopSnapshot, String> {
        let mut map = serde_json::Map::new();
        map.insert("firstRunSkipped".into(), serde_json::json!(true));
        if let Some(p) = repo_path {
            let p = config::expand_path(p.trim());
            if p.is_empty() {
                return Err("仓库路径不能为空".into());
            }
            map.insert("repoPath".into(), serde_json::json!(p));
        }
        config::apply_patch(&serde_json::Value::Object(map))?;
        let _ = skip; // skip 与完成都标记 firstRunSkipped=true;差异仅在是否保存 repoPath
                      // repoPath 可能变化 → 环境缓存失效
        self.invalidate_env_cache();
        // 刷新仓库快照(完成路径下 repoPath 可能变化)
        self.refresh_repo();
        let snap = self.snapshot();
        let _ = app.emit(EVENT_STATE_CHANGED, &snap);
        Ok(self.desktop_snapshot())
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
        let snap = self.snapshot();
        self.can_run("save-settings", &snap)?;
        let saved = config::apply_patch(patch)?;
        // 设置可能含 repoPath 等影响环境检测的字段 → 缓存失效
        self.invalidate_env_cache();
        Ok(saved)
    }

    // ── 环境 ─────────────────────────────────────────────

    /// 环境检测(带文件缓存):探测 node/pnpm/git 需要拉起子进程,Windows 上可能耗时
    /// 数百毫秒到数秒;检测结果落盘缓存,force=false 且缓存有效时直接返回,秒开。
    /// 缓存由工具链安装/克隆/设置变更显式失效;「重新检测」传 force=true 绕过。
    pub fn environment(&self, force: bool) -> EnvironmentSnapshot {
        if !force {
            if let Some(snap) = load_env_cache() {
                return snap;
            }
        }
        let snap = self.detect_environment();
        save_env_cache(&snap);
        snap
    }

    /// 实际探测(不读缓存)。
    fn detect_environment(&self) -> EnvironmentSnapshot {
        let settings = config::load();
        let tools = self.tools.lock().unwrap().clone();
        let mut warnings = Vec::new();

        // 「当前实际生效」的工具链:托管优先,系统回退(与子进程 PATH 组装一致)。
        let node = crate::toolchain::current_node();
        let pnpm = crate::toolchain::current_pnpm(&tools);
        let git = crate::toolchain::current_git(&tools);

        if node.is_missing() {
            warnings.push("未找到 dsh 要求版本(^22.19 || >=24)的 Node;可安装托管 Node".into());
        } else if node.status == ToolCheck::Incompatible {
            warnings
                .push("系统 Node 版本不在 dsh 要求范围(^22.19 || >=24),推荐安装托管 Node".into());
        }
        if pnpm.is_missing() {
            warnings.push(
                "未找到 pnpm(系统/托管均无);「启动/开发模式/更新并构建」需要它,可安装托管 pnpm"
                    .into(),
            );
        }
        if git.is_missing() {
            warnings.push(
                "未找到 git,「克隆仓库/更新并构建」不可用;macOS/Linux 请安装系统 Git,Windows 可安装托管 MinGit"
                    .into(),
            );
        }

        EnvironmentSnapshot {
            repo_path: settings.repo_path.clone(),
            repo_usable: config::repo_usable(&settings.repo_path),
            dist_built: config::dist_built(&settings.repo_path),
            platform: crate::toolchain::platform_name().to_string(),
            node,
            pnpm,
            git,
            warnings,
        }
    }

    /// 环境缓存失效(工具链安装/克隆/仓库路径/构建等影响检测结果的动作后调用)。
    pub fn invalidate_env_cache(&self) {
        let _ = std::fs::remove_file(env_cache_file());
    }

    // ── 性能测量 ─────────────────────────────────────────

    pub fn emit_perf(&self, app: &AppHandle) {
        use tauri::Emitter as _;
        let _ = app.emit(crate::perf::EVENT_PERF_METRICS, self.timings.snapshot());
    }

    // ── 动作协调 ─────────────────────────────────────────

    /// 动作入口:长流程在后台线程执行,立即返回「已接受」。
    /// 成功与否只由快照中的 operation.status 终态决定。
    pub fn run_action(self: &Arc<Self>, app: &AppHandle, action: &str) -> ActionAccepted {
        let allowed = [
            "start",
            "dev",
            "update",
            "rebuild",
            "stop",
            "cancel",
            "install-node",
            "install-git",
            "install-pnpm",
            "install-toolchain",
            "clone-repo",
            "full-setup",
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
            // stop 不需要流程锁:取消令牌 + epoch++ + 停止全部进程树
            self.cancel_flow(app);
            return ActionAccepted {
                ok: true,
                reason: None,
                aborted: None,
                already: None,
            };
        }
        if action == "cancel" {
            if !self.ops.is_active() {
                return ActionAccepted {
                    ok: false,
                    reason: Some("没有进行中的任务".into()),
                    aborted: Some(true),
                    already: None,
                };
            }
            self.cancel_flow(app);
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
                reason: Some("自动更新由设置页的异步命令提供".into()),
                aborted: None,
                already: None,
            };
        }

        let kind: OperationKind = match action {
            "start" => OperationKind::StartWeb,
            "dev" => OperationKind::StartDev,
            "update" => OperationKind::UpdateRebuild,
            "rebuild" => OperationKind::RebuildRestart,
            "install-node" => OperationKind::InstallNode,
            "install-git" => OperationKind::InstallGit,
            "install-pnpm" => OperationKind::InstallPnpm,
            "install-toolchain" => OperationKind::InstallToolchain,
            "clone-repo" => OperationKind::CloneRepo,
            "full-setup" => OperationKind::FullSetup,
            _ => {
                return ActionAccepted {
                    ok: false,
                    reason: Some(format!("动作 {action} 未绑定流程")),
                    aborted: None,
                    already: None,
                };
            }
        };

        // 动作矩阵:被禁用时给出具体原因
        let snap = self.snapshot();
        if let Err(reason) = self.can_run(action, &snap) {
            return ActionAccepted {
                ok: false,
                reason: Some(reason),
                aborted: None,
                already: None,
            };
        }

        // 开始操作(exclusive-write 与 start/dev 互斥由 can_run + coordinator 双保险)
        let (id, token) = match self.ops.begin(kind, true, "准备中…") {
            Ok(x) => x,
            Err(e) => {
                return ActionAccepted {
                    ok: false,
                    reason: Some(e),
                    aborted: None,
                    already: None,
                };
            }
        };
        if self.flow_active.swap(true, Ordering::SeqCst) {
            self.ops
                .finish(id, OperationStatus::Failed, Some("已有流程在执行".into()));
            return ActionAccepted {
                ok: false,
                reason: Some("已有流程在执行".into()),
                aborted: None,
                already: None,
            };
        }

        // 立即广播「已接受」快照(operation 可见)
        self.set_snapshot(app, |_| {});

        // 记录「成功后进入 DeepSeek」意图(accepted ≠ success;终态成功才切换)
        if matches!(action, "start" | "dev" | "update" | "rebuild") {
            let mgr = app.state::<Arc<crate::dsh_view::DshViewManager>>();
            mgr.pending_enter
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        let app = app.clone();
        let me = self.clone();
        let action = action.to_string();
        std::thread::spawn(move || {
            let result: Result<(), OperationError> = match action.as_str() {
                "start" => me.start_flow(&app, "normal", &token),
                "dev" => me.start_flow(&app, "dev", &token),
                "update" => me.update_flow(&app, &token),
                "rebuild" => me.rebuild_flow(&app, &token),
                "install-node" => me.install_node_flow(&app, &token),
                "install-git" => me.install_git_flow(&app, &token),
                "install-pnpm" => me.install_pnpm_flow(&app, &token),
                "install-toolchain" => me.install_toolchain_flow(&app, &token),
                "clone-repo" => me.clone_repo_flow(&app, &token),
                "full-setup" => me.full_setup_flow(&app, &token),
                _ => Err(OperationError::Failed("未知流程".into())),
            };
            match result {
                Ok(()) => me.ops.finish(id, OperationStatus::Success, None),
                Err(OperationError::Cancelled) => {
                    me.ops
                        .finish(id, OperationStatus::Cancelled, Some("已取消".into()))
                }
                Err(OperationError::Failed(e)) => {
                    me.ops.finish(id, OperationStatus::Failed, Some(e))
                }
            }
            me.flow_active.store(false, Ordering::SeqCst);
            // 广播终态快照(operation 已清空或为终态)
            me.set_snapshot(&app, |_| {});
        });
        ActionAccepted {
            ok: true,
            reason: None,
            aborted: None,
            already: None,
        }
    }

    /// 取消当前流程:令牌置位 + epoch++ + 停止全部进程树。
    /// 正在执行的流程线程检测到令牌后自行 finish(Cancelled)。
    fn cancel_flow(self: &Arc<Self>, app: &AppHandle) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.ops.request_cancel();
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Info,
            "收到停止/取消请求:置位取消令牌并停止全部进程树",
        );
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Stopping;
            s.phase = "停止中…".into();
            s.error = None;
        });
        let app = app.clone();
        let me = self.clone();
        std::thread::spawn(move || {
            me.supervisor.stop_all();
            me.set_snapshot(&app, |s| {
                s.state = LauncherState::Idle;
                s.mode = LauncherMode::None;
                s.phase = String::new();
                s.web_pid = None;
                s.dev_pid = None;
                s.url = None;
                s.hmr_active = false;
                s.started_at = None;
                s.ready_at = None;
            });
            me.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Ok,
                "已停止全部进程(dsh web / dev:web)",
            );
        });
    }

    // ── 流程实现 ─────────────────────────────────────────

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
            s.phase = String::new();
        });
    }

    /// 端口占用诊断。
    fn port_diag(&self, port: u16) -> String {
        match crate::services::supervisor::port_holder_pid(port) {
            Some(pid) => format!("占用进程 PID {pid}"),
            None => "占用进程未知".into(),
        }
    }

    fn set_operation_stage(&self, stage: &str) {
        if let Some(op) = self.ops.current() {
            self.ops.set_stage(op.operation_id, stage, None);
        }
    }

    /// 统一处理源码/packaged Host 的进程登记、就绪检测和失败清理。
    fn wait_for_web(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: &str,
        pid: u32,
        port: u16,
        ready_timeout: u64,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        self.set_snapshot(app, |s| {
            s.web_pid = Some(pid);
            s.started_at = Some(now_ms());
            s.mode = if mode == "dev" {
                LauncherMode::Dev
            } else {
                LauncherMode::Normal
            };
        });
        let cancel_flag = token.flag();
        match self.supervisor.wait_ready_cancellable(
            pid,
            port,
            ready_timeout.max(10_000),
            cancel_flag,
        ) {
            Ok(url) => {
                token.check()?;
                self.set_snapshot(app, |s| {
                    s.state = LauncherState::Running;
                    s.url = Some(url.clone());
                    s.web_pid = Some(pid);
                    s.started_at = Some(now_ms());
                    s.ready_at = Some(now_ms());
                    s.error = None;
                    s.phase = "就绪".into();
                    s.hmr_active = false;
                });
                self.timings.mark("dsh_ready");
                self.emit_perf(app);
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
                Ok(())
            }
            Err(e) => {
                token.check()?;
                self.supervisor.stop("dsh web");
                self.fail(app, "启动失败", &e);
                Err(OperationError::Failed(e))
            }
        }
    }

    /// 启动源码 dsh web；仅由开发模式调用。
    fn launch_source_web(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: &str,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        let tools = self.tools.lock().unwrap().clone();
        let pid = self
            .supervisor
            .spawn_web(
                &tools,
                &settings.repo_path,
                settings.port,
                &settings.host,
                &settings.dsh_home,
                |_url| {},
            )
            .map_err(OperationError::Failed)?;
        self.wait_for_web(
            app,
            mode,
            pid,
            settings.port,
            settings.ready_timeout_ms,
            token,
        )
    }

    /// 启动安装包内随附的 DSH Web Host；不读取 repo_path、不调用 build。
    fn launch_packaged_web(
        self: &Arc<Self>,
        app: &AppHandle,
        packaged: &runtime::PackagedRuntime,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        let pid = self
            .supervisor
            .spawn_packaged_web(
                &packaged.node_binary,
                &packaged.cli_entry,
                &packaged.harness_root,
                settings.port,
                &settings.host,
                &packaged.dsh_home,
                &packaged.tools,
            )
            .map_err(OperationError::Failed)?;
        self.wait_for_web(
            app,
            "normal",
            pid,
            settings.port,
            settings.ready_timeout_ms,
            token,
        )
    }

    fn start_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        mode: &str,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        if !self.bootstrap_ready.load(Ordering::SeqCst) {
            self.fail(app, "启动器仍在准备运行环境", "请稍候片刻后重试启动");
            return Err(OperationError::Failed("bootstrap-pending".into()));
        }
        if let Some(error) = self.boot_error.lock().unwrap().clone() {
            self.fail(
                app,
                "启动安全检查失败",
                &format!(
                    "{error}\n未启动任何 DSH 运行时。请重新安装正式包；开发模式可使用本地仓库。"
                ),
            );
            return Err(OperationError::Failed("bootstrap-failed".into()));
        }
        if start_plan(mode).packaged {
            return self.start_normal_flow(app, token);
        }
        self.start_dev_flow(app, token)
    }

    /// 普通启动:只使用安装包内 Harness + manifest 运行时，不触碰本地 repo/build/watcher。
    fn start_normal_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        // 已在运行 → 直接召回(不打开系统浏览器;成功后自动进入 DeepSeek 工作区)
        {
            let snap = self.snapshot.lock().unwrap();
            if snap.state == LauncherState::Running && snap.web_pid.is_some() {
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Info,
                    "dsh web 已在运行,将自动进入 DeepSeek 工作区",
                );
                return Ok(());
            }
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
            return Err(OperationError::Failed("port-busy".into()));
        }
        self.set_snapshot(app, |s| {
            s.error = None;
            s.mode = LauncherMode::Normal;
            s.state = LauncherState::Starting;
            s.phase = "准备正式运行时…".into();
        });
        self.log_hub.append(
            "launcher",
            crate::contract::LogLevel::Info,
            &format!(
                "状态 → starting · 准备 packaged DSH(端口 {})",
                settings.port
            ),
        );
        self.set_operation_stage("准备正式运行时…");
        let packaged = match runtime::ensure_packaged_runtime(app, &self.log_hub, token, &|stage| {
            self.set_operation_stage(stage);
            self.set_snapshot(app, |s| s.phase = stage.to_string());
        }) {
            Ok(runtime) => runtime,
            Err(OperationError::Cancelled) => return Err(OperationError::Cancelled),
            Err(OperationError::Failed(error)) => {
                self.fail(
                    app,
                    "正式运行时不可用",
                    &format!(
                        "{error}\n安装包缺少运行资源或预配失败,请重新构建正式包；开发模式可以使用本地仓库。"
                    ),
                );
                return Err(OperationError::Failed(error));
            }
        };
        // 正式运行时可能刚刚下载/安装了托管 pnpm；后续插件、技能和配置
        // 操作都从 AppState 取工具，必须立即共享这次预配得到的工具链。
        self.apply_packaged_tools(&packaged.tools);
        token.check()?;
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Starting;
            s.phase = "启动正式 DSH Web Host…".into();
        });
        self.set_operation_stage("启动正式 DSH Web Host…");
        self.launch_packaged_web(app, &packaged, token)
    }

    /// 开发启动:保留本地 repo、源码构建、dev:web watcher 和源码入口。
    fn start_dev_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("开发模式仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err(OperationError::Failed("repo-unusable".into()));
        }
        {
            let snap = self.snapshot.lock().unwrap();
            if snap.state == LauncherState::Running && snap.web_pid.is_some() {
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Info,
                    "dsh web 已在运行,将自动进入 DeepSeek 工作区",
                );
                return Ok(());
            }
        }
        self.refresh_tools();
        if runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("开发模式需要 Node {}", runtime::NODE_RANGE_MSG),
                "当前未找到兼容 Node,tsx/tsdown 会崩溃。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err(OperationError::Failed("node-unsupported".into()));
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
            return Err(OperationError::Failed("port-busy".into()));
        }
        if self.tools.lock().unwrap().pnpm.is_none() {
            self.fail(
                app,
                "开发模式未找到 pnpm",
                "开发模式需要 pnpm run dev:web;请先安装 pnpm 或托管工具链",
            );
            return Err(OperationError::Failed("no-pnpm".into()));
        }
        if config::repo_needs_build(&settings.repo_path) {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Info,
                "开发仓库缺少构建产物,启动前执行源码构建",
            );
            self.set_snapshot(app, |s| {
                s.error = None;
                s.state = LauncherState::Building;
                s.phase = "开发模式源码构建中…".into();
            });
            self.set_operation_stage("开发模式源码构建中…");
            let tools = self.tools.lock().unwrap().clone();
            let ok = crate::services::build::run_build(
                &self.log_hub,
                &tools,
                &settings.repo_path,
                &settings.build_args,
                &|p| {
                    self.set_snapshot(app, |s| s.phase = p.to_string());
                },
                token,
            )?;
            token.check()?;
            if !ok {
                self.fail(
                    app,
                    "开发模式构建失败",
                    "源码构建失败,服务未启动;请查看日志尾部",
                );
                return Err(OperationError::Failed("build-failed".into()));
            }
            self.invalidate_env_cache();
        }
        self.set_snapshot(app, |s| {
            s.error = None;
            s.mode = LauncherMode::Dev;
            s.state = LauncherState::Starting;
            s.phase = "启动开发模式…".into();
        });
        self.set_operation_stage("启动开发模式…");
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
                token.check()?;
                self.fail(app, "开发模式启动失败", &e);
                return Err(OperationError::Failed(e));
            }
        }
        self.launch_source_web(app, "dev", token)
    }

    fn update_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err(OperationError::Failed("repo-unusable".into()));
        }
        if runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("更新并构建需要 Node {}", runtime::NODE_RANGE_MSG),
                "未找到兼容 Node,tsx/tsdown 会崩溃。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err(OperationError::Failed("node-unsupported".into()));
        }
        if self.tools.lock().unwrap().git.is_none() {
            self.fail(
                app,
                "未找到 git",
                "「更新并构建」需要 git,请先安装或使用「安装托管工具链」",
            );
            return Err(OperationError::Failed("no-git".into()));
        }
        let mode = self.snapshot.lock().unwrap().mode.clone();
        self.set_snapshot(app, |s| {
            s.error = None;
            s.mode = mode.clone();
            s.state = LauncherState::Syncing;
            s.phase = "同步远端…".into();
        });
        self.ops.set_stage(
            self.ops.current().map(|o| o.operation_id).unwrap_or(0),
            "同步远端…",
            None,
        );
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
        token.check()?;
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
            return Err(OperationError::Failed("sync-failed".into()));
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
            token,
        )?;
        let _ = needed;
        token.check()?;
        if !ok {
            self.fail(app, "依赖安装失败", "查看日志尾部(pnpm install 退出码非 0)");
            return Err(OperationError::Failed("install-failed".into()));
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
            token,
        )?;
        token.check()?;
        if !ok {
            self.fail(
                app,
                "构建失败",
                "退出码非 0;查看日志尾部定位到阶段。修复后重试「更新并构建」",
            );
            return Err(OperationError::Failed("build-failed".into()));
        }
        // dist 构建产物变化 → 环境缓存失效
        self.invalidate_env_cache();

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
        let r = self.launch_source_web(
            app,
            if mode == LauncherMode::Dev {
                "dev"
            } else {
                "normal"
            },
            token,
        );
        token.check()?;
        r
    }

    fn rebuild_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        let settings = config::load();
        let usable = config::repo_usable(&settings.repo_path);
        if !usable.ok {
            self.fail(
                app,
                &format!("仓库不可用:{}", settings.repo_path),
                usable.reason.as_deref().unwrap_or("未知"),
            );
            return Err(OperationError::Failed("repo-unusable".into()));
        }
        if runtime::resolve_dsh_node().is_none() {
            self.fail(
                app,
                &format!("重建并重启需要 Node {}", runtime::NODE_RANGE_MSG),
                "未找到兼容 Node。请在「设置 → 运行时」一键安装 Node 24 LTS",
            );
            return Err(OperationError::Failed("node-unsupported".into()));
        }
        if self.tools.lock().unwrap().pnpm.is_none() {
            self.fail(app, "未找到 pnpm", "请先安装 pnpm,然后重试");
            return Err(OperationError::Failed("no-pnpm".into()));
        }
        let mode = self.snapshot.lock().unwrap().mode.clone();
        self.set_snapshot(app, |s| {
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
        token.check()?;
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
            token,
        )?;
        token.check()?;
        if !ok {
            self.fail(
                app,
                "构建失败",
                "构建失败,服务已停止。修复后重试「重建并重启」",
            );
            return Err(OperationError::Failed("build-failed".into()));
        }
        // dist 构建产物变化 → 环境缓存失效
        self.invalidate_env_cache();
        self.refresh_repo();
        self.set_snapshot(app, |s| {
            s.state = LauncherState::Starting;
            s.phase = "启动 dsh web…".into();
        });
        let r = self.launch_source_web(
            app,
            if mode == LauncherMode::Dev {
                "dev"
            } else {
                "normal"
            },
            token,
        );
        token.check()?;
        r
    }

    // ── 托管工具链安装流程(M1:签名 catalog + 国内下载 + 校验 + 安全解压) ──

    fn install_node_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        token.check()?;
        if runtime::resolve_dsh_node().is_some() {
            self.log_hub.append(
                "launcher",
                crate::contract::LogLevel::Info,
                "Node 运行时已就绪,无需安装",
            );
            return Ok(());
        }
        self.set_snapshot(app, |s| {
            s.error = None;
            s.state = LauncherState::Starting;
            s.phase = "安装 Node 24 LTS…".into();
        });
        let result = runtime::install_node(&self.log_hub, token, &|p| {
            self.set_snapshot(app, |s| s.phase = p.to_string());
        });
        token.check()?;
        match result {
            Ok((path, version)) => {
                self.refresh_tools();
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    &format!("Node {version} 安装完成并已自动选用 → {}", path.display()),
                );
                self.set_snapshot(app, |s| {
                    s.state = LauncherState::Idle;
                    s.phase = String::new();
                });
                Ok(())
            }
            Err(OperationError::Cancelled) => Err(OperationError::Cancelled),
            Err(OperationError::Failed(e)) => {
                self.fail(app, "Node 安装失败", &e);
                Err(OperationError::Failed(e))
            }
        }
    }

    fn install_git_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.run_toolchain_install(
            app,
            token,
            crate::toolchain::Tool::Git,
            "安装托管 Git(MinGit)…",
        )
    }

    fn install_pnpm_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.run_toolchain_install(app, token, crate::toolchain::Tool::Pnpm, "安装托管 pnpm…")
    }

    fn install_toolchain_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.run_toolchain_install(
            app,
            token,
            crate::toolchain::Tool::All,
            "安装托管工具链(Node + Git + pnpm)…",
        )
    }

    fn run_toolchain_install(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
        tool: crate::toolchain::Tool,
        phase: &str,
    ) -> Result<(), OperationError> {
        token.check()?;
        self.set_snapshot(app, |s| {
            s.error = None;
            s.state = LauncherState::Starting;
            s.phase = phase.to_string();
        });
        let result = crate::toolchain::ensure_tool(
            &self.log_hub,
            tool,
            token,
            &|p| {
                self.set_snapshot(app, |s| s.phase = p.to_string());
            },
            &self.tools.lock().unwrap().clone(),
        );
        token.check()?;
        match result {
            Ok(report) => {
                self.refresh_tools();
                for line in report.messages {
                    self.log_hub
                        .append("launcher", crate::contract::LogLevel::Ok, &line);
                }
                self.set_snapshot(app, |s| {
                    s.state = LauncherState::Idle;
                    s.phase = String::new();
                });
                Ok(())
            }
            Err(OperationError::Cancelled) => Err(OperationError::Cancelled),
            Err(OperationError::Failed(e)) => {
                self.fail(app, "工具链安装失败", &e);
                Err(OperationError::Failed(e))
            }
        }
    }

    fn clone_repo_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.run_full_setup(app, token, false)
    }

    fn full_setup_flow(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.run_full_setup(app, token, true)
    }

    /// Clone(或一键全套)流程:远端验证 → staging clone → 校验 → 安装/构建/post-check
    /// → 原子提交最终目录 → 保存配置。目标非空绝不覆盖;失败只清理本次 staging。
    /// full 为 true 时在 staging 中完成 install + build + post-check 再提交。
    fn run_full_setup(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &CancellationToken,
        full: bool,
    ) -> Result<(), OperationError> {
        token.check()?;
        let Some(request) = self.pending_clone.lock().unwrap().take() else {
            self.fail(
                app,
                "没有待执行的克隆请求",
                "请先打开克隆弹窗填写 URL 与目标目录",
            );
            return Err(OperationError::Failed("no-clone-request".into()));
        };
        let done = crate::clone::run_clone_full(
            &self.log_hub,
            &request,
            full,
            token,
            &|stage, progress| {
                self.ops.set_stage(
                    self.ops.current().map(|o| o.operation_id).unwrap_or(0),
                    stage,
                    progress,
                );
                self.set_snapshot(app, |s| s.phase = stage.to_string());
            },
            &self.tools.lock().unwrap().clone(),
        );
        token.check()?;
        match done {
            Ok(result) => {
                // 提交最终目录成功后,保存 repoPath 与 last-good URL
                let mut settings = config::load();
                settings.repo_path = result.final_dir.clone();
                config::save(&settings).map_err(OperationError::Failed)?;
                crate::clone::remember_good_url(&request.url);
                self.refresh_tools();
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    &format!(
                        "仓库就绪 → {} (分支 {},HEAD {})",
                        result.final_dir, result.branch, result.head
                    ),
                );
                // 立即刷新仓库快照,仓库页/首页不再显示「读取不到」
                self.refresh_repo();
                self.set_snapshot(app, |s| {
                    s.state = LauncherState::Idle;
                    s.phase = String::new();
                    s.error = None;
                });
                Ok(())
            }
            Err(OperationError::Cancelled) => Err(OperationError::Cancelled),
            Err(OperationError::Failed(e)) => {
                self.fail(app, "克隆/安装失败", &e);
                Err(OperationError::Failed(e))
            }
        }
    }

    /// 安装后刷新当前进程采用的托管工具(修复“安装完成但需重启才生效”问题)。
    pub fn refresh_tools(&self) {
        let mut tools = self.tools.lock().unwrap();
        let node_dir =
            runtime::resolve_dsh_node().and_then(|(bin, _)| bin.parent().map(PathBuf::from));
        tools.dsh_node_dir = node_dir;
        let git = crate::toolchain::resolve_git(&tools.clone())
            .or_else(|| runtime::resolve_executable("git"));
        if let Some(g) = git {
            tools.git = Some(g);
        }
        let pnpm = crate::toolchain::resolve_pnpm(&tools.clone())
            .or_else(|| runtime::resolve_executable("pnpm"));
        if let Some(p) = pnpm {
            tools.pnpm = Some(p);
        }
        // 工具链可能已变化 → 环境缓存失效
        self.invalidate_env_cache();
    }

    /// 合并 packaged runtime 提供的工具，不覆盖已有的 Git 解析结果。
    /// 这样普通模式首次预配后，插件/技能操作可以直接使用刚安装的 pnpm。
    pub fn apply_packaged_tools(&self, packaged: &Tools) {
        let mut tools = self.tools.lock().unwrap();
        if packaged.pnpm.is_some() {
            tools.pnpm = packaged.pnpm.clone();
        }
        if packaged.dsh_node_dir.is_some() {
            tools.dsh_node_dir = packaged.dsh_node_dir.clone();
        }
        self.invalidate_env_cache();
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

    /// 下载并安装更新(独占 SelfUpdate 操作),完成后等待用户确认重启。
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
        let (id, _token) = match self.ops.begin(OperationKind::SelfUpdate, true, "下载更新…")
        {
            Ok(x) => x,
            Err(e) => {
                return ActionAccepted {
                    ok: false,
                    reason: Some(e),
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
                self.ops.finish(id, OperationStatus::Success, None);
                if let Err(e) = crate::config::mark_update_restart_pending() {
                    self.log_hub.append(
                        "launcher",
                        crate::contract::LogLevel::Warn,
                        &format!("更新已安装但无法写入延后重启标记:{e}"),
                    );
                }
                self.set_update(app, |u| {
                    u.installing = false;
                    u.available = true;
                    u.message = Some("更新已下载,等待重启应用".into());
                });
                self.log_hub.append(
                    "launcher",
                    crate::contract::LogLevel::Ok,
                    "更新已安装,等待用户确认重启",
                );
                ActionAccepted {
                    ok: true,
                    reason: Some("更新已下载,请确认是否重启应用".into()),
                    aborted: None,
                    already: None,
                }
            }
            Err(e) => {
                self.ops
                    .finish(id, OperationStatus::Failed, Some(e.to_string()));
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

    /// 用户确认后重启应用;清除延后重启标记避免下一次启动再次触发。
    pub fn restart_app(&self, app: &AppHandle) -> ! {
        crate::config::clear_update_restart_pending();
        app.restart()
    }

    pub fn open_dsh(&self) -> Result<(), String> {
        let url = crate::dsh_view::dsh_url();
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

    /// 安装快照(供设置页显示托管工具链状态;offered 读取时按当前 catalog 实时补齐)。
    pub fn installation(&self) -> InstallationSnapshot {
        let mut snap = crate::ops::load_installation();
        snap.offered = crate::toolchain::offered_versions();
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_start_plan_is_packaged_and_has_no_repo_build_or_watcher_prerequisite() {
        let plan = start_plan("normal");
        assert!(plan.packaged);
        assert!(!plan.requires_repo);
        assert!(!plan.builds_on_start);
        assert!(!plan.starts_watcher);
    }

    #[test]
    fn dev_start_plan_keeps_repo_build_and_watcher_flow() {
        let plan = start_plan("dev");
        assert!(!plan.packaged);
        assert!(plan.requires_repo);
        assert!(plan.builds_on_start);
        assert!(plan.starts_watcher);
    }

    #[test]
    fn env_cache_roundtrip_and_invalidation() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-envcache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));

        let empty = crate::contract::ToolRuntime {
            version: None,
            source: None,
            path: None,
            status: crate::contract::ToolCheck::Missing,
            verified: false,
            hint: None,
            managed_available: false,
        };
        let snap = EnvironmentSnapshot {
            repo_path: "/tmp/x".into(),
            repo_usable: crate::contract::RepoUsable {
                ok: true,
                reason: None,
            },
            dist_built: Some(true),
            platform: "test".into(),
            node: empty.clone(),
            pnpm: empty.clone(),
            git: empty,
            warnings: vec!["w".into()],
        };
        assert!(load_env_cache().is_none(), "无缓存时应为 None");
        save_env_cache(&snap);
        assert_eq!(load_env_cache(), Some(snap.clone()));
        std::fs::write(
            env_cache_file(),
            serde_json::json!({"cached_at_ms": now_ms(), "snapshot": snap}).to_string(),
        )
        .unwrap();
        assert!(load_env_cache().is_none(), "旧版本环境缓存必须自动失效");
        test_state().invalidate_env_cache();
        assert!(load_env_cache().is_none(), "失效后应清除");
        let _ = std::fs::remove_dir_all(&base);
    }

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

    #[test]
    fn packaged_tools_are_available_to_follow_up_operations() {
        let st = test_state();
        st.apply_packaged_tools(&Tools {
            pnpm: Some(PathBuf::from("/managed/pnpm")),
            git: None,
            dsh_node_dir: Some(PathBuf::from("/managed/node")),
        });
        let tools = st.tools.lock().unwrap().clone();
        assert_eq!(tools.pnpm, Some(PathBuf::from("/managed/pnpm")));
        assert_eq!(tools.dsh_node_dir, Some(PathBuf::from("/managed/node")));
        assert!(tools.git.is_none(), "packaged 工具不应覆盖已有 Git 状态");
    }

    fn test_state() -> AppState {
        let hub = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-state-test-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        let sink: Arc<crate::ops::OpSink> = Arc::new(|_| {});
        AppState::new(
            hub,
            Arc::new(Supervisor::new(Arc::new(LogHub::new(
                std::env::temp_dir().join(format!("dsh-state-sup-{}.log", std::process::id())),
                Arc::new(|_| {}),
                true,
            )))),
            Tools {
                pnpm: None,
                git: None,
                dsh_node_dir: None,
            },
            OperationCoordinator::new(
                Arc::new(LogHub::new(
                    std::env::temp_dir().join(format!("dsh-state-ops-{}.log", std::process::id())),
                    Arc::new(|_| {}),
                    true,
                )),
                sink,
            ),
            Arc::new(BootTimings::new(std::time::Instant::now())),
        )
    }

    #[test]
    fn action_matrix_disables_conflicting_actions() {
        let st = test_state();
        // 空闲:仓库/构建/工具链动作都允许
        let idle = AppSnapshot::mock_idle();
        assert!(st.can_run("update", &idle).is_ok());
        assert!(st.can_run("start", &idle).is_ok());
        assert!(st.can_run("install-toolchain", &idle).is_ok());

        // 运行中:仓库/构建动作被禁用并给出原因
        let mut running = AppSnapshot::mock_idle();
        running.state = LauncherState::Running;
        let err = st.can_run("update", &running).unwrap_err();
        assert!(err.contains("停止"), "{err}");
        assert!(
            st.can_run("start", &running).is_ok(),
            "运行中 start 是召回,允许"
        );

        // 有 exclusive-write 操作:start/dev/仓库动作全部禁用,stop/cancel 允许
        let mut busy = AppSnapshot::mock_idle();
        busy.operation = Some(crate::contract::OperationSnapshot {
            operation_id: 1,
            kind: OperationKind::CloneRepo,
            status: OperationStatus::Running,
            stage: "克隆中…".into(),
            progress: None,
            error: None,
            started_at: None,
            finished_at: None,
            cancellable: true,
        });
        let err = st.can_run("start", &busy).unwrap_err();
        assert!(err.contains("克隆仓库"), "{err}");
        assert!(st.can_run("stop", &busy).is_ok());
        assert!(st.can_run("cancel", &busy).is_ok());
        assert!(st.can_run("clear", &busy).is_ok());
        let err2 = st.can_run("clone-repo", &busy).unwrap_err();
        assert!(err2.contains("克隆仓库"), "{err2}");

        // save-settings 在任务期间禁用
        assert!(st.can_run("save-settings", &busy).is_err());
    }

    #[test]
    fn finalize_injects_operation_and_disabled() {
        let st = test_state();
        let s = st.snapshot();
        assert!(s.operation.is_none());
        assert!(s.disabled_actions.is_empty() || !s.busy);
    }

    #[test]
    fn first_run_done_requires_usable_repo_or_skipped_flag() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-state-fr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
        // 强制初始仓库不可用(避免本机默认 ~/Desktop/deepseek-harness 恰好存在)
        std::fs::write(
            base.join("dsh-launcher.json"),
            r#"{"repoPath":"/definitely/not/exists/dsh"}"#,
        )
        .unwrap();

        let st = test_state();
        // 仓库不可用且未跳过 → 首次运行未完成(向导应展示)
        let snap = st.desktop_snapshot();
        assert!(
            !snap.first_run_done,
            "仓库不可用且未跳过时必须展示首次运行向导"
        );

        // 跳过(标记 firstRunSkipped)后 → 首次运行完成,不再卡向导
        let patch = serde_json::json!({ "firstRunSkipped": true });
        config::apply_patch(&patch).unwrap();
        let snap2 = st.desktop_snapshot();
        assert!(snap2.first_run_done, "跳过后必须能进入启动器主界面");

        // 可用仓库本身也视为完成(不依赖跳过标记)
        std::fs::create_dir_all(base.join("proj/.git")).unwrap();
        config::apply_patch(&serde_json::json!({
            "repoPath": base.join("proj").display().to_string(),
            "firstRunSkipped": false,
        }))
        .unwrap();
        let snap3 = st.desktop_snapshot();
        assert!(snap3.first_run_done, "可用仓库即视为首次运行完成");

        let _ = std::fs::remove_dir_all(&base);
    }
}
