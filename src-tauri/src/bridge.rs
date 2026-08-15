// dsh-launcher · 过渡期 bridge:连接/托管现有 Node daemon(3090)
// React 不直接访问 3090;本模块是唯一 HTTP 客户端(带 bearer token)。
// 删除条件:M4 完成全部 service 迁移到 Rust 后,与 Node daemon 一并删除。
use crate::contract::{
    ActionAccepted, AppSnapshot, EnvironmentSnapshot, LogPage, SettingsSnapshot, UpdateResult,
    EVENT_LOG_APPENDED, EVENT_STATE_CHANGED,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const LAUNCHER_PORT: u16 = 3090;
pub const BASE_URL: &str = "http://127.0.0.1:3090";

/// daemon 描述:本进程 spawn 的(owned)或接管已有的。
#[derive(Debug)]
pub struct DaemonHandle {
    pub pid: u32,
    pub owned: bool,
}

#[derive(Debug)]
pub struct DaemonClient {
    base: String,
    token: String,
}

impl Clone for DaemonClient {
    fn clone(&self) -> Self {
        Self {
            base: self.base.clone(),
            token: self.token.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PollState {
    pub snapshot: Arc<Mutex<AppSnapshot>>,
    pub settings: Arc<Mutex<SettingsSnapshot>>,
    pub environment: Arc<Mutex<EnvironmentSnapshot>>,
    pub logs_since: Arc<Mutex<u64>>,
    /// 本地日志 ring(上限 RING_CAP),Logs 页面历史来源;M4 由 LogHub 替代。
    pub ring: Arc<Mutex<std::collections::VecDeque<crate::contract::LogEntry>>>,
}

pub const RING_CAP: usize = 2_000;

impl Default for PollState {
    fn default() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(AppSnapshot::mock_idle())),
            settings: Arc::new(Mutex::new(SettingsSnapshot::default())),
            environment: Arc::new(Mutex::new(EnvironmentSnapshot {
                repo_path: String::new(),
                repo_usable: crate::contract::RepoUsable {
                    ok: false,
                    reason: Some("等待 Node 核心".into()),
                },
                dist_built: None,
                node: crate::contract::EnvironmentNode {
                    current: String::new(),
                    in_range: false,
                    used: None,
                    used_version: None,
                    used_source: None,
                },
                pnpm: None,
                git: None,
                warnings: vec![],
            })),
            logs_since: Arc::new(Mutex::new(0)),
            ring: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        }
    }
}

impl PollState {
    /// 追加日志到 ring(容量上限 RING_CAP,防止长期后台内存增长)。
    pub fn append_ring(&self, entries: Vec<crate::contract::LogEntry>) {
        let mut ring = self.ring.lock().unwrap();
        for e in entries {
            ring.push_back(e);
            while ring.len() > RING_CAP {
                ring.pop_front();
            }
        }
    }
}

impl DaemonClient {
    pub fn new(token: String) -> Self {
        Self {
            base: BASE_URL.to_string(),
            token,
        }
    }

    /// 旧版 daemon(无 token)兼容客户端。
    pub fn new_legacy() -> Self {
        Self {
            base: BASE_URL.to_string(),
            token: String::new(),
        }
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let req = if !self.token.is_empty() {
            match method {
                "GET" => ureq::get(&format!("{}{}", self.base, path))
                    .set("Authorization", &format!("Bearer {}", self.token)),
                _ => ureq::post(&format!("{}{}", self.base, path))
                    .set("Authorization", &format!("Bearer {}", self.token)),
            }
        } else {
            match method {
                "GET" => ureq::get(&format!("{}{}", self.base, path)),
                _ => ureq::post(&format!("{}{}", self.base, path)),
            }
        };
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => r
                .into_json::<serde_json::Value>()
                .map_err(|e| format!("解析响应失败: {e}")),
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                Err(format!("HTTP {code}: {text}"))
            }
            Err(e) => Err(format!("请求失败: {e}")),
        }
    }

    pub fn get_state(&self) -> Result<AppSnapshot, String> {
        let v = self.send("GET", "/api/state", None)?;
        serde_json::from_value(v.get("state").cloned().unwrap_or(v))
            .map_err(|e| format!("state 契约解析失败: {e}"))
    }

    /// 一次轮询:拉取 state / logs / config,更新共享状态并推送事件。
    pub fn poll(&self, ps: &PollState, app: &AppHandle) -> Result<(), String> {
        let v = self.send("GET", "/api/state", None)?;
        let snap: AppSnapshot = serde_json::from_value(v.get("state").cloned().unwrap_or(v))
            .map_err(|e| format!("state 契约解析失败: {e}"))?;
        {
            let mut cur = ps.snapshot.lock().unwrap();
            if *cur != snap {
                *cur = snap.clone();
                let _ = app.emit(EVENT_STATE_CHANGED, &snap);
            }
        }

        let since = *ps.logs_since.lock().unwrap();
        let v = self.send("GET", &format!("/api/logs?since={since}"), None)?;
        let page: LogPage =
            serde_json::from_value(v).map_err(|e| format!("logs 契约解析失败: {e}"))?;
        if !page.logs.is_empty() {
            let entries = page.logs;
            let last_id = entries.last().map(|l| l.id).unwrap_or(since);
            *ps.logs_since.lock().unwrap() = last_id;
            ps.append_ring(entries.clone());
            for entry in entries {
                let _ = app.emit(EVENT_LOG_APPENDED, &entry);
            }
        }

        if let Ok(v) = self.send("GET", "/api/config", None) {
            if let Some(cfg) = v.get("config") {
                if let Ok(settings) = serde_json::from_value::<SettingsSnapshot>(cfg.clone()) {
                    *ps.settings.lock().unwrap() = settings;
                }
            }
            if let Ok(env) = serde_json::from_value::<EnvironmentSnapshot>(v.clone()) {
                *ps.environment.lock().unwrap() = env;
            }
        }
        Ok(())
    }

    pub fn run_action(&self, action: &str) -> Result<ActionAccepted, String> {
        let v = self.send(
            "POST",
            "/api/action",
            Some(&serde_json::json!({ "action": action })),
        )?;
        serde_json::from_value(v).map_err(|e| format!("action 契约解析失败: {e}"))
    }

    pub fn update_check(&self) -> Result<UpdateResult, String> {
        let v = self.send(
            "POST",
            "/api/update",
            Some(&serde_json::json!({ "action": "check" })),
        )?;
        let ok = v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false);
        Ok(UpdateResult {
            ok,
            reason: v.get("reason").and_then(|x| x.as_str()).map(String::from),
            version: v
                .get("result")
                .and_then(|r| r.get("version"))
                .and_then(|x| x.as_str())
                .map(String::from),
            error: v.get("error").and_then(|x| x.as_str()).map(String::from),
        })
    }

    pub fn update_apply(&self) -> Result<ActionAccepted, String> {
        let v = self.send(
            "POST",
            "/api/update",
            Some(&serde_json::json!({ "action": "apply" })),
        )?;
        serde_json::from_value(v).map_err(|e| format!("update 契约解析失败: {e}"))
    }

    pub fn save_settings(&self, patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
        let v = self.send("POST", "/api/config", Some(patch))?;
        let cfg = v.get("config").cloned().unwrap_or(v);
        serde_json::from_value(cfg).map_err(|e| format!("config 契约解析失败: {e}"))
    }

    pub fn health(&self) -> Option<serde_json::Value> {
        match ureq::get(&format!("{}{}", self.base, "/api/health"))
            .timeout(Duration::from_secs(2))
            .call()
        {
            Ok(r) => r.into_json().ok(),
            Err(_) => None,
        }
    }
}

// ── 环境解析 ──────────────────────────────────────────────

fn which_in_path(name: &str) -> Option<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(sep) {
        let p = Path::new(dir).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// 解析 node 可执行(PATH → 常见安装目录 → nvm 最高版本)。
pub fn resolve_node() -> Option<PathBuf> {
    if let Some(p) = which_in_path("node") {
        return Some(p);
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
        let p = Path::new(dir).join("node");
        if p.is_file() {
            return Some(p);
        }
    }
    let home = std::env::var("HOME").ok()?;
    let root = Path::new(&home).join(".nvm/versions/node");
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let parts: Vec<u32> = name
                .trim_start_matches('v')
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            if parts.len() == 3 {
                let cand = e.path().join("bin/node");
                if cand.is_file() && best.as_ref().is_none_or(|(k, _)| parts > *k) {
                    best = Some((parts, cand));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// dev 运行:src/server.mjs 位于 src-tauri 的上级目录;打包形态由 M3+ 补充 resources。
pub fn server_path() -> Result<PathBuf, String> {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/server.mjs");
    if dev.is_file() {
        return Ok(dev);
    }
    Err("找不到 src/server.mjs(过渡期需在仓库内运行)".into())
}

/// 运行态目录:与 Node 侧 STATE_DIR 保持一致(env 可覆盖,便于测试)。
pub fn state_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_STATE_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&home).join(".local/state/dsh-launcher")
}

// ── token 管理 ────────────────────────────────────────────

pub fn token_file(state_dir: &Path) -> PathBuf {
    state_dir.join("daemon.token")
}

/// 读取或生成随机 bearer token(0600 落盘)。删除条件:随 bridge 一起删除。
pub fn load_or_create_token(state_dir: &Path) -> Result<String, String> {
    let f = token_file(state_dir);
    if let Ok(s) = std::fs::read_to_string(&f) {
        let t = s.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let mut buf = [0u8; 32];
    #[cfg(unix)]
    {
        let mut fh = std::fs::File::open("/dev/urandom")
            .map_err(|e| format!("无法打开 /dev/urandom: {e}"))?;
        fh.read_exact(&mut buf)
            .map_err(|e| format!("读取随机数失败: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        // Windows 过渡实现:时间+pid 混淆(仅防顺手,非强随机;M4 移除前需换强随机源)
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        buf[..8].copy_from_slice(&t.to_le_bytes());
        buf[8..12].copy_from_slice(&pid.to_le_bytes());
    }
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(state_dir).map_err(|e| format!("创建状态目录失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut fh = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&f)
            .map_err(|e| format!("写入 token 失败: {e}"))?;
        fh.write_all(hex.as_bytes())
            .map_err(|e| format!("写入 token 失败: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&f, &hex).map_err(|e| format!("写入 token 失败: {e}"))?;
    }
    Ok(hex)
}

// ── 进程工具 ──────────────────────────────────────────────

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_alive(pid: u32) -> bool {
    let _ = pid;
    false
}

fn read_pid_file(state_dir: &Path, name: &str) -> Option<u32> {
    let p = state_dir.join(name);
    let s = std::fs::read_to_string(&p).ok()?;
    s.trim().parse().ok()
}

fn port_holder(port: u16) -> Option<u32> {
    #[cfg(unix)]
    {
        let out = Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().nth(1)?;
        line.split_whitespace().nth(1)?.parse().ok()
    }
    #[cfg(not(unix))]
    {
        let _ = port;
        None
    }
}

fn ps_command(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// 3090 是否被监听(TCP connect 探测)。
pub fn port_busy(port: u16) -> bool {
    use std::net::TcpStream;
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(600),
    )
    .is_ok()
}

// ── daemon 生命周期 ───────────────────────────────────────

fn spawn_daemon(
    node: &Path,
    server: &Path,
    state_dir: &Path,
    token: &str,
) -> Result<Child, String> {
    let log_dir = state_dir.join("logs");
    std::fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("daemon-stdout.log"))
        .map_err(|e| format!("打开 daemon 日志失败: {e}"))?;
    let err = out
        .try_clone()
        .map_err(|e| format!("daemon 日志克隆失败: {e}"))?;
    Command::new(node)
        .arg(server)
        .env("DSH_LAUNCHER_TOKEN", token)
        .env("DSH_NO_AUTOOPEN", "1")
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("无法启动 Node 核心: {e}"))
}

/// 启动新 daemon 或接管已有实例。三重校验:pid 文件 / 命令行 / 端口。
/// 返回 (句柄, 客户端, owned 时的子进程句柄)。
pub fn start_or_takeover(
    state_dir: &Path,
) -> Result<(DaemonHandle, DaemonClient, Option<Child>), String> {
    let token = load_or_create_token(state_dir)?;

    if port_busy(LAUNCHER_PORT) {
        let client = DaemonClient::new(token.clone());
        if let Some(health) = client.health() {
            let pid = health.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
            if let Some(pid) = pid {
                let is_known = read_pid_file(state_dir, "launcher.pid")
                    .is_some_and(|saved| saved == pid && is_alive(pid));
                if is_known {
                    // 已运行的 dsh-launcher daemon:用 token 验证接管
                    if client.get_state().is_ok() {
                        return Ok((DaemonHandle { pid, owned: false }, client, None));
                    }
                    return Err("3090 上的 launcher 实例 token 不匹配,请先手动停止旧实例".into());
                }
            }
        }
        // 占用者不是已知 daemon:校验命令行,是旧版 launcher(无 token)则兼容接管
        if let Some(holder) = port_holder(LAUNCHER_PORT) {
            if let Some(cmd) = ps_command(holder) {
                if cmd.contains("server.mjs") {
                    let legacy = DaemonClient::new_legacy();
                    if legacy.get_state().is_ok() {
                        return Ok((
                            DaemonHandle {
                                pid: holder,
                                owned: false,
                            },
                            legacy,
                            None,
                        ));
                    }
                }
            }
        }
        return Err(format!(
            "端口 {LAUNCHER_PORT} 已被占用(pid {holder},非 dsh-launcher 实例),未做任何操作",
            holder = port_holder(LAUNCHER_PORT).unwrap_or(0)
        ));
    }

    // 启动新 daemon
    let node = resolve_node().ok_or_else(|| {
        "未找到 Node.js:请安装 Node(brew install node@24 或 nvm install 24)".to_string()
    })?;
    let server = server_path()?;
    log::info!("启动 Node 核心: {} {}", node.display(), server.display());
    let child = spawn_daemon(&node, &server, state_dir, &token)?;
    let pid = child.id();

    // 等待 daemon 就绪(≤10s)
    let client = DaemonClient::new(token.clone());
    for _ in 0..50 {
        if client.health().is_some() {
            return Ok((DaemonHandle { pid, owned: true }, client, Some(child)));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("Node 核心启动超时(10s),查看 ~/.local/state/dsh-launcher/logs/daemon-stdout.log".into())
}

// ── 轮询线程 ──────────────────────────────────────────────

pub struct BridgeSupervisor {
    pub stop: Arc<AtomicBool>,
    pub client: DaemonClient,
    pub daemon: Arc<Mutex<Option<Child>>>,
    pub poll_state: PollState,
}

/// 启动轮询线程:每 1s 拉取 state/logs/config,推送 Tauri 事件。
pub fn start_poller(app: AppHandle, sup: Arc<BridgeSupervisor>) {
    std::thread::spawn(move || {
        let mut failures = 0u32;
        while !sup.stop.load(Ordering::Relaxed) {
            match sup.client.poll(&sup.poll_state, &app) {
                Ok(_) => failures = 0,
                Err(e) => {
                    failures += 1;
                    if failures == 1 {
                        log::warn!("bridge 轮询失败: {e}");
                    }
                    // 连续失败且 daemon 归我们管:尝试拉起
                    if failures >= 5 {
                        let mut guard = sup.daemon.lock().unwrap();
                        let dead = match guard.as_mut() {
                            Some(c) => c.try_wait().ok().flatten().is_some(),
                            None => true,
                        };
                        if dead && sup.client.health().is_none() {
                            if let Some(node) = resolve_node() {
                                if let Ok(server) = server_path() {
                                    match spawn_daemon(
                                        &node,
                                        &server,
                                        &state_dir(),
                                        &sup.client.token,
                                    ) {
                                        Ok(c) => {
                                            log::info!("bridge 重启 Node 核心");
                                            *guard = Some(c);
                                        }
                                        Err(e) => log::error!("重启 Node 核心失败: {e}"),
                                    }
                                }
                            }
                        }
                        failures = 0;
                    }
                }
            }
            crate::tray::refresh(&app);
            std::thread::sleep(Duration::from_millis(1000));
        }
        log::info!("bridge 轮询已停止");
    });
}

/// 应用退出:通知 daemon detach(不停止 dsh web),停止轮询。
pub fn shutdown(sup: &BridgeSupervisor) {
    sup.stop.store(true, Ordering::Relaxed);
    let _ = sup.client.run_action("detach");
    if let Some(mut child) = sup.daemon.lock().unwrap().take() {
        let _ = child.try_wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("dsh-bridge-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn token_create_read_roundtrip_and_perms() {
        let dir = temp_dir("token");
        let t1 = load_or_create_token(&dir).unwrap();
        assert_eq!(t1.len(), 64, "token 应为 32 字节 hex");
        let t2 = load_or_create_token(&dir).unwrap();
        assert_eq!(t1, t2, "重复读取应幂等");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(token_file(&dir)).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600, "token 文件权限应为 0600");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_node_finds_executable() {
        let node = resolve_node().expect("测试机应有 node");
        assert!(node.is_file());
    }

    /// 3090 全局资源:串行化相关测试(避免并行竞争端口)。
    static PORT_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn start_or_takeover_rejects_foreign_occupant() {
        let _guard = PORT_LOCK.lock().unwrap();
        if port_busy(LAUNCHER_PORT) {
            eprintln!("跳过:端口 3090 已被占用");
            return;
        }
        // 用一个非 daemon 进程占用 3090
        let listener = std::net::TcpListener::bind("127.0.0.1:3090").unwrap();
        let dir = temp_dir("foreign");
        let err = start_or_takeover(&dir).unwrap_err();
        assert!(err.contains("已被占用"), "err = {err}");
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端:启动真实 Node 核心(隔离 state dir),接管并执行动作,detach 退出。
    #[test]
    fn spawn_daemon_poll_and_action() {
        let _guard = PORT_LOCK.lock().unwrap();
        if port_busy(LAUNCHER_PORT) {
            eprintln!("跳过:端口 3090 已被占用");
            return;
        }
        let dir = temp_dir("daemon");
        let old_env = std::env::var("DSH_LAUNCHER_STATE_DIR").ok();
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &dir);
        let (handle, client, child) = start_or_takeover(&dir).expect("应能启动 daemon");
        assert!(handle.owned, "空闲端口应为本进程托管 daemon");

        let snap = client.get_state().expect("state 契约应解析");
        assert_eq!(snap.state, crate::contract::LauncherState::Idle);
        let act = client.run_action("clear").expect("动作应成功");
        assert!(act.ok);
        let settings = client
            .save_settings(&serde_json::json!({ "port": 3081 }))
            .expect("设置应保存");
        assert_eq!(settings.port, 3081);
        // 还原端口,避免影响其他测试
        let _ = client.save_settings(&serde_json::json!({ "port": 3080 }));
        // daemon 退出(detach 语义)
        let _ = client.run_action("detach");
        for _ in 0..50 {
            if !port_busy(LAUNCHER_PORT) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!port_busy(LAUNCHER_PORT), "daemon 应已退出");
        if let Some(mut c) = child {
            let _ = c.try_wait();
        }
        if let Some(e) = old_env {
            std::env::set_var("DSH_LAUNCHER_STATE_DIR", e);
        } else {
            std::env::remove_var("DSH_LAUNCHER_STATE_DIR");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
