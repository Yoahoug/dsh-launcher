// dsh-launcher · DshCtl:dsh CLI 子进程封装
// 插件 add/remove、--dump-config 等一律经本模块执行:
// - 复用 supervisor 的 node 直连解析(仓库 package.json scripts.dsh)与 runtime::Tools
//   的 PATH 注入;cwd = repo_path;DSH_HOME / DSH_NO_AUTOOPEN 注入;
// - 流式模式:stdout/stderr 逐行进 log_hub(来源 "dsh"),可取消(ops::CancellationToken);
// - 捕获模式:一次性取回 stdout(如 --dump-config),带超时。
use crate::log_hub::LogHub;
use crate::ops::CancellationToken;
use crate::services::runtime::Tools;
use crate::services::supervisor::dsh_entry_args;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// dsh 子进程日志来源(Logs 页筛选)。
pub const DSH_LOG_LABEL: &str = "dsh";

/// 捕获模式超时(dump-config 首启需 tsx 冷启动,给足时间)。
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(90);

/// 解析 node 可执行文件(与 supervisor 同规则)。
fn resolve_node(tools: &Tools) -> Result<std::path::PathBuf, String> {
    tools
        .dsh_node_dir
        .as_ref()
        .map(|d| {
            if cfg!(windows) {
                d.join("node.exe")
            } else {
                d.join("node")
            }
        })
        .filter(|p| p.is_file())
        .or_else(|| crate::services::runtime::resolve_executable("node"))
        .ok_or_else(|| "未找到 node(需要 dsh 兼容 Node)".to_string())
}

/// 构建 dsh CLI 命令(node 直连优先,仓库未声明 scripts.dsh 时回退 pnpm)。
/// 返回 (启动描述, Command);启动描述用于报错与日志。
pub fn build_dsh_cmd(
    tools: &Tools,
    repo_path: &str,
    dsh_home: &str,
    args: &[String],
) -> Result<(String, Command), String> {
    let mut cmd;
    let label_desc;
    if let Some(entry_args) = dsh_entry_args(repo_path) {
        let node = resolve_node(tools)?;
        let mut c = Command::new(&node);
        c.args(&entry_args);
        c.args(args);
        label_desc = format!("dsh {}", args.join(" "));
        cmd = c;
    } else {
        let pnpm = tools
            .pnpm
            .as_ref()
            .ok_or_else(|| "未找到 pnpm".to_string())?
            .clone();
        let mut c = Command::new(&pnpm);
        c.arg("dsh");
        c.args(args);
        label_desc = format!("pnpm dsh {}", args.join(" "));
        cmd = c;
    }
    cmd.current_dir(repo_path);
    cmd.envs(tools.env());
    if !dsh_home.is_empty() {
        cmd.env("DSH_HOME", dsh_home);
    }
    cmd.env("DSH_NO_AUTOOPEN", "1");
    Ok((label_desc, cmd))
}

#[cfg(windows)]
fn configure_spawn(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW(GUI 无控制台闪窗)
    cmd.creation_flags(0x00000200 | 0x08000000);
}

/// 捕获模式:运行 dsh 子命令并取回 stdout 全文(如 --dump-config)。
/// 退出码非 0 或超时返回 Err(含 stderr 尾段诊断)。阻塞调用,勿在事件循环热路径使用。
pub fn run_capture(
    tools: &Tools,
    repo_path: &str,
    dsh_home: &str,
    args: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let (label, mut cmd) = build_dsh_cmd(tools, repo_path, dsh_home, args)?;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    configure_spawn(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("无法执行 {label}:{e}"))?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{label} 超时(>{:?}),已终止", timeout));
            }
            Err(e) => return Err(format!("{label} wait 失败:{e}")),
        }
    };
    let mut out = String::new();
    let mut err = String::new();
    if let Some(mut so) = child.stdout.take() {
        use std::io::Read;
        let _ = so.read_to_string(&mut out);
    }
    if let Some(mut se) = child.stderr.take() {
        use std::io::Read;
        let _ = se.read_to_string(&mut err);
    }
    if !status.success() {
        let tail: String = err
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "{label} 退出码 {}{}",
            status.code().unwrap_or(-1),
            if tail.is_empty() {
                String::new()
            } else {
                format!("\n{tail}")
            }
        ));
    }
    Ok(out)
}

/// 流式模式:stdout/stderr 并发逐行进 log_hub,可取消;退出码非 0 返回 Err。
pub fn run_stream(
    log: &Arc<LogHub>,
    tools: &Tools,
    repo_path: &str,
    dsh_home: &str,
    args: &[String],
    token: &CancellationToken,
) -> Result<(), String> {
    let (label, mut cmd) = build_dsh_cmd(tools, repo_path, dsh_home, args)?;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    configure_spawn(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("无法执行 {label}:{e}"))?;

    log.append(
        DSH_LOG_LABEL,
        crate::contract::LogLevel::Info,
        &format!("$ {label}"),
    );
    let cancel = token.arc_flag();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let pump = |reader: Option<Box<dyn std::io::Read + Send>>,
                warn: bool,
                log: Arc<LogHub>,
                cancel: &std::sync::atomic::AtomicBool| {
        let Some(mut reader) = reader else { return };
        let mut reader = BufReader::new(&mut reader);
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let l = line.trim_end().to_string();
                    if !l.is_empty() {
                        log.append(
                            DSH_LOG_LABEL,
                            if warn {
                                crate::contract::LogLevel::Warn
                            } else {
                                crate::contract::LogLevel::Info
                            },
                            &l,
                        );
                    }
                    line.clear();
                }
            }
        }
    };

    let cancel1 = cancel.clone();
    let cancel2 = cancel.clone();
    let mut threads = Vec::new();
    threads.push(std::thread::spawn({
        let log = log.clone();
        move || {
            pump(
                stdout.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
                false,
                log,
                cancel1.as_ref(),
            )
        }
    }));
    threads.push(std::thread::spawn({
        let log = log.clone();
        move || {
            pump(
                stderr.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
                true,
                log,
                cancel2.as_ref(),
            )
        }
    }));

    let code: i32 = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            for t in threads {
                let _ = t.join();
            }
            return Err("cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                for t in threads {
                    let _ = t.join();
                }
                return Err(format!("{label} wait 失败:{e}"));
            }
        }
    };
    for t in threads {
        let _ = t.join();
    }
    if code != 0 {
        return Err(format!("{label} 退出码 {code}"));
    }
    log.append(
        DSH_LOG_LABEL,
        crate::contract::LogLevel::Ok,
        &format!("{label} → 完成 ✓"),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_cmd_prefers_node_direct_when_script_present() {
        let base = std::env::temp_dir().join(format!("dsh-dshctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("package.json"),
            r#"{"scripts":{"dsh":"node --import tsx/esm apps/cli/src/bin.ts"}}"#,
        )
        .unwrap();
        let tools = Tools {
            pnpm: None,
            git: None,
            dsh_node_dir: std::env::var("HOME").ok().map(std::path::PathBuf::from),
        };
        let args = vec![
            "--profile".to_string(),
            "web".to_string(),
            "--dump-config".to_string(),
        ];
        let (label, _cmd) = build_dsh_cmd(&tools, &base.display().to_string(), "", &args).unwrap();
        assert!(label.contains("dsh --profile web --dump-config"), "{label}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn capture_timeout_is_finite() {
        // 仅验证常量非零;超时行为依赖真实 CLI,不做子进程集成测试
        assert!(CAPTURE_TIMEOUT > Duration::from_secs(0));
    }
}
