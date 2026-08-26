//! 桥 API（`window.xhub.*`）宿主侧实现：统一分发命令 `xhub_call`。
//!
//! 调用链：扩展 iframe 内 `window.xhub.*` → `postMessage` 到主窗口 →
//! 主窗口 `invoke('xhub_call', { extId, namespace, method, args })` →
//! 这里按 (namespace, method) 查「能力表」分发。`ext_id` 由主窗口转发时携带，
//! 用于权限检查与 storage 隔离。
//!
//! 阶段 1（能力注册表化）：新增一个桥 API 能力 = 在下方 `CAPABILITIES` 表里
//! 加一行（namespace/method/权限/处理器）+ 写一个 handler 函数，无需再改
//! dispatch 的 match。权限检查统一在 `dispatch` 里做；`runtime.info` 返回完整
//! 能力清单，扩展可据此感知宿主能力并优雅降级。
//!
//! 说明：`theme.*` 与 `events.*` 由前端桥（useExtensionFrame.ts）直接回包/广播，
//! 不经 `xhub_call`，故不在本表；其能力清单见前端能力注册点。

use crate::commands::DbState;
use crate::extension::{extensions_root, ExtensionManifest, ExtensionRuntime};
use crate::repo;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tauri::{Manager, State};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// 同步处理器：借用 app/state/args。
type SyncHandler = fn(&tauri::AppHandle, &DbState, &str, Value) -> Result<Value, String>;
/// 异步处理器：owned 参数（跨 await 持有）。
type AsyncHandler = fn(tauri::AppHandle, String, Value) -> BoxFuture<Result<Value, String>>;

/// 桥 API 能力处理器（同步 / 异步二选一）。
pub enum CapabilityHandler {
    Sync(SyncHandler),
    Async(AsyncHandler),
}

/// 一个桥 API 能力：namespace + method + 权限要求 + 处理器。
///
/// `permission` 为 `Some(perm)` 时，dispatch 会校验该扩展的 manifest 声明了该权限
/// 且用户未显式关闭；`None` 表示无需权限（如 runtime.* / storage.* / service.request）。
pub struct Capability {
    pub namespace: &'static str,
    pub method: &'static str,
    pub permission: Option<&'static str>,
    pub handler: CapabilityHandler,
}

/// 全部桥 API 能力表。新增能力只改这里 + 补 handler 函数。
static CAPABILITIES: &[Capability] = &[
    Capability {
        namespace: "runtime",
        method: "info",
        permission: None,
        handler: CapabilityHandler::Sync(runtime_info),
    },
    Capability {
        namespace: "runtime",
        method: "callExtension",
        permission: None,
        handler: CapabilityHandler::Sync(runtime_call_extension),
    },
    Capability {
        namespace: "storage",
        method: "get",
        permission: None,
        handler: CapabilityHandler::Sync(storage_get),
    },
    Capability {
        namespace: "storage",
        method: "set",
        permission: None,
        handler: CapabilityHandler::Sync(storage_set),
    },
    Capability {
        namespace: "storage",
        method: "remove",
        permission: None,
        handler: CapabilityHandler::Sync(storage_remove),
    },
    Capability {
        namespace: "storage",
        method: "clear",
        permission: None,
        handler: CapabilityHandler::Sync(storage_clear),
    },
    Capability {
        namespace: "data",
        method: "notes.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_notes_list),
    },
    Capability {
        namespace: "data",
        method: "notes.get",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_notes_get),
    },
    Capability {
        namespace: "data",
        method: "todos.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_todos_list),
    },
    Capability {
        namespace: "data",
        method: "resources.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_resources_list),
    },
    Capability {
        namespace: "config",
        method: "all",
        permission: None,
        handler: CapabilityHandler::Sync(config_all),
    },
    Capability {
        namespace: "config",
        method: "get",
        permission: None,
        handler: CapabilityHandler::Sync(config_get),
    },
    Capability {
        namespace: "config",
        method: "set",
        permission: None,
        handler: CapabilityHandler::Sync(config_set),
    },
    Capability {
        namespace: "config",
        method: "remove",
        permission: None,
        handler: CapabilityHandler::Sync(config_remove),
    },
    Capability {
        namespace: "events",
        method: "emit",
        permission: Some("events"),
        handler: CapabilityHandler::Sync(events_emit),
    },
    Capability {
        namespace: "sharedStorage",
        method: "get",
        permission: Some("shared-storage"),
        handler: CapabilityHandler::Sync(shared_storage_get),
    },
    Capability {
        namespace: "sharedStorage",
        method: "set",
        permission: Some("shared-storage"),
        handler: CapabilityHandler::Sync(shared_storage_set),
    },
    Capability {
        namespace: "sharedStorage",
        method: "remove",
        permission: Some("shared-storage"),
        handler: CapabilityHandler::Sync(shared_storage_remove),
    },
    Capability {
        namespace: "service",
        method: "request",
        permission: None,
        handler: CapabilityHandler::Async(service_request),
    },
];

/// 暴露全部能力表（供 extension.rs 在扫描时做 requires 能力校验）。
pub fn capabilities() -> &'static [Capability] {
    CAPABILITIES
}

/// 加载扩展 manifest（不存在或解析失败返回 None）
fn load_manifest(app: &tauri::AppHandle, ext_id: &str) -> Option<ExtensionManifest> {
    let dir = extensions_root(app).ok()?.join(ext_id);
    let content = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str::<ExtensionManifest>(&content).ok()
}

/// manifest 是否声明了某权限（纯函数，供单测）
fn declares(manifest: &ExtensionManifest, perm: &str) -> bool {
    manifest.permissions.iter().any(|p| p == perm)
}

/// 校验扩展声明了某权限且未被用户关闭；否则返回带 PERMISSION_DENIED 前缀的 Err。
/// 权限检查已从各 handler 上移到 dispatch 统一执行。
fn require_permission(
    app: &tauri::AppHandle,
    ext_id: &str,
    perm: &str,
) -> Result<(), String> {
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    if !declares(&manifest, perm) {
        return Err(format!("PERMISSION_DENIED: 需要 {perm} 权限"));
    }
    if !crate::extension::permission_granted(app, ext_id, perm) {
        return Err(format!("PERMISSION_DENIED: 权限 {perm} 已被用户关闭"));
    }
    Ok(())
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

/// 扩展用户配置覆盖文件路径：`extensions/<id>/.config.json`（配置分层的「用户覆盖」层）
fn config_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(extensions_root(app)?.join(ext_id).join(".config.json"))
}

fn read_user_config(app: &tauri::AppHandle, ext_id: &str) -> Map<String, Value> {
    let path = match config_path(app, ext_id) {
        Ok(p) => p,
        Err(_) => return Map::new(),
    };
    if !path.exists() {
        return Map::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_user_config(
    app: &tauri::AppHandle,
    ext_id: &str,
    map: &Map<String, Value>,
) -> Result<(), String> {
    let path = config_path(app, ext_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content =
        serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 扩展部署覆盖文件路径：`extensions/<id>/.deploy-config.json`（部署方放置，优先级最高）
fn deploy_config_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(extensions_root(app)?.join(ext_id).join(".deploy-config.json"))
}

fn read_deploy_config(app: &tauri::AppHandle, ext_id: &str) -> Map<String, Value> {
    let path = match deploy_config_path(app, ext_id) {
        Ok(p) => p,
        Err(_) => return Map::new(),
    };
    if !path.exists() {
        return Map::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// 跨扩展共享存储文件路径：`extensions/.shared-storage.json`（opt-in，需 `shared-storage` 权限）
fn shared_storage_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(extensions_root(app)?.join(".shared-storage.json"))
}

fn read_shared_storage(app: &tauri::AppHandle) -> Result<Map<String, Value>, String> {
    let path = shared_storage_path(app)?;
    if !path.exists() {
        return Ok(Map::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Ok(Map::new()),
    }
}

fn write_shared_storage(app: &tauri::AppHandle, map: &Map<String, Value>) -> Result<(), String> {
    let path = shared_storage_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content =
        serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 桥 API 统一分发命令（Tauri 命令；参数由主窗口以 camelCase 转发：extId/namespace/method/args）
/// async：service.request 走 reqwest 网络转发，避免阻塞主线程。
#[tauri::command]
pub async fn xhub_call(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    ext_id: String,
    namespace: String,
    method: String,
    args: Value,
) -> Result<Value, String> {
    dispatch(&app, &state, &ext_id, &namespace, &method, args).await
}

/// 查能力表分发：找 (namespace, method) → 统一权限检查 → 调处理器。
async fn dispatch(
    app: &tauri::AppHandle,
    state: &DbState,
    ext_id: &str,
    namespace: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let cap = CAPABILITIES
        .iter()
        .find(|c| c.namespace == namespace && c.method == method)
        .ok_or_else(|| format!("INVALID_ARGUMENT: 未知方法 {namespace}.{method}"))?;

    // 统一权限检查（handler 内不再各自检查）
    if let Some(perm) = cap.permission {
        require_permission(app, ext_id, perm)?;
    }

    match cap.handler {
        CapabilityHandler::Sync(f) => f(app, state, ext_id, args),
        CapabilityHandler::Async(f) => f(app.clone(), ext_id.to_string(), args).await,
    }
}

// ---------- runtime ----------

/// runtime.callExtension：校验目标扩展 manifest.expose 是否包含该方法（跨扩展调用白名单）。
/// 实际的请求-响应路由在前端完成（调用方 iframe → 主窗口 → 目标 iframe）。
fn runtime_call_extension(
    app: &tauri::AppHandle,
    _state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let target_id = args
        .get("targetId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 targetId".to_string())?;
    let method = args
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 method".to_string())?;
    let target = load_manifest(app, target_id)
        .ok_or_else(|| format!("NOT_FOUND: 扩展 {target_id} 不存在"))?;
    if !target.expose.iter().any(|m| m == method) {
        return Err(format!(
            "PERMISSION_DENIED: 扩展 {target_id} 未暴露方法 {method}"
        ));
    }
    Ok(Value::Null)
}

/// runtime.info：扩展身份 + service 就绪态 + 代理前缀 + 宿主能力清单。
/// 能力清单让扩展能感知「宿主提供哪些方法、各自需要什么权限」，缺能力时优雅降级而非裸报错。
fn runtime_info(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    let runtime = match manifest.runtime {
        ExtensionRuntime::Web => "web",
        ExtensionRuntime::Service => "service",
    };
    let is_service = manifest.runtime == ExtensionRuntime::Service;
    // 代理前缀：service 扩展返回完整 URL（前端可直接 fetch/WebSocket 该前缀访问后端）
    let proxy_prefix = if is_service {
        let proxy_port = app.state::<crate::proxy::ProxyState>().0;
        if proxy_port > 0 {
            Some(format!("http://127.0.0.1:{proxy_port}/svc/{ext_id}"))
        } else {
            None
        }
    } else {
        None
    };

    let capabilities: Vec<Value> = CAPABILITIES
        .iter()
        .map(|c| {
            json!({
                "namespace": c.namespace,
                "method": c.method,
                "permission": c.permission,
            })
        })
        .collect();

    Ok(json!({
        "id": manifest.id,
        "name": manifest.name,
        "version": manifest.version,
        "runtime": runtime,
        // service 就绪 = 后端进程已启动且探活成功（打开扩展时懒启动）
        "serviceReady": is_service && crate::service::service_ready(app, ext_id),
        "proxyPrefix": proxy_prefix,
        "capabilities": capabilities,
    }))
}

// ---------- storage ----------

fn storage_get(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let map = read_storage(app, ext_id)?;
    Ok(map.get(key).cloned().unwrap_or(Value::Null))
}

fn storage_set(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
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

fn storage_remove(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let mut map = read_storage(app, ext_id)?;
    map.remove(key);
    write_storage(app, ext_id, &map)?;
    Ok(Value::Null)
}

fn storage_clear(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    write_storage(app, ext_id, &Map::new())?;
    Ok(Value::Null)
}

// ---------- data（读，需 data:read 权限，权限检查已在 dispatch 统一做） ----------

/// data 读方法通用脚手架：锁数据库连接并执行闭包（权限检查已上移 dispatch）。
fn data_read<F>(state: &DbState, f: F) -> Result<Value, String>
where
    F: FnOnce(&Connection) -> Result<Value, String>,
{
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

fn data_notes_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let notes = repo::note::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(notes).map_err(|e| e.to_string())
    })
}

fn data_notes_get(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_read(state, |conn| {
        let note = repo::note::get(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(note).map_err(|e| e.to_string())
    })
}

fn data_todos_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let todos = repo::todo::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(todos).map_err(|e| e.to_string())
    })
}

fn data_resources_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let resources = repo::resource::list_all(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(resources).map_err(|e| e.to_string())
    })
}

// ---------- config（扩展配置：manifest.config 默认 ∪ 用户覆盖，用户覆盖优先） ----------

/// 合并后的完整配置：manifest.config 默认 → 用户覆盖（.config.json）→ 部署覆盖（.deploy-config.json，最高优先级）。
fn config_all(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    let mut merged = manifest.config.clone();
    for (k, v) in read_user_config(app, ext_id) {
        merged.insert(k, v);
    }
    for (k, v) in read_deploy_config(app, ext_id) {
        merged.insert(k, v);
    }
    Ok(Value::Object(merged))
}

fn config_get(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    // 优先级：部署覆盖 > 用户覆盖 > manifest 默认
    let deploy = read_deploy_config(app, ext_id);
    if let Some(v) = deploy.get(key) {
        return Ok(v.clone());
    }
    let user = read_user_config(app, ext_id);
    if let Some(v) = user.get(key) {
        return Ok(v.clone());
    }
    let manifest =
        load_manifest(app, ext_id).ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 不存在"))?;
    Ok(manifest.config.get(key).cloned().unwrap_or(Value::Null))
}

fn config_set(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let value = args.get("value").cloned().unwrap_or(Value::Null);
    let mut user = read_user_config(app, ext_id);
    user.insert(key.to_string(), value);
    write_user_config(app, ext_id, &user)?;
    Ok(Value::Null)
}

fn config_remove(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let mut user = read_user_config(app, ext_id);
    user.remove(key);
    write_user_config(app, ext_id, &user)?;
    Ok(Value::Null)
}

// ---------- sharedStorage（跨扩展共享键值，opt-in 需 shared-storage 权限） ----------

fn shared_storage_get(
    app: &tauri::AppHandle,
    _state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let map = read_shared_storage(app)?;
    Ok(map.get(key).cloned().unwrap_or(Value::Null))
}

fn shared_storage_set(
    app: &tauri::AppHandle,
    _state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let value = args.get("value").cloned().unwrap_or(Value::Null);
    let mut map = read_shared_storage(app)?;
    map.insert(key.to_string(), value);
    write_shared_storage(app, &map)?;
    Ok(Value::Null)
}

fn shared_storage_remove(
    app: &tauri::AppHandle,
    _state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 key".to_string())?;
    let mut map = read_shared_storage(app)?;
    map.remove(key);
    write_shared_storage(app, &map)?;
    Ok(Value::Null)
}

// ---------- events（扩展间事件总线；emit 权限校验在 dispatch，广播在前端） ----------

/// events.emit：只做权限校验占位（dispatch 已统一校验 events 权限）。
/// 实际的事件广播在前端 `broadcastExtensionEvent` 完成（跨 iframe postMessage）。
fn events_emit(
    _app: &tauri::AppHandle,
    _state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    Ok(Value::Null)
}

// ---------- service ----------

/// service 扩展调用自身受托管后端（非流式代理转发，无需权限）。
/// 返回 `{ status, headers, body }`；前端桥据此构造带 text()/json() 的 HttpResult。
fn service_request(app: tauri::AppHandle, ext_id: String, args: Value) -> BoxFuture<Result<Value, String>> {
    Box::pin(async move {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "INVALID_ARGUMENT: 缺少 path".to_string())?;
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");
        let port = crate::service::service_port(&app, &ext_id)
            .ok_or_else(|| format!("NOT_FOUND: service 未启动（{ext_id}）"))?;
        let url = format!("http://127.0.0.1:{port}{path}");

        let client = reqwest::Client::new();
        let mut req = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            other => return Err(format!("INVALID_ARGUMENT: 不支持的 method {other}")),
        };
        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }
        if let Some(body) = args.get("body").and_then(|v| v.as_str()) {
            req = req.body(body.to_string());
        }
        let resp = req.send().await.map_err(|e| format!("NETWORK_ERROR: {e}"))?;
        let status = resp.status().as_u16();
        let headers: HashMap<String, String> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        Ok(json!({ "status": status, "headers": headers, "body": body }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_checks_permission_presence() {
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
            requires: vec![],
            depends_on: vec![],
            disabled: None,
            expose: vec![],
            actions: vec![],
            icon: None,
            min_size: None,
            backend: None,
            description: String::new(),
            config: Map::new(),
        };
        assert!(declares(&manifest, "data:read"));
        assert!(!declares(&manifest, "data:write"));
    }

    #[test]
    fn capabilities_have_unique_keys() {
        let mut seen = std::collections::HashSet::new();
        for c in CAPABILITIES {
            assert!(
                !c.namespace.is_empty() && !c.method.is_empty(),
                "capability with empty namespace/method"
            );
            let key = (c.namespace, c.method);
            assert!(
                seen.insert(key),
                "duplicate capability: {}:{}",
                c.namespace,
                c.method
            );
        }
    }

    #[test]
    fn capabilities_cover_known_methods() {
        let keys: std::collections::HashSet<_> =
            CAPABILITIES.iter().map(|c| (c.namespace, c.method)).collect();
        assert!(keys.contains(&("runtime", "info")));
        assert!(keys.contains(&("storage", "get")));
        assert!(keys.contains(&("storage", "set")));
        assert!(keys.contains(&("data", "notes.list")));
        assert!(keys.contains(&("data", "todos.list")));
        assert!(keys.contains(&("data", "resources.list")));
        assert!(keys.contains(&("service", "request")));
    }

    #[test]
    fn data_read_capabilities_declare_data_read_permission() {
        for c in CAPABILITIES {
            if c.namespace == "data" {
                assert_eq!(
                    c.permission,
                    Some("data:read"),
                    "data capability {}:{} must require data:read",
                    c.namespace,
                    c.method
                );
            }
        }
    }
}
