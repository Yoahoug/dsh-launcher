// dsh-launcher · Clone 与事务性安装(M1/M2)
//
// - Clone 弹窗请求由 UI 提交(pending_clone),流程层按 operationId 执行;
// - 默认填「上一次远端验证通过或 clone 成功」的地址(last-good URL 持久化);
// - 只允许 HTTPS 与受控 SSH(禁任意 remote helper、shell 拼接、凭证落盘;日志脱敏);
// - 先禁交互、带超时的只读远端检查,再 clone 到最终目录同卷的 runId staging;
// - 一键全套流程在 staging 中完成 clone → install → build → post-check,最后原子提交;
//   目标非空绝不覆盖;失败/取消只清理本次 operation 创建的 staging,绝不删除用户目录;
// - 默认不 shallow clone;默认分支从远端 HEAD 动态发现,不硬编码 main。
use crate::config::state_dir;
use crate::log_hub::LogHub;
use crate::ops::{CancellationToken, OperationError};
use crate::services::runtime::Tools;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 受控 SSH 环境(禁交互、接受新 host key、连接超时)。
const GIT_SSH_COMMAND: &str =
    "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15";

/// 用户提交的克隆请求(UI → 后端)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneRequest {
    pub url: String,
    pub target_dir: String,
    /// 网络源显示:mirror | official | custom
    pub source: String,
    /// 高级选项:分支(留空 = 远端 HEAD 动态发现)
    pub branch: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneOutcome {
    pub final_dir: String,
    pub branch: String,
    pub head: String,
}

/// Clone 弹窗初始数据。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneDialogData {
    pub last_good_url: Option<String>,
    pub default_target: String,
    pub official_url: String,
}

/// 默认目标目录:当前 repoPath 的父目录(或用户主目录/Desktop 兜底)。
pub fn default_target_dir(current_repo_path: &str) -> String {
    if !current_repo_path.is_empty() {
        if let Some(parent) = Path::new(current_repo_path).parent() {
            let s = parent.display().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    let home = crate::config::home_dir();
    let desktop = Path::new(&home).join("Desktop");
    if desktop.is_dir() {
        desktop.display().to_string()
    } else {
        home
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloneState {
    /// 上一次远端验证通过或 clone 成功的地址(非法/失败输入不得覆盖)。
    pub last_good_url: Option<String>,
}

fn clone_state_file() -> PathBuf {
    state_dir().join("clone-state.json")
}

pub fn load_clone_state() -> CloneState {
    std::fs::read_to_string(clone_state_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_clone_state(s: &CloneState) {
    let _ = std::fs::create_dir_all(state_dir());
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(clone_state_file(), json);
    }
}

/// 记住成功地址(只有远端验证通过或 clone 成功才调用)。
pub fn remember_good_url(url: &str) {
    let mut s = load_clone_state();
    s.last_good_url = Some(url.to_string());
    save_clone_state(&s);
}

/// 上次成功地址(UI 默认填充)。
pub fn last_good_url() -> Option<String> {
    load_clone_state().last_good_url
}

/// URL 校验:https 或受控 ssh;拒绝凭证、http 明文、shell 元字符。
pub fn validate_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("URL 不能为空".into());
    }
    if u.chars()
        .any(|c| c.is_whitespace() || "&|;`$<>".contains(c))
    {
        return Err("URL 包含非法字符(不允许 shell 元字符/空白)".into());
    }
    // scp-like SSH:git@host:path
    if u.contains('@') && u.contains(':') && !u.starts_with("ssh://") && !u.starts_with("http") {
        let (user_host, _path) = u.split_once(':').unwrap();
        let (user, host) = user_host
            .split_once('@')
            .ok_or("SSH 地址格式应为 user@host:path")?;
        if user.is_empty() || host.is_empty() {
            return Err("SSH 地址格式非法".into());
        }
        return Ok(());
    }
    let parsed = url::Url::parse(u).map_err(|e| format!("URL 格式非法:{e}"))?;
    match parsed.scheme() {
        "https" => {
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(
                    "URL 不能包含用户名/密码凭证;请使用 Git Credential Manager 或 SSH 密钥".into(),
                );
            }
        }
        "ssh" => {
            // ssh:// 的 user 是 SSH 登录身份(允许);密码不是 URL 凭证,拒绝
            if parsed.password().is_some() {
                return Err("ssh URL 不允许携带密码".into());
            }
        }
        "http" => return Err("不允许 http 明文克隆,请使用 https 或受控 ssh".into()),
        other => return Err(format!("不允许的协议 {other};只允许 https 与受控 ssh")),
    }
    if parsed.host_str().is_none() {
        return Err("URL 缺少主机名".into());
    }
    Ok(())
}

/// 脱敏后的显示 URL(凭证已拒绝,这里做最后防线)。
pub fn redact_url(url: &str) -> String {
    let mut out = url.to_string();
    if let Some(at) = out.find('@') {
        if out.contains("://") && at > out.find("://").unwrap() + 3 {
            // userinfo@ 段脱敏
            let scheme_end = out.find("://").unwrap() + 3;
            if at > scheme_end {
                out.replace_range(scheme_end..at + 1, "[redacted]@");
            }
        }
    }
    out
}

/// 子进程环境:git/pnpm 注入国内 registry 与受控行为(仅当前子进程,不改全局配置)。
fn git_env(tools: &Tools) -> HashMap<String, String> {
    let mut env = tools.env();
    env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    env.insert("GIT_SSH_COMMAND".into(), GIT_SSH_COMMAND.into());
    env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
    env
}

fn registry_env(tools: &Tools) -> HashMap<String, String> {
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

/// 取消感知的子进程执行:参数数组 + 流式日志 + 取消即杀进程树。
#[allow(clippy::too_many_arguments)]
pub fn run_cancellable(
    log: &Arc<LogHub>,
    label: &str,
    bin: &Path,
    args: &[&str],
    cwd: &Path,
    env: &HashMap<String, String>,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<(i32, Vec<String>), OperationError> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    cmd.current_dir(cwd);
    cmd.envs(env.clone());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000008 | 0x08000000); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| OperationError::Failed(format!("无法执行 {label}:{e}")))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let tail: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log2 = log.clone();
    let tail2 = tail.clone();
    let label2 = label.to_string();
    // stdout 读取线程
    let mut threads = Vec::new();
    let t1 = std::thread::spawn(move || {
        let push = |l: String| {
            let mut t = tail2.lock().unwrap();
            t.push(l.clone());
            if t.len() > 60 {
                t.remove(0);
            }
            log2.append(&label2, crate::contract::LogLevel::Info, &l);
        };
        if let Some(mut out) = stdout {
            let reader = BufReader::new(&mut out);
            for line in reader.lines().map_while(Result::ok) {
                push(line);
            }
        }
    });
    threads.push(t1);
    let log3 = log.clone();
    let tail3 = tail.clone();
    let label3 = label.to_string();
    let t2 = std::thread::spawn(move || {
        let push = |l: String| {
            let mut t = tail3.lock().unwrap();
            t.push(l.clone());
            if t.len() > 60 {
                t.remove(0);
            }
            log3.append(&label3, crate::contract::LogLevel::Warn, &l);
        };
        if let Some(mut err) = stderr {
            let reader = BufReader::new(&mut err);
            for line in reader.lines().map_while(Result::ok) {
                push(line);
            }
        }
    });
    threads.push(t2);

    // 主线程:等待退出 / 超时 / 取消
    let started = Instant::now();
    let code = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            for t in threads {
                let _ = t.join();
            }
            return Err(OperationError::Cancelled);
        }
        if let Some(t) = timeout {
            if started.elapsed() > t {
                let _ = child.kill();
                let _ = child.wait();
                for t in threads {
                    let _ = t.join();
                }
                return Err(OperationError::Failed(format!(
                    "{label} 超时(>{}s)",
                    t.as_secs()
                )));
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                for t in threads {
                    let _ = t.join();
                }
                return Err(OperationError::Failed(format!("{label} wait 失败:{e}")));
            }
        }
    };
    for t in threads {
        let _ = t.join();
    }
    let result = (code, tail.lock().unwrap().clone());
    Ok(result)
}

/// 只读远端检查:解析默认分支(禁交互、带超时)。
pub fn remote_default_branch(
    log: &Arc<LogHub>,
    git: &Path,
    url: &str,
    tools: &Tools,
    cancel: &AtomicBool,
) -> Result<String, OperationError> {
    validate_url(url).map_err(OperationError::Failed)?;
    log.append(
        "git",
        crate::contract::LogLevel::Info,
        &format!("远端检查(只读) → {}", redact_url(url)),
    );
    let (code, lines) = run_cancellable(
        log,
        "git ls-remote",
        git,
        &["ls-remote", "--symref", url, "HEAD"],
        &state_dir(),
        &git_env(tools),
        cancel,
        Some(Duration::from_secs(30)),
    )?;
    if code != 0 {
        return Err(OperationError::Failed(format!(
            "远端验证失败(退出码 {code});请检查地址与网络:{}",
            lines.last().unwrap_or(&"无输出".to_string())
        )));
    }
    // 解析 `ref: refs/heads/master\tHEAD`
    let head_line = lines
        .iter()
        .find(|l| l.contains("HEAD"))
        .cloned()
        .unwrap_or_default();
    let branch = head_line
        .split_whitespace()
        .nth(1)
        .and_then(|r| r.strip_prefix("refs/heads/"))
        .map(|b| b.to_string())
        .ok_or_else(|| OperationError::Failed("远端未返回 HEAD 引用(可能不是 git 仓库)".into()))?;
    log.append(
        "git",
        crate::contract::LogLevel::Ok,
        &format!("远端可用,默认分支: {branch}"),
    );
    Ok(branch)
}

/// 目标目录是否可写入(不存在或空目录)。
fn target_usable(target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    if !target.is_dir() {
        return Err("目标路径存在但不是目录".into());
    }
    let mut entries = std::fs::read_dir(target).map_err(|e| format!("读取目标目录失败:{e}"))?;
    if entries.next().is_some() {
        return Err("目标目录非空,绝不覆盖;请选择空目录或不存在的路径".into());
    }
    Ok(())
}

/// 一键全套流程(clone 或 clone+install+build+post-check)。
pub fn run_clone_full(
    log: &Arc<LogHub>,
    request: &CloneRequest,
    full: bool,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str, Option<u8>),
    tools: &Tools,
) -> Result<CloneOutcome, OperationError> {
    token.check()?;
    let cancel = Arc::new(AtomicBool::new(false));
    // 取消桥:令牌置位 → 子进程取消标志(用 Arc 副本,避免引用逃逸线程)
    let token_flag = token.arc_flag();
    let cancel2 = cancel.clone();
    let watcher = std::thread::spawn(move || loop {
        if token_flag.load(Ordering::SeqCst) {
            cancel2.store(true, Ordering::SeqCst);
            break;
        }
        if cancel2.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    });

    let result = run_clone_full_inner(log, request, full, token, on_stage, tools, &cancel);
    cancel.store(true, Ordering::SeqCst); // 通知 watcher 结束
    let _ = watcher.join();
    token.check()?;
    result
}

fn run_clone_full_inner(
    log: &Arc<LogHub>,
    request: &CloneRequest,
    full: bool,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str, Option<u8>),
    tools: &Tools,
    cancel: &Arc<AtomicBool>,
) -> Result<CloneOutcome, OperationError> {
    token.check()?;
    validate_url(&request.url).map_err(OperationError::Failed)?;
    let git = tools
        .git
        .clone()
        .ok_or_else(|| OperationError::Failed("未找到 git,请先安装托管 Git 或系统 git".into()))?;

    // 1. 目标可用性(不存在或空)
    let target = PathBuf::from(&request.target_dir);
    target_usable(&target).map_err(OperationError::Failed)?;
    let parent = target
        .parent()
        .ok_or_else(|| OperationError::Failed("目标目录缺少父目录".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| OperationError::Failed(format!("创建父目录失败:{e}")))?;

    // 2. 只读远端检查 + 默认分支动态发现
    on_stage("验证远端…", None);
    let remote_branch = remote_default_branch(log, &git, &request.url, tools, cancel)?;
    let branch = request
        .branch
        .clone()
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| remote_branch.clone());

    // 3. 同卷 runId staging
    let run_id = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        std::process::id()
    );
    let staging = parent.join(format!(".dsh-launcher-staging-{run_id}"));
    let _ = std::fs::remove_dir_all(&staging);
    log.append(
        "git",
        crate::contract::LogLevel::Info,
        &format!("克隆到 staging(同卷): {}", staging.display()),
    );

    // 4. clone(不 shallow;分支已动态发现)
    on_stage(&format!("克隆 {branch} 分支…"), None);
    let mut clone_args = vec!["clone".to_string(), request.url.clone()];
    if !branch.is_empty() {
        clone_args.push("--branch".to_string());
        clone_args.push(branch.clone());
    }
    clone_args.push(staging.display().to_string());
    let arg_refs: Vec<&str> = clone_args.iter().map(String::as_str).collect();
    let (code, lines) = run_cancellable(
        log,
        "git clone",
        &git,
        &arg_refs,
        parent,
        &git_env(tools),
        cancel,
        None,
    )?;
    if code != 0 {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(OperationError::Failed(format!(
            "git clone 失败(退出码 {code}):{}",
            lines.last().unwrap_or(&"无输出".to_string())
        )));
    }
    token.check()?;

    // 5. 仓库特征验证
    if !staging.join(".git").exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(OperationError::Failed(
            "clone 产物缺少 .git,仓库身份异常".into(),
        ));
    }
    if !staging.join("package.json").exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(OperationError::Failed(
            "仓库缺少 package.json,不是预期项目".into(),
        ));
    }

    // 6. 一键全套:pnpm(精确版本)→ install → build → post-check
    if full {
        token.check()?;
        on_stage("解析 pnpm 版本(packageManager)…", None);
        let pnpm_ver =
            crate::toolchain::pnpm_version_from_package_json(&staging.display().to_string())?;
        crate::toolchain::ensure_pnpm_version(log, &pnpm_ver, token, &|s| on_stage(s, None))?;
        let pnpm = crate::toolchain::resolve_pnpm(tools)
            .ok_or_else(|| OperationError::Failed("托管 pnpm 未就绪".into()))?;

        token.check()?;
        on_stage("安装依赖(pnpm install)…", None);
        let (code, _) = run_cancellable(
            log,
            "pnpm install",
            &pnpm,
            &["install"],
            &staging,
            &registry_env(tools),
            cancel,
            None,
        )?;
        if code != 0 {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(OperationError::Failed(format!(
                "pnpm install 失败(退出码 {code});国内 registry 已注入当前子进程"
            )));
        }
        token.check()?;

        on_stage("构建(pnpm run build)…", None);
        let (code, _) = run_cancellable(
            log,
            "pnpm build",
            &pnpm,
            &["run", "build"],
            &staging,
            &registry_env(tools),
            cancel,
            None,
        )?;
        if code != 0 {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(OperationError::Failed(format!(
                "构建失败(退出码 {code});查看日志尾部定位阶段"
            )));
        }
        token.check()?;

        // post-check:前端 dist 存在
        on_stage("post-check(产物校验)…", None);
        let dist = staging.join("apps/web/dist/index.html");
        if !dist.is_file() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(OperationError::Failed(format!(
                "post-check 失败:缺少前端产物 {};构建不完整",
                dist.display()
            )));
        }
        log.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            "post-check 通过:前端产物存在,仓库可启动",
        );
    }

    // 7. 原子提交(短临界区,不可取消):rename staging → final
    if target.exists() {
        // 提交前再次确认(与 1 之间的竞态)
        if std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some()) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(OperationError::Failed(
                "提交瞬间目标目录非空,已中止且未覆盖".into(),
            ));
        }
        let _ = std::fs::remove_dir_all(&target);
    }
    std::fs::rename(&staging, &target).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staging);
        OperationError::Failed(format!("原子提交失败(目标未被覆盖):{e}"))
    })?;
    on_stage("提交完成 ✓", Some(100));

    // 8. 结果(head/分支)
    let head = clone_head(&git, &target, tools, cancel).unwrap_or_default();
    // 只有提交成功才记 last-good
    remember_good_url(&request.url);

    Ok(CloneOutcome {
        final_dir: target.display().to_string(),
        branch,
        head,
    })
}

fn clone_head(
    git: &Path,
    repo: &Path,
    tools: &Tools,
    cancel: &AtomicBool,
) -> Result<String, OperationError> {
    let log: Arc<LogHub> = Arc::new(LogHub::new(
        state_dir().join("logs/clone.log"),
        Arc::new(|_| {}),
        true,
    ));
    let (code, lines) = run_cancellable(
        &log,
        "git rev-parse",
        git,
        &["rev-parse", "--short", "HEAD"],
        repo,
        &git_env(tools),
        cancel,
        Some(Duration::from_secs(10)),
    )?;
    if code == 0 {
        Ok(lines.first().cloned().unwrap_or_default())
    } else {
        Ok(String::new())
    }
}

/// 是否使用托管 git(设置页展示来源)。
pub fn git_source_label(tools: &Tools) -> String {
    let resolved = crate::toolchain::resolve_git(tools);
    match resolved {
        Some(p) if p.to_string_lossy().contains("toolchains") => "托管 MinGit".into(),
        Some(_) => "系统安装".into(),
        None => "未找到".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation_https_ok() {
        assert!(validate_url("https://github.com/deepseek-ai/deepseek-harness.git").is_ok());
        assert!(validate_url("https://gitee.com/foo/bar.git").is_ok());
    }

    #[test]
    fn url_validation_rejects_credentials() {
        let err = validate_url("https://user:pass@github.com/foo/bar.git").unwrap_err();
        assert!(err.contains("凭证"), "{err}");
        let err2 = validate_url("https://user@github.com/foo/bar.git").unwrap_err();
        assert!(err2.contains("凭证"), "{err2}");
    }

    #[test]
    fn url_validation_rejects_http_and_protocols() {
        let err = validate_url("http://github.com/foo/bar.git").unwrap_err();
        assert!(err.contains("明文"), "{err}");
        let err2 = validate_url("ftp://x/y").unwrap_err();
        assert!(err2.contains("不允许的协议"), "{err2}");
        let err3 = validate_url("javascript:alert(1)").unwrap_err();
        assert!(err3.contains("不允许的协议"), "{err3}");
    }

    #[test]
    fn url_validation_rejects_shell_metachars() {
        assert!(validate_url("https://x/y.git;rm -rf /").is_err());
        assert!(validate_url("https://x/y.git --upload-pack=sh").is_err());
    }

    #[test]
    fn url_validation_ssh_forms_ok() {
        assert!(validate_url("ssh://git@github.com/deepseek-ai/deepseek-harness.git").is_ok());
        assert!(validate_url("git@github.com:deepseek-ai/deepseek-harness.git").is_ok());
        assert!(validate_url("git@github.com:foo/bar.git").is_ok());
    }

    #[test]
    fn url_redaction() {
        let r = redact_url("https://user:secret@host/x.git");
        assert!(!r.contains("secret"), "{r}");
        assert!(r.contains("[redacted]"));
        let plain = redact_url("https://host/x.git");
        assert_eq!(plain, "https://host/x.git");
    }

    #[test]
    fn target_usable_checks() {
        let base = std::env::temp_dir().join(format!("dsh-target-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("empty")).unwrap();
        assert!(target_usable(&base.join("empty")).is_ok());
        assert!(target_usable(&base.join("missing")).is_ok());
        std::fs::write(base.join("empty/keep.txt"), "x").unwrap();
        let err = target_usable(&base.join("empty")).unwrap_err();
        assert!(err.contains("非空"), "{err}");
        // 文件不是目录
        assert!(target_usable(&base.join("empty/keep.txt")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn clone_state_persists_good_url() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-cs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));
        assert!(last_good_url().is_none());
        remember_good_url("https://example.com/repo.git");
        assert_eq!(
            last_good_url().as_deref(),
            Some("https://example.com/repo.git")
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
