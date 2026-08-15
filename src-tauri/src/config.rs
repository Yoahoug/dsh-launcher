// dsh-launcher · 引擎设置(EngineSettings)
// 兼容旧 ~/.config/dsh-launcher.json;写入 temp + fsync + rename(原子)。
use crate::contract::{RepoUsable, SettingsSnapshot};
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// HOME(Windows 回退 USERPROFILE)。
pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
}

/// 配置目录:环境变量可覆盖(便于测试),默认 ~/.config/。
pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    Path::new(&home_dir()).join(".config")
}

/// 配置文件(与旧 Node daemon 同一路径,保证配置兼容)。
pub fn config_file() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_CONFIG_DIR") {
        return PathBuf::from(d).join("dsh-launcher.json");
    }
    Path::new(&home_dir()).join(".config/dsh-launcher.json")
}

/// 运行态目录:pid 文件、状态快照、日志、托管 Node。
pub fn state_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_STATE_DIR") {
        return PathBuf::from(d);
    }
    Path::new(&home_dir()).join(".local/state/dsh-launcher")
}

pub fn logs_dir() -> PathBuf {
    state_dir().join("logs")
}

/// 展开路径中的 ~ 与 $HOME。
pub fn expand_path(p: &str) -> String {
    let home = home_dir();
    match p {
        "~" => home,
        s if s.starts_with("~/") => format!("{}{}", home, &s[1..]),
        s if s.starts_with("$HOME/") => format!("{}{}", home, &s[5..]),
        s => s.to_string(),
    }
}

/// 读设置:磁盘 JSON + 默认值合并(缺失字段用默认)。
pub fn load() -> SettingsSnapshot {
    let defaults = SettingsSnapshot::default();
    let raw = match std::fs::read_to_string(config_file()) {
        Ok(s) => s,
        Err(_) => return defaults,
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return defaults,
    };
    let field = |name: &str| v.get(name).and_then(|x| x.as_str()).map(String::from);
    SettingsSnapshot {
        repo_path: expand_path(&field("repoPath").unwrap_or(defaults.repo_path)),
        port: v
            .get("port")
            .and_then(|x| x.as_u64())
            .map(|n| n as u16)
            .filter(|n| *n >= 1)
            .unwrap_or(defaults.port),
        host: field("host").unwrap_or(defaults.host),
        dsh_home: field("dshHome").unwrap_or(defaults.dsh_home),
        autostart: v
            .get("autostart")
            .and_then(|x| x.as_bool())
            .unwrap_or(defaults.autostart),
        open_browser: v
            .get("openBrowser")
            .and_then(|x| x.as_bool())
            .unwrap_or(defaults.open_browser),
        auto_update_check: v
            .get("autoUpdateCheck")
            .and_then(|x| x.as_bool())
            .unwrap_or(defaults.auto_update_check),
        build_args: field("buildArgs").unwrap_or(defaults.build_args),
        ready_timeout_ms: v
            .get("readyTimeoutMs")
            .and_then(|x| x.as_u64())
            .filter(|n| *n >= 5000)
            .unwrap_or(defaults.ready_timeout_ms),
        start_timeout_ms: v
            .get("startTimeoutMs")
            .and_then(|x| x.as_u64())
            .filter(|n| *n >= 5000)
            .unwrap_or(defaults.start_timeout_ms),
    }
}

/// 校验补丁(规则与旧 validateConfig 对齐),返回合并后的完整设置。
pub fn validate_patch(patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
    let cur = load();
    let mut next = cur.clone();
    if let Some(p) = patch.get("repoPath").and_then(|x| x.as_str()) {
        let p = expand_path(p.trim());
        if p.is_empty() {
            return Err("仓库路径不能为空".into());
        }
        next.repo_path = p;
    }
    if let Some(p) = patch.get("port") {
        let n = p.as_u64().ok_or("端口必须是数字")?;
        if !(1..=65535).contains(&n) {
            return Err("端口必须是 1–65535 的数字".into());
        }
        next.port = n as u16;
    }
    if let Some(h) = patch.get("host").and_then(|x| x.as_str()) {
        next.host = h.trim().to_string();
    }
    if let Some(d) = patch.get("dshHome").and_then(|x| x.as_str()) {
        next.dsh_home = d.trim().to_string();
    }
    if let Some(b) = patch.get("buildArgs").and_then(|x| x.as_str()) {
        next.build_args = b.trim().to_string();
    }
    if let Some(n) = patch.get("readyTimeoutMs").and_then(|x| x.as_u64()) {
        if n >= 5000 {
            next.ready_timeout_ms = n;
        }
    }
    if let Some(n) = patch.get("startTimeoutMs").and_then(|x| x.as_u64()) {
        if n >= 5000 {
            next.start_timeout_ms = n;
        }
    }
    if let Some(b) = patch.get("openBrowser").and_then(|x| x.as_bool()) {
        next.open_browser = b;
    }
    if let Some(b) = patch.get("autoUpdateCheck").and_then(|x| x.as_bool()) {
        next.auto_update_check = b;
    }
    // autostart 字段兼容读取(桌面版由 preferences 管理,不再通过本文件生效)
    Ok(next)
}

/// 原子保存完整设置。
pub fn save(settings: &SettingsSnapshot) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let tmp = dir.join("dsh-launcher.json.tmp");
    {
        let mut fh = std::fs::File::create(&tmp).map_err(|e| format!("写入配置失败: {e}"))?;
        let json =
            serde_json::to_string_pretty(settings).map_err(|e| format!("序列化失败: {e}"))?;
        fh.write_all(json.as_bytes())
            .map_err(|e| format!("写入配置失败: {e}"))?;
        fh.sync_all().map_err(|e| format!("配置 fsync 失败: {e}"))?;
    }
    std::fs::rename(&tmp, config_file()).map_err(|e| format!("配置落盘失败: {e}"))
}

/// 校验并保存补丁;返回保存后的完整设置。
pub fn apply_patch(patch: &serde_json::Value) -> Result<SettingsSnapshot, String> {
    let next = validate_patch(patch)?;
    save(&next)?;
    Ok(next)
}

/// 仓库路径是否可用(git 仓库)。
pub fn repo_usable(repo_path: &str) -> RepoUsable {
    let p = Path::new(repo_path);
    if repo_path.is_empty() || !p.exists() {
        return RepoUsable {
            ok: false,
            reason: Some("目录不存在".into()),
        };
    }
    if !p.join(".git").exists() {
        return RepoUsable {
            ok: false,
            reason: Some("不是 git 仓库(缺少 .git)".into()),
        };
    }
    RepoUsable {
        ok: true,
        reason: None,
    }
}

/// 前端 dist 是否已构建。
pub fn dist_built(repo_path: &str) -> Option<bool> {
    let p = Path::new(repo_path).join("apps/web/dist/index.html");
    match p.exists() {
        true => Some(true),
        false if Path::new(repo_path).exists() => Some(false),
        false => None,
    }
}

/// 端口是否被监听(127.0.0.1 connect 探测)。
pub fn probe_port(host: &str, port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:1".parse().unwrap()),
        Duration::from_millis(800),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::ENV_LOCK;

    fn temp_dir(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("dsh-config-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
        base
    }

    #[test]
    fn defaults_when_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("missing");
        let s = load();
        assert_eq!(s.port, 3080);
        assert_eq!(s.ready_timeout_ms, 120_000);
        assert!(s.open_browser);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_legacy_file_with_expansion() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("legacy");
        std::fs::write(
            dir.join("dsh-launcher.json"),
            r#"{"repoPath":"~/Desktop/deepseek-harness","port":3081,"readyTimeoutMs":60000}"#,
        )
        .unwrap();
        let s = load();
        let home = home_dir();
        assert_eq!(s.repo_path, format!("{home}/Desktop/deepseek-harness"));
        assert_eq!(s.port, 3081);
        assert_eq!(s.ready_timeout_ms, 60_000);
        assert_eq!(s.host, "127.0.0.1", "缺失字段用默认");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validates_port_range() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("port");
        let err = validate_patch(&serde_json::json!({ "port": 70000 })).unwrap_err();
        assert!(err.contains("1–65535"));
        let err2 = validate_patch(&serde_json::json!({ "port": 0 })).unwrap_err();
        assert!(err2.contains("1–65535"));
        let ok = validate_patch(&serde_json::json!({ "port": 4000 })).unwrap();
        assert_eq!(ok.port, 4000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_patch_persists_atomically() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("persist");
        let s = apply_patch(&serde_json::json!({ "port": 3082, "host": "0.0.0.0" })).unwrap();
        assert_eq!(s.port, 3082);
        assert_eq!(s.host, "0.0.0.0");
        let s2 = load();
        assert_eq!(s2.port, 3082);
        assert!(dir.join("dsh-launcher.json").exists());
        assert!(
            !dir.join("dsh-launcher.json.tmp").exists(),
            "临时文件应已 rename"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_usable_checks_git_dir() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("repo");
        let missing = repo_usable(&dir.join("nope").display().to_string());
        assert!(!missing.ok);
        std::fs::create_dir_all(dir.join("proj/.git")).unwrap();
        let ok = repo_usable(&dir.join("proj").display().to_string());
        assert!(ok.ok);
        let notgit = repo_usable(&dir.display().to_string());
        assert!(!notgit.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dist_built_detection() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("dist");
        assert_eq!(dist_built(&dir.display().to_string()), Some(false));
        std::fs::create_dir_all(dir.join("apps/web/dist")).unwrap();
        std::fs::write(dir.join("apps/web/dist/index.html"), "x").unwrap();
        assert_eq!(dist_built(&dir.display().to_string()), Some(true));
        assert_eq!(dist_built("/definitely/not/exists"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
