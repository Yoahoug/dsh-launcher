// dsh-launcher · RepoService:git 状态/fetch/stash/rebase
// 铁律:冲突只报告、绝不 reset --hard;用户 stash 必须保留(自动 stash 带标记可恢复)。
use crate::log_hub::LogHub;
use crate::services::runtime::Tools;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// git 命令结果。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GitOut {
    pub code: i32,
    pub lines: Vec<String>,
    pub tail: Vec<String>,
}

/// 同步结果。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SyncOut {
    pub ok: bool,
    pub stage: String,
    pub error: Option<String>,
    pub conflicts: Vec<String>,
    pub stashed: bool,
    pub tail: Vec<String>,
}

pub struct RepoService {
    pub log: Arc<LogHub>,
    pub tools: Tools,
}

impl RepoService {
    pub fn new(log: Arc<LogHub>, tools: Tools) -> Self {
        Self { log, tools }
    }

    fn git(&self, cwd: &str, args: &[&str]) -> Result<GitOut, String> {
        let git = self
            .tools
            .git
            .as_ref()
            .ok_or_else(|| "未找到 git".to_string())?
            .clone();
        let mut cmd = std::process::Command::new(&git);
        cmd.args(args);
        cmd.current_dir(cwd);
        cmd.envs(self.tools.env());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("无法执行 git:{e}"))?;
        let mut out = String::new();
        let mut err = String::new();
        {
            use std::io::Read;
            let mut so = child.stdout.take().unwrap();
            let mut se = child.stderr.take().unwrap();
            so.read_to_string(&mut out).ok();
            se.read_to_string(&mut err).ok();
        }
        let status = child.wait().map_err(|e| format!("git wait 失败:{e}"))?;
        let code = status.code().unwrap_or(-1);
        let mut lines: Vec<String> = out.lines().chain(err.lines()).map(String::from).collect();
        if lines.is_empty() && !err.is_empty() {
            lines.push(err.trim().to_string());
        }
        let tail: Vec<String> = lines.iter().rev().take(40).cloned().collect();
        for l in lines.iter() {
            self.log.append("git", crate::contract::LogLevel::Info, l);
        }
        Ok(GitOut { code, lines, tail })
    }

    /// 静默查询(状态类,不打扰日志)。
    fn git_quiet(&self, cwd: &str, args: &[&str]) -> Result<String, String> {
        let git = self
            .tools
            .git
            .as_ref()
            .ok_or_else(|| "未找到 git".to_string())?
            .clone();
        let out = std::process::Command::new(&git)
            .args(args)
            .current_dir(cwd)
            .envs(self.tools.env())
            .output()
            .map_err(|e| format!("无法执行 git:{e}"))?;
        if !out.status.success() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn current_branch(&self, cwd: &str) -> String {
        self.git_quiet(cwd, &["branch", "--show-current"])
            .unwrap_or_default()
    }

    pub fn head_short(&self, cwd: &str) -> String {
        self.git_quiet(cwd, &["rev-parse", "--short", "HEAD"])
            .unwrap_or_default()
    }

    /// 相对 origin/<branch> 领先/落后(依赖最近 fetch)。
    pub fn ahead_behind(&self, cwd: &str, branch: &str) -> (i64, i64) {
        let r = self
            .git_quiet(
                cwd,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...refs/remotes/origin/{branch}"),
                ],
            )
            .unwrap_or_default();
        let parts: Vec<&str> = r.split_whitespace().collect();
        if parts.len() == 2 {
            let a = parts[0].parse().unwrap_or(-1);
            let b = parts[1].parse().unwrap_or(-1);
            (a, b)
        } else {
            (-1, -1)
        }
    }

    pub fn is_dirty(&self, cwd: &str) -> bool {
        self.git_quiet(cwd, &["status", "--porcelain"])
            .is_ok_and(|r| !r.is_empty())
    }

    pub fn dirty_files(&self, cwd: &str) -> u64 {
        self.git_quiet(cwd, &["status", "--porcelain"])
            .map(|r| r.lines().count() as u64)
            .unwrap_or(0)
    }

    /// 仓库状态快照。
    pub fn status(&self, cwd: &str, sync_at: Option<i64>) -> crate::contract::RepoSnapshot {
        let branch = self.current_branch(cwd);
        let head = self.head_short(cwd);
        let dirty = self.is_dirty(cwd);
        let dirty_files = if dirty { self.dirty_files(cwd) } else { 0 };
        let (ahead, behind) = self.ahead_behind(cwd, &branch);
        crate::contract::RepoSnapshot {
            branch,
            head,
            behind,
            ahead,
            dirty,
            dirty_files,
            sync_at,
            remote_up_to_date: behind == 0,
        }
    }

    /// fetch origin(网络失败给出可读诊断)。
    pub fn fetch(&self, cwd: &str) -> Result<(), String> {
        let r = self.git(cwd, &["fetch", "origin"])?;
        if r.code == 0 {
            return Ok(());
        }
        let detail = r.tail.join(" ").chars().take(400).collect::<String>();
        if detail.contains("Could not resolve host")
            || detail.contains("Failed to connect")
            || detail.contains("Operation timed out")
        {
            Err("网络无法连接远端(检查网络/代理/远端地址)".into())
        } else {
            Err(detail)
        }
    }

    /// 完整同步:fetch → dirty 自动 stash → pull --rebase --autostash → 冲突只报告。
    pub fn sync(&self, cwd: &str) -> SyncOut {
        self.log
            .append("git", crate::contract::LogLevel::Info, "git fetch origin …");
        if let Err(e) = self.fetch(cwd) {
            return SyncOut {
                ok: false,
                stage: "fetch".into(),
                error: Some(e),
                conflicts: vec![],
                stashed: false,
                tail: vec![],
            };
        }
        let branch = self.current_branch(cwd);
        let (_, behind) = self.ahead_behind(cwd, &branch);
        let dirty = self.is_dirty(cwd);

        let mut stashed = false;
        if dirty {
            self.log.append(
                "git",
                crate::contract::LogLevel::Info,
                "工作区有未提交改动,自动 git stash push -u(可随时 git stash pop 恢复)",
            );
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let st = match self.git(
                cwd,
                &[
                    "stash",
                    "push",
                    "-u",
                    "-m",
                    &format!("dsh-launcher autostash {stamp}"),
                ],
            ) {
                Ok(r) => r,
                Err(e) => {
                    return SyncOut {
                        ok: false,
                        stage: "stash".into(),
                        error: Some(e),
                        conflicts: vec![],
                        stashed: false,
                        tail: vec![],
                    };
                }
            };
            if st.code != 0 {
                return SyncOut {
                    ok: false,
                    stage: "stash".into(),
                    error: Some("自动暂存失败:工作区有冲突性改动或文件被占用".into()),
                    conflicts: vec![],
                    stashed: false,
                    tail: st.tail,
                };
            }
            stashed = true;
        }

        self.log.append(
            "git",
            crate::contract::LogLevel::Info,
            &format!(
                "git pull --rebase --autostash(落后 {} 个提交)",
                if behind >= 0 {
                    behind.to_string()
                } else {
                    "?".into()
                }
            ),
        );
        let pull = match self.git(cwd, &["pull", "--rebase", "--autostash"]) {
            Ok(r) => r,
            Err(e) => {
                return SyncOut {
                    ok: false,
                    stage: "pull".into(),
                    error: Some(e),
                    conflicts: vec![],
                    stashed,
                    tail: vec![],
                };
            }
        };
        if pull.code != 0 {
            let conflicts = self.conflicted_files(cwd);
            let in_rebase = self.rebase_in_progress(cwd);
            if !conflicts.is_empty() || in_rebase {
                return SyncOut {
                    ok: false,
                    stage: "conflict".into(),
                    error: Some("rebase 冲突:工作区未被破坏,请手动解决(编辑冲突文件 → git add → git rebase --continue;或 git rebase --abort 放弃本次合并)".into()),
                    conflicts,
                    stashed,
                    tail: pull.tail,
                };
            }
            return SyncOut {
                ok: false,
                stage: "pull".into(),
                error: Some(pull.tail.join(" ").chars().take(400).collect()),
                conflicts: vec![],
                stashed,
                tail: pull.tail,
            };
        }
        self.log.append(
            "git",
            crate::contract::LogLevel::Ok,
            &format!(
                "pull 完成 → {};{}",
                self.head_short(cwd),
                if behind == 0 {
                    "已是最新"
                } else {
                    "已更新"
                }
            ),
        );
        SyncOut {
            ok: true,
            stage: "ok".into(),
            error: None,
            conflicts: vec![],
            stashed,
            tail: pull.tail,
        }
    }

    pub fn conflicted_files(&self, cwd: &str) -> Vec<String> {
        let r = self
            .git_quiet(cwd, &["diff", "--name-only", "--diff-filter=U"])
            .unwrap_or_default();
        if r.is_empty() {
            vec![]
        } else {
            r.lines().map(String::from).collect()
        }
    }

    /// rebase 是否进行中(目录探测)。
    pub fn rebase_in_progress(&self, cwd: &str) -> bool {
        let git_dir = Path::new(cwd).join(".git");
        let probes = [
            git_dir.join("rebase-merge/head-name"),
            git_dir.join("rebase-apply/rebasing"),
        ];
        probes
            .iter()
            .any(|p| std::fs::read_to_string(p).is_ok_and(|s| !s.is_empty()))
    }

    /// lockfile(pnpm-lock.yaml)在 from..HEAD 之间是否变化。
    pub fn lockfile_changed(&self, cwd: &str, from: &str) -> bool {
        if from.is_empty() {
            return false;
        }
        self.git_quiet(
            cwd,
            &[
                "diff",
                "--name-only",
                &format!("{from}..HEAD"),
                "--",
                "pnpm-lock.yaml",
            ],
        )
        .is_ok_and(|r| !r.is_empty())
    }
}

/// 等待进程退出(超时返回 false)。供集成测试复用。
#[allow(dead_code)]
pub fn wait_process(pid: u32, timeout_ms: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(timeout_ms) {
        if !crate::services::supervisor::Supervisor::is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
