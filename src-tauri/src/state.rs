// dsh-launcher · 应用状态持有者
// M2:由 bridge 轮询 Node daemon 数据驱动,AppState 只是共享数据 + 命令转发。
use crate::bridge::{self, BridgeSupervisor, DaemonClient, PollState};
use crate::contract::{
    ActionAccepted, AppSnapshot, EnvironmentSnapshot, LogPage, SettingsSnapshot, UpdateResult,
};
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub poll_state: PollState,
    pub client: Mutex<Option<DaemonClient>>,
    pub supervisor: Mutex<Option<Arc<BridgeSupervisor>>>,
    pub boot_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            poll_state: PollState::default(),
            client: Mutex::new(None),
            supervisor: Mutex::new(None),
            boot_error: Mutex::new(None),
        }
    }

    /// 快照:bridge 数据;启动失败时附加诊断(应用仍可打开)。
    pub fn snapshot(&self) -> AppSnapshot {
        let mut snap = self.poll_state.snapshot.lock().unwrap().clone();
        if let Some(err) = self.boot_error.lock().unwrap().clone() {
            snap.error = Some(crate::contract::ErrorSummary {
                summary: "桌面核心启动失败".into(),
                detail: err,
            });
        }
        snap
    }

    /// 动作转发(bridge 白名单)。
    pub fn run_action(&self, action: &str) -> ActionAccepted {
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
            "quit",
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
        match self.client.lock().unwrap().as_ref() {
            Some(c) => c.run_action(action).unwrap_or(ActionAccepted {
                ok: false,
                reason: Some("Node 核心不可达".into()),
                aborted: None,
                already: None,
            }),
            None => ActionAccepted {
                ok: false,
                reason: self
                    .boot_error
                    .lock()
                    .unwrap()
                    .clone()
                    .or_else(|| Some("Node 核心未连接".into())),
                aborted: None,
                already: None,
            },
        }
    }

    pub fn logs(&self, _since_id: u64) -> LogPage {
        // M2:日志经 app://log-appended 事件流推送,查询接口保留契约占位
        LogPage {
            logs: vec![],
            sources: vec![
                "launcher".into(),
                "dsh web".into(),
                "dev:web".into(),
                "git".into(),
                "pnpm".into(),
            ],
        }
    }

    pub fn settings(&self) -> SettingsSnapshot {
        self.poll_state.settings.lock().unwrap().clone()
    }

    pub fn save_settings(&self, patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
        match self.client.lock().unwrap().as_ref() {
            Some(c) => {
                let s = c.save_settings(patch)?;
                *self.poll_state.settings.lock().unwrap() = s.clone();
                Ok(s)
            }
            None => Err("Node 核心未连接".into()),
        }
    }

    pub fn environment(&self) -> EnvironmentSnapshot {
        self.poll_state.environment.lock().unwrap().clone()
    }

    pub fn check_for_update(&self) -> UpdateResult {
        match self.client.lock().unwrap().as_ref() {
            Some(c) => c.update_check().unwrap_or(UpdateResult {
                ok: false,
                reason: Some("Node 核心不可达".into()),
                version: None,
                error: None,
            }),
            None => UpdateResult {
                ok: false,
                reason: self.boot_error.lock().unwrap().clone(),
                version: None,
                error: None,
            },
        }
    }

    pub fn apply_update(&self) -> ActionAccepted {
        match self.client.lock().unwrap().as_ref() {
            Some(c) => c.update_apply().unwrap_or(ActionAccepted {
                ok: false,
                reason: Some("Node 核心不可达".into()),
                aborted: None,
                already: None,
            }),
            None => ActionAccepted {
                ok: false,
                reason: self.boot_error.lock().unwrap().clone(),
                aborted: None,
                already: None,
            },
        }
    }

    pub fn open_dsh(&self) -> Result<(), String> {
        let s = self.settings();
        let url = format!("http://{}:{}/", s.host, s.port);
        tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| format!("打开 dsh 失败:{e}"))
    }

    pub fn open_repo_directory(&self) -> Result<(), String> {
        let s = self.settings();
        tauri_plugin_opener::open_path(std::path::Path::new(&s.repo_path), None::<&str>)
            .map_err(|e| format!("打开仓库目录失败:{e}"))
    }

    pub fn open_log_directory(&self) -> Result<(), String> {
        let dir = bridge::state_dir().join("logs");
        if !dir.exists() {
            let _ = std::fs::create_dir_all(&dir);
        }
        tauri_plugin_opener::open_path(&dir, None::<&str>)
            .map_err(|e| format!("打开日志目录失败:{e}"))
    }

    pub fn supervisor(&self) -> Option<Arc<BridgeSupervisor>> {
        self.supervisor.lock().unwrap().clone()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
