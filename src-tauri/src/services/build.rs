// dsh-launcher · BuildService:lockfile 变化检测 → pnpm install → 分阶段 pnpm run build
// stdout/stderr 并发消费;取消标志置位即终止进程树;npm/pnpm registry 仅通过当前
// 子进程环境注入国内镜像(npmmirror),绝不修改全局配置。
use crate::log_hub::LogHub;
use crate::ops::CancellationToken;
use crate::services::runtime::Tools;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 构建失败诊断阶段。
const STAGE_RE: &str = r"build:lib:host|build:lib:client|build:web";

/// 国内 registry(仅当前子进程环境注入)。
pub fn registry_env(tools: &Tools) -> HashMap<String, String> {
    let mut env = tools.env();
    env.insert(
        "npm_config_registry".into(),
        "https://registry.npmmirror.com/".into(),
    );
    env.insert(
        "NPM_CONFIG_REGISTRY".into(),
        "https://registry.npmmirror.com/".into(),
    );
    env
}

/// 运行 pnpm 命令,stdout/stderr 并发流式输出;可取消。
/// 返回 { code, tail };取消时返回 Err("cancelled")。
#[allow(clippy::too_many_arguments)]
pub fn run_pnpm(
    log: &Arc<LogHub>,
    tools: &Tools,
    cwd: &str,
    args: &[&str],
    label: &str,
    on_line: Option<&(dyn Fn(&str) + Send + Sync)>,
    extra_env: &HashMap<String, String>,
    token: &CancellationToken,
) -> Result<(i32, Vec<String>), String> {
    let pnpm = tools
        .pnpm
        .as_ref()
        .ok_or_else(|| "未找到 pnpm".to_string())?
        .clone();
    let mut cmd = std::process::Command::new(&pnpm);
    cmd.args(args);
    cmd.current_dir(cwd);
    cmd.envs(extra_env.clone());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP(0x200)| CREATE_NO_WINDOW(0x08000000):
        // 0x8 是 DETACHED_PROCESS,会让 cmd.exe 执行 .cmd(pnpm.cmd)时子进程输出丢失。
        cmd.creation_flags(0x00000200 | 0x08000000); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    }
    let mut child = cmd.spawn().map_err(|e| format!("无法执行 pnpm:{e}"))?;

    let cancel = token.arc_flag();
    let tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // stdout/stderr 并发 pump:行 → mpsc channel(主线程统一日志 + on_line + tail)
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();

    let pump = |reader: Option<Box<dyn std::io::Read + Send>>,
                warn: bool,
                tx: std::sync::mpsc::Sender<(bool, String)>,
                cancel: &AtomicBool| {
        let Some(mut reader) = reader else {
            return;
        };
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
                    if !l.is_empty() && tx.send((warn, l)).is_err() {
                        break;
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
        let tx = tx.clone();
        move || {
            pump(
                stdout.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
                false,
                tx,
                cancel1.as_ref(),
            )
        }
    }));
    threads.push(std::thread::spawn({
        let tx = tx.clone();
        move || {
            pump(
                stderr.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
                true,
                tx,
                cancel2.as_ref(),
            )
        }
    }));
    drop(tx);

    // 主线程:等待退出 / 取消;同时消费日志行(on_line 只在主线程调用,无 Send 要求)
    let code: i32 = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            for t in threads {
                let _ = t.join();
            }
            return Err("cancelled".into());
        }
        // 先消费已到达的日志行
        while let Ok((warn, l)) = rx.try_recv() {
            let mut t = tail.lock().unwrap();
            t.push(l.clone());
            if t.len() > 40 {
                t.remove(0);
            }
            log.append(
                "pnpm",
                if warn {
                    crate::contract::LogLevel::Warn
                } else {
                    crate::contract::LogLevel::Info
                },
                &l,
            );
            if let Some(f) = on_line {
                f(&l);
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                for t in threads {
                    let _ = t.join();
                }
                return Err(format!("pnpm wait 失败:{e}"));
            }
        }
    };
    // 排空剩余日志
    while let Ok((warn, l)) = rx.try_recv() {
        let mut t = tail.lock().unwrap();
        t.push(l.clone());
        if t.len() > 40 {
            t.remove(0);
        }
        log.append(
            "pnpm",
            if warn {
                crate::contract::LogLevel::Warn
            } else {
                crate::contract::LogLevel::Info
            },
            &l,
        );
        if let Some(f) = on_line {
            f(&l);
        }
    }
    for t in threads {
        let _ = t.join();
    }
    log.append(
        "pnpm",
        if code == 0 {
            crate::contract::LogLevel::Ok
        } else {
            crate::contract::LogLevel::Err
        },
        &format!(
            "{label} → 退出码 {code}{}",
            if code == 0 { " ✓" } else { " ✗" }
        ),
    );
    let result = (code, tail.lock().unwrap().clone());
    Ok(result)
}

/// 定位失败阶段(tsc / tsdown / vite)。
pub fn blame_stage(tail: &[String]) -> String {
    let joined = tail.join("\n");
    if regex::Regex::new(r"error TS\d|TS\d{4}")
        .unwrap()
        .is_match(&joined)
    {
        "tsc 类型检查错误".into()
    } else if joined.contains("tsdown") {
        "tsdown 打包错误".into()
    } else if joined.to_lowercase().contains("vite") || joined.to_lowercase().contains("rollup") {
        "vite 构建错误".into()
    } else {
        "构建错误".into()
    }
}

/// 安装依赖(仅当 lockfile 变化)。返回 (needed, ok)。
pub fn install_if_needed(
    log: &Arc<LogHub>,
    tools: &Tools,
    repo: &crate::services::repo::RepoService,
    cwd: &str,
    from: &str,
    on_stage: &dyn Fn(&str),
    token: &CancellationToken,
) -> Result<(bool, bool), crate::ops::OperationError> {
    let changed = repo.lockfile_changed(cwd, from);
    if !changed {
        log.append(
            "pnpm",
            crate::contract::LogLevel::Ok,
            "pnpm-lock.yaml 无变化,跳过 pnpm install",
        );
        return Ok((false, true));
    }
    on_stage("安装依赖(pnpm install)…");
    log.append(
        "pnpm",
        crate::contract::LogLevel::Info,
        "pnpm-lock.yaml 有变化 → pnpm install",
    );
    let env = registry_env(tools);
    let (code, _tail) = run_pnpm(
        log,
        tools,
        cwd,
        &["install"],
        "pnpm install",
        None,
        &env,
        token,
    )
    .map_err(crate::ops::OperationError::Failed)?;
    Ok((true, code == 0))
}

/// 构建:pnpm run build [args],按阶段上报。
pub fn run_build(
    log: &Arc<LogHub>,
    tools: &Tools,
    cwd: &str,
    build_args: &str,
    on_stage: &(dyn Fn(&str) + Send + Sync),
    token: &CancellationToken,
) -> Result<bool, crate::ops::OperationError> {
    on_stage("构建中…");
    log.append(
        "pnpm",
        crate::contract::LogLevel::Info,
        "pnpm run build 开始(阶段:build:lib:host → build:lib:client → build:web)",
    );
    let mut args: Vec<String> = vec!["run".into(), "build".into()];
    for a in build_args.split_whitespace() {
        args.push(a.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let stage_re = regex::Regex::new(STAGE_RE).unwrap();
    let env = registry_env(tools);
    let (code, tail) = run_pnpm(
        log,
        tools,
        cwd,
        &arg_refs,
        "pnpm run build",
        Some(&|line| {
            if let Some(m) = stage_re.find(line) {
                let label = match m.as_str() {
                    "build:lib:host" => "构建 lib(host)…",
                    "build:lib:client" => "构建 lib(client)…",
                    "build:web" => "构建 web 前端…",
                    _ => "构建中…",
                };
                on_stage(label);
            }
        }),
        &env,
        token,
    )
    .map_err(crate::ops::OperationError::Failed)?;
    if code == 0 {
        on_stage("构建完成 ✓");
        log.append("pnpm", crate::contract::LogLevel::Ok, "构建完成 ✓");
        return Ok(true);
    }
    let stage = blame_stage(&tail);
    log.append(
        "pnpm",
        crate::contract::LogLevel::Err,
        &format!("{stage} — 构建失败,退出码 {code}"),
    );
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_env_injects_domestic_only() {
        let tools = Tools {
            pnpm: None,
            git: None,
            dsh_node_dir: None,
        };
        let env = registry_env(&tools);
        assert_eq!(
            env.get("npm_config_registry").map(String::as_str),
            Some("https://registry.npmmirror.com/")
        );
        assert_eq!(
            env.get("NPM_CONFIG_REGISTRY").map(String::as_str),
            Some("https://registry.npmmirror.com/")
        );
    }

    #[test]
    fn blame_stage_detects() {
        assert!(blame_stage(&["error TS2322: x".into()]).contains("tsc"));
        assert!(blame_stage(&["tsdown failed".into()]).contains("tsdown"));
        assert!(blame_stage(&["vite build error".into()]).contains("vite"));
        assert!(blame_stage(&["???".into()]).contains("构建错误"));
    }
}
