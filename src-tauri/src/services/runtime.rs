// dsh-launcher · RuntimeService:node/pnpm/git 解析 + 托管 Node 24 安装
// App 自身不依赖 Node;本模块只负责为 dsh 子进程准备兼容 Node(^22.19 || >=24)。
use crate::config::state_dir;
use crate::log_hub::LogHub;
use crate::ops::{CancellationToken, OperationError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

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

/// Tauri resources 中的正式 Harness 根目录名。
pub const PACKAGED_HARNESS_RESOURCE: &str = "harness";
const BUNDLE_MANIFEST_FILE: &str = "bundle-manifest.json";
const RUNTIME_MANIFEST_FILE: &str = "manifest.json";
const RUNTIME_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub schema: u32,
    pub bundle_hash: String,
    pub source_version: Option<String>,
    pub generated_at: String,
    pub files: Vec<BundleFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub schema: u32,
    pub bundle_hash: String,
    pub harness_root: String,
    pub cli_entry: String,
    pub node_binary: String,
    pub pnpm_binary: String,
    pub dsh_home: String,
    pub dependencies_ready: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct PackagedRuntime {
    pub manifest: RuntimeManifest,
    pub harness_root: PathBuf,
    pub cli_entry: PathBuf,
    pub node_binary: PathBuf,
    pub pnpm_binary: PathBuf,
    pub dsh_home: PathBuf,
    pub tools: Tools,
}

impl Tools {
    pub fn empty() -> Self {
        Self {
            pnpm: None,
            git: None,
            dsh_node_dir: None,
        }
    }
}

fn runtime_root() -> PathBuf {
    state_dir().join("runtime")
}

fn runtime_manifest_path() -> PathBuf {
    runtime_root().join(RUNTIME_MANIFEST_FILE)
}

fn harness_versions_dir() -> PathBuf {
    state_dir().join("harness-versions")
}

fn required_bundle_files() -> [&'static str; 6] {
    [
        "apps/cli/lib/bin.js",
        "apps/cli/package.json",
        "apps/web/dist/index.html",
        "apps/web/package.json",
        "package.json",
        "pnpm-lock.yaml",
    ]
}

fn safe_bundle_path(path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute()
        && !p
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 读取并校验安装包随附的 bundle manifest。
/// 这里不扫描所有内容的 SHA-256，避免每次启动重新遍历大型 web dist；首次 bundle
/// 生成时已记录每个文件的校验值，运行时仍验证结构、路径、大小和关键入口存在。
pub fn load_bundle_manifest(bundle_root: &Path) -> Result<BundleManifest, String> {
    let path = bundle_root.join(BUNDLE_MANIFEST_FILE);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("安装包缺少 Harness manifest:{} ({e})", path.display()))?;
    let manifest: BundleManifest = serde_json::from_str(&raw)
        .map_err(|e| format!("Harness bundle manifest 无法解析:{} ({e})", path.display()))?;
    if manifest.schema != RUNTIME_SCHEMA {
        return Err(format!(
            "Harness bundle manifest schema 不兼容:{}",
            manifest.schema
        ));
    }
    if manifest.bundle_hash.len() != 64
        || !manifest.bundle_hash.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Harness bundle manifest 的 bundleHash 非法".into());
    }
    if manifest.files.is_empty() {
        return Err("Harness bundle manifest 未列出正式运行文件".into());
    }
    for file in &manifest.files {
        if !safe_bundle_path(&file.path) || file.sha256.len() != 64 {
            return Err(format!(
                "Harness bundle manifest 文件路径或 SHA-256 非法:{}",
                file.path
            ));
        }
        let path = bundle_root.join(&file.path);
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("Harness bundle 文件缺失:{} ({e})", path.display()))?;
        if !metadata.is_file() || metadata.len() != file.size {
            return Err(format!("Harness bundle 文件大小不匹配:{}", path.display()));
        }
    }
    for required in required_bundle_files() {
        if !bundle_root.join(required).is_file() {
            return Err(format!("安装包缺少 Harness 运行入口:{required}"));
        }
    }
    if !bundle_root.join("apps/web/dist").is_dir() {
        return Err("安装包缺少 apps/web/dist/".into());
    }
    Ok(manifest)
}

fn runtime_manifest_from_file() -> Result<Option<RuntimeManifest>, String> {
    let path = runtime_manifest_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&raw).ok())
}

/// 校验运行时 manifest；`node_version` 由调用方在需要时探测，便于纯函数单测。
pub fn validate_runtime_manifest(
    manifest: &RuntimeManifest,
    bundle: &BundleManifest,
    node_version: Option<&str>,
) -> Result<(), String> {
    if manifest.schema != RUNTIME_SCHEMA {
        return Err(format!("运行时 manifest schema 不兼容:{}", manifest.schema));
    }
    if manifest.bundle_hash != bundle.bundle_hash {
        return Err("运行时 manifest bundleHash 与安装包不匹配".into());
    }
    let harness_root = Path::new(&manifest.harness_root);
    let cli_entry = Path::new(&manifest.cli_entry);
    let node_binary = Path::new(&manifest.node_binary);
    let pnpm_binary = Path::new(&manifest.pnpm_binary);
    let dsh_home = Path::new(&manifest.dsh_home);
    if !harness_root.is_dir() {
        return Err("运行时 Harness 根目录不存在".into());
    }
    if cli_entry != harness_root.join("apps/cli/lib/bin.js") || !cli_entry.is_file() {
        return Err("运行时 CLI 入口缺失或路径不匹配".into());
    }
    if !harness_root.join("apps/web/dist").is_dir() {
        return Err("运行时 apps/web/dist 缺失".into());
    }
    if !harness_root.join("node_modules").is_dir() || !manifest.dependencies_ready {
        return Err("运行时生产依赖尚未安装".into());
    }
    if !executable_file(node_binary) {
        return Err("运行时 Node 可执行文件缺失".into());
    }
    let Some(node_version) = node_version else {
        return Err("运行时 Node 版本无法读取".into());
    };
    if !node_in_range(node_version) {
        return Err(format!(
            "运行时 Node {node_version} 不兼容,需要 {}",
            NODE_RANGE_MSG
        ));
    }
    if !executable_file(pnpm_binary) {
        return Err("运行时 pnpm 可执行文件缺失".into());
    }
    if !dsh_home.is_dir() {
        return Err("运行时 DSH_HOME 不可访问".into());
    }
    Ok(())
}

fn atomic_write_runtime_manifest(manifest: &RuntimeManifest) -> Result<(), String> {
    let dir = runtime_root();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建运行时目录失败:{e}"))?;
    let tmp = dir.join(format!("manifest.json.tmp-{}", std::process::id()));
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("序列化运行时 manifest 失败:{e}"))?;
    {
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("写入运行时 manifest 失败:{e}"))?;
        use std::io::Write;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("写入运行时 manifest 失败:{e}"))?;
        file.sync_all()
            .map_err(|e| format!("运行时 manifest fsync 失败:{e}"))?;
    }
    std::fs::rename(&tmp, runtime_manifest_path())
        .map_err(|e| format!("发布运行时 manifest 失败:{e}"))
}

fn copy_bundle_tree(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| format!("创建 Harness 目录失败:{e}"))?;
    for entry in std::fs::read_dir(source).map_err(|e| format!("读取 Harness 资源失败:{e}"))?
    {
        let entry = entry.map_err(|e| format!("读取 Harness 资源失败:{e}"))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from)
            .map_err(|e| format!("读取 Harness 资源元数据失败:{} ({e})", from.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("Harness bundle 不允许符号链接:{}", from.display()));
        }
        if metadata.is_dir() {
            copy_bundle_tree(&from, &to)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("复制 Harness 文件失败:{} ({e})", from.display()))?;
        }
    }
    Ok(())
}

fn install_production_dependencies(
    log: &Arc<LogHub>,
    pnpm: &Path,
    harness_root: &Path,
    tools: &Tools,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    token.check()?;
    let mut cmd = std::process::Command::new(pnpm);
    cmd.args(["install", "--prod", "--frozen-lockfile"]);
    cmd.current_dir(harness_root);
    cmd.envs(tools.env());
    cmd.env("NODE_ENV", "production");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    let output = cmd
        .output()
        .map_err(|e| OperationError::Failed(format!("无法启动 pnpm 生产依赖安装:{}", e)))?;
    token.check()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stdout.lines() {
        log.append("pnpm", crate::contract::LogLevel::Info, line);
    }
    for line in stderr.lines() {
        log.append("pnpm", crate::contract::LogLevel::Warn, line);
    }
    if !output.status.success() {
        let tail = stderr
            .lines()
            .chain(stdout.lines())
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(OperationError::Failed(format!(
            "pnpm 生产依赖安装失败(code={:?}){}",
            output.status.code(),
            if tail.is_empty() {
                String::new()
            } else {
                format!("\n{tail}")
            }
        )));
    }
    Ok(())
}

fn managed_dsh_home() -> PathBuf {
    state_dir().join("dsh-home")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 读取有效 manifest；有效时 normal 启动不会复制 Harness、安装工具或重装依赖。
pub fn load_valid_runtime_manifest(
    bundle: &BundleManifest,
) -> Result<Option<RuntimeManifest>, String> {
    let Some(manifest) = runtime_manifest_from_file()? else {
        return Ok(None);
    };
    let node_version = probe_version(Path::new(&manifest.node_binary));
    match validate_runtime_manifest(&manifest, bundle, node_version.as_deref()) {
        Ok(()) => Ok(Some(manifest)),
        Err(_) => Ok(None),
    }
}

/// setup 后用于选择 bootstrap 分支的轻量检查；有效时不触发 Node/pnpm/git 全量扫描。
pub fn packaged_runtime_fast_path_available(app: &AppHandle) -> bool {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return false;
    };
    let bundle_root = resource_dir.join(PACKAGED_HARNESS_RESOURCE);
    let Ok(bundle) = load_bundle_manifest(&bundle_root) else {
        return false;
    };
    matches!(load_valid_runtime_manifest(&bundle), Ok(Some(_)))
}

/// 预配正式 Harness。该函数只接受安装包资源，不读取 repo_path，也不执行本地 build。
pub fn ensure_packaged_runtime(
    app: &AppHandle,
    log: &Arc<LogHub>,
    token: &CancellationToken,
    on_stage: &dyn Fn(&str),
) -> Result<PackagedRuntime, OperationError> {
    token.check()?;
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| OperationError::Failed(format!("无法定位应用资源目录:{e}")))?;
    let bundle_root = resource_dir.join(PACKAGED_HARNESS_RESOURCE);
    let bundle = load_bundle_manifest(&bundle_root).map_err(OperationError::Failed)?;
    if let Some(manifest) = load_valid_runtime_manifest(&bundle).map_err(OperationError::Failed)? {
        let harness_root = PathBuf::from(&manifest.harness_root);
        let cli_entry = PathBuf::from(&manifest.cli_entry);
        let node_binary = PathBuf::from(&manifest.node_binary);
        let pnpm_binary = PathBuf::from(&manifest.pnpm_binary);
        let dsh_home = PathBuf::from(&manifest.dsh_home);
        let tools = Tools {
            pnpm: Some(pnpm_binary.clone()),
            git: None,
            dsh_node_dir: node_binary.parent().map(Path::to_path_buf),
        };
        log.append(
            "launcher",
            crate::contract::LogLevel::Info,
            "正式运行时 manifest 有效,跳过预配",
        );
        return Ok(PackagedRuntime {
            manifest,
            harness_root,
            cli_entry,
            node_binary,
            pnpm_binary,
            dsh_home,
            tools,
        });
    }

    on_stage("检查兼容 Node…");
    let mut tools = Tools::empty();
    if resolve_dsh_node().is_none() {
        crate::toolchain::ensure_tool(log, crate::toolchain::Tool::Node, token, on_stage, &tools)?;
    }
    let node_binary = resolve_dsh_node().map(|(path, _)| path).ok_or_else(|| {
        OperationError::Failed(format!(
            "未找到兼容 Node {},无法准备正式运行时",
            NODE_RANGE_MSG
        ))
    })?;
    tools.dsh_node_dir = node_binary.parent().map(Path::to_path_buf);

    on_stage("检查或安装 pnpm…");
    tools.pnpm = resolve_executable("pnpm");
    if crate::toolchain::resolve_pnpm(&tools).is_none() {
        crate::toolchain::ensure_tool(log, crate::toolchain::Tool::Pnpm, token, on_stage, &tools)?;
    }
    let pnpm_binary = crate::toolchain::resolve_pnpm(&tools)
        .or_else(|| resolve_executable("pnpm"))
        .ok_or_else(|| OperationError::Failed("未找到 pnpm,无法安装正式依赖".into()))?;
    tools.pnpm = Some(pnpm_binary.clone());

    let dsh_home = managed_dsh_home();
    std::fs::create_dir_all(&dsh_home).map_err(|e| {
        OperationError::Failed(format!(
            "创建 managed DSH_HOME 失败:{} ({e})",
            dsh_home.display()
        ))
    })?;
    let versions = harness_versions_dir();
    std::fs::create_dir_all(&versions)
        .map_err(|e| OperationError::Failed(format!("创建 Harness 版本目录失败:{e}")))?;
    let staging = versions.join(format!(
        ".provision-{}-{}",
        bundle.bundle_hash,
        std::process::id()
    ));
    let harness_root = if versions.join(&bundle.bundle_hash).exists() {
        versions.join(format!("{}-{}", bundle.bundle_hash, now_ms()))
    } else {
        versions.join(&bundle.bundle_hash)
    };
    let _ = std::fs::remove_dir_all(&staging);
    copy_bundle_tree(&bundle_root, &staging).map_err(OperationError::Failed)?;
    on_stage("安装生产依赖…");
    install_production_dependencies(log, &pnpm_binary, &staging, &tools, token)?;
    token.check()?;
    std::fs::rename(&staging, &harness_root)
        .map_err(|e| OperationError::Failed(format!("发布 Harness 版本目录失败:{e}")))?;

    let manifest = RuntimeManifest {
        schema: RUNTIME_SCHEMA,
        bundle_hash: bundle.bundle_hash.clone(),
        harness_root: harness_root.display().to_string(),
        cli_entry: harness_root
            .join("apps/cli/lib/bin.js")
            .display()
            .to_string(),
        node_binary: node_binary.display().to_string(),
        pnpm_binary: pnpm_binary.display().to_string(),
        dsh_home: dsh_home.display().to_string(),
        dependencies_ready: true,
        created_at: now_ms().to_string(),
    };
    validate_runtime_manifest(&manifest, &bundle, probe_version(&node_binary).as_deref())
        .map_err(OperationError::Failed)?;
    atomic_write_runtime_manifest(&manifest).map_err(OperationError::Failed)?;
    let cli_entry = PathBuf::from(&manifest.cli_entry);
    log.append(
        "launcher",
        crate::contract::LogLevel::Ok,
        &format!("正式 Harness 预配完成 → {}", harness_root.display()),
    );
    Ok(PackagedRuntime {
        manifest,
        harness_root,
        cli_entry,
        node_binary,
        pnpm_binary,
        dsh_home,
        tools,
    })
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

    fn manifest_fixture() -> (BundleManifest, RuntimeManifest, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "dsh-runtime-manifest-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("harness");
        std::fs::create_dir_all(root.join("apps/cli/lib")).unwrap();
        std::fs::create_dir_all(root.join("apps/web/dist")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(base.join("dsh-home")).unwrap();
        std::fs::write(root.join("apps/cli/lib/bin.js"), b"fixture").unwrap();
        std::fs::write(base.join("node"), b"node").unwrap();
        std::fs::write(base.join("pnpm"), b"pnpm").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(base.join("node"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            std::fs::set_permissions(base.join("pnpm"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let bundle = BundleManifest {
            schema: 1,
            bundle_hash: "a".repeat(64),
            source_version: Some("fixture".into()),
            generated_at: "now".into(),
            files: Vec::new(),
        };
        let manifest = RuntimeManifest {
            schema: 1,
            bundle_hash: bundle.bundle_hash.clone(),
            harness_root: root.display().to_string(),
            cli_entry: root.join("apps/cli/lib/bin.js").display().to_string(),
            node_binary: base.join("node").display().to_string(),
            pnpm_binary: base.join("pnpm").display().to_string(),
            dsh_home: base.join("dsh-home").display().to_string(),
            dependencies_ready: true,
            created_at: "now".into(),
        };
        (bundle, manifest, base)
    }

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

    #[test]
    fn runtime_manifest_missing_requires_provision() {
        let _g = crate::test_lock::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("dsh-runtime-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::env::set_var("DSH_LAUNCHER_STATE_DIR", &base);
        let (bundle, _, _) = manifest_fixture();
        assert!(load_valid_runtime_manifest(&bundle).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn runtime_manifest_hash_mismatch_is_rejected() {
        let (bundle, mut manifest, base) = manifest_fixture();
        manifest.bundle_hash = "b".repeat(64);
        let error = validate_runtime_manifest(&manifest, &bundle, Some("v24.0.0")).unwrap_err();
        assert!(error.contains("bundleHash"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn runtime_manifest_missing_cli_is_rejected() {
        let (bundle, manifest, base) = manifest_fixture();
        std::fs::remove_file(&manifest.cli_entry).unwrap();
        let error = validate_runtime_manifest(&manifest, &bundle, Some("v24.0.0")).unwrap_err();
        assert!(error.contains("CLI"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn runtime_manifest_missing_web_dist_is_rejected() {
        let (bundle, manifest, base) = manifest_fixture();
        std::fs::remove_dir_all(Path::new(&manifest.harness_root).join("apps/web/dist")).unwrap();
        let error = validate_runtime_manifest(&manifest, &bundle, Some("v24.0.0")).unwrap_err();
        assert!(error.contains("apps/web/dist"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn runtime_manifest_incompatible_node_is_rejected() {
        let (bundle, manifest, base) = manifest_fixture();
        let error = validate_runtime_manifest(&manifest, &bundle, Some("v23.1.0")).unwrap_err();
        assert!(error.contains("不兼容"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn runtime_manifest_missing_pnpm_is_rejected() {
        let (bundle, manifest, base) = manifest_fixture();
        std::fs::remove_file(&manifest.pnpm_binary).unwrap();
        let error = validate_runtime_manifest(&manifest, &bundle, Some("v24.0.0")).unwrap_err();
        assert!(error.contains("pnpm"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn valid_runtime_manifest_skips_provision_checks() {
        let (bundle, manifest, base) = manifest_fixture();
        validate_runtime_manifest(&manifest, &bundle, Some("v24.0.0")).unwrap();
        assert!(safe_bundle_path("apps/cli/lib/bin.js"));
        assert!(!safe_bundle_path("../outside"));
        let _ = std::fs::remove_dir_all(base);
    }
}
