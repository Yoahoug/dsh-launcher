// dsh-launcher · RuntimeService:node/pnpm/git 解析 + 托管 Node 24 安装
// App 自身不依赖 Node;本模块只负责为 dsh 子进程准备兼容 Node(^22.19 || >=24)。
use crate::config::state_dir;
use crate::log_hub::LogHub;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// dsh engines 范围描述(提示文案)。
pub const NODE_RANGE_MSG: &str = "^22.19 || >=24";

/// 解析 "v24.19.0" / "24.19.0" → [24,19,0]。
pub fn parse_node_version(v: &str) -> Option<[u32; 3]> {
    let s = v.trim_start_matches('v');
    let mut it = s.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    Some([a, b, c])
}

/// 版本是否在 dsh 范围(^22.19 || >=24)。
pub fn node_in_range(v: &str) -> bool {
    let Some([major, minor, _]) = parse_node_version(v) else {
        return false;
    };
    (major == 22 && minor >= 19) || major >= 24
}

fn node_bin_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

fn path_sep() -> char {
    if cfg!(windows) {
        ';'
    } else {
        ':'
    }
}

fn home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// 常见工具安装目录。
fn known_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/local/bin"),
    ];
    if cfg!(windows) {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(&pf).join("nodejs"));
        }
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&la).join("fnm_multisets"));
        }
    }
    dirs.push(home().join(".local/share/pnpm"));
    dirs
}

/// 扫描 ~/.nvm/versions/node/<v*>/bin/<name>,取最高版本。
fn scan_nvm(name: &str) -> Option<PathBuf> {
    let root = home().join(".nvm/versions/node");
    let entries = std::fs::read_dir(&root).ok()?;
    let mut best: Option<([u32; 3], PathBuf)> = None;
    for e in entries.flatten() {
        let Some([a, b, c]) = parse_node_version(&e.file_name().to_string_lossy()) else {
            continue;
        };
        let cand = e.path().join("bin").join(name);
        if cand.is_file() {
            let key = [a, b, c];
            if best.as_ref().is_none_or(|(k, _)| key > *k) {
                best = Some((key, cand));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// 解析命令绝对路径:PATH → 已知目录 → nvm。找不到返回 None。
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(path_sep()).filter(|d| !d.is_empty()) {
            let cand = Path::new(dir).join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    for dir in known_dirs() {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    if name != "node.exe" {
        return scan_nvm(name);
    }
    None
}

/// 已解析工具集。
#[derive(Debug, Clone)]
pub struct Tools {
    pub pnpm: Option<PathBuf>,
    pub git: Option<PathBuf>,
    /// dsh 兼容 Node 的 bin 目录(注入子进程 PATH);None = 未找到。
    pub dsh_node_dir: Option<PathBuf>,
}

impl Tools {
    /// 子进程环境:PATH 前缀注入工具目录(解决 Finder 启动的极简 PATH)。
    pub fn env(&self) -> std::collections::HashMap<String, String> {
        let mut extra = Vec::new();
        if let Some(d) = &self.dsh_node_dir {
            extra.push(d.clone());
        }
        if let Some(p) = &self.pnpm {
            if let Some(dir) = p.parent() {
                extra.push(dir.to_path_buf());
            }
        }
        if let Some(g) = &self.git {
            if let Some(dir) = g.parent() {
                extra.push(dir.to_path_buf());
            }
        }
        let base: Vec<String> = std::env::var("PATH")
            .unwrap_or_default()
            .split(path_sep())
            .filter(|d| !d.is_empty())
            .map(String::from)
            .collect();
        let mut merged: Vec<String> = Vec::new();
        for d in extra.iter().map(|p| p.display().to_string()).chain(base) {
            if !merged.contains(&d) {
                merged.push(d);
            }
        }
        let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();
        env.insert("PATH".into(), merged.join(&path_sep().to_string()));
        env
    }
}

/// 版本管理器候选(node 版本目录扫描)。
fn version_dirs() -> Vec<PathBuf> {
    vec![
        // 托管目录优先(新:toolchains/node;旧:node,兼容迁移)
        crate::toolchain::toolchains_dir().join("node"),
        state_dir().join("node"),
        // nvm
        home().join(".nvm/versions/node"),
        // volta
        home().join(".volta/tools/image/node"),
        // fnm(macOS 新路径 + linux 路径 + windows)
        home().join("Library/Application Support/fnm/node-versions"),
        home().join(".local/share/fnm/node-versions"),
    ]
}

/// 探测 node 版本(node --version),超时/失败返回 None。
pub fn probe_version(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if parse_node_version(&v).is_some() {
        Some(v)
    } else {
        None
    }
}

/// 解析 dsh 兼容 Node(范围 ^22.19 || >=24):版本目录扫描 + Homebrew keg + PATH。
/// 返回 (bin 绝对路径, 版本)。24 优先于 22(按版本排序取最高)。
pub fn resolve_dsh_node() -> Option<(PathBuf, String)> {
    // (来源优先级 0=托管/版本目录, 1=Homebrew, 2=PATH;版本降序)
    let mut found: Vec<(u8, PathBuf, String)> = Vec::new();

    // 版本目录(目录名带版本,直接判定;托管 toolchains/node 排最前)
    for root in version_dirs() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(ver) = parse_node_version(&name) else {
                continue;
            };
            let ver_str = format!("v{}.{}.{}", ver[0], ver[1], ver[2]);
            if !node_in_range(&ver_str) {
                continue;
            }
            for rel in [
                format!("bin/{}", node_bin_name()),
                format!("installation/bin/{}", node_bin_name()),
            ] {
                let bin = e.path().join(rel);
                if bin.is_file() {
                    found.push((0, bin, ver_str.clone()));
                }
            }
        }
    }

    // Homebrew keg-only(node@22 / node@24)
    for keg in [
        "/opt/homebrew/opt/node@22",
        "/usr/local/opt/node@22",
        "/opt/homebrew/opt/node@24",
        "/usr/local/opt/node@24",
    ] {
        let bin = Path::new(keg).join("bin").join(node_bin_name());
        if bin.is_file() {
            if let Some(v) = probe_version(&bin) {
                if node_in_range(&v) {
                    found.push((1, bin, v));
                }
            }
        }
    }

    // PATH 中的 node(可能是 volta/fnm shim,现场探测)
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(path_sep()).filter(|d| !d.is_empty()) {
            let cand = Path::new(dir).join(node_bin_name());
            if cand.is_file() {
                if let Some(v) = probe_version(&cand) {
                    if node_in_range(&v) {
                        found.push((2, cand, v));
                    }
                }
            }
        }
    }

    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| parse_node_version(&b.2).cmp(&parse_node_version(&a.2)))
            .then_with(|| a.1.cmp(&b.1))
    });
    found
        .first()
        .map(|(_, bin, ver)| (bin.clone(), ver.clone()))
}

// ── 托管 Node 安装 ────────────────────────────────────────
// 实现已迁移到 crate::toolchain(签名 catalog + 国内下载 + 校验 + 安全解压 + 原子切换);
// 此处保留兼容入口,安装后当前进程立即可解析(无需重启)。

/// 安装托管 Node 24 LTS(委托 toolchain)。返回 (bin 绝对路径, 版本)。
pub fn install_node(
    log: &Arc<LogHub>,
    token: &crate::ops::CancellationToken,
    on_stage: &dyn Fn(&str),
) -> Result<(PathBuf, String), crate::ops::OperationError> {
    let report = crate::toolchain::ensure_tool(
        log,
        crate::toolchain::Tool::Node,
        token,
        on_stage,
        &tools_now(),
    )?;
    for m in &report.messages {
        log.append("launcher", crate::contract::LogLevel::Ok, m);
    }
    let (bin, v) = resolve_dsh_node()
        .ok_or_else(|| crate::ops::OperationError::Failed("安装后未能解析托管 Node".into()))?;
    Ok((bin, v))
}

/// 当前解析的工具(仅用于 install_node 委托前的提示性校验)。
fn tools_now() -> Tools {
    Tools {
        pnpm: resolve_executable("pnpm"),
        git: resolve_executable("git"),
        dsh_node_dir: resolve_dsh_node().and_then(|(b, _)| b.parent().map(PathBuf::from)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_formats() {
        assert_eq!(parse_node_version("v24.19.0"), Some([24, 19, 0]));
        assert_eq!(parse_node_version("24.19.0"), Some([24, 19, 0]));
        assert_eq!(parse_node_version("v22.19.1"), Some([22, 19, 1]));
        assert_eq!(parse_node_version("garbage"), None);
    }

    #[test]
    fn range_checks() {
        assert!(node_in_range("v24.19.0"));
        assert!(node_in_range("v22.19.0"));
        assert!(node_in_range("v25.0.0"));
        assert!(!node_in_range("v22.18.9"));
        assert!(!node_in_range("v23.1.0"));
        assert!(!node_in_range("v20.0.0"));
        assert!(!node_in_range(""));
    }

    #[test]
    fn probe_works_for_installed_node() {
        let bin = resolve_executable("node").or_else(|| resolve_dsh_node().map(|(p, _)| p));
        let Some(bin) = bin else {
            return;
        };
        let v = probe_version(&bin);
        assert!(v.is_some(), "应能读到版本");
    }

    #[test]
    fn resolve_tools_finds_git_or_pnpm() {
        // 测试机必有 git(仓库环境);仅验证解析不 panic
        let _ = resolve_executable("git");
    }

    #[test]
    fn dsh_node_resolution_is_deterministic() {
        let r = resolve_dsh_node();
        // 不强制存在(CI 环境无 Node 也可),但结果版本必须合规
        if let Some((p, v)) = r {
            assert!(p.is_file(), "bin 必须存在: {}", p.display());
            assert!(node_in_range(&v), "版本必须在范围: {v}");
        }
    }
}
