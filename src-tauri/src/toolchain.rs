// dsh-launcher · 托管工具链(M1)
//
// 版本化目录 + active pointer:state_dir()/toolchains/{node,git,pnpm}/<version>,
// active pointer 为 toolchains/active.json;不修改系统 PATH / 全局 npm / Git 配置。
// 所有下载只走签名 catalog 内的国内镜像 URL;下载 → .part → 长度+SHA-256 校验 →
// 安全解压 → 工具自检 → 原子切换 → 更新 InstallationSnapshot。
// pnpm 精确版本必须来自 clone 后真实 package.json 的 packageManager,且必须在 catalog 内
// (否则安全失败并给出明确提示,不静默使用其它版本)。
use crate::archive;
use crate::catalog::{self, CatalogEntry};
use crate::config::state_dir;
use crate::download;
use crate::log_hub::LogHub;
use crate::ops::{CancellationToken, InstalledComponent, OperationError};
use crate::services::runtime::{self, Tools};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const NODE_CATALOG_VERSION: &str = "v24.9.0";
pub const MINGIT_CATALOG_VERSION: &str = "2.55.0.4";
pub const PNPM_CATALOG_VERSIONS: [&str; 2] = ["11.7.0", "11.21.0"];

/// 可安装的工具(All = Node + Git + pnpm 全量)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Node,
    Git,
    Pnpm,
    All,
}

/// 安装结果(供流程层写入日志)。
pub struct InstallReport {
    pub messages: Vec<String>,
}

pub fn toolchains_dir() -> PathBuf {
    state_dir().join("toolchains")
}

fn active_file() -> PathBuf {
    toolchains_dir().join("active.json")
}

fn downloads_dir() -> PathBuf {
    state_dir().join("downloads")
}

// ── active pointer ───────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePointers {
    pub node: Option<String>,
    pub git: Option<String>,
    pub pnpm: Option<String>,
}

fn load_active() -> ActivePointers {
    std::fs::read_to_string(active_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_active(a: &ActivePointers) {
    let _ = std::fs::create_dir_all(toolchains_dir());
    if let Ok(json) = serde_json::to_string_pretty(a) {
        let _ = std::fs::write(active_file(), json);
    }
}

/// 组件目录 + 解压后自检可执行文件路径。
fn component_paths(cat: &CatalogEntry) -> (PathBuf, PathBuf, PathBuf) {
    // (版本目录, 自检可执行, 校验后解压的根)
    match cat.id.as_str() {
        "node" => {
            let dir = toolchains_dir().join("node").join(&cat.version);
            let bin = if cfg!(windows) {
                dir.join("node.exe")
            } else {
                dir.join("bin/node")
            };
            let root = dir.clone();
            (dir, bin, root)
        }
        "mingit" => {
            let dir = toolchains_dir().join("git").join(&cat.version);
            let bin = dir.join("cmd/git.exe");
            let root = dir.clone();
            (dir, bin, root)
        }
        "pnpm" => {
            let dir = toolchains_dir().join("pnpm").join(&cat.version);
            let bin = if cfg!(windows) {
                dir.join("pnpm.cmd")
            } else {
                dir.join("pnpm")
            };
            let root = dir.clone();
            (dir, bin, root)
        }
        other => panic!("未知组件 {other}"),
    }
}

/// 组件是否已就绪(目录 + 自检可执行存在 + active pointer 指向该版本)。
fn component_ready(id: &str, version: &str) -> bool {
    let active = load_active();
    let (dir, bin, _) = match id {
        "node" => component_paths(&CatalogEntry {
            id: "node".into(),
            version: version.into(),
            platform: String::new(),
            kind: String::new(),
            url: String::new(),
            size: 0,
            sha256: String::new(),
        }),
        "mingit" => component_paths(&CatalogEntry {
            id: "mingit".into(),
            version: version.into(),
            platform: String::new(),
            kind: String::new(),
            url: String::new(),
            size: 0,
            sha256: String::new(),
        }),
        _ => component_paths(&CatalogEntry {
            id: "pnpm".into(),
            version: version.into(),
            platform: String::new(),
            kind: String::new(),
            url: String::new(),
            size: 0,
            sha256: String::new(),
        }),
    };
    if !dir.is_dir() || !bin.is_file() {
        return false;
    }
    match id {
        "node" => active.node.as_deref() == Some(version),
        "mingit" => active.git.as_deref() == Some(version),
        _ => active.pnpm.as_deref() == Some(version),
    }
}

/// 安装单个组件(已就绪则跳过,幂等)。
fn install_component(
    log: &Arc<LogHub>,
    entry: &CatalogEntry,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str),
) -> Result<InstallReport, OperationError> {
    let mut report = InstallReport {
        messages: Vec::new(),
    };
    let (dir, bin, _) = component_paths(entry);
    if dir.is_dir() && bin.is_file() {
        report
            .messages
            .push(format!("{} {} 已安装,跳过", entry.id, entry.version));
        return Ok(report);
    }

    // 1. 下载(.part → 长度 + SHA-256 校验 → 原子改名)
    let file_name = entry
        .url
        .rsplit('/')
        .next()
        .ok_or_else(|| OperationError::Failed(format!("URL 缺少文件名:{}", entry.url)))?;
    let dest = downloads_dir().join(file_name);
    std::fs::create_dir_all(downloads_dir())
        .map_err(|e| OperationError::Failed(format!("创建下载目录失败:{e}")))?;
    let cached_verified = if dest.is_file() {
        verify_cached_download(&dest, entry.size, &entry.sha256)
    } else {
        false
    };
    if !cached_verified {
        if dest.exists() {
            let _ = std::fs::remove_file(&dest);
            log.append(
                "launcher",
                crate::contract::LogLevel::Warn,
                &format!("{} 安装包缓存校验失败,已清理并重新下载", entry.id),
            );
        }
        on_stage(&format!("下载 {} {}…", entry.id, entry.version));
        log.append(
            "launcher",
            crate::contract::LogLevel::Info,
            &format!(
                "下载 {} {} → {} ({} MB,国内镜像)",
                entry.id,
                entry.version,
                entry.url,
                entry.size / 1048576
            ),
        );
        download::download_and_verify(
            &entry.url,
            &dest,
            entry.size,
            &entry.sha256,
            token,
            600_000,
            &|received, total| {
                let pct = (received as f64 / total as f64 * 100.0) as u64;
                if pct % 10 == 0 {
                    log.append(
                        "launcher",
                        crate::contract::LogLevel::Info,
                        &format!("下载 {}: {pct}%", entry.id),
                    );
                }
            },
        )?;
        log.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            &format!("下载完成并通过长度/SHA-256 校验({} 字节)", entry.size),
        );
    } else {
        log.append(
            "launcher",
            crate::contract::LogLevel::Info,
            &format!("{} 安装包已缓存,复用 {}", entry.id, dest.display()),
        );
    }

    // 2. 安全解压到临时目录
    let tmp = toolchains_dir().join(format!(".tmp-{}-{}", entry.id, entry.version));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| OperationError::Failed(format!("创建临时目录失败:{e}")))?;
    on_stage(&format!("解压 {}…", entry.id));
    let extract_result: Result<(), OperationError> = match entry.kind.as_str() {
        "zip" => archive::extract_zip(&dest, &tmp, token),
        "tar.gz" | "tgz" => archive::extract_tar_gz(&dest, &tmp, token),
        other => Err(OperationError::Failed(format!("未知归档类型 {other}"))),
    };
    if let Err(e) = extract_result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }

    // 3. 移动到版本目录(原子切换)
    //    先按自检可执行定位「内容根」:Node 官方归档带外层包装目录
    //    (node-v24.9.0-darwin-arm64/ 或 node-v24.9.0-win-x64/),MinGit zip 直接是根,
    //    pnpm tgz 是 package/。
    let _ = std::fs::remove_dir_all(&dir);
    let rel_bin = match entry.id.as_str() {
        "node" => {
            if cfg!(windows) {
                "node.exe"
            } else {
                "bin/node"
            }
        }
        "mingit" => "cmd/git.exe",
        "pnpm" => "bin/pnpm.cjs",
        other => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(OperationError::Failed(format!("未知组件 {other}")));
        }
    };
    let content_root = find_content_root(&tmp, rel_bin).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&tmp);
        OperationError::Failed(format!("解压产物缺少自检可执行({rel_bin}),结构异常"))
    })?;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
    }
    std::fs::rename(&content_root, &dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp);
        OperationError::Failed(format!("移动版本目录失败:{e}"))
    })?;
    if entry.id == "pnpm" {
        write_pnpm_shims(&dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            OperationError::Failed(e)
        })?;
    }
    let _ = std::fs::remove_dir_all(&tmp);

    // 4. 工具自检
    on_stage(&format!("自检 {}…", entry.id));
    let selfcheck = match entry.id.as_str() {
        "node" => runtime::probe_version(&bin),
        "mingit" => run_selfcheck(&bin, &["--version"]),
        "pnpm" => run_selfcheck(&bin, &["--version"]),
        _other => None,
    };
    if selfcheck.is_none() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(OperationError::Failed(format!(
            "{} 自检失败(安装后无法运行),已清理",
            entry.id
        )));
    }

    // 5. 更新 active pointer + InstallationSnapshot
    let mut active = load_active();
    match entry.id.as_str() {
        "node" => active.node = Some(entry.version.clone()),
        "mingit" => active.git = Some(entry.version.clone()),
        "pnpm" => active.pnpm = Some(entry.version.clone()),
        other => {
            let _ = other;
        }
    }
    save_active(&active);
    let mut snap = crate::ops::load_installation();
    let component = InstalledComponent {
        version: entry.version.clone(),
        path: bin.display().to_string(),
        verified: true,
        source: "managed".into(),
    };
    match entry.id.as_str() {
        "node" => snap.node = Some(component),
        "mingit" => snap.git = Some(component),
        "pnpm" => snap.pnpm = Some(component),
        _ => {}
    }
    snap.catalog_version = catalog::CATALOG_SCHEMA;
    snap.installed_at = Some(now_ms());
    crate::ops::save_installation(&snap).map_err(OperationError::Failed)?;

    report.messages.push(format!(
        "{} {} 安装完成并通过自检 → {}",
        entry.id,
        entry.version,
        bin.display()
    ));
    Ok(report)
}

/// 缓存同样属于供应链输入：每次复用前重新核对 catalog 中的长度与 SHA-256。
fn verify_cached_download(path: &Path, expected_size: u64, expected_sha256: &str) -> bool {
    let size_ok = std::fs::metadata(path)
        .map(|metadata| metadata.len() == expected_size)
        .unwrap_or(false);
    size_ok
        && download::sha256_hex(path)
            .map(|actual| actual.eq_ignore_ascii_case(expected_sha256))
            .unwrap_or(false)
}

/// 定位解压内容根:优先 tmp 直接包含自检可执行;否则在唯一的顶层子目录中找
/// (Node 官方归档带 node-vX.Y.Z-<platform>/ 包装目录;多候选视为结构异常)。
fn find_content_root(tmp: &Path, rel_bin: &str) -> Option<PathBuf> {
    if tmp.join(rel_bin).is_file() {
        return Some(tmp.to_path_buf());
    }
    let mut found: Option<PathBuf> = None;
    let entries = std::fs::read_dir(tmp).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && p.join(rel_bin).is_file() {
            if found.is_some() {
                return None; // 多个候选 → 歧义
            }
            found = Some(p);
        }
    }
    found
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn run_selfcheck(bin: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// 写 pnpm shim(node 从子进程 PATH 解析,由 Tools::env() 注入托管 node 目录)。
fn write_pnpm_shims(dir: &Path) -> Result<(), String> {
    let entry = dir.join("bin/pnpm.cjs");
    if !entry.is_file() {
        return Err("pnpm 缺少 bin/pnpm.cjs(入口文件)".into());
    }
    #[cfg(windows)]
    {
        let cmd = dir.join("pnpm.cmd");
        let content = "@echo off\r\nnode \"%~dp0bin\\pnpm.cjs\" %*\r\n";
        std::fs::write(&cmd, content).map_err(|e| format!("写 pnpm.cmd 失败:{e}"))?;
    }
    #[cfg(unix)]
    {
        let sh = dir.join("pnpm");
        let content = "#!/bin/sh\nexec node \"$(dirname \"$0\")/bin/pnpm.cjs\" \"$@\"\n";
        std::fs::write(&sh, content).map_err(|e| format!("写 pnpm shim 失败:{e}"))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置 pnpm 可执行失败:{e}"))?;
    }
    Ok(())
}

// ── 解析(托管优先,系统回退;UI 显示来源) ────────────────

/// 托管 git(Windows MinGit);非 Windows 用系统 git。
pub fn resolve_git(tools: &Tools) -> Option<PathBuf> {
    let active = load_active();
    if cfg!(windows) {
        if let Some(v) = active.git {
            let p = toolchains_dir().join("git").join(&v).join("cmd/git.exe");
            if p.is_file() {
                return Some(p);
            }
        }
        // 未托管时回退系统 git
        tools.git.clone()
    } else {
        tools.git.clone()
    }
}

/// 托管 pnpm;否则系统 pnpm。
pub fn resolve_pnpm(tools: &Tools) -> Option<PathBuf> {
    let active = load_active();
    if let Some(v) = active.pnpm {
        let shim = if cfg!(windows) {
            toolchains_dir().join("pnpm").join(&v).join("pnpm.cmd")
        } else {
            toolchains_dir().join("pnpm").join(&v).join("pnpm")
        };
        if shim.is_file() {
            return Some(shim);
        }
    }
    tools.pnpm.clone()
}

// ── 顶层安装入口 ─────────────────────────────────────────

/// 确保工具链就绪(幂等;缺失才安装)。
pub fn ensure_tool(
    log: &Arc<LogHub>,
    tool: Tool,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str),
    tools: &Tools,
) -> Result<InstallReport, OperationError> {
    token.check()?;
    let cat = catalog::load_catalog().map_err(OperationError::Failed)?;
    let platform = catalog::current_platform();
    let mut report = InstallReport {
        messages: Vec::new(),
    };

    let install_one = |id: &str, version: &str| -> Result<Vec<String>, OperationError> {
        if component_ready(id, version) {
            return Ok(vec![format!("{id} {version} 已就绪(托管),跳过")]);
        }
        let entry = catalog::lookup(&cat, id, version, &platform).ok_or_else(|| {
            OperationError::Failed(format!(
                "catalog 中没有 {id} {version} 的 {platform} 条目;请升级 Launcher 或联系维护者"
            ))
        })?;
        let r = install_component(log, entry, token, on_stage)?;
        Ok(r.messages)
    };

    match tool {
        Tool::Node => report
            .messages
            .extend(install_one("node", NODE_CATALOG_VERSION)?),
        Tool::Git => {
            if !cfg!(windows) {
                if tools.git.is_some() {
                    report
                        .messages
                        .push("macOS 使用系统 git(来源:系统安装)".into());
                    return Ok(report);
                }
                return Err(OperationError::Failed(
                    "macOS 未检测到系统 git,请先安装 Git for macOS 或 Homebrew git".into(),
                ));
            }
            report
                .messages
                .extend(install_one("mingit", MINGIT_CATALOG_VERSION)?);
        }
        Tool::Pnpm => {
            // 默认安装 catalog 内的 pnpm 11.7.0(与 DSH master 的 packageManager 一致)
            report
                .messages
                .extend(install_one("pnpm", PNPM_CATALOG_VERSIONS[0])?);
        }
        Tool::All => {
            if runtime::resolve_dsh_node().is_none() {
                report
                    .messages
                    .extend(install_one("node", NODE_CATALOG_VERSION)?);
            } else {
                report.messages.push("Node 已就绪,跳过".into());
            }
            if cfg!(windows) && resolve_git(tools).is_none() {
                report
                    .messages
                    .extend(install_one("mingit", MINGIT_CATALOG_VERSION)?);
            }
            if resolve_pnpm(tools).is_none() {
                report
                    .messages
                    .extend(install_one("pnpm", PNPM_CATALOG_VERSIONS[0])?);
            } else {
                report.messages.push("pnpm 已就绪,跳过".into());
            }
        }
    }
    Ok(report)
}

/// 从 clone 后真实 package.json 解析 packageManager 的精确版本。
/// 返回 (pnpm 版本, 是否在 catalog 内)。版本不在 catalog 时返回 Err(安全失败)。
pub fn pnpm_version_from_package_json(repo_path: &str) -> Result<String, OperationError> {
    let raw = std::fs::read_to_string(Path::new(repo_path).join("package.json"))
        .map_err(|e| OperationError::Failed(format!("读取 package.json 失败:{e}")))?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| OperationError::Failed(format!("package.json 解析失败:{e}")))?;
    let pm = v
        .get("packageManager")
        .and_then(|x| x.as_str())
        .ok_or_else(|| OperationError::Failed("package.json 缺少 packageManager 字段".into()))?;
    // pnpm@11.7.0 或 pnpm@11.7.0+sha512.xxx
    let rest = pm
        .strip_prefix("pnpm@")
        .ok_or_else(|| OperationError::Failed(format!("packageManager 不是 pnpm:{pm}")))?;
    let version = rest.split('+').next().unwrap_or("").to_string();
    if version.is_empty() {
        return Err(OperationError::Failed(format!(
            "packageManager 格式非法:{pm}"
        )));
    }
    // 必须在 catalog 内(安全失败,不静默使用其它版本)
    let cat = catalog::load_catalog().map_err(OperationError::Failed)?;
    if catalog::lookup(&cat, "pnpm", &version, "any").is_none() {
        return Err(OperationError::Failed(format!(
            "仓库要求的 pnpm@{version} 不在本版本 Launcher 的受信 catalog 中;\
             请升级 Launcher 或联系维护者(不会静默使用其它版本)"
        )));
    }
    Ok(version)
}

/// 确保托管 pnpm 已安装指定精确版本(供 clone 全流程调用)。
pub fn ensure_pnpm_version(
    log: &Arc<LogHub>,
    version: &str,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str),
) -> Result<(), OperationError> {
    let cat = catalog::load_catalog().map_err(OperationError::Failed)?;
    if component_ready("pnpm", version) {
        return Ok(());
    }
    let entry = catalog::lookup(&cat, "pnpm", version, "any").ok_or_else(|| {
        OperationError::Failed(format!("catalog 中没有 pnpm {version};请升级 Launcher"))
    })?;
    install_component(log, entry, token, on_stage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256(bytes: &[u8]) -> String {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(bytes))
    }

    #[test]
    fn cached_download_must_match_catalog_size_and_hash() {
        let path = std::env::temp_dir().join(format!(
            "dsh-cache-verify-{}-{}.bin",
            std::process::id(),
            now_ms()
        ));
        let bytes = b"trusted artifact";
        std::fs::write(&path, bytes).unwrap();

        assert!(verify_cached_download(
            &path,
            bytes.len() as u64,
            &sha256(bytes)
        ));
        assert!(!verify_cached_download(
            &path,
            (bytes.len() + 1) as u64,
            &sha256(bytes)
        ));
        assert!(!verify_cached_download(
            &path,
            bytes.len() as u64,
            &"0".repeat(64)
        ));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pnpm_version_parsing() {
        let base = std::env::temp_dir().join(format!("dsh-pm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("package.json"),
            r#"{"packageManager":"pnpm@11.7.0"}"#,
        )
        .unwrap();
        let v = pnpm_version_from_package_json(&base.display().to_string());
        // 需要在 ENV_LOCK 内? 不依赖 env;但 load_catalog 只读资源,安全
        assert_eq!(v.unwrap(), "11.7.0");

        std::fs::write(
            base.join("package.json"),
            r#"{"packageManager":"pnpm@99.0.0+sha512.abc"}"#,
        )
        .unwrap();
        let err = pnpm_version_from_package_json(&base.display().to_string()).unwrap_err();
        assert!(err.to_string().contains("受信 catalog"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn active_pointer_roundtrip() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-ap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &base);
        std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));
        let a = ActivePointers {
            node: Some("v24.9.0".into()),
            git: None,
            pnpm: Some("11.7.0".into()),
        };
        save_active(&a);
        let b = load_active();
        assert_eq!(b.node.as_deref(), Some("v24.9.0"));
        assert_eq!(b.pnpm.as_deref(), Some("11.7.0"));
        assert!(active_file().exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn shims_written_and_executable_marker() {
        // pnpm shim 写入逻辑(POSIX)
        let dir = std::env::temp_dir().join(format!("dsh-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/pnpm.cjs"), "//x").unwrap();
        #[cfg(unix)]
        write_pnpm_shims(&dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(dir.join("pnpm"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(m & 0o111, 0, "shim 必须可执行");
            let content = std::fs::read_to_string(dir.join("pnpm")).unwrap();
            assert!(content.contains("bin/pnpm.cjs"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
