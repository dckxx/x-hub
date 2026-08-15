use crate::config::AppConfig;
use crate::models::{
    Countdown, DetachedSticky, Note, Resource, ResourceKind, SearchResult, Snippet, Sticky,
    SyncResult, Tag, Todo, UsageDetail, UsageSummary,
};
use crate::process;
use crate::repo::{countdown, detached_sticky, note, resource, snippet, sticky, tag, todo};
use crate::usage;
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

pub struct DbState(pub Mutex<Connection>);

#[tauri::command]
pub fn get_initial_data(state: State<'_, DbState>) -> Result<InitialData, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::list_all(&conn).map_err(err_str)?;
    let notes = note::list(&conn).map_err(err_str)?;
    let tags = tag::list(&conn).map_err(err_str)?;
    let todos = todo::list(&conn).map_err(err_str)?;
    let stickies = sticky::list(&conn).map_err(err_str)?;
    let detached = detached_sticky::list(&conn).map_err(err_str)?;
    let usage_summary = usage::query_summary(&conn).map_err(err_str)?;
    let countdowns = countdown::list(&conn).map_err(err_str)?;
    let config = crate::config::load();
    log::info!(
        "初始化数据加载完成: resources={} notes={} tags={} todos={} stickies={} detached={} countdowns={}",
        resources.len(),
        notes.len(),
        tags.len(),
        todos.len(),
        stickies.len(),
        detached.len(),
        countdowns.len()
    );
    Ok(InitialData {
        resources,
        notes,
        tags,
        todos,
        stickies,
        detached,
        countdowns,
        usage_summary,
        config,
    })
}

#[derive(serde::Serialize)]
pub struct InitialData {
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub tags: Vec<Tag>,
    pub todos: Vec<Todo>,
    pub stickies: Vec<Sticky>,
    pub detached: Vec<DetachedSticky>,
    pub countdowns: Vec<Countdown>,
    pub usage_summary: UsageSummary,
    pub config: AppConfig,
}

// ---------- 速达资源（应用 / 网页 / 文件合一） ----------

#[tauri::command]
pub fn create_resource(
    state: State<'_, DbState>,
    kind: String,
    name: String,
    target: String,
    category: Option<String>,
    icon: Option<String>,
    args: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    let res = resource::create(
        &conn,
        kind,
        &name,
        &target,
        category.as_deref(),
        icon.as_deref(),
        args.as_deref(),
    )
    .map_err(err_str)?;
    log::info!(
        "添加资源: {} ({:?}) category={:?}",
        res.name,
        res.kind,
        res.category
    );
    Ok(res)
}

#[tauri::command]
pub fn update_resource(
    state: State<'_, DbState>,
    id: i64,
    kind: String,
    name: String,
    target: String,
    category: Option<String>,
    icon: Option<String>,
    args: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    let res = resource::update(
        &conn,
        id,
        kind,
        &name,
        &target,
        category.as_deref(),
        icon.as_deref(),
        args.as_deref(),
    )
    .map_err(err_str)?;
    log::info!("更新资源: id={} {} ({:?})", res.id, res.name, res.kind);
    Ok(res)
}

#[tauri::command]
pub fn delete_resource(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::delete(&conn, id).map_err(err_str)?;
    log::info!("删除资源: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn reorder_resources(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::reorder(&conn, &ids).map_err(err_str)?;
    log::info!("资源排序更新: {:?}", ids);
    Ok(())
}

#[tauri::command]
pub fn launch_resource(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    match res.kind {
        ResourceKind::App => match process::launch_program(&res.target, res.args.as_deref()) {
            Ok(()) => {
                let _ = resource::touch(&conn, id);
                log::info!("启动程序: {} ({})", res.name, res.target);
                Ok(())
            }
            Err(e) => {
                log::error!("启动程序失败: {} ({}) -> {}", res.name, res.target, e);
                Err(e)
            }
        },
        ResourceKind::Web => match process::open_url(&res.target) {
            Ok(()) => {
                let _ = resource::touch(&conn, id);
                log::info!("打开网页: {} ({})", res.name, res.target);
                Ok(())
            }
            Err(e) => {
                log::error!("打开网页失败: {} ({}) -> {}", res.name, res.target, e);
                Err(e)
            }
        },
        ResourceKind::File => match process::open_path(&res.target) {
            Ok(()) => {
                let _ = resource::touch(&conn, id);
                log::info!("打开文件: {} ({})", res.name, res.target);
                Ok(())
            }
            Err(e) => {
                log::error!("打开文件失败: {} ({}) -> {}", res.name, res.target, e);
                Err(e)
            }
        },
    }
}

// ---------- 笔记 ----------

#[tauri::command]
pub fn create_note(state: State<'_, DbState>, title: String) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let note = note::create(&conn, &title).map_err(err_str)?;
    log::info!("新建笔记: id={} ({})", note.id, note.title);
    Ok(note)
}

#[tauri::command]
pub fn update_note(
    state: State<'_, DbState>,
    id: i64,
    title: String,
    content: String,
) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let note = note::update(&conn, id, &title, &content).map_err(err_str)?;
    log::debug!("更新笔记: id={} 内容 {} 字", id, content.chars().count());
    Ok(note)
}

#[tauri::command]
pub fn delete_note(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::delete(&conn, id).map_err(err_str)?;
    log::info!("删除笔记: id={}", id);
    Ok(())
}

// ---------- 待办清单 ----------

#[tauri::command]
pub fn list_todos(state: State<'_, DbState>) -> Result<Vec<Todo>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todos = todo::list(&conn).map_err(err_str)?;
    log::debug!("加载待办清单: {} 条", todos.len());
    Ok(todos)
}

#[tauri::command]
pub fn create_todo(state: State<'_, DbState>, title: String) -> Result<Todo, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("标题不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::create(&conn, t).map_err(err_str)?;
    log::info!("添加待办: id={} {}", todo.id, todo.title);
    Ok(todo)
}

#[tauri::command]
pub fn toggle_todo(state: State<'_, DbState>, id: i64) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::toggle(&conn, id).map_err(err_str)?;
    log::info!("切换待办状态: id={} done={}", todo.id, todo.done);
    Ok(todo)
}

#[tauri::command]
pub fn update_todo(
    state: State<'_, DbState>,
    id: i64,
    title: String,
    priority: i64,
) -> Result<Todo, String> {
    if !(0..=2).contains(&priority) {
        return Err("优先级取值 0-2".into());
    }
    let t = title.trim();
    if t.is_empty() {
        return Err("标题不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::update(&conn, id, t, priority).map_err(err_str)?;
    log::info!("更新待办: id={} {} (优先级 {})", todo.id, todo.title, todo.priority);
    Ok(todo)
}

#[tauri::command]
pub fn delete_todo(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    todo::delete(&conn, id).map_err(err_str)?;
    log::info!("删除待办: id={}", id);
    Ok(())
}

// ---------- 便签 ----------

#[tauri::command]
pub fn list_stickies(state: State<'_, DbState>) -> Result<Vec<Sticky>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let stickies = sticky::list(&conn).map_err(err_str)?;
    log::debug!("加载便签: {} 条", stickies.len());
    Ok(stickies)
}

#[tauri::command]
pub fn save_sticky(
    state: State<'_, DbState>,
    slot: i64,
    content: String,
) -> Result<Sticky, String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let sticky = sticky::upsert(&conn, slot, &content).map_err(err_str)?;
    log::debug!("保存便签: slot={} 内容 {} 字", slot, content.chars().count());
    Ok(sticky)
}

// ---------- 便签脱离浮窗 ----------

/// 列出所有已脱离的浮窗便签（启动恢复 / 前端同步用）
#[tauri::command]
pub fn get_detached_stickies(state: State<'_, DbState>) -> Result<Vec<DetachedSticky>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    detached_sticky::list(&conn).map_err(err_str)
}

/// 将便签卡脱离为系统级浮窗：
/// 复制原卡内容到 detached_stickies → 清空原卡 → 创建浮窗。
/// 若该卡已有浮窗则改为聚焦已有浮窗（每卡最多一个）。
/// 必须 async：同步命令运行在主线程，会与 WebviewWindow 创建互相阻塞（死锁）。
#[tauri::command]
pub async fn detach_sticky(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    slot: i64,
) -> Result<DetachedSticky, String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    // 已存在浮窗：聚焦并直接返回
    if let Some(existing) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        detached_sticky::get_by_slot(&conn, slot).map_err(err_str)?
    } {
        crate::sticky_window::focus(&app, slot);
        return Ok(existing);
    }

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let origin = sticky::get_by_slot(&conn, slot).map_err(err_str)?;
    let content = origin.map(|s| s.content).unwrap_or_default();

    let saved = detached_sticky::upsert(&conn, slot, &content, None, None, true).map_err(err_str)?;
    // 脱离 = 复制并清空原卡，之后各自独立
    sticky::upsert(&conn, slot, "").map_err(err_str)?;
    drop(conn);

    let win = crate::sticky_window::create_or_focus(&app, slot, saved.x, saved.y, true)
        .map_err(|e| format!("创建浮窗失败: {}", e))?;
    log::info!("便签脱离浮窗: slot={} 内容 {} 字", slot, content.chars().count());
    drop(win);

    Ok(saved)
}

/// 再次点击脱离 icon 时聚焦已有浮窗（无浮窗则返回 false）
#[tauri::command]
pub async fn focus_detached_sticky(app: tauri::AppHandle, slot: i64) -> Result<bool, String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    Ok(crate::sticky_window::focus(&app, slot))
}

/// 浮窗内容随输入保存（防抖由前端处理）
#[tauri::command]
pub fn save_detached_sticky(
    state: State<'_, DbState>,
    slot: i64,
    content: String,
) -> Result<(), String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    detached_sticky::update_content(&conn, slot, &content).map_err(err_str)?;
    log::debug!("保存浮窗便签: slot={} 内容 {} 字", slot, content.chars().count());
    Ok(())
}

/// 切换浮窗置顶（默认置顶，点击可切换）
#[tauri::command]
pub async fn toggle_detached_sticky_pin(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    slot: i64,
    always_on_top: bool,
) -> Result<(), String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    detached_sticky::update_pin(&conn, slot, always_on_top).map_err(err_str)?;
    drop(conn);
    if let Some(win) = app.get_webview_window(&crate::sticky_window::window_label(slot)) {
        let _ = win.set_always_on_top(always_on_top);
    }
    log::info!("浮窗置顶切换: slot={} 置顶={}", slot, always_on_top);
    Ok(())
}

/// 还原浮窗到主面板：写入空闲槽（slot1/2 哪个空写哪个），
/// 两个槽都有内容则失败（由前端改为询问删除）。还原成功后浮窗数据删除（收回）。
#[tauri::command]
pub async fn restore_detached_sticky(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    slot: i64,
) -> Result<i64, String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let detached = detached_sticky::get_by_slot(&conn, slot)
        .map_err(err_str)?
        .ok_or_else(|| "浮窗便签不存在".to_string())?;

    // 找空闲槽：优先原槽（脱离时已清空），否则另一个槽
    let occupied = |s: i64| -> bool {
        sticky::get_by_slot(&conn, s)
            .ok()
            .flatten()
            .map(|x| !x.content.trim().is_empty())
            .unwrap_or(false)
    };
    let target_slot = if !occupied(slot) {
        slot
    } else if !occupied(3 - slot) {
        3 - slot
    } else {
        return Err("两个便签槽都有内容，无法还原，只能删除".into());
    };

    let content = detached.content.clone();
    sticky::upsert(&conn, target_slot, &content).map_err(err_str)?;
    detached_sticky::delete_by_slot(&conn, slot).map_err(err_str)?;
    drop(conn);

    crate::sticky_window::destroy(&app, slot);
    log::info!("浮窗便签还原: slot={} -> 主面板槽位 {}", slot, target_slot);

    // 通知主窗口刷新便签数据
    let _ = app.emit("stickies-changed", ());
    Ok(target_slot)
}

/// 删除浮窗便签（浮窗数据彻底删除）
#[tauri::command]
pub async fn delete_detached_sticky(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    slot: i64,
) -> Result<(), String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    detached_sticky::delete_by_slot(&conn, slot).map_err(err_str)?;
    drop(conn);

    crate::sticky_window::destroy(&app, slot);
    log::info!("删除浮窗便签: slot={}", slot);
    Ok(())
}

// ---------- 倒计时 ----------

#[tauri::command]
pub fn list_countdowns(state: State<'_, DbState>) -> Result<Vec<Countdown>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let items = countdown::list(&conn).map_err(err_str)?;
    log::debug!("加载倒计时: {} 个", items.len());
    Ok(items)
}

#[tauri::command]
pub fn create_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    name: String,
    repeat_mode: String,
    end_at: i64,
    total_ms: i64,
    interval_minutes: Option<i64>,
) -> Result<Countdown, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("倒计时名称不能为空".into());
    }
    if !matches!(repeat_mode.as_str(), "once" | "daily" | "interval") {
        return Err("重复模式不合法".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let total = countdown::count(&conn).map_err(err_str)?;
    if total >= countdown::MAX_COUNTDOWNS {
        return Err(format!("最多只能创建 {} 个倒计时", countdown::MAX_COUNTDOWNS));
    }
    let c = countdown::create(&conn, n, &repeat_mode, end_at, total_ms, interval_minutes)
        .map_err(err_str)?;
    drop(conn);
    let _ = app.emit("countdowns-changed", ());
    log::info!(
        "创建倒计时: id={} name={} mode={} end_at={}",
        c.id,
        c.name,
        c.repeat_mode,
        c.end_at
    );
    Ok(c)
}

#[tauri::command]
pub fn update_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    name: String,
    repeat_mode: String,
    end_at: i64,
    total_ms: i64,
    interval_minutes: Option<i64>,
) -> Result<Countdown, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("倒计时名称不能为空".into());
    }
    if !matches!(repeat_mode.as_str(), "once" | "daily" | "interval") {
        return Err("重复模式不合法".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let c = countdown::update(&conn, id, n, &repeat_mode, end_at, total_ms, interval_minutes)
        .map_err(err_str)?;
    drop(conn);
    let _ = app.emit("countdowns-changed", ());
    log::info!("更新倒计时: id={} name={} mode={}", id, c.name, c.repeat_mode);
    Ok(c)
}

#[tauri::command]
pub fn delete_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    countdown::delete(&conn, id).map_err(err_str)?;
    drop(conn);
    crate::countdown_window::destroy(&app, id);
    let _ = app.emit("countdowns-changed", ());
    log::info!("删除倒计时: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn pause_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Countdown, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let c = countdown::pause(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("countdowns-changed", ());
    log::info!("暂停倒计时: id={} name={}", c.id, c.name);
    Ok(c)
}

#[tauri::command]
pub fn resume_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Countdown, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let c = countdown::resume(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("countdowns-changed", ());
    log::info!("恢复倒计时: id={} name={} 下次={}", c.id, c.name, c.end_at);
    Ok(c)
}

/// 浮窗浮起：持久化状态并创建独立圆窗。
/// 必须 async：同步命令运行在主线程，会与 WebviewWindow 创建互相阻塞（死锁），
/// 表现为主窗口卡死且浮窗不出现。
#[tauri::command]
pub async fn float_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Countdown, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let c = countdown::set_floated(&conn, id, true, None, None).map_err(err_str)?;
    drop(conn);
    crate::countdown_window::create_or_focus(&app, id, c.float_x, c.float_y)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("countdowns-changed", ());
    log::info!("倒计时浮窗浮起: id={} name={}", id, c.name);
    Ok(c)
}

/// 浮窗收起：销毁窗口并落盘位置
#[tauri::command]
pub async fn unfloat_countdown(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Countdown, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let c = countdown::set_floated(&conn, id, false, None, None).map_err(err_str)?;
    drop(conn);
    crate::countdown_window::destroy(&app, id);
    let _ = app.emit("countdowns-changed", ());
    log::info!("倒计时浮窗收起: id={} name={}", id, c.name);
    Ok(c)
}

// ---------- 提示词百宝箱 ----------

#[tauri::command]
pub fn list_snippets(state: State<'_, DbState>) -> Result<Vec<Snippet>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippets = snippet::list(&conn).map_err(err_str)?;
    log::debug!("加载提示词百宝箱: {} 条", snippets.len());
    Ok(snippets)
}

#[tauri::command]
pub fn create_snippet(
    state: State<'_, DbState>,
    title: String,
    content: String,
) -> Result<Snippet, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("标题不能为空".into());
    }
    let c = content.trim();
    if c.is_empty() {
        return Err("内容不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippet = snippet::create(&conn, t, c).map_err(err_str)?;
    log::info!("添加提示词: id={} {}", snippet.id, snippet.title);
    Ok(snippet)
}

#[tauri::command]
pub fn update_snippet(
    state: State<'_, DbState>,
    id: i64,
    title: String,
    content: String,
) -> Result<Snippet, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("标题不能为空".into());
    }
    let c = content.trim();
    if c.is_empty() {
        return Err("内容不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippet = snippet::update(&conn, id, t, c).map_err(err_str)?;
    log::info!("更新提示词: id={} {}", snippet.id, snippet.title);
    Ok(snippet)
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    snippet::delete(&conn, id).map_err(err_str)?;
    log::info!("删除提示词: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn toggle_snippet_pin(state: State<'_, DbState>, id: i64) -> Result<Snippet, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippet = snippet::toggle_pin(&conn, id).map_err(err_str)?;
    log::info!("切换提示词置顶: id={} pinned={}", snippet.id, snippet.is_pinned);
    Ok(snippet)
}

#[tauri::command]
pub fn record_snippet_copy(state: State<'_, DbState>, id: i64) -> Result<Snippet, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippet = snippet::record_copy(&conn, id).map_err(err_str)?;
    log::debug!("提示词已复制: id={} 累计 {} 次", snippet.id, snippet.copy_count);
    Ok(snippet)
}

// ---------- AI 用量统计 ----------

/// 同步 opencode 用量。`path` 可选，缺省自动探测。
/// 游标存于 AppConfig（usage_sync_cursor），避免频繁写库。
#[tauri::command]
pub fn sync_ai_usage(
    state: State<'_, DbState>,
    path: Option<String>,
) -> Result<SyncResult, String> {
    let mut config = crate::config::load();
    let resolved = path.or(config.usage_db_path.clone()).or_else(usage::probe_opencode_path);

    let Some(db_path) = resolved else {
        return Ok(SyncResult {
            inserted: 0,
            cursor: config.usage_sync_cursor,
            listening: false,
            path: None,
        });
    };

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let result = usage::sync_from_opencode(&conn, &db_path, config.usage_sync_cursor).map_err(err_str)?;
    // 推进游标并记录路径
    if result.inserted > 0 || config.usage_db_path.as_deref() != Some(db_path.as_str()) {
        config.usage_sync_cursor = result.cursor;
        config.usage_db_path = Some(db_path.clone());
        let _ = crate::config::save(&config);
    }
    log::info!(
        "同步 AI 用量: 新增 {} 条, 游标 {} (db: {})",
        result.inserted,
        result.cursor,
        db_path
    );
    Ok(result)
}

/// 用量汇总（含同步后返回）
#[tauri::command]
pub fn get_usage_summary(state: State<'_, DbState>) -> Result<UsageSummary, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let summary = usage::query_summary(&conn).map_err(err_str)?;
    log::debug!(
        "查询用量汇总: 今日 input={} cache={} output={}",
        summary.today_input,
        summary.today_cache_input,
        summary.today_output
    );
    Ok(summary)
}

/// 用量详情（趋势 + 排行 + 明细）
#[tauri::command]
pub fn get_usage_detail(
    state: State<'_, DbState>,
    days: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<UsageDetail, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let detail = usage::query_detail(
        &conn,
        days.unwrap_or(7).clamp(1, 90),
        limit.unwrap_or(50).clamp(1, 500),
        offset.unwrap_or(0).max(0),
    )
    .map_err(err_str)?;
    log::debug!("查询用量详情: {} 条记录", detail.total);
    Ok(detail)
}

// ---------- 全局搜索 ----------

#[tauri::command]
pub fn search_all(state: State<'_, DbState>, keyword: String) -> Result<SearchResult, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::search(&conn, &keyword).map_err(err_str)?;
    let notes = note::search(&conn, &keyword).map_err(err_str)?;
    let todos = todo::search(&conn, &keyword).map_err(err_str)?;
    log::debug!(
        "全局搜索「{}」: 资源 {} 条, 笔记 {} 条, 待办 {} 条",
        keyword,
        resources.len(),
        notes.len(),
        todos.len()
    );
    Ok(SearchResult {
        resources,
        notes,
        todos,
    })
}

/// 笔记-标签全量关联（列表筛选用）
#[tauri::command]
pub fn list_note_tags(state: State<'_, DbState>) -> Result<Vec<NoteTagRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let rows = tag::list_note_tags(&conn).map_err(err_str)?;
    Ok(rows
        .into_iter()
        .map(|(note_id, tag_id)| NoteTagRow { note_id, tag_id })
        .collect())
}

#[derive(serde::Serialize)]
pub struct NoteTagRow {
    pub note_id: i64,
    pub tag_id: i64,
}

// ---------- 配置 ----------

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<AppConfig, String> {
    crate::config::save(&config)?;
    log::info!(
        "配置已保存: theme={} window={}x{} always_on_top={}",
        config.theme,
        config.window.width,
        config.window.height,
        config.window.always_on_top
    );
    Ok(config)
}

#[tauri::command]
pub fn set_window_always_on_top(window: tauri::WebviewWindow, value: bool) -> Result<(), String> {
    window
        .set_always_on_top(value)
        .map_err(|e| e.to_string())?;
    log::info!("窗口置顶: {}", if value { "开" } else { "关" });
    Ok(())
}

#[tauri::command]
pub fn set_always_on_top_config(value: bool) -> Result<(), String> {
    let mut config = crate::config::load();
    config.window.always_on_top = value;
    crate::config::save(&config)?;
    log::info!("置顶配置持久化: {}", value);
    Ok(())
}

#[tauri::command]
pub fn get_global_shortcut() -> Result<String, String> {
    Ok(crate::config::load().global_shortcut)
}

#[tauri::command]
pub fn set_global_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    let shortcut = value.trim();
    if shortcut.is_empty() {
        return Err("快捷键不能为空".into());
    }

    let mut config = crate::config::load();
    let previous = config.global_shortcut.clone();
    if previous == shortcut {
        return Ok(config.global_shortcut);
    }

    // 同一物理按键组合仅换了写法（如 Windows 上 CommandOrControl→Ctrl），
    // 无需重新注册，直接更新存储的字符串
    if crate::shortcut::same_hotkey(&previous, shortcut) {
        config.global_shortcut = shortcut.to_string();
        crate::config::save(&config)?;
        return Ok(config.global_shortcut);
    }

    if crate::shortcut::is_shortcut_registered(&app, shortcut) {
        return Err("快捷键冲突".into());
    }

    if let Err(e) = crate::shortcut::unregister_toggle_shortcut(&app, &previous) {
        if !crate::shortcut::is_conflict_error(&e) {
            return Err(e);
        }
        return Err("快捷键冲突".into());
    }

    if let Err(e) = crate::shortcut::register_toggle_shortcut(&app, shortcut) {
        let mapped = crate::shortcut::format_shortcut_error(&e);
        if !mapped.eq(&e) {
            let _ = crate::shortcut::register_toggle_shortcut(&app, &previous);
        }
        return Err(mapped);
    }
    config.global_shortcut = shortcut.to_string();
    crate::config::save(&config)?;
    Ok(config.global_shortcut)
}

#[tauri::command]
pub fn log_client_error(message: String, detail: Option<String>) -> Result<(), String> {
    match detail {
        Some(detail) if !detail.trim().is_empty() => log::error!("{} | {}", message, detail),
        _ => log::error!("{}", message),
    }
    Ok(())
}

// ---------- 窗口控制 ----------

#[tauri::command]
pub fn minimize_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())?;
    log::info!("窗口最小化");
    Ok(())
}

#[tauri::command]
pub fn toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())?;
        log::info!("窗口还原");
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        log::info!("窗口最大化");
    }
    Ok(())
}

#[tauri::command]
pub fn hide_to_tray(app: tauri::AppHandle) -> Result<(), String> {
    crate::tray::hide_window(&app);
    log::info!("窗口隐藏至托盘");
    Ok(())
}

// ---------- 笔记标签 ----------

#[tauri::command]
pub fn list_tags(state: State<'_, DbState>) -> Result<Vec<Tag>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    tag::list(&conn).map_err(err_str)
}

#[tauri::command]
pub fn create_tag(state: State<'_, DbState>, name: String) -> Result<Tag, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = tag::create(&conn, &name).map_err(err_str)?;
    log::info!("创建标签: {} (id={})", t.name, t.id);
    Ok(t)
}

#[tauri::command]
pub fn delete_tag(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    tag::delete(&conn, id).map_err(err_str)?;
    log::info!("删除标签: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn get_note_tags(state: State<'_, DbState>, note_id: i64) -> Result<Vec<Tag>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    tag::tags_of_note(&conn, note_id).map_err(err_str)
}

#[tauri::command]
pub fn set_note_tags(
    state: State<'_, DbState>,
    note_id: i64,
    tag_ids: Vec<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    tag::set_note_tags(&conn, note_id, &tag_ids).map_err(err_str)?;
    log::debug!("设置笔记标签: note={} tags={:?}", note_id, tag_ids);
    Ok(())
}

// ---------- 文件链接 ----------

/// 检查拖入路径的基本信息（是否目录 / 名称），用于文件拖拽导入
#[tauri::command]
pub fn inspect_path(path: String) -> Result<PathInfo, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("无法访问路径: {}", e))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名")
        .to_string();
    log::debug!("路径检查: {} (is_dir={})", path, meta.is_dir());
    Ok(PathInfo {
        name,
        is_dir: meta.is_dir(),
    })
}

#[derive(serde::Serialize)]
pub struct PathInfo {
    pub name: String,
    pub is_dir: bool,
}

// ---------- 数据备份 / 恢复 ----------

/// 备份数据到指定目录：在线备份数据库（SQLite backup API）+ 复制图标目录
#[tauri::command]
pub fn backup_data(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    target_dir: String,
) -> Result<(), String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let target = std::path::Path::new(&target_dir);
    std::fs::create_dir_all(target).map_err(|e| format!("创建备份目录失败: {}", e))?;

    // 数据库在线备份（WAL 安全）
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let db_path = target.join("app.db");
    conn.backup("main", &db_path, None)
        .map_err(|e| format!("备份失败: {}", e))?;
    drop(conn);

    // 复制图标目录
    let src_icons = app_data.join("icons");
    let dst_icons = target.join("icons");
    if src_icons.exists() {
        let _ = copy_dir_recursive(&src_icons, &dst_icons);
    }

    log::info!("数据备份完成 -> {}", target_dir);
    Ok(())
}

/// 从备份目录恢复数据：暂存数据库与图标，重启应用后生效
/// （运行中的数据库文件被占用，无法直接覆盖，采用启动时应用的方式）
#[tauri::command]
pub fn restore_data(
    app: tauri::AppHandle,
    source_dir: String,
) -> Result<(), String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let source = std::path::Path::new(&source_dir);
    let src_db = source.join("app.db");
    if !src_db.exists() {
        return Err("备份目录中未找到 app.db".into());
    }

    // 暂存备份文件
    let restore_db = app_data.join("restore.db");
    std::fs::copy(&src_db, &restore_db).map_err(|e| format!("暂存备份失败: {}", e))?;

    // 暂存图标目录
    let src_icons = source.join("icons");
    let restore_icons = app_data.join("restore_icons");
    if src_icons.exists() {
        let _ = std::fs::remove_dir_all(&restore_icons);
        let _ = copy_dir_recursive(&src_icons, &restore_icons);
    }

    // 写入待恢复标志
    std::fs::write(app_data.join(".restore_pending"), "1")
        .map_err(|e| format!("写入恢复标志失败: {}", e))?;

    log::info!("数据恢复已暂存，重启后生效 <- {}", source_dir);
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir_recursive(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

// ---------- 拖拽导入 ----------

#[derive(serde::Serialize)]
pub struct DroppedAppInfo {
    pub name: String,
    pub target: String,
    pub icon: Option<String>,
}

/// 解析拖入的文件信息：.exe 直接读取，.lnk 快捷方式解析其目标路径；均尝试提取程序图标
#[tauri::command]
pub fn parse_dropped_path(
    app: tauri::AppHandle,
    path: String,
) -> Result<DroppedAppInfo, String> {
    let p = std::path::Path::new(&path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let (name, target, icon) = match ext.as_str() {
        "exe" => {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("本地应用")
                .to_string();
            let target = path.clone();
            let icon = extract_app_icon(&app, &target);
            (name, target, icon)
        }
        "lnk" => {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("快捷方式")
                .to_string();
            let (target, icon) = resolve_lnk_target_and_icon(&app, &path)?;
            (name, target, icon)
        }
        _ => {
            log::warn!("拖入文件不支持: {}", path);
            return Err("仅支持 .exe 文件或 .lnk 快捷方式".into());
        }
    };
    log::info!("拖入解析成功: {} -> {} (图标: {})", name, target, if icon.is_some() { "有" } else { "无" });
    Ok(DroppedAppInfo {
        name,
        target,
        icon,
    })
}

/// 创建隐藏窗口的 powershell 命令：避免 GUI 应用调用时弹出黑色控制台窗口
fn powershell() -> std::process::Command {
    let mut cmd = std::process::Command::new("powershell");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

/// 单次 PowerShell 进程内解析 .lnk 目标并提取图标
/// （原两段式需要先后启动两次 PowerShell，合并为一次调用可省约一半耗时）
/// 图标仍按「目标路径」命名缓存，与 .exe 导入共用缓存键
fn resolve_lnk_target_and_icon(
    app: &tauri::AppHandle,
    lnk_path: &str,
) -> Result<(String, Option<String>), String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取数据目录: {}", e))?
        .join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 先用 lnk 路径生成临时输出路径；解析出目标后再按目标路径（既有缓存键）重命名
    let mut tmp_hasher = DefaultHasher::new();
    lnk_path.hash(&mut tmp_hasher);
    let tmp_path = dir.join(format!("{:016x}.png", tmp_hasher.finish()));

    let script = "Add-Type -AssemblyName System.Drawing; $sh=New-Object -ComObject WScript.Shell; $t=$sh.CreateShortcut($env:XHUB_LNK).TargetPath; [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Write-Output ('TARGET='+$t); if($t -ne ''){$i=[System.Drawing.Icon]::ExtractAssociatedIcon($t); if($i -ne $null){$i.ToBitmap().Save($env:XHUB_OUT,[System.Drawing.Imaging.ImageFormat]::Png)}}";
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .env("XHUB_LNK", lnk_path)
        .env("XHUB_OUT", tmp_path.to_str().unwrap_or(""))
        .output()
        .map_err(|e| {
            log::error!("解析快捷方式失败（PowerShell 执行错误）: {}", e);
            format!("解析快捷方式失败: {}", e)
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let target = stdout
        .lines()
        .find_map(|l| l.strip_prefix("TARGET="))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!(
                "解析快捷方式失败（目标为空）: {} {}",
                lnk_path,
                stderr.trim()
            );
            "无法解析快捷方式目标路径".to_string()
        })?;

    // 图标按目标路径命名：与 .exe 拖入/已缓存图标共用缓存键，避免重复提取
    let icon = if tmp_path.exists() {
        let mut final_hasher = DefaultHasher::new();
        target.hash(&mut final_hasher);
        let final_path = dir.join(format!("{:016x}.png", final_hasher.finish()));
        if final_path != tmp_path {
            if final_path.exists() {
                let _ = std::fs::remove_file(&tmp_path);
            } else {
                let _ = std::fs::rename(&tmp_path, &final_path);
            }
        }
        Some(final_path.to_string_lossy().into_owned())
    } else {
        None
    };

    Ok((target, icon))
}

/// 提取程序图标（System.Drawing.ExtractAssociatedIcon），保存 PNG 到 app_data_dir/icons/
/// 提取失败或无图标时返回 None（前端回退到名称首字母）
fn extract_app_icon(app: &tauri::AppHandle, source: &str) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = app.path().app_data_dir().ok()?.join("icons");
    std::fs::create_dir_all(&dir).ok()?;

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let file_name = format!("{:016x}.png", hasher.finish());
    let output_path = dir.join(&file_name);

    // 已提取过则直接复用
    if output_path.exists() {
        return Some(output_path.to_string_lossy().into_owned());
    }

    let script = "Add-Type -AssemblyName System.Drawing; $i=[System.Drawing.Icon]::ExtractAssociatedIcon($env:XHUB_SRC); if($i -ne $null){$i.ToBitmap().Save($env:XHUB_OUT,[System.Drawing.Imaging.ImageFormat]::Png); Write-Output 'OK'}";
    let output = match powershell()
        .args(["-NoProfile", "-Command", script])
        .env("XHUB_SRC", source)
        .env("XHUB_OUT", output_path.to_str().unwrap_or(""))
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            log::warn!("图标提取失败（PowerShell 无法执行）: {} -> {}", source, e);
            return None;
        }
    };

    if String::from_utf8_lossy(&output.stdout).contains("OK") {
        Some(output_path.to_string_lossy().into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("图标提取失败（程序无图标或提取出错）: {} -> {}", source, stderr.trim());
        None
    }
}

/// 导入用户选择的图标文件到 icons 目录：
/// - .ico 经 System.Drawing 转为 PNG
/// - png/jpg 等图片直接复制
/// 返回存储后的 PNG 路径（失败返回 None）
#[tauri::command]
pub fn import_icon_file(
    app: tauri::AppHandle,
    source: String,
) -> Result<Option<String>, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let ext = std::path::Path::new(&source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let file_name = format!("{:016x}.png", hasher.finish());
    let output_path = dir.join(&file_name);

    // 已导入过则直接复用
    if output_path.exists() {
        return Ok(Some(output_path.to_string_lossy().into_owned()));
    }

    if ext == "ico" {
        let script = "Add-Type -AssemblyName System.Drawing; $i=New-Object System.Drawing.Icon($env:XHUB_SRC); $i.ToBitmap().Save($env:XHUB_OUT,[System.Drawing.Imaging.ImageFormat]::Png); Write-Output 'OK'";
        let output = powershell()
            .args(["-NoProfile", "-Command", script])
            .env("XHUB_SRC", &source)
            .env("XHUB_OUT", output_path.to_str().unwrap_or(""))
            .output()
            .map_err(|e| {
                log::error!("图标转换失败（PowerShell 无法执行）: {}", e);
                format!("图标转换失败: {}", e)
            })?;
        if String::from_utf8_lossy(&output.stdout).contains("OK") {
            log::info!("图标导入成功: {} -> {}", source, output_path.display());
            Ok(Some(output_path.to_string_lossy().into_owned()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::error!("图标转换失败: {} -> {}", source, stderr.trim());
            Err("图标转换失败".into())
        }
    } else {
        match std::fs::copy(&source, &output_path) {
            Ok(_) => {
                log::info!("图标导入成功: {} -> {}", source, output_path.display());
                Ok(Some(output_path.to_string_lossy().into_owned()))
            }
            Err(e) => {
                log::error!("图标复制失败: {} -> {}", source, e);
                Err(format!("复制图标失败: {}", e))
            }
        }
    }
}

// ---------- 扫描已安装应用 ----------

#[derive(serde::Serialize)]
pub struct InstalledAppInfo {
    pub name: String,
    pub target: String,
    pub icon: Option<String>,
}

/// 扫描本机已安装应用（注册表卸载项 + 用户/公共开始菜单快捷方式），
/// 去重、过滤系统噪音后批量提取程序图标（icons/<hash>.png，与拖拽导入共用缓存键）。
/// 必须 async：扫描 + 图标提取耗时数秒，同步命令会卡死主线程冻结 UI。
#[tauri::command]
pub async fn scan_installed_apps(app: tauri::AppHandle) -> Result<Vec<InstalledAppInfo>, String> {
    let candidates = scan_app_candidates()?;
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let icons = batch_extract_icons(&app, &candidates)?;
    Ok(candidates
        .into_iter()
        .zip(icons)
        .map(|((name, target), icon)| InstalledAppInfo { name, target, icon })
        .collect())
}

/// 单次 PowerShell 扫描注册表卸载项 + 开始菜单快捷方式，
/// 输出 APP=<json> 行（name/target），Rust 侧解析并二次去重、按名称排序、限量。
/// 命名/路径等取值一律在 PS 内 Trim + 环境变量展开，中文经 UTF-8 输出。
fn scan_app_candidates() -> Result<Vec<(String, String)>, String> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$sh = New-Object -ComObject WScript.Shell
$seen = @{}
$out = New-Object System.Collections.ArrayList

function Add-App([string]$name, [string]$target) {
  if (-not $name) { return }
  $name = $name.Trim()
  if (-not $target) { return }
  $target = $target.Trim()
  if (-not (Test-Path -LiteralPath $target)) { return }
  # 只收可执行目标（exe/bat/cmd），过滤 dll 图标源等
  if ($target -notmatch '\.(exe|bat|cmd)$') { return }
  $key = $target.ToLower()
  if ($seen.ContainsKey($key)) { return }
  $seen[$key] = $true
  [void]$out.Add(@{ name = $name; target = $target })
}

# ---- 注册表卸载项（HKLM 32/64 + HKCU）----
$regRoots = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
foreach ($root in $regRoots) {
  Get-ItemProperty $root | ForEach-Object {
    $dn = $_.DisplayName
    if (-not $dn) { return }
    $dn = ([string]$dn).Trim()
    # 过滤系统组件/运行时/更新类噪音
    if ($dn -match '^(KB\d+|Update for|Security Update|Hotfix|Microsoft Update Health|Microsoft Edge (Update|WebView)|Microsoft Windows|Windows (SDK|Driver|Update|PowerShell|Terminal|Web Experience|Package Manager|App Runtime|App Certification|Kits)|Microsoft Visual C\+\+|Microsoft \.NET|\.NET (Runtime|Host)|Windows App Runtime|Microsoft Office (ClickToRun|Microsoft 365 Apps for enterprise))') { return }
    if ($dn -match '(卸载|Uninstall|Update|Updater)$') { return }
    $target = ''
    # DisplayIcon 常直接指向主 exe（可能带 ,0 序号或 %环境变量%）
    if ($_.DisplayIcon) {
      $di = (([string]$_.DisplayIcon) -split ',')[0].Trim()
      if ($di) {
        try { $di = $ExecutionContext.InvokeCommand.ExpandString($di) } catch {}
        if ($di -and (Test-Path -LiteralPath $di)) { $target = $di }
      }
    }
    # 无 DisplayIcon 时从安装目录挑一个主 exe
    if (-not $target -and $_.InstallLocation) {
      $loc = ([string]$_.InstallLocation).Trim()
      try { $loc = $ExecutionContext.InvokeCommand.ExpandString($loc) } catch {}
      if ($loc -and (Test-Path -LiteralPath $loc)) {
        $exe = Get-ChildItem -LiteralPath $loc -Filter *.exe -File -Recurse -Depth 1 -ErrorAction SilentlyContinue |
          Where-Object { $_.FullName -notmatch '(unins\d*\.exe|uninstall(\.exe|_?[\w-]*\.exe)?|update(\.exe|r\.exe)?)$' } |
          Select-Object -First 1
        if ($exe) { $target = $exe.FullName }
      }
    }
    if (-not $target) { return }
    if ($target -match '\\Windows\\(System32|SysWOW64|servicing|WinSxS)\\' -or $target -match '(unins\d*\.exe|uninstall(\.exe|_?[\w-]*\.exe)?)$') { return }
    Add-App $dn $target
  }
}

# ---- 开始菜单快捷方式（用户 + 公共）----
$lnkDirs = @(
  "$env:APPDATA\Microsoft\Windows\Start Menu\Programs",
  "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
)
foreach ($dir in $lnkDirs) {
  Get-ChildItem -LiteralPath $dir -Filter *.lnk -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
    try {
      $lnk = $sh.CreateShortcut($_.FullName)
      $t = $lnk.TargetPath
    } catch { return }
    if (-not $t) { return }
    if ($t -match '\\Windows\\(System32|SysWOW64|servicing|WinSxS)\\' -or $t -match '(unins\d*\.exe|uninstall(\.exe|_?[\w-]*\.exe)?)$') { return }
    $bn = $_.BaseName
    $bn = $bn -replace '\s*[-–—]\s*(快捷方式|shortcut)$','' -replace '\s*\(\d+\)\s*$',''
    Add-App $bn $t
  }
}

foreach ($a in $out) {
  Write-Output ('APP=' + ($a | ConvertTo-Json -Compress))
}
"#;
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("扫描已安装应用失败（PowerShell 执行错误）: {}", e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        log::debug!("扫描应用 PowerShell stderr: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut apps: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in stdout.lines() {
        let Some(json) = line.strip_prefix("APP=") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        let (Some(name), Some(target)) = (
            v.get("name").and_then(|x| x.as_str()),
            v.get("target").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        let (name, target) = (name.trim(), target.trim());
        if name.is_empty() || target.is_empty() {
            continue;
        }
        if !seen.insert(target.to_lowercase()) {
            continue;
        }
        apps.push((name.to_string(), target.to_string()));
    }
    apps.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.1.cmp(&b.1))
    });
    const MAX_APPS: usize = 500;
    if apps.len() > MAX_APPS {
        apps.truncate(MAX_APPS);
    }
    log::info!("扫描已安装应用: 共 {} 个", apps.len());
    Ok(apps)
}

/// 批量提取程序图标：单次 PowerShell 提取所有未缓存目标图标到临时目录，
/// 再按 DefaultHasher(target) 重命名为正式缓存键（与 extract_app_icon 共用缓存，
/// 已缓存的目标直接复用，重复扫描零开销）。
fn batch_extract_icons(
    app: &tauri::AppHandle,
    apps: &[(String, String)],
) -> Result<Vec<Option<String>>, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let icons_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("icons");
    std::fs::create_dir_all(&icons_dir).map_err(|e| e.to_string())?;

    // 已缓存目标直接复用，只收集未缓存的索引
    let mut missing: Vec<usize> = Vec::new();
    let mut result: Vec<Option<String>> = Vec::with_capacity(apps.len());
    for (i, (_, target)) in apps.iter().enumerate() {
        let mut hasher = DefaultHasher::new();
        target.hash(&mut hasher);
        let cached = icons_dir.join(format!("{:016x}.png", hasher.finish()));
        if cached.exists() {
            result.push(Some(cached.to_string_lossy().into_owned()));
        } else {
            result.push(None);
            missing.push(i);
        }
    }
    if missing.is_empty() {
        return Ok(result);
    }

    let tmp_dir = icons_dir.join(".scan_tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let list_path = tmp_dir.join("list.txt");
    let mut list = String::new();
    for &i in &missing {
        list.push_str(&apps[i].1);
        list.push('\n');
    }
    std::fs::write(&list_path, list).map_err(|e| e.to_string())?;

    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Drawing
$listFile = $env:XHUB_LIST
$outDir = $env:XHUB_OUTDIR
$idx = 0
Get-Content -LiteralPath $listFile -Encoding UTF8 | ForEach-Object {
  $p = $_.Trim()
  if ($p) {
    try {
      $i = [System.Drawing.Icon]::ExtractAssociatedIcon($p)
      if ($i -ne $null) {
        try {
          $bmp = $i.ToBitmap()
          $bmp.Save((Join-Path $outDir ('{0}.png' -f $idx)), [System.Drawing.Imaging.ImageFormat]::Png)
          $bmp.Dispose()
        } catch {}
        $i.Dispose()
      }
    } catch {}
  }
  $idx++
}
"#;
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .env("XHUB_LIST", list_path.to_str().unwrap_or(""))
        .env("XHUB_OUTDIR", tmp_dir.to_str().unwrap_or(""))
        .output()
        .map_err(|e| format!("应用图标提取失败（PowerShell 执行错误）: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("应用图标提取脚本异常退出: {}", stderr.trim());
    }

    // 临时图标重命名为正式缓存键（缺文件 = 该程序无可用图标）
    for (n, &i) in missing.iter().enumerate() {
        let tmp_file = tmp_dir.join(format!("{}.png", n));
        if !tmp_file.exists() {
            continue;
        }
        let mut hasher = DefaultHasher::new();
        apps[i].1.hash(&mut hasher);
        let final_path = icons_dir.join(format!("{:016x}.png", hasher.finish()));
        if final_path.exists() {
            let _ = std::fs::remove_file(&tmp_file);
        } else {
            let _ = std::fs::rename(&tmp_file, &final_path);
        }
        result[i] = Some(final_path.to_string_lossy().into_owned());
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    log::info!("应用图标提取完成: {} 个（缺 {} 个）", apps.len(), missing.len());
    Ok(result)
}

// ---------- 运行状态检测 ----------

/// 返回当前所有正在运行的进程名（ImageName，小写去重、排序）。
/// 前端按速达 app 资源的目标文件名匹配，判断应用是否已启动。
/// 由前端每 3s 轮询；进程枚举走系统快照，单次开销约几十毫秒。
#[tauri::command]
pub fn get_running_processes() -> Result<Vec<String>, String> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut names: Vec<String> = sys
        .processes()
        .values()
        .filter_map(|p| p.name().to_str().map(|s| s.to_lowercase()))
        .collect();
    names.sort();
    names.dedup();
    log::debug!("查询运行中进程: {} 个", names.len());
    Ok(names)
}

// ---------- 工具 ----------

fn err_str(e: rusqlite::Error) -> String {
    format!("数据库错误: {}", e)
}

fn parse_kind(kind: &str) -> Result<ResourceKind, String> {
    match kind {
        "app" => Ok(ResourceKind::App),
        "web" => Ok(ResourceKind::Web),
        "file" => Ok(ResourceKind::File),
        _ => Err(format!("未知资源类型: {}", kind)),
    }
}
