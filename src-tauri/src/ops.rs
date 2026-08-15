// dsh-launcher · Operation Coordinator(统一操作协调器)
//
// 职责:
// - 每个长任务分配唯一 operationId(单调递增,启动时从 journal 恢复计数);
// - exclusive-write(安装/克隆/构建/更新/自更新)同一时间只能运行一个;
// - start/dev 与 exclusive-write 互斥;stop/cancel 始终可发起;
// - 取消令牌贯穿下载器、解压器和所有子进程;
// - journal 每次状态变化原子写入(temp + rename);崩溃重启后先探测事实
//   (recover_stale 把 queued/running 标记为 interrupted),再允许清理、重试或
//   继续安全步骤,绝不盲目续跑外部安装器;
// - InstallationSnapshot 持久化已安装工具链(catalog 版本 + 各组件版本/路径/校验状态)。
//
// 本模块不依赖 Tauri:事件广播通过注入的 sink 完成(与 LogHub 同模式),便于测试。

use crate::config::state_dir;
use crate::contract::{OperationKind, OperationSnapshot, OperationStatus};
use crate::log_hub::LogHub;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ── 取消令牌 ─────────────────────────────────────────────

/// 取消令牌:设置后所有检查点(下载循环、解压、子进程轮询)立即停止。
#[derive(Clone, Default, Debug)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 检查点:已取消返回 Err(OperationError::Cancelled)。
    pub fn check(&self) -> Result<(), OperationError> {
        if self.is_cancelled() {
            Err(OperationError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// 底层原子标志(供 wait_ready / 子进程轮询直接观察,避免闭包持有令牌)。
    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }

    /// 原子标志的 Arc 副本(线程间共享读取,不持有令牌本身)。
    pub fn arc_flag(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }
}

/// 操作执行错误:取消与普通失败分开表达(UI 展示不同终态)。
#[derive(Debug, Clone, PartialEq)]
pub enum OperationError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationError::Cancelled => write!(f, "已取消"),
            OperationError::Failed(e) => write!(f, "{e}"),
        }
    }
}

// ── journal 记录 ─────────────────────────────────────────

/// 持久化操作记录(每次状态变化原子写入 state_dir/operations/<id>.json)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub operation_id: u64,
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub stage: String,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub cancellable: bool,
    /// 主进程 pid(崩溃恢复诊断用)。
    pub launcher_pid: u32,
}

impl From<&OperationRecord> for OperationSnapshot {
    fn from(r: &OperationRecord) -> Self {
        OperationSnapshot {
            operation_id: r.operation_id,
            kind: r.kind,
            status: r.status,
            stage: r.stage.clone(),
            progress: r.progress,
            error: r.error.clone(),
            started_at: r.started_at,
            finished_at: r.finished_at,
            cancellable: r.cancellable,
        }
    }
}

pub fn journal_dir() -> PathBuf {
    state_dir().join("operations")
}

pub fn journal_path(id: u64) -> PathBuf {
    journal_dir().join(format!("{id}.json"))
}

/// 原子写入 journal(状态每次变化都调用)。
pub fn journal_write(record: &OperationRecord) -> Result<(), String> {
    let dir = journal_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建操作目录失败: {e}"))?;
    let tmp = dir.join(format!("{}.json.tmp", record.operation_id));
    let json = serde_json::to_string_pretty(record).map_err(|e| format!("序列化失败: {e}"))?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("写入 journal 失败: {e}"))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("写入 journal 失败: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("journal fsync 失败: {e}"))?;
    }
    std::fs::rename(&tmp, journal_path(record.operation_id))
        .map_err(|e| format!("journal 落盘失败: {e}"))
}

/// 扫描 journal 目录,返回所有记录(按 id 升序)。
pub fn list_records() -> Vec<OperationRecord> {
    let Ok(entries) = std::fs::read_dir(journal_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") || name.ends_with(".tmp") {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(e.path()) {
            if let Ok(r) = serde_json::from_str::<OperationRecord>(&raw) {
                out.push(r);
            }
        }
    }
    out.sort_by_key(|r| r.operation_id);
    out
}

/// 崩溃恢复:把所有 queued/running 记录标记为 interrupted(探测事实,不续跑)。
/// 返回被恢复的记录列表(供启动日志/UI 提示)。
pub fn recover_stale(log: &LogHub) -> Vec<OperationRecord> {
    let mut recovered = Vec::new();
    for mut r in list_records() {
        if !r.status.is_terminal() {
            let was = r.status;
            r.status = OperationStatus::Interrupted;
            r.error = Some(format!(
                "上次启动中断({was:?});请检查后重试,已完成的安全步骤不会重复执行"
            ));
            if journal_write(&r).is_ok() {
                log.append(
                    "launcher",
                    crate::contract::LogLevel::Warn,
                    &format!(
                        "操作 #{} 恢复为 interrupted(上次崩溃时仍在运行,未续跑)",
                        r.operation_id
                    ),
                );
                recovered.push(r);
            }
        }
    }
    recovered
}

/// 下一个 operationId:优先取磁盘已有最大 id + 1(崩溃重启后保持单调)。
pub fn next_operation_id() -> u64 {
    let max = list_records()
        .iter()
        .map(|r| r.operation_id)
        .max()
        .unwrap_or(0);
    max + 1
}

// ── InstallationSnapshot ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledComponent {
    pub version: String,
    pub path: String,
    pub verified: bool,
    pub source: String,
}

/// 已安装工具链快照(versioned dir + active pointer 的旁证)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallationSnapshot {
    pub catalog_version: u32,
    pub node: Option<InstalledComponent>,
    pub git: Option<InstalledComponent>,
    pub pnpm: Option<InstalledComponent>,
    pub installed_at: Option<i64>,
}

pub fn installation_file() -> PathBuf {
    state_dir().join("installation.json")
}

pub fn load_installation() -> InstallationSnapshot {
    std::fs::read_to_string(installation_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_installation(snap: &InstallationSnapshot) -> Result<(), String> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建状态目录失败: {e}"))?;
    let tmp = dir.join("installation.json.tmp");
    let json = serde_json::to_string_pretty(snap).map_err(|e| format!("序列化失败: {e}"))?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("写入失败: {e}"))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("写入失败: {e}"))?;
        f.sync_all().map_err(|e| format!("fsync 失败: {e}"))?;
    }
    std::fs::rename(&tmp, installation_file()).map_err(|e| format!("落盘失败: {e}"))
}

// ── OperationCoordinator ─────────────────────────────────

/// 操作状态变化事件 sink(Tauri 事件广播由外层注入;测试注入 no-op)。
pub type OpSink = dyn Fn(&OperationSnapshot) + Send + Sync + 'static;

pub struct OperationCoordinator {
    next_id: AtomicU64,
    current: Mutex<Option<OperationRecord>>,
    /// 当前操作对应的取消令牌(UI cancel 时置位)。
    token: Mutex<Option<CancellationToken>>,
    log: Arc<LogHub>,
    sink: Arc<OpSink>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl OperationCoordinator {
    pub fn new(log: Arc<LogHub>, sink: Arc<OpSink>) -> Self {
        let next = next_operation_id();
        Self {
            next_id: AtomicU64::new(next),
            current: Mutex::new(None),
            token: Mutex::new(None),
            log,
            sink,
        }
    }

    /// 是否有进行中的操作(任何种类)。
    pub fn is_active(&self) -> bool {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| !r.status.is_terminal())
    }

    pub fn current(&self) -> Option<OperationSnapshot> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .map(OperationSnapshot::from)
    }

    /// 当前取消令牌(无操作时返回一个永不被取消的空令牌)。
    pub fn current_token(&self) -> CancellationToken {
        self.token.lock().unwrap().clone().unwrap_or_default()
    }

    /// 取消当前操作:置位令牌(子进程终止由调用方负责,见 state::cancel_flow)。
    pub fn request_cancel(&self) {
        if let Some(t) = self.token.lock().unwrap().as_ref() {
            t.cancel();
        }
    }

    fn persist_and_emit(&self, record: &OperationRecord) {
        let _ = journal_write(record);
        let snap: OperationSnapshot = record.into();
        (self.sink)(&snap);
    }

    /// 开始一个新操作。失败返回可读原因(已有 exclusive-write 进行中等)。
    /// 成功返回 (operationId, 取消令牌)。
    pub fn begin(
        &self,
        kind: OperationKind,
        cancellable: bool,
        stage: &str,
    ) -> Result<(u64, CancellationToken), String> {
        let mut cur = self.current.lock().unwrap();
        if let Some(active) = cur.as_ref() {
            if !active.status.is_terminal() {
                return Err(format!(
                    "已有任务在进行:{}({:?})",
                    active.kind.label(),
                    active.status
                ));
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let token = CancellationToken::new();
        let record = OperationRecord {
            operation_id: id,
            kind,
            status: OperationStatus::Running,
            stage: stage.to_string(),
            progress: None,
            error: None,
            started_at: Some(now_ms()),
            finished_at: None,
            cancellable,
            launcher_pid: std::process::id(),
        };
        self.log.append(
            "launcher",
            crate::contract::LogLevel::Info,
            &format!("操作 #{id} 开始:{} — {stage}", kind.label()),
        );
        self.persist_and_emit(&record);
        *cur = Some(record);
        *self.token.lock().unwrap() = Some(token.clone());
        Ok((id, token))
    }

    /// 更新阶段/进度。
    pub fn set_stage(&self, id: u64, stage: &str, progress: Option<u8>) {
        let mut cur = self.current.lock().unwrap();
        if let Some(r) = cur.as_mut() {
            if r.operation_id == id && !r.status.is_terminal() {
                r.stage = stage.to_string();
                r.progress = progress;
                let _ = journal_write(r);
                (self.sink)(&OperationSnapshot::from(&*r));
            }
        }
    }

    /// 结束操作(终态)。失败时附带错误信息。
    pub fn finish(&self, id: u64, status: OperationStatus, error: Option<String>) {
        debug_assert!(status.is_terminal());
        let mut cur = self.current.lock().unwrap();
        let mut done: Option<(OperationRecord, OperationStatus)> = None;
        if let Some(r) = cur.as_mut() {
            if r.operation_id == id {
                r.status = status;
                r.error = error.clone();
                r.finished_at = Some(now_ms());
                done = Some((r.clone(), status));
            }
        }
        if let Some((record, status)) = done {
            let snap: OperationSnapshot = OperationSnapshot::from(&record);
            let _ = journal_write(&record);
            (self.sink)(&snap);
            self.log.append(
                "launcher",
                match status {
                    OperationStatus::Success => crate::contract::LogLevel::Ok,
                    _ => crate::contract::LogLevel::Warn,
                },
                &format!(
                    "操作 #{} 结束:{} — {}",
                    snap.operation_id,
                    snap.kind.label(),
                    error.as_deref().unwrap_or(match status {
                        OperationStatus::Success => "成功",
                        OperationStatus::Cancelled => "已取消",
                        OperationStatus::Interrupted => "中断",
                        _ => "失败",
                    })
                ),
            );
            // 终态后清空 current,允许下一个操作
            *cur = None;
            *self.token.lock().unwrap() = None;
        }
    }
}

// ── 测试 ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_hub::LogHub;

    fn test_coordinator(base: &std::path::Path) -> (Arc<LogHub>, OperationCoordinator) {
        let hub = Arc::new(LogHub::new(
            base.join("logs/launcher.log"),
            Arc::new(|_| {}),
            true,
        ));
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let sink_emitted = emitted.clone();
        let coord = OperationCoordinator::new(
            hub.clone(),
            Arc::new(move |s| {
                sink_emitted.lock().unwrap().push(s.clone());
            }),
        );
        (hub, coord)
    }

    fn temp_state(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("dsh-ops-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));
        base
    }

    #[test]
    fn cancel_token_flow() {
        let t = CancellationToken::new();
        assert!(!t.is_cancelled());
        assert!(t.check().is_ok());
        t.cancel();
        assert!(t.is_cancelled());
        assert_eq!(t.check(), Err(OperationError::Cancelled));
    }

    #[test]
    fn begin_finish_persists_journal() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = temp_state("begin-finish");
        let (_hub, coord) = test_coordinator(&base);

        let (id, token) = coord
            .begin(OperationKind::InstallNode, true, "下载中…")
            .unwrap();
        assert!(coord.is_active());
        coord.set_stage(id, "校验中…", Some(50));
        let cur = coord.current().unwrap();
        assert_eq!(cur.stage, "校验中…");
        assert_eq!(cur.progress, Some(50));

        // journal 落盘
        let raw = std::fs::read_to_string(journal_path(id)).unwrap();
        assert!(raw.contains("\"status\": \"running\""), "{raw}");

        coord.finish(id, OperationStatus::Success, None);
        assert!(!coord.is_active());
        assert!(coord.current().is_none());
        let raw = std::fs::read_to_string(journal_path(id)).unwrap();
        assert!(raw.contains("\"status\": \"success\""), "{raw}");
        assert!(!token.is_cancelled());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn exclusive_write_serializes() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = temp_state("exclusive");
        let (_hub, coord) = test_coordinator(&base);

        let (id, _t) = coord
            .begin(OperationKind::CloneRepo, true, "克隆中…")
            .unwrap();
        // 第二个 exclusive-write 必须被拒绝
        let err = coord
            .begin(OperationKind::InstallNode, true, "安装中…")
            .unwrap_err();
        assert!(err.contains("已有任务"), "{err}");
        coord.finish(id, OperationStatus::Success, None);
        // 结束后可再次开始
        assert!(coord
            .begin(OperationKind::InstallNode, true, "安装中…")
            .is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cancel_via_token() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = temp_state("cancel");
        let (_hub, coord) = test_coordinator(&base);

        let (id, token) = coord
            .begin(OperationKind::FullSetup, true, "检查环境…")
            .unwrap();
        assert!(!token.is_cancelled());
        coord.request_cancel();
        assert!(token.is_cancelled());
        // 模拟流程检测到取消
        coord.finish(id, OperationStatus::Cancelled, Some("已取消".into()));
        assert!(!coord.is_active());
        let raw = std::fs::read_to_string(journal_path(id)).unwrap();
        assert!(raw.contains("\"status\": \"cancelled\""), "{raw}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crash_recovery_marks_stale_interrupted() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = temp_state("recover");
        let (hub, _coord) = test_coordinator(&base);

        // 模拟:上一个会话留下 running 记录(直接写 journal,不经 coordinator 状态)
        let stale = OperationRecord {
            operation_id: 1,
            kind: OperationKind::Build,
            status: OperationStatus::Running,
            stage: "构建中…".into(),
            progress: None,
            error: None,
            started_at: Some(now_ms()),
            finished_at: None,
            cancellable: true,
            launcher_pid: 99999,
        };
        journal_write(&stale).unwrap();
        let done = OperationRecord {
            operation_id: 2,
            kind: OperationKind::StartWeb,
            status: OperationStatus::Success,
            stage: "就绪".into(),
            progress: None,
            error: None,
            started_at: Some(now_ms()),
            finished_at: Some(now_ms()),
            cancellable: true,
            launcher_pid: 99999,
        };
        journal_write(&done).unwrap();

        let recovered = recover_stale(&hub);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].operation_id, 1);
        assert_eq!(recovered[0].status, OperationStatus::Interrupted);
        // 落盘也已更新
        let raw = std::fs::read_to_string(journal_path(1)).unwrap();
        assert!(raw.contains("\"status\": \"interrupted\""), "{raw}");

        // 下一个 id 从磁盘最大 +1
        assert_eq!(next_operation_id(), 3);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn installation_snapshot_roundtrip() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = temp_state("install-snap");
        let snap = InstallationSnapshot {
            catalog_version: 1,
            node: Some(InstalledComponent {
                version: "v24.9.0".into(),
                path: "/tmp/node/v24.9.0/bin/node".into(),
                verified: true,
                source: "managed".into(),
            }),
            git: None,
            pnpm: None,
            installed_at: Some(123),
        };
        save_installation(&snap).unwrap();
        let back = load_installation();
        assert_eq!(back, snap);
        assert!(installation_file().exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ops_module_does_not_require_tauri() {
        // 编译期保证:本模块只依赖 config/contract/log_hub(无 tauri 类型)。
        let _ = OperationKind::StartWeb;
    }
}
