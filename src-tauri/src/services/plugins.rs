// dsh-launcher · PluginsService:profile/bundle/patch 组合视图 + 补丁读写
//
// 数据来源:
// - `$DSH_HOME/profiles/<p>/package.json`(dsh.profile.bundles / dependencies);
// - profile 与 `$DSH_HOME` 的 `cordis.patch.yml`(用户补丁层 + home 层);
// - `dsh --profile <p> --dump-config` 权威组合视图(层标记 + 行块,`!!js` 原样透出)。
//
// 写语义(与 dsh 补丁层一致):
// - patch 按 `id` **整行替换** config(非深合并);
// - 写入前先备份(`cordis.patch.yml.bak-<ts>`),写后跑 dump-config 校验,失败自动回滚;
// - 含 `!!js` 表达式的行锁定为「原始 YAML」模式(不经表单)。
use crate::contract::{
    ConfigSource, DshPluginPackage, PatchWriteResult, PluginLayer, PluginRow, PluginsSnapshot,
    ProfileSummary,
};
use crate::log_hub::LogHub;
use crate::services::dshctl;
use crate::services::runtime::Tools;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ── 路径与 profile 清单 ───────────────────────────────────

/// 解析 DSH_HOME(设置为空时默认 ~/.dsh)。
pub fn dsh_home_dir(dsh_home_setting: &str) -> PathBuf {
    if dsh_home_setting.is_empty() {
        Path::new(&crate::config::home_dir()).join(".dsh")
    } else {
        PathBuf::from(dsh_home_setting)
    }
}

pub fn profiles_dir(dsh_home: &Path) -> PathBuf {
    dsh_home.join("profiles")
}

pub fn profile_dir(dsh_home: &Path, name: &str) -> PathBuf {
    profiles_dir(dsh_home).join(name)
}

pub fn profile_patch_path(dsh_home: &Path, name: &str) -> PathBuf {
    profile_dir(dsh_home, name).join("cordis.patch.yml")
}

pub fn home_patch_path(dsh_home: &Path) -> PathBuf {
    dsh_home.join("cordis.patch.yml")
}

/// 读 profile 的 package.json manifest → (bundles, deps)。
fn read_profile_manifest(dir: &Path) -> Option<(Vec<String>, BTreeMap<String, String>)> {
    let raw = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let bundles = v
        .pointer("/dsh/profile/bundles")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut deps = BTreeMap::new();
    if let Some(d) = v.get("dependencies").and_then(|x| x.as_object()) {
        for (k, val) in d {
            if let Some(s) = val.as_str() {
                deps.insert(k.clone(), s.to_string());
            }
        }
    }
    Some((bundles, deps))
}

/// 扫描 $DSH_HOME/profiles/* 的 profile 摘要(按名排序)。
pub fn profiles(dsh_home: &Path) -> Vec<ProfileSummary> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(profiles_dir(dsh_home)) else {
        return out;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let Some((bundles, deps)) = read_profile_manifest(&e.path()) else {
            continue;
        };
        out.push(ProfileSummary {
            name,
            bundles,
            deps,
            patch_ok: e.path().join("cordis.patch.yml").is_file(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── patch 文档(行级拆分,保留注释与原始文本) ────────────────

/// 新 profile patch 的默认头注释(与 dsh 模板一致)。
pub const DEFAULT_PATCH_HEADER: &str = "# Your patch layer for this dsh profile, applied after every bundle layer:\n# a top-level YAML array of loader patch entries (id-targeted config\n# overrides, disables, and insert lists; `!!js` expressions allowed).";

#[derive(Debug, Clone)]
pub struct PatchEntry {
    /// 条目 id;`- insert:` 块为 "insert"。
    pub id: String,
    /// 前导注释/空行(保留原样)。
    pub prefix: String,
    /// 条目行体(以 `- id:` / `- insert:` 开头,不含前导注释)。
    pub block: String,
    pub has_js: bool,
}

#[derive(Debug, Clone)]
pub struct PatchDoc {
    pub header: String,
    pub entries: Vec<PatchEntry>,
    /// 末尾注释/空行(保留原样)。
    pub trailing: String,
    /// 顶层显式空数组标记(`[]` 行):仅当无条目时输出,保证补丁是合法 YAML 数组。
    pub empty_array: bool,
}

fn line_is_top_entry(line: &str) -> bool {
    line.starts_with("- id:") || line.starts_with("- insert:")
}

/// 收束当前条目:pending → header(首条目前)/ prefix(条目间),条目体入 entries。
fn flush_entry(
    current: &mut Option<(String, Vec<String>, bool)>,
    entries: &mut Vec<PatchEntry>,
    pending: &mut Vec<String>,
    header: &mut String,
    started: &mut bool,
) {
    if let Some((id, body, has_js)) = current.take() {
        if !*started {
            *header = std::mem::take(pending).join("\n");
            *started = true;
        }
        let prefix = std::mem::take(pending).join("\n");
        entries.push(PatchEntry {
            id,
            prefix,
            block: body.join("\n"),
            has_js,
        });
    }
}

/// 把 patch/dump 文本拆分为顶层条目;列首注释保留为下一条目前导(首条目前为 header)。
pub fn split_patch_doc(text: &str) -> PatchDoc {
    let mut header = String::new();
    let mut entries: Vec<PatchEntry> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut current: Option<(String, Vec<String>, bool)> = None;
    let mut started = false;
    let mut empty_array = false;

    for line in text.lines() {
        if line.trim() == "[]" {
            // 顶层显式空数组标记(仅无条目时输出)
            flush_entry(
                &mut current,
                &mut entries,
                &mut pending,
                &mut header,
                &mut started,
            );
            empty_array = true;
            continue;
        }
        if line_is_top_entry(line) {
            flush_entry(
                &mut current,
                &mut entries,
                &mut pending,
                &mut header,
                &mut started,
            );
            let id = if line.starts_with("- id:") {
                line.trim_start_matches("- id:").trim().to_string()
            } else {
                "insert".to_string()
            };
            current = Some((id, vec![line.to_string()], line.contains("!!js")));
            continue;
        }
        match current.as_mut() {
            Some((_, body, has_js)) => {
                if line.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
                    if line.contains("!!js") {
                        *has_js = true;
                    }
                    body.push(line.to_string());
                } else {
                    // 列首注释 / 异常列首行:结束当前条目
                    flush_entry(
                        &mut current,
                        &mut entries,
                        &mut pending,
                        &mut header,
                        &mut started,
                    );
                    pending.push(line.to_string());
                }
            }
            None => {
                pending.push(line.to_string());
            }
        }
    }
    flush_entry(
        &mut current,
        &mut entries,
        &mut pending,
        &mut header,
        &mut started,
    );
    PatchDoc {
        header,
        entries,
        trailing: pending.join("\n"),
        empty_array,
    }
}

/// 按行体重组文本(header + entries(prefix+block) + trailing;仅无条目时输出 `[]`)。
pub fn reassemble(doc: &PatchDoc) -> String {
    let mut out = String::new();
    if !doc.header.is_empty() {
        out.push_str(&doc.header);
        out.push('\n');
    }
    for e in &doc.entries {
        if !e.prefix.is_empty() {
            out.push_str(&e.prefix);
            out.push('\n');
        }
        out.push_str(&e.block);
        out.push('\n');
    }
    if !doc.trailing.is_empty() {
        out.push_str(&doc.trailing);
        out.push('\n');
    }
    if doc.entries.is_empty() && doc.empty_array {
        out.push_str("[]\n");
    }
    out
}

fn entry_index(doc: &PatchDoc, id: &str) -> Option<usize> {
    doc.entries
        .iter()
        .position(|e| e.id == id && e.block.starts_with("- id:"))
}

fn block_lines(block: &str) -> Vec<String> {
    block.split('\n').map(String::from).collect()
}

fn lines_to_block(lines: Vec<String>) -> String {
    lines.join("\n")
}

fn entry_has_disabled_true(block: &str) -> bool {
    block.lines().any(|l| l == "  disabled: true")
}

/// set_enabled 纯逻辑(doc 不可变 → 返回新 doc)。effective_disabled 来自 dump(组合后)。
pub fn apply_set_enabled(
    doc: &PatchDoc,
    id: &str,
    enabled: bool,
    effective_disabled: bool,
) -> (PatchDoc, bool, String) {
    let mut next = doc.clone();
    let idx = entry_index(&next, id);
    let mut changed = false;
    if enabled {
        // 启用:移除用户条目里的 disabled:true;若停用来自非用户层,写显式 disabled:false 覆盖
        let had_user_disable = idx
            .and_then(|i| next.entries.get(i))
            .is_some_and(|e| entry_has_disabled_true(&e.block));
        if had_user_disable {
            if let Some(i) = idx {
                let e = &mut next.entries[i];
                let lines: Vec<String> = block_lines(&e.block)
                    .into_iter()
                    .filter(|l| l != "  disabled: true")
                    .collect();
                if lines.len() <= 1 {
                    // 仅剩 `- id:` 一行:整条移除(语义 = 移除该字段)
                    next.entries.remove(i);
                } else {
                    e.block = lines_to_block(lines);
                }
                changed = true;
            }
        }
        if !had_user_disable && effective_disabled {
            // 停用来自 bundle/home 层:必须写 disabled:false 显式覆盖
            if let Some(i) = idx {
                let e = &mut next.entries[i];
                if !e.block.ends_with('\n') {
                    e.block.push('\n');
                }
                e.block.push_str("  disabled: false");
            } else {
                next.entries.push(PatchEntry {
                    id: id.to_string(),
                    prefix: String::new(),
                    block: format!("- id: {id}\n  disabled: false"),
                    has_js: false,
                });
            }
            changed = true;
        }
        if !changed {
            return (next, false, format!("{id} 已启用,无需修改"));
        }
        (next, true, format!("{id} 已启用(写 profile patch 覆盖)"))
    } else {
        // 停用
        if let Some(i) = idx {
            let e = &mut next.entries[i];
            if entry_has_disabled_true(&e.block) {
                return (next, false, format!("{id} 已停用,无需修改"));
            }
            if !e.block.ends_with('\n') {
                e.block.push('\n');
            }
            e.block.push_str("  disabled: true");
        } else {
            next.entries.push(PatchEntry {
                id: id.to_string(),
                prefix: String::new(),
                block: format!("- id: {id}\n  disabled: true"),
                has_js: false,
            });
        }
        (next, true, format!("{id} 已停用(写 profile patch 覆盖)"))
    }
}

/// 生成 id-targeted patch 条目块(form 模式)。config 为空对象时省略 config 段。
fn build_form_block(id: &str, config: &serde_json::Value, effective_disabled: bool) -> String {
    let mut lines = vec![format!("- id: {id}")];
    if effective_disabled {
        lines.push("  disabled: true".to_string());
    }
    let empty_obj = matches!(config, serde_json::Value::Object(m) if m.is_empty());
    if !empty_obj {
        if let Ok(yaml) = serde_yaml_ng::to_string(config) {
            lines.push("  config:".to_string());
            for l in yaml.lines() {
                lines.push(format!("    {l}"));
            }
        }
    }
    lines.join("\n")
}

/// save_config 纯逻辑(form 模式):整行替换 config(全量键由 UI 给出)。
pub fn apply_save_config_form(
    doc: &PatchDoc,
    id: &str,
    config: &serde_json::Value,
    effective_disabled: bool,
) -> (PatchDoc, bool, String) {
    let mut next = doc.clone();
    let block = build_form_block(id, config, effective_disabled);
    let has_js = block.contains("!!js");
    let changed = true;
    if let Some(i) = entry_index(&next, id) {
        let prefix = next.entries[i].prefix.clone();
        next.entries[i] = PatchEntry {
            id: id.to_string(),
            prefix,
            block,
            has_js,
        };
    } else {
        next.entries.push(PatchEntry {
            id: id.to_string(),
            prefix: String::new(),
            block,
            has_js,
        });
    }
    let summary = format!("{id} 的 config 已固化整行(非深合并)");
    (next, changed, summary)
}

/// save_config 纯逻辑(raw 模式):raw_yaml 为完整行块文本(`- id: <id>` 开头)。
pub fn apply_save_config_raw(
    doc: &PatchDoc,
    id: &str,
    raw_yaml: &str,
) -> Result<(PatchDoc, bool, String), String> {
    let trimmed = raw_yaml.trim_start();
    if !trimmed.starts_with("- id:") {
        return Err("原始 YAML 必须以 `- id:` 开头".into());
    }
    let got_id = trimmed
        .trim_start_matches("- id:")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if got_id != id {
        return Err(format!("原始 YAML 的 id({got_id})与目标({id})不一致"));
    }
    let block = raw_yaml.trim_end().to_string();
    let mut next = doc.clone();
    if let Some(i) = entry_index(&next, id) {
        let prefix = next.entries[i].prefix.clone();
        next.entries[i] = PatchEntry {
            id: id.to_string(),
            prefix,
            block: block.clone(),
            has_js: block.contains("!!js"),
        };
    } else {
        next.entries.push(PatchEntry {
            id: id.to_string(),
            prefix: String::new(),
            block: block.clone(),
            has_js: block.contains("!!js"),
        });
    }
    Ok((
        next,
        true,
        format!("{id} 已按原始 YAML 写入(含 !!js 的行保持原样)"),
    ))
}

/// reset 纯逻辑:移除用户 patch 中的该行条目(回落 bundle/home 层)。
pub fn apply_reset_row(doc: &PatchDoc, id: &str) -> (PatchDoc, bool, String) {
    let mut next = doc.clone();
    let Some(i) = entry_index(&next, id) else {
        return (next, false, format!("{id} 没有用户补丁条目,无需重置"));
    };
    // 前导注释保留(追加到末尾,避免丢注释)
    let prefix = next.entries[i].prefix.clone();
    let removed = next.entries.remove(i);
    if !prefix.is_empty() {
        if next.trailing.is_empty() {
            next.trailing = prefix;
        } else {
            next.trailing = format!("{prefix}\n{}", next.trailing);
        }
    }
    (
        next,
        true,
        format!("{id} 已重置:移除用户补丁条目,回落到 {} 层默认", removed.id),
    )
}

// ── dump-config 解析(权威组合视图) ────────────────────────

/// 单行解析结果。
#[derive(Debug, Clone)]
pub struct DumpRow {
    pub layer_label: String,
    pub id: String,
    pub block: String,
}

/// 拆分 dump-config 输出:层标记(`# == <layer>`) + 顶层 `- id:` 行块。
pub fn parse_dump(text: &str) -> Vec<DumpRow> {
    let mut rows = Vec::new();
    let mut layer = String::new();
    let mut cur: Option<(String, Vec<String>)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# == ") {
            if let Some((id, body)) = cur.take() {
                rows.push(DumpRow {
                    layer_label: layer.clone(),
                    id,
                    block: body.join("\n"),
                });
            }
            layer = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("- id:") {
            if let Some((id, body)) = cur.take() {
                rows.push(DumpRow {
                    layer_label: layer.clone(),
                    id,
                    block: body.join("\n"),
                });
            }
            cur = Some((rest.trim().to_string(), vec![line.to_string()]));
            continue;
        }
        if let Some((_, body)) = cur.as_mut() {
            body.push(line.to_string());
        }
    }
    if let Some((id, body)) = cur.take() {
        rows.push(DumpRow {
            layer_label: layer,
            id,
            block: body.join("\n"),
        });
    }
    rows
}

/// 从行块提取 name 字段(首行 `- id:` 之后第一个 `name:` 行;剥离引号)。
fn extract_module(block: &str) -> String {
    block
        .lines()
        .skip(1)
        .find_map(|l| {
            let t = l.trim_start();
            t.strip_prefix("name:").map(|r| {
                let s = r.trim().to_string();
                let s = s
                    .strip_prefix('\'')
                    .and_then(|x| x.strip_suffix('\''))
                    .unwrap_or(&s);
                let s = s
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(s);
                s.to_string()
            })
        })
        .unwrap_or_default()
}

/// 从行块提取 disabled(行级 `  disabled:` 字段)。
pub(crate) fn extract_disabled(block: &str) -> bool {
    block.lines().any(|l| {
        l.trim_start()
            .strip_prefix("disabled:")
            .is_some_and(|r| r.trim().parse::<bool>().ok() == Some(true))
    })
}

/// 从行块提取 config(不含 !!js 时解析为 JSON;含 !!js 返回 None)。
pub(crate) fn extract_config(block: &str, has_js: bool) -> Option<serde_json::Value> {
    if has_js {
        return None;
    }
    let lines: Vec<&str> = block.lines().collect();
    let idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with("config:"))?;
    let indent = lines[idx].len() - lines[idx].trim_start().len();
    let first = lines[idx]
        .trim_start()
        .strip_prefix("config:")
        .unwrap_or("")
        .trim();
    if !first.is_empty() {
        // 单行值(标量/内联)
        return serde_yaml_ng::from_str::<serde_json::Value>(first).ok();
    }
    let mut body: Vec<String> = Vec::new();
    for l in &lines[idx + 1..] {
        let lindent = l.len() - l.trim_start().len();
        if lindent > indent {
            body.push((*l).to_string());
        } else {
            break;
        }
    }
    if body.is_empty() {
        return Some(serde_json::Value::Object(Default::default()));
    }
    serde_yaml_ng::from_str::<serde_json::Value>(&body.join("\n"))
        .ok()
        .or_else(|| Some(serde_json::Value::Object(Default::default())))
}

/// 层分类:label → (layer, 主层名)。
fn classify_layer(label: &str, dsh_home: &Path, profile: &str) -> (PluginLayer, String) {
    let pp = profile_patch_path(dsh_home, profile);
    let hp = home_patch_path(dsh_home);
    // Windows:路径分隔符不敏感比较(Node 的 dump 输出可能混用正/反斜杠)
    let norm = |s: &str| -> String {
        if cfg!(windows) {
            s.replace('\\', "/")
        } else {
            s.to_string()
        }
    };
    if label.contains("cordis.patch.yml") {
        // label 形如 "…/cordis.patch.yml" 或 "bundle, patched by …/cordis.patch.yml"
        let last = label.rsplit(',').next().unwrap_or(label).trim();
        let last_norm = norm(last);
        let pp_norm = norm(&pp.to_string_lossy());
        let hp_norm = norm(&hp.to_string_lossy());
        let label_norm = norm(label);
        if last_norm == pp_norm || label_norm.contains(&pp_norm) {
            (PluginLayer::ProfilePatch, last.to_string())
        } else if last_norm == hp_norm || label_norm.contains(&hp_norm) {
            (PluginLayer::HomePatch, last.to_string())
        } else {
            (PluginLayer::Overlay, last.to_string())
        }
    } else {
        let main = label.split(',').next().unwrap_or(label).trim().to_string();
        // 非 cordis.patch.yml 的绝对路径 / ~ 路径 / .yml|.yaml 文件视为 --patch overlay
        let is_path = main.starts_with('/')
            || main.starts_with('~')
            || main.ends_with(".yml")
            || main.ends_with(".yaml")
            || (cfg!(windows) && main.chars().nth(1) == Some(':'));
        if is_path {
            (PluginLayer::Overlay, main)
        } else {
            (PluginLayer::Bundle, main)
        }
    }
}

/// 组合快照(文件层 + dump-config 交叉)。
pub fn snapshot(
    tools: &Tools,
    repo_path: &str,
    dsh_home_setting: &str,
    profile_name: &str,
    dsh_plugins_path: &str,
) -> PluginsSnapshot {
    let home = dsh_home_dir(dsh_home_setting);
    let profiles = profiles(&home);
    let profile = if profile_name.is_empty() {
        profiles.first().map(|p| p.name.clone())
    } else if profiles.iter().any(|p| p.name == profile_name) {
        Some(profile_name.to_string())
    } else {
        None
    };

    let mut rows = Vec::new();
    let mut dump_error = None;
    if let Some(p) = profile.as_ref() {
        let dump = dshctl::run_capture(
            tools,
            repo_path,
            dsh_home_setting,
            &[
                "--profile".to_string(),
                p.clone(),
                "--dump-config".to_string(),
            ],
            dshctl::CAPTURE_TIMEOUT,
        );
        match dump {
            Ok(text) => {
                let dump_rows = parse_dump(&text);
                let user_doc = read_patch_doc(&profile_patch_path(&home, p));
                let user_ids: std::collections::HashSet<String> = user_doc
                    .entries
                    .iter()
                    .filter(|e| e.block.starts_with("- id:"))
                    .map(|e| e.id.clone())
                    .collect();
                let packages = scan_packages(dsh_plugins_path, &profiles);
                let desc_map: BTreeMap<String, String> = packages
                    .iter()
                    .map(|p| (p.name.clone(), p.description.clone()))
                    .collect();
                for r in dump_rows {
                    let (layer, layer_label) = classify_layer(&r.layer_label, &home, p);
                    let has_js = r.block.contains("!!js");
                    let config = extract_config(&r.block, has_js);
                    rows.push(PluginRow {
                        id: r.id.clone(),
                        module: extract_module(&r.block),
                        layer,
                        layer_label,
                        in_user_patch: user_ids.contains(&r.id),
                        enabled: !extract_disabled(&r.block),
                        config,
                        config_source: if has_js {
                            ConfigSource::RawYaml
                        } else {
                            ConfigSource::Dump
                        },
                        raw_block: format!("{}\n", r.block),
                        editable: true,
                        description: desc_map.get(&extract_module(&r.block)).cloned(),
                    });
                }
                return PluginsSnapshot {
                    profiles,
                    rows,
                    packages,
                    profile,
                    dump_error: None,
                };
            }
            Err(e) => {
                dump_error = Some(format!("dump-config 失败:{e}"));
            }
        }
    }
    let packages = scan_packages(dsh_plugins_path, &profiles);
    PluginsSnapshot {
        profiles,
        rows,
        packages,
        profile,
        dump_error,
    }
}

/// 读 patch 文件(缺失时视为空文档)。
fn read_patch_doc(path: &Path) -> PatchDoc {
    match std::fs::read_to_string(path) {
        Ok(text) => split_patch_doc(&text),
        Err(_) => PatchDoc {
            header: String::new(),
            entries: Vec::new(),
            trailing: String::new(),
            empty_array: false,
        },
    }
}

// ── 写入:备份 → 原子写 → dump-config 校验 → 失败回滚 ─────

fn now_ts() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// 写 profile patch(带备份 + 校验 + 回滚)。
/// changed=false 时不写不校验,直接返回 ok。
#[allow(clippy::too_many_arguments)]
fn write_patch_validated(
    log: &Arc<LogHub>,
    tools: &Tools,
    repo_path: &str,
    dsh_home_setting: &str,
    profile: &str,
    new_text: &str,
    changed: bool,
    summary: String,
) -> Result<PatchWriteResult, String> {
    if !changed {
        return Ok(PatchWriteResult {
            backup: None,
            ok: true,
            summary,
            validated: true,
            error: None,
        });
    }
    let home = dsh_home_dir(dsh_home_setting);
    let dir = profile_dir(&home, profile);
    if !dir.is_dir() {
        return Err(format!("profile '{profile}' 不存在({})", dir.display()));
    }
    let patch_path = dir.join("cordis.patch.yml");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 profile 目录失败:{e}"))?;

    // 备份
    let backup = if patch_path.exists() {
        let bak = format!("cordis.patch.yml.bak-{}", now_ts());
        std::fs::copy(&patch_path, dir.join(&bak)).map_err(|e| format!("备份补丁失败:{e}"))?;
        Some(bak)
    } else {
        None
    };

    // 原子写
    let tmp = dir.join("cordis.patch.yml.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("写入补丁失败:{e}"))?;
        f.write_all(new_text.as_bytes())
            .map_err(|e| format!("写入补丁失败:{e}"))?;
        f.sync_all().map_err(|e| format!("补丁 fsync 失败:{e}"))?;
    }
    std::fs::rename(&tmp, &patch_path).map_err(|e| format!("补丁落盘失败:{e}"))?;
    log.append(
        "launcher",
        crate::contract::LogLevel::Info,
        &format!("已写入 profile '{profile}' 补丁:{}", patch_path.display()),
    );

    // dump-config 校验;失败自动回滚
    let dump = dshctl::run_capture(
        tools,
        repo_path,
        dsh_home_setting,
        &[
            "--profile".to_string(),
            profile.to_string(),
            "--dump-config".to_string(),
        ],
        dshctl::CAPTURE_TIMEOUT,
    );
    match dump {
        Ok(_) => Ok(PatchWriteResult {
            backup,
            ok: true,
            summary,
            validated: true,
            error: None,
        }),
        Err(e) => {
            // 回滚
            if let Some(bak) = backup.as_ref() {
                let _ = std::fs::copy(dir.join(bak), &patch_path);
                log.append(
                    "launcher",
                    crate::contract::LogLevel::Err,
                    &format!(
                        "补丁校验失败,已回滚备份 {}:{}",
                        bak,
                        e.lines().next().unwrap_or("")
                    ),
                );
            }
            Err(format!("补丁校验失败(已自动回滚):{e}"))
        }
    }
}

/// 组装最终文本:缺失文件时用默认头;空补丁(无条目)必须显式 `[]`
/// 才能通过 dsh 的「顶层必须是 YAML 数组」校验(dsh 自带模板即因空文件报错)。
fn final_text(doc: &PatchDoc, existed: bool) -> String {
    let mut d = doc.clone();
    if !existed && d.header.is_empty() && d.entries.is_empty() && d.trailing.is_empty() {
        d.header = DEFAULT_PATCH_HEADER.to_string();
    }
    let mut text = reassemble(&d);
    if d.entries.is_empty() && !text.trim_end().ends_with(']') {
        text.push_str("[]\n");
    }
    text
}

// ── 对外写操作 ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WriteCtx {
    pub tools: Tools,
    pub repo_path: String,
    pub dsh_home_setting: String,
}

impl WriteCtx {
    fn dump(&self, profile: &str) -> Result<Vec<DumpRow>, String> {
        let text = dshctl::run_capture(
            &self.tools,
            &self.repo_path,
            &self.dsh_home_setting,
            &[
                "--profile".to_string(),
                profile.to_string(),
                "--dump-config".to_string(),
            ],
            dshctl::CAPTURE_TIMEOUT,
        )?;
        Ok(parse_dump(&text))
    }
}

/// 插件启停。
pub fn set_enabled(
    log: &Arc<LogHub>,
    ctx: &WriteCtx,
    profile: &str,
    id: &str,
    enabled: bool,
) -> Result<PatchWriteResult, String> {
    let home = dsh_home_dir(&ctx.dsh_home_setting);
    let patch_path = profile_patch_path(&home, profile);
    let existed = patch_path.exists();
    let doc = read_patch_doc(&patch_path);
    // 组合后停用态来自 dump(可能源于 bundle/home 层)
    let rows = ctx.dump(profile)?;
    let effective_disabled = rows
        .iter()
        .find(|r| r.id == id)
        .is_some_and(|r| extract_disabled(&r.block));
    let (doc, changed, summary) = apply_set_enabled(&doc, id, enabled, effective_disabled);
    write_patch_validated(
        log,
        &ctx.tools,
        &ctx.repo_path,
        &ctx.dsh_home_setting,
        profile,
        &final_text(&doc, existed),
        changed,
        summary,
    )
}

/// 保存配置:form(config JSON)或 raw(raw_yaml Some 时优先)。
pub fn save_config(
    log: &Arc<LogHub>,
    ctx: &WriteCtx,
    profile: &str,
    id: &str,
    config: &serde_json::Value,
    raw_yaml: Option<&str>,
) -> Result<PatchWriteResult, String> {
    let home = dsh_home_dir(&ctx.dsh_home_setting);
    let patch_path = profile_patch_path(&home, profile);
    let existed = patch_path.exists();
    let doc = read_patch_doc(&patch_path);
    let rows = ctx.dump(profile)?;
    let effective_disabled = rows
        .iter()
        .find(|r| r.id == id)
        .is_some_and(|r| extract_disabled(&r.block));
    let (doc, changed, summary) = match raw_yaml {
        Some(raw) => apply_save_config_raw(&doc, id, raw)?,
        None => apply_save_config_form(&doc, id, config, effective_disabled),
    };
    write_patch_validated(
        log,
        &ctx.tools,
        &ctx.repo_path,
        &ctx.dsh_home_setting,
        profile,
        &final_text(&doc, existed),
        changed,
        summary,
    )
}

/// 重置行(删除 profile patch 中该 id 条目)。
pub fn reset_row(
    log: &Arc<LogHub>,
    ctx: &WriteCtx,
    profile: &str,
    id: &str,
) -> Result<PatchWriteResult, String> {
    let home = dsh_home_dir(&ctx.dsh_home_setting);
    let patch_path = profile_patch_path(&home, profile);
    let existed = patch_path.exists();
    let doc = read_patch_doc(&patch_path);
    let (doc, changed, summary) = apply_reset_row(&doc, id);
    write_patch_validated(
        log,
        &ctx.tools,
        &ctx.repo_path,
        &ctx.dsh_home_setting,
        profile,
        &final_text(&doc, existed),
        changed,
        summary,
    )
}

/// 仅校验补丁(不写文件)。
pub fn validate_patch(ctx: &WriteCtx, profile: &str) -> PatchWriteResult {
    match ctx.dump(profile) {
        Ok(rows) => PatchWriteResult {
            backup: None,
            ok: true,
            summary: format!("校验通过:{} 个 loader 行", rows.len()),
            validated: true,
            error: None,
        },
        Err(e) => PatchWriteResult {
            backup: None,
            ok: false,
            summary: "校验失败".into(),
            validated: false,
            error: Some(e),
        },
    }
}

/// 一键启用技能根:写 `skill-filesystem.customSkillDirs`(id-targeted,整行重述既有键)。
/// 若该行当前被上层停用(bundle/profile),一并写 `disabled: false` 覆盖,确保目录真正生效。
pub fn enable_skill_root(
    log: &Arc<LogHub>,
    ctx: &WriteCtx,
    profile: &str,
    root_path: &str,
) -> Result<PatchWriteResult, String> {
    let home = dsh_home_dir(&ctx.dsh_home_setting);
    let patch_path = profile_patch_path(&home, profile);
    let existed = patch_path.exists();
    let doc = read_patch_doc(&patch_path);
    let rows = ctx.dump(profile)?;
    let row = rows.iter().find(|r| r.id == "skill-filesystem");
    if let Some(r) = row {
        if r.block.contains("!!js") {
            return Err("skill-filesystem 行含 !!js 表达式,请手动编辑补丁".into());
        }
    }
    let current: serde_json::Value = row
        .and_then(|r| extract_config(&r.block, false))
        .unwrap_or_else(|| serde_json::json!({}));
    let mut cfg = match current {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    let dirs = cfg
        .get("customSkillDirs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut dirs = dirs;
    let abs = Path::new(root_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(root_path));
    let abs_s = abs.to_string_lossy().to_string();
    if !dirs.iter().any(|d| d.as_str() == Some(abs_s.as_str())) {
        dirs.push(serde_json::json!(abs_s));
    }
    cfg.insert("customSkillDirs".into(), serde_json::Value::Array(dirs));
    // 强制启用:即使当前被上层停用,也写无 disabled 的整行覆盖
    let (doc, changed, summary) = apply_save_config_form(
        &doc,
        "skill-filesystem",
        &serde_json::Value::Object(cfg),
        false,
    );
    write_patch_validated(
        log,
        &ctx.tools,
        &ctx.repo_path,
        &ctx.dsh_home_setting,
        profile,
        &final_text(&doc, existed),
        changed,
        summary,
    )
}

/// 一键启用注入控制:把 skillControlFile/activeFile 写进 skill-external-roots 行
/// (id-targeted,整行重述既有键;行被上层停用时一并强制启用)。
pub fn enable_skill_control(
    log: &Arc<LogHub>,
    ctx: &WriteCtx,
    profile: &str,
    control_file: &str,
    active_file: &str,
) -> Result<PatchWriteResult, String> {
    let home = dsh_home_dir(&ctx.dsh_home_setting);
    let patch_path = profile_patch_path(&home, profile);
    let existed = patch_path.exists();
    let doc = read_patch_doc(&patch_path);
    let rows = ctx.dump(profile)?;
    let row = rows.iter().find(|r| r.id == "skill-external-roots");
    if let Some(r) = row {
        if r.block.contains("!!js") {
            return Err("skill-external-roots 行含 !!js 表达式,请手动编辑补丁".into());
        }
    }
    let current: serde_json::Value = row
        .and_then(|r| extract_config(&r.block, false))
        .unwrap_or_else(|| serde_json::json!({}));
    let mut cfg = match current {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    cfg.insert("skillControlFile".into(), serde_json::json!(control_file));
    cfg.insert("activeFile".into(), serde_json::json!(active_file));
    let (doc, changed, summary) = apply_save_config_form(
        &doc,
        "skill-external-roots",
        &serde_json::Value::Object(cfg),
        false,
    );
    write_patch_validated(
        log,
        &ctx.tools,
        &ctx.repo_path,
        &ctx.dsh_home_setting,
        profile,
        &final_text(&doc, existed),
        changed,
        summary,
    )
}

// ── dsh-plugins 扫描 ──────────────────────────────────────

/// 扫描 <dshPluginsPath>/packages/* 的包清单。
pub fn scan_packages(dsh_plugins_path: &str, profiles: &[ProfileSummary]) -> Vec<DshPluginPackage> {
    if dsh_plugins_path.is_empty() {
        return Vec::new();
    }
    let root = Path::new(dsh_plugins_path);
    let pkg_dir = root.join("packages");
    let Ok(entries) = std::fs::read_dir(&pkg_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let pj = e.path().join("package.json");
        let Ok(raw) = std::fs::read_to_string(&pj) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let name = v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&e.file_name().to_string_lossy())
            .to_string();
        let version = v
            .get("version")
            .and_then(|x| x.as_str())
            .unwrap_or("?")
            .to_string();
        let description = v
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let is_bundle = v
            .pointer("/dsh/bundle/patch")
            .and_then(|x| x.as_str())
            .is_some();
        let patch_file = v
            .pointer("/dsh/bundle/patch")
            .and_then(|x| x.as_str())
            .map(String::from);
        let installed_in = profiles
            .iter()
            .filter(|p| p.deps.contains_key(&name))
            .map(|p| p.name.clone())
            .collect();
        out.push(DshPluginPackage {
            dir: e.file_name().to_string_lossy().to_string(),
            abs_dir: e.path().to_string_lossy().to_string(),
            name,
            version,
            description,
            is_bundle,
            patch_file,
            installed_in,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 自动探测 dsh-plugins 根:解析各 profile deps 里 `file:*dsh-plugins/packages/*` 链接。
/// 全部链接指向同一根时才返回;否则 None(提示手工填写)。
pub fn detect_plugins_path(profiles: &[ProfileSummary]) -> Option<String> {
    let mut found: Vec<String> = Vec::new();
    for p in profiles {
        for spec in p.deps.values() {
            let Some(rest) = spec.strip_prefix("file:") else {
                continue;
            };
            for marker in ["/packages/", "\\packages\\"] {
                if let Some(idx) = rest.find(marker) {
                    found.push(rest[..idx].to_string());
                    break;
                }
            }
        }
    }
    found.sort();
    found.dedup();
    (found.len() == 1).then(|| found[0].clone())
}

// ── 测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_from(text: &str) -> PatchDoc {
        split_patch_doc(text)
    }

    #[test]
    fn split_reassembles_preserving_comments() {
        let text = "# header comment\n# second line\n\n- id: web\n  config:\n    searchProvider: tavily\n\n# explain next\n- id: web-search-deepseek\n  disabled: true\n";
        let d = split_patch_doc(text);
        assert_eq!(d.header, "# header comment\n# second line\n");
        assert_eq!(d.entries.len(), 2);
        assert_eq!(d.entries[0].id, "web");
        assert_eq!(d.entries[1].prefix, "# explain next");
        assert_eq!(d.entries[1].id, "web-search-deepseek");
        // 重组必须无损(注释/空行/缩进原样保留)
        assert_eq!(reassemble(&d), text);
    }

    #[test]
    fn set_enabled_writes_disable_and_remove() {
        let d = doc_from("- id: web\n  config:\n    searchProvider: deepseek-official\n");
        // 停用
        let (d2, changed, _) = apply_set_enabled(&d, "web", false, false);
        assert!(changed);
        assert!(d2.entries[0].block.contains("disabled: true"));
        // 再启用:移除 disabled 行,配置保留
        let (d3, changed2, _) = apply_set_enabled(&d2, "web", true, false);
        assert!(changed2);
        assert!(!d3.entries[0].block.contains("disabled"));
        assert!(d3.entries[0].block.contains("searchProvider"));
    }

    #[test]
    fn set_enabled_override_from_bundle_layer() {
        // 行本身被 bundle 停用(用户无条目):启用必须写 disabled:false 显式覆盖
        let d = doc_from("");
        let (d2, changed, _) = apply_set_enabled(&d, "hmr", true, true);
        assert!(changed);
        assert_eq!(d2.entries.len(), 1);
        assert!(d2.entries[0].block.contains("disabled: false"));
    }

    #[test]
    fn set_enabled_drop_bare_entry() {
        let d = doc_from("- id: web-search-deepseek\n  disabled: true\n");
        let (d2, changed, _) = apply_set_enabled(&d, "web-search-deepseek", true, false);
        assert!(changed);
        assert!(d2.entries.is_empty(), "仅停用条目的行应整条移除");
    }

    #[test]
    fn save_config_form_full_line_replace() {
        let d =
            doc_from("- id: web\n  config:\n    searchProvider: deepseek-official\n    port: 1\n");
        let cfg = serde_json::json!({ "searchProvider": "tavily" });
        let (d2, changed, _) = apply_save_config_form(&d, "web", &cfg, false);
        assert!(changed);
        let block = &d2.entries[0].block;
        assert!(block.contains("searchProvider: tavily"), "{block}");
        assert!(!block.contains("port:"), "整行替换:旧键不得残留: {block}");
    }

    #[test]
    fn save_config_preserves_effective_disabled() {
        let d = doc_from("");
        let cfg = serde_json::json!({ "a": 1 });
        let (d2, _, _) = apply_save_config_form(&d, "hmr", &cfg, true);
        assert!(d2.entries[0].block.contains("disabled: true"));
    }

    #[test]
    fn save_config_raw_roundtrip_with_js() {
        let d = doc_from("");
        let raw =
            "- id: session-persistence-jsonl\n  config:\n    root: !!js dshHomePath('sessions')\n";
        let (d2, changed, _) = apply_save_config_raw(&d, "session-persistence-jsonl", raw).unwrap();
        assert!(changed);
        assert!(d2.entries[0].block.contains("!!js"));
        assert!(d2.entries[0].has_js);
        // id 不匹配必须拒绝
        let err = apply_save_config_raw(&d, "other", raw).unwrap_err();
        assert!(err.contains("不一致"), "{err}");
    }

    #[test]
    fn reset_row_removes_entry_keeps_others() {
        let d = doc_from("- id: a\n  disabled: true\n- id: b\n  config:\n    x: 1\n");
        let (d2, changed, _) = apply_reset_row(&d, "a");
        assert!(changed);
        assert_eq!(d2.entries.len(), 1);
        assert_eq!(d2.entries[0].id, "b");
        let (d3, changed2, _) = apply_reset_row(&d2, "a");
        assert!(!changed2);
        assert_eq!(d3.entries.len(), 1);
    }

    #[test]
    fn parse_dump_extracts_layers_and_rows() {
        let dump = "# == @deepseek-ai/dsh-base\n- id: timer\n  name: '@deepseek-ai/cordis-plugin-timer'\n# == @deepseek-ai/dsh-base, patched by @deepseek-ai/dsh-web-app\n- id: hmr\n  name: '@deepseek-ai/cordis-plugin-hmr'\n  config:\n    root:\n      - .\n  disabled: true\n# == /Users/u/.dsh/profiles/web/cordis.patch.yml\n- id: web-search-tavily\n  name: '@dsh-plugins/web-search-tavily'\n  config:\n    apiKeyEnv: TAVILY_API_KEY\n";
        let rows = parse_dump(dump);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "timer");
        assert_eq!(rows[1].id, "hmr");
        assert!(rows[1].layer_label.contains("patched by"));
        assert_eq!(
            extract_module(&rows[1].block),
            "@deepseek-ai/cordis-plugin-hmr"
        );
        assert!(extract_disabled(&rows[1].block));
        let cfg = extract_config(&rows[1].block, false).unwrap();
        assert_eq!(cfg["root"][0], ".");
        assert_eq!(
            extract_config(&rows[2].block, false).unwrap()["apiKeyEnv"],
            "TAVILY_API_KEY"
        );
    }

    #[test]
    fn classify_layer_kinds() {
        let home = Path::new("/Users/u/.dsh");
        let (l1, _) = classify_layer("@deepseek-ai/dsh-base", home, "web");
        assert_eq!(l1, PluginLayer::Bundle);
        let (l2, _) = classify_layer(
            "@deepseek-ai/dsh-base, patched by /Users/u/.dsh/profiles/web/cordis.patch.yml",
            home,
            "web",
        );
        assert_eq!(l2, PluginLayer::ProfilePatch);
        let (l3, _) = classify_layer("/Users/u/.dsh/cordis.patch.yml", home, "web");
        assert_eq!(l3, PluginLayer::HomePatch);
        let (l4, _) = classify_layer("/tmp/overlay.yml", home, "web");
        assert_eq!(l4, PluginLayer::Overlay);
    }

    #[test]
    fn js_rows_are_raw_yaml() {
        let block = "- id: session-persistence-jsonl\n  name: '@deepseek-ai/dsh-session-persistence-jsonl'\n  config:\n    root: !!js dshHomePath('sessions')\n";
        let has_js = block.contains("!!js");
        assert!(has_js);
        assert!(extract_config(block, has_js).is_none());
    }

    #[test]
    fn detect_plugins_path_from_file_deps() {
        let p = ProfileSummary {
            name: "web".into(),
            bundles: vec![],
            deps: BTreeMap::from([
                (
                    "@dsh-plugins/web-search-tavily".into(),
                    "file:/Users/u/Desktop/dsh-plugins/packages/web-search-tavily".into(),
                ),
                (
                    "@dsh-plugins/vision-bridge".into(),
                    "file:/Users/u/Desktop/dsh-plugins/packages/vision-bridge".into(),
                ),
            ]),
            patch_ok: true,
        };
        let found = detect_plugins_path(std::slice::from_ref(&p));
        assert_eq!(found.as_deref(), Some("/Users/u/Desktop/dsh-plugins"));
        // 指向不同根 → None
        let p2 = ProfileSummary {
            deps: BTreeMap::from([("@x/y".into(), "file:/other/dsh-plugins/packages/y".into())]),
            ..p.clone()
        };
        assert_eq!(detect_plugins_path(&[p, p2]), None);
    }

    #[test]
    fn serde_yaml_emits_clean_object() {
        // 关键:JSON → YAML 输出不得带文档标记/多余引号
        let cfg = serde_json::json!({ "searchProvider": "tavily", "enabled": { "codex": true } });
        let yaml = serde_yaml_ng::to_string(&cfg).unwrap();
        assert!(!yaml.starts_with("---"), "{yaml}");
        assert!(yaml.contains("searchProvider: tavily"), "{yaml}");
        assert!(yaml.contains("enabled:"), "{yaml}");
        assert!(yaml.contains("codex: true"), "{yaml}");
        // 回读一致
        let back: serde_json::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn final_text_uses_default_header_for_new_file() {
        let d = PatchDoc {
            header: String::new(),
            entries: Vec::new(),
            trailing: String::new(),
            empty_array: false,
        };
        let t = final_text(&d, false);
        assert!(t.starts_with("# Your patch layer"), "{t}");
        assert!(t.trim_end().ends_with(']'), "空补丁必须显式 []: {t}");
        let t2 = final_text(&d, true);
        assert_eq!(t2.trim_end(), "[]");

        // 移除唯一条目后(如启用仅含 disabled 的行)也必须产出合法空数组
        let d2 = split_patch_doc("- id: web-search-deepseek\n  disabled: true\n");
        let (d3, changed, _) = apply_set_enabled(&d2, "web-search-deepseek", true, false);
        assert!(changed);
        let t3 = final_text(&d3, true);
        assert!(t3.trim_end().ends_with(']'), "{t3}");

        // 既有空数组文件:加条目后 [] 标记消失,加回时恢复
        let d4 = split_patch_doc("[]\n");
        let (d5, changed5, _) = apply_set_enabled(&d4, "timer", false, false);
        assert!(changed5);
        let t4 = final_text(&d5, true);
        assert!(t4.contains("disabled: true"), "{t4}");
        assert!(!t4.contains("[]"), "有条目时不得残留 []: {t4}");
        assert!(d5.empty_array);
    }
}
