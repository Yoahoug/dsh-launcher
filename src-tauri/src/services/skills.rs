// dsh-launcher · SkillsService:技能根目录扫描 + managed 技能 CRUD + 导入 + 预览
//
// 解析规则与 dsh `skill-filesystem` 一致(见 deepseek-harness packages/skill/skill-filesystem):
// - 接受目录包 `<kebab>/SKILL.md` 与扁平 `<kebab>.md`;
// - frontmatter 必须含 `name` + `description`;可选 `whenToUse` / `disable-model-invocation` /
//   `user-invocable`;废弃键(disableModelInvocation/modelInvocable/userInvocable)拒绝;
// - 非法条目跳过并日志点名;managed 根跳过 `.system`;
// - 写操作只允许落在 managed 根(默认 $DSH_HOME/skills,可在设置改 skillManagedRoot);
//   `skills_delete` 对任何外部路径一律拒绝(路径围栏 + canonicalize 校验);
// - 扫描不跟随符号链接出目录;预览正文限制大小(256 KB)。
use crate::contract::{SkillRoot, SkillSource, SkillSummary, SkillsSnapshot};
use crate::log_hub::LogHub;
use crate::services::plugins;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 预览正文上限。
pub const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

fn home() -> PathBuf {
    let h = crate::config::home_dir();
    if h.is_empty() {
        PathBuf::from("~")
    } else {
        PathBuf::from(h)
    }
}

fn expand_home(p: &str) -> PathBuf {
    let h = home();
    if p == "~" {
        h
    } else if let Some(rest) = p.strip_prefix("~/") {
        h.join(rest)
    } else {
        PathBuf::from(p)
    }
}

/// 扫描上下文(设置 + 仓库路径)。
#[derive(Debug, Clone)]
pub struct ScanCtx {
    pub repo_path: String,
    pub dsh_home_setting: String,
    pub skill_managed_root_setting: String,
    pub external_skill_roots: Vec<String>,
}

impl ScanCtx {
    /// managed 根:设置 skillManagedRoot 优先,否则 $DSH_HOME/skills。
    pub fn managed_root(&self) -> PathBuf {
        if self.skill_managed_root_setting.is_empty() {
            plugins::dsh_home_dir(&self.dsh_home_setting).join("skills")
        } else {
            PathBuf::from(&self.skill_managed_root_setting)
        }
    }
}

fn push_root(out: &mut Vec<SkillRoot>, key: &str, label: &str, path: PathBuf, managed: bool) {
    let exists = path.is_dir();
    out.push(SkillRoot {
        key: key.to_string(),
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        exists,
        managed,
        enabled: false,
    });
}

fn is_cursor_system_root(path: &str) -> bool {
    expand_home(path).starts_with(expand_home("~/.cursor"))
}

/// 读取 profile patch + home patch 中 skill-filesystem 行的 customSkillDirs(一键启用状态)。
fn custom_skill_dirs(dsh_home: &Path, profile: &str) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for p in [
        plugins::profile_patch_path(dsh_home, profile),
        plugins::home_patch_path(dsh_home),
    ] {
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let doc = plugins::split_patch_doc(&text);
        for e in doc.entries {
            if e.id != "skill-filesystem" {
                continue;
            }
            if let Some(v) = plugins::extract_config(&e.block, e.block.contains("!!js")) {
                if let Some(arr) = v.get("customSkillDirs").and_then(|x| x.as_array()) {
                    for d in arr {
                        if let Some(s) = d.as_str() {
                            dirs.push(s.to_string());
                        }
                    }
                }
            }
        }
    }
    dirs
}

/// 默认根映射 + 设置追加(§5.4)。Cursor 系统技能不纳入 launcher 扫描。
pub fn root_entries(ctx: &ScanCtx) -> Vec<SkillRoot> {
    let mut out = Vec::new();
    push_root(
        &mut out,
        "managed",
        "已管理 · $DSH_HOME/skills",
        ctx.managed_root(),
        true,
    );
    push_root(
        &mut out,
        "codex",
        "Codex",
        expand_home("~/.codex/skills"),
        false,
    );
    push_root(
        &mut out,
        "claude",
        "Claude Code",
        expand_home("~/.claude/skills"),
        false,
    );
    push_root(
        &mut out,
        "opencode",
        "OpenCode",
        expand_home("~/.config/opencode/skills"),
        false,
    );
    push_root(
        &mut out,
        "agents",
        "Agents",
        expand_home("~/.agents/skills"),
        false,
    );
    // 项目根(只读展示)
    for (rel, label) in [
        (".dsh/skills", "项目 · .dsh/skills"),
        (".agents/skills", "项目 · .agents/skills"),
    ] {
        push_root(
            &mut out,
            "project",
            label,
            Path::new(&ctx.repo_path).join(rel),
            false,
        );
    }
    // 自定义根(设置追加)
    for r in &ctx.external_skill_roots {
        if is_cursor_system_root(r) {
            continue;
        }
        let p = PathBuf::from(r);
        push_root(&mut out, "custom", &format!("自定义 · {r}"), p, false);
    }
    out
}

// ── frontmatter 解析(与 skill-filesystem 规则一致) ───────

/// 拆 frontmatter:返回 (yaml 头, 正文)。
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let first_nl = raw.find('\n')?;
    if raw[..first_nl].trim_end_matches('\r') != "---" {
        return None;
    }
    let mut pos = first_nl + 1;
    loop {
        let next_nl = raw[pos..].find('\n');
        let line_end = match next_nl {
            Some(n) => pos + n,
            None => raw.len(),
        };
        let line = &raw[pos..line_end];
        if line.trim_end_matches('\r') == "---" {
            let body_start = if line_end < raw.len() {
                line_end + 1
            } else {
                line_end
            };
            return Some((&raw[first_nl + 1..pos], &raw[body_start..]));
        }
        next_nl?;
        pos = line_end + 1;
    }
}

/// 与 skill-filesystem 的 frontmatterBoolean 规则一致。
fn yaml_bool(v: &serde_json::Value) -> Result<bool, String> {
    match v {
        serde_json::Value::Bool(b) => Ok(*b),
        serde_json::Value::Number(n) if n.as_i64() == Some(1) => Ok(true),
        serde_json::Value::Number(n) if n.as_i64() == Some(0) => Ok(false),
        serde_json::Value::String(s) => match s.to_lowercase().as_str() {
            "true" | "yes" | "on" => Ok(true),
            "false" | "no" | "off" => Ok(false),
            _ => Err(format!("{s} 不是布尔值")),
        },
        _ => Err("必须是布尔值".into()),
    }
}

fn kebab_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap())
}

/// 严格 kebab-case 校验(`^[a-z0-9]+(?:-[a-z0-9]+)*$`,与 dsh isSkillName 一致)。
pub fn is_kebab(name: &str) -> bool {
    name.len() <= 64 && kebab_re().is_match(name)
}

struct ParsedSkill {
    name: String,
    description: String,
    when_to_use: Option<String>,
    model_invocable: bool,
    user_invocable: bool,
}

/// 解析单个技能文件;失败返回可读原因(跳过日志点名用)。
fn parse_skill_file(path: &Path) -> Result<ParsedSkill, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取失败:{e}"))?;
    let (yaml, _body) = split_frontmatter(&raw).ok_or("缺少 YAML frontmatter(必须以 --- 开头)")?;
    let data: serde_json::Value =
        serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter YAML 非法:{e}"))?;
    let obj = data.as_object().ok_or("frontmatter 必须是映射")?;

    // 废弃键拒绝(与 dsh 一致)
    for legacy in ["disableModelInvocation", "modelInvocable", "userInvocable"] {
        if obj.contains_key(legacy) {
            return Err(format!(
                "frontmatter 字段 {legacy} 已废弃,请用 disable-model-invocation / user-invocable"
            ));
        }
    }
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("frontmatter 缺少 name")?
        .to_string();
    if !is_kebab(&name) {
        return Err(format!("名称 {name} 非 kebab-case"));
    }
    let description = obj
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("frontmatter 缺少 description")?
        .to_string();
    let when_to_use = obj
        .get("whenToUse")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from);
    let mut model_invocable = true;
    let mut user_invocable = true;
    if let Some(v) = obj.get("disable-model-invocation") {
        model_invocable = !yaml_bool(v).map_err(|e| format!("disable-model-invocation {e}"))?;
    }
    if let Some(v) = obj.get("user-invocable") {
        user_invocable = yaml_bool(v).map_err(|e| format!("user-invocable {e}"))?;
    }
    Ok(ParsedSkill {
        name,
        description,
        when_to_use,
        model_invocable,
        user_invocable,
    })
}

/// 技能目录是否在根内(canonicalize 围栏,防符号链接逃逸)。
fn within_root(candidate: &Path, root: &Path) -> bool {
    let c = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf());
    let r = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    c.starts_with(&r)
}

fn make_summary(
    source: SkillSource,
    dir: &Path,
    path: &Path,
    parsed: &ParsedSkill,
    has_scripts: bool,
) -> SkillSummary {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    SkillSummary {
        name: parsed.name.clone(),
        description: parsed.description.clone(),
        when_to_use: parsed.when_to_use.clone(),
        model_invocable: parsed.model_invocable,
        user_invocable: parsed.user_invocable,
        source,
        dir: dir.to_string_lossy().to_string(),
        path: path.to_string_lossy().to_string(),
        size_bytes: size,
        has_scripts,
    }
}

fn scan_root(
    root: &SkillRoot,
    source: SkillSource,
    log: &Arc<LogHub>,
    skills: &mut Vec<SkillSummary>,
    skipped: &mut Vec<String>,
) {
    let root_path = Path::new(&root.path);
    let Ok(entries) = std::fs::read_dir(root_path) else {
        return;
    };
    let mut names: Vec<std::fs::DirEntry> = entries.flatten().collect();
    names.sort_by_key(|e| e.file_name());
    let mut record_skip = |reason: String| {
        log.append(
            "launcher",
            crate::contract::LogLevel::Warn,
            &format!("技能扫描跳过:{reason}(根:{})", root.path),
        );
        skipped.push(reason);
    };
    for entry in names {
        let name = entry.file_name().to_string_lossy().to_string();
        if root.managed && name == ".system" {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                record_skip(format!("{name}:无法读取条目:{e}"));
                continue;
            }
        };
        if ft.is_dir() {
            let skill_dir = entry.path();
            if !is_kebab(&name) {
                record_skip(format!("{name}:目录名非 kebab-case,跳过"));
                continue;
            }
            if !within_root(&skill_dir, root_path) {
                record_skip(format!("{name}:符号链接指向根外,跳过"));
                continue;
            }
            let md = skill_dir.join("SKILL.md");
            if !md.is_file() {
                record_skip(format!("{name}:目录包缺少 SKILL.md"));
                continue;
            }
            match parse_skill_file(&md) {
                Ok(parsed) => {
                    if parsed.name != name {
                        record_skip(format!(
                            "{name}:frontmatter 名称({})与目录名不一致",
                            parsed.name
                        ));
                        continue;
                    }
                    let has_scripts = std::fs::read_dir(&skill_dir)
                        .map(|rd| {
                            rd.flatten()
                                .any(|e| e.file_name().to_string_lossy() != "SKILL.md")
                        })
                        .unwrap_or(false);
                    skills.push(make_summary(source, &skill_dir, &md, &parsed, has_scripts));
                }
                Err(e) => record_skip(format!("{name}:{e}")),
            }
        } else if ft.is_file() && name.ends_with(".md") {
            let stem = name.trim_end_matches(".md");
            if !is_kebab(stem) {
                record_skip(format!("{name}:文件名非 kebab-case,跳过"));
                continue;
            }
            let f = entry.path();
            match parse_skill_file(&f) {
                Ok(parsed) => {
                    skills.push(make_summary(source, root_path, &f, &parsed, false));
                }
                Err(e) => record_skip(format!("{name}:{e}")),
            }
        }
    }
}

/// 全量扫描(§5.4)。
pub fn scan(ctx: &ScanCtx, log: &Arc<LogHub>) -> SkillsSnapshot {
    let roots = root_entries(ctx);
    let mut skills = Vec::new();
    let mut skipped = Vec::new();
    for root in &roots {
        if !root.exists {
            continue;
        }
        let source = match root.key.as_str() {
            "managed" => SkillSource::Managed,
            "codex" => SkillSource::Codex,
            "claude" => SkillSource::Claude,
            "cursor" => SkillSource::Cursor,
            "opencode" => SkillSource::Opencode,
            "agents" => SkillSource::Agents,
            "project" => SkillSource::Project,
            _ => SkillSource::Custom,
        };
        scan_root(root, source, log, &mut skills, &mut skipped);
    }
    skills.sort_by(|a, b| {
        let ka = format!("{:?}", a.source);
        let kb = format!("{:?}", b.source);
        ka.cmp(&kb).then_with(|| a.name.cmp(&b.name))
    });
    SkillsSnapshot {
        roots,
        skills,
        plugins_installed: false,
        skipped,
    }
}

/// 快照(含插件安装状态:目标 profile 是否装了 skill-external-roots)。
pub fn snapshot(ctx: &ScanCtx, log: &Arc<LogHub>, profile_name: &str) -> SkillsSnapshot {
    let mut snap = scan(ctx, log);
    let home = plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let profiles = plugins::profiles(&home);
    snap.plugins_installed = profiles.iter().any(|p| {
        p.name == profile_name && p.deps.contains_key("@dsh-plugins/skill-external-roots")
    });
    // 一键启用状态:外部根是否已在 skill-filesystem.customSkillDirs 中
    if !profile_name.is_empty() {
        let dirs = custom_skill_dirs(&home, profile_name);
        for root in snap.roots.iter_mut() {
            if root.managed || !root.exists {
                continue;
            }
            let canon =
                std::fs::canonicalize(&root.path).unwrap_or_else(|_| PathBuf::from(&root.path));
            root.enabled = dirs.iter().any(|d| {
                let dc = std::fs::canonicalize(d).unwrap_or_else(|_| PathBuf::from(d));
                dc == canon
            });
        }
    }
    snap
}

// ── managed CRUD(只写 managed 根) ────────────────────────

/// 定位 managed 技能:(目录包路径, SKILL.md 路径) 或 (根, 扁平 md)。
fn find_managed(root: &Path, name: &str) -> Option<(PathBuf, PathBuf)> {
    let dir_pkg = root.join(name).join("SKILL.md");
    let flat = root.join(format!("{name}.md"));
    if dir_pkg.is_file() {
        Some((root.join(name), dir_pkg))
    } else if flat.is_file() {
        Some((root.to_path_buf(), flat))
    } else {
        None
    }
}

fn frontmatter_text(
    name: &str,
    description: &str,
    when_to_use: Option<&str>,
) -> Result<String, String> {
    let mut map = serde_json::Map::new();
    map.insert("name".into(), serde_json::json!(name));
    map.insert("description".into(), serde_json::json!(description));
    if let Some(w) = when_to_use {
        map.insert("whenToUse".into(), serde_json::json!(w));
    }
    let yaml = serde_yaml_ng::to_string(&serde_json::Value::Object(map))
        .map_err(|e| format!("frontmatter 生成失败:{e}"))?;
    Ok(format!("---\n{yaml}---\n"))
}

/// 新建技能(自动生成 frontmatter;kebab + 唯一性校验)。
pub fn create(
    _log: &Arc<LogHub>,
    ctx: &ScanCtx,
    name: &str,
    description: &str,
    when_to_use: Option<&str>,
    body: &str,
) -> Result<SkillSummary, String> {
    let name = name.trim();
    if !is_kebab(name) {
        return Err(format!(
            "技能名 {name} 必须为 kebab-case(小写字母/数字/中划线)"
        ));
    }
    if description.trim().is_empty() {
        return Err("描述不能为空".into());
    }
    let root = ctx.managed_root();
    let dir_pkg = root.join(name);
    let flat = root.join(format!("{name}.md"));
    if dir_pkg.exists() || flat.exists() {
        return Err(format!("技能 {name} 已存在(managed 根)"));
    }
    std::fs::create_dir_all(&dir_pkg).map_err(|e| format!("创建技能目录失败:{e}"))?;
    let md = dir_pkg.join("SKILL.md");
    let fm = frontmatter_text(name, description.trim(), when_to_use)?;
    let content = format!("{fm}\n{}", body.trim_start());
    let size = content.len() as u64;
    std::fs::write(&md, &content).map_err(|e| format!("写入 SKILL.md 失败:{e}"))?;
    Ok(SkillSummary {
        name: name.to_string(),
        description: description.trim().to_string(),
        when_to_use: when_to_use.map(String::from),
        model_invocable: true,
        user_invocable: true,
        source: SkillSource::Managed,
        dir: dir_pkg.to_string_lossy().to_string(),
        path: md.to_string_lossy().to_string(),
        size_bytes: size,
        has_scripts: false,
    })
}

/// 更新技能(仅 managed 根;保留既有 frontmatter 其它键;body 为空时保留既有正文)。
pub fn update(
    _log: &Arc<LogHub>,
    ctx: &ScanCtx,
    name: &str,
    description: &str,
    when_to_use: Option<&str>,
    body: &str,
) -> Result<SkillSummary, String> {
    let root = ctx.managed_root();
    let Some((dir, md)) = find_managed(&root, name) else {
        return Err(format!("技能 {name} 不存在于 managed 根"));
    };
    let raw = std::fs::read_to_string(&md).map_err(|e| format!("读取失败:{e}"))?;
    let (yaml, old_body) = split_frontmatter(&raw).ok_or("缺少 frontmatter,拒绝覆盖")?;
    let data: serde_json::Value =
        serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter 非法:{e}"))?;
    let mut map = data.as_object().cloned().ok_or("frontmatter 必须是映射")?;
    map.insert("name".into(), serde_json::json!(name.trim()));
    map.insert("description".into(), serde_json::json!(description.trim()));
    match when_to_use {
        Some(w) => {
            map.insert("whenToUse".into(), serde_json::json!(w.trim()));
        }
        None => {
            map.remove("whenToUse");
        }
    }
    let yaml = serde_yaml_ng::to_string(&serde_json::Value::Object(map))
        .map_err(|e| format!("frontmatter 生成失败:{e}"))?;
    let new_body = if body.trim().is_empty() {
        old_body
    } else {
        body
    };
    let content = format!("---\n{yaml}---\n\n{}", new_body.trim_start());
    std::fs::write(&md, content).map_err(|e| format!("写入失败:{e}"))?;
    let parsed = parse_skill_file(&md).map_err(|e| format!("更新后解析失败:{e}"))?;
    let has_scripts = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy() != "SKILL.md")
        })
        .unwrap_or(false);
    Ok(make_summary(
        SkillSource::Managed,
        &dir,
        &md,
        &parsed,
        has_scripts,
    ))
}

/// 删除技能(路径围栏:仅 managed 根内;外部路径一律拒绝)。
pub fn delete(_log: &Arc<LogHub>, ctx: &ScanCtx, name: &str) -> Result<(), String> {
    let root = ctx.managed_root();
    if !root.is_dir() {
        return Err("managed 根不存在".into());
    }
    let root_canon = root
        .canonicalize()
        .map_err(|e| format!("解析 managed 根失败:{e}"))?;
    let Some((dir, _md)) = find_managed(&root, name) else {
        return Err(format!("技能 {name} 不存在于 managed 根"));
    };
    let target_canon = dir
        .canonicalize()
        .map_err(|e| format!("解析目标失败:{e}"))?;
    if !target_canon.starts_with(&root_canon) {
        return Err(format!("拒绝删除:{name} 不在 managed 根内(路径围栏)"));
    }
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除失败:{e}"))?;
    } else {
        std::fs::remove_file(&dir).map_err(|e| format!("删除失败:{e}"))?;
    }
    Ok(())
}

/// 导入外部技能:递归拷贝 SKILL.md + scripts/references 等到 managed 根。
/// 目标名冲突时返回错误(提示改名或覆盖)。
pub fn import(
    _log: &Arc<LogHub>,
    ctx: &ScanCtx,
    source_path: &str,
    name: Option<&str>,
) -> Result<SkillSummary, String> {
    let src = Path::new(source_path);
    let (src_dir, md_path, parsed) = if src.is_dir() {
        let md = src.join("SKILL.md");
        let parsed =
            parse_skill_file(&md).map_err(|e| format!("源技能非法:{e}(路径:{})", md.display()))?;
        (src.to_path_buf(), md, parsed)
    } else if src.is_file() && source_path.ends_with(".md") {
        let parsed = parse_skill_file(src).map_err(|e| format!("源技能非法:{e}"))?;
        (
            src.parent().map(PathBuf::from).unwrap_or_default(),
            src.to_path_buf(),
            parsed,
        )
    } else {
        return Err("导入源必须是技能目录(<name>/SKILL.md)或 <name>.md 文件".into());
    };
    let target_name = match name {
        Some(n) => n.trim().to_string(),
        None => parsed.name.clone(),
    };
    if !is_kebab(&target_name) {
        return Err(format!("目标名 {target_name} 必须为 kebab-case"));
    }
    let root = ctx.managed_root();
    let target_dir = root.join(&target_name);
    let target_flat = root.join(format!("{target_name}.md"));
    if target_dir.exists() || target_flat.exists() {
        return Err(format!(
            "技能 {target_name} 已存在(managed 根),可先删除或换名导入"
        ));
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("创建 managed 根失败:{e}"))?;
    if src.is_dir() {
        copy_tree(&src_dir, &target_dir)?;
    } else {
        std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建目录失败:{e}"))?;
        std::fs::copy(&md_path, target_dir.join("SKILL.md"))
            .map_err(|e| format!("拷贝失败:{e}"))?;
    }
    let md = target_dir.join("SKILL.md");
    let parsed = parse_skill_file(&md).map_err(|e| format!("导入后解析失败:{e}"))?;
    let has_scripts = std::fs::read_dir(&target_dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy() != "SKILL.md")
        })
        .unwrap_or(false);
    Ok(make_summary(
        SkillSource::Managed,
        &target_dir,
        &md,
        &parsed,
        has_scripts,
    ))
}

/// 递归拷贝(不跟随符号链接;失败即中断)。
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目标失败:{e}"))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("读源失败:{e}"))?;
    for e in entries.flatten() {
        let ft = e.file_type().map_err(|e| format!("读类型失败:{e}"))?;
        let to = dst.join(e.file_name());
        if ft.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else if ft.is_file() {
            let from = e.path();
            std::fs::copy(&from, &to)
                .map_err(|err| format!("拷贝 {} 失败:{err}", from.display()))?;
        }
        // 符号链接跳过(安全)
    }
    Ok(())
}

/// 预览正文(SKILL.md 全量,上限 256 KB)。
pub fn preview(source_path: &str) -> Result<String, String> {
    let p = Path::new(source_path);
    let f = if p.is_dir() {
        p.join("SKILL.md")
    } else {
        p.to_path_buf()
    };
    if !f.is_file() {
        return Err(format!("找不到技能文件:{}", f.display()));
    }
    let meta = std::fs::metadata(&f).map_err(|e| format!("读取失败:{e}"))?;
    if meta.len() > MAX_PREVIEW_BYTES {
        return Err("技能正文超过预览上限(256 KB)".to_string());
    }
    std::fs::read_to_string(&f).map_err(|e| format!("读取失败:{e}"))
}

// ── 注入控制(与 skill-external-roots v0.2 联动) ──────────

/// 控制文件路径:约定 $DSH_HOME/skills-control.json。
pub fn control_file(dsh_home: &Path) -> PathBuf {
    dsh_home.join("skills-control.json")
}

/// active 清单路径:约定 $DSH_HOME/state/skills-active.json。
pub fn active_file(dsh_home: &Path) -> PathBuf {
    dsh_home.join("state").join("skills-active.json")
}

/// 读目标 profile 补丁中 skill-external-roots 行配置的 skillControlFile。
fn patch_skill_control_file(dsh_home: &Path, profile: &str) -> Option<String> {
    let path = plugins::profile_patch_path(dsh_home, profile);
    let text = std::fs::read_to_string(&path).ok()?;
    let doc = plugins::split_patch_doc(&text);
    let block = plugins::entry_block(&doc, "skill-external-roots")?;
    let v = plugins::extract_config(&block, block.contains("!!js"))?;
    let s = v.get("skillControlFile").and_then(|x| x.as_str())?;
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// 已启动技能清单快照(读插件回写的 skills-active.json + 控制配置状态)。
pub fn active_snapshot(ctx: &ScanCtx, profile: &str) -> crate::contract::SkillsActiveSnapshot {
    let home = plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let file = active_file(&home);
    let control_file = patch_skill_control_file(&home, profile);
    let control_file_exists = control_file
        .as_ref()
        .is_some_and(|p| Path::new(p).is_file());
    let (written_at, skills, error) = match std::fs::read_to_string(&file) {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => {
                let written_at = v.get("writtenAt").and_then(|x| x.as_i64());
                let skills: Vec<crate::contract::ActiveSkill> = v
                    .get("skills")
                    .and_then(|x| x.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| serde_json::from_value(e.clone()).ok())
                            .filter(|skill: &crate::contract::ActiveSkill| {
                                !is_cursor_system_root(&skill.root)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (written_at, skills, None)
            }
            Err(e) => (None, Vec::new(), Some(format!("解析失败:{e}"))),
        },
        Err(_) => (None, Vec::new(), None),
    };
    crate::contract::SkillsActiveSnapshot {
        file: file.to_string_lossy().to_string(),
        written_at,
        skills,
        error,
        control_file,
        control_file_exists,
    }
}

/// 注入控制文件状态(启动器写,插件读;缺失 = 默认全开)。
pub fn control_state(ctx: &ScanCtx) -> crate::contract::SkillsControlState {
    let home = plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let file = control_file(&home);
    let (version, roots, skills) = match std::fs::read_to_string(&file) {
        Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .map(|v| {
                let version = v.get("version").and_then(|x| x.as_u64()).unwrap_or(1) as u32;
                let roots = bool_map(v.get("roots"));
                let skills = bool_map(v.get("skills"));
                (version, roots, skills)
            })
            .unwrap_or((1, Default::default(), Default::default())),
        Err(_) => (1, Default::default(), Default::default()),
    };
    crate::contract::SkillsControlState {
        file: file.to_string_lossy().to_string(),
        version,
        roots,
        skills,
    }
}

fn bool_map(v: Option<&serde_json::Value>) -> std::collections::BTreeMap<String, bool> {
    v.and_then(|x| x.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_bool().map(|b| (k.clone(), b)))
                .collect()
        })
        .unwrap_or_default()
}

fn write_control_file(
    file: &Path,
    roots: &std::collections::BTreeMap<String, bool>,
    skills: &std::collections::BTreeMap<String, bool>,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "version": 1,
        "roots": serde_json::Value::Object(roots.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect()),
        "skills": serde_json::Value::Object(skills.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect()),
    });
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败:{e}"))?;
    }
    let tmp = file.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("写入控制文件失败:{e}"))?;
        f.write_all(
            serde_json::to_string_pretty(&payload)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|e| format!("写入控制文件失败:{e}"))?;
    }
    std::fs::rename(&tmp, file).map_err(|e| format!("控制文件落盘失败:{e}"))
}

/// 技能注入开关:更新控制文件 skills[name] = enabled(原子写)。
pub fn set_injected(
    _log: &Arc<LogHub>,
    ctx: &ScanCtx,
    name: &str,
    enabled: bool,
) -> Result<crate::contract::SkillToggleResult, String> {
    let name = name.trim();
    if !is_kebab(name) {
        return Err(format!("技能名 {name} 必须为 kebab-case"));
    }
    let home = plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let file = control_file(&home);
    let current = control_state(ctx);
    let mut skills = current.skills;
    skills.insert(name.to_string(), enabled);
    write_control_file(&file, &current.roots, &skills)?;
    Ok(crate::contract::SkillToggleResult {
        ok: true,
        summary: format!(
            "{name} 已{}注入(控制文件 {})· 运行中 dsh 约 1-2 秒内热更新",
            if enabled { "开启" } else { "关闭" },
            file.display()
        ),
        enabled,
    })
}

/// 按外部工具族根目录批量开关(Cursor/Codex/Claude/OpenCode)。
/// roots.cursor=false 会同时关闭 Cursor 的 skills 与 skills-* 根。
pub fn set_root_injected(
    _log: &Arc<LogHub>,
    ctx: &ScanCtx,
    root_key: &str,
    enabled: bool,
) -> Result<crate::contract::SkillToggleResult, String> {
    if !matches!(root_key, "codex" | "claude" | "cursor" | "opencode") {
        return Err(format!("不支持按根目录开关:{root_key}"));
    }
    let home = plugins::dsh_home_dir(&ctx.dsh_home_setting);
    let file = control_file(&home);
    let current = control_state(ctx);
    let mut roots = current.roots;
    roots.insert(root_key.to_string(), enabled);
    write_control_file(&file, &roots, &current.skills)?;
    Ok(crate::contract::SkillToggleResult {
        ok: true,
        summary: format!(
            "{root_key} 根目录下技能已{}注入(控制文件 {})· 运行中 dsh 约 1-2 秒内热更新",
            if enabled { "开启" } else { "关闭" },
            file.display()
        ),
        enabled,
    })
}

/// 外部发现扫描与已启动清单的去重视图:返回 (技能名 → 是否已注入)。
pub fn injected_map(
    active: &crate::contract::SkillsActiveSnapshot,
) -> std::collections::HashSet<String> {
    active.skills.iter().map(|s| s.name.clone()).collect()
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_entries_exclude_cursor_system_skills() {
        let ctx = ScanCtx {
            repo_path: "/tmp/dsh-launcher-test-repo".into(),
            dsh_home_setting: "/tmp/dsh-launcher-test-home".into(),
            skill_managed_root_setting: "/tmp/dsh-launcher-test-home/skills".into(),
            external_skill_roots: vec![
                "~/.cursor/skills-cursor".into(),
                "/tmp/custom-skills".into(),
            ],
        };
        let roots = root_entries(&ctx);
        assert!(roots.iter().all(|root| root.key != "cursor"));
        assert!(roots.iter().all(|root| !root.path.contains(".cursor")));
        assert!(roots.iter().any(|root| root.path == "/tmp/custom-skills"));
        assert!(is_cursor_system_root("~/.cursor/skills-cursor"));
        assert!(!is_cursor_system_root("~/.codex/skills"));
    }

    #[test]
    fn kebab_validation_matches_dsh() {
        assert!(is_kebab("foo"));
        assert!(is_kebab("foo-bar"));
        assert!(is_kebab("a1-b2-c3"));
        assert!(!is_kebab("Foo"));
        assert!(!is_kebab("foo_bar"));
        assert!(!is_kebab("-foo"));
        assert!(!is_kebab("foo-"));
        assert!(!is_kebab("foo--bar"));
        assert!(!is_kebab("foo bar"));
        assert!(!is_kebab(""));
        assert!(!is_kebab("技能"));
    }

    #[test]
    fn frontmatter_split_basic() {
        let raw = "---\nname: foo\ndescription: bar\n---\n\nbody text";
        let (yaml, body) = split_frontmatter(raw).unwrap();
        assert!(yaml.contains("name: foo"));
        assert_eq!(body.trim_start(), "body text");
        // 无 frontmatter
        assert!(split_frontmatter("just text").is_none());
        // 未闭合
        assert!(split_frontmatter("---\nname: foo").is_none());
    }

    #[test]
    fn parse_skill_file_accepts_real_world() {
        // claude 风格:块标量 description + 额外键 allowed-tools
        let base = std::env::temp_dir().join(format!("dsh-skills-parse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let f = base.join("tavily-extract.md");
        std::fs::write(
            &f,
            "---\nname: tavily-extract\ndescription: |\n  Extract clean markdown from URLs. Can process up to 20 URLs.\nallowed-tools: Bash(tvly *)\n---\n\nbody",
        )
        .unwrap();
        let p = parse_skill_file(&f).unwrap();
        assert_eq!(p.name, "tavily-extract");
        assert!(p.description.contains("Extract clean markdown"));
        assert!(p.model_invocable);
        assert!(p.user_invocable);

        // cursor 风格:environments 额外键
        let f2 = base.join("automate/SKILL.md");
        std::fs::create_dir_all(f2.parent().unwrap()).unwrap();
        std::fs::write(
            &f2,
            "---\nname: automate\ndescription: Create Cursor Automations.\nenvironments:\n  - local\n---\nbody",
        )
        .unwrap();
        let p2 = parse_skill_file(&f2).unwrap();
        assert_eq!(p2.name, "automate");

        // 缺 description → Err
        let f3 = base.join("bad.md");
        std::fs::write(&f3, "---\nname: bad\n---\nbody").unwrap();
        assert!(parse_skill_file(&f3).is_err());

        // 非法名 → Err
        let f4 = base.join("Bad_Name.md");
        std::fs::write(&f4, "---\nname: Bad_Name\ndescription: x\n---\nbody").unwrap();
        assert!(parse_skill_file(&f4).is_err());

        // 废弃键 → Err
        let f5 = base.join("legacy.md");
        std::fs::write(
            &f5,
            "---\nname: legacy\ndescription: x\nmodelInvocable: false\n---\nbody",
        )
        .unwrap();
        assert!(parse_skill_file(&f5).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_groups_by_source_and_skips_invalid() {
        let base = std::env::temp_dir().join(format!("dsh-skills-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // 非法目录名(非 kebab)的目录包 → 跳过
        std::fs::create_dir_all(base.join("codex/skills/good/SKILL.md").parent().unwrap()).unwrap();
        std::fs::create_dir_all(
            base.join("codex/skills/Bad_Name/SKILL.md")
                .parent()
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            base.join("codex/skills/good/SKILL.md"),
            "---\nname: good\ndescription: ok\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            base.join("codex/skills/Bad_Name/SKILL.md"),
            "---\nname: bad\ndescription: ok\n---\nbody",
        )
        .unwrap();
        // 无 frontmatter 的 md → 跳过并点名
        std::fs::write(base.join("codex/skills/no-fm.md"), "no frontmatter").unwrap();
        // managed 根:flat + 目录包 + .system 跳过
        std::fs::create_dir_all(base.join("dsh/skills/zip/SKILL.md").parent().unwrap()).unwrap();
        std::fs::create_dir_all(base.join("dsh/skills/.system")).unwrap();
        std::fs::write(
            base.join("dsh/skills/zip/SKILL.md"),
            "---\nname: zip\ndescription: z\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            base.join("dsh/skills/win-host.md"),
            "---\nname: win-host\ndescription: w\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            base.join("dsh/skills/.system/hidden.md"),
            "---\nname: hidden\ndescription: h\n---\nbody",
        )
        .unwrap();

        let log = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-skills-scan-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: base.join("dsh").to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![base.join("codex/skills").to_string_lossy().to_string()],
        };
        let snap = scan(&ctx, &log);
        let names: Vec<&str> = snap.skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"good"), "{names:?}");
        assert!(names.contains(&"zip"), "{names:?}");
        assert!(names.contains(&"win-host"), "{names:?}");
        assert!(!names.contains(&"bad"), "目录名非 kebab 应跳过: {names:?}");
        assert!(!names.contains(&"no-fm"), "无 frontmatter 应跳过");
        assert!(!names.contains(&"hidden"), ".system 应跳过");
        let bad = snap.skills.iter().find(|s| s.name == "good").unwrap();
        assert_eq!(bad.source, SkillSource::Custom, "codex 根经 custom 追加");
        let managed = snap.skills.iter().find(|s| s.name == "zip").unwrap();
        assert_eq!(managed.source, SkillSource::Managed);
        assert!(
            snap.skipped.iter().any(|s| s.contains("Bad_Name")),
            "{:?}",
            snap.skipped
        );
        assert!(
            snap.skipped.iter().any(|s| s.contains("no-fm")),
            "{:?}",
            snap.skipped
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn create_update_delete_respects_fence() {
        let base = std::env::temp_dir().join(format!("dsh-skills-crud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("dsh/skills");
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: base.join("dsh").to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![],
        };
        let log = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-skills-crud-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        // 非法名拒绝
        assert!(create(&log, &ctx, "Bad_Name", "d", None, "b").is_err());
        // 创建
        let s = create(
            &log,
            &ctx,
            "my-skill",
            "我的技能",
            Some("when needed"),
            "正文",
        )
        .unwrap();
        assert_eq!(s.name, "my-skill");
        assert!(root.join("my-skill/SKILL.md").is_file());
        // 重复创建拒绝
        assert!(create(&log, &ctx, "my-skill", "d", None, "b").is_err());
        // 更新
        let u = update(&log, &ctx, "my-skill", "新描述", None, "新正文").unwrap();
        assert_eq!(u.description, "新描述");
        let raw = std::fs::read_to_string(root.join("my-skill/SKILL.md")).unwrap();
        assert!(!raw.contains("whenToUse"), "{raw}");
        assert!(raw.contains("新正文"));
        // 删除
        delete(&log, &ctx, "my-skill").unwrap();
        assert!(!root.join("my-skill").exists());
        assert!(delete(&log, &ctx, "my-skill").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_external_path_refused() {
        // 技能名含路径穿越:find_managed 只在根内查找,不存在则拒绝
        let base = std::env::temp_dir().join(format!("dsh-skills-fence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("dsh/skills");
        std::fs::create_dir_all(&root).unwrap();
        // 在根外放一个同名技能,模拟外部路径
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("SKILL.md"),
            "---\nname: evil\ndescription: x\n---\nbody",
        )
        .unwrap();
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: base.join("dsh").to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![],
        };
        let log = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-skills-fence-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        // name 无法定位到根外 → 拒绝
        assert!(delete(&log, &ctx, "evil").is_err());
        assert!(outside.join("SKILL.md").is_file(), "外部文件不得被删");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_copies_tree_and_validates() {
        let base = std::env::temp_dir().join(format!("dsh-skills-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src/codex-skill");
        std::fs::create_dir_all(src.join("scripts")).unwrap();
        std::fs::create_dir_all(src.join("references")).unwrap();
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: codex-skill\ndescription: from codex\n---\nbody",
        )
        .unwrap();
        std::fs::write(src.join("scripts/run.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(src.join("references/ref.md"), "# ref").unwrap();
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: base.join("dsh").to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![],
        };
        let log = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-skills-import-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        let s = import(&log, &ctx, &src.to_string_lossy(), None).unwrap();
        assert_eq!(s.name, "codex-skill");
        assert!(s.has_scripts);
        let root = ctx.managed_root();
        assert!(root.join("codex-skill/scripts/run.sh").is_file());
        assert!(root.join("codex-skill/references/ref.md").is_file());
        // 重复导入冲突
        assert!(import(&log, &ctx, &src.to_string_lossy(), None).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn preview_caps_size() {
        let base = std::env::temp_dir().join(format!("dsh-skills-prev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let f = base.join("SKILL.md");
        std::fs::write(&f, "hello").unwrap();
        assert_eq!(preview(&base.to_string_lossy()).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn set_injected_writes_control_file_atomically() {
        let base = std::env::temp_dir().join(format!("dsh-skills-ctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dsh = base.join("dsh");
        std::fs::create_dir_all(&dsh).unwrap();
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: dsh.to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![],
        };
        let log = Arc::new(LogHub::new(
            std::env::temp_dir().join(format!("dsh-skills-ctl-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        ));
        // 非法名拒绝
        assert!(set_injected(&log, &ctx, "Bad Name", false).is_err());
        // 关闭 → 打开
        let r = set_injected(&log, &ctx, "win-host", false).unwrap();
        assert!(r.ok && !r.enabled);
        let ctl = control_state(&ctx);
        assert_eq!(ctl.skills.get("win-host"), Some(&false));
        assert_eq!(ctl.version, 1);
        let r2 = set_injected(&log, &ctx, "win-host", true).unwrap();
        assert!(r2.enabled);
        let ctl2 = control_state(&ctx);
        assert_eq!(ctl2.skills.get("win-host"), Some(&true));
        // 文件可解析为合法 JSON(插件读取格式)
        let raw = std::fs::read_to_string(control_file(&dsh)).unwrap();
        assert!(raw.contains("\"win-host\""));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn active_snapshot_parses_plugin_report_and_detects_config() {
        let base = std::env::temp_dir().join(format!("dsh-skills-active-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dsh = base.join("dsh");
        std::fs::create_dir_all(dsh.join("state")).unwrap();
        std::fs::create_dir_all(dsh.join("profiles/web")).unwrap();
        // profile patch:skill-external-roots 行配置了 skillControlFile
        std::fs::write(
            dsh.join("profiles/web/cordis.patch.yml"),
            "- id: skill-external-roots\n  config:\n    skillControlFile: /tmp/x/skills-control.json\n",
        )
        .unwrap();
        std::fs::write(
            dsh.join("state/skills-active.json"),
            r#"{"version":1,"writtenAt":1723800000000,"skills":[
              {"name":"tavily-extract","description":"d","whenToUse":"w","source":"external",
               "root":"/Users/u/.claude/skills","path":"/Users/u/.claude/skills/tavily-extract/SKILL.md",
               "modelInvocable":true,"userInvocable":true}]}"#,
        )
        .unwrap();
        let ctx = ScanCtx {
            repo_path: "/tmp/none".into(),
            dsh_home_setting: dsh.to_string_lossy().to_string(),
            skill_managed_root_setting: String::new(),
            external_skill_roots: vec![],
        };
        let snap = active_snapshot(&ctx, "web");
        assert!(snap.error.is_none(), "{:?}", snap.error);
        assert_eq!(snap.skills.len(), 1);
        assert_eq!(snap.skills[0].name, "tavily-extract");
        assert_eq!(snap.skills[0].root, "/Users/u/.claude/skills");
        assert_eq!(snap.written_at, Some(1723800000000));
        assert_eq!(
            snap.control_file.as_deref(),
            Some("/tmp/x/skills-control.json")
        );
        assert!(!snap.control_file_exists);
        // 去重集合
        let injected = injected_map(&snap);
        assert!(injected.contains("tavily-extract"));
        assert!(!injected.contains("win-host"));
        // 无文件 → 空 + 无错误
        let _ = std::fs::remove_file(dsh.join("state/skills-active.json"));
        let snap2 = active_snapshot(&ctx, "web");
        assert!(snap2.skills.is_empty() && snap2.error.is_none());
        let _ = std::fs::remove_dir_all(&base);
    }
}
