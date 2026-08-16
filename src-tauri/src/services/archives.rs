// dsh-launcher · 归档会话服务
//
// 读取 DSH 的 workspace/session projection JSON 生成 Codex 风格列表。
// DSH 运行时只通过 session-archive-restore 插件读写 live domain；停止后
// 才允许对 workspace.json 做一次原子 JSON 更新。

use crate::contract::{
    ArchiveDeleteResult, ArchiveGroup, ArchiveRestoreResult, ArchiveSession, ArchivesSnapshot,
    SettingsSnapshot,
};
use crate::services::plugins::dsh_home_dir;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ARCHIVE_ROUTE: &str = "/api/dsh-launcher/archive-sessions";
const RESTORE_ROUTE: &str = "/api/dsh-launcher/archive-sessions/restore";
const DELETE_ROUTE: &str = "/api/dsh-launcher/archive-sessions/delete";
const DELETE_ALL_ROUTE: &str = "/api/dsh-launcher/archive-sessions/delete-all";

#[derive(Debug, Clone)]
struct WorkspaceRecord {
    title: String,
    path: String,
    session_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProjectionMeta {
    title: Option<String>,
    created_at: Option<i64>,
    last_prompt_at: Option<i64>,
}

#[derive(Debug, Clone)]
struct WorkspaceData {
    archived_ids: Vec<String>,
    workspace_ids: Vec<String>,
    workspaces: HashMap<String, WorkspaceRecord>,
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} 必须是对象"))
}

fn string_field(value: &Map<String, Value>, key: &str, label: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{label}.{key} 缺失或不是字符串"))
}

fn string_array(value: Option<&Value>, label: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| format!("{label} 必须是数组"))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{label} 包含非字符串值"))
        })
        .collect()
}

fn workspace_path(settings: &SettingsSnapshot) -> PathBuf {
    dsh_home_dir(&settings.dsh_home).join("storages/workspace.json")
}

fn projection_path(settings: &SettingsSnapshot) -> PathBuf {
    dsh_home_dir(&settings.dsh_home).join("storages/session_projcache.json")
}

fn sessions_path(settings: &SettingsSnapshot) -> PathBuf {
    dsh_home_dir(&settings.dsh_home).join("sessions")
}

fn load_json(path: &Path, label: &str) -> Result<Value, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("读取 {label} 失败:{}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 {label} 失败:{}: {e}", path.display()))
}

fn parse_workspace(value: &Value) -> Result<WorkspaceData, String> {
    let root = object(value, "workspace.json")?;
    let unit = object(
        root.get("unit")
            .ok_or_else(|| "workspace.json.unit 缺失".to_string())?,
        "unit",
    )?;
    if unit.get("name").and_then(Value::as_str) != Some("workspace") {
        return Err("不是 workspace domain 文件".into());
    }
    let version = unit.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version != 2 {
        return Err(format!("不支持 workspace domain version {version}"));
    }
    let global = object(
        root.get("global")
            .ok_or_else(|| "workspace.json.global 缺失".to_string())?,
        "global",
    )?;
    let workspace_ids = string_array(global.get("workspaceIds"), "global.workspaceIds")?;
    let archived_ids = string_array(
        global.get("archivedSessionIds"),
        "global.archivedSessionIds",
    )?;
    let tables = object(
        root.get("tables")
            .ok_or_else(|| "workspace.json.tables 缺失".to_string())?,
        "tables",
    )?;
    let records = object(
        tables
            .get("workspaces")
            .ok_or_else(|| "tables.workspaces 缺失".to_string())?,
        "tables.workspaces",
    )?;
    let mut workspaces = HashMap::new();
    for (id, raw) in records {
        let record = object(raw, &format!("tables.workspaces.{id}"))?;
        workspaces.insert(
            id.clone(),
            WorkspaceRecord {
                title: string_field(record, "title", &format!("tables.workspaces.{id}"))?,
                path: string_field(record, "path", &format!("tables.workspaces.{id}"))?,
                session_ids: string_array(
                    record.get("sessionIds"),
                    &format!("tables.workspaces.{id}.sessionIds"),
                )?,
            },
        );
    }
    Ok(WorkspaceData {
        archived_ids,
        workspace_ids,
        workspaces,
    })
}

fn number_field(object: &Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(Value::as_i64)
}

fn parse_projections(value: &Value) -> HashMap<String, ProjectionMeta> {
    let mut result = HashMap::new();
    let Ok(root) = object(value, "session_projcache.json") else {
        return result;
    };
    let Some(tables) = root.get("tables").and_then(Value::as_object) else {
        return result;
    };
    let Some(sessions) = tables.get("sessions").and_then(Value::as_object) else {
        return result;
    };
    for (id, raw) in sessions {
        let Some(record) = raw.as_object() else {
            continue;
        };
        let identity = record.get("identity").and_then(Value::as_object);
        let rows = record.get("rows").and_then(Value::as_object);
        let title = rows
            .and_then(|rows| rows.get("title"))
            .and_then(Value::as_object)
            .and_then(|row| row.get("val"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty());
        let last_prompt_at = rows
            .and_then(|rows| rows.get("sessionListMetadata"))
            .and_then(Value::as_object)
            .and_then(|row| row.get("val"))
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("lastPromptAt"))
            .and_then(Value::as_i64);
        result.insert(
            id.clone(),
            ProjectionMeta {
                title,
                created_at: identity.and_then(|identity| number_field(identity, "createdAt")),
                last_prompt_at,
            },
        );
    }
    result
}

fn session_for(id: &str, meta: Option<&ProjectionMeta>) -> ArchiveSession {
    let fallback_title = if id.len() > 16 {
        format!("会话 {}…", &id[..16])
    } else {
        format!("会话 {id}")
    };
    let (title, created_at, last_prompt_at) = meta
        .map(|meta| {
            (
                meta.title.clone().unwrap_or(fallback_title.clone()),
                meta.created_at,
                meta.last_prompt_at,
            )
        })
        .unwrap_or((fallback_title, None, None));
    ArchiveSession {
        session_id: id.to_string(),
        title,
        created_at,
        last_activity_at: last_prompt_at.or(created_at),
    }
}

fn build_snapshot(
    data: WorkspaceData,
    projections: HashMap<String, ProjectionMeta>,
    running: bool,
    plugin_available: bool,
    status: Option<String>,
) -> ArchivesSnapshot {
    let archived: HashSet<&str> = data.archived_ids.iter().map(String::as_str).collect();
    let mut assigned = HashSet::new();
    let mut groups = Vec::new();
    for workspace_id in data.workspace_ids {
        let Some(workspace) = data.workspaces.get(&workspace_id) else {
            continue;
        };
        let sessions = workspace
            .session_ids
            .iter()
            .filter(|id| archived.contains(id.as_str()) && assigned.insert((*id).clone()))
            .map(|id| session_for(id, projections.get(id)))
            .collect::<Vec<_>>();
        if sessions.is_empty() {
            continue;
        }
        groups.push(ArchiveGroup {
            workspace_id: Some(workspace_id),
            title: workspace.title.clone(),
            path: Some(workspace.path.clone()),
            sessions,
        });
    }
    let orphan_sessions = data
        .archived_ids
        .iter()
        .filter(|id| !assigned.contains(*id))
        .map(|id| session_for(id, projections.get(id)))
        .collect::<Vec<_>>();
    if !orphan_sessions.is_empty() {
        groups.push(ArchiveGroup {
            workspace_id: None,
            title: "无项目".into(),
            path: None,
            sessions: orphan_sessions,
        });
    }
    let total = groups.iter().map(|group| group.sessions.len()).sum();
    ArchivesSnapshot {
        groups,
        total,
        running,
        plugin_available,
        restore_available: !running || plugin_available,
        delete_available: !running || plugin_available,
        status,
    }
}

fn plugin_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout_read(Duration::from_secs(2))
        .timeout_write(Duration::from_secs(2))
        .build()
}

fn plugin_url(settings: &SettingsSnapshot, route: &str) -> String {
    format!("http://127.0.0.1:{}{route}", settings.port)
}

fn response_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let detail = response.into_string().unwrap_or_default();
            if detail.is_empty() {
                format!("归档插件返回 HTTP {code}")
            } else {
                format!("归档插件返回 HTTP {code}: {detail}")
            }
        }
        other => format!("归档插件请求失败: {other}"),
    }
}

fn plugin_archived_ids(settings: &SettingsSnapshot) -> Result<Vec<String>, String> {
    let response = plugin_agent()
        .get(&plugin_url(settings, ARCHIVE_ROUTE))
        .call()
        .map_err(response_error)?;
    let value: Value = serde_json::from_str(
        &response
            .into_string()
            .map_err(|e| format!("读取归档插件响应失败:{e}"))?,
    )
    .map_err(|e| format!("解析归档插件响应失败:{e}"))?;
    string_array(
        value.get("archivedSessionIds"),
        "归档插件 archivedSessionIds",
    )
}

fn plugin_restore(settings: &SettingsSnapshot, session_id: &str) -> Result<(), String> {
    let response = plugin_agent()
        .post(&plugin_url(settings, RESTORE_ROUTE))
        .set("content-type", "application/json")
        .send_string(&serde_json::json!({ "sessionId": session_id }).to_string())
        .map_err(response_error)?;
    let value: Value = serde_json::from_str(
        &response
            .into_string()
            .map_err(|e| format!("读取归档恢复响应失败:{e}"))?,
    )
    .map_err(|e| format!("解析归档恢复响应失败:{e}"))?;
    if value.get("restoredSessionId").and_then(Value::as_str) != Some(session_id) {
        return Err("归档插件未确认恢复目标".into());
    }
    Ok(())
}

fn plugin_delete(settings: &SettingsSnapshot, session_id: &str) -> Result<(), String> {
    let response = plugin_agent()
        .post(&plugin_url(settings, DELETE_ROUTE))
        .set("content-type", "application/json")
        .send_string(&serde_json::json!({ "sessionId": session_id }).to_string())
        .map_err(response_error)?;
    let value: Value = serde_json::from_str(
        &response
            .into_string()
            .map_err(|e| format!("读取归档删除响应失败:{e}"))?,
    )
    .map_err(|e| format!("解析归档删除响应失败:{e}"))?;
    if value.get("deletedSessionId").and_then(Value::as_str) != Some(session_id) {
        return Err("归档插件未确认删除目标".into());
    }
    Ok(())
}

fn plugin_delete_all(settings: &SettingsSnapshot) -> Result<usize, String> {
    let response = plugin_agent()
        .post(&plugin_url(settings, DELETE_ALL_ROUTE))
        .set("content-type", "application/json")
        .send_string("{}")
        .map_err(response_error)?;
    let value: Value = serde_json::from_str(
        &response
            .into_string()
            .map_err(|e| format!("读取全部归档删除响应失败:{e}"))?,
    )
    .map_err(|e| format!("解析全部归档删除响应失败:{e}"))?;
    value
        .get("deletedCount")
        .and_then(Value::as_u64)
        .map(|count| count as usize)
        .ok_or_else(|| "归档插件未确认全部删除结果".into())
}

fn atomic_write_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的父目录", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("创建归档存储目录失败:{}: {e}", parent.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".{}-tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace.json"),
        std::process::id(),
        stamp
    ));
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|e| format!("序列化 workspace.json 失败:{e}"))?;
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("创建归档临时文件失败:{}: {e}", temp.display()))?;
        file.write_all(&bytes)
            .map_err(|e| format!("写入归档临时文件失败:{e}"))?;
        file.sync_all()
            .map_err(|e| format!("同步归档临时文件失败:{e}"))?;
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("替换 workspace.json 前删除旧文件失败:{e}"))?;
        }
        std::fs::rename(&temp, path).map_err(|e| format!("替换 workspace.json 失败:{e}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Stop-only fallback: remove one id from workspace.json and preserve all other JSON fields.
pub fn restore_archive_file(path: &Path, session_id: &str) -> Result<bool, String> {
    let mut root = load_json(path, "workspace.json")?;
    let root_object = object(&root, "workspace.json")?;
    let global = root_object
        .get("global")
        .and_then(Value::as_object)
        .ok_or_else(|| "workspace.json.global 缺失或不是对象".to_string())?;
    let mut ids = string_array(
        global.get("archivedSessionIds"),
        "global.archivedSessionIds",
    )?;
    if !ids.iter().any(|id| id == session_id) {
        return Ok(false);
    }
    ids.retain(|id| id != session_id);
    root.get_mut("global")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "workspace.json.global 不可写".to_string())?
        .insert(
            "archivedSessionIds".into(),
            Value::Array(ids.into_iter().map(Value::String).collect()),
        );
    atomic_write_json(path, &root)?;
    Ok(true)
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 256
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn session_dirs(root: &Path, session_id: &str) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let entries = std::fs::read_dir(root)
        .map_err(|e| format!("读取会话存储目录失败:{}: {e}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取会话项目目录失败:{e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取会话项目目录类型失败:{e}"))?;
        if !file_type.is_dir() {
            continue;
        }
        let candidate = entry.path().join(session_id);
        if candidate.is_dir() {
            result.push(candidate);
        }
    }
    Ok(result)
}

fn remove_session_artifacts(root: &Path, session_id: &str) -> Result<(), String> {
    for dir in session_dirs(root, session_id)? {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("删除会话日志目录失败:{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// Stop-only permanent deletion. Removes the archive marker, workspace slots,
/// projection-cache row, and JSONL session directory while keeping unrelated
/// domain fields intact.
pub fn delete_archive_files(
    workspace_path: &Path,
    projection_path: &Path,
    sessions_root: &Path,
    requested_ids: &[String],
) -> Result<usize, String> {
    if requested_ids.iter().any(|id| !valid_session_id(id)) {
        return Err("会话 ID 非法".into());
    }
    let mut root = load_json(workspace_path, "workspace.json")?;
    let root_object = object(&root, "workspace.json")?;
    let global = root_object
        .get("global")
        .and_then(Value::as_object)
        .ok_or_else(|| "workspace.json.global 缺失或不是对象".to_string())?;
    let archived = string_array(
        global.get("archivedSessionIds"),
        "global.archivedSessionIds",
    )?;
    let requested = requested_ids.iter().collect::<HashSet<_>>();
    let targets = archived
        .iter()
        .filter(|id| requested.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(0);
    }
    let target_set: HashSet<&str> = targets.iter().map(String::as_str).collect();

    root.get_mut("global")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "workspace.json.global 不可写".to_string())?
        .insert(
            "archivedSessionIds".into(),
            Value::Array(
                archived
                    .into_iter()
                    .filter(|id| !target_set.contains(id.as_str()))
                    .map(Value::String)
                    .collect(),
            ),
        );
    let tables = root
        .get_mut("tables")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "workspace.json.tables 不可写".to_string())?;
    let workspaces = tables
        .get_mut("workspaces")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "workspace.json.tables.workspaces 不可写".to_string())?;
    for record in workspaces.values_mut() {
        let Some(record) = record.as_object_mut() else {
            return Err("workspace.json.tables.workspaces 包含非法记录".into());
        };
        if let Some(session_ids) = record.get_mut("sessionIds") {
            let ids = session_ids.as_array_mut().ok_or_else(|| {
                "workspace.json.tables.workspaces.sessionIds 必须是数组".to_string()
            })?;
            ids.retain(|id| {
                id.as_str()
                    .map(|id| !target_set.contains(id))
                    .unwrap_or(true)
            });
        }
    }
    atomic_write_json(workspace_path, &root)?;

    if projection_path.exists() {
        let mut projection = load_json(projection_path, "session_projcache.json")?;
        let tables = projection
            .get_mut("tables")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "session_projcache.json.tables 不可写".to_string())?;
        if let Some(sessions) = tables.get_mut("sessions").and_then(Value::as_object_mut) {
            for id in &targets {
                sessions.remove(id);
            }
        }
        atomic_write_json(projection_path, &projection)?;
    }
    for id in &targets {
        remove_session_artifacts(sessions_root, id)?;
    }
    Ok(targets.len())
}

/// Read and group all archived sessions. The live plugin ID set wins when DSH is running.
pub fn get_snapshot(
    settings: &SettingsSnapshot,
    running: bool,
) -> Result<ArchivesSnapshot, String> {
    let workspace = load_json(&workspace_path(settings), "workspace.json")?;
    let mut data = parse_workspace(&workspace)?;
    let projections = load_json(&projection_path(settings), "session_projcache.json")
        .map(|value| parse_projections(&value))
        .unwrap_or_default();
    let (plugin_available, status) = if running {
        match plugin_archived_ids(settings) {
            Ok(ids) => {
                data.archived_ids = ids;
                (true, None)
            }
            Err(error) => (false, Some(format!("归档恢复插件未启用或不可用：{error}"))),
        }
    } else {
        (false, None)
    };
    Ok(build_snapshot(
        data,
        projections,
        running,
        plugin_available,
        status,
    ))
}

/// Restore through the hot plugin while running, or through the JSON fallback while stopped.
pub fn restore(
    settings: &SettingsSnapshot,
    running: bool,
    session_id: &str,
) -> Result<ArchiveRestoreResult, String> {
    if session_id.trim().is_empty() {
        return Err("会话 ID 不能为空".into());
    }
    if running {
        plugin_restore(settings, session_id)?;
        return Ok(ArchiveRestoreResult {
            session_id: session_id.into(),
            hot: true,
        });
    }
    let changed = restore_archive_file(&workspace_path(settings), session_id)?;
    if !changed {
        return Err("会话不在归档列表中".into());
    }
    Ok(ArchiveRestoreResult {
        session_id: session_id.into(),
        hot: false,
    })
}

/// Permanently delete one archived session through the live plugin or stop-only JSON fallback.
pub fn delete(
    settings: &SettingsSnapshot,
    running: bool,
    session_id: &str,
) -> Result<ArchiveDeleteResult, String> {
    if !valid_session_id(session_id) {
        return Err("会话 ID 非法".into());
    }
    if running {
        plugin_delete(settings, session_id)?;
        return Ok(ArchiveDeleteResult {
            deleted_count: 1,
            hot: true,
        });
    }
    let deleted_count = delete_archive_files(
        &workspace_path(settings),
        &projection_path(settings),
        &sessions_path(settings),
        &[session_id.to_string()],
    )?;
    if deleted_count == 0 {
        return Err("会话不在归档列表中".into());
    }
    Ok(ArchiveDeleteResult {
        deleted_count,
        hot: false,
    })
}

/// Permanently delete all archived sessions through the live plugin or stop-only JSON fallback.
pub fn delete_all(
    settings: &SettingsSnapshot,
    running: bool,
) -> Result<ArchiveDeleteResult, String> {
    if running {
        return Ok(ArchiveDeleteResult {
            deleted_count: plugin_delete_all(settings)?,
            hot: true,
        });
    }
    let workspace = load_json(&workspace_path(settings), "workspace.json")?;
    let data = parse_workspace(&workspace)?;
    let deleted_count = delete_archive_files(
        &workspace_path(settings),
        &projection_path(settings),
        &sessions_path(settings),
        &data.archived_ids,
    )?;
    Ok(ArchiveDeleteResult {
        deleted_count,
        hot: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (PathBuf, Value, Value) {
        let root = std::env::temp_dir().join(format!(
            "dsh-archive-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let workspace = serde_json::json!({
            "unit": { "name": "workspace", "version": 2 },
            "global": { "initialized": true, "workspaceIds": ["w2", "w1"], "archivedSessionIds": ["orphan", "s2", "s1"] },
            "tables": { "workspaces": {
                "w1": { "path": "/one", "title": "one", "sessionIds": ["s1"], "createdAt": "", "updatedAt": "" },
                "w2": { "path": "/two", "title": "two", "sessionIds": ["s2"], "createdAt": "", "updatedAt": "" }
            }}
        });
        let projection = serde_json::json!({
            "tables": { "sessions": {
                "s1": { "identity": { "createdAt": 100 }, "rows": { "title": { "val": "First" }, "sessionListMetadata": { "val": { "lastPromptAt": 300 } } } },
                "s2": { "identity": { "createdAt": 200 }, "rows": { "title": { "val": "Second" } } }
            }}
        });
        (root, workspace, projection)
    }

    #[test]
    fn groups_by_workspace_order_and_puts_orphans_last() {
        let (_, workspace, projection) = fixture();
        let result = build_snapshot(
            parse_workspace(&workspace).unwrap(),
            parse_projections(&projection),
            false,
            false,
            None,
        );
        assert_eq!(
            result
                .groups
                .iter()
                .map(|group| group.title.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "one", "无项目"]
        );
        assert_eq!(result.groups[0].sessions[0].title, "Second");
        assert_eq!(result.groups[1].sessions[0].last_activity_at, Some(300));
        assert_eq!(result.total, 3);
    }

    #[test]
    fn stop_only_restore_is_atomic_and_preserves_workspace_records() {
        let (root, workspace, _) = fixture();
        let path = root.join("workspace.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&workspace).unwrap()).unwrap();
        assert!(restore_archive_file(&path, "s1").unwrap());
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            after["global"]["archivedSessionIds"],
            serde_json::json!(["orphan", "s2"])
        );
        assert_eq!(
            after["tables"]["workspaces"]["w1"]["sessionIds"],
            serde_json::json!(["s1"])
        );
        assert!(!restore_archive_file(&path, "s1").unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stop_only_delete_removes_archive_workspace_cache_and_session_directory() {
        let (root, workspace, projection) = fixture();
        let workspace_path = root.join("workspace.json");
        let projection_path = root.join("session_projcache.json");
        let sessions_root = root.join("sessions");
        let session_dir = sessions_root.join("project").join("s1");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("session.jsonl.zstd"), b"session").unwrap();
        std::fs::write(
            &workspace_path,
            serde_json::to_vec_pretty(&workspace).unwrap(),
        )
        .unwrap();
        std::fs::write(
            &projection_path,
            serde_json::to_vec_pretty(&projection).unwrap(),
        )
        .unwrap();

        assert_eq!(
            delete_archive_files(
                &workspace_path,
                &projection_path,
                &sessions_root,
                &["s1".into()],
            )
            .unwrap(),
            1
        );
        let after_workspace: Value =
            serde_json::from_str(&std::fs::read_to_string(&workspace_path).unwrap()).unwrap();
        let after_projection: Value =
            serde_json::from_str(&std::fs::read_to_string(&projection_path).unwrap()).unwrap();
        assert_eq!(
            after_workspace["global"]["archivedSessionIds"],
            serde_json::json!(["orphan", "s2"])
        );
        assert_eq!(
            after_workspace["tables"]["workspaces"]["w1"]["sessionIds"],
            serde_json::json!([])
        );
        assert!(after_projection["tables"]["sessions"].get("s1").is_none());
        assert!(!session_dir.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
