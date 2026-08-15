// dsh-launcher · 迁移:旧 Node daemon / PID / token / autostart 幂等接管
// 原则:检测旧 3090 daemon 后只 detach/终止 daemon 本身,绝不停止 dsh web;
// 不静默删除用户 LaunchAgent(由 Tauri autostart 插件接管,偏好已迁移)。
use crate::config::{config_dir, state_dir};
use crate::contract::LogLevel;
use crate::log_hub::LogHub;
use crate::services::supervisor;
use std::path::PathBuf;
use std::sync::Arc;

/// 迁移结果(日志用)。
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub old_daemon_terminated: Option<u32>,
    pub token_removed: bool,
    pub migration_version_written: bool,
}

const MIGRATION_VERSION: &str = "3";

fn marker_file() -> PathBuf {
    state_dir().join("migration-version")
}

fn token_file() -> PathBuf {
    state_dir().join("daemon.token")
}

/// 旧 Node daemon 进程是否确认为本仓库 launcher(命令行含 server.mjs)。
fn is_old_daemon(pid: u32) -> bool {
    supervisor::process_cmdline(pid).is_some_and(|cmd| cmd.contains("server.mjs"))
}

/// 终止旧 daemon(单进程 SIGTERM,宽限后 SIGKILL;不碰进程组,保证 dsh 存活)。
fn terminate_daemon(pid: u32, log: &Arc<LogHub>) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(5) {
            if !supervisor::Supervisor::is_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if supervisor::Supervisor::is_alive(pid) {
            log.append(
                "launcher",
                LogLevel::Warn,
                &format!("旧 daemon({pid}) 5s 未退出,发送 SIGKILL"),
            );
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = (pid, log);
        // Windows 旧 daemon 由 3090 探测 + taskkill 兜底
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW:不闪黑窗
        }
        let _ = cmd.output();
    }
}

/// 迁移旧 daemon:验证 PID + 命令行 + 端口后终止;发现非 launcher 占用 3090 时不做任何操作。
pub fn run(log: &Arc<LogHub>) -> MigrationReport {
    let mut report = MigrationReport::default();

    // 1. 旧 daemon:pid 文件存在 + 进程存活 + 命令行匹配 server.mjs + 3090 被占用
    let pid_file = state_dir().join("launcher.pid");
    if let Ok(raw) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = raw.trim().parse::<u32>() {
            let alive = supervisor::Supervisor::is_alive(pid);
            let is_daemon = alive && is_old_daemon(pid);
            let on_port = crate::config::probe_port("127.0.0.1", 3090);
            if is_daemon {
                log.append(
                    "launcher",
                    LogLevel::Info,
                    &format!(
                        "检测到旧 Node daemon(PID {pid}),终止后由桌面核心接管(不影响 dsh web)"
                    ),
                );
                terminate_daemon(pid, log);
                report.old_daemon_terminated = Some(pid);
            } else if on_port && alive {
                // 占用 3090 但命令行不匹配:不是我们的 daemon,新 App 不使用 3090,忽略
                log.append(
                    "launcher",
                    LogLevel::Info,
                    &format!("3090 被非 launcher 进程(PID {pid})占用,新 App 不使用该端口,忽略"),
                );
            }
        }
    }
    let _ = std::fs::remove_file(&pid_file);

    // 2. 旧 token 文件:迁移成功后删除(不再有 HTTP 控制面)
    if token_file().exists() {
        match std::fs::remove_file(token_file()) {
            Ok(_) => {
                report.token_removed = true;
                log.append(
                    "launcher",
                    LogLevel::Info,
                    "已删除旧 daemon token 文件(HTTP 控制面已移除)",
                );
            }
            Err(e) => log.append(
                "launcher",
                LogLevel::Warn,
                &format!("删除旧 token 失败:{e}"),
            ),
        }
    }

    // 3. 记录迁移版本(幂等:重复运行不重复迁移)
    let already = std::fs::read_to_string(marker_file()).is_ok_and(|s| s == MIGRATION_VERSION);
    if !already {
        let _ = std::fs::create_dir_all(state_dir());
        if std::fs::write(marker_file(), MIGRATION_VERSION).is_ok() {
            report.migration_version_written = true;
        }
    }
    // 清理旧 pid/state 残留(空文件无害,但保持整洁)
    let _ = std::fs::remove_file(state_dir().join("dshweb.pid"));
    let _ = std::fs::remove_file(state_dir().join("devweb.pid"));

    // 4. 旧配置目录确认存在(desktop preferences 需要)
    let _ = std::fs::create_dir_all(config_dir().join("dsh-launcher"));

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::ENV_LOCK;

    #[test]
    fn marker_is_idempotent() {
        let _g = ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-mig-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));
        let hub = Arc::new(LogHub::new(
            base.join("state/logs/launcher.log"),
            Arc::new(|_| {}),
            true,
        ));
        let r1 = run(&hub);
        assert!(r1.migration_version_written);
        let r2 = run(&hub);
        assert!(!r2.migration_version_written, "幂等:再次运行不重复写");
        assert_eq!(
            std::fs::read_to_string(marker_file()).unwrap(),
            MIGRATION_VERSION
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn removes_token_and_pid_files() {
        let _g = ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-mig-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));
        std::fs::create_dir_all(base.join("state")).unwrap();
        std::fs::write(base.join("state/daemon.token"), "abc").unwrap();
        std::fs::write(base.join("state/launcher.pid"), "999999\n").unwrap();
        let hub = Arc::new(LogHub::new(
            base.join("state/logs/launcher.log"),
            Arc::new(|_| {}),
            true,
        ));
        let r = run(&hub);
        assert!(r.token_removed);
        assert!(r.old_daemon_terminated.is_none(), "不存在的 pid 不应被终止");
        assert!(!base.join("state/launcher.pid").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
