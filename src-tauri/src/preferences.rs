// dsh-launcher · 桌面偏好持久化
// 只管理桌面行为(主题/关闭行为/自启/托盘等),与 Node daemon 的 engine 设置完全分离。
use crate::contract::DesktopPreferences;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 偏好文件:配置目录下 preferences.json(与旧 ~/.config/dsh-launcher.json 分开)。
pub fn preferences_file() -> PathBuf {
    config_dir().join("preferences.json")
}

/// 配置目录:环境变量可覆盖(便于测试),默认 ~/.config/dsh-launcher/。
pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let home = crate::config::home_dir();
    Path::new(&home).join(".config/dsh-launcher")
}

/// 旧 Node daemon 配置(迁移来源)。
fn legacy_node_config() -> PathBuf {
    if let Ok(d) = std::env::var("DSH_LAUNCHER_CONFIG_DIR") {
        return PathBuf::from(d).join("dsh-launcher.json");
    }
    let home = crate::config::home_dir();
    Path::new(&home).join(".config/dsh-launcher.json")
}

/// 读偏好;文件缺失时返回默认值。不修改磁盘(迁移由 load_and_migrate 触发)。
pub fn load() -> DesktopPreferences {
    let f = preferences_file();
    std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| serde_json::from_str::<DesktopPreferences>(&s).ok())
        .unwrap_or_default()
}

/// 加载偏好;若偏好文件不存在,尝试从旧 Node 配置一次性迁移 autostart。
/// 幂等:偏好文件一旦存在就不再迁移。
pub fn load_and_migrate() -> DesktopPreferences {
    let f = preferences_file();
    if f.exists() {
        return load();
    }
    let migrated = migrate_autostart();
    if migrated {
        log::info!("已从旧配置迁移 autostart → launch_on_startup");
    }
    load()
}

/// 一次性迁移:旧 ~/.config/dsh-launcher.json 的 autostart → launch_on_startup。
/// 只读取,不写旧文件,也不调用旧 LaunchAgent 脚本。
fn migrate_autostart() -> bool {
    let raw = match std::fs::read_to_string(legacy_node_config()) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let autostart = v
        .get("autostart")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if !autostart {
        return false;
    }
    let mut prefs = load();
    prefs.launch_on_startup = true;
    save(&prefs).is_ok()
}

/// 原子保存:temp + fsync + rename。失败返回可读错误。
pub fn save(prefs: &DesktopPreferences) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let tmp = dir.join("preferences.json.tmp");
    {
        let mut fh = std::fs::File::create(&tmp).map_err(|e| format!("写入偏好失败: {e}"))?;
        fh.write_all(
            serde_json::to_string_pretty(prefs)
                .map_err(|e| format!("偏好序列化失败: {e}"))?
                .as_bytes(),
        )
        .map_err(|e| format!("写入偏好失败: {e}"))?;
        fh.sync_all().map_err(|e| format!("偏好 fsync 失败: {e}"))?;
    }
    std::fs::rename(&tmp, preferences_file()).map_err(|e| format!("偏好落盘失败: {e}"))
}

/// 校验并保存:theme/close_behavior 枚举由 serde 保证,此处只做完整替换。
pub fn save_validated(prefs: &DesktopPreferences) -> Result<DesktopPreferences, String> {
    save(prefs)?;
    Ok(prefs.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{CloseBehavior, Theme};
    use crate::test_lock::ENV_LOCK;

    fn temp_config_dir(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("dsh-prefs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", &base);
        base
    }

    #[test]
    fn save_load_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_config_dir("roundtrip");
        let prefs = DesktopPreferences {
            theme: Theme::Dark,
            close_behavior: CloseBehavior::Quit,
            launch_on_startup: true,
            silent_startup: true,
            show_tray_icon: false,
            confirm_stop_and_quit: false,
        };
        save(&prefs).unwrap();
        assert_eq!(load(), prefs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_config_dir("missing");
        let p = load();
        assert_eq!(p, DesktopPreferences::default());
        assert_eq!(p.theme, Theme::System);
        assert_eq!(p.close_behavior, CloseBehavior::Tray);
        assert!(p.show_tray_icon);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_autostart_once_from_legacy_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_config_dir("migrate");
        // 先造旧 Node 配置
        std::fs::write(
            dir.join("dsh-launcher.json"),
            r#"{"repoPath":"/x","autostart":true,"port":3080}"#,
        )
        .unwrap();
        let p = load_and_migrate();
        assert!(p.launch_on_startup, "autostart 应迁移为 launch_on_startup");
        // 幂等:再次迁移不改变已存在文件
        std::fs::write(dir.join("dsh-launcher.json"), r#"{"autostart":false}"#).unwrap();
        let p2 = load_and_migrate();
        assert!(p2.launch_on_startup, "偏好文件存在时不应重复迁移");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_migration_when_autostart_false() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_config_dir("no-migrate");
        std::fs::write(dir.join("dsh-launcher.json"), r#"{"autostart":false}"#).unwrap();
        let p = load_and_migrate();
        assert!(!p.launch_on_startup);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_preferences_file_falls_back_to_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_config_dir("corrupt");
        std::fs::write(dir.join("preferences.json"), "not json{{{").unwrap();
        let p = load();
        assert_eq!(p, DesktopPreferences::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
