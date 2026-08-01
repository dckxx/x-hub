use crate::config::AppConfig;
use crate::models::{Group, Note, Resource, ResourceKind, SearchResult};
use crate::process;
use crate::repo::{group, note, resource};
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Manager, State};

pub struct DbState(pub Mutex<Connection>);

#[tauri::command]
pub fn get_initial_data(state: State<'_, DbState>) -> Result<InitialData, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let groups = group::list(&conn).map_err(err_str)?;
    let resources = resource::list_all(&conn).map_err(err_str)?;
    let notes = note::list(&conn).map_err(err_str)?;
    let config = crate::config::load();
    Ok(InitialData {
        groups,
        resources,
        notes,
        config,
    })
}

#[derive(serde::Serialize)]
pub struct InitialData {
    pub groups: Vec<Group>,
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub config: AppConfig,
}

// ---------- 分组 ----------

#[tauri::command]
pub fn create_group(state: State<'_, DbState>, name: String) -> Result<Group, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    group::create(&conn, &name).map_err(err_str)
}

#[tauri::command]
pub fn update_group(state: State<'_, DbState>, id: i64, name: String) -> Result<Group, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    group::rename(&conn, id, &name).map_err(err_str)
}

#[tauri::command]
pub fn delete_group(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    group::delete(&conn, id).map_err(err_str)
}

#[tauri::command]
pub fn reorder_groups(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    group::reorder(&conn, &ids).map_err(err_str)
}

// ---------- 快捷资源 ----------

#[tauri::command]
pub fn create_resource(
    state: State<'_, DbState>,
    group_id: i64,
    kind: String,
    name: String,
    target: String,
    icon: Option<String>,
    args: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    resource::create(
        &conn,
        group_id,
        kind,
        &name,
        &target,
        icon.as_deref(),
        args.as_deref(),
    )
    .map_err(err_str)
}

#[tauri::command]
pub fn update_resource(
    state: State<'_, DbState>,
    id: i64,
    group_id: i64,
    kind: String,
    name: String,
    target: String,
    icon: Option<String>,
    args: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    resource::update(
        &conn,
        id,
        group_id,
        kind,
        &name,
        &target,
        icon.as_deref(),
        args.as_deref(),
    )
    .map_err(err_str)
}

#[tauri::command]
pub fn delete_resource(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::delete(&conn, id).map_err(err_str)
}

#[tauri::command]
pub fn reorder_resources(
    state: State<'_, DbState>,
    group_id: i64,
    ids: Vec<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::reorder(&conn, group_id, &ids).map_err(err_str)
}

#[tauri::command]
pub fn launch_resource(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    drop(conn);
    match res.kind {
        ResourceKind::App => process::launch_program(&res.target, res.args.as_deref()),
        ResourceKind::Web => process::open_url(&res.target),
    }
}

// ---------- 笔记 ----------

#[tauri::command]
pub fn create_note(state: State<'_, DbState>, title: String) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::create(&conn, &title).map_err(err_str)
}

#[tauri::command]
pub fn update_note(
    state: State<'_, DbState>,
    id: i64,
    title: String,
    content: String,
) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::update(&conn, id, &title, &content).map_err(err_str)
}

#[tauri::command]
pub fn delete_note(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::delete(&conn, id).map_err(err_str)
}

// ---------- 全局搜索 ----------

#[tauri::command]
pub fn search_all(state: State<'_, DbState>, keyword: String) -> Result<SearchResult, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::search(&conn, &keyword).map_err(err_str)?;
    let notes = note::search(&conn, &keyword).map_err(err_str)?;
    Ok(SearchResult { resources, notes })
}

// ---------- 配置 ----------

#[tauri::command]
pub fn get_config() -> AppConfig {
    crate::config::load()
}

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<AppConfig, String> {
    crate::config::save(&config)?;
    Ok(config)
}

#[derive(serde::Deserialize)]
pub struct WindowPosPayload {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub fn save_window_state(
    _state: State<'_, DbState>,
    payload: WindowPosPayload,
) -> Result<(), String> {
    let mut config = crate::config::load();
    config.window.x = payload.x;
    config.window.y = payload.y;
    config.window.width = payload.width;
    config.window.height = payload.height;
    crate::config::save(&config)
}

#[tauri::command]
pub fn set_window_always_on_top(window: tauri::WebviewWindow, value: bool) -> Result<(), String> {
    window
        .set_always_on_top(value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_always_on_top_config(value: bool) -> Result<(), String> {
    let mut config = crate::config::load();
    config.window.always_on_top = value;
    crate::config::save(&config)
}

// ---------- 窗口控制 ----------

#[tauri::command]
pub fn minimize_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn hide_to_tray(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::hide_window(&app);
    Ok(())
}

#[tauri::command]
pub fn toggle_window_visibility(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            crate::tray::hide_window(&app);
        } else {
            crate::tray::show_window(&app);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ---------- 工具 ----------

fn err_str(e: rusqlite::Error) -> String {
    format!("数据库错误: {}", e)
}

fn parse_kind(kind: &str) -> Result<ResourceKind, String> {
    match kind {
        "app" => Ok(ResourceKind::App),
        "web" => Ok(ResourceKind::Web),
        _ => Err(format!("未知资源类型: {}", kind)),
    }
}
