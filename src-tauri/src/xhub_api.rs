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
use crate::models::RepeatRule;
use crate::repo;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tauri::{Emitter, Manager, State};

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
pub(crate) static CAPABILITIES: &[Capability] = &[
    // 开外链用**窄权限 `open-url`**：`system` 只被 openExternal 用过，但它将来要留给
    // `system.*`（openUrl/openPath/openApp），所以拆出一个语义准确的窄权限，
    // 既避免作者为开个链接去要"系统级"权限，也避免以后 system 真扩权时收不回。
    Capability {
        namespace: "runtime", method: "openExternal", permission: Some("open-url"),
        handler: CapabilityHandler::Sync(runtime_open_external),
    },
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
        namespace: "runtime",
        method: "openPermissions",
        permission: None,
        handler: CapabilityHandler::Sync(runtime_open_permissions),
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
    // ----- data 读扩展：单条 get + 便签/提示词/标签 -----
    Capability {
        namespace: "data",
        method: "todos.get",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_todos_get),
    },
    Capability {
        namespace: "data",
        method: "resources.get",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_resources_get),
    },
    Capability {
        namespace: "data",
        method: "stickies.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_stickies_list),
    },
    Capability {
        namespace: "data",
        method: "detachedStickies.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_detached_stickies_list),
    },
    Capability {
        namespace: "data",
        method: "snippets.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_snippets_list),
    },
    Capability {
        namespace: "data",
        method: "snippets.get",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_snippets_get),
    },
    Capability {
        namespace: "data",
        method: "tags.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_tags_list),
    },
    Capability {
        namespace: "data",
        method: "tags.ofNote",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_tags_of_note),
    },
    // ----- data 写：消费 data:write 权限（授权给第三方扩展增删改宿主数据） -----
    Capability {
        namespace: "data",
        method: "notes.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_notes_create),
    },
    Capability {
        namespace: "data",
        method: "notes.update",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_notes_update),
    },
    Capability {
        namespace: "data",
        method: "notes.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_notes_delete),
    },
    Capability {
        namespace: "data",
        method: "todos.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_create),
    },
    Capability {
        namespace: "data",
        method: "todos.update",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_update),
    },
    Capability {
        namespace: "data",
        method: "todos.toggle",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_toggle),
    },
    Capability {
        namespace: "data",
        method: "todos.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_delete),
    },
    Capability {
        namespace: "data",
        method: "todos.schedule",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_schedule),
    },
    // ----- data 待办 v1：描述 / 置顶 / 周期 / 标签 -----
    Capability {
        namespace: "data",
        method: "todos.setDescription",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_set_description),
    },
    Capability {
        namespace: "data",
        method: "todos.setPinned",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_set_pinned),
    },
    Capability {
        namespace: "data",
        method: "todos.setRepeat",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_set_repeat),
    },
    Capability {
        namespace: "data",
        method: "todos.completeRecurring",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_complete_recurring),
    },
    Capability {
        namespace: "data",
        method: "todos.undoRecurring",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_undo_recurring),
    },
    Capability {
        namespace: "data",
        method: "todos.setTags",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todos_set_tags),
    },
    Capability {
        namespace: "data",
        method: "todos.expandOccurrences",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_todos_expand_occurrences),
    },
    Capability {
        namespace: "data",
        method: "todoTags.list",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_todo_tags_list),
    },
    Capability {
        namespace: "data",
        method: "todoTags.links",
        permission: Some("data:read"),
        handler: CapabilityHandler::Sync(data_todo_tags_links),
    },
    Capability {
        namespace: "data",
        method: "todoTags.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todo_tags_create),
    },
    Capability {
        namespace: "data",
        method: "todoTags.update",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todo_tags_update),
    },
    Capability {
        namespace: "data",
        method: "todoTags.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_todo_tags_delete),
    },
    Capability {
        namespace: "data",
        method: "stickies.save",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_stickies_save),
    },
    Capability {
        namespace: "data",
        method: "detachedStickies.save",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_detached_stickies_save),
    },
    Capability {
        namespace: "data",
        method: "resources.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_resources_create),
    },
    Capability {
        namespace: "data",
        method: "resources.update",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_resources_update),
    },
    Capability {
        namespace: "data",
        method: "resources.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_resources_delete),
    },
    Capability {
        namespace: "data",
        method: "snippets.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_snippets_create),
    },
    Capability {
        namespace: "data",
        method: "snippets.update",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_snippets_update),
    },
    Capability {
        namespace: "data",
        method: "snippets.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_snippets_delete),
    },
    Capability {
        namespace: "data",
        method: "snippets.togglePin",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_snippets_toggle_pin),
    },
    Capability {
        namespace: "data",
        method: "tags.create",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_tags_create),
    },
    Capability {
        namespace: "data",
        method: "tags.delete",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_tags_delete),
    },
    Capability {
        namespace: "data",
        method: "tags.setNoteTags",
        permission: Some("data:write"),
        handler: CapabilityHandler::Sync(data_tags_set_note_tags),
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
        namespace: "fs",
        method: "saveText",
        permission: Some("fs"),
        handler: CapabilityHandler::Sync(fs_save_text),
    },
    Capability {
        namespace: "fs",
        method: "saveFile",
        permission: Some("fs"),
        handler: CapabilityHandler::Sync(fs_save_file),
    },
    Capability {
        namespace: "fs",
        method: "saveAs",
        permission: Some("fs"),
        handler: CapabilityHandler::Async(fs_save_as),
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
///
/// 目录一律走 `resolve_ext_dir`（开发扩展 → 源码目录，已装扩展 → 扩展根）：
/// 直接 `extensions_root().join(ext_id)` 会让**开发扩展**的桥 API 全部 NOT_FOUND。
fn load_manifest(app: &tauri::AppHandle, ext_id: &str) -> Option<ExtensionManifest> {
    let dir = crate::ext_protocol::resolve_ext_dir(app, ext_id).ok()?;
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

/// storage 文件路径：`<扩展目录>/.storage.json`（隔离，随扩展卸载可清除）。
///
/// ⚠️ 必须经 `resolve_ext_dir` 解析：开发扩展的数据落在**源码目录**，已装扩展落在扩展根。
/// 曾用 `extensions_root().join(id)`，于是开发扩展一调 `storage.set`，`write_storage` 的
/// `create_dir_all` 就在扩展根下凭空建出 `<id>/` 目录——下次扫描把它当成一个没有 manifest
/// 的扩展（invalid，显示「读取 manifest.json 失败」），又因 id 相同把**真正的开发扩展挤掉**
/// （scan_extensions 的同 id 冲突检查），列表里就只剩一条点不开的「不可用」。
fn storage_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(crate::ext_protocol::resolve_ext_dir(app, ext_id)?.join(".storage.json"))
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

/// 扩展用户配置覆盖文件路径：`<扩展目录>/.config.json`（配置分层的「用户覆盖」层；目录解析见 storage_path 注释）
fn config_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(crate::ext_protocol::resolve_ext_dir(app, ext_id)?.join(".config.json"))
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

/// 扩展部署覆盖文件路径：`<扩展目录>/.deploy-config.json`（部署方放置，优先级最高；目录解析见 storage_path 注释）
fn deploy_config_path(app: &tauri::AppHandle, ext_id: &str) -> Result<PathBuf, String> {
    Ok(crate::ext_protocol::resolve_ext_dir(app, ext_id)?.join(".deploy-config.json"))
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

fn runtime_open_external(app: &tauri::AppHandle, _state: &DbState, ext_id: &str, args: Value) -> Result<Value, String> {
    let url = args.get("url").and_then(Value::as_str).ok_or("INVALID_ARGUMENT: 缺少 URL")?;
    // 链接打开方式（扩展详情弹窗「链接打开方式」可配）：browser = 系统默认浏览器；
    // 其余/未配置 = inapp，走应用内置浏览器窗口（suda 浏览器池，ADR 0011）
    let config = crate::config::load();
    let mode = config
        .extension_link_modes
        .get(ext_id)
        .map(String::as_str)
        .unwrap_or("inapp");
    if mode == "browser" {
        crate::process::open_external(url.to_string())?;
    } else {
        crate::suda_browser::suda_browser_open_url(app.clone(), url.to_string())?;
    }
    Ok(Value::Null)
}

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

/// runtime.openPermissions：请求宿主打开本扩展的设置/授权弹窗（无需权限——
/// 只能打开自己的设置，不暴露任何数据）。实现为广播 `open-extension-settings`
/// 事件，主窗 index.vue 收到后切到扩展中心并打开该扩展的设置弹窗。
/// 典型用途：service 后端未授权（PERMISSION_DENIED）时扩展页给「去授权」跳转。
fn runtime_open_permissions(
    _app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    // 需要发事件而非纯回值：设置弹窗挂在主窗前端的扩展中心里
    let _ = _app.emit("open-extension-settings", ext_id.to_string());
    Ok(json!({ "opened": true }))
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
        let proxy = app.state::<crate::proxy::ProxyState>();
        if proxy.port > 0 {
            Some(proxy.prefix(ext_id)?)
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

/// data 写方法通用脚手架：锁连接执行闭包，落库后 emit 变更事件让宿主主窗口 UI 刷新。
/// 解析可选整型参数：缺失 / null → None；存在但非整数（字符串/浮点/对象等）报
/// INVALID_ARGUMENT——静默降级会改变语义（expectedVersion 失效、dueAt 被当「清除排期」、
/// parentId 掉成顶级待办），fail-fast 比静默改写安全。
fn expect_opt_i64(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("INVALID_ARGUMENT: {key} 必须为整数")),
    }
}

/// 必填整数数组（如 tagIds）：缺失报错；元素非整数 fail-fast（静默丢弃会改变语义）。
fn expect_i64_array(args: &Value, key: &str) -> Result<Vec<i64>, String> {
    let arr = args
        .get(key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("INVALID_ARGUMENT: 缺少 {key}（应为整数数组）"))?;
    arr.iter()
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| format!("INVALID_ARGUMENT: {key} 元素必须为整数"))
        })
        .collect()
}

/// 必填非空字符串（trim 后非空）：与宿主命令同强度校验——扩展是不可信输入源，
/// 空/纯空白标题会制造列表空白条目。
fn expect_non_empty(args: &Value, key: &str) -> Result<String, String> {
    let s = args
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("INVALID_ARGUMENT: 缺少 {key}"))?
        .trim()
        .to_owned();
    if s.is_empty() {
        return Err(format!("INVALID_ARGUMENT: {key} 不能为空"));
    }
    Ok(s)
}

/// 权限检查已在 dispatch 统一做（data:write）。event 为前端监听的事件名
/// （如 todos-changed / notes-changed / stickies-changed / snippets-changed；resources 无独立事件传 None）。
fn data_write<F>(
    app: &tauri::AppHandle,
    state: &DbState,
    event: Option<&str>,
    f: F,
) -> Result<Value, String>
where
    F: FnOnce(&Connection) -> Result<Value, String>,
{
    let events: Vec<&str> = event.into_iter().collect();
    data_write_events(app, state, &events, f)
}

/// 同 `data_write`，但落库后按顺序 emit 多个变更事件。
/// 需要它的场合：一次写入同时改变了两个前端缓存域——例如删标签既改了标签定义
/// （`todo-tags-changed` → refreshTodoTags）又改了待办关联（`todos-changed`），
/// 只发一个会让另一个缓存留着旧数据。
fn data_write_events<F>(
    app: &tauri::AppHandle,
    state: &DbState,
    events: &[&str],
    f: F,
) -> Result<Value, String>
where
    F: FnOnce(&Connection) -> Result<Value, String>,
{
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let result = f(&conn)?;
    drop(conn);
    for ev in events {
        let _ = app.emit(*ev, ());
    }
    Ok(result)
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

/// 扩展桥资源载荷统一剥离 remark：备注里存的是用户密码（secret.rs 加密落盘），
/// 持有 data:read/write 权限的扩展不该读到明文；说明与备注标签非敏感，保留
fn strip_resource_remark(mut r: crate::models::Resource) -> crate::models::Resource {
    r.remark = None;
    r
}

fn data_resources_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let resources = repo::resource::list_all(conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(strip_resource_remark)
            .collect::<Vec<_>>();
        serde_json::to_value(resources).map_err(|e| e.to_string())
    })
}

// ---------- data 读扩展：单条 get + 便签/提示词/标签（需 data:read） ----------

fn data_todos_get(
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
        let todo = repo::todo::get(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_resources_get(
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
        let resource = repo::resource::get(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(strip_resource_remark(resource)).map_err(|e| e.to_string())
    })
}

fn data_stickies_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let stickies = repo::sticky::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(stickies).map_err(|e| e.to_string())
    })
}

fn data_detached_stickies_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let stickies = repo::detached_sticky::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(stickies).map_err(|e| e.to_string())
    })
}

fn data_snippets_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let snippets = repo::snippet::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(snippets).map_err(|e| e.to_string())
    })
}

fn data_snippets_get(
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
        let snippet = repo::snippet::get(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(snippet).map_err(|e| e.to_string())
    })
}

fn data_tags_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let tags = repo::tag::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(tags).map_err(|e| e.to_string())
    })
}

fn data_tags_of_note(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let note_id = args
        .get("noteId")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 noteId".to_string())?;
    data_read(state, |conn| {
        let tags = repo::tag::tags_of_note(conn, note_id).map_err(|e| e.to_string())?;
        serde_json::to_value(tags).map_err(|e| e.to_string())
    })
}

// ---------- data 写：增删改宿主数据（需 data:write） ----------

fn data_notes_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let title = expect_non_empty(&args, "title")?;
    data_write(app, state, Some("notes-changed"), |conn| {
        let note = repo::note::create(conn, &title).map_err(|e| e.to_string())?;
        serde_json::to_value(note).map_err(|e| e.to_string())
    })
}

fn data_notes_update(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let title = expect_non_empty(&args, "title")?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_write(app, state, Some("notes-changed"), |conn| {
        let note = repo::note::update(conn, id, &title, &content).map_err(|e| e.to_string())?;
        serde_json::to_value(note).map_err(|e| e.to_string())
    })
}

fn data_notes_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_write(app, state, Some("notes-changed"), |conn| {
        repo::note::delete(conn, id).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

fn data_todos_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let title = expect_non_empty(&args, "title")?;
    let parent_id = expect_opt_i64(&args, "parentId")?;
    let created_at = args
        .get("createdAt")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::create(conn, &title, parent_id, created_at.as_deref())
            .map_err(|e| e.to_string())?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_update(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let title = expect_non_empty(&args, "title")?;
    let priority = args.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
    if !(0..=2).contains(&priority) {
        return Err("INVALID_ARGUMENT: 优先级取值 0-2".to_string());
    }
    // 乐观锁：期望版本（局域网同步冲突检测）；缺省不校验
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::update_with_version(conn, id, &title, priority, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_toggle(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::toggle_with_version(conn, id, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        repo::todo::delete_with_version(conn, id, expected)?;
        Ok(Value::Null)
    })
}

fn data_todos_schedule(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let due_at = expect_opt_i64(&args, "dueAt")?;
    let remind_at = expect_opt_i64(&args, "remindAt")?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::schedule_with_version(conn, id, due_at, remind_at, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

// ---------- data 写扩展：待办升级（描述 / 置顶 / 周期 / 标签） ----------

/// 解析扩展传入的周期规则对象（camelCase 键）；非对象报错，不静默降级成 once。
fn parse_repeat_rule(v: &Value) -> Result<RepeatRule, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "INVALID_ARGUMENT: repeat 必须是对象".to_string())?;
    let mode = obj
        .get("mode")
        .and_then(|m| m.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: repeat.mode 缺失".to_string())?
        .to_string();
    let rule = RepeatRule {
        mode,
        every: expect_opt_i64(v, "every")?,
        unit: obj.get("unit").and_then(|u| u.as_str()).map(str::to_owned),
        weekdays: expect_opt_i64(v, "weekdays")?,
        month_day: expect_opt_i64(v, "monthDay")?,
        month_nth: expect_opt_i64(v, "monthNth")?,
        end_mode: obj
            .get("endMode")
            .and_then(|e| e.as_str())
            .map(str::to_owned),
        end_at: expect_opt_i64(v, "endAt")?,
        count: expect_opt_i64(v, "count")?,
    };
    rule.validate()?;
    Ok(rule)
}

fn data_todos_set_description(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 description".to_string())?
        .to_string();
    if description.chars().count() > repo::todo::MAX_DESCRIPTION_LEN {
        return Err(format!(
            "INVALID_ARGUMENT: 描述过长（最多 {} 字）",
            repo::todo::MAX_DESCRIPTION_LEN
        ));
    }
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo =
            repo::todo::set_description_with_version(conn, id, &description, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_set_pinned(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let pinned = args
        .get("pinned")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 pinned".to_string())?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::set_pinned_with_version(conn, id, pinned, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_set_repeat(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let repeat = args
        .get("repeat")
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 repeat".to_string())?;
    let mut rule = parse_repeat_rule(repeat)?;
    rule.normalize();
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::set_repeat_with_version(conn, id, &rule, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

/// 周期待办「完成本轮」：due_at 滚动到下一个未来时刻、计数 +1、子待办复位
fn data_todos_complete_recurring(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::complete_recurring_with_version(conn, id, now_ms, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

fn data_todos_undo_recurring(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("todos-changed"), |conn| {
        let todo = repo::todo::undo_recurring_with_version(conn, id, expected)?;
        serde_json::to_value(todo).map_err(|e| e.to_string())
    })
}

/// 全量设置某条待办的标签（tagIds 必须为整数数组）
fn data_todos_set_tags(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let tag_ids = expect_i64_array(&args, "tagIds")?;
    // 与宿主命令 set_todo_tags 同口径：只改关联，但标签定义缓存也要跟着刷新
    data_write_events(app, state, &["todos-changed", "todo-tags-changed"], |conn| {
        repo::todo_tag::set_todo_tags(conn, id, &tag_ids).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

fn data_todo_tags_list(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let tags = repo::todo_tag::list(conn).map_err(|e| e.to_string())?;
        serde_json::to_value(tags).map_err(|e| e.to_string())
    })
}

/// 待办-标签全量关联（前端/扩展构建筛选映射用）
fn data_todo_tags_links(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    _args: Value,
) -> Result<Value, String> {
    data_read(state, |conn| {
        let links = repo::todo_tag::list_links(conn).map_err(|e| e.to_string())?;
        let out: Vec<Value> = links
            .into_iter()
            .map(|(todo_id, tag_id)| serde_json::json!({ "todo_id": todo_id, "tag_id": tag_id }))
            .collect();
        Ok(Value::Array(out))
    })
}

fn data_todo_tags_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let name = expect_non_empty(&args, "name")?;
    let color = args
        .get("color")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_write(app, state, Some("todo-tags-changed"), |conn| {
        let tag = repo::todo_tag::create(conn, &name, &color).map_err(|e| e.to_string())?;
        serde_json::to_value(tag).map_err(|e| e.to_string())
    })
}

fn data_todo_tags_update(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let name = expect_non_empty(&args, "name")?;
    let color = args
        .get("color")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_write(app, state, Some("todo-tags-changed"), |conn| {
        let tag = repo::todo_tag::update(conn, id, &name, &color).map_err(|e| e.to_string())?;
        serde_json::to_value(tag).map_err(|e| e.to_string())
    })
}

fn data_todo_tags_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    // 删标签会级联删关联行：标签定义与待办关联两个缓存域都要刷新（宿主命令同样发两个）
    data_write_events(app, state, &["todo-tags-changed", "todos-changed"], |conn| {
        repo::todo_tag::delete(conn, id).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

/// 展开区间内的周期待办虚拟实例（只读；规则只实现于 Rust 侧）
fn data_todos_expand_occurrences(
    _app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let from_ms = args
        .get("fromMs")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 fromMs".to_string())?;
    let to_ms = args
        .get("toMs")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 toMs".to_string())?;
    // 区间上限：扩展能传任意大区间，而展开全程持着数据库互斥锁（见 validate_range 注释）
    crate::todo_recurrence::validate_range(from_ms, to_ms)?;
    data_read(state, |conn| {
        let todos = repo::todo::list_recurring_candidates(conn).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for t in todos {
            let Some(due) = t.due_at else { continue };
            let rule = RepeatRule::from_todo(&t);
            for at_ms in crate::todo_recurrence::expand_occurrences(
                &rule,
                due,
                t.repeat_done_count,
                from_ms,
                to_ms,
            )? {
                out.push(serde_json::json!({ "todo_id": t.id, "at_ms": at_ms }));
            }
        }
        // 与宿主命令 expand_todo_occurrences 同口径：按时刻升序。
        // 两条路径必须给出同一顺序，否则扩展日历与宿主日历的格子排布会不一致。
        out.sort_by_key(|o| o["at_ms"].as_i64().unwrap_or(0));
        Ok(Value::Array(out))
    })
}

/// stickies.save：slot 无记录时自动建档（upsert 语义，version 从 0 起算）；
/// 已有记录时 expectedVersion 命中才写并 +1（缺省不校验）。
fn data_stickies_save(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let slot = args
        .get("slot")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 slot".to_string())?;
    if !(1..=2).contains(&slot) {
        return Err("INVALID_ARGUMENT: 便签槽位取值 1-2".to_string());
    }
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("stickies-changed"), |conn| {
        let sticky =
            repo::sticky::upsert_with_version(conn, slot, &content, expected)?;
        serde_json::to_value(sticky).map_err(|e| e.to_string())
    })
}

/// detachedStickies.save：仅更新「已脱离为主卡浮窗」的记录（update 语义），slot 无记录
/// 报 NOT_FOUND、不自动建档——浮窗便签由主卡「脱离」动作创建，扩展不应凭空造出浮窗。
fn data_detached_stickies_save(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let slot = args
        .get("slot")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 slot".to_string())?;
    if !(1..=2).contains(&slot) {
        return Err("INVALID_ARGUMENT: 便签槽位取值 1-2".to_string());
    }
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expected = expect_opt_i64(&args, "expectedVersion")?;
    data_write(app, state, Some("stickies-changed"), |conn| {
        repo::detached_sticky::update_content_with_version(conn, slot, &content, expected)?;
        Ok(Value::Null)
    })
}

/// 解析资源类型字符串（app/web/file）到 ResourceKind；非法值报错
fn parse_resource_kind(v: &Value) -> Result<crate::models::ResourceKind, String> {
    let s = v
        .as_str()
        .ok_or_else(|| "INVALID_ARGUMENT: kind 必须为字符串 app/web/file".to_string())?;
    match s {
        "app" => Ok(crate::models::ResourceKind::App),
        "web" => Ok(crate::models::ResourceKind::Web),
        "file" => Ok(crate::models::ResourceKind::File),
        _ => Err(format!("INVALID_ARGUMENT: 未知资源类型 {s}")),
    }
}

fn data_resources_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let kind = parse_resource_kind(
        args.get("kind")
            .ok_or_else(|| "INVALID_ARGUMENT: 缺少 kind".to_string())?,
    )?;
    let name = expect_non_empty(&args, "name")?;
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 target".to_string())?;
    let category = args.get("category").and_then(|v| v.as_str()).map(str::to_owned);
    let zone_id = args.get("zoneId").and_then(|v| v.as_i64());
    let icon = args.get("icon").and_then(|v| v.as_str()).map(str::to_owned);
    let extra = args.get("args").and_then(|v| v.as_str()).map(str::to_owned);
    data_write(app, state, None, |conn| {
        if let Some(zid) = zone_id {
            if !repo::zone::exists(conn, zid).map_err(|e| e.to_string())? {
                return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
            }
        }
        let resource = repo::resource::create(
            conn,
            kind,
            &name,
            &target,
            category.as_deref(),
            icon.as_deref(),
            extra.as_deref(),
            zone_id,
            // 扩展不得写入备注（密码）与说明：一律空
            None,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
        serde_json::to_value(strip_resource_remark(resource)).map_err(|e| e.to_string())
    })
}

fn data_resources_update(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let kind = parse_resource_kind(
        args.get("kind")
            .ok_or_else(|| "INVALID_ARGUMENT: 缺少 kind".to_string())?,
    )?;
    let name = expect_non_empty(&args, "name")?;
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 target".to_string())?;
    let category = args.get("category").and_then(|v| v.as_str()).map(str::to_owned);
    // 与 category 同款「全对象写」语义：调用方不传 zoneId 即写为未分区
    let zone_id = args.get("zoneId").and_then(|v| v.as_i64());
    let icon = args.get("icon").and_then(|v| v.as_str()).map(str::to_owned);
    let extra = args.get("args").and_then(|v| v.as_str()).map(str::to_owned);
    data_write(app, state, None, |conn| {
        if let Some(zid) = zone_id {
            if !repo::zone::exists(conn, zid).map_err(|e| e.to_string())? {
                return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
            }
        }
        // 「全对象写」语义下扩展不知道备注/说明字段：先读现值原样带回，防止扩展
        // 更新其它字段时把用户的备注（密码）与说明整体抹掉；扩展无路径改写它们。
        // 备注透传**落盘原值**（密文/旧明文），不做解密→重加密往返
        let existing = repo::resource::get(conn, id).map_err(|e| e.to_string())?;
        let remark_stored = repo::resource::remark_raw(conn, id).map_err(|e| e.to_string())?;
        let resource = repo::resource::update_with_stored_remark(
            conn,
            id,
            kind,
            &name,
            &target,
            category.as_deref(),
            icon.as_deref(),
            extra.as_deref(),
            zone_id,
            existing.description.as_deref(),
            remark_stored,
            existing.remark_label.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        serde_json::to_value(strip_resource_remark(resource)).map_err(|e| e.to_string())
    })
}

fn data_resources_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_write(app, state, None, |conn| {
        repo::resource::delete(conn, id).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

fn data_snippets_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let title = expect_non_empty(&args, "title")?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_write(app, state, Some("snippets-changed"), |conn| {
        let snippet = repo::snippet::create(conn, &title, &content).map_err(|e| e.to_string())?;
        serde_json::to_value(snippet).map_err(|e| e.to_string())
    })
}

fn data_snippets_update(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    let title = expect_non_empty(&args, "title")?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    data_write(app, state, Some("snippets-changed"), |conn| {
        let snippet = repo::snippet::update(conn, id, &title, &content).map_err(|e| e.to_string())?;
        serde_json::to_value(snippet).map_err(|e| e.to_string())
    })
}

fn data_snippets_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_write(app, state, Some("snippets-changed"), |conn| {
        repo::snippet::delete(conn, id).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

fn data_snippets_toggle_pin(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_write(app, state, Some("snippets-changed"), |conn| {
        let snippet = repo::snippet::toggle_pin(conn, id).map_err(|e| e.to_string())?;
        serde_json::to_value(snippet).map_err(|e| e.to_string())
    })
}

fn data_tags_create(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let name = expect_non_empty(&args, "name")?;
    data_write(app, state, None, |conn| {
        let tag = repo::tag::create(conn, &name).map_err(|e| e.to_string())?;
        serde_json::to_value(tag).map_err(|e| e.to_string())
    })
}

fn data_tags_delete(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 id".to_string())?;
    data_write(app, state, None, |conn| {
        repo::tag::delete(conn, id).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    })
}

fn data_tags_set_note_tags(
    app: &tauri::AppHandle,
    state: &DbState,
    _ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let note_id = args
        .get("noteId")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 noteId".to_string())?;
    // 逐项校验：set_note_tags 是「先清空再写入」的全量替换语义，静默丢弃非整数项
    // 会退化成「清空该笔记全部标签」且调用方无感知
    let tag_ids: Vec<i64> = match args.get("tagIds").and_then(|v| v.as_array()) {
        Some(arr) => {
            let mut ids = Vec::with_capacity(arr.len());
            for x in arr {
                match x.as_i64() {
                    Some(id) => ids.push(id),
                    None => return Err("INVALID_ARGUMENT: tagIds 必须为整数数组".to_string()),
                }
            }
            ids
        }
        None => return Err("INVALID_ARGUMENT: 缺少 tagIds".to_string()),
    };
    data_write(app, state, Some("notes-changed"), |conn| {
        repo::tag::set_note_tags(conn, note_id, &tag_ids).map_err(|e| e.to_string())?;
        Ok(Value::Null)
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

// ---------- fs（受控文件保存：写入系统下载目录，需 fs 权限） ----------
//
// 背景：Tauri/WebView2 下网页级下载（<a download> + blob 等）默认被宿主拦截
// （wry 仅在注册 download handler 时才监听 DownloadStarting，且默认参数禁用了
// WebView2 原生下载 UI），扩展页面的「下载」按钮会静默失效。这里提供走桥 API
// 的落盘通道：扩展把内容交宿主写盘，位置固定为系统下载目录，扩展不可指定
// 任意路径（完整的沙箱文件读写见 extension-api.md §6 后续规划）。

/// fs 保存字节量上限（解码后 64MB）：内容经 JSON+base64 过桥，限上限既防扩展
/// 误用打爆内存，也覆盖工具类扩展的导出场景。
const FS_SAVE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 清洗扩展提交的保存文件名：仅保留最后一段路径分量（杜绝路径穿越 / 绝对路径）、
/// 剔除 Windows 非法字符与 ASCII 控制符、修剪首尾空白与句点、限长 180；
/// 清洗后为空则回退带时间戳的默认名。
fn sanitize_save_name(raw: &str) -> String {
    let last_segment = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last_segment
        .chars()
        .filter(|c| !"<>:\"|?*".contains(*c) && !c.is_ascii_control())
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    let mut name: String = cleaned.chars().take(180).collect();
    if name.is_empty() || is_reserved_device_name(&name) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        name = format!("download-{ts}");
    }
    name
}

/// Windows 保留设备名（CON/PRN/AUX/NUL、COM0-9、LPT0-9，含带扩展名形式如 CON.txt）：
/// 以这类名字写盘会命中设备而非文件（如 COM1 可能无限阻塞写入线程），统一回退默认名
fn is_reserved_device_name(name: &str) -> bool {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.len() > prefix.len()
            && stem.starts_with(prefix)
            && stem[prefix.len()..].bytes().all(|b| b.is_ascii_digit())
    };
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
}

/// 目标路径冲突时自动追加序号：`name (1).ext`、`name (2).ext` …
fn dedupe_save_path(dir: &Path, name: &str) -> PathBuf {
    let base = dir.join(name);
    if !base.exists() {
        return base;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download")
        .to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    for i in 1u32..10_000 {
        let candidate = match &ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let p = dir.join(candidate);
        if !p.exists() {
            return p;
        }
    }
    // 理论兜底：同毫秒级高频冲突（几乎不可能），退时间戳名
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("download-{ts}"))
}

/// fs.save* 共用实现：清洗文件名 → 下载目录冲突改名 → 写盘。
/// 返回 `{ path, name }`（name 为最终落盘文件名，可能与请求名因冲突改名而不同）。
fn fs_save_bytes(
    app: &tauri::AppHandle,
    ext_id: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    if bytes.len() > FS_SAVE_MAX_BYTES {
        return Err("INVALID_ARGUMENT: 保存内容超过 64MB 上限".to_string());
    }
    let safe_name = sanitize_save_name(name);
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("IO_ERROR: 解析系统下载目录失败: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("IO_ERROR: 创建下载目录失败: {e}"))?;
    let path = dedupe_save_path(&dir, &safe_name);
    std::fs::write(&path, &bytes).map_err(|e| format!("IO_ERROR: 写入文件失败: {e}"))?;
    let final_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&safe_name)
        .to_string();
    log::info!("扩展保存文件: {ext_id} -> {}", path.display());
    Ok(json!({
        "path": path.to_string_lossy(),
        "name": final_name,
    }))
}

/// fs.saveText：把 UTF-8 文本保存为文件（需 `fs` 权限，dispatch 统一校验）。
fn fs_save_text(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 name".to_string())?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 content".to_string())?;
    fs_save_bytes(app, ext_id, name, content.as_bytes().to_vec())
}

/// fs.saveFile：把 base64 编码的二进制内容保存为文件（需 `fs` 权限）。
fn fs_save_file(
    app: &tauri::AppHandle,
    _state: &DbState,
    ext_id: &str,
    args: Value,
) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 name".to_string())?;
    let data = args
        .get("base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "INVALID_ARGUMENT: 缺少 base64".to_string())?;
    // 解码前先按 base64 膨胀系数（4/3）预检查，避免超大串先解码再拒收
    let max_b64 = (FS_SAVE_MAX_BYTES / 3 + 1) * 4;
    if data.len() > max_b64 {
        return Err("INVALID_ARGUMENT: 保存内容超过 64MB 上限".to_string());
    }
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("INVALID_ARGUMENT: base64 解码失败: {e}"))?;
    fs_save_bytes(app, ext_id, name, bytes)
}

/// fs.saveAs：把 base64 编码的二进制内容保存为文件，先弹系统「另存为」对话框
/// 由用户选择目录与文件名（需 `fs` 权限）。用户确认 → 写盘返回
/// `{ path, name, canceled: false }`；取消返回 `{ canceled: true }`（不写盘）。
fn fs_save_as(
    app: tauri::AppHandle,
    ext_id: String,
    args: Value,
) -> BoxFuture<Result<Value, String>> {
    Box::pin(async move {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "INVALID_ARGUMENT: 缺少 name".to_string())?;
        let data = args
            .get("base64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "INVALID_ARGUMENT: 缺少 base64".to_string())?;
        // 解码前先按 base64 膨胀系数（4/3）预检查，避免超大串先解码再拒收
        let max_b64 = (FS_SAVE_MAX_BYTES / 3 + 1) * 4;
        if data.len() > max_b64 {
            return Err("INVALID_ARGUMENT: 保存内容超过 64MB 上限".to_string());
        }
        // 弹原生保存对话框（spawn_blocking：对话框会一直等到用户操作，别占 tokio worker）
        let safe_name = sanitize_save_name(name);
        let picked = {
            let app = app.clone();
            let default_name = safe_name.clone();
            tauri::async_runtime::spawn_blocking(move || {
                use tauri_plugin_dialog::DialogExt as _;
                app.dialog()
                    .file()
                    .set_file_name(&default_name)
                    .blocking_save_file()
            })
            .await
            .map_err(|e| format!("INTERNAL: 保存对话框线程失败: {e}"))?
        };
        let Some(picked) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        let path: PathBuf = match picked {
            tauri_plugin_dialog::FilePath::Path(p) => p,
            tauri_plugin_dialog::FilePath::Url(u) => u
                .to_file_path()
                .map_err(|_| "IO_ERROR: 对话框未返回本地文件路径".to_string())?,
        };
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("INVALID_ARGUMENT: base64 解码失败: {e}"))?;
        if bytes.len() > FS_SAVE_MAX_BYTES {
            return Err("INVALID_ARGUMENT: 保存内容超过 64MB 上限".to_string());
        }
        std::fs::write(&path, &bytes).map_err(|e| format!("IO_ERROR: 写入文件失败: {e}"))?;
        let final_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&safe_name)
            .to_string();
        log::info!("扩展另存文件: {ext_id} -> {}", path.display());
        Ok(json!({
            "path": path.to_string_lossy(),
            "name": final_name,
            "canceled": false,
        }))
    })
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
        if !path.starts_with('/') || path.starts_with("//") || path.contains('\\') {
            return Err("INVALID_ARGUMENT: service 路径必须以单个 / 开头".into());
        }
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");
        let port = crate::service::service_port(&app, &ext_id)
            .ok_or_else(|| format!("NOT_FOUND: service 未启动（{ext_id}）"))?;
        let url = format!("http://127.0.0.1:{port}{path}");

        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
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
            author: None,
            config: Map::new(),
            module_variants: vec![],
            module_options: Default::default(),
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
    fn open_external_requires_open_url_permission() {
        // 语义对齐：打开外链归**窄权限 `open-url`**，不归「network：访问网络」
        // （否则任何带 network 的扩展都能往默认浏览器弹任意 https 页面钓鱼），
        // 也不归「system」（那会逼作者为开个链接去要系统级权限，且以后 system 扩权就收不回）。
        let cap = CAPABILITIES
            .iter()
            .find(|c| c.namespace == "runtime" && c.method == "openExternal")
            .expect("runtime.openExternal 必须在能力表中");
        assert_eq!(cap.permission, Some("open-url"));
        // 其余 runtime.* 仍然无需权限
        let info = CAPABILITIES
            .iter()
            .find(|c| c.namespace == "runtime" && c.method == "info")
            .expect("runtime.info 必须在能力表中");
        assert_eq!(info.permission, None);
        // openPermissions：打开自己的设置/授权弹窗，同样无需权限（只能开自己的，不暴露数据）
        let perms = CAPABILITIES
            .iter()
            .find(|c| c.namespace == "runtime" && c.method == "openPermissions")
            .expect("runtime.openPermissions 必须在能力表中");
        assert_eq!(perms.permission, None);
    }

    #[test]
    fn capabilities_cover_known_methods() {
        let keys: std::collections::HashSet<_> =
            CAPABILITIES.iter().map(|c| (c.namespace, c.method)).collect();
        assert!(keys.contains(&("runtime", "info")));
        assert!(keys.contains(&("storage", "get")));
        assert!(keys.contains(&("storage", "set")));
        // data 读
        assert!(keys.contains(&("data", "notes.list")));
        assert!(keys.contains(&("data", "notes.get")));
        assert!(keys.contains(&("data", "todos.list")));
        assert!(keys.contains(&("data", "todos.get")));
        assert!(keys.contains(&("data", "resources.list")));
        assert!(keys.contains(&("data", "resources.get")));
        assert!(keys.contains(&("data", "stickies.list")));
        assert!(keys.contains(&("data", "detachedStickies.list")));
        assert!(keys.contains(&("data", "snippets.list")));
        assert!(keys.contains(&("data", "tags.list")));
        // data 写
        assert!(keys.contains(&("data", "notes.create")));
        assert!(keys.contains(&("data", "notes.delete")));
        assert!(keys.contains(&("data", "todos.create")));
        assert!(keys.contains(&("data", "todos.toggle")));
        assert!(keys.contains(&("data", "todos.delete")));
        assert!(keys.contains(&("data", "todos.schedule")));
        // data 待办升级（描述 / 置顶 / 周期 / 标签）：漏一个键 = 扩展静默拿不到能力
        assert!(keys.contains(&("data", "todos.setDescription")));
        assert!(keys.contains(&("data", "todos.setPinned")));
        assert!(keys.contains(&("data", "todos.setRepeat")));
        assert!(keys.contains(&("data", "todos.setTags")));
        assert!(keys.contains(&("data", "todos.completeRecurring")));
        assert!(keys.contains(&("data", "todos.undoRecurring")));
        assert!(keys.contains(&("data", "todos.expandOccurrences")));
        assert!(keys.contains(&("data", "todoTags.list")));
        assert!(keys.contains(&("data", "todoTags.links")));
        assert!(keys.contains(&("data", "todoTags.create")));
        assert!(keys.contains(&("data", "todoTags.update")));
        assert!(keys.contains(&("data", "todoTags.delete")));
        assert!(keys.contains(&("data", "stickies.save")));
        assert!(keys.contains(&("data", "detachedStickies.save")));
        assert!(keys.contains(&("data", "resources.create")));
        assert!(keys.contains(&("data", "resources.delete")));
        assert!(keys.contains(&("data", "snippets.create")));
        assert!(keys.contains(&("data", "snippets.delete")));
        assert!(keys.contains(&("data", "tags.create")));
        assert!(keys.contains(&("data", "tags.setNoteTags")));
        assert!(keys.contains(&("service", "request")));
    }

    /// data 读方法（挂 data:read 权限）方法名约定：list / get / ofNote 结尾。
    fn is_data_read_method(method: &str) -> bool {
        method.ends_with(".list")
            || method.ends_with(".get")
            || method.ends_with(".ofNote")
            // 只读的关联/展开查询（不落库）
            || method.ends_with(".links")
            || method.ends_with(".expandOccurrences")
    }

    #[test]
    fn data_read_capabilities_declare_data_read_permission() {
        for c in CAPABILITIES {
            if c.namespace == "data" && is_data_read_method(c.method) {
                assert_eq!(
                    c.permission,
                    Some("data:read"),
                    "data read capability {}:{} must require data:read",
                    c.namespace,
                    c.method
                );
            }
        }
    }

    #[test]
    fn data_write_capabilities_declare_data_write_permission() {
        // 所有 data 能力必须二选一挂 data:read 或 data:write，禁止裸奔
        for c in CAPABILITIES {
            if c.namespace == "data" {
                assert!(
                    c.permission == Some("data:read") || c.permission == Some("data:write"),
                    "data capability {}:{} must require data:read or data:write",
                    c.namespace,
                    c.method
                );
            }
        }
        // 写方法必须挂 data:write
        for c in CAPABILITIES {
            if c.namespace == "data" && !is_data_read_method(c.method) {
                assert_eq!(
                    c.permission,
                    Some("data:write"),
                    "data write capability {}:{} must require data:write",
                    c.namespace,
                    c.method
                );
            }
        }
    }

    #[test]
    fn fs_capabilities_declare_fs_permission() {
        for c in CAPABILITIES {
            if c.namespace == "fs" {
                assert_eq!(c.permission, Some("fs"), "fs capability {}:{} must require fs", c.namespace, c.method);
            }
        }
    }

    #[test]
    fn sanitize_save_name_strips_traversal_and_illegal_chars() {
        // 路径穿越 / 绝对路径：只保留最后一段分量
        assert_eq!(sanitize_save_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_save_name("C:\\Users\\a\\out.txt"), "out.txt");
        assert_eq!(sanitize_save_name("/tmp/x/result.json"), "result.json");
        // Windows 非法字符被剔除
        assert_eq!(sanitize_save_name("a<b>c:d\"e|f?g*h.csv"), "abcdefgh.csv");
        // 首尾空白 / 句点修剪
        assert_eq!(sanitize_save_name("  report.  "), "report");
        // 空 / 全非法字符回退默认名（带时间戳前缀）
        let fallback = sanitize_save_name("");
        assert!(fallback.starts_with("download-"));
        let fallback2 = sanitize_save_name("???");
        assert!(fallback2.starts_with("download-"));
        // Windows 保留设备名（含带扩展名形式）回退默认名
        for reserved in ["CON", "con.txt", "NUL", "Com1", "lpt3.log"] {
            assert!(
                sanitize_save_name(reserved).starts_with("download-"),
                "reserved device name {reserved} must fall back"
            );
        }
        // 非保留名不受影响（stem 非保留词即可）
        assert_eq!(sanitize_save_name("console.txt"), "console.txt");
        // 正常名不动
        assert_eq!(sanitize_save_name("ctool-2026-08-29.txt"), "ctool-2026-08-29.txt");
        // 超长截断到 180 字符
        let long = "x".repeat(300);
        assert_eq!(sanitize_save_name(&long).chars().count(), 180);
    }

    #[test]
    fn dedupe_save_path_appends_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let first = dedupe_save_path(dir.path(), "out.txt");
        assert_eq!(first.file_name().unwrap(), "out.txt");
        std::fs::write(&first, b"x").unwrap();
        // 第二次同文名 → out (1).txt
        let second = dedupe_save_path(dir.path(), "out.txt");
        assert_eq!(second.file_name().unwrap(), "out (1).txt");
        std::fs::write(&second, b"x").unwrap();
        // 第三次 → out (2).txt
        let third = dedupe_save_path(dir.path(), "out.txt");
        assert_eq!(third.file_name().unwrap(), "out (2).txt");
        // 无扩展名文件同样适用
        let a = dedupe_save_path(dir.path(), "Makefile");
        std::fs::write(&a, b"x").unwrap();
        let b = dedupe_save_path(dir.path(), "Makefile");
        assert_eq!(b.file_name().unwrap(), "Makefile (1)");
    }
}
