// dsh-launcher · 前后端共享契约(Rust 侧定义,与 src-ui/src/types/schema.ts 对齐)
// 变更时需同步两侧,并通过 contract tests / 手工核对保持不漂移。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LauncherState {
    Idle,
    Syncing,
    Installing,
    Building,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LauncherMode {
    None,
    Normal,
    Dev,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
    Err,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSummary {
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoSnapshot {
    pub branch: String,
    pub head: String,
    pub behind: i64,
    pub ahead: i64,
    pub dirty: bool,
    pub dirty_files: u64,
    pub sync_at: Option<i64>,
    pub remote_up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub mode: Option<String>,
    pub checking: bool,
    pub available: bool,
    pub version: Option<String>,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub notes: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub installing: bool,
    pub progress: Option<u8>,
}

/// 与 src/state.mjs `state` 对象对齐的完整快照。
/// 长任务种类。
/// exclusive-write 分组(安装/克隆/构建/更新/自更新)同一时间只能运行一个;
/// start/dev 与 exclusive-write 互斥;stop/cancel 始终可发起。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    InstallNode,
    InstallGit,
    InstallPnpm,
    InstallToolchain,
    CloneRepo,
    FullSetup,
    InstallDeps,
    Build,
    UpdateRebuild,
    RebuildRestart,
    StartWeb,
    StartDev,
    StopAll,
    SelfUpdate,
}

impl OperationKind {
    /// 是否属于 exclusive-write(同一时间只能运行一个)。
    pub fn is_exclusive_write(self) -> bool {
        matches!(
            self,
            OperationKind::InstallNode
                | OperationKind::InstallGit
                | OperationKind::InstallPnpm
                | OperationKind::InstallToolchain
                | OperationKind::CloneRepo
                | OperationKind::FullSetup
                | OperationKind::InstallDeps
                | OperationKind::Build
                | OperationKind::UpdateRebuild
                | OperationKind::RebuildRestart
                | OperationKind::SelfUpdate
        )
    }

    /// 中文动作名(日志/UI 提示)。
    pub fn label(self) -> &'static str {
        match self {
            OperationKind::InstallNode => "安装 Node",
            OperationKind::InstallGit => "安装 Git",
            OperationKind::InstallPnpm => "安装 pnpm",
            OperationKind::InstallToolchain => "安装工具链",
            OperationKind::CloneRepo => "克隆仓库",
            OperationKind::FullSetup => "一键安装",
            OperationKind::InstallDeps => "安装依赖",
            OperationKind::Build => "构建",
            OperationKind::UpdateRebuild => "更新并构建",
            OperationKind::RebuildRestart => "重建并重启",
            OperationKind::StartWeb => "启动 dsh",
            OperationKind::StartDev => "启动开发模式",
            OperationKind::StopAll => "停止",
            OperationKind::SelfUpdate => "应用自更新",
        }
    }
}

/// 操作状态:只有 Success 才是终态成功;Failed/Cancelled/Interrupted 均为终态失败。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
    Interrupted,
}

impl OperationStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OperationStatus::Success
                | OperationStatus::Failed
                | OperationStatus::Cancelled
                | OperationStatus::Interrupted
        )
    }
}

/// 单个长任务的可见快照(写入 AppSnapshot.operation)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    pub operation_id: u64,
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub stage: String,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub cancellable: bool,
}

/// 被动作矩阵禁用的动作及其具体原因(UI 按钮禁用时展示)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisabledAction {
    pub action: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub version: String,
    pub state: LauncherState,
    pub mode: LauncherMode,
    pub phase: String,
    pub error: Option<ErrorSummary>,
    pub url: Option<String>,
    pub web_pid: Option<u32>,
    pub dev_pid: Option<u32>,
    pub started_at: Option<i64>,
    pub ready_at: Option<i64>,
    pub hmr_active: bool,
    pub repo: RepoSnapshot,
    pub busy: bool,
    pub launcher_pid: u32,
    pub update: UpdateSnapshot,
    /// 当前长任务(无任务时 None)。UI 只在 status == success 时显示成功。
    pub operation: Option<OperationSnapshot>,
    /// 动作矩阵:被禁用的动作与原因。
    pub disabled_actions: Vec<DisabledAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub ts: i64,
    pub src: String,
    pub level: LogLevel,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub logs: Vec<LogEntry>,
    pub sources: Vec<String>,
}

/// 设置(与 src/config.mjs DEFAULTS 对齐)。engine 行为,由 Node daemon 持久化。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub repo_path: String,
    pub port: u16,
    pub host: String,
    pub dsh_home: String,
    pub autostart: bool,
    pub open_browser: bool,
    pub auto_update_check: bool,
    pub build_args: String,
    pub ready_timeout_ms: u64,
    pub start_timeout_ms: u64,
}

/// 主题偏好。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// 关闭窗口行为。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    #[default]
    Tray,
    Quit,
}

/// 桌面偏好:仅由 Rust 持久化,不写入 Node 配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub theme: Theme,
    pub close_behavior: CloseBehavior,
    pub launch_on_startup: bool,
    pub silent_startup: bool,
    pub show_tray_icon: bool,
    pub confirm_stop_and_quit: bool,
}

/// 桌面信息:偏好 + 首次运行状态(供 First-run 流程判定)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    pub preferences: DesktopPreferences,
    pub first_run_done: bool,
    pub version: String,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            close_behavior: CloseBehavior::default(),
            launch_on_startup: false,
            silent_startup: false,
            show_tray_icon: true,
            confirm_stop_and_quit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoUsable {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentNode {
    pub current: String,
    pub in_range: bool,
    pub used: Option<String>,
    pub used_version: Option<String>,
    pub used_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub repo_path: String,
    pub repo_usable: RepoUsable,
    pub dist_built: Option<bool>,
    pub node: EnvironmentNode,
    pub pnpm: Option<String>,
    pub git: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionAccepted {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aborted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 事件名(与前端 EVENTS 常量对齐)。M2 bridge 开始使用,届时移除 allow。
pub const EVENT_STATE_CHANGED: &str = "app://state-changed";
pub const EVENT_LOG_APPENDED: &str = "app://log-appended";
/// 托盘要求 renderer 打开某页面(取值 dashboard|logs|settings)。
pub const EVENT_OPEN_PAGE: &str = "app://open-page";
/// 桌面偏好已变更(Renderer 应用主题等)。
pub const EVENT_PREFERENCES_CHANGED: &str = "app://preferences-changed";

impl AppSnapshot {
    /// M1 mock:空闲快照(M2 由 bridge 提供真实数据)。
    pub fn mock_idle() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            state: LauncherState::Idle,
            mode: LauncherMode::None,
            phase: String::new(),
            error: None,
            url: None,
            web_pid: None,
            dev_pid: None,
            started_at: None,
            ready_at: None,
            hmr_active: false,
            repo: RepoSnapshot {
                branch: String::new(),
                head: String::new(),
                behind: -1,
                ahead: -1,
                dirty: false,
                dirty_files: 0,
                sync_at: None,
                remote_up_to_date: true,
            },
            busy: false,
            launcher_pid: std::process::id(),
            update: UpdateSnapshot {
                mode: None,
                checking: false,
                available: false,
                version: None,
                url: None,
                size: None,
                notes: None,
                message: None,
                error: None,
                installing: false,
                progress: None,
            },
            operation: None,
            disabled_actions: Vec::new(),
        }
    }
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        let home = crate::config::home_dir();
        Self {
            repo_path: format!("{home}/Desktop/deepseek-harness"),
            port: 3080,
            host: "127.0.0.1".into(),
            dsh_home: String::new(),
            autostart: false,
            open_browser: true,
            auto_update_check: true,
            build_args: String::new(),
            ready_timeout_ms: 120_000,
            start_timeout_ms: 120_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前后端契约抽查:M1 前端 mock 快照与 Rust 结构互转。
    #[test]
    fn snapshot_serde_roundtrip() {
        let s = AppSnapshot::mock_idle();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // 字段命名必须 camelCase(与 TS schema 一致)
        assert!(json.contains("\"webPid\""));
        assert!(json.contains("\"launcherPid\""));
        assert!(json.contains("\"startedAt\""));
        assert!(json.contains("\"remoteUpToDate\""));
        let s2 = SettingsSnapshot::default();
        let j2 = serde_json::to_string(&s2).unwrap();
        assert!(j2.contains("\"readyTimeoutMs\""));
        assert!(j2.contains("\"repoPath\""));
        assert!(j2.contains("\"autoUpdateCheck\""));
    }

    #[test]
    fn action_payload_shape() {
        let ok = ActionAccepted {
            ok: true,
            reason: None,
            aborted: None,
            already: None,
        };
        let j = serde_json::to_string(&ok).unwrap();
        assert_eq!(j, r#"{"ok":true}"#);
    }

    #[test]
    fn state_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&LauncherState::Running).unwrap(),
            r#""running""#
        );
        assert_eq!(
            serde_json::to_string(&LauncherMode::Dev).unwrap(),
            r#""dev""#
        );
    }

    #[test]
    fn operation_kind_exclusive_write_groups() {
        assert!(OperationKind::InstallNode.is_exclusive_write());
        assert!(OperationKind::CloneRepo.is_exclusive_write());
        assert!(OperationKind::FullSetup.is_exclusive_write());
        assert!(OperationKind::UpdateRebuild.is_exclusive_write());
        assert!(OperationKind::SelfUpdate.is_exclusive_write());
        assert!(!OperationKind::StartWeb.is_exclusive_write());
        assert!(!OperationKind::StartDev.is_exclusive_write());
        assert!(!OperationKind::StopAll.is_exclusive_write());
    }

    #[test]
    fn operation_snapshot_serde_camel() {
        let op = OperationSnapshot {
            operation_id: 7,
            kind: OperationKind::CloneRepo,
            status: OperationStatus::Running,
            stage: "克隆中…".into(),
            progress: Some(42),
            error: None,
            started_at: Some(1),
            finished_at: None,
            cancellable: true,
        };
        let j = serde_json::to_string(&op).unwrap();
        assert!(j.contains("\"operationId\":7"), "{j}");
        assert!(j.contains("\"kind\":\"clone_repo\""), "{j}");
        assert!(j.contains("\"status\":\"running\""), "{j}");
        let back: OperationSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, op);
        assert!(!OperationStatus::Running.is_terminal());
        assert!(OperationStatus::Success.is_terminal());
        assert!(OperationStatus::Cancelled.is_terminal());
        assert!(OperationStatus::Interrupted.is_terminal());
    }

    #[test]
    fn snapshot_roundtrip_with_operation_and_disabled() {
        let mut s = AppSnapshot::mock_idle();
        s.operation = Some(OperationSnapshot {
            operation_id: 3,
            kind: OperationKind::InstallNode,
            status: OperationStatus::Running,
            stage: "下载中…".into(),
            progress: None,
            error: None,
            started_at: None,
            finished_at: None,
            cancellable: true,
        });
        s.disabled_actions = vec![DisabledAction {
            action: "start".into(),
            reason: "正在安装 Node".into(),
        }];
        let j = serde_json::to_string(&s).unwrap();
        let back: AppSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert!(j.contains("\"operationId\":3"), "{j}");
    }
}
