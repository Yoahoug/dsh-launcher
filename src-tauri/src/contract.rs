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
        }
    }
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
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
}
