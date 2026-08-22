//! 桥 API（`window.xhub.*`）宿主侧实现：统一分发命令 `xhub_call`。
//!
//! 调用链：扩展 iframe 内 `window.xhub.*` → `postMessage` 到主窗口 →
//! 主窗口 `invoke('xhub_call', { extId, namespace, method, args })` →
//! 这里按 namespace/method 分发。`ext_id` 由主窗口转发时携带，用于权限检查与 storage 隔离。
//!
//! 一期范围（extension-api §13）：`runtime.*`、`storage.*`、`data`（读）。
//! 错误以 `CODE: message` 前缀返回，主窗口 bridge 据此构造 `XHubError.code`。

use crate::commands::DbState;
use crate::extension::{extensions_root, ExtensionManifest, ExtensionRuntime};
use crate::repo;
use crate::usage;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use tauri::State;

/// 加载扩展 manifest（不存在或解析失败返回 None）
fn load_manifest(app: &tauri::AppHandle, ext_id: &str) -> Option<ExtensionManifest> {
    let dir = extensions_root(app).ok()?.join(ext_id);
    let content = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str::<ExtensionManifest>(&content).ok()
}

/// 校验扩展是否声明了某权限；未声明返回带 PERMISSION_DENIED 前缀的 Err
fn require_permission(manifest: &ExtensionManifest, perm: &str) -> Result<(), String> {
    if manifest.permissions.iter().any(|p| p == perm) {
        Ok(())
    } else {
        Err(format!("PERMISSION_DENIED: 需要 {perm} 权限"))
    }
}

/// storage 文件路径：`extensions/<id>/.storage.json`（隔离，随扩展卸载可清除）
fn storage_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(extensions_root(app)?.join(ext_id).join(".storage.json"))
}

fn read_storage(app: &tauri::AppHandle, ext_id: &str) -> Result<Map<String, Value>, String> {
    let path = storage_path(app, ext_id)?;
    if !path.exists() {
        return Ok(Map::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Ok(Map::new()), // 非对象或损坏时视为空存储
    }
}

fn write_storage(app: &tauri::AppHandle, ext_id: &str, map: &Map<String, Value>) -> Result<(), String> {
    let path = storage_path(app, ext_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content =
        serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 桥 API 统一分发命令（Tauri 命令；参数由主窗口以 camelCase 转发：extId/namespace/method/args）
#[tauri::command]
pub fn xhub_call(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    ext_id: String,
    namespace: String,
    method: String,
    args: Value,
) -> Result<Value, String> {
    dispatch(&app, &state, &ext_id, &namespace, &method, args)
}

fn dispatch(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
    namespace: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    match (namespace, method) {
        ("runtime", "info") => runtime_info(app, ext_id),
        ("storage", "get") => storage_get(app, ext_id, args),
        ("storage", "set") => storage_set(app, ext_id, args),
        ("storage", "remove") => storage_remove(app, ext_id, args),
        ("storage", "clear") => storage_clear(app, ext_id),
        ("data", "notes.list") => data_notes_list(app, state, ext_id),
        ("data", "notes.get") => data_notes_get(app, state, ext_id, args),
        ("data", "todos.list") => data_todos_list(app, state, ext_id),
        ("data", "resources.list") => data_resources_list(app, state, ext_id),
        ("data", "usage.summary") => data_usage_summary(app, state, ext_id),
        _ => Err(format!("INVALID_ARGUMENT: 未知方法 {namespace}.{method}")),
    }
}

// ---------- runtime ----------

fn runtime_info(app: &tauri::AppHandle, ext_id: &str) -> Result<Value, String> {
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    let runtime = match manifest.runtime {
        ExtensionRuntime::Web => "web",
        ExtensionRuntime::Service => "service",
    };
    Ok(json!({
        "id": manifest.id,
        "name": manifest.name,
        "version": manifest.version,
        "runtime": runtime,
        // service 托管（§12.6）实现前恒为 false；实现后由宿主管进程状态决定
        "serviceReady": false,
        "proxyPrefix": if manifest.runtime == ExtensionRuntime::Service {
            Some(format!("/svc/{ext_id}"))
        } else {
            None
        },
    }))
}

// ---------- storage ----------

fn storage_get(app: &tauri::AppHandle, ext_id: &str, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let map = read_storage(app, ext_id)?;
    Ok(map.get(key).cloned().unwrap_or(Value::Null))
}

fn storage_set(app: &tauri::AppHandle, ext_id: &str, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let value = args.get("value").cloned().unwrap_or(Value::Null);
    let mut map = read_storage(app, ext_id)?;
    map.insert(key.to_string(), value);
    write_storage(app, ext_id, &map)?;
    Ok(Value::Null)
}

fn storage_remove(app: &tauri::AppHandle, ext_id: &str, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let mut map = read_storage(app, ext_id)?;
    map.remove(key);
    write_storage(app, ext_id, &map)?;
    Ok(Value::Null)
}

fn storage_clear(app: &tauri::AppHandle, ext_id: &str) -> Result<Value, String> {
    write_storage(app, ext_id, &Map::new())?;
    Ok(Value::Null)
}

// ---------- data（读，需 data:read 权限） ----------

/// data 读方法的通用脚手架：加载 manifest 做权限检查，再持有数据库连接执行闭包。
fn data_read<F>(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
    f: F,
) -> Result<Value, String>
where
    F: FnOnce(&Connection) -> Result<Value, String>,
{
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    require_permission(&manifest, "data:read")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

fn data_notes_list(app: &tauri::AppHandle, state: &DbState, ext_id: &str) -> Result<Value, String> {
    data_read(app, state, ext_id, |conn| {
        let notes = repo::note::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(notes).map_err(|e| e.to_string())
    })
}

fn data_notes_get(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_read(app, state, ext_id, |conn| {
        let note = repo::note::get(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(note).map_err(|e| e.to_string())
    })
}

fn data_todos_list(app: &tauri::AppHandle, state: &DbState, ext_id: &str) -> Result<Value, String> {
    data_read(app, state, ext_id, |conn| {
        let todos = repo::todo::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(todos).map_err(|e| e.to_string())
    })
}

fn data_resources_list(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
) -> Result<Value, String> {
    data_read(app, state, ext_id, |conn| {
        let resources = repo::resource::list_all(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(resources).map_err(|e| e.to_string())
    })
}

fn data_usage_summary(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
) -> Result<Value, String> {
    data_read(app, state, ext_id, |conn| {
        let summary = usage::query_summary(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(summary).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_permission_checks_presence() {
        let manifest = ExtensionManifest {
            id: "t".into(),
            name: "t".into(),
            version: "1".into(),
            runtime: ExtensionRuntime::Web,
            kind: "view".into(),
            surfaces: vec![],
            open_in: vec![],
            entry: Default::default(),
            permissions: vec!["data:read".into()],
            icon: None,
            min_size: None,
            backend: None,
            description: String::new(),
        };
        assert!(require_permission(&manifest, "data:read").is_ok());
        assert!(require_permission(&manifest, "data:write").is_err());
    }
}
