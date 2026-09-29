use crate::browsers::{self, InstalledBrowser};
use crate::config;
use crate::config::AppConfig;
use crate::models::{
    ChatMessage, ChatModelConfig, ChatSession, ClipboardItem, Countdown, DetachedSticky, Note,
    RepeatRule, Resource, ResourceKind, ResourceSubcategory, SearchResult, Snippet, Sticky, Tag,
    Todo, TodoOccurrence, TodoTag, TodoTagLink,
};
use crate::process;
use crate::repo::{
    chat, clipboard, countdown, detached_sticky, note, resource, snippet, sticky, subcategory,
    tag, todo, todo_tag,
};
use crate::todo_recurrence;
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

pub struct DbState(pub Mutex<Connection>);

/// AI 对话发送请求时携带的上下文窗口（消息条数）：长对话只取最近这段作为模型上下文，
/// 避免历史越长加载越慢、内存/请求体按全量历史成倍膨胀。约合 15 轮对话。
const CHAT_CONTEXT_WINDOW: i64 = 30;

/// 轻量配置读取：AI 对话独立窗唤起时专用——get_initial_data 会全量拉九类业务数据，
/// 独立窗只需要 config（主题三件套/字号等），业务数据由 ChatPanel 自己的 refresh 拉
#[tauri::command]
pub fn get_ui_config() -> crate::config::AppConfig {
    crate::config::load()
}

#[tauri::command]
pub fn get_initial_data(state: State<'_, DbState>) -> Result<InitialData, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::list_all(&conn).map_err(err_str)?;
    let notes = note::list(&conn).map_err(err_str)?;
    let tags = tag::list(&conn).map_err(err_str)?;
    let todos = todo::list(&conn).map_err(err_str)?;
    let stickies = sticky::list(&conn).map_err(err_str)?;
    let detached = detached_sticky::list(&conn).map_err(err_str)?;
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

/// 枚举本机已安装浏览器（注册表 StartMenuInternet，按 exe 去重）
#[tauri::command]
pub fn list_installed_browsers() -> Vec<InstalledBrowser> {
    browsers::list_installed()
}

/// 用指定浏览器打开速达网页资源（URL 从数据库读取：仅放行 Web 类型 + http/https）
#[tauri::command]
pub fn open_url_with_browser(
    state: State<'_, DbState>,
    id: i64,
    browser_exe: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    if !matches!(res.kind, ResourceKind::Web) {
        return Err("仅网页资源支持指定浏览器打开".to_string());
    }
    if !(res.target.starts_with("http://") || res.target.starts_with("https://")) {
        return Err("只能打开 http/https 链接".to_string());
    }
    process::open_with_browser(&browser_exe, &res.target)?;
    resource::touch(&conn, id).map_err(err_str)?;
    log::info!(
        "用浏览器打开网页: {} ({}) -> {}",
        res.name,
        res.target,
        browser_exe
    );
    Ok(())
}

#[tauri::command]
pub fn launch_resource(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    match res.kind {
        ResourceKind::App => {
            // 程序已在运行 → 只把已有窗口调度到前台，不再拉起第二个实例。
            // 带参数的资源仍按原样启动：参数往往就是「这次要打开的东西」（如 --incognito、
            // 要打开的文件夹），忽略它会丢语义。
            let no_args = res.args.as_deref().map(|a| a.trim().is_empty()).unwrap_or(true);
            if no_args && process::activate_existing(&res.target) {
                let _ = resource::touch(&conn, id);
                log::info!("程序已在运行，已调度到前台: {} ({})", res.name, res.target);
                return Ok(());
            }
            match process::launch_program(&res.target, res.args.as_deref()) {
                Ok(()) => {
                    let _ = resource::touch(&conn, id);
                    log::info!("启动程序: {} ({})", res.name, res.target);
                    Ok(())
                }
                Err(e) => {
                    log::error!("启动程序失败: {} ({}) -> {}", res.name, res.target, e);
                    Err(e)
                }
            }
        }
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

/// 以管理员身份启动速达「程序」资源（触发 UAC 确认）。仅 App 类型支持——
/// 网页/文件没有「提权运行」的语义。不走 launch_program 的 740 自动提权路径：
/// 这里是用户显式要求提权，直接 Start-Process -Verb RunAs。
#[tauri::command]
pub fn launch_resource_as_admin(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    match res.kind {
        ResourceKind::App => {
            process::launch_elevated(&res.target, res.args.as_deref())?;
            let _ = resource::touch(&conn, id);
            log::info!("以管理员身份启动程序: {} ({})", res.name, res.target);
            Ok(())
        }
        _ => Err("只有「程序」类型的资源支持以管理员身份运行".into()),
    }
}

// ---------- 速达小类（ADR 0012）----------

fn validate_subcategory_input(kind: &str, name: &str) -> Result<(), String> {
    if !subcategory::VALID_KINDS.contains(&kind) {
        return Err("无效的大类".into());
    }
    if name.is_empty() || name.chars().count() > 20 {
        return Err("小类名称需为 1–20 个字符".into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_subcategories(state: State<'_, DbState>) -> Result<Vec<ResourceSubcategory>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    subcategory::list(&conn).map_err(err_str)
}

#[tauri::command]
pub fn create_subcategory(
    state: State<'_, DbState>,
    kind: String,
    name: String,
) -> Result<ResourceSubcategory, String> {
    let kind = kind.trim().to_string();
    let name = name.trim().to_string();
    validate_subcategory_input(&kind, &name)?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let free = subcategory::is_name_free(&conn, &kind, &name, None).map_err(err_str)?;
    if !free {
        return Err(format!("DUP:「{name}」已存在于该大类"));
    }
    let sub = subcategory::create(&conn, &kind, &name).map_err(err_str)?;
    log::info!("新建速达小类: {} / {}", kind, name);
    Ok(sub)
}

#[tauri::command]
pub fn rename_subcategory(state: State<'_, DbState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 20 {
        return Err("小类名称需为 1–20 个字符".into());
    }
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    subcategory::rename(&mut conn, id, &name)
}

/// 删除小类：条目批量改挂默认小类（单事务，见 repo::subcategory::delete）
#[tauri::command]
pub fn delete_subcategory(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    subcategory::delete(&mut conn, id)
}

#[tauri::command]
pub fn reorder_subcategories(
    state: State<'_, DbState>,
    kind: String,
    ids: Vec<i64>,
) -> Result<(), String> {
    if !subcategory::VALID_KINDS.contains(&kind.as_str()) {
        return Err("无效的大类".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    subcategory::reorder(&conn, &kind, &ids).map_err(err_str)
}

#[tauri::command]
pub fn set_default_subcategory(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    subcategory::set_default(&conn, id)
}

/// 速达网页默认打开方式（panel=内嵌面板 / window=独立窗口，ADR 0011 2026-09-25 拍板）
#[tauri::command]
pub fn set_suda_web_open_mode(mode: String) -> Result<String, String> {
    let mode = mode.trim().to_string();
    if !["panel", "window"].contains(&mode.as_str()) {
        return Err("无效的打开方式".into());
    }
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.suda_web_open_mode = mode.clone();
    crate::config::save(&config)?;
    Ok(config.suda_web_open_mode)
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

/// 笔记列表（仅元信息，不拉正文）：外部浮层保存速记后主窗口刷新列表用，
/// 轻量于 get_initial_data 的全量加载
#[tauri::command]
pub fn list_notes(state: State<'_, DbState>) -> Result<Vec<Note>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::list_meta(&conn).map_err(err_str)
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
pub fn create_todo(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    title: String,
    parent_id: Option<i64>,
    created_at: Option<String>,
) -> Result<Todo, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("标题不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::create(&conn, t, parent_id, created_at.as_deref()).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!("添加待办: id={} {} (parent={:?})", todo.id, todo.title, parent_id);
    Ok(todo)
}

#[tauri::command]
pub fn toggle_todo(app: tauri::AppHandle, state: State<'_, DbState>, id: i64) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::toggle(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!("切换待办状态: id={} done={}", todo.id, todo.done);
    Ok(todo)
}

#[tauri::command]
pub fn update_todo(
    app: tauri::AppHandle,
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
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!("更新待办: id={} {} (优先级 {})", todo.id, todo.title, todo.priority);
    Ok(todo)
}

#[tauri::command]
pub fn delete_todo(app: tauri::AppHandle, state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    // 子待办经外键级联删除，返回被级联的子 id 仅供日志
    let kids = todo::delete(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    if kids.is_empty() {
        log::info!("删除待办: id={}", id);
    } else {
        log::info!("删除待办: id={} (级联删除子待办 {} 条)", id, kids.len());
    }
    Ok(())
}

/// 设置待办截止/提醒时刻（毫秒时间戳，None 即清除）；同时重置提醒触发标记
#[tauri::command]
pub fn schedule_todo(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    due_at: Option<i64>,
    remind_at: Option<i64>,
) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::schedule(&conn, id, due_at, remind_at).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!(
        "待办排期: id={} due_at={:?} remind_at={:?}",
        todo.id,
        todo.due_at,
        todo.remind_at
    );
    Ok(todo)
}

/// 待办拖拽排序：按传入顺序写入手动排序位（前端按分组计算完整顺序）
#[tauri::command]
pub fn reorder_todo_orders(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    ids: Vec<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    todo::reorder(&conn, &ids).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::debug!("待办排序更新: {} 条", ids.len());
    Ok(())
}

/// 跨父拖拽：把子待办改挂到另一个顶级父待办下，并按传入顺序重写目标父下的子项排序。
/// `ordered_ids` 为目标落点后的完整子项顺序（含被移动项）。
#[tauri::command]
pub fn move_todo_child(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    new_parent_id: i64,
    ordered_ids: Vec<i64>,
) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::move_child(&conn, id, new_parent_id, &ordered_ids)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!(
        "子待办改挂父级: id={} -> parent={} (目标下 {} 条)",
        id,
        new_parent_id,
        ordered_ids.len()
    );
    Ok(t)
}

// ---------- 待办升级：描述 / 置顶 / 周期 / 标签 ----------

/// 设置待办描述（轻量 Markdown）
#[tauri::command]
pub fn set_todo_description(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    description: String,
) -> Result<Todo, String> {
    if description.chars().count() > todo::MAX_DESCRIPTION_LEN {
        return Err(format!("描述过长（最多 {} 字）", todo::MAX_DESCRIPTION_LEN));
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::set_description(&conn, id, &description).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::debug!("待办描述更新: id={} ({} 字)", id, description.chars().count());
    Ok(t)
}

/// 置顶开关：置顶条目脱离日期分组，固定排在列表最顶部「置顶」区
#[tauri::command]
pub fn set_todo_pinned(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    pinned: bool,
) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::set_pinned(&conn, id, pinned).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::debug!("待办置顶更新: id={} pinned={}", id, pinned);
    Ok(t)
}

/// 写入周期规则（整组 repeat_* 列一起写；非法取值 fail-fast）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_todo_repeat(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    repeat_mode: String,
    repeat_every: Option<i64>,
    repeat_unit: Option<String>,
    repeat_weekdays: Option<i64>,
    repeat_month_day: Option<i64>,
    repeat_month_nth: Option<i64>,
    repeat_end_mode: Option<String>,
    repeat_end_at: Option<i64>,
    repeat_count: Option<i64>,
) -> Result<Todo, String> {
    let mut rule = RepeatRule {
        mode: repeat_mode,
        every: repeat_every,
        unit: repeat_unit,
        weekdays: repeat_weekdays,
        month_day: repeat_month_day,
        month_nth: repeat_month_nth,
        end_mode: repeat_end_mode,
        end_at: repeat_end_at,
        count: repeat_count,
    };
    rule.validate()?;
    rule.normalize();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::set_repeat(&conn, id, &rule).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!(
        "待办周期更新: id={} mode={} (下次 due_at={:?})",
        id,
        t.repeat_mode,
        t.due_at
    );
    Ok(t)
}

/// 周期待办「完成本轮」：due_at 滚到下一个未来时刻、计数 +1、子待办复位
#[tauri::command]
pub fn complete_todo_recurring(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Todo, String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::complete_recurring(&conn, id, now_ms)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!(
        "周期待办滚动: id={} mode={} 下一轮 due_at={:?} 累计完成={}",
        id,
        t.repeat_mode,
        t.due_at,
        t.repeat_done_count
    );
    Ok(t)
}

/// 撤销「完成本轮」：计数 −1，due_at 滚回上一个实例
#[tauri::command]
pub fn undo_todo_recurring(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let t = todo::undo_recurring(&conn, id)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!("周期待办撤销: id={} 回到 due_at={:?}", id, t.due_at);
    Ok(t)
}

/// 展开 [from_ms, to_ms] 内的周期待办虚拟实例（日历渲染用；规则只实现于 Rust 侧）
#[tauri::command]
pub fn expand_todo_occurrences(
    state: State<'_, DbState>,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<TodoOccurrence>, String> {
    // 先校验区间再取锁：超长区间直接拒绝，不白占数据库互斥锁
    todo_recurrence::validate_range(from_ms, to_ms)?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todos = todo::list_recurring_candidates(&conn).map_err(err_str)?;
    let mut out = Vec::new();
    for t in todos {
        let Some(due) = t.due_at else { continue };
        let rule = RepeatRule::from_todo(&t);
        for at_ms in todo_recurrence::expand_occurrences(
            &rule,
            due,
            t.repeat_done_count,
            from_ms,
            to_ms,
        )? {
            out.push(TodoOccurrence {
                todo_id: t.id,
                at_ms,
            });
        }
    }
    out.sort_by_key(|o| o.at_ms);
    Ok(out)
}

#[tauri::command]
pub fn list_todo_tags(state: State<'_, DbState>) -> Result<Vec<TodoTag>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    todo_tag::list(&conn).map_err(err_str)
}

#[tauri::command]
pub fn create_todo_tag(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    name: String,
    color: Option<String>,
) -> Result<TodoTag, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("标签名不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let tag = todo_tag::create(&conn, n, color.as_deref().unwrap_or("")).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todo-tags-changed", ());
    log::info!("新建待办标签: id={} {}", tag.id, tag.name);
    Ok(tag)
}

#[tauri::command]
pub fn update_todo_tag(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    name: String,
    color: Option<String>,
) -> Result<TodoTag, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("标签名不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let tag = todo_tag::update(&conn, id, n, color.as_deref().unwrap_or("")).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todo-tags-changed", ());
    Ok(tag)
}

#[tauri::command]
pub fn delete_todo_tag(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    todo_tag::delete(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todo-tags-changed", ());
    let _ = app.emit("todos-changed", ());
    Ok(())
}

/// 全量设置某条待办的标签
#[tauri::command]
pub fn set_todo_tags(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    tag_ids: Vec<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    todo_tag::set_todo_tags(&conn, id, &tag_ids).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::debug!("待办标签更新: id={} → {:?}", id, tag_ids);
    Ok(())
}

/// 待办-标签全量关联（前端构建筛选映射用）
#[tauri::command]
pub fn list_todo_tag_links(state: State<'_, DbState>) -> Result<Vec<TodoTagLink>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let links = todo_tag::list_links(&conn).map_err(err_str)?;
    Ok(links
        .into_iter()
        .map(|(todo_id, tag_id)| TodoTagLink { todo_id, tag_id })
        .collect())
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
    // 已存在浮窗记录：窗口在 → 聚焦；窗口不在（上次创建失败/被异常销毁）→ 原位重建，
    // 否则记录永远在而窗口永远不在，用户再点「脱离」只会走本分支静默聚焦，卡死到重启
    if let Some(existing) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        detached_sticky::get_by_slot(&conn, slot).map_err(err_str)?
    } {
        if !crate::sticky_window::focus(&app, slot) {
            log::warn!("便签浮窗记录存在但窗口缺失，重建自愈: slot={}", slot);
            crate::sticky_window::create_or_focus(
                &app,
                slot,
                existing.x,
                existing.y,
                existing.always_on_top,
            )
            .map_err(|e| format!("创建浮窗失败: {}", e))?;
        }
        return Ok(existing);
    }

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let origin = sticky::get_by_slot(&conn, slot).map_err(err_str)?;
    let content = origin.map(|s| s.content).unwrap_or_default();

    let saved = detached_sticky::upsert(&conn, slot, &content, None, None, true).map_err(err_str)?;
    // 脱离 = 复制并清空原卡，之后各自独立
    sticky::upsert(&conn, slot, "").map_err(err_str)?;
    drop(conn);

    // 创建失败必须回滚（删除浮窗记录 + 恢复原卡内容并广播刷新）：
    // DB 已写而窗口没建出来，就是「内容从主卡消失 + 再也浮不起来」的根源
    if let Err(e) = crate::sticky_window::create_or_focus(&app, slot, saved.x, saved.y, true) {
        log::error!("便签浮窗创建失败，回滚脱离: slot={} err={}", slot, e);
        let conn = state.0.lock().map_err(|err| err.to_string())?;
        let _ = detached_sticky::delete_by_slot(&conn, slot);
        let _ = sticky::upsert(&conn, slot, &content);
        drop(conn);
        let _ = app.emit("stickies-changed", ());
        return Err(format!("创建浮窗失败: {}", e));
    }
    log::info!("便签脱离浮窗: slot={} 内容 {} 字", slot, content.chars().count());

    Ok(saved)
}

/// 再次点击脱离 icon 时聚焦已有浮窗；窗口缺失但记录在 → 原位重建自愈；
/// 记录也不在 → 返回 false（前端据此走脱离分支）
#[tauri::command]
pub async fn focus_detached_sticky(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    slot: i64,
) -> Result<bool, String> {
    if !(1..=2).contains(&slot) {
        return Err("便签槽位取值 1-2".into());
    }
    if crate::sticky_window::focus(&app, slot) {
        return Ok(true);
    }
    let existing = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        detached_sticky::get_by_slot(&conn, slot).map_err(err_str)?
    };
    let Some(existing) = existing else {
        return Ok(false);
    };
    log::warn!("便签浮窗窗口缺失，重建自愈: slot={}", slot);
    crate::sticky_window::create_or_focus(&app, slot, existing.x, existing.y, existing.always_on_top)
        .map_err(|e| format!("创建浮窗失败: {}", e))?;
    Ok(true)
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

/// 删除浮窗便签（浮窗数据彻底删除）。
/// 空内容关闭浮窗也走这里：必须广播 stickies-changed，否则主窗口 state.detached
/// 留下幻影记录，便签卡的「脱离」按钮停在「已脱离」，再点只走聚焦分支永远浮不起来
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
    let _ = app.emit("stickies-changed", ());
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

/// 同步工作台倒计时卡片可见性（前端按「已提交」的 dashboard_layout 上报，仅主窗口调用）：
/// 卡片不在工作台时冻结全部非浮窗倒计时（不计时、到点不提醒），
/// 恢复显示时按暂停语义续跑（once 续剩余，daily/interval 顺延到下一次）
#[tauri::command]
pub fn set_countdown_card_visible(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    visible: bool,
) -> Result<(), String> {
    if let Some(flag) = app.try_state::<crate::countdown_ticker::CardVisible>() {
        flag.0.store(visible, std::sync::atomic::Ordering::Relaxed);
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let changed = if visible {
        let ids = countdown::list_auto_paused_ids(&conn).map_err(err_str)?;
        for id in &ids {
            countdown::resume(&conn, *id).map_err(err_str)?;
        }
        let resumed = !ids.is_empty();
        if resumed {
            log::info!("倒计时卡片回到工作台，恢复 {} 个倒计时", ids.len());
        }
        resumed
    } else {
        let n = countdown::auto_pause_all(&conn).map_err(err_str)?;
        let frozen = n > 0;
        if frozen {
            log::info!("倒计时卡片不在工作台，冻结 {} 个倒计时", n);
        }
        frozen
    };
    drop(conn);
    if changed {
        let _ = app.emit("countdowns-changed", ());
    }
    Ok(())
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
    // 卡片不在工作台时浮窗就是唯一显示面：浮起即恢复该倒计时的计时
    if !crate::countdown_ticker::card_visible(&app) {
        countdown::resume_if_auto_paused(&conn, id).map_err(err_str)?;
    }
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
    // 卡片不在工作台时浮窗收起即无处显示：冻结计时，卡片恢复显示时再续跑
    if !crate::countdown_ticker::card_visible(&app) {
        countdown::auto_pause_single(&conn, id).map_err(err_str)?;
    }
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
    app: tauri::AppHandle,
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
    drop(conn);
    let _ = app.emit("snippets-changed", ());
    log::info!("添加提示词: id={} {}", snippet.id, snippet.title);
    Ok(snippet)
}

#[tauri::command]
pub fn update_snippet(
    app: tauri::AppHandle,
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
    drop(conn);
    let _ = app.emit("snippets-changed", ());
    log::info!("更新提示词: id={} {}", snippet.id, snippet.title);
    Ok(snippet)
}

#[tauri::command]
pub fn delete_snippet(app: tauri::AppHandle, state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    snippet::delete(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("snippets-changed", ());
    log::info!("删除提示词: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn toggle_snippet_pin(app: tauri::AppHandle, state: State<'_, DbState>, id: i64) -> Result<Snippet, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let snippet = snippet::toggle_pin(&conn, id).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("snippets-changed", ());
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

/// 提示词浮窗：打开（或聚焦）/ 关闭切换
#[tauri::command]
pub async fn toggle_prompt_float(app: tauri::AppHandle) -> Result<(), String> {
    let label = crate::float_window::PROMPT_FLOAT_LABEL;
    if crate::float_window::is_visible(&app, label) {
        crate::float_window::destroy(&app, label);
    } else {
        crate::float_window::create_or_focus(&app, label, "提示词", 300.0, 420.0, false)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 待办浮窗：打开（或聚焦）/ 关闭切换
#[tauri::command]
pub async fn toggle_todo_float(app: tauri::AppHandle) -> Result<(), String> {
    let label = crate::float_window::TODO_FLOAT_LABEL;
    if crate::float_window::is_visible(&app, label) {
        crate::float_window::destroy(&app, label);
    } else {
        crate::float_window::create_or_focus(&app, label, "待办", 320.0, 440.0, true)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 切换提示词/待办浮窗的置顶状态（与便签浮窗一致的「是否置顶」开关）
#[tauri::command]
pub async fn toggle_float_pin(
    app: tauri::AppHandle,
    label: String,
    always_on_top: bool,
) -> Result<(), String> {
    let valid = matches!(
        label.as_str(),
        crate::float_window::PROMPT_FLOAT_LABEL | crate::float_window::TODO_FLOAT_LABEL
    );
    if !valid {
        return Err("未知的浮窗 label".into());
    }
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| "浮窗不存在".to_string())?;
    win.set_always_on_top(always_on_top).map_err(|e| e.to_string())?;
    log::info!("浮窗置顶切换: label={} 置顶={}", label, always_on_top);
    Ok(())
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

// ---------- 关于 / 更新日志 ----------

#[derive(serde::Serialize)]
pub struct AppInfo {
    /// 当前应用版本号（运行时读取打包版本，与 tauri.conf.json 一致）
    pub version: String,
}

/// 返回应用版本号，供「关于」页与扩展市场的最低版本判断使用。
/// 版本历史不再内置：客户端「关于」页直接跳转 GitHub Releases。
#[tauri::command]
pub fn get_app_info(app: tauri::AppHandle) -> Result<AppInfo, String> {
    let version = app.package_info().version.to_string();
    Ok(AppInfo { version })
}

// ---------- 配置 ----------

/// 主题配置（悬浮球等独立窗口自取通道：主窗 useTheme 运行时推送之外的初始值来源）
#[derive(serde::Serialize)]
pub struct ThemeConfig {
    pub mode: String,
    pub preset: String,
    pub accent: Option<String>,
}

#[tauri::command]
pub fn get_theme_config() -> ThemeConfig {
    let cfg = crate::config::load();
    ThemeConfig {
        mode: cfg.theme_mode,
        preset: cfg.theme_preset,
        accent: cfg.accent_color,
    }
}

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<AppConfig, String> {
    let _guard = crate::config::lock();
    // 前端提交的是**启动快照**（state.config），只认识用户可编辑项；模型配置、独立窗几何、
    // 悬浮球位置、`dev_extensions`/`skill_roots`/`skipped_update_version` 等由后端命令单独写盘的
    // 字段一律以磁盘为准，否则「保存任意设置」会把它们回滚到启动时刻（合并清单与机理见
    // config::merge_disk_authoritative，带回归测试兜底）。
    let mut merged = config;
    let disk = crate::config::load();
    crate::config::merge_disk_authoritative(&mut merged, &disk);
    crate::config::save(&merged)?;
    log::info!(
        "配置已保存: theme_mode={} theme_preset={} accent_color={:?} window={}x{} always_on_top={}",
        merged.theme_mode,
        merged.theme_preset,
        merged.accent_color,
        merged.window.width,
        merged.window.height,
        merged.window.always_on_top
    );
    Ok(merged)
}

#[tauri::command]
// ⚠️ 参数必须是 Webview 而非 WebviewWindow：主窗自速达浏览器起是多 webview 窗口，
// WebviewWindow 的命令注入走 is_webview_window 校验、对多 webview 窗口解析直接报错
// （实测踩过：最小化/最大化/置顶全部静默失效）。webview.window() 对任何窗口都成立。
pub fn set_window_always_on_top(webview: tauri::Webview, value: bool) -> Result<(), String> {
    webview
        .window()
        .set_always_on_top(value)
        .map_err(|e| e.to_string())?;
    log::info!("窗口置顶: {}", if value { "开" } else { "关" });
    Ok(())
}

#[tauri::command]
pub fn set_always_on_top_config(value: bool) -> Result<(), String> {
    let _guard = crate::config::lock();
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
    set_configured_shortcut(app, value, ConfiguredShortcut::Main)
}

/// 可自定义快捷键在配置里的落点（set_*_shortcut 命令共用同一套改绑/持久化流程）
enum ConfiguredShortcut {
    Main,
    Clipboard,
    Search,
    Chat,
}

impl ConfiguredShortcut {
    fn field<'a>(&self, cfg: &'a mut crate::config::AppConfig) -> &'a mut String {
        match self {
            ConfiguredShortcut::Main => &mut cfg.global_shortcut,
            ConfiguredShortcut::Clipboard => &mut cfg.clipboard_shortcut,
            ConfiguredShortcut::Search => &mut cfg.search_shortcut,
            ConfiguredShortcut::Chat => &mut cfg.chat_shortcut,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ConfiguredShortcut::Main => "主窗口",
            ConfiguredShortcut::Clipboard => "剪贴板",
            ConfiguredShortcut::Search => "搜索",
            ConfiguredShortcut::Chat => "AI 对话",
        }
    }
}

/// 更新某个可自定义全局快捷键：改绑（冲突预检/反注册/注册/回滚）+ 配置持久化。
/// 同一物理按键组合仅换写法（CommandOrControl→Ctrl）时直接改存储字符串，不重新注册。
fn set_configured_shortcut(
    app: tauri::AppHandle,
    value: String,
    which: ConfiguredShortcut,
) -> Result<String, String> {
    let _guard = crate::config::lock();
    let shortcut = value.trim();
    if shortcut.is_empty() {
        return Err("快捷键不能为空".into());
    }

    let mut config = crate::config::load();
    let previous = config_field(&config, &which);
    if previous == shortcut {
        return Ok(previous);
    }
    if crate::shortcut::same_hotkey(&previous, shortcut) {
        *which.field(&mut config) = shortcut.to_string();
        crate::config::save(&config)?;
        return Ok(shortcut.to_string());
    }
    crate::shortcut::rebind_shortcut(&app, &previous, shortcut)?;
    *which.field(&mut config) = shortcut.to_string();
    crate::config::save(&config)?;
    log::info!("[快捷键] {}快捷键已改为 {}", which.label(), shortcut);
    Ok(shortcut.to_string())
}

fn config_field(cfg: &crate::config::AppConfig, which: &ConfiguredShortcut) -> String {
    match which {
        ConfiguredShortcut::Main => cfg.global_shortcut.clone(),
        ConfiguredShortcut::Clipboard => cfg.clipboard_shortcut.clone(),
        ConfiguredShortcut::Search => cfg.search_shortcut.clone(),
        ConfiguredShortcut::Chat => cfg.chat_shortcut.clone(),
    }
}

/// 更新全局搜索呼出快捷键
#[tauri::command]
pub fn set_search_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Search)
}

/// 更新 AI 对话呼出快捷键
#[tauri::command]
pub fn set_chat_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Chat)
}

// ---------- 开机自启动 ----------

/// 自启动当前状态
#[derive(serde::Serialize)]
pub struct RunAtStartupStatus {
    /// 是否真正会开机自启：已开启 + Run 键指向当前 exe + 未被系统禁用。供 UI 如实反映。
    pub enabled: bool,
    /// 用户开关意图（config.run_at_startup）。
    pub configured: bool,
    /// Run 键是否存在且指向当前 exe（路径不符/被删 = false）。
    pub registered: bool,
    /// 是否被任务管理器/安全软件在启动项里禁用（StartupApproved 置 0x03）。
    pub os_disabled: bool,
}

/// 读取自启动状态：以系统真实注册为准，而非仅配置值。
/// 配置开着但注册丢失/路径变更/被禁用时，`enabled=false`，UI 可据此提示「失效」。
#[tauri::command]
pub fn get_run_at_startup() -> Result<RunAtStartupStatus, String> {
    let config = crate::config::load();
    let configured = config.run_at_startup;
    let (registered, os_disabled) = crate::autostart::probe();
    let enabled = configured && registered && !os_disabled;
    Ok(RunAtStartupStatus {
        enabled,
        configured,
        registered,
        os_disabled,
    })
}

/// 设置自启动开关：写入系统注册（Run 键）成功后持久化配置，
/// 并顺带清理旧版「管理员启动」模式残留的计划任务。
#[tauri::command]
pub fn set_run_at_startup(enabled: bool) -> Result<(), String> {
    let _guard = crate::config::lock();
    crate::autostart::apply(enabled)?;
    let mut config = crate::config::load();
    config.run_at_startup = enabled;
    crate::config::save(&config)?;
    log::info!("开机自启动: enabled={}", enabled);
    Ok(())
}

/// 本次启动是否来自「自启动静默模式」（命令行带 --autostart-hidden）。
/// 前端据此不主动显示主窗口，直接驻留托盘。
#[tauri::command]
pub fn get_startup_hidden() -> Result<bool, String> {
    Ok(crate::autostart::is_hidden_launch())
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
pub fn minimize_window(webview: tauri::Webview) -> Result<(), String> {
    webview
        .window()
        .minimize()
        .map_err(|e| e.to_string())?;
    log::info!("窗口最小化");
    Ok(())
}

#[tauri::command]
pub fn toggle_maximize(webview: tauri::Webview) -> Result<(), String> {
    let window = webview.window();
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
    // hide_window 找不到主窗时会自己落 WARN；这里只在真隐藏成功时打 INFO，
    // 避免「日志说已隐藏、窗口其实还在」的假成功（实测踩过：主窗查不到时仍报成功）
    if crate::tray::hide_window(&app) {
        log::info!("窗口隐藏至托盘");
    }
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

/// 备份数据到指定目录：把在线备份的数据库（SQLite backup API）与图标目录
/// 打包成单个压缩包（如 `x-hub-backup-20260815-143022.zip`），避免散落。
/// 返回生成的压缩包文件名。
#[tauri::command]
pub fn backup_data(state: State<'_, DbState>, target_dir: String) -> Result<String, String> {
    let app_data = crate::paths::data_root().to_path_buf();
    let target = std::path::Path::new(&target_dir);
    std::fs::create_dir_all(target).map_err(|e| format!("创建备份目录失败: {}", e))?;

    // 1. 在线备份数据库到临时文件（运行中数据库被占用，不能直接复制；WAL 安全）
    let tmp_db = std::env::temp_dir().join(format!("x-hub-backup-{}.db", std::process::id()));
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        conn.backup("main", &tmp_db, None)
            .map_err(|e| format!("备份数据库失败: {}", e))?;
    }

    // 2. 生成压缩包文件名（带时间戳，多次备份互不覆盖）
    let name = format!(
        "x-hub-backup-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let zip_path = target.join(&name);

    // 3. 打包成单个压缩包
    let out = std::fs::File::create(&zip_path)
        .map_err(|e| format!("创建备份压缩包失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(out);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // app.db
    zip.start_file("app.db", opts).map_err(|e| e.to_string())?;
    {
        let mut f = std::fs::File::open(&tmp_db).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut zip).map_err(|e| format!("写入数据库失败: {}", e))?;
    }

    // icons/
    let icons = app_data.join("icons");
    if icons.exists() {
        write_dir_to_zip(&mut zip, &icons, "icons", opts)?;
    }

    zip.finish()
        .map_err(|e| format!("完成备份压缩包失败: {}", e))?;
    let _ = std::fs::remove_file(&tmp_db);

    log::info!("数据备份完成 -> {}", zip_path.display());
    Ok(name)
}

/// 递归把目录写入压缩包（压缩包内路径统一用 `/` 分隔）
fn write_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &std::path::Path,
    prefix: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().into_owned();
        let zname = if prefix.is_empty() {
            fname
        } else {
            format!("{}/{}", prefix, fname)
        };
        if path.is_dir() {
            write_dir_to_zip(zip, &path, &zname, opts)?;
        } else {
            zip.start_file(&zname, opts).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 从备份压缩包恢复数据：解压出数据库与图标暂存，重启应用后生效
/// （运行中的数据库文件被占用，无法直接覆盖，采用启动时应用的方式）
#[tauri::command]
pub fn restore_data(source: String) -> Result<(), String> {
    let app_data = crate::paths::data_root().to_path_buf();
    let zip_path = std::path::Path::new(&source);
    if !zip_path.exists() {
        return Err("备份压缩包不存在".into());
    }

    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开备份压缩包失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("备份压缩包无效或已损坏: {}", e))?;

    // 清理旧的暂存内容
    let restore_db = app_data.join("restore.db");
    let restore_icons = app_data.join("restore_icons");
    let _ = std::fs::remove_file(&restore_db);
    let _ = std::fs::remove_dir_all(&restore_icons);

    let mut found_db = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name 防止路径穿越（zip slip）；非法路径条目直接跳过
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str == "app.db" {
            let mut out = std::fs::File::create(&restore_db).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("恢复数据库失败: {}", e))?;
            found_db = true;
        } else if let Some(inner) = rel_str.strip_prefix("icons/") {
            let dst = restore_icons.join(inner);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut out = std::fs::File::create(&dst).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("恢复图标失败: {}", e))?;
        }
    }

    if !found_db {
        let _ = std::fs::remove_file(&restore_db);
        let _ = std::fs::remove_dir_all(&restore_icons);
        return Err("备份压缩包中未找到 app.db".into());
    }

    // 写入待恢复标志
    std::fs::write(app_data.join(".restore_pending"), "1")
        .map_err(|e| format!("写入恢复标志失败: {}", e))?;

    log::info!("数据恢复已暂存，重启后生效 <- {}", source);
    Ok(())
}

// ---------- 数据存储路径（可迁移 / 便携） ----------

#[derive(serde::Serialize)]
pub struct DataPathInfo {
    pub path: String,
    /// default（默认 %APPDATA% 路径）/ custom（用户自定义）/ portable（便携模式，跟随程序目录）
    pub mode: String,
}

/// 返回当前数据根路径与模式
#[tauri::command]
pub fn get_data_path() -> Result<DataPathInfo, String> {
    let (path, mode) = crate::paths::data_path_info();
    Ok(DataPathInfo {
        path,
        mode: mode.to_string(),
    })
}

/// 更改数据存储目录：把现有数据（数据库 / 图标 / 剪贴板图片 / 配置 / 密钥）复制到新目录，
/// 写引导文件指向新目录，重启后生效。旧目录保留（安全起见不删除，由用户自行清理）。
#[tauri::command]
pub fn change_data_dir(state: State<'_, DbState>, new_dir: String) -> Result<(), String> {
    // 便携版数据固定跟随程序目录（exe\data），不支持更改
    if crate::paths::is_portable() {
        return Err("便携版数据跟随程序目录，不支持更改".into());
    }
    let target = std::path::Path::new(&new_dir);
    if !target.is_absolute() {
        return Err("数据目录必须是绝对路径".into());
    }
    std::fs::create_dir_all(target).map_err(|e| format!("创建数据目录失败: {}", e))?;

    let src = crate::paths::data_root();
    if src.canonicalize().ok() == target.canonicalize().ok() {
        return Err("新路径与当前路径相同".into());
    }

    // 1. 数据库在线备份到新目录（运行中数据库被占用，不能直接复制文件；WAL 安全）
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        conn.backup("main", &target.join("app.db"), None)
            .map_err(|e| format!("迁移数据库失败: {}", e))?;
    }

    // 2. 复制图标 / 剪贴板图片目录
    for name in ["icons", "clipboard"] {
        let s = src.join(name);
        let d = target.join(name);
        if s.exists() {
            let _ = copy_dir_recursive(&s, &d);
        }
    }

    // 3. 复制配置与密钥文件（app.json 随数据走，实现 U 盘换机配置一并继承）
    for name in ["app.json", "chat_keys.json"] {
        let s = src.join(name);
        if s.exists() {
            let _ = std::fs::copy(&s, target.join(name));
        }
    }

    // 4. 写引导文件指向新目录（重启后 init_database 读它）
    crate::paths::set_data_root(target)?;

    log::info!(
        "数据目录已迁移: {} -> {}",
        src.display(),
        target.display()
    );
    Ok(())
}

/// 重启应用（更改数据目录 / 恢复数据 / 更新就绪后前端调用）。
/// 统一走 `updater::relaunch_app`：释放 single-instance 互斥后 spawn 新进程并退出。
/// 不再用 `app.restart()`：tauri 的重启在非主线程（IPC 命令线程）触发时依赖
/// `RunEvent::Exit` 事件循环分支，而 x-hub 在 `.run()` 回调里对 Exit 直接
/// `std::process::exit(0)`（为保证托盘退出生效），会先杀死进程导致 restart
/// 永不执行——表现为「点了立即重启却只退出不重启」。显式 spawn 规避该路径。
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    crate::updater::relaunch_app(&app);
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
pub fn parse_dropped_path(path: String) -> Result<DroppedAppInfo, String> {
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
            let icon = extract_app_icon(&target);
            (name, target, icon)
        }
        "lnk" => {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("快捷方式")
                .to_string();
            let (target, icon) = resolve_lnk_target_and_icon(&path)?;
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
    use crate::process::NoConsoleWindow;
    let mut cmd = std::process::Command::new("powershell");
    cmd.no_console_window();
    cmd
}

/// 单次 PowerShell 进程内解析 .lnk 目标并提取图标
/// （原两段式需要先后启动两次 PowerShell，合并为一次调用可省约一半耗时）
/// 图标仍按「目标路径」命名缓存，与 .exe 导入共用缓存键
fn resolve_lnk_target_and_icon(lnk_path: &str) -> Result<(String, Option<String>), String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = crate::paths::data_root().join("icons");
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
fn extract_app_icon(source: &str) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = crate::paths::data_root().join("icons");
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
pub fn import_icon_file(source: String) -> Result<Option<String>, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = crate::paths::data_root().join("icons");
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

/// 壁纸支持的静态图片格式（gif 等动图会持续重绘，与 GPU 性能约束冲突，不收）
const WALLPAPER_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];
/// 壁纸大小上限：壁纸是常驻 GPU 纹理，过大直接拒绝
const WALLPAPER_MAX_BYTES: u64 = 30 * 1024 * 1024;

/// 导入用户选择的壁纸图片：复制进数据根 wallpapers 目录（内容哈希命名），
/// 并清理目录内不再被配置引用的文件，避免 %APPDATA% 堆积废弃图片。
/// 返回落盘后的绝对路径，前端写入配置 wallpaper_path
#[tauri::command]
pub fn import_wallpaper(source: String) -> Result<String, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::io::Read;

    let ext = std::path::Path::new(&source)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if !WALLPAPER_EXTENSIONS.contains(&ext.as_str()) {
        return Err("仅支持 png/jpg/webp/bmp 静态图片".into());
    }

    let dir = crate::paths::data_root().join("wallpapers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut bytes = Vec::new();
    std::fs::File::open(&source)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| format!("读取图片失败: {}", e))?;
    if bytes.len() as u64 > WALLPAPER_MAX_BYTES {
        return Err("图片超过 30MB，请压缩后再试".into());
    }

    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let output_path = dir.join(format!("{:016x}.{}", hasher.finish(), ext));
    std::fs::write(&output_path, &bytes).map_err(|e| format!("保存壁纸失败: {}", e))?;

    // 内容哈希命名天然去重：重复导入同一张图时落盘只有一份。
    // 此时磁盘上的 config 尚未指向新文件，引用集 = 旧配置路径 + 新文件
    let config = crate::config::load();
    prune_wallpapers_unreferenced(&dir, &config, Some(&output_path));

    log::info!("壁纸导入成功: {} -> {}", source, output_path.display());
    Ok(output_path.to_string_lossy().into_owned())
}

/// 清空 wallpapers 目录中当前配置未引用的壁纸文件
fn prune_wallpapers_unreferenced(
    dir: &std::path::Path,
    config: &crate::config::AppConfig,
    extra_keep: Option<&std::path::Path>,
) {
    let mut keep: Vec<std::path::PathBuf> = Vec::new();
    if !config.wallpaper_path.is_empty() {
        keep.push(std::path::PathBuf::from(&config.wallpaper_path));
    }
    if let Some(e) = extra_keep {
        keep.push(e.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !keep.iter().any(|k| k == &p) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

/// 壁纸目录清理：删除当前配置未引用的壁纸文件。
/// 前端先更新配置再调用本命令，即可完成「清除壁纸」并顺手回收孤儿文件
#[tauri::command]
pub fn cleanup_wallpapers() -> Result<(), String> {
    let dir = crate::paths::data_root().join("wallpapers");
    if dir.exists() {
        let config = crate::config::load();
        prune_wallpapers_unreferenced(&dir, &config, None);
    }
    log::info!("壁纸目录已按当前配置清理");
    Ok(())
}

// ---------- 笔记图片 ----------

/// 笔记内嵌图片支持的格式（笔记是文档配图，gif 动图放行——仅 <img> 渲染，无壁纸的常驻 GPU 纹理问题）
const NOTE_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif"];
/// 单张笔记图片大小上限
const NOTE_IMAGE_MAX_BYTES: usize = 10 * 1024 * 1024;

/// 保存笔记编辑器导入的图片（粘贴 / 拖拽 / 上传按钮）：base64 解码后按内容哈希命名
/// 落盘到数据根 notes/images/，返回可内嵌 Markdown 的 xhub-note 协议 URL。
/// 同内容同文件名天然去重；孤儿文件回收（删笔记/删图后）属后续 GC，暂不清理。
#[tauri::command]
pub fn import_note_image(data_b64: String, ext: String) -> Result<String, String> {
    use base64::Engine as _;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let ext = ext.to_lowercase();
    if !NOTE_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err("仅支持 png/jpg/webp/bmp/gif 图片".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data_b64)
        .map_err(|e| format!("图片数据解码失败: {}", e))?;
    if bytes.is_empty() {
        return Err("图片数据为空".into());
    }
    if bytes.len() > NOTE_IMAGE_MAX_BYTES {
        return Err("图片超过 10MB，请压缩后再试".into());
    }

    let dir = crate::paths::data_root().join("notes").join("images");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let name = format!("{:016x}.{}", hasher.finish(), ext);
    std::fs::write(dir.join(&name), &bytes).map_err(|e| format!("保存图片失败: {}", e))?;

    log::debug!("笔记图片已保存: {}", name);
    Ok(note_image_url(&name))
}

/// 笔记图片的内嵌 URL。xhub-note 协议（lib.rs 注册）按数据根 notes/images 解析，
/// URL 中不含数据根绝对路径——「更改数据存储路径」或整目录迁移后，已写入笔记的 URL 仍有效。
pub fn note_image_url(name: &str) -> String {
    // Windows/Android 上 Tauri 自定义协议以 http://<scheme>.localhost/ 形式访问；
    // macOS/Linux 为 <scheme>://localhost/（本应用仅面向 Windows 桌面，如需跨平台再分支）
    format!("http://xhub-note.localhost/{}", name)
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
pub async fn scan_installed_apps() -> Result<Vec<InstalledAppInfo>, String> {
    let candidates = scan_app_candidates()?;
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let icons = batch_extract_icons(&candidates)?;
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
    apps: &[(String, String)],
) -> Result<Vec<Option<String>>, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let icons_dir = crate::paths::data_root().join("icons");
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

// ---------- AI 对话 ----------

/// 会话列表（按最近更新倒序）
#[tauri::command]
pub fn list_chat_sessions(state: State<'_, DbState>) -> Result<Vec<ChatSession>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::list_sessions(&conn).map_err(err_str)
}

/// 新建会话
#[tauri::command]
pub fn create_chat_session(
    state: State<'_, DbState>,
    title: Option<String>,
    model_name: Option<String>,
) -> Result<ChatSession, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let title = title.unwrap_or_else(|| "新对话".to_string());
    let model_name = model_name.unwrap_or_else(|| default_session_model_name(&config::load().chat_models));
    let s = chat::create_session(&conn, &title, &model_name).map_err(err_str)?;
    log::info!("新建对话会话: id={} title={}", s.id, s.title);
    Ok(s)
}

/// 删除会话（级联删除消息）
#[tauri::command]
pub fn delete_chat_session(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::delete_session(&conn, id).map_err(err_str)?;
    log::info!("删除对话会话: id={}", id);
    Ok(())
}

/// 重命名会话
#[tauri::command]
pub fn rename_chat_session(
    state: State<'_, DbState>,
    id: i64,
    title: String,
) -> Result<ChatSession, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::rename_session(&conn, id, &title).map_err(err_str)
}

/// 切换会话使用的模型
#[tauri::command]
pub fn set_chat_session_model(
    state: State<'_, DbState>,
    id: i64,
    model_name: String,
) -> Result<ChatSession, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::set_session_model(&conn, id, &model_name).map_err(err_str)
}

/// 会话消息列表（按时间正序）
#[tauri::command]
pub fn list_chat_messages(
    state: State<'_, DbState>,
    session_id: i64,
) -> Result<Vec<ChatMessage>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::list_messages(&conn, session_id).map_err(err_str)
}

/// 平台额度的轮询游标（负载切换）：进程内计数，每次平台请求取下一个模型。
/// 不持久化——重启后从 0 开始，对「在多个平台模型之间负载切换」这个语义没有影响。
static PLATFORM_RR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 本次请求实际使用哪个模型配置。
///
/// 平台额度在用户侧是**一个入口**（`chat::PLATFORM_ENTRY_NAME`）：只有一个平台模型时永远用它，
/// 多个时在它们之间轮询（负载切换）——用户不需要、界面上也不再能选具体是哪一个
/// （2026-09-17 用户要求）。所以判定分两支：
/// - 会话选中的是平台入口名，**或**命中了某个 `platform:*` 条目（老会话存的是具体平台模型名）
///   → 走平台轮询；一个平台条目都没有时明确提示开关未开，而不是含糊地回退到别的模型
/// - 其余 → 精确命中会话选中的自备供应商模型，再回退全局默认（照旧）
fn pick_chat_model(
    models: &[ChatModelConfig],
    session_model_name: &str,
) -> Result<ChatModelConfig, String> {
    let platform: Vec<&ChatModelConfig> = models
        .iter()
        .filter(|m| crate::chat::is_platform_model(m))
        .collect();
    let wants_platform = session_model_name == crate::chat::PLATFORM_ENTRY_NAME
        || platform.iter().any(|m| m.name == session_model_name);
    if wants_platform {
        if platform.is_empty() {
            return Err("平台额度未开启，请在「设置 → 功能 → AI 助手」开启后再发送".into());
        }
        let i = PLATFORM_RR.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % platform.len();
        return Ok(platform[i].clone());
    }
    models
        .iter()
        .find(|m| m.name == session_model_name)
        .or_else(|| models.iter().find(|m| m.is_default))
        .cloned()
        .ok_or_else(|| "未配置任何对话模型，请先在对话设置中添加".to_string())
}

/// 新会话默认用的模型名：默认模型是平台条目（或只有平台条目）时统一存**入口名**，
/// 而不是某一个具体的平台模型——这样会话不会把「当时那一个模型」固化下来。
fn default_session_model_name(models: &[ChatModelConfig]) -> String {
    if let Some(d) = models.iter().find(|m| m.is_default) {
        if crate::chat::is_platform_model(d) {
            return crate::chat::PLATFORM_ENTRY_NAME.to_string();
        }
        return d.name.clone();
    }
    if models.iter().any(|m| crate::chat::is_platform_model(m)) {
        return crate::chat::PLATFORM_ENTRY_NAME.to_string();
    }
    models.first().map(|m| m.name.clone()).unwrap_or_default()
}

/// 模型配置列表：返回时清空 api_key，填充 has_api_key（真实 Key 存系统钥匙串）
#[tauri::command]
pub fn get_chat_models() -> Result<Vec<ChatModelConfig>, String> {
    let mut models = config::load().chat_models;
    // 平台模型的 base_url 以内置服务端地址为准（见 chat::platform_base_url 注释）：
    // 旧条目里可能存着开发期的临时地址，这里顺手纠正显示值，用户下次保存即落盘迁移
    let platform_base = crate::chat::platform_base_url();
    for m in &mut models {
        let key = crate::chat::get_api_key(&m.id);
        if key.as_deref() == Some(crate::chat::PLATFORM_KEY_SENTINEL) {
            m.base_url = platform_base.clone();
        }
        m.has_api_key = key.is_some();
        m.api_key.clear();
    }
    Ok(models)
}

/// 保存模型配置：非空 api_key 写入钥匙串，落盘时一律清空；保证有且仅有一个默认模型
#[tauri::command]
pub fn save_chat_models(models: Vec<ChatModelConfig>) -> Result<Vec<ChatModelConfig>, String> {
    let _guard = crate::config::lock();
    let mut config = config::load();
    let mut next = Vec::with_capacity(models.len());
    for m in models {
        if !m.api_key.trim().is_empty() {
            crate::chat::save_api_key(&m.id, m.api_key.trim())?;
        }
        let mut m = m;
        m.api_key.clear();
        next.push(m);
    }
    // 默认模型归一：无默认则第一个为默认；多默认只保留第一个
    let has_default = next.iter().any(|m| m.is_default);
    if !has_default {
        if let Some(first) = next.first_mut() {
            first.is_default = true;
        }
    } else {
        let mut seen = false;
        for m in &mut next {
            if m.is_default {
                if seen {
                    m.is_default = false;
                }
                seen = true;
            }
        }
    }
    // 供应商名称约束：不能为空。多账号可能共用同一 base_url，靠供应商名称区分，
    // 因此不按 base_url 分组/归一名称
    for m in &next {
        if m.provider_name.trim().is_empty() {
            return Err("供应商名称不能为空".into());
        }
    }
    config.chat_models = next;
    config::save(&config)?;
    // 供应商级 Key 传播：同一「名称 + base_url」组内模型补齐 Key
    // （保证「获取模型」后新加入的模型无需重复填写 Key；多账号同 URL 不串 Key）
    propagate_provider_keys(&config.chat_models);
    log::info!("保存对话模型配置: {} 条", config.chat_models.len());
    Ok(config
        .chat_models
        .iter()
        .map(|m| {
            let mut m = m.clone();
            m.has_api_key = crate::chat::get_api_key(&m.id).is_some();
            m
        })
        .collect())
}

/// 供应商级 Key 传播：同一「供应商名称 + base_url」组内任意模型已存有 Key 时，
/// 补齐到组内其余模型。以名称 + URL 为组，多账号同 URL（名称不同）互不串 Key。
fn propagate_provider_keys(models: &[ChatModelConfig]) {
    use std::collections::HashMap;
    let mut key_by_group: HashMap<(String, String), Option<String>> = HashMap::new();
    for m in models {
        let name = m.provider_name.trim().to_string();
        let base = m.base_url.trim().to_string();
        if name.is_empty() && base.is_empty() {
            continue;
        }
        let slot = key_by_group.entry((name, base)).or_insert(None);
        if slot.is_none() {
            *slot = crate::chat::get_api_key(&m.id);
        }
    }
    for m in models {
        let name = m.provider_name.trim().to_string();
        let base = m.base_url.trim().to_string();
        if name.is_empty() && base.is_empty() {
            continue;
        }
        if let Some(Some(key)) = key_by_group.get(&(name, base)) {
            if crate::chat::get_api_key(&m.id).is_none() {
                let _ = crate::chat::save_api_key(&m.id, key);
            }
        }
    }
}

/// 通用探测要用的 Key：前端传入值优先（尚未保存时），为空则用 key_id 从钥匙串读已保存的 Key。
///
/// ⚠️ 平台条目（`platform:<模型名>`）的钥匙串里存的是 `chat::PLATFORM_KEY_SENTINEL`，真凭据是
/// **账号登录态**、只在真正发对话时由 `chat::resolve_api_key` 现取 —— 拿占位符当 Key 发出去
/// 必然 401。平台供应商的连通性与模型列表一律走 `platform_models`（`/api/v1/ai/models`），
/// 那条路才是平台自己的接口；平台中转也不提供 OpenAI 的 `GET {base}/models`（恒 404）。
fn probe_key(api_key: &str, stored: Option<String>) -> Result<String, String> {
    let key = if api_key.trim().is_empty() {
        stored.ok_or_else(|| "未填写 API Key，且未找到已保存的 Key".to_string())?
    } else {
        api_key.trim().to_string()
    };
    if key.trim() == crate::chat::PLATFORM_KEY_SENTINEL {
        return Err(
            "平台供应商请用「测试连通/获取模型」（走平台账号接口），不能用平台占位 Key 探测通用 /models"
                .to_string(),
        );
    }
    Ok(key)
}

/// 连通性测试 + 拉取模型列表（OpenAI 兼容 `GET {base_url}/models`）
///
/// 供设置页「测试连通」「获取模型」使用，**只服务自备 Key 的供应商**；平台供应商走
/// `platform_models`（理由见 `probe_key`）。
#[tauri::command]
pub async fn fetch_chat_provider_models(
    base_url: String,
    api_key: String,
    key_id: Option<String>,
) -> Result<Vec<String>, String> {
    let stored = if api_key.trim().is_empty() {
        key_id.as_deref().and_then(crate::chat::get_api_key)
    } else {
        None
    };
    let key = probe_key(&api_key, stored)?;
    crate::chat::fetch_provider_models(&base_url, &key).await
}

/// 读取某个模型已保存的 API Key（设置页脱敏展示 / 眼睛查看 / 复制用）。
///
/// ⚠️ 平台模型（`platform:<模型名>`）的条目**不下发占位符**：它钥匙串里存的是
/// `chat::PLATFORM_KEY_SENTINEL`，真凭据是账号登录态、只在请求时由 `chat::resolve_api_key`
/// 现取。把占位符交给界面只会有两种坏结果 —— 让人以为平台 Key 泄露了，或以为这里要填 Key
/// （2026-09-17 用户反馈：界面不要展示 x-hub 平台的 key）。
#[tauri::command]
pub fn get_chat_api_key(model_id: String) -> Result<String, String> {
    api_key_for_ui(crate::chat::get_api_key(&model_id))
}

/// 界面可见的 Key（纯函数，便于回归测试）：平台占位符一律返回 Err。
fn api_key_for_ui(stored: Option<String>) -> Result<String, String> {
    match stored {
        Some(k) if k == crate::chat::PLATFORM_KEY_SENTINEL => {
            Err("平台模型使用账号登录态，没有可展示的 API Key".to_string())
        }
        Some(k) => Ok(k),
        None => Err("未找到已保存的 API Key".to_string()),
    }
}

/// 保存 AI 对话面板宽度/高度（按方位使用）与展开状态（持久化）
#[tauri::command]
pub fn set_chat_panel(width: f64, height: f64, open: bool) -> Result<(), String> {
    let _guard = crate::config::lock();
    let mut config = config::load();
    config.chat_panel_width = width.clamp(320.0, 640.0);
    config.chat_panel_height = height.clamp(280.0, 640.0);
    config.chat_panel_open = open;
    config::save(&config)
}

/// 获取 AI 对话面板宽度、高度与展开状态
#[tauri::command]
pub fn get_chat_panel() -> Result<(f64, f64, bool), String> {
    let config = config::load();
    Ok((config.chat_panel_width, config.chat_panel_height, config.chat_panel_open))
}

/// 设置 AI 对话面板方位（left / right / top / bottom），持久化到配置
#[tauri::command]
pub fn set_chat_panel_side(side: String) -> Result<(), String> {
    if !matches!(side.as_str(), "left" | "right" | "top" | "bottom") {
        return Err(format!("无效的面板方位: {side}"));
    }
    let _guard = crate::config::lock();
    let mut config = config::load();
    config.chat_panel_side = side;
    config::save(&config)
}

/// 用首条用户消息自动生成会话标题：压缩空白、截断到 24 字、超长加省略号
fn auto_chat_title(content: &str) -> String {
    let one_line: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = one_line.chars();
    let mut t = String::new();
    while t.chars().count() < 24 {
        match chars.next() {
            Some(c) => t.push(c),
            None => break,
        }
    }
    if chars.next().is_some() {
        t.push('…');
    }
    if t.trim().is_empty() {
        "新对话".to_string()
    } else {
        t
    }
}

/// 发送一条对话消息并流式接收回复（SSE → Channel 增量推送）
///
/// 流程：落库用户消息 → 组装历史上下文 → 流式请求 → 增量逐段推 Chunk →
/// 完整回复落库后推 Done；出错时推 Error 并保留已生成部分（前端展示，不入库）
#[tauri::command]
pub async fn send_chat_message(
    state: State<'_, DbState>,
    session_id: i64,
    content: String,
    on_event: tauri::ipc::Channel<crate::chat::ChatStreamEvent>,
) -> Result<(), String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("消息不能为空".into());
    }

    // 1) 加锁读取会话（尽量短持锁，流式请求期间不占锁）
    let session = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        chat::get_session(&conn, session_id).map_err(err_str)?
    };

    // 2) 落库用户消息；若仍为默认标题，则用首条消息自动命名
    let user_msg = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let m = chat::add_message(&conn, session_id, "user", &content).map_err(err_str)?;
        let _ = chat::touch_session(&conn, session_id);
        if session.title == "新对话" {
            let title = auto_chat_title(&content);
            let _ = chat::rename_session(&conn, session_id, &title);
        }
        m
    };

    // 3) 读取最近一段历史作为上下文窗口（长对话不再全量加载，
    //    避免历史越长发送越慢、内存按全量历史成倍膨胀）
    let history = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        chat::list_recent_messages(&conn, session_id, CHAT_CONTEXT_WINDOW).map_err(err_str)?
    };

    // 4) 解析模型配置：平台额度走「一个入口 + 多模型负载切换」，自备供应商精确命中（见 pick_chat_model）
    let models = config::load().chat_models;
    let model = pick_chat_model(&models, &session.model_name)?;
    if crate::chat::is_platform_model(&model) {
        // 平台请求的实际模型是负载切换选出来的，落日志便于排查「这次用的是哪个模型」
        log::info!("平台额度请求使用模型: {}", model.model);
    }

    // 5) 流式请求（history 已含刚落的 user 消息）；回复累积进 reply 单份 buffer，
    //    成功即完整回复，出错时保留已生成部分（partial 语义），不再产生双份全量副本
    let mut reply = String::new();
    let mut usage: Option<crate::chat::ChatUsage> = None;
    let mut send_error: Option<String> = None;

    let chunk_sender = on_event.clone();
    let started = std::time::Instant::now();
    let result = crate::chat::stream_chat(&model, &history, &mut reply, |delta| {
        chunk_sender
            .send(crate::chat::ChatStreamEvent::Chunk {
                content: delta,
            })
            .map_err(|e| e.to_string())
    })
    .await;
    let elapsed_ms = started.elapsed().as_millis() as i64;

    match result {
        Ok(u) => {
            if reply.trim().is_empty() {
                send_error = Some("模型未返回任何内容".to_string());
            } else {
                usage = Some(u);
            }
        }
        Err(e) => send_error = Some(e),
    }

    // 6) 落库完整回复 / 清空本次失败残留
    {
        let mut saved: Option<ChatMessage> = None;
        let mut updated_session: Option<ChatSession> = None;
        {
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            // 清理可能残留的半截 assistant 消息（中断场景）
            let _ = chat::delete_messages_from(&conn, session_id, user_msg.id);
            if let Some(usage) = &usage {
                if !reply.trim().is_empty() {
                    let msg =
                        chat::add_message(&conn, session_id, "assistant", &reply).map_err(err_str)?;
                    // token 统计累加失败不阻断主流程（回复已落库，前端仍需收到 Done）
                    let _ = chat::add_session_usage(
                        &conn,
                        session_id,
                        usage.input,
                        usage.output,
                        usage.cache_read,
                        usage.reasoning,
                        elapsed_ms,
                    );
                    let _ = chat::touch_session(&conn, session_id);
                    let updated = chat::get_session(&conn, session_id).map_err(err_str)?;
                    saved = Some(msg);
                    updated_session = Some(updated);
                } else {
                    send_error = Some("模型未返回任何内容".to_string());
                }
            }
        }

        if let Some(msg) = saved {
            on_event
                .send(crate::chat::ChatStreamEvent::Done {
                    message: msg,
                    session: updated_session.expect("done 事件必须携带会话"),
                })
                .map_err(|e| e.to_string())?;
        } else if let Some(e) = send_error {
            on_event
                .send(crate::chat::ChatStreamEvent::Error {
                    message: e,
                    partial: reply,
                })
                .map_err(|e2| e2.to_string())?;
        }
    }

    Ok(())
}

// ---------- 剪贴板历史 ----------

/// 浮层状态（暂停 / 保留策略 / 总条数），前端底部栏展示
#[derive(serde::Serialize)]
pub struct ClipboardInfo {
    pub paused: bool,
    pub max_items: i64,
    pub ttl_days: i64,
    pub total: i64,
    pub shortcut: String,
}

/// 历史列表：Q8 异步加载，首次唤起只拉最近 50 条；滚动到底按 offset 续拉；搜索时传 keyword
#[tauri::command]
pub fn clipboard_list(
    state: State<'_, DbState>,
    keyword: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ClipboardItem>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    clipboard::list(
        &conn,
        keyword.as_deref(),
        limit.unwrap_or(50),
        offset.unwrap_or(0),
    )
    .map_err(err_str)
}

/// 按条目类型把内容写入系统剪贴板（文本 / 图片 / 文件）
fn set_item_clipboard(item: &ClipboardItem) -> Result<(), String> {
    match item.kind.as_str() {
        "image" => {
            let path = item.image_path.as_deref().ok_or("图片快照缺失")?;
            crate::clipboard::set_clipboard_image(path)
        }
        "file" => crate::clipboard::set_clipboard_files(&item.file_paths),
        _ => crate::clipboard::set_clipboard(&item.content, item.html.as_deref()),
    }
}

/// 仅复制到系统剪贴板（不注入粘贴）
#[tauri::command]
pub fn clipboard_copy(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let item = clipboard::get(&conn, id).map_err(err_str)?;
    set_item_clipboard(&item)
}

/// 粘贴到唤起前窗口：写入剪贴板 → 条目挪到最前 → 本应用主窗口直接插入 / 外部窗口注入 Ctrl+V
#[tauri::command]
pub fn clipboard_paste(app: tauri::AppHandle, state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let item = clipboard::get(&conn, id).map_err(err_str)?;
    set_item_clipboard(&item)?;
    // 使用即前置：粘贴过的条目刷新时间挪到列表最前，配合入库去重不会产生重复条目
    clipboard::touch(&conn, id).map_err(err_str)?;
    // 文本走主窗口 JS 直插 + 外部窗口 Ctrl+V；图片/文件统一走 Ctrl+V 注入（content 传空以绕过主窗口直插分支）
    if item.kind == "text" {
        crate::clipboard::paste_to_previous_window(&app, &item.content, item.html.as_deref());
    } else {
        crate::clipboard::paste_to_previous_window(&app, "", None);
    }
    Ok(())
}

#[tauri::command]
pub fn clipboard_toggle_pin(state: State<'_, DbState>, id: i64) -> Result<ClipboardItem, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    clipboard::toggle_pin(&conn, id).map_err(err_str)
}

#[tauri::command]
pub fn clipboard_delete(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    // 删除前清理图片快照文件，避免磁盘泄漏
    if let Ok(item) = clipboard::get(&conn, id) {
        if let Some(path) = item.image_path.as_deref() {
            let _ = std::fs::remove_file(path);
        }
    }
    clipboard::delete(&conn, id).map_err(err_str)
}

#[tauri::command]
pub fn clipboard_clear(state: State<'_, DbState>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    // 清空前清理所有图片快照文件
    if let Ok(paths) = clipboard::image_paths(&conn) {
        for p in paths {
            let _ = std::fs::remove_file(&p);
        }
    }
    clipboard::clear(&conn).map_err(err_str)
}

/// 暂停/恢复记录（配置持久化，监听线程每次变更前读取）
#[tauri::command]
pub fn clipboard_set_paused(paused: bool) -> Result<(), String> {
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.clipboard_paused = paused;
    crate::config::save(&config)?;
    // 恢复记录时清掉自复制指纹：暂停期间产生的指纹可能抑制恢复后的首次复制
    if !paused {
        crate::clipboard::clear_self_set_fingerprint();
    }
    log::info!("剪贴板记录 {}", if paused { "已暂停" } else { "已恢复" });
    Ok(())
}

/// 激活剪贴板浮层（用户点击搜索框开始键盘操作时调用）：
/// 清除 WS_EX_NOACTIVATE 并把浮层带到前台
#[tauri::command]
pub fn clipboard_activate(app: tauri::AppHandle) -> Result<(), String> {
    crate::clipboard::activate_overlay(&app);
    Ok(())
}

/// 收起剪贴板浮层并恢复唤起前窗口焦点（Esc 关闭时调用）
#[tauri::command]
pub fn clipboard_hide(app: tauri::AppHandle) -> Result<(), String> {
    crate::clipboard::hide_overlay(&app);
    Ok(())
}

/// 更新粘贴快捷键方式（auto / ctrl_v / ctrl_shift_v / shift_insert）
#[tauri::command]
pub fn set_clipboard_paste_method(method: String) -> Result<String, String> {
    let method = method.trim().to_string();
    if !["auto", "ctrl_v", "ctrl_shift_v", "shift_insert"].contains(&method.as_str()) {
        return Err("无效的粘贴方式".into());
    }
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.clipboard_paste_method = method.clone();
    crate::config::save(&config)?;
    Ok(config.clipboard_paste_method)
}

/// 更新图片/文件记录开关（配置持久化，监听线程每次剪贴板变化时读取）
#[tauri::command]
pub fn set_clipboard_media_enabled(image: bool, file: bool) -> Result<(), String> {
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.clipboard_image_enabled = image;
    config.clipboard_file_enabled = file;
    crate::config::save(&config)?;
    log::info!(
        "剪贴板记录开关：图片={} 文件={}",
        config.clipboard_image_enabled,
        config.clipboard_file_enabled
    );
    Ok(())
}

/// 导出图片快照到用户指定路径（不移动原文件）。
/// 截图类应用往剪贴板写的是 CF_DIB 位图、快照落盘为 .bmp，导出为 .png 时在此
/// 转码（`clipboard::transcode_image_bytes`），格式一致则原样写入。
#[tauri::command]
pub fn clipboard_export_image(
    state: State<'_, DbState>,
    id: i64,
    dest: String,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let item = clipboard::get(&conn, id).map_err(err_str)?;
    let src = item.image_path.as_deref().ok_or("图片快照缺失")?;
    let bytes = std::fs::read(src).map_err(|e| format!("读取图片快照失败: {}", e))?;
    let ext = std::path::Path::new(&dest)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let data = crate::clipboard::transcode_image_bytes(&bytes, &ext)?;
    std::fs::write(&dest, data).map_err(|e| format!("保存图片失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn clipboard_get_info(state: State<'_, DbState>) -> Result<ClipboardInfo, String> {
    let cfg = crate::config::load();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let total = clipboard::count(&conn).map_err(err_str)?;
    Ok(ClipboardInfo {
        paused: cfg.clipboard_paused,
        max_items: cfg.clipboard_max_items,
        ttl_days: cfg.clipboard_ttl_days,
        total,
        shortcut: cfg.clipboard_shortcut,
    })
}

/// 更新剪贴板全局快捷键（注册/反注册与配置持久化）
#[tauri::command]
pub fn set_clipboard_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Clipboard)
}

/// 更新剪贴板保留策略（条数上限 / 保留天数），保存后立即执行一次清理
#[tauri::command]
pub fn set_clipboard_retention(
    state: State<'_, DbState>,
    max_items: i64,
    ttl_days: i64,
) -> Result<(), String> {
    let max_items = max_items.clamp(20, 5000);
    let ttl_days = ttl_days.clamp(1, 365);
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.clipboard_max_items = max_items;
    config.clipboard_ttl_days = ttl_days;
    crate::config::save(&config)?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    clipboard::cleanup(&conn).map_err(err_str)
}

// ---------- 在线服务（天气 / 名言 / 连通性） ----------

/// 外网连通性探活（前端据此切换在线/离线显隐）
#[tauri::command]
pub async fn check_connectivity() -> bool {
    if !config::load().online_enabled { return false; }
    crate::online::check_connectivity().await
}

/// 获取当前天气：优先用已缓存的经纬度请求；未配置城市/未开启联网返回 None
#[tauri::command]
pub async fn get_weather() -> Result<Option<crate::online::WeatherCurrent>, String> {
    let config = config::load();
    if !config.online_enabled {
        return Ok(None);
    }
    if config.weather_lat == 0.0 || config.weather_lng == 0.0 {
        return Ok(None);
    }
    let weather =
        crate::online::fetch_weather(config.weather_lat, config.weather_lng, &config.weather_city)
            .await?;
    Ok(Some(weather))
}

/// 随机获取一条名言（hitokoto）
#[tauri::command]
pub async fn get_quote() -> Result<crate::online::Quote, String> {
    if !config::load().online_enabled { return Err("联网功能已关闭".into()); }
    crate::online::fetch_quote().await
}

/// 按城市名解析经纬度并缓存到配置（设置里手动配城市）
#[tauri::command]
pub async fn set_weather_city(city: String) -> Result<crate::online::GeoLocation, String> {
    if !config::load().online_enabled { return Err("联网功能已关闭，请开启后设置城市".into()); }
    let city = city.trim().to_string();
    if city.is_empty() {
        return Err("城市名不能为空".to_string());
    }
    let loc = crate::online::geocode_city(&city).await?;
    let _guard = crate::config::lock();
    let mut config = config::load();
    config.weather_city = loc.name.clone();
    config.weather_lat = loc.lat;
    config.weather_lng = loc.lng;
    crate::config::save(&config)?;
    Ok(loc)
}

/// IP 自动定位并缓存经纬度（设置里「自动定位」按钮）
#[tauri::command]
pub async fn locate_weather_by_ip() -> Result<crate::online::GeoLocation, String> {
    if !config::load().online_enabled { return Err("联网功能已关闭".into()); }
    let loc = crate::online::ip_locate().await?;
    let _guard = crate::config::lock();
    let mut config = config::load();
    config.weather_city = loc.name.clone();
    config.weather_lat = loc.lat;
    config.weather_lng = loc.lng;
    crate::config::save(&config)?;
    Ok(loc)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 平台模型的 Key 是占位符（真凭据是账号登录态、请求时现取），**任何界面都不该拿到它**：
    /// 展示出来只会有两种坏结果 —— 让人以为平台 Key 泄露了，或让人以为这里必须填 Key。
    #[test]
    fn ui_never_receives_platform_key_placeholder() {
        let sentinel = crate::chat::PLATFORM_KEY_SENTINEL.to_string();
        assert!(api_key_for_ui(Some(sentinel)).is_err());
        assert_eq!(
            api_key_for_ui(Some("sk-real-key".to_string())).unwrap(),
            "sk-real-key"
        );
        assert!(api_key_for_ui(None).is_err());
    }

    /// 平台占位符不许当 Key 发去探测通用 `/models`：占位符不是凭据（真凭据是登录态），
    /// 发出去只会得到 401，报错完全指不到真因（2026-09-17 用户反馈的 404 也是同一条错路：
    /// 平台中转根本没有 `GET /v1/models`，平台列表接口是 `/api/v1/ai/models`）。
    #[test]
    fn probe_key_rejects_platform_placeholder() {
        let sentinel = crate::chat::PLATFORM_KEY_SENTINEL.to_string();
        assert!(probe_key("", Some(sentinel.clone())).is_err());
        assert!(probe_key(&sentinel, None).is_err());
        // 前端传了真 Key 就优先用它（尚未保存的供应商场景）
        assert_eq!(probe_key(" sk-front ", None).unwrap(), "sk-front");
        // 未传且钥匙串里也没有 → 明确提示
        assert!(probe_key("", None).is_err());
        assert_eq!(probe_key("", Some("sk-stored".into())).unwrap(), "sk-stored");
    }

    // ---- 平台额度：一个入口 + 多模型负载切换（自备供应商精确命中照旧）----

    fn chat_model(id: &str, name: &str, is_default: bool) -> ChatModelConfig {
        ChatModelConfig {
            id: id.to_string(),
            name: name.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            model: name.to_string(),
            api_key: String::new(),
            is_default,
            has_api_key: true,
            provider_name: "自备供应商".to_string(),
        }
    }

    fn platform_chat_model(model: &str) -> ChatModelConfig {
        ChatModelConfig {
            id: format!("platform:{model}"),
            name: format!("{model}（平台额度）"),
            base_url: "https://x-hub.example/v1".to_string(),
            model: model.to_string(),
            api_key: crate::chat::PLATFORM_KEY_SENTINEL.to_string(),
            is_default: false,
            has_api_key: true,
            provider_name: crate::chat::PLATFORM_ENTRY_NAME.to_string(),
        }
    }

    /// 平台侧只有一个模型 → 永远用它；多个 → 在它们之间轮询（用户不再选具体模型）
    #[test]
    fn platform_entry_load_balances_across_models() {
        let one = vec![platform_chat_model("LongCat-2.0")];
        for _ in 0..3 {
            let m = pick_chat_model(&one, crate::chat::PLATFORM_ENTRY_NAME).unwrap();
            assert_eq!(m.model, "LongCat-2.0");
        }

        let many = vec![
            platform_chat_model("a"),
            platform_chat_model("b"),
            platform_chat_model("c"),
        ];
        let picked: Vec<String> = (0..4)
            .map(|_| {
                pick_chat_model(&many, crate::chat::PLATFORM_ENTRY_NAME)
                    .unwrap()
                    .model
            })
            .collect();
        // 起点由进程内游标决定，断言只依赖「4 次覆盖全部 3 个且回到起点」——
        // 这正是负载切换的语义：不偏向其中任何一个
        assert_eq!(picked[0], picked[3]);
        let uniq: std::collections::HashSet<&String> = picked[0..3].iter().collect();
        assert_eq!(uniq.len(), 3);
    }

    /// 老会话存的是具体平台模型名（历史数据）→ 同样按平台口径轮询，不固化成当时那一个
    #[test]
    fn legacy_session_with_concrete_platform_name_still_load_balances() {
        let models = vec![platform_chat_model("a"), platform_chat_model("b")];
        let first = pick_chat_model(&models, "a（平台额度）").unwrap().model;
        let second = pick_chat_model(&models, "a（平台额度）").unwrap().model;
        assert_ne!(first, second);
    }

    /// 开关没开（配置里没有平台条目）时给明确指路的错误，而不是静默换用别的模型
    #[test]
    fn platform_entry_without_models_points_at_the_switch() {
        let only_custom = vec![chat_model("m1", "DeepSeek", true)];
        let e = pick_chat_model(&only_custom, crate::chat::PLATFORM_ENTRY_NAME).unwrap_err();
        assert!(e.contains("平台额度未开启"), "实际: {e}");
    }

    /// 自备供应商照旧：精确命中用户选的那个模型，选不到才回退默认
    #[test]
    fn custom_provider_still_matches_exactly() {
        let models = vec![
            chat_model("m1", "DeepSeek", true),
            chat_model("m2", "Kimi", false),
        ];
        assert_eq!(pick_chat_model(&models, "Kimi").unwrap().name, "Kimi");
        assert_eq!(pick_chat_model(&models, "已删除的模型").unwrap().name, "DeepSeek");
        assert!(pick_chat_model(&[], "任意").is_err());
    }

    /// 新会话默认名：自备默认优先；默认就是平台条目（或只有平台条目）时存入口名，不固化具体模型
    #[test]
    fn default_session_name_uses_platform_entry() {
        assert_eq!(
            default_session_model_name(&[chat_model("m1", "DeepSeek", true)]),
            "DeepSeek"
        );
        assert_eq!(
            default_session_model_name(&[platform_chat_model("a")]),
            crate::chat::PLATFORM_ENTRY_NAME
        );
        assert_eq!(
            default_session_model_name(&[chat_model("m1", "DeepSeek", true), platform_chat_model("a")]),
            "DeepSeek"
        );
        let mut p = platform_chat_model("a");
        p.is_default = true;
        assert_eq!(
            default_session_model_name(&[chat_model("m1", "DeepSeek", false), p]),
            crate::chat::PLATFORM_ENTRY_NAME
        );
        assert_eq!(default_session_model_name(&[]), "");
    }
}
