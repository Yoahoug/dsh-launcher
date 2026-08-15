// dsh-launcher · RuntimeService:node/pnpm/git 解析 + 托管 Node 24 安装
// App 自身不依赖 Node;本模块只负责为 dsh 子进程准备兼容 Node(^22.19 || >=24)。
use crate::config::state_dir;
use crate::log_hub::LogHub;
use std::io::Read;
use std::path::{Path, PathBuf};

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
        // nvm
        home().join(".nvm/versions/node"),
        // volta
        home().join(".volta/tools/image/node"),
        // fnm(macOS 新路径 + linux 路径 + windows)
        home().join("Library/Application Support/fnm/node-versions"),
        home().join(".local/share/fnm/node-versions"),
        // 托管目录
        state_dir().join("node"),
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
    let mut found: Vec<(PathBuf, String)> = Vec::new();

    // 版本目录(目录名带版本,直接判定)
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
                format!("{name}/bin/{}", node_bin_name()),
                format!("{name}/installation/bin/{}", node_bin_name()),
            ] {
                let bin = e.path().join(rel);
                if bin.is_file() {
                    found.push((bin, ver_str.clone()));
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
                    found.push((bin, v));
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
                        found.push((cand, v));
                    }
                }
            }
        }
    }

    found.sort_by(|a, b| {
        parse_node_version(&b.1)
            .cmp(&parse_node_version(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    found.first().cloned()
}

// ── 托管 Node 安装 ────────────────────────────────────────

fn platform_suffix() -> Option<&'static str> {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => match arch {
            "aarch64" => Some("darwin-arm64"),
            "x86_64" => Some("darwin-x64"),
            _ => None,
        },
        "windows" if arch == "x86_64" => Some("win-x64"),
        "linux" => match arch {
            "aarch64" => Some("linux-arm64"),
            "x86_64" => Some("linux-x64"),
            _ => None,
        },
        _ => None,
    }
}

/// 查询 nodejs.org dist index.json 最新 LTS v24。
fn latest_lts_24() -> Result<String, String> {
    let body = download("https://nodejs.org/dist/index.json", 120_000, &[], &|_| {})
        .map_err(|e| format!("查询 nodejs.org 失败:{e}"))?;
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&body).map_err(|e| format!("index.json 解析失败:{e}"))?;
    let mut v24: Vec<[u32; 3]> = list
        .iter()
        .filter_map(|e| {
            let v = e.get("version")?.as_str()?;
            if !v.starts_with("v24.") {
                return None;
            }
            let p = parse_node_version(v)?;
            e.get("lts")
                .and_then(|l| l.as_bool())
                .and_then(|b| if b { Some(p) } else { None })
        })
        .collect();
    v24.sort();
    v24.last()
        .map(|p| format!("v{}.{}.{}", p[0], p[1], p[2]))
        .ok_or_else(|| "nodejs.org index 中无 v24 LTS".into())
}

/// 下载到内存(小文件:index.json / SHASUMS256.txt)。
fn download(
    url: &str,
    timeout_ms: u64,
    _headers: &[(&str, &str)],
    _on_progress: &dyn Fn(u64),
) -> Result<Vec<u8>, String> {
    let res = ureq::get(url)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .call()
        .map_err(|e| format!("HTTP 请求失败:{e}"))?;
    let mut buf = Vec::new();
    let mut reader = res.into_reader();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取响应失败:{e}"))?;
    Ok(buf)
}

/// 下载大文件到路径(带进度回调)。
fn download_to_file(
    url: &str,
    dest: &Path,
    timeout_ms: u64,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), String> {
    use std::io::Write;
    let _ = (timeout_ms, on_progress);
    let res = ureq::get(url)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .call()
        .map_err(|e| format!("HTTP 请求失败:{e}"))?;
    let total = res
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut received: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    let mut out = std::fs::File::create(dest).map_err(|e| format!("创建文件失败:{e}"))?;
    let mut reader = res.into_reader();
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取下载流失败:{e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("写入文件失败:{e}"))?;
        received += n as u64;
        if total > 0 {
            on_progress(received, total);
        }
    }
    Ok(())
}

/// 校验下载文件 SHA256(对照官方 SHASUMS256.txt)。
fn verify_sha256(file: &Path, version: &str, suffix: &str) -> Result<(), String> {
    let shasums = download(
        &format!("https://nodejs.org/dist/{version}/SHASUMS256.txt"),
        120_000,
        &[],
        &|_| {},
    )
    .map_err(|e| format!("下载 SHASUMS256.txt 失败:{e}"))?;
    let target = format!("node-{version}-{suffix}");
    let expected = String::from_utf8_lossy(&shasums)
        .lines()
        .find_map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            if name == target
                || name == format!("{target}.zip")
                || name == format!("{target}.tar.gz")
            {
                Some(hash)
            } else {
                None
            }
        })
        .ok_or_else(|| format!("SHASUMS256.txt 中未找到 {target}"))?;
    use sha2::Digest;
    let mut f = std::fs::File::open(file).map_err(|e| format!("打开下载文件失败:{e}"))?;
    let mut ctx = <sha2::Sha256 as sha2::Digest>::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取失败:{e}"))?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    let actual = format!("{:x}", ctx.finalize());
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "SHA256 校验失败:期望 {expected},实际 {actual}(已清理,请重试)"
        ));
    }
    Ok(())
}

/// 安装托管 Node 24 LTS 到 state_dir/node/<vX.Y.Z>/。
pub fn install_node(log: &LogHub, on_stage: &dyn Fn(&str)) -> Result<(PathBuf, String), String> {
    let suffix = platform_suffix().ok_or_else(|| {
        format!(
            "不支持的平台 {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    on_stage("查询 Node 24 最新 LTS…");
    let version = latest_lts_24()?;
    let base = state_dir().join("node");
    let target_dir = base.join(&version);
    let bin_path = if cfg!(windows) {
        target_dir.join("node.exe")
    } else {
        target_dir.join("bin/node")
    };
    if bin_path.is_file() {
        log.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            &format!("Node {version} 已存在({})", bin_path.display()),
        );
        return Ok((bin_path, version));
    }

    let is_win = suffix.starts_with("win");
    let file_name = if is_win {
        format!("node-{version}-{suffix}.zip")
    } else {
        format!("node-{version}-{suffix}.tar.gz")
    };
    let url = format!("https://nodejs.org/dist/{version}/{file_name}");
    let tmp_dir = base.join(format!(".tmp-{version}"));
    let tmp_file = base.join(&file_name);
    let inner = tmp_dir.join(format!("node-{version}-{suffix}"));

    log.append(
        "launcher",
        crate::contract::LogLevel::Info,
        &format!("将安装 Node {version}(官方 LTS,平台 {suffix})"),
    );

    let cleanup = |tmp_dir: &Path, tmp_file: &Path, target_dir: &Path| {
        let _ = std::fs::remove_dir_all(tmp_dir);
        let _ = std::fs::remove_file(tmp_file);
        let _ = std::fs::remove_dir_all(target_dir);
    };
    let result = (|| -> Result<(PathBuf, String), String> {
        std::fs::create_dir_all(&base).map_err(|e| format!("创建托管目录失败:{e}"))?;
        cleanup(&tmp_dir, &tmp_file, &target_dir);

        // 1. 下载(带进度)
        on_stage("下载 Node 二进制…");
        download_to_file(&url, &tmp_file, 600_000, &|received, total| {
            let pct = (received as f64 / total as f64 * 100.0) as u64;
            on_stage(&format!("下载 Node 二进制… {pct}%"));
            if pct % 20 == 0 {
                log.append(
                    "launcher",
                    crate::contract::LogLevel::Info,
                    &format!("下载中 {pct}%(共 {}MB)", total / 1048576),
                );
            }
        })
        .map_err(|e| format!("下载 Node 失败:{e}"))?;

        // 2. SHA256 校验
        on_stage("校验 SHA256…");
        verify_sha256(&tmp_file, &version, suffix)?;

        // 3. 解压
        on_stage("解压 Node…");
        std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败:{e}"))?;
        if is_win {
            unzip_to(&tmp_file, &tmp_dir)?;
        } else {
            let status = std::process::Command::new("tar")
                .args(["-xzf"])
                .arg(&tmp_file)
                .args(["-C"])
                .arg(&tmp_dir)
                .status()
                .map_err(|e| format!("无法调用 tar:{e}"))?;
            if !status.success() {
                return Err(format!("tar 解压失败(码 {})", status.code().unwrap_or(-1)));
            }
        }

        // 4. 移入版本目录
        let src_bin = if is_win {
            inner.join("node.exe")
        } else {
            inner.join("bin/node")
        };
        if !src_bin.is_file() {
            return Err(format!(
                "解压产物缺少 node 可执行文件({})",
                src_bin.display()
            ));
        }
        std::fs::rename(&inner, &target_dir).map_err(|e| format!("移动版本目录失败:{e}"))?;

        // 5. 校验
        let v = probe_version(&bin_path).ok_or_else(|| "安装后的 Node 无法运行".to_string())?;
        if !node_in_range(&v) {
            return Err(format!("安装后的 Node 版本异常({v}),已清理"));
        }
        let _ = std::fs::remove_file(&tmp_file);
        let _ = std::fs::remove_dir_all(&tmp_dir);
        log.append(
            "launcher",
            crate::contract::LogLevel::Ok,
            &format!("Node {v} 安装完成 → {}", bin_path.display()),
        );
        Ok((bin_path, v))
    })();
    if result.is_err() {
        cleanup(&tmp_dir, &tmp_file, &target_dir);
    }
    result
}

/// zip 解压(Windows;Node 官方 zip 无嵌套目录外的文件)。
#[cfg(windows)]
fn unzip_to(zip: &Path, dest: &Path) -> Result<(), String> {
    let _ = (zip, dest);
    Err("Windows zip 解压需 zip crate(M4 Windows CI 上验证)".into())
}

#[cfg(not(windows))]
fn unzip_to(_zip: &Path, _dest: &Path) -> Result<(), String> {
    Err("unreachable on unix".into())
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
