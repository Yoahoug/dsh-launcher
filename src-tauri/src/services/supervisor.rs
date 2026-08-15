// dsh-launcher · Supervisor:托管 dsh web / dev:web 进程树
// Unix:setsid 新会话(进程组 leader),停止对进程组 SIGTERM → 5s → SIGKILL;
// Windows:CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW + Job Object(不设 KILL_ON_JOB_CLOSE,
// 保证 launcher 退出后 dsh 继续运行;stop 时优雅信号优先,最终 TerminateJobObject)。
use crate::config::{logs_dir, state_dir};
use crate::contract::LogLevel;
use crate::log_hub::LogHub;
use crate::services::runtime::Tools;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 就绪行正则(与 dsh 仓库测试同款)。
const READY_RE: &str = r"dsh web: (http://[^\s]+)";

/// 单条托管进程。
#[derive(Clone)]
pub struct Managed {
    pub pid: u32,
    pub label: &'static str,
    pub started_at: Option<i64>,
    pub ready_at: Option<i64>,
    pub url: Option<String>,
    /// 进程已退出(reader 线程置位)。
    pub exited: Arc<AtomicBool>,
    /// 就绪信号(reader 线程置位)。
    pub ready: Arc<Mutex<Option<String>>>,
    #[cfg(windows)]
    pub job: windows_sys::Win32::Foundation::HANDLE,
}

/// Windows Job Object HANDLE 可跨线程使用,声明 Send(供 Mutex<Option<Managed>>)。
#[cfg(windows)]
unsafe impl Send for Managed {}

impl Managed {
    fn new(
        pid: u32,
        label: &'static str,
        exited: Arc<AtomicBool>,
        ready: Arc<Mutex<Option<String>>>,
        #[cfg(windows)] job: windows_sys::Win32::Foundation::HANDLE,
    ) -> Self {
        Self {
            pid,
            label,
            started_at: None,
            ready_at: None,
            url: None,
            exited,
            ready,
            #[cfg(windows)]
            job,
        }
    }
}

/// 进程托管状态(PID 文件 + 状态持久化共用)。
pub struct Supervisor {
    pub web: Mutex<Option<Managed>>,
    pub dev: Mutex<Option<Managed>>,
    pub log: Arc<LogHub>,
}

/// 终止结果。
#[derive(Debug, PartialEq)]
pub enum StopOutcome {
    Exited,
    Killed,
    Missing,
}

impl Supervisor {
    pub fn new(log: Arc<LogHub>) -> Self {
        Self {
            web: Mutex::new(None),
            dev: Mutex::new(None),
            log,
        }
    }

    /// 进程是否存活(pid 校验)。
    #[cfg(unix)]
    pub fn is_alive(pid: u32) -> bool {
        let r = unsafe { libc::kill(pid as i32, 0) };
        r == 0
    }

    #[cfg(windows)]
    pub fn is_alive(pid: u32) -> bool {
        crate::services::supervisor::win::is_alive(pid)
    }

    fn take(&self, label: &'static str) -> Option<Managed> {
        if label == "dsh web" {
            self.web.lock().unwrap().take()
        } else {
            self.dev.lock().unwrap().take()
        }
    }

    /// 启动 pnpm dsh web --port <port> [--host <host>]。
    pub fn spawn_web(
        &self,
        tools: &Tools,
        repo_path: &str,
        port: u16,
        host: &str,
        dsh_home: &str,
        on_ready: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<u32, String> {
        let pnpm = tools
            .pnpm
            .as_ref()
            .ok_or_else(|| "未找到 pnpm".to_string())?
            .clone();
        let port_s = port.to_string();
        let mut args = vec!["dsh", "web", "--port", port_s.as_str()];
        let host_owned;
        if host != "127.0.0.1" && !host.is_empty() {
            host_owned = format!("--host={host}");
            args.push(host_owned.as_str());
        }
        let mut cmd = std::process::Command::new(&pnpm);
        cmd.args(&args);
        cmd.current_dir(repo_path);
        cmd.envs(tools.env());
        if !dsh_home.is_empty() {
            cmd.env("DSH_HOME", dsh_home);
        }
        cmd.env("DSH_NO_AUTOOPEN", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 新会话:进程组 leader(detach 后 dsh 继续后台运行)
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        crate::services::supervisor::win::configure_spawn(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("无法启动 pnpm {args:?}:{e}"))?;
        let pid = child.id();
        let exited = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Mutex::new(None));

        #[cfg(windows)]
        let job = crate::services::supervisor::win::create_job(&child);
        #[cfg(windows)]
        if job.is_null() {
            let _ = child.kill();
            return Err("Windows Job Object 创建失败".into());
        }

        // 输出读取线程(stdout/stderr 并发消费,防管道写满死锁):
        // 两个 reader 线程各自逐行 → LogHub + 就绪行检测(共享 tail/ready)。
        let label: &'static str = "dsh web";
        let log = self.log.clone();
        let ready_flag = ready.clone();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let ready_re = regex::Regex::new(READY_RE).expect("就绪正则必须合法");

        let reader = |stream: Option<Box<dyn std::io::Read + Send>>,
                      is_err: bool,
                      log: Arc<LogHub>,
                      ready_flag: Arc<Mutex<Option<String>>>,
                      tail: Arc<Mutex<Vec<String>>>,
                      ready_re: regex::Regex| {
            let Some(stream) = stream else { return };
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                let level = if is_err {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                };
                log.append(label, level, &line);
                {
                    let mut t = tail.lock().unwrap();
                    t.push(line.clone());
                    if t.len() > 100 {
                        t.remove(0);
                    }
                }
                if let Some(cap) = ready_re.captures(&line) {
                    if let Some(url) = cap.get(1) {
                        let mut r = ready_flag.lock().unwrap();
                        if r.is_none() {
                            *r = Some(url.as_str().to_string());
                        }
                    }
                }
            }
        };
        let mut threads = Vec::new();
        threads.push(std::thread::spawn({
            let log = log.clone();
            let ready_flag = ready_flag.clone();
            let tail = tail.clone();
            let ready_re = ready_re.clone();
            move || {
                reader(
                    stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
                    false,
                    log,
                    ready_flag,
                    tail,
                    ready_re,
                )
            }
        }));
        threads.push(std::thread::spawn({
            let log = log.clone();
            let ready_flag = ready_flag.clone();
            let tail = tail.clone();
            let ready_re = ready_re.clone();
            move || {
                reader(
                    stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
                    true,
                    log,
                    ready_flag,
                    tail,
                    ready_re,
                )
            }
        }));

        // 等待子进程退出(监视线程,退出时置位)
        let exited_flag2 = exited.clone();
        let log2 = self.log.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            match status {
                Ok(s) => log2.append(
                    label,
                    LogLevel::Info,
                    &format!("进程退出 code={}", s.code().unwrap_or(-1)),
                ),
                Err(e) => log2.append(label, LogLevel::Err, &format!("wait 失败:{e}")),
            }
            for t in threads {
                let _ = t.join();
            }
            exited_flag2.store(true, Ordering::SeqCst);
        });

        self.web.lock().unwrap().replace(Managed::new(
            pid,
            label,
            exited,
            ready,
            #[cfg(windows)]
            job,
        ));
        let _ = on_ready;
        Ok(pid)
    }

    /// 启动 pnpm run dev:web(HMR watcher)。
    pub fn spawn_dev(&self, tools: &Tools, repo_path: &str) -> Result<u32, String> {
        let pnpm = tools
            .pnpm
            .as_ref()
            .ok_or_else(|| "未找到 pnpm".to_string())?
            .clone();
        let mut cmd = std::process::Command::new(&pnpm);
        cmd.args(["run", "dev:web"]);
        cmd.current_dir(repo_path);
        cmd.envs(tools.env());
        cmd.env("DSH_NO_AUTOOPEN", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        crate::services::supervisor::win::configure_spawn(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("无法启动 pnpm run dev:web:{e}"))?;
        let pid = child.id();
        let exited = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Mutex::new(None));
        #[cfg(windows)]
        let job = crate::services::supervisor::win::create_job(&child);
        #[cfg(windows)]
        if job.is_null() {
            let _ = child.kill();
            return Err("Windows Job Object 创建失败".into());
        }

        let label: &'static str = "dev:web";
        let log = self.log.clone();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        // stdout/stderr 并发消费(防管道写满死锁)
        let pump =
            |stream: Option<Box<dyn std::io::Read + Send>>, is_err: bool, log: Arc<LogHub>| {
                let Some(stream) = stream else { return };
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    log.append(
                        label,
                        if is_err {
                            LogLevel::Warn
                        } else {
                            LogLevel::Info
                        },
                        &line,
                    );
                }
            };
        let mut reader_threads = Vec::new();
        reader_threads.push(std::thread::spawn({
            let log = log.clone();
            move || {
                pump(
                    stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
                    false,
                    log,
                )
            }
        }));
        reader_threads.push(std::thread::spawn({
            let log = log.clone();
            move || {
                pump(
                    stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
                    true,
                    log,
                )
            }
        }));
        let exited_flag2 = exited.clone();
        let log2 = self.log.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            match status {
                Ok(s) => log2.append(
                    label,
                    LogLevel::Info,
                    &format!("进程退出 code={}", s.code().unwrap_or(-1)),
                ),
                Err(e) => log2.append(label, LogLevel::Err, &format!("wait 失败:{e}")),
            }
            for t in reader_threads {
                let _ = t.join();
            }
            exited_flag2.store(true, Ordering::SeqCst);
        });

        self.dev.lock().unwrap().replace(Managed::new(
            pid,
            label,
            exited,
            ready,
            #[cfg(windows)]
            job,
        ));
        Ok(pid)
    }

    /// 等待就绪(就绪行 + 端口确认);超时/早退返回诊断。
    pub fn wait_ready(&self, pid: u32, port: u16, timeout_ms: u64) -> Result<String, String> {
        self.wait_ready_cancellable(pid, port, timeout_ms, &AtomicBool::new(false))
    }

    /// 可取消的就绪等待:取消标志置位立即返回 Err("cancelled")。
    pub fn wait_ready_cancellable(
        &self,
        pid: u32,
        port: u16,
        timeout_ms: u64,
        cancel: &AtomicBool,
    ) -> Result<String, String> {
        let start = std::time::Instant::now();
        let deadline = start + Duration::from_millis(timeout_ms);
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            // 早退检测
            let (exited, ready) = {
                let web = self.web.lock().unwrap();
                match web.as_ref().filter(|m| m.pid == pid) {
                    Some(m) => (
                        m.exited.load(Ordering::SeqCst),
                        m.ready.lock().unwrap().clone(),
                    ),
                    None => (false, None),
                }
            };
            if exited {
                return Err(format!(
                    "dsh web 进程提前退出({}s 内未见就绪行);若前端 dist 未构建,请先「更新并构建」",
                    timeout_ms / 1000
                ));
            }
            if let Some(url) = ready {
                // 端口确认:URL 端口可连接才算就绪
                let port_check = url.parse::<std::net::SocketAddr>().ok().or_else(|| {
                    let host_part = url
                        .trim_start_matches("http://")
                        .split(':')
                        .next()
                        .unwrap_or("127.0.0.1");
                    format!("{host_part}:{port}").parse().ok()
                });
                let ok = match port_check {
                    Some(addr) => {
                        std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(800))
                            .is_ok()
                    }
                    None => false,
                };
                if ok {
                    return Ok(url);
                }
            }
            if std::time::Instant::now() > deadline {
                return Err(format!(
                    "未在 {}s 内出现就绪行(dsh web: http://…);查看日志尾部,必要时先「更新并构建」",
                    timeout_ms / 1000
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// 停止单个托管进程:优雅 → 5s → 强杀。
    pub fn stop(&self, label: &'static str) -> StopOutcome {
        let Some(mut m) = self.take(label) else {
            return StopOutcome::Missing;
        };
        if !Self::is_alive(m.pid) {
            return StopOutcome::Missing;
        }
        self.log.append(
            "launcher",
            LogLevel::Info,
            &format!("{}(PID {}) 收到停止信号", label, m.pid),
        );
        let outcome = Self::kill(&mut m);
        self.after_stop(&mut m);
        outcome
    }

    fn after_stop(&self, m: &mut Managed) {
        if m.label == "dsh web" {
            let _ = std::fs::remove_file(state_dir().join("dshweb.pid"));
        } else {
            let _ = std::fs::remove_file(state_dir().join("devweb.pid"));
        }
        let _ = m;
    }

    /// 进程组 SIGTERM → 5s → SIGKILL。
    #[cfg(unix)]
    fn kill(_m: &mut Managed) -> StopOutcome {
        let pid = _m.pid;
        let sigterm = unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
        if sigterm != 0 {
            // 进程组不存在(pid 已被回收):按单进程再试
            let single = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if single != 0 {
                return StopOutcome::Missing;
            }
        }
        if wait_group_dead(pid, 5_000) {
            return StopOutcome::Exited;
        }
        unsafe {
            if libc::kill(-(pid as i32), libc::SIGKILL) != 0 {
                let _ = libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        StopOutcome::Killed
    }

    /// Windows:优雅信号优先,最终 TerminateJobObject(并释放 job 句柄)。
    #[cfg(windows)]
    fn kill(m: &mut Managed) -> StopOutcome {
        let job = std::mem::take(&mut m.job);
        if job.is_null() {
            return StopOutcome::Missing;
        }
        // SAFETY: job 句柄由 create_job 创建且未被提前释放
        unsafe { crate::services::supervisor::win::stop_job(m.pid, job) }
    }

    /// 停止全部托管进程。
    pub fn stop_all(&self) {
        for label in ["dsh web", "dev:web"] {
            self.stop(label);
        }
    }

    /// 当前托管 PID(web/dev)。
    pub fn web_pid(&self) -> Option<u32> {
        self.web.lock().unwrap().as_ref().map(|m| m.pid)
    }

    /// detach:清空注册表但保留子进程(进程组已脱离,无需操作)。
    pub fn detach(&self) {
        self.web.lock().unwrap().take();
        self.dev.lock().unwrap().take();
    }

    /// 持久化运行信息(recall 用)。
    pub fn persist_running(&self) {
        let web = self.web.lock().unwrap().clone();
        let dev = self.dev.lock().unwrap().clone();
        let mut payload = serde_json::Map::new();
        if let Some(w) = web.as_ref() {
            payload.insert("webPid".into(), serde_json::json!(w.pid));
            payload.insert("webStartedAt".into(), serde_json::json!(w.started_at));
            payload.insert("webReadyAt".into(), serde_json::json!(w.ready_at));
            payload.insert("webUrl".into(), serde_json::json!(w.url));
        }
        if let Some(d) = dev.as_ref() {
            payload.insert("devPid".into(), serde_json::json!(d.pid));
        }
        let path = state_dir().join("runtime.json");
        let _ = std::fs::create_dir_all(state_dir());
        if let Ok(json) = serde_json::to_string_pretty(&payload) {
            let _ = std::fs::write(path, json);
        }
    }

    /// 召回:根据 runtime.json 的 PID 检查存活并重建托管记录(仅记录,不重连管道)。
    /// 三重校验由调用方(migration/state)完成:pid 存活 + 命令行匹配 + 端口占用。
    pub fn recall(&self) -> Option<Managed> {
        let path = state_dir().join("runtime.json");
        let raw = std::fs::read_to_string(&path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let pid = v.get("webPid")?.as_u64()? as u32;
        if !Self::is_alive(pid) {
            return None;
        }
        let mut m = Managed::new(
            pid,
            "dsh web",
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(
                v.get("webUrl").and_then(|u| u.as_str()).map(String::from),
            )),
            #[cfg(windows)]
            windows_sys::Win32::Foundation::HANDLE::default(),
        );
        m.started_at = v.get("webStartedAt").and_then(|x| x.as_i64());
        m.ready_at = v.get("webReadyAt").and_then(|x| x.as_i64());
        m.url = v.get("webUrl").and_then(|x| x.as_str()).map(String::from);
        self.web.lock().unwrap().replace(m);
        self.web.lock().unwrap().clone()
    }
}

#[cfg(unix)]
fn wait_group_dead(pid: u32, timeout_ms: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(timeout_ms) {
        let r = unsafe { libc::kill(-(pid as i32), 0) };
        if r != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// 端口占用者 pid(诊断;Unix lsof / Windows netstat)。
pub fn port_holder_pid(port: u16) -> Option<u32> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().nth(1)?;
        line.split_whitespace().nth(1)?.parse().ok()
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("netstat")
            .args(["-ano"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let needle = format!(":{port}");
        for line in text.lines() {
            if !line.contains("LISTENING") {
                continue;
            }
            // 行格式: proto local foreign state pid
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 5 {
                continue;
            }
            if fields[1].ends_with(&needle) {
                return fields[4].parse().ok();
            }
        }
        None
    }
}

/// 进程命令行(诊断 / 三重校验)。
pub fn process_cmdline(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
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
    #[cfg(windows)]
    {
        crate::services::supervisor::win::process_cmdline(pid)
    }
}

/// 日志文件路径(按日期?统一 runtime.log;M4 保留单文件方案)。
pub fn log_file() -> PathBuf {
    logs_dir().join("launcher.log")
}

#[cfg(windows)]
pub mod win {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, PROCESS_QUERY_INFORMATION,
        PROCESS_TERMINATE,
    };

    /// 创建进程:CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW(GUI 无控制台闪窗)。
    pub fn configure_spawn(cmd: &mut std::process::Command) {
        let flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        cmd.creation_flags(flags);
    }

    /// 创建 Job Object 并把子进程加入。不设置 KILL_ON_JOB_CLOSE:
    /// launcher 退出后 dsh 进程树继续运行(close job handle 不自动杀)。
    pub fn create_job(child: &std::process::Child) -> HANDLE {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return job;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        // 显式不设置 JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        info.BasicLimitInformation.LimitFlags = 0;
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            unsafe { CloseHandle(job) };
            return HANDLE::default();
        }
        let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) };
        if assigned == 0 {
            unsafe { CloseHandle(job) };
            return HANDLE::default();
        }
        job
    }

    /// 停止:先优雅(GenerateConsoleCtrlEvent 到进程组),5s 后 TerminateJobObject。
    ///
    /// # Safety
    /// 调用方必须保证 `job` 是有效的 Job Object HANDLE,且未被并发释放。
    pub unsafe fn stop_job(pid: u32, job: HANDLE) -> StopOutcome {
        // 优雅信号:CTRL_BREAK 到该进程组
        let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_TERMINATE, 0, pid) };
        if !process.is_null() {
            unsafe {
                use windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent;
                let _ = GenerateConsoleCtrlEvent(1, pid); // CTRL_BREAK_EVENT
                CloseHandle(process);
            }
        }
        if wait_process_dead(pid, 5_000) {
            return StopOutcome::Exited;
        }
        // 强杀整个 Job 进程树
        unsafe { TerminateJobObject(job, 0x1) };
        // 关句柄(不杀进程,仅释放资源)
        unsafe { CloseHandle(job) };
        StopOutcome::Killed
    }

    pub fn is_alive(pid: u32) -> bool {
        let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(process, &mut code)
        };
        unsafe { CloseHandle(process) };
        ok != 0 && code == 259 // STILL_ACTIVE
    }

    fn wait_process_dead(pid: u32, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(timeout_ms) {
            if !is_alive(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }

    pub fn process_cmdline(pid: u32) -> Option<String> {
        // 只读诊断(recall 三重校验):PowerShell Get-CimInstance,不提权、不加载模块。
        // Win10/11 自带 PowerShell;失败返回 None 只影响诊断精度,不影响主流程。
        let script = format!(
            "try {{ (Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine }} catch {{ '' }}"
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}
