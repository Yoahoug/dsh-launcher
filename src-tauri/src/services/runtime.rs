// dsh-launcher · RuntimeService:node/pnpm/git 解析 + 托管 Node 24 安装
// App 自身不依赖 Node;本模块只负责为 dsh 子进程准备兼容 Node(^22.19 || >=24)。
use crate::config::state_dir;
use crate::log_hub::LogHub;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        // Git for Windows(系统安装 + 每用户安装 + 32 位)
        for base in [
            std::env::var("ProgramFiles").map(PathBuf::from).ok(),
            std::env::var("ProgramFiles(x86)").map(PathBuf::from).ok(),
            std::env::var("LOCALAPPDATA")
                .map(|d| PathBuf::from(&d).join("Programs"))
                .ok(),
        ]
        .into_iter()
        .flatten()
        {
            dirs.push(base.join("Git").join("cmd"));
            dirs.push(base.join("Git").join("bin"));
            dirs.push(base.join("Git").join("mingw64").join("bin"));
            dirs.push(base.join("Git").join("mingw32").join("bin"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(&pf).join("nodejs"));
            // scoop:每用户安装到 %USERPROFILE%\scoop\shims
            dirs.push(home().join("scoop").join("shims"));
            dirs.push(
                home()
                    .join("scoop")
                    .join("apps")
                    .join("git")
                    .join("current")
                    .join("cmd"),
            );
        }
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&la).join("fnm_multisets"));
            dirs.push(PathBuf::from(&la).join("Volta").join("bin"));
            dirs.push(PathBuf::from(&la).join("Programs").join("nodejs"));
            dirs.push(PathBuf::from(&la).join("pnpm"));
        }
        if let Ok(cd) = std::env::var("ChocolateyInstall") {
            dirs.push(PathBuf::from(&cd).join("bin"));
        }
    }
    dirs.push(home().join(".local/share/pnpm"));
    dirs
}

/// Windows PATHEXT 扩展名列表(小写,去重;找不到变量时用常见默认)。
#[cfg(windows)]
fn pathext_extensions() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let mut seen = std::collections::HashSet::new();
    raw.split(';')
        .filter(|e| !e.trim().is_empty())
        .map(|e| e.trim().to_lowercase())
        .filter(|e| seen.insert(e.clone()))
        .collect()
}

/// 一个可执行名在 Windows 下对应的候选文件名(git → git, git.exe, git.bat, git.cmd…)，
/// 非 Windows 只返回原名。
fn candidate_names(name: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let mut out = vec![name.to_string()];
        let lower = name.to_lowercase();
        let has_ext = [".exe", ".cmd", ".bat", ".com"]
            .iter()
            .any(|e| lower.ends_with(e));
        if !has_ext {
            for e in pathext_extensions() {
                if e.starts_with('.') {
                    out.push(format!("{name}{e}"));
                } else {
                    out.push(format!("{name}.{e}"));
                }
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec![name.to_string()]
    }
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
    let names = candidate_names(name);
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(path_sep()).filter(|d| !d.is_empty()) {
            for n in &names {
                let cand = Path::new(dir).join(n);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    for dir in known_dirs() {
        for n in &names {
            let cand = dir.join(n);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // node.exe 等显式带 .exe 的名字由 resolve_dsh_node 的版本目录扫描处理
    if !name.to_lowercase().ends_with(".exe") {
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
    let mut v = vec![
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
    ];
    if cfg!(windows) {
        // nvm-windows:%APPDATA%\nvm\<version>\node.exe
        if let Ok(ap) = std::env::var("APPDATA") {
            v.push(PathBuf::from(&ap).join("nvm"));
        }
        // volta:%LOCALAPPDATA%\Volta\tools\image\node\<version>\bin\node.exe
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            v.push(
                PathBuf::from(&la)
                    .join("Volta")
                    .join("tools")
                    .join("image")
                    .join("node"),
            );
            // fnm(windows 布局:%LOCALAPPDATA%\fnm_multisets\<version>)
            v.push(PathBuf::from(&la).join("fnm_multisets"));
            v.push(PathBuf::from(&la).join("fnm").join("node-versions"));
        }
    }
    v
}

/// 探测 node 版本(node --version),超时/失败返回 None。
pub fn probe_version(bin: &Path) -> Option<String> {
    probe_version_with(bin, &std::collections::HashMap::new())
}

/// 探测版本(带注入环境变量;在隔离目录运行,避免命中仓库 packageManager 语义)。
/// 供 pnpm 等需要注入 PATH 的组件使用(托管 shim 依赖托管 node)。
pub fn probe_version_with(
    bin: &Path,
    env: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let (ok, out) = run_captured(bin, &["--version"], env, PROBE_TIMEOUT)?;
    if !ok {
        return None;
    }
    let v = out.trim().to_string();
    if parse_node_version(&v).is_some() {
        Some(v)
    } else {
        None
    }
}

/// 探测类子进程最长等待(防止 shim/杀软扫描导致环境检测永久阻塞)。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

/// 带超时的子进程捕获(探测用):参数数组 + 注入环境 + 超时即杀。
/// 返回 (退出是否成功, stdout 全文)。超时/无法启动返回 None。
pub fn run_captured(
    bin: &Path,
    args: &[&str],
    env: &std::collections::HashMap<String, String>,
    timeout: Duration,
) -> Option<(bool, String)> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    cmd.envs(env.clone());
    cmd.current_dir(std::env::temp_dir()); // 隔离目录:避免命中仓库 packageManager 语义
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 必须用 CREATE_NO_WINDOW(0x08000000):0x8 是 DETACHED_PROCESS,会让
        // cmd.exe 执行的 .cmd(如 pnpm.cmd)子进程输出全部丢失(探测到空版本)。
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW:探测进程不闪黑窗
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                // 超时:杀进程树后放弃(探测值 None → 视为不可用)
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    };
    let mut out = String::new();
    if let Some(mut so) = child.stdout.take() {
        use std::io::Read;
        let _ = so.read_to_string(&mut out);
    }
    Some((status.success(), out))
}

/// 系统存在但不在 dsh 范围(^22.19 || >=24)的 Node;返回 (bin, 版本)。
/// 托管目录中的 Node 不在此列(由 resolve_dsh_node 优先处理)。
pub fn incompatible_node() -> Option<(PathBuf, String)> {
    let bin = resolve_executable("node")?;
    if crate::toolchain::is_managed_path(&bin) {
        return None;
    }
    let v = probe_version(&bin)?;
    if node_in_range(&v) {
        None
    } else {
        Some((bin, v))
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
            // 兼容三种目录布局:
            //   <ver>/bin/node(.exe)            — unix 风格 / volta / fnm
            //   <ver>/installation/bin/node(.exe) — scoop 布局
            //   <ver>/node(.exe)                — 托管目录 / nvm-windows 直接布局
            for rel in [
                format!("bin/{}", node_bin_name()),
                format!("installation/bin/{}", node_bin_name()),
                node_bin_name().to_string(),
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
