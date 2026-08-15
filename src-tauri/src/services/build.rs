// dsh-launcher · BuildService:lockfile 变化检测 → pnpm install → 分阶段 pnpm run build
use crate::log_hub::LogHub;
use crate::services::runtime::Tools;
use std::sync::Arc;

/// 构建失败诊断阶段。
const STAGE_RE: &str = r"build:lib:host|build:lib:client|build:web";

/// 运行 pnpm 命令,流式输出;返回 { code, tail }。
pub fn run_pnpm(
    log: &Arc<LogHub>,
    tools: &Tools,
    cwd: &str,
    args: &[&str],
    label: &str,
    on_line: Option<&dyn Fn(&str)>,
) -> Result<(i32, Vec<String>), String> {
    let pnpm = tools
        .pnpm
        .as_ref()
        .ok_or_else(|| "未找到 pnpm".to_string())?
        .clone();
    let mut cmd = std::process::Command::new(&pnpm);
    cmd.args(args);
    cmd.current_dir(cwd);
    cmd.envs(tools.env());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("无法执行 pnpm:{e}"))?;
    let mut tail: Vec<String> = Vec::new();
    {
        use std::io::{BufRead, BufReader};
        let so = child.stdout.take();
        let se = child.stderr.take();
        let pump = |reader: Option<Box<dyn std::io::Read + Send>>,
                    warn: bool,
                    log: &Arc<LogHub>,
                    tail: &mut Vec<String>,
                    on_line: Option<&dyn Fn(&str)>| {
            let Some(mut reader) = reader else {
                return;
            };
            let mut reader = BufReader::new(&mut reader);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                let l = line.trim_end().to_string();
                if !l.is_empty() {
                    tail.push(l.clone());
                    if tail.len() > 40 {
                        tail.remove(0);
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
                line.clear();
            }
        };
        pump(
            so.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
            false,
            log,
            &mut tail,
            on_line,
        );
        pump(
            se.map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
            true,
            log,
            &mut tail,
            on_line,
        );
    }
    let status = child.wait().map_err(|e| format!("pnpm wait 失败:{e}"))?;
    let code = status.code().unwrap_or(-1);
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
    Ok((code, tail))
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
) -> Result<(bool, bool), String> {
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
    let (code, _tail) = run_pnpm(log, tools, cwd, &["install"], "pnpm install", None)?;
    Ok((true, code == 0))
}

/// 构建:pnpm run build [args],按阶段上报。
pub fn run_build(
    log: &Arc<LogHub>,
    tools: &Tools,
    cwd: &str,
    build_args: &str,
    on_stage: &dyn Fn(&str),
) -> Result<bool, String> {
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
    )?;
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
