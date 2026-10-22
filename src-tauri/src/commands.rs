use crate::browsers::{self, InstalledBrowser};
use crate::config;
use crate::config::AppConfig;
use crate::models::{
    ChatMessage, ChatModelConfig, ChatSession, ClipboardItem, Countdown, DetachedSticky, Note,
    NoteFolder, NoteImageGcReport, NoteLinks, PurgeReport, RepeatRule, Resource, ResourceKind,
    ResourceSubcategory, ResourceZone, SearchResult, Snippet, Sticky, Tag, Todo, TodoOccurrence,
    TodoTag, TodoTagLink,
};
use crate::process;
use crate::repo::{
    chat, clipboard, countdown, detached_sticky, note, note_folder, note_link, resource, snippet,
    sticky, subcategory, tag, todo, todo_tag, zone,
};
use crate::todo_recurrence;
use rusqlite::{params, Connection};
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
    let note_folders = note_folder::list(&conn).map_err(err_str)?;
    let tags = tag::list(&conn).map_err(err_str)?;
    let todos = todo::list(&conn).map_err(err_str)?;
    let stickies = sticky::list(&conn).map_err(err_str)?;
    let detached = detached_sticky::list(&conn).map_err(err_str)?;
    let countdowns = countdown::list(&conn).map_err(err_str)?;
    let config = crate::config::load();
    log::info!(
        "初始化数据加载完成: resources={} notes={} folders={} tags={} todos={} stickies={} detached={} countdowns={}",
        resources.len(),
        notes.len(),
        note_folders.len(),
        tags.len(),
        todos.len(),
        stickies.len(),
        detached.len(),
        countdowns.len()
    );
    Ok(InitialData {
        resources,
        notes,
        note_folders,
        tags,
        todos,
        stickies,
        detached,
        countdowns,
        config,
    })
}

/// 速达资源全量清单（resources-changed 刷新用；agent 工具改库后前端重拉此命令）
#[tauri::command]
pub fn list_resources(state: State<'_, DbState>) -> Result<Vec<crate::models::Resource>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::list_all(&conn).map_err(err_str)
}

#[derive(serde::Serialize)]
pub struct InitialData {
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub note_folders: Vec<NoteFolder>,
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
    zone_id: Option<i64>,
    description: Option<String>,
    remark: Option<String>,
    remark_label: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    if let Some(zid) = zone_id {
        if !zone::exists(&conn, zid).map_err(err_str)? {
            return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
        }
    }
    let res = resource::create(
        &conn,
        kind,
        &name,
        &target,
        category.as_deref(),
        icon.as_deref(),
        args.as_deref(),
        zone_id,
        description.as_deref(),
        remark.as_deref(),
        remark_label.as_deref(),
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
    zone_id: Option<i64>,
    description: Option<String>,
    remark: Option<String>,
    remark_label: Option<String>,
) -> Result<Resource, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let kind = parse_kind(&kind)?;
    if let Some(zid) = zone_id {
        if !zone::exists(&conn, zid).map_err(err_str)? {
            return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
        }
    }
    let res = resource::update(
        &conn,
        id,
        kind,
        &name,
        &target,
        category.as_deref(),
        icon.as_deref(),
        args.as_deref(),
        zone_id,
        description.as_deref(),
        remark.as_deref(),
        remark_label.as_deref(),
    )
    .map_err(err_str)?;
    log::info!("更新资源: id={} {} ({:?})", res.id, res.name, res.kind);
    Ok(res)
}

/// 备注明文按需解密（编辑弹窗打开时拉一次）：明文不随资源列表/get 下发
#[tauri::command]
pub fn get_resource_remark(
    state: State<'_, DbState>,
    resource_id: i64,
) -> Result<Option<String>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    resource::remark_plaintext(&conn, resource_id).map_err(err_str)
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
            // 程序已在运行且有可见（含最小化）窗口 → 只把已有窗口调度到前台，不再拉起第二个
            // 实例。窗口全隐藏（托盘挂后台，如微信/WorkBuddy）时 activate_existing 返回 false，
            // 照常启动 exe——应用自带的单实例逻辑会把主窗正规唤起（外部 SW_SHOW 隐藏窗只会
            // 得到点不动/不重绘的空壳，实测记录见 process.rs）。
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
            // 打包应用（shell:AppsFolder\<AUMID>）没有提权语义：Start-Process -Verb RunAs
            // 对 shell: URI 直接失败且 PS 子进程照常 spawn 成功，会「报成功但没反应」
            if process::is_shell_app_target(&res.target) {
                return Err("MSIX 打包应用不支持以管理员身份运行".into());
            }
            process::launch_elevated(&res.target, res.args.as_deref())?;
            let _ = resource::touch(&conn, id);
            log::info!("以管理员身份启动程序: {} ({})", res.name, res.target);
            Ok(())
        }
        _ => Err("只有「程序」类型的资源支持以管理员身份运行".into()),
    }
}

/// 在资源管理器中打开速达资源所在位置并选中该项（右键「打开文件所在位置」）。
/// 仅本地路径型资源（程序 / 文件）有意义；网页资源没有「所在目录」概念，直接拒绝。
#[tauri::command]
pub fn reveal_resource_in_explorer(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let res = resource::get(&conn, id).map_err(err_str)?;
    if matches!(res.kind, ResourceKind::Web) {
        return Err("网页资源没有所在目录".to_string());
    }
    process::reveal_in_explorer(&res.target).map_err(|e| {
        log::warn!("打开文件所在位置失败: {} ({}) -> {}", res.name, res.target, e);
        e
    })?;
    log::info!("打开文件所在位置: {} ({})", res.name, res.target);
    Ok(())
}

// ---------- 速达小类（ADR 0012）----------

/// 小类名（可含「/」层级）的形状校验：全路径 1–60 字符、每段 1–20、分段首尾禁空格。
/// create 与 rename 共用——资源按全路径字符串匹配，形状不一会让行名与树推导的路径对不上。
fn validate_subcategory_name(name: &str) -> Result<(), String> {
    let chars = name.chars().count();
    if chars == 0 || chars > 60 {
        return Err("小类名称需为 1–60 个字符".into());
    }
    for seg in name.split('/') {
        if seg.trim() != seg {
            return Err("小类路径分段的前后不能有空格（用 / 分隔层级）".into());
        }
        let n = seg.chars().count();
        if n == 0 || n > 20 {
            return Err("小类路径的每一段需为 1–20 个字符（用 / 分隔层级）".into());
        }
    }
    Ok(())
}

fn validate_subcategory_input(kind: &str, name: &str) -> Result<(), String> {
    if !subcategory::VALID_KINDS.contains(&kind) {
        return Err("无效的大类".into());
    }
    validate_subcategory_name(name)
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
    validate_subcategory_name(&name)?;
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

// ---------- 速达分区（「全部」tab 自定义成组陈列，独立于小类） ----------

/// 分区名形状校验：trim 后 1–20 字符（无 kind 维度、无层级，`/` 是普通字符不禁）
fn validate_zone_name(name: &str) -> Result<(), String> {
    let chars = name.chars().count();
    if chars == 0 || chars > 20 {
        return Err("分区名称需为 1–20 个字符".into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_zones(state: State<'_, DbState>) -> Result<Vec<ResourceZone>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::list(&conn).map_err(err_str)
}

#[tauri::command]
pub fn create_zone(state: State<'_, DbState>, name: String) -> Result<ResourceZone, String> {
    let name = name.trim().to_string();
    validate_zone_name(&name)?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if !zone::is_name_free(&conn, &name, None).map_err(err_str)? {
        return Err(format!("DUP: 已有名为「{name}」的分区"));
    }
    let z = zone::create(&conn, &name).map_err(err_str)?;
    log::info!("新建分区: {}", z.name);
    Ok(z)
}

#[tauri::command]
pub fn rename_zone(state: State<'_, DbState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    validate_zone_name(&name)?;
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::rename(&mut conn, id, &name)
}

/// 删除分区：成员批量落「未分区」（资源本身不动，单事务，见 repo::zone::delete）
#[tauri::command]
pub fn delete_zone(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::delete(&mut conn, id)
}

#[tauri::command]
pub fn reorder_zones(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::reorder(&conn, &ids).map_err(err_str)
}

/// 调整分区框尺寸（cols/rows 为卡片格数，1..=12；新建默认 3×2）。
/// 尺寸是下限语义：内容超出时前端按行自动膨胀，这里只改空框占位。
#[tauri::command]
pub fn resize_zone(
    state: State<'_, DbState>,
    id: i64,
    cols: i64,
    rows: i64,
) -> Result<(), String> {
    if !(1..=12).contains(&cols) || !(1..=12).contains(&rows) {
        return Err("分区尺寸需在 1–12 格之间".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::resize(&conn, id, cols, rows)
}

/// 设置分区级图标缩放覆盖（issue #37；None = 清除覆盖，跟随全局 suda_icon_scale）
#[tauri::command]
pub fn set_zone_scale(
    state: State<'_, DbState>,
    id: i64,
    scale: Option<f64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    zone::set_scale(&conn, id, scale)
}

/// 批量改分区归属（右键「移动到分区」/ 删分区撤销恢复），不动 sort_order
#[tauri::command]
pub fn set_resources_zone(
    state: State<'_, DbState>,
    ids: Vec<i64>,
    zone_id: Option<i64>,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    if let Some(zid) = zone_id {
        if !zone::exists(&conn, zid).map_err(err_str)? {
            return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
        }
    }
    resource::set_zone(&conn, &ids, zone_id).map_err(err_str)
}

/// 分区模式拖拽的原子写回载荷：顺序即新的全表 sort_order，zone_id 为目标分区（null=未分区）
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZonedOrderEntry {
    pub id: i64,
    pub zone_id: Option<i64>,
}

/// 「全部」tab 分区模式下的拖拽落盘：全表 sort_order 与每项的分区归属单事务同写。
/// entries 必须覆盖全表（「全部」tab 下所有资源都可见，前端天然满足），
/// 目标分区不存在时整批拒绝——宁可让前端报错重拉，也不留「有 zone_id 却无分区」的孤儿成员。
#[tauri::command]
pub fn reorder_resources_zoned(
    state: State<'_, DbState>,
    entries: Vec<ZonedOrderEntry>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut known: Vec<i64> = Vec::new();
    for zid in entries.iter().filter_map(|e| e.zone_id) {
        if !known.contains(&zid) {
            if !zone::exists(&conn, zid).map_err(err_str)? {
                return Err(format!("NOT_FOUND: 分区 {zid} 不存在"));
            }
            known.push(zid);
        }
    }
    let pairs: Vec<(i64, Option<i64>)> = entries.iter().map(|e| (e.id, e.zone_id)).collect();
    resource::reorder_zoned(&conn, &pairs).map_err(err_str)?;
    log::info!("资源分区排序更新: {} 项", entries.len());
    Ok(())
}

/// 速达网页默认打开方式（panel=内嵌面板 / window=独立窗口 / system=系统默认浏览器，ADR 0011 2026-09-25 拍板）
#[tauri::command]
pub fn set_suda_web_open_mode(mode: String) -> Result<String, String> {
    let mode = mode.trim().to_string();
    if !["panel", "window", "system"].contains(&mode.as_str()) {
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

/// 新建笔记（速记视图口径）：一次性落 folder / source_url / 初始正文。
/// folder_id 传 null = 树根（未选中文件夹时的新建落根，方案 Q13–Q18）。
#[tauri::command]
pub fn create_note_in(
    state: State<'_, DbState>,
    title: String,
    content: Option<String>,
    folder_id: Option<i64>,
    source_url: Option<String>,
) -> Result<Note, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let note = note::create_in(
        &conn,
        &title,
        content.as_deref().unwrap_or(""),
        folder_id,
        source_url.as_deref().unwrap_or(""),
    )
    .map_err(err_str)?;
    log::info!("新建笔记(带目录): id={} folder={:?}", note.id, note.folder_id);
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
    // 改名感知：标题变化时同事务做全库 [[旧标题]]→[[新标题]] 替换与链索引重建（双链断链防护）
    let note = note::update_with_link_fixup(&conn, id, &title, &content).map_err(err_str)?;
    log::debug!("更新笔记: id={} 内容 {} 字", id, content.chars().count());
    Ok(note)
}

/// 移入回收站（软删）。UI 删除按钮与撤销恢复走这里；硬删见 purge_note。
#[tauri::command]
pub fn trash_note(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::trash(&conn, id).map_err(err_str)?;
    log::info!("笔记移入回收站: id={}", id);
    Ok(())
}

#[tauri::command]
pub fn restore_note(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::restore(&conn, id).map_err(err_str)?;
    log::info!("笔记从回收站还原: id={}", id);
    Ok(())
}

/// 永久删除（回收站内）：连带 note_tags、note_links 出链。扩展桥 notes.delete 同语义。
#[tauri::command]
pub fn purge_note(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::delete(&conn, id).map_err(err_str)?;
    log::info!("笔记永久删除: id={}", id);
    Ok(())
}

/// 按设置的保留天数清理回收站（note_trash_retention_days，0 = 永久保留）。
/// 启动时与设置变更时各跑一次；也可手动触发。
#[tauri::command]
pub fn purge_expired_notes(state: State<'_, DbState>) -> Result<PurgeReport, String> {
    let days = config::load().note_trash_retention_days;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let purged = note::purge_expired(&conn, days).map_err(err_str)?;
    if purged > 0 {
        log::info!("回收站清理: {} 条（保留 {} 天）", purged, days);
    }
    Ok(PurgeReport { purged })
}

/// 删除笔记（硬删语义保持向后兼容：扩展桥 notes.delete 与旧调用方期望「删干净」）。
/// UI 删除一律走 trash_note（软删进回收站）。
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

/// 单条笔记全量（含正文）：回收站还原回填活列表、刷新列表补拉外部新建条目用。
/// 只传 note_id 不存在时返回 None，其余错误照常上报
#[tauri::command]
pub fn get_note(state: State<'_, DbState>, note_id: i64) -> Result<Option<Note>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    Ok(note::get(&conn, note_id).ok())
}

/// 回收站列表（含正文，还原/永久删除界面用）
#[tauri::command]
pub fn list_trashed_notes(state: State<'_, DbState>) -> Result<Vec<Note>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::list_trashed(&conn).map_err(err_str)
}

/// 设置/清除笔记自定义树图标（emoji，None = 恢复默认）
#[tauri::command]
pub fn set_note_icon(state: State<'_, DbState>, id: i64, icon: Option<String>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::set_icon(&conn, id, icon.as_deref()).map_err(err_str)
}

/// 一键清空回收站：逐条硬删（不可恢复；调用方负责先向用户确认）。单事务——
/// 中断（错误/进程退出）要么全清要么全留，不留半截
#[tauri::command]
pub fn purge_all_trashed_notes(state: State<'_, DbState>) -> Result<usize, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.unchecked_transaction().map_err(err_str)?;
    let ids: Vec<i64> = {
        let mut stmt = tx
            .prepare("SELECT id FROM notes WHERE deleted_at IS NOT NULL")
            .map_err(err_str)?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(err_str)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err_str)?;
        rows
    };
    for id in &ids {
        tx.execute("DELETE FROM notes WHERE id = ?1", params![id])
            .map_err(err_str)?;
    }
    tx.commit().map_err(err_str)?;
    if !ids.is_empty() {
        log::info!("回收站已清空: {} 条", ids.len());
    }
    Ok(ids.len())
}

// ---------- 笔记文件夹（速记三栏视图左栏，ADR 0015） ----------

#[tauri::command]
pub fn list_note_folders(state: State<'_, DbState>) -> Result<Vec<NoteFolder>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note_folder::list(&conn).map_err(err_str)
}

#[tauri::command]
pub fn create_note_folder(
    state: State<'_, DbState>,
    name: String,
    parent_id: Option<i64>,
) -> Result<NoteFolder, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("文件夹名不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let f = note_folder::create(&conn, name, parent_id).map_err(err_str)?;
    log::info!("新建笔记文件夹: {} (parent={:?})", f.name, f.parent_id);
    Ok(f)
}

#[tauri::command]
pub fn rename_note_folder(state: State<'_, DbState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("文件夹名不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note_folder::rename(&conn, id, name).map_err(err_str)
}

/// 删除文件夹：笔记与子文件夹上移一级，不级联删（ADR 0015）。
/// 文件夹本身不进回收站；其成员原样保留。
#[tauri::command]
pub fn delete_note_folder(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note_folder::delete(&conn, id).map_err(err_str)?;
    log::info!("删除笔记文件夹: id={}（成员上移一级）", id);
    Ok(())
}

/// 拖拽移动 + 排序的原子写回（环检测在 repo 层：不能拖进自己或自己的后代）。
/// ⚠️ 字段名按**蛇形**反序列化，与前端 `reorderNoteFolders` 载荷（tauri.ts / store /
/// NoteFolderTree 的 emit 类型一路都是 `parent_id`/`sort_order`，对齐 NoteFolder 模型）
/// 一致——嵌套载荷不做 Tauri 的驼峰自动转换，标 `rename_all = "camelCase"` 会让
/// `sort_order` 读成缺失、整批反序列化失败，表现为文件夹拖拽完全无效果。
#[derive(serde::Deserialize)]
pub struct NoteFolderMove {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
}

#[tauri::command]
pub fn reorder_note_folders(
    state: State<'_, DbState>,
    moves: Vec<NoteFolderMove>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let entries: Vec<note_folder::FolderMove> = moves
        .into_iter()
        .map(|m| note_folder::FolderMove {
            id: m.id,
            parent_id: m.parent_id,
            sort_order: m.sort_order,
        })
        .collect();
    note_folder::reorder(&conn, &entries).map_err(err_str)
}

/// 移动单条笔记到文件夹（folder_id = null 回树根）
#[tauri::command]
pub fn set_note_folder(
    state: State<'_, DbState>,
    note_id: i64,
    folder_id: Option<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::set_folder(&conn, note_id, folder_id).map_err(err_str)
}

/// 笔记组内拖拽排序：按传入顺序写手动排序位（前端按分组计算完整顺序）
#[tauri::command]
pub fn reorder_note_orders(state: State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note::reorder(&conn, &ids).map_err(err_str)?;
    log::debug!("笔记排序更新: {} 条", ids.len());
    Ok(())
}

// ---------- 笔记图片孤儿 GC（只手动触发；dry_run 先出报告，确认后才真删） ----------

/// 收集 notes/images 下全部图片文件名，与「未永久删除笔记（含回收站）」正文里的
/// xhub-note 引用做差集——差集即孤儿。回收站内笔记的引用必须算活引用（方案 §10 风险 4）。
pub fn scan_orphan_note_images(conn: &Connection) -> Result<NoteImageGcReport, String> {
    use std::collections::HashSet;

    let dir = crate::paths::data_root().join("notes").join("images");
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                // 文件名严格为 16 位哈希 + 扩展（与 import_note_image 的落盘口径一致）
                if is_note_image_name(name) {
                    files.push(name.to_string());
                }
            }
        }
    }
    let mut referenced: HashSet<String> = HashSet::new();
    for content in note::all_contents_including_trashed(conn).map_err(err_str)? {
        for hash in extract_note_image_hashes(&content) {
            referenced.insert(hash);
        }
    }
    let orphans: Vec<String> = files
        .iter()
        .filter(|f| {
            let stem = f.split('.').next().unwrap_or("");
            !referenced.contains(stem)
        })
        .cloned()
        .collect();
    Ok(NoteImageGcReport {
        dry_run: true,
        total_files: files.len(),
        referenced: referenced.len(),
        orphan_files: orphans,
        removed: 0,
        failed: 0,
    })
}

/// 孤儿图片扫描（dry_run=true 恒定：本命令只报告，真删走 gc_orphan_note_images_commit）
#[tauri::command]
pub fn gc_orphan_note_images(state: State<'_, DbState>, dry_run: bool) -> Result<NoteImageGcReport, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut report = scan_orphan_note_images(&conn)?;
    report.dry_run = dry_run;
    if dry_run {
        return Ok(report);
    }
    let dir = crate::paths::data_root().join("notes").join("images");
    let mut removed = 0usize;
    let mut failed = 0usize;
    for name in &report.orphan_files {
        if std::fs::remove_file(dir.join(name)).is_ok() {
            removed += 1;
        } else {
            failed += 1;
        }
    }
    report.removed = removed;
    report.failed = failed;
    log::info!(
        "笔记孤儿图片清理: 孤儿 {} 张，删除 {} 张，失败 {} 张",
        report.orphan_files.len(),
        removed,
        failed
    );
    Ok(report)
}

/// 笔记图片文件名口径：16 位十六进制哈希 + 白名单扩展
fn is_note_image_name(name: &str) -> bool {
    const EXTS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s, e.to_lowercase()),
        None => return false,
    };
    stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && EXTS.contains(&ext.as_str())
}

/// 从笔记正文里抽取引用的图片哈希（xhub-note.localhost/<hash>.<ext> 两种 host 形态都认）
fn extract_note_image_hashes(content: &str) -> Vec<String> {
    const NEEDLE: &str = "xhub-note.localhost/";
    let mut hashes = Vec::new();
    let mut rest = content;
    while let Some(pos) = rest.find(NEEDLE) {
        let tail = &rest[pos + NEEDLE.len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let stem = name.split('.').next().unwrap_or("");
        if stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) {
            hashes.push(stem.to_string());
        }
        rest = tail;
    }
    hashes
}

// ---------- 双链（轻量版：引用键 = 标题） ----------

#[tauri::command]
pub fn get_note_links(state: State<'_, DbState>, note_id: i64) -> Result<NoteLinks, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let (outgoing, incoming) = note_link::for_note(&conn, note_id).map_err(err_str)?;
    Ok(NoteLinks { outgoing, incoming })
}

/// 存量笔记的双链索引重建（升级后首次使用：老笔记没有索引）。幂等。
#[tauri::command]
pub fn rebuild_note_links(state: State<'_, DbState>) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    note_link::reindex_all(&conn).map_err(err_str)?;
    log::info!("笔记双链索引已全量重建");
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

/// 设置待办提醒通知方式（both/popup/sound/none，非法值归一为 popup）。
/// 不重置 remind_fired：改模式不是改排期，已触发过的提醒不该被重新引爆。
#[tauri::command]
pub fn set_todo_notify_mode(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    mode: String,
) -> Result<Todo, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let todo = todo::set_notify_mode(&conn, id, &mode).map_err(err_str)?;
    drop(conn);
    let _ = app.emit("todos-changed", ());
    log::info!("待办提醒方式: id={} mode={}", todo.id, todo.remind_notify_mode);
    Ok(todo)
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
    Notes,
}

impl ConfiguredShortcut {
    fn field<'a>(&self, cfg: &'a mut crate::config::AppConfig) -> &'a mut String {
        match self {
            ConfiguredShortcut::Main => &mut cfg.global_shortcut,
            ConfiguredShortcut::Clipboard => &mut cfg.clipboard_shortcut,
            ConfiguredShortcut::Search => &mut cfg.search_shortcut,
            ConfiguredShortcut::Chat => &mut cfg.chat_shortcut,
            ConfiguredShortcut::Notes => &mut cfg.notes_shortcut,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ConfiguredShortcut::Main => "主窗口",
            ConfiguredShortcut::Clipboard => "剪贴板",
            ConfiguredShortcut::Search => "搜索",
            ConfiguredShortcut::Chat => "AI 对话",
            ConfiguredShortcut::Notes => "速记",
        }
    }

    /// 该快捷键当前是否处于启用状态（禁用 = 不注册但保留键值）
    fn enabled(&self, cfg: &crate::config::AppConfig) -> bool {
        match self {
            ConfiguredShortcut::Main => cfg.global_shortcut_enabled,
            ConfiguredShortcut::Clipboard => cfg.clipboard_shortcut_enabled,
            ConfiguredShortcut::Search => cfg.search_shortcut_enabled,
            ConfiguredShortcut::Chat => cfg.chat_shortcut_enabled,
            ConfiguredShortcut::Notes => cfg.notes_shortcut_enabled,
        }
    }

    fn set_enabled(&self, cfg: &mut crate::config::AppConfig, value: bool) {
        match self {
            ConfiguredShortcut::Main => cfg.global_shortcut_enabled = value,
            ConfiguredShortcut::Clipboard => cfg.clipboard_shortcut_enabled = value,
            ConfiguredShortcut::Search => cfg.search_shortcut_enabled = value,
            ConfiguredShortcut::Chat => cfg.chat_shortcut_enabled = value,
            ConfiguredShortcut::Notes => cfg.notes_shortcut_enabled = value,
        }
    }

    /// 键值是否与**其它**快捷键里某一个相同（物理按键口径，CommandOrControl 与 Ctrl
    /// 视为同键）。改键/启用前的配置层冲突预检用——配置相同而 OS 各自注册必然撞车，
    /// 与其在启用时报一句含糊的「快捷键冲突」，不如在写入配置时就拦下并点名是谁。
    fn conflicts_with_other(
        &self,
        cfg: &crate::config::AppConfig,
        key: &str,
    ) -> Option<(&'static str, String)> {
        let others = [
            (
                ConfiguredShortcut::Main,
                cfg.global_shortcut.clone(),
                "主窗口",
            ),
            (
                ConfiguredShortcut::Clipboard,
                cfg.clipboard_shortcut.clone(),
                "剪贴板",
            ),
            (ConfiguredShortcut::Search, cfg.search_shortcut.clone(), "搜索"),
            (ConfiguredShortcut::Chat, cfg.chat_shortcut.clone(), "AI 对话"),
            (ConfiguredShortcut::Notes, cfg.notes_shortcut.clone(), "速记"),
        ];
        others
            .into_iter()
            .find(|(which, value, _)| {
                std::mem::discriminant(which) != std::mem::discriminant(self)
                    && !value.is_empty()
                    && crate::shortcut::same_hotkey(value, key)
            })
            .map(|(_, value, label)| (label, value))
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
    // 格式与「单键劫持」校验放在最前：裸空格/裸字母一旦注册就全局吞键，
    // 用户连回设置里改回来都困难（实测：空格单键会让所有程序打不出空格）
    crate::shortcut::validate_custom_shortcut(shortcut)?;

    let mut config = crate::config::load();
    let previous = config_field(&config, &which);
    if previous == shortcut {
        return Ok(previous);
    }
    // 改键前先做配置层冲突预检：其它快捷键已占用同一物理按键时无论本键是否禁用
    // 都拦下（禁用态存进去就是颗雷——重新启用时注册必然撞车，报错还不知所云）
    if let Some((label, _)) = which.conflicts_with_other(&config, shortcut) {
        return Err(format!("与「{label}」快捷键冲突，请换一个组合"));
    }
    // 该快捷键处于「禁用」状态（config.*_shortcut_enabled = false）时只改存储值、不注册，
    // 否则禁用后一改键就又把热键注册上了，开关形同虚设（重新启用时按新值注册）
    if !which.enabled(&config) {
        *which.field(&mut config) = shortcut.to_string();
        crate::config::save(&config)?;
        log::info!("[快捷键] {}快捷键（当前已禁用）已改为 {}", which.label(), shortcut);
        return Ok(shortcut.to_string());
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
        ConfiguredShortcut::Notes => cfg.notes_shortcut.clone(),
    }
}

/// 更新全局搜索呼出快捷键
#[tauri::command]
pub fn set_search_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Search)
}

/// 更新速记呼出快捷键（唤起主窗 → 切速记视图 → 聚焦新建）
#[tauri::command]
pub fn set_notes_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Notes)
}

/// 更新 AI 对话呼出快捷键
#[tauri::command]
pub fn set_chat_shortcut(app: tauri::AppHandle, value: String) -> Result<String, String> {
    set_configured_shortcut(app, value, ConfiguredShortcut::Chat)
}

/// 启用/禁用某个可自定义全局快捷键：禁用 = 注销该热键但保留键值；启用 = 按当前键值重新注册。
/// 与 set_*_shortcut 分开：这里只切开关，不动键值本身。
#[tauri::command]
pub fn set_shortcut_enabled(
    app: tauri::AppHandle,
    kind: String,
    enabled: bool,
) -> Result<(), String> {
    let which = match kind.as_str() {
        "main" => ConfiguredShortcut::Main,
        "clipboard" => ConfiguredShortcut::Clipboard,
        "search" => ConfiguredShortcut::Search,
        "chat" => ConfiguredShortcut::Chat,
        "notes" => ConfiguredShortcut::Notes,
        _ => return Err("未知的快捷键类型".into()),
    };
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    let value = config_field(&config, &which);
    if enabled {
        if value.trim().is_empty() {
            return Err("尚未设置快捷键".into());
        }
        // 存量坏值兜底：老版本可能已把裸空格/裸字母存进配置，启用时同样拦下
        crate::shortcut::validate_custom_shortcut(&value)?;
        // 只在实际未注册时注册（已注册则幂等跳过，避免「已注册」冲突）
        if !crate::shortcut::is_shortcut_registered(&app, &value) {
            if let Err(e) = crate::shortcut::register_toggle_shortcut(&app, &value) {
                // 注册报「已被注册」但登记表说没有：先强反注册再试一次自愈——登记表与
                // 系统热键状态失同步（历史反注册失败留下的残留等）时按原样重试永远失败
                let mut last_err = crate::shortcut::format_shortcut_error(&e);
                if crate::shortcut::is_conflict_error(&e) {
                    let _ = crate::shortcut::unregister_toggle_shortcut(&app, &value);
                    match crate::shortcut::register_toggle_shortcut(&app, &value) {
                        Ok(()) => last_err.clear(),
                        Err(e2) => last_err = crate::shortcut::format_shortcut_error(&e2),
                    }
                }
                if !last_err.is_empty() {
                    // 报错要点名冲突来源：其它三个快捷键占了同键（配置撞车）与外部程序
                    // 占用（本进程从未注册成功过）对用户是完全不同的两件事
                    if let Some((label, _)) = which.conflicts_with_other(&config, &value) {
                        return Err(format!("与「{label}」快捷键键值相同，请先修改其中一个"));
                    }
                    return Err(format!(
                        "启用失败：{last_err}（该组合可能正被其它程序占用）"
                    ));
                }
            }
        }
    } else if crate::shortcut::is_shortcut_registered(&app, &value) {
        // 未注册时忽略：可能当初注册就被别的程序占用而失败过
        if let Err(e) = crate::shortcut::unregister_toggle_shortcut(&app, &value) {
            log::warn!("[快捷键] 禁用时反注册失败（残留会导致下次启用报冲突）: {e}");
        }
    }
    which.set_enabled(&mut config, enabled);
    crate::config::save(&config)?;
    log::info!(
        "[快捷键] {}快捷键已{}",
        which.label(),
        if enabled { "启用" } else { "禁用" }
    );
    Ok(())
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

/// 笔记标签改名（修缺陷③：归属关系 note_tags 不动，所有引用它的笔记自动跟随新名字）
#[tauri::command]
pub fn rename_tag(state: State<'_, DbState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("标签名不能为空".into());
    }
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    tag::rename(&conn, id, name).map_err(err_str)?;
    log::info!("标签改名: id={} → {}", id, name);
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

/// 更改数据存储目录：把**整个数据根**迁移到新目录（数据库走 SQLite 在线备份，
/// 其余条目原样复制，见 [`copy_data_root`]），并把库内与配置里的绝对路径从旧根
/// 改写为新根（见 [`rewrite_migrated_db_paths`] / [`rewrite_migrated_config_paths`]），
/// 最后写引导文件指向新目录，重启后生效。旧目录保留（安全起见不删除，由用户自行清理）。
#[tauri::command]
pub fn change_data_dir(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    new_dir: String,
) -> Result<(), String> {
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
    // 互相包含会把「复制数据根」变成递归复制（目标在源内时现场建目录并读到自己），
    // 也会让「目标已含旧根」产生不可预期的嵌套数据，直接拒绝
    let src_c = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
    let target_c = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    if src_c == target_c {
        return Err("新路径与当前路径相同".into());
    }
    if target_c.starts_with(&src_c) || src_c.starts_with(&target_c) {
        return Err("新目录不能与当前数据目录相同或互相包含".into());
    }

    // 0. 先停掉 service 扩展后端：它们可能在扩展目录里持有打开的文件（扩展自带
    //    SQLite 等），复制会因共享冲突失败。迁移后本就立即重启，提前停止无副作用
    crate::service::stop_all(&app);

    // 1. 数据库在线备份到新目录（运行中数据库被占用，不能直接复制文件；WAL 安全）
    {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        conn.backup("main", &target.join("app.db"), None)
            .map_err(|e| format!("迁移数据库失败: {}", e))?;
    }
    // 目标目录若以前就是数据根（迁移回来 / 复用过），残留的 -wal/-shm 属于旧库，
    // 下次启动会被 SQLite 当作本次快照的 WAL 回放——直接清掉，让备份库干净打开
    for suffix in ["app.db-wal", "app.db-shm", "app.db-journal"] {
        let _ = std::fs::remove_file(target.join(suffix));
    }

    // 2. 数据库之外的整个数据根原样复制：扩展（代码 + `.storage.json` /
    //    `.config.json` / `.shared-storage.json`）、笔记图片、壁纸、市场缓存、
    //    剪贴板图片、安装标识、配置与密钥……漏掉任一目录，重启后都表现为
    //    「数据消失」（扩展全没了 / 速达图标要重新指定 / 笔记配图裂图）
    copy_data_root(src, target)?;

    // 3. 库内绝对路径改写：resources.icon 与 clipboard_history.image_path 都存旧根
    //    绝对路径，而 asset 协议作用域只放行当前数据根——不改写就是图标全丢
    rewrite_migrated_db_paths(&target.join("app.db"), src, target)?;

    // 4. app.json 内的绝对路径（壁纸 / 「我的扩展」源码目录 / Skills 根目录）
    rewrite_migrated_config_paths(&target.join("app.json"), src, target);

    // 5. 写引导文件指向新目录（重启后 init_database 读它）
    crate::paths::set_data_root(target)?;

    log::info!(
        "数据目录已迁移: {} -> {}",
        src.display(),
        target.display()
    );
    Ok(())
}

/// 数据目录迁移时不需要跟着走的顶层条目：
/// - `app.db*`：数据库走 `conn.backup` 在线快照；直接复制运行中的 `app.db` /
///   `-wal` / `-shm` 会把不一致状态带到新目录；
/// - `restore.db` / `restore_icons` / `.restore_pending`：待恢复的一次性中间态；
/// - `logs` / `updates`：日志与已下载/待应用的更新包，可重建；
///   （`runtime/` 内置 Node **要复制**：没有系统 Node 的用户迁移后 service 扩展
///   仍需它才能启动，跳过会变成一次隐性下载）
/// - `data_path.json`：引导文件锚点（固定放 `%APPDATA%\x-hub`），不属于数据本身。
fn data_root_entry_skipped(name: &str) -> bool {
    name.starts_with("app.db")
        || matches!(
            name,
            "restore.db"
                | "restore_icons"
                | ".restore_pending"
                | "logs"
                | "updates"
                | "data_path.json"
        )
}

/// 复制数据根的**全部业务条目**（[`data_root_entry_skipped`] 之外的目录与文件），
/// 覆盖目标同名文件。收集逐项失败而不是遇到第一个错误就中止：把能复制的都复制过去，
/// 最后一次性报错（此时不写引导文件，重启仍回旧目录，不会半迁半丢）。
fn copy_data_root(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("读取当前数据目录失败: {e}"))?;
    let mut failures: Vec<String> = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("读取数据目录项失败: {e}"));
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if data_root_entry_skipped(&name_str) {
            continue;
        }
        copy_entry(&entry.path(), &dst.join(&name), &mut failures);
    }
    if failures.is_empty() {
        return Ok(());
    }
    let first = &failures[0];
    Err(format!(
        "有 {} 项数据复制失败（首个：{}）；已复制的内容保留在新目录，旧目录未改动。请关闭占用文件的程序并重启应用后重试",
        failures.len(),
        first
    ))
}

/// 单个文件/目录的递归复制（失败只记录不中断，交给 [`copy_data_root`] 汇总）
fn copy_entry(src: &std::path::Path, dst: &std::path::Path, failures: &mut Vec<String>) {
    if src.is_dir() {
        if let Err(e) = std::fs::create_dir_all(dst) {
            failures.push(format!("{}: {e}", dst.display()));
            return;
        }
        match std::fs::read_dir(src) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            copy_entry(&entry.path(), &dst.join(entry.file_name()), failures)
                        }
                        Err(e) => failures.push(format!("{}: {e}", src.display())),
                    }
                }
            }
            Err(e) => failures.push(format!("{}: {e}", src.display())),
        }
    } else if let Err(e) = std::fs::copy(src, dst) {
        failures.push(format!("{}: {e}", src.display()));
    }
}

/// 把迁移后数据库里指向旧数据根的绝对路径改写为新根。
///
/// 两处需要改：`resources.icon`（速达图标缓存绝对路径）与
/// `clipboard_history.image_path`（剪贴板图片快照绝对路径）。asset 协议作用域
/// 启动时只放行**当前**数据根下的 icons/wallpapers/clipboard-images，旧根路径的
/// `<img>`/缩略图一律 403——用户看到的就是「迁移后图标全没了，得重新指定」。
fn rewrite_migrated_db_paths(
    db: &std::path::Path,
    old_root: &std::path::Path,
    new_root: &std::path::Path,
) -> Result<(), String> {
    let mut conn =
        Connection::open(db).map_err(|e| format!("打开迁移后的数据库失败: {e}"))?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启迁移路径改写事务失败: {e}"))?;
    let icons = rewrite_table_root_paths(&tx, "resources", "icon", old_root, new_root)?;
    let clipboard = rewrite_table_root_paths(
        &tx,
        "clipboard_history",
        "image_path",
        old_root,
        new_root,
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    if icons + clipboard > 0 {
        log::info!("已改写库内旧数据目录路径：图标 {icons} 条 / 剪贴板图片 {clipboard} 条");
    }
    Ok(())
}

/// 表内某一列（存绝对路径）批量换根；返回改写的行数。
fn rewrite_table_root_paths(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    old_root: &std::path::Path,
    new_root: &std::path::Path,
) -> Result<usize, String> {
    let select = format!("SELECT id, {column} FROM {table} WHERE {column} IS NOT NULL AND {column} <> ''");
    let rows: Vec<(i64, String)> = {
        let mut stmt = tx.prepare(&select).map_err(|e| e.to_string())?;
        let collected = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        collected
    };
    let update = format!("UPDATE {table} SET {column} = ?1 WHERE id = ?2");
    let mut changed = 0;
    for (id, path) in rows {
        let Some(relinked) = crate::paths::relink_root_path(&path, old_root, new_root) else {
            continue;
        };
        tx.execute(&update, params![relinked, id])
            .map_err(|e| e.to_string())?;
        changed += 1;
    }
    Ok(changed)
}

/// 改写迁移后 app.json 里指向旧数据根的绝对路径（当前字段集：壁纸、
/// 「我的扩展」源码目录、Skills 自定义根目录）。解析/写盘失败只记日志不阻断迁移：
/// 这些路径最坏是暂时失效，可再选一次，而阻断迁移会留下半套数据。
fn rewrite_migrated_config_paths(
    path: &std::path::Path,
    old_root: &std::path::Path,
    new_root: &std::path::Path,
) {
    if !path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        log::warn!("迁移后读取 app.json 失败，跳过配置路径改写");
        return;
    };
    let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&content) else {
        log::warn!("迁移后解析 app.json 失败，跳过配置路径改写");
        return;
    };
    let mut changed = false;
    if let Some(p) = crate::paths::relink_root_path(&cfg.wallpaper_path, old_root, new_root) {
        cfg.wallpaper_path = p;
        changed = true;
    }
    for dir in cfg.dev_extensions.iter_mut().chain(cfg.skill_roots.iter_mut()) {
        if let Some(p) = crate::paths::relink_root_path(dir, old_root, new_root) {
            *dir = p;
            changed = true;
        }
    }
    if changed {
        if let Err(e) = config::save_to(&cfg, path) {
            log::warn!("迁移后配置路径改写落盘失败: {e}");
        }
    }
}

/// 重启应用（更改数据目录 / 恢复数据 / 更新就绪后前端调用）。
/// 统一走 `updater::relaunch_app`：释放 single-instance 互斥后 spawn 新进程并退出。
/// 不再用 `app.restart()`：tauri 的重启在非主线程（IPC 命令线程）触发时依赖
/// `RunEvent::Exit` 事件循环分支，而 x-hub 在 `.run()` 回调里对 Exit 直接
/// `std::process::exit(0)`（为保证托盘退出生效），会先杀死进程导致 restart
/// 永不执行——表现为「点了立即重启却只退出不重启」。显式 spawn 规避该路径。
/// strip_hidden_arg=true：这是用户主动点的重启，剥掉 `--autostart-hidden`
/// （开机自启静默实例装完更新重启后应直接弹出主窗口，而不是隐回托盘）。
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    crate::updater::relaunch_app(&app, true);
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

/// 解析 .lnk 目标并提取图标：目标解析走 IShellLink COM（app_icon.rs，不再为取
/// 目标单独起一个 PowerShell），图标提取复用 extract_app_icon（含低清缓存升级）。
/// 图标仍按「目标路径」命名缓存，与 .exe 导入共用缓存键。
fn resolve_lnk_target_and_icon(lnk_path: &str) -> Result<(String, Option<String>), String> {
    let target = crate::app_icon::resolve_lnk_target(lnk_path)?;
    let icon = extract_app_icon(&target);
    Ok((target, icon))
}

/// 图标缓存判旧阈值（像素）：旧 PowerShell ExtractAssociatedIcon 只能产出 32×32，
/// 宽度低于此值的缓存视为低清、重新提取（新链路产出 ≥48 或按帧尺寸的紧凑 PNG）。
/// 注意 32 档新产物（图标最大帧只有 32 的程序）与本阈值天然「永远判旧」——重提是
/// 幂等的（结果恒为同一张 32×32），每次扫描只多两次 COM 调用，接受；不能为此把
/// 阈值降到 32 以下，否则存量旧 32×32 低清缓存永远不升级。
const ICON_CACHE_MIN_WIDTH: u32 = 48;

/// 图标缓存文件名：DefaultHasher(target) 的 16 位十六进制（沿用旧缓存键，
/// 老数据直接命中缓存，不健康的经宽度+内容判别升级）。
fn icon_cache_path(target: &str) -> std::path::PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    target.hash(&mut hasher);
    crate::paths::data_root()
        .join("icons")
        .join(format!("{:016x}.png", hasher.finish()))
}

/// 原子写缓存：先写 .tmp 再替换，避免前端 <img> 恰好读到半截文件。
/// Windows 的 rename 不能覆盖既有文件，先删再挪（窗口极小，丢了也只是重提一次）。
fn write_png_atomically(path: &std::path::Path, bytes: &[u8]) -> bool {
    let tmp = path.with_extension("png.tmp");
    if std::fs::write(&tmp, bytes).is_err() {
        return false;
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path).is_ok()
}

/// 缓存文件「健康」才可直接复用：宽度达标（≥48；旧 PowerShell 链路只出 32×32）
/// **且**解码后非「小帧居中垫图」鬼影产物——v0.7.6 一度的回归产物宽度就是 256、
/// 字形墨迹却只占中间一小块（Cheat Engine 48 帧垫进 256 画布），按宽度判旧永远
/// 抓不到，必须解码看墨迹（app_icon::icon_png_padded）。非 PNG（历史脏文件）与
/// 解码失败同样判旧重提（重提自愈）。
fn icon_cache_usable(path: &std::path::Path) -> bool {
    crate::app_icon::png_width(path)
        .map(|w| w >= ICON_CACHE_MIN_WIDTH && !crate::app_icon::icon_png_padded(path))
        .unwrap_or(false)
}

/// 提取程序图标：缓存键 = target 哈希，位图默认也从 target 提取。
/// 打包应用（target = shell:AppsFolder\<AUMID>）走 with_source 现查包内主程序。
fn extract_app_icon(source: &str) -> Option<String> {
    extract_app_icon_with_source(source, None)
}

/// 提取程序图标（Shell IShellItemImageFactory，进程内无子进程；有 256 帧得 256、
/// 最大帧不足得按帧尺寸的紧凑画布，见 app_icon.rs「小帧垫图补偿」），
/// 保存 PNG 到数据根 icons/。缓存键 = target 路径哈希；不健康缓存（旧 32×32 低清
/// / 垫图鬼影）自动重提，重提失败保留旧图（宁可糊着不能没图标）。失败且无缓存
/// 返回 None（前端回退到名称首字母）。
///
/// icon_source：位图的实际来源，缓存键不受影响——打包应用的 shell: 目标不是磁盘
/// 文件，位图从包安装目录的主程序 exe 提取（批量扫描随单直传；手动录入/图标清扫
/// 等零星场合传 None，这里现查一次 PS）。现查失败按提取失败处理，走旧缓存兜底。
fn extract_app_icon_with_source(target: &str, icon_source: Option<&str>) -> Option<String> {
    let output_path = icon_cache_path(target);
    if icon_cache_usable(&output_path) {
        return Some(output_path.to_string_lossy().into_owned());
    }
    if let Some(dir) = output_path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    let resolved;
    let bmp_source: &str = if let Some(s) = icon_source {
        s
    } else if process::is_shell_app_target(target) {
        resolved = match resolve_packaged_icon_source(target) {
            Some(s) => s,
            None => return old_cache_fallback(&output_path, target),
        };
        &resolved
    } else {
        target
    };
    match crate::app_icon::extract_icon_png(bmp_source) {
        Some(png) => {
            // 写失败时若旧缓存文件还在（原子写 remove 后 rename 失败会连旧文件一起
            // 丢，此时 exists 为假）就沿用旧图；都不在才回 None，绝不返回指向
            // 不存在文件的假路径——前端 <img> 会显示破图占位。
            if write_png_atomically(&output_path, &png) || output_path.exists() {
                Some(output_path.to_string_lossy().into_owned())
            } else {
                log::warn!("图标写入失败且无旧缓存: {}", target);
                None
            }
        }
        None => old_cache_fallback(&output_path, target),
    }
}

/// 提取失败时的旧缓存兜底：有旧图沿用（宁可糊着不能没图标），都没有才返回 None。
fn old_cache_fallback(output_path: &std::path::Path, target: &str) -> Option<String> {
    if output_path.exists() {
        log::warn!("图标高清重提失败，沿用旧缓存: {}", target);
        Some(output_path.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// 解析打包应用（shell:AppsFolder\<AUMID>）的图标源 exe：按包系列名找安装目录，
/// 从 AppxManifest.xml 取目标应用（Id 对不上取第一个）的 Executable 拼绝对路径。
/// 单次 PowerShell 现查，环境变量传参（约定 15）；拿不到返回 None。
#[cfg(target_os = "windows")]
fn resolve_packaged_icon_source(target: &str) -> Option<String> {
    let (family, app_id) = process::split_shell_aumid(target)?;
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$pkg = Get-AppxPackage -PackageTypeFilter Main | Where-Object { $_.PackageFamilyName -eq $env:XHUB_PKG_FAMILY } | Select-Object -First 1
if ($pkg -and $pkg.InstallLocation) {
  try {
    [xml]$mx = Get-Content -LiteralPath (Join-Path $pkg.InstallLocation 'AppxManifest.xml')
    $app = $mx.Package.Applications.Application | Where-Object { $_.Id -eq $env:XHUB_APP_ID } | Select-Object -First 1
    if (-not $app) { $app = @($mx.Package.Applications.Application)[0] }
    if ($app -and $app.Executable) {
      Write-Output ('ICONSRC=' + (Join-Path $pkg.InstallLocation $app.Executable))
    }
  } catch {}
}
"#;
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .env("XHUB_PKG_FAMILY", family)
        .env("XHUB_APP_ID", app_id)
        .output()
        .ok()?;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(src) = line.strip_prefix("ICONSRC=") {
            let src = src.trim();
            if !src.is_empty() {
                return Some(src.to_string());
            }
        }
    }
    None
}

/// 导入用户选择的图标文件到 icons 目录：
/// - .ico 经 image crate 解码（自动选目录里最大的一帧，旧 PowerShell 写法只会拿 32×32）转存 PNG
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

    // 已导入且健康（宽度达标且非垫图鬼影）则直接复用（旧 32×32 缓存重导入时升级；
    // 垫图判定对「用户自选的稀疏图片」的解码误报也只是幂等重导一次，无副作用）
    if icon_cache_usable(&output_path) {
        return Ok(Some(output_path.to_string_lossy().into_owned()));
    }

    if ext == "ico" {
        let bytes =
            std::fs::read(&source).map_err(|e| format!("读取图标文件失败: {}", e))?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico)
            .map_err(|e| format!("图标转换失败（.ico 解码失败）: {}", e))?;
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| format!("图标转换失败: {}", e))?;
        if !write_png_atomically(&output_path, &png) {
            return Err("图标转换失败（写入缓存失败）".into());
        }
        log::info!("图标导入成功: {} -> {}", source, output_path.display());
        Ok(Some(output_path.to_string_lossy().into_owned()))
    } else {
        // 与 .ico 分支同款原子写（先 .tmp 再替换）：前端 <img> 不会读到半截文件
        match std::fs::read(&source) {
            Ok(bytes) => {
                if !write_png_atomically(&output_path, &bytes) {
                    log::error!("图标写入失败: {} -> {}", source, output_path.display());
                    return Err("图标导入失败（写入缓存失败）".into());
                }
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

/// 从 xhub-note 协议 URL 里取图片文件名（16 位哈希 + 白名单扩展，协议侧同款口径）。
/// 尾随任何多余字符（query / 路径穿越 / 空串）一律判非法——这是借 URL 把数据根外
/// 任意文件写进剪贴板的唯一入口，必须从严。
fn note_image_name_from_url(url: &str) -> Option<String> {
    const NEEDLE: &str = "xhub-note.localhost/";
    let tail = url.split_once(NEEDLE)?.1;
    let name: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    (name.len() == tail.len() && is_note_image_name(&name)).then_some(name)
}

/// 复制笔记内嵌图片到系统剪贴板（灯箱「复制图片」按钮 / 整图选中 Ctrl+C）。
/// 入参为笔记里的 xhub-note 协议 URL，文件名按协议同款口径校验后才拼数据根路径。
/// 写入后清掉回声指纹：从笔记复制图片与从浏览器复制一样应正常进剪贴板历史
/// （历史项自身回写才需要抑制回声，这里不是）。
#[tauri::command]
pub fn copy_note_image(url: String) -> Result<(), String> {
    let name =
        note_image_name_from_url(&url).ok_or_else(|| "不是有效的笔记图片地址".to_string())?;
    let path = crate::paths::data_root()
        .join("notes")
        .join("images")
        .join(&name);
    crate::clipboard::set_clipboard_image(&path.to_string_lossy())?;
    crate::clipboard::clear_self_set_fingerprint();
    Ok(())
}

// ---------- 扫描已安装应用 ----------

#[derive(serde::Serialize)]
pub struct InstalledAppInfo {
    pub name: String,
    pub target: String,
    pub icon: Option<String>,
}

/// 扫描出的单条候选。`icon_source` 仅打包应用（target = shell:AppsFolder\<AUMID>）有值：
/// shell: 目标不是磁盘文件，提取位图须从包安装目录的主程序 exe 取；缓存键仍按
/// target 哈希，与普通应用、图标清扫共用一套口径。
#[derive(Debug, Clone)]
struct ScanCandidate {
    name: String,
    target: String,
    icon_source: Option<String>,
}

/// 扫描本机已安装应用（用户/公共开始菜单快捷方式 + MSIX 打包应用），
/// 去重、过滤系统噪音后批量提取程序图标（icons/<hash>.png，与拖拽导入共用缓存键）。
/// 必须 async：扫描 + 图标提取耗时数秒，同步命令会卡死主线程冻结 UI。
#[tauri::command]
pub async fn scan_installed_apps() -> Result<Vec<InstalledAppInfo>, String> {
    let candidates = scan_app_candidates()?;
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let pairs: Vec<(String, Option<String>)> = candidates
        .iter()
        .map(|c| (c.target.clone(), c.icon_source.clone()))
        .collect();
    let icons = {
        tauri::async_runtime::spawn_blocking(move || batch_extract_icons(&pairs))
            .await
            .map_err(|e| format!("应用图标提取失败: {}", e))??
    };
    Ok(candidates
        .into_iter()
        .zip(icons)
        .map(|(c, icon)| InstalledAppInfo {
            name: c.name,
            target: c.target,
            icon,
        })
        .collect())
}

/// 单次 PowerShell 扫描开始菜单快捷方式（用户 + 公共）与 MSIX 打包应用，
/// 输出 APP=<json> 行（name/target/iconSource），Rust 侧解析并二次去重、按名称排序、限量。
/// 命名/路径等取值一律在 PS 内 Trim，中文经 UTF8 输出。
///
/// 已不再扫注册表卸载项（2026-09-30 按需求去掉）：卸载项常把 DisplayIcon 指向
/// 卸载器/维护程序，图标与名称噪音大。只保留开始菜单 `.lnk`（用户真正点得到的东西）。
///
/// 打包应用段（issue #20/#32，2026-10-18 起）：`Get-StartApps` 里 AppID 含「!」
/// 即 MSIX/Store 打包应用，target 存 `shell:AppsFolder\<AUMID>`（包路径带版本号、
/// WindowsApps 受保护，AUMID 是唯一稳定引用）；iconSource 顺带按包系列名查安装
/// 目录、从 AppxManifest.xml 解析主程序 exe，供图标提取。开始菜单列表会自动排除
/// 不上开始菜单的包内辅助程序；AppID 不允许含路径分隔符，防桌面 .lnk 路径撞「!」
/// 误入。`Get-StartApps`/`Get-AppxPackage` 失败时该段静默为空，退化成原有行为。
fn scan_app_candidates() -> Result<Vec<ScanCandidate>, String> {
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

# ---- MSIX/Store 打包应用（issue #20）：target 存 shell:AppsFolder\<AUMID>，
# iconSource = 包安装目录下清单主程序 exe（提取位图用，缓存键仍是 target）----
$pkgByFam = @{}
Get-AppxPackage -PackageTypeFilter Main | ForEach-Object { $pkgByFam[$_.PackageFamilyName] = $_ }
Get-StartApps | ForEach-Object {
  $id = '' + $_.AppID
  $nm = ('' + $_.Name).Trim()
  if (-not $id -or -not $nm) { return }
  # 只收「包系列名!应用 ID」形态（不含路径分隔符）；桌面 .lnk 的 AppID 是路径，天然排除
  if ($id -notmatch '^[^\\/]+![^\\/]+$') { return }
  $target = 'shell:AppsFolder\' + $id
  $key = $target.ToLower()
  if ($seen.ContainsKey($key)) { return }
  $seen[$key] = $true
  $iconSource = ''
  $parts = $id.Split('!')
  $pkg = $pkgByFam[$parts[0]]
  if ($pkg -and $pkg.InstallLocation) {
    try {
      [xml]$mx = Get-Content -LiteralPath (Join-Path $pkg.InstallLocation 'AppxManifest.xml')
      $app = $mx.Package.Applications.Application | Where-Object { $_.Id -eq $parts[1] } | Select-Object -First 1
      if (-not $app) { $app = @($mx.Package.Applications.Application)[0] }
      if ($app -and $app.Executable) { $iconSource = Join-Path $pkg.InstallLocation $app.Executable }
    } catch {}
  }
  [void]$out.Add(@{ name = $nm; target = $target; iconSource = $iconSource })
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
    let mut apps: Vec<ScanCandidate> = Vec::new();
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
        let icon_source = v
            .get("iconSource")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        apps.push(ScanCandidate {
            name: name.to_string(),
            target: target.to_string(),
            icon_source,
        });
    }
    apps.sort_by(|a, b| {
        a.name.to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.target.cmp(&b.target))
    });
    const MAX_APPS: usize = 500;
    if apps.len() > MAX_APPS {
        apps.truncate(MAX_APPS);
    }
    log::info!("扫描已安装应用: 共 {} 个", apps.len());
    Ok(apps)
}

/// 批量提取程序图标：逐个复用 extract_app_icon_with_source（Shell COM 进程内提取，
/// 免去旧方案的 PowerShell 子进程 + 临时目录周转）。入参 = (target, icon_source)，
/// icon_source 仅打包应用有值（见 ScanCandidate）。已缓存且高清的目标直接复用、
/// 低清旧缓存就地升级，重复扫描零开销。调用方须在阻塞线程上执行
/// （scan_installed_apps / scan_desktop 均经 spawn_blocking 调入）。
fn batch_extract_icons(apps: &[(String, Option<String>)]) -> Result<Vec<Option<String>>, String> {
    let result: Vec<Option<String>> = apps
        .iter()
        .map(|(target, icon_source)| {
            extract_app_icon_with_source(target, icon_source.as_deref())
        })
        .collect();
    log::info!("应用图标提取完成: 共 {} 个", apps.len());
    Ok(result)
}

/// 图标缓存清扫统计（速达页「修复图标」手动触发后的结果）。
#[derive(serde::Serialize)]
pub struct IconSweepSummary {
    /// 检查的自动提取键条目数（去重后；手动导入/favicon 不在统计内）
    pub scanned: usize,
    /// 判为不健康并重提的缓存数
    pub reextracted: usize,
    /// 重提失败数（失败项保留旧图）
    pub failed: usize,
}

/// 图标缓存清扫（速达页右上角「修复图标」手动触发；2026-10-11 前为启动 15s
/// 自动跑，按需求改为手动）：把不健康的缓存按 target 键就地重提——含旧
/// PowerShell 链路的 32×32 低清缓存（宽度判旧），**以及 v0.7.6 一度产出的
/// 「小帧居中垫图」鬼影缓存**（宽度是 256、字形墨迹只占中间一小块，须解码看
/// 墨迹，icon_cache_usable 统一判定）。只处理「资源图标路径 == target 的缓存键」
/// 的条目——用户手动导入的图标（键 = 图标文件路径哈希）与网页 favicon（fav- 前缀）
/// 不越权重置；重提失败保留旧图。图标路径不变，前端清扫完成后带版本参数刷新。
/// 必须 async + spawn_blocking：全量解码 PNG 与 COM 提取是秒级 IO，同步命令会
/// 卡死主线程冻结 UI（同 scan_installed_apps）。
#[tauri::command]
pub async fn sweep_stale_icons(state: tauri::State<'_, DbState>) -> Result<IconSweepSummary, String> {
    // 读完全量资源立刻放锁：后续解码/COM 提取是秒级 IO，不能攥着 DB 互斥锁
    let resources = {
        let conn = state.0.lock().map_err(|_| "数据库不可用".to_string())?;
        crate::repo::resource::list_all(&conn).map_err(|e| format!("读取资源列表失败: {e}"))?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut scanned = 0usize;
        let mut stale: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for r in &resources {
            let Some(icon) = &r.icon else { continue };
            // 只认自动提取键（icon == hash(target).png），手动导入/favicon 不动
            if icon_cache_path(&r.target) != std::path::PathBuf::from(icon) {
                continue;
            }
            if !seen.insert(r.target.clone()) {
                continue;
            }
            scanned += 1;
            if !icon_cache_usable(std::path::Path::new(icon)) {
                stale.push(r.target.clone());
            }
        }
        if stale.is_empty() {
            log::info!("图标清扫：{scanned} 个缓存全部健康，无需重提");
            return IconSweepSummary { scanned, reextracted: 0, failed: 0 };
        }
        log::info!("图标清扫：{} 个不健康缓存待重提", stale.len());
        let failed = stale.iter().filter(|t| extract_app_icon(t).is_none()).count();
        let reextracted = stale.len() - failed;
        log::info!("图标清扫完成：{reextracted}/{} 重提成功（失败项保留旧图）", stale.len());
        IconSweepSummary { scanned, reextracted, failed }
    })
    .await
    .map_err(|e| format!("图标清扫失败: {e}"))
}

// ---------- 扫描桌面 ----------

#[derive(serde::Serialize)]
pub struct DesktopEntry {
    pub name: String,
    pub target: String,
    pub icon: Option<String>,
    /// 展示用分类：`app` | `web` | `file` | `folder`（导入速达时 folder 归入 file 大类）
    pub kind: String,
    /// 桌面上的快捷方式原始路径（仅 `.lnk`/`.url` 有；供「导入后清理桌面快捷方式」使用）
    pub source: Option<String>,
}

/// 扫描【用户桌面】一层（不递归，只 `%USERPROFILE%\Desktop`，不含公共桌面）：
/// - `.lnk` 解析目标 → 按目标分类为 应用 / 网页 / 文件 / 文件夹（解析不到目标的 UWP 等跳过）
/// - `.url` 解析 URL → 网页
/// - `.exe/.bat/.cmd` → 应用
/// - 文件夹 → 文件夹；其它文件 → 文件
/// 图标沿用 batch_extract_icons 批量缓存（与拖拽导入、已安装应用扫描共用 icons/<hash>.png）。
/// 必须 async：解析快捷方式 + 提取图标耗时数秒，同步命令会冻结 UI。
#[tauri::command]
pub async fn scan_desktop() -> Result<Vec<DesktopEntry>, String> {
    let candidates = scan_desktop_candidates()?;
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    // 网页（URL）没有可提取的图标资源，跳过图标提取（前端回退到名称首字母）
    let icon_pairs: Vec<(String, Option<String>)> = candidates
        .iter()
        .filter(|(_, _, kind, _)| kind != "web")
        .map(|(_, target, _, _)| (target.clone(), None))
        .collect();
    let icons =
        tauri::async_runtime::spawn_blocking(move || batch_extract_icons(&icon_pairs))
            .await
            .map_err(|e| format!("应用图标提取失败: {}", e))??;
    let mut icon_iter = icons.into_iter();
    let entries = candidates
        .into_iter()
        .map(|(name, target, kind, source)| {
            let icon = if kind == "web" {
                None
            } else {
                icon_iter.next().flatten()
            };
            DesktopEntry {
                name,
                target,
                icon,
                kind,
                source,
            }
        })
        .collect();
    Ok(entries)
}

/// 单次 PowerShell 枚举用户桌面一层并分类，输出 DESK=<json> 行（name/target/kind/src），
/// Rust 侧解析、二次去重、排序、限量。名称/路径取值一律在 PS 内 Trim，中文经 UTF-8 输出。
/// `src` = 该条目对应的桌面快捷方式原始路径（仅 `.lnk`/`.url` 非空），供「导入后清理」用。
fn scan_desktop_candidates() -> Result<Vec<(String, String, String, Option<String>)>, String> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$sh = New-Object -ComObject WScript.Shell
$desktop = [Environment]::GetFolderPath('Desktop')
if (-not (Test-Path -LiteralPath $desktop)) { return }
$seen = @{}
$out = New-Object System.Collections.ArrayList

function Add-Entry([string]$name, [string]$target, [string]$kind, [string]$src) {
  if (-not $target) { return }
  $target = $target.Trim()
  $name = ([string]$name).Trim()
  if (-not $name) { $name = [System.IO.Path]::GetFileNameWithoutExtension($target) }
  if (-not $name) { return }
  $key = $target.ToLower()
  if ($seen.ContainsKey($key)) { return }
  $seen[$key] = $true
  if ($null -eq $src) { $src = '' }
  [void]$out.Add(@{ name = $name; target = $target; kind = $kind; src = $src })
}

Get-ChildItem -LiteralPath $desktop | ForEach-Object {
  $item = $_
  $full = $item.FullName
  if ($item.PSIsContainer) {
    Add-Entry $item.Name $full 'folder' ''
    return
  }
  $ext = [System.IO.Path]::GetExtension($item.Name).ToLower()
  switch ($ext) {
    '.lnk' {
      $t = ''
      try { $t = $sh.CreateShortcut($full).TargetPath } catch { $t = '' }
      if (-not $t) { return }   # UWP / 失效快捷方式：没有可启动的路径目标，跳过
      if ($t -match '^https?://') { Add-Entry $item.BaseName $t 'web' $full }
      elseif (Test-Path -LiteralPath $t) {
        if ((Get-Item -LiteralPath $t).PSIsContainer) { Add-Entry $item.BaseName $t 'folder' $full }
        elseif ($t -match '\.(exe|bat|cmd|msi)$') { Add-Entry $item.BaseName $t 'app' $full }
        else { Add-Entry $item.BaseName $t 'file' $full }
      }
      else { return }           # 目标已不存在：不导出一个死链
    }
    '.url' {
      $line = Get-Content -LiteralPath $full -TotalCount 20 |
        Where-Object { $_ -match '^URL=' } | Select-Object -First 1
      if ($line) {
        $u = (($line -replace '^URL=', '')).Trim()
        if ($u) { Add-Entry $item.BaseName $u 'web' $full }
      }
    }
    '.exe' { Add-Entry $item.BaseName $full 'app' '' }
    '.bat' { Add-Entry $item.BaseName $full 'app' '' }
    '.cmd' { Add-Entry $item.BaseName $full 'app' '' }
    default { Add-Entry $item.BaseName $full 'file' '' }
  }
}

foreach ($e in $out) { Write-Output ('DESK=' + ($e | ConvertTo-Json -Compress)) }
"#;
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("扫描桌面失败（PowerShell 执行错误）: {}", e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        log::debug!("扫描桌面 PowerShell stderr: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<(String, String, String, Option<String>)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in stdout.lines() {
        let Some(json) = line.strip_prefix("DESK=") else {
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
        let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("file");
        let kind = match kind {
            "app" | "web" | "file" | "folder" => kind,
            _ => "file",
        };
        let source = v
            .get("src")
            .and_then(|x| x.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let (name, target) = (name.trim(), target.trim());
        if name.is_empty() || target.is_empty() {
            continue;
        }
        if !seen.insert(target.to_lowercase()) {
            continue;
        }
        entries.push((name.to_string(), target.to_string(), kind.to_string(), source));
    }
    // 分类展示顺序：应用 → 网页 → 文件 → 文件夹，同类别内按名称排序
    let kind_rank = |k: &str| match k {
        "app" => 0,
        "web" => 1,
        "file" => 2,
        _ => 3,
    };
    entries.sort_by(|a, b| {
        kind_rank(&a.2)
            .cmp(&kind_rank(&b.2))
            .then_with(|| {
                a.0.to_lowercase()
                    .cmp(&b.0.to_lowercase())
                    .then_with(|| a.1.cmp(&b.1))
            })
    });
    const MAX_DESKTOP: usize = 500;
    if entries.len() > MAX_DESKTOP {
        entries.truncate(MAX_DESKTOP);
    }
    log::info!("扫描桌面: 共 {} 项", entries.len());
    Ok(entries)
}

/// 删除桌面上的快捷方式（仅 `.lnk`/`.url`），供「扫描桌面 → 导入后清理」使用。
/// 安全护栏（缺一不可）：①扩展名必须是 `.lnk`/`.url` ②必须是普通文件 ③必须是**用户桌面**
/// 的直接子项。绝不删除文件夹、`.exe` 及其它文件；不在护栏杆内的路径静默跳过。
#[tauri::command]
pub fn delete_desktop_shortcuts(paths: Vec<String>) -> Result<usize, String> {
    let Some(desktop) = dirs::desktop_dir() else {
        return Err("找不到桌面目录".into());
    };
    let desktop = desktop.canonicalize().unwrap_or(desktop);
    let mut removed = 0usize;
    for raw in paths {
        let path = std::path::Path::new(&raw);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if ext != "lnk" && ext != "url" {
            log::warn!("跳过清理（非快捷方式）: {}", raw);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let parent = path
            .parent()
            .map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf()));
        if parent.as_deref() != Some(desktop.as_path()) {
            log::warn!("跳过清理（不在用户桌面）: {}", raw);
            continue;
        }
        match std::fs::remove_file(path) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("清理桌面快捷方式失败: {} -> {}", raw, e),
        }
    }
    log::info!("清理桌面快捷方式: {} 个", removed);
    Ok(removed)
}

// ---------- 扫描浏览器书签 ----------

#[derive(serde::Serialize)]
pub struct BrowserBookmark {
    pub name: String,
    pub target: String,
    /// 书签所在文件夹（用 `/` 连接层级；顶层书签栏内为空 → 「书签栏」等根名）
    pub folder: String,
    /// 来源浏览器名（Chrome / Edge / Brave / Chromium）
    pub browser: String,
}

/// 单个浏览器配置文件（profile 目录）的原始书签条数（去重前）。
/// 用来解释「x-hub 计数 ≠ 浏览器收藏夹管理器计数」：所有 profile 合并 + 同网址去重。
#[derive(serde::Serialize)]
pub struct BrowserProfileStat {
    pub browser: String,
    /// 配置目录名（Default / Profile 3…），不是 Edge 里显示的「个人资料名称」
    pub profile: String,
    /// 该配置文件里 type=url 的条数（去重前）
    pub count: usize,
}

/// 扫描浏览器书签的完整结果：条目 + 数量口径（前端弹窗展示，用户可自查差异来源）
#[derive(serde::Serialize)]
pub struct BrowserBookmarkScan {
    pub items: Vec<BrowserBookmark>,
    /// 各浏览器各配置文件的原始条数（去重前）
    pub profiles: Vec<BrowserProfileStat>,
    /// 同一浏览器内相同网址（跨配置文件/文件夹）被合并掉的条数；跨浏览器不合并、各自展示
    pub duplicates: usize,
    /// 空名称 / 空网址 / javascript: 跳过的条数
    pub skipped: usize,
    /// 超过单次上限被截断的条数
    pub truncated: usize,
}

/// 读取 Chromium 系浏览器书签（Chrome / Edge / Brave / Chromium）。
/// 纯文件读取（不跑 PowerShell、不读历史），遍历各浏览器 User Data 下所有配置目录的
/// `Bookmarks` **与 `AccountBookmarks`**（后者是登录 Google 账号后的账号存储，存在只写
/// 它而无本地 Bookmarks 的机器），递归 roots 收集 `type=url` 节点；`dedupe` 为 true 时
/// **同一浏览器内**按 URL 去重（多配置文件/文件夹的相同网址只留一条），**跨浏览器不合并、
/// 各自展示**（曾按「跨浏览器只留第一条」去重：第二个浏览器的书签若与第一个大量重复会整个
/// 被吞、前端按浏览器分组后该浏览器直接从列表消失，dckxx 2026-10-09 改口径；导入侧按网址
/// 只建一条资源，不怕跨浏览器重复展示）。Firefox 的 places.sqlite 属二期，不在此列。
#[tauri::command]
pub fn scan_browser_bookmarks(dedupe: bool) -> Result<BrowserBookmarkScan, String> {
    const MAX_BOOKMARKS: usize = 2000;
    let Some(local) = dirs::data_local_dir() else {
        return Ok(BrowserBookmarkScan {
            items: vec![],
            profiles: vec![],
            duplicates: 0,
            skipped: 0,
            truncated: 0,
        });
    };
    // (展示名, User Data 相对路径)；均为 Chromium 系，Bookmarks 结构一致
    let vendors: [(&str, &str); 4] = [
        ("Chrome", r"Google\Chrome\User Data"),
        ("Edge", r"Microsoft\Edge\User Data"),
        ("Brave", r"BraveSoftware\Brave-Browser\User Data"),
        ("Chromium", r"Chromium\User Data"),
    ];

    let mut found: Vec<BrowserBookmark> = Vec::new();
    let mut profiles: Vec<BrowserProfileStat> = Vec::new();
    let mut skipped: usize = 0;
    for (browser, rel) in vendors {
        let user_data = local.join(rel);
        if !user_data.is_dir() {
            continue;
        }
        let Ok(dirs) = std::fs::read_dir(&user_data) else {
            continue;
        };
        for profile in dirs.flatten() {
            let path = profile.path();
            if !path.is_dir() {
                continue;
            }
            // profile 目录名（Default / Profile 3…），用于数量口径展示
            let profile_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let mut counted = 0usize;
            // 书签文件有两个来源：经典本地存储 `Bookmarks` + 登录 Google 账号后的账号存储
            // `AccountBookmarks`（结构与 Bookmarks 完全同构；实测存在只写后者的机器——本地
            // Bookmarks 根本不存在，2026-10-09 用户 Chrome 书签「消失」即漏读它）。两文件
            // 都读：同一网址同时落在两文件时由浏览器内去重合并，原始条数照实计入 counted
            for file_name in ["Bookmarks", "AccountBookmarks"] {
                let bookmarks = path.join(file_name);
                if !bookmarks.is_file() {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&bookmarks) else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                    log::warn!("浏览器书签解析失败: {}", bookmarks.display());
                    continue;
                };
                let Some(roots) = json.get("roots").and_then(|r| r.as_object()) else {
                    continue;
                };
                for (key, node) in roots {
                    // 根节点自身有 name（本地化，如「书签栏」）；没有则按 key 兜底
                    let root_name = node
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| match key.as_str() {
                            "bookmark_bar" => "书签栏".to_string(),
                            "other" => "其他书签".to_string(),
                            "synced" => "移动端".to_string(),
                            _ => key.to_string(),
                        });
                    collect_bookmark_children(
                        node,
                        &root_name,
                        browser,
                        &mut found,
                        &mut counted,
                        &mut skipped,
                    );
                }
            }
            if counted > 0 {
                profiles.push(BrowserProfileStat {
                    browser: browser.to_string(),
                    profile: profile_name,
                    count: counted,
                });
            }
        }
    }

    // 浏览器内按 URL 去重（同一浏览器多配置文件/文件夹的相同网址只留一条）；
    // 跨浏览器不合并（各浏览器独立展示）；关掉则原样保留
    let raw_total = found.len();
    let duplicates = dedupe_within_browser(&mut found, dedupe);
    found.sort_by(|a, b| {
        a.browser
            .cmp(&b.browser)
            .then_with(|| a.folder.cmp(&b.folder))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let truncated = found.len().saturating_sub(MAX_BOOKMARKS);
    if truncated > 0 {
        found.truncate(MAX_BOOKMARKS);
    }
    log::info!(
        "扫描浏览器书签: 原始 {} 条（配置文件 {} 个），去重{}、合并 {}、跳过无效 {}、截断 {}，保留 {} 条",
        raw_total,
        profiles.len(),
        if dedupe { "开" } else { "关" },
        duplicates,
        skipped,
        truncated,
        found.len()
    );
    Ok(BrowserBookmarkScan {
        items: found,
        profiles,
        duplicates,
        skipped,
        truncated,
    })
}

/// 递归收集 Chromium 书签节点：`type=url` 收下，`type=folder` 带前缀继续下钻。
/// `counted` 累计有效条数（按 profile 统计用）、`skipped` 累计无效跳过数（空名/空 URL/javascript:）。
/// 浏览器内按 URL（小写）去重：同一浏览器的重复条目只留第一条，跨浏览器**不**合并。
/// 返回被合并掉的条数。抽成纯函数用单测锁口径——「跨浏览器去重」曾把整个浏览器吞掉。
fn dedupe_within_browser(found: &mut Vec<BrowserBookmark>, dedupe: bool) -> usize {
    if !dedupe {
        return 0;
    }
    let raw_total = found.len();
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    found.retain(|b| seen.insert((b.browser.clone(), b.target.to_lowercase())));
    raw_total - found.len()
}

fn collect_bookmark_children(
    node: &serde_json::Value,
    prefix: &str,
    browser: &str,
    out: &mut Vec<BrowserBookmark>,
    counted: &mut usize,
    skipped: &mut usize,
) {
    let Some(children) = node.get("children").and_then(|c| c.as_array()) else {
        return;
    };
    for child in children {
        let ty = child.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let name = child
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .trim();
        match ty {
            "url" => {
                let url = child
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .trim();
                if name.is_empty() || url.is_empty() || url.starts_with("javascript:") {
                    *skipped += 1;
                    continue;
                }
                *counted += 1;
                out.push(BrowserBookmark {
                    name: name.to_string(),
                    target: url.to_string(),
                    folder: if prefix.is_empty() {
                        "未分类".to_string()
                    } else {
                        prefix.to_string()
                    },
                    browser: browser.to_string(),
                });
            }
            "folder" => {
                if name.is_empty() {
                    continue;
                }
                let sub = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{}/{}", prefix, name)
                };
                collect_bookmark_children(child, &sub, browser, out, counted, skipped);
            }
            _ => {}
        }
    }
}

/// 批量抓取网页图标（favicon）：书签/网页资源导入后自动补齐站点图标（见 favicon.rs）。
/// 返回「原样 target → 图标绝对路径」映射（抓不到为 None）；同域名只抓一次，永不整体报错。
#[tauri::command]
pub async fn fetch_favicons(
    targets: Vec<String>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    Ok(crate::favicon::fetch_favicons(targets).await)
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

/// 删除会话（级联删除消息；同时清掉该会话的「本会话允许」授权，防残留语义直跑）
#[tauri::command]
pub fn delete_chat_session(
    state: State<'_, DbState>,
    agent: State<'_, std::sync::Arc<crate::agent::state::AgentState>>,
    id: i64,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    chat::delete_session(&conn, id).map_err(err_str)?;
    drop(conn);
    // 会话删了，其进行中的 agent 回合也要中断：不取消会继续烧模型调用，
    // 收尾写入还会因外键级联（消息已随会话清掉）而报错（0.9.0 审查优化）
    agent.cancel_session_turns(id);
    agent.clear_session(id);
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
    // 默认模型归一：无默认则第一个为默认；多默认只保留第一个（与平台清单对账共用）
    normalize_single_default(&mut next);
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

/// 默认模型归一（`save_chat_models` 与平台清单对账共用）：
/// 无默认则第一个为默认；多默认只保留第一个
fn normalize_single_default(models: &mut [ChatModelConfig]) {
    let has_default = models.iter().any(|m| m.is_default);
    if !has_default {
        if let Some(first) = models.first_mut() {
            first.is_default = true;
        }
    } else {
        let mut seen = false;
        for m in models.iter_mut() {
            if m.is_default {
                if seen {
                    m.is_default = false;
                }
                seen = true;
            }
        }
    }
}

/// 平台条目的统一构造（与设置页 `platformModel()` 逐字段一致）：
/// `platform:` 前缀是后端识别平台模型的依据（`chat::is_platform_model`），Key 走
/// 钥匙串占位符（对账时补写，见 `sync_platform_models`），base_url 由内置服务端地址现算。
fn platform_model_entry(name: &str, base_url: &str) -> ChatModelConfig {
    ChatModelConfig {
        id: format!("platform:{name}"),
        name: format!("{name}（平台额度）"),
        base_url: base_url.to_string(),
        model: name.to_string(),
        api_key: String::new(),
        is_default: false,
        has_api_key: false,
        provider_name: crate::chat::PLATFORM_ENTRY_NAME.to_string(),
    }
}

/// 平台条目对账（纯函数）：自备供应商条目原样保留在前，平台条目按服务端清单顺序重列。
/// 返回 (新列表, 新增模型名, 移除模型名)——增减清单同时供「清单没变就不落盘」判定。
fn reconcile_platform_models(
    existing: Vec<ChatModelConfig>,
    names: &[String],
    base_url: &str,
) -> (Vec<ChatModelConfig>, Vec<String>, Vec<String>) {
    let (mut next, old_platform): (Vec<_>, Vec<_>) = existing
        .into_iter()
        .partition(|m| !crate::chat::is_platform_model(m));
    let old_names: Vec<String> = old_platform.iter().map(|m| m.model.clone()).collect();
    next.extend(names.iter().map(|n| platform_model_entry(n, base_url)));
    let added: Vec<String> = names.iter().filter(|n| !old_names.contains(n)).cloned().collect();
    let removed: Vec<String> = old_names.iter().filter(|n| !names.contains(n)).cloned().collect();
    (next, added, removed)
}

/// 平台模型清单对账结果（仅记录增减，供日志与 `chat-models-changed` 事件载荷）
#[derive(Debug)]
pub(crate) struct PlatformModelsSync {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

/// 平台模型清单对账（约定 33 自愈）：平台条目只在「打开平台额度开关」那一刻拉
/// `/api/v1/ai/models` 整体写入本地配置，服务端后台加减模型后这份快照不会自己更新，
/// 对话仍带旧模型名就会撞 400 model_not_allowed。这里拉当前清单与本地快照多退少补
/// （自备供应商条目不动），有变化才落盘并广播 `chat-models-changed` 让已打开的
/// 对话面板/设置页刷新。触发点：启动 8s 对账（lib.rs）+ 发送撞 400 自愈（chat.rs）。
///
/// 返回 None = 无需变化：平台额度没开（配置里没有平台条目，绝不反向自动开启）、
/// 拉到空清单（可能是服务端瞬时问题，绝不当成「全部下架」清空本地）、或清单没变。
pub(crate) async fn sync_platform_models() -> Result<Option<PlatformModelsSync>, String> {
    if !config::load().chat_models.iter().any(crate::chat::is_platform_model) {
        return Ok(None);
    }
    let names = crate::chat::fetch_platform_model_names().await?;
    if names.is_empty() {
        log::warn!("平台模型清单为空，跳过同步（保留本地条目）");
        return Ok(None);
    }
    let base_url = crate::chat::platform_base_url();
    let _guard = crate::config::lock();
    let mut cfg = config::load();
    // 拉取期间用户可能刚把平台额度关掉：以磁盘现状为准，别把刚删的条目写回去
    if !cfg.chat_models.iter().any(crate::chat::is_platform_model) {
        return Ok(None);
    }
    let current: Vec<String> = cfg
        .chat_models
        .iter()
        .filter(|m| crate::chat::is_platform_model(m))
        .map(|m| m.model.clone())
        .collect();
    if current == names {
        return Ok(None);
    }
    let (mut next, added, removed) = reconcile_platform_models(cfg.chat_models, &names, &base_url);
    // 新平台条目补钥匙串占位 Key：`chat::resolve_api_key` 靠它认出「换账号 token」；
    // 移除条目的占位残留无害（同名模型回归时直接复用），不做清理
    for name in &added {
        let id = format!("platform:{name}");
        if crate::chat::get_api_key(&id).as_deref() != Some(crate::chat::PLATFORM_KEY_SENTINEL) {
            crate::chat::save_api_key(&id, crate::chat::PLATFORM_KEY_SENTINEL)?;
        }
    }
    normalize_single_default(&mut next);
    cfg.chat_models = next;
    config::save(&cfg)?;
    log::info!("平台模型清单已同步: +{:?} -{:?}", added, removed);
    // 已打开的对话面板/设置页就地刷新模型列表（主窗与独立对话窗各自监听）
    if let Some(handle) = crate::APP_HANDLE.get() {
        use tauri::Emitter;
        let _ = handle.emit(
            "chat-models-changed",
            serde_json::json!({ "added": added, "removed": removed }),
        );
    }
    Ok(Some(PlatformModelsSync { added, removed }))
}

/// 400 自愈重试用：RR 取下一个平台条目（`pick_chat_model` 的平台分支同款游标）。
/// 平台额度没开 / 条目被对账清空时返回 None，调用方放弃重试原样报错。
pub(crate) fn pick_fresh_platform_model() -> Option<ChatModelConfig> {
    let models = config::load().chat_models;
    let platform: Vec<&ChatModelConfig> = models
        .iter()
        .filter(|m| crate::chat::is_platform_model(m))
        .collect();
    if platform.is_empty() {
        return None;
    }
    let i = PLATFORM_RR.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % platform.len();
    Some(platform[i].clone())
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
        // 截断边界可能落在 agent 回合中间：头部孤儿 tool 消息会被上游 400，先剪掉
        //（这里的 chat 是 repo::chat；剪裁助手在 crate::chat）
        crate::chat::trim_leading_orphan_tools(
            chat::list_recent_messages(&conn, session_id, CHAT_CONTEXT_WINDOW).map_err(err_str)?,
        )
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
    // turn_id：本轮用户消息的 turn（#t42 生成）；平台链路以 X-XHub-Turn-Id 透传给服务端对账（§8）
    let result = crate::chat::stream_chat_ex(
        &model,
        &history,
        None,
        user_msg.turn_id.as_deref(),
        &mut reply,
        |delta| {
            chunk_sender
                .send(crate::chat::ChatStreamEvent::Chunk {
                    content: delta,
                })
                .map_err(|e| e.to_string())
        },
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis() as i64;

    match result {
        Ok(u) => {
            if reply.trim().is_empty() {
                send_error = Some("模型未返回任何内容".to_string());
            } else {
                usage = Some(u.usage);
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

// ---------------------------------------------------------------------------
// Agent 模式（#t44，计划 §4）：带工具循环的对话
// ---------------------------------------------------------------------------

/// 模型调用实现体：包 `chat::stream_chat_ex`，增量经 Channel 推 Chunk；
/// 每次回调前检查中断旗标（用户取消 → 掐断上游流）。
struct AgentStreamInvoke {
    model: crate::models::ChatModelConfig,
    turn_id: String,
    ch: tauri::ipc::Channel<crate::chat::ChatStreamEvent>,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::agent::turn::ModelInvoke for AgentStreamInvoke {
    fn invoke<'a>(
        &mut self,
        msgs: Vec<crate::models::ChatMessage>,
        tools: Option<Vec<serde_json::Value>>,
        _cb: crate::agent::turn::ChunkCb<'a>,
    ) -> crate::agent::turn::InvokeFuture<'a> {
        let model = self.model.clone();
        let turn_id = self.turn_id.clone();
        let ch = self.ch.clone();
        let flag = self.flag.clone();
        Box::pin(async move {
            let mut buf = String::new();
            let cancelled = flag.clone();
            let mut fwd = |delta: String| -> Result<(), String> {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("已中断".into());
                }
                ch.send(crate::chat::ChatStreamEvent::Chunk { content: delta })
                    .map_err(|e| e.to_string())
            };
            let res = crate::chat::stream_chat_ex_with_cancel(
                &model,
                &msgs,
                tools.as_deref(),
                Some(&turn_id),
                Some(flag.clone()),
                &mut buf,
                &mut fwd,
            )
            .await?;
            Ok(crate::agent::turn::RoundOutput {
                content: buf,
                usage: res.usage,
                tool_calls: res.tool_calls,
            })
        })
    }
}

/// 审批实现体：挂 oneshot 等前端 `respond_agent_approval` 回答；
/// 前端关窗/超时未答（接收端 Err）→ 按拒绝处理，宁严勿宽。
/// 0.9.0 修复：等待期间同时轮询中断旗标——用户点「停止」能立刻结束等待；
/// 否则 Ask 档在无人应答时会把回合挂死（审批卡丢失/前端异常后循环永不退出）。
struct OneshotApprover {
    state: std::sync::Arc<crate::agent::state::AgentState>,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::agent::turn::Approver for OneshotApprover {
    fn approve(&mut self, ask: crate::agent::turn::ApprovalAsk) -> crate::agent::turn::ApproveFuture {
        let st = self.state.clone();
        let flag = self.flag.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            st.open_approval(ask, tx);
            // 每 200ms 查一次中断旗标；中断 = 按拒绝收尾，循环随后的检查点会结束回合
            async fn cancelled(flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
                loop {
                    if flag.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
            tokio::select! {
                r = rx => match r {
                    Ok(a) => a,
                    Err(_) => crate::agent::approval::ApproveAnswer::Denied,
                },
                _ = cancelled(flag) => crate::agent::approval::ApproveAnswer::Denied,
            }
        })
    }
}

/// Agent 模式发消息：带工具循环的对话（计划 §4）。
/// 与 `send_chat_message` 的区别：下发权限档过滤后的工具清单，模型发起工具调用时
/// 循环执行（Ask 档先经前端内联确认），过程事件（ToolCall/ToolResult/ApprovalRequest）
/// 与最终 TurnDone 一并推给前端渲染（工具卡片/时间线 UI 在 #t46）。
#[tauri::command]
pub async fn send_agent_message(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    agent: State<'_, std::sync::Arc<crate::agent::state::AgentState>>,
    session_id: i64,
    content: String,
    // 对话框快捷切换的权限档（readonly/ask/full）；缺省回落配置（#t47 接 AppConfig，当前默认 ask）
    permission_mode: Option<String>,
    on_event: tauri::ipc::Channel<crate::chat::ChatStreamEvent>,
) -> Result<(), String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("消息不能为空".into());
    }
    let started = std::time::Instant::now();

    // 1) 会话 + 用户消息落库（自动标题 / touch 与普通对话同一套）
    let session = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        chat::get_session(&conn, session_id).map_err(err_str)?
    };
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
    // user 消息落库时一定生成了 turn_id；兜底以防历史数据/未来改动破坏该约定
    let turn_id = user_msg
        .turn_id
        .clone()
        .unwrap_or_else(|| format!("turn_{}", chrono::Utc::now().timestamp_micros()));

    // 2) 模型配置 + 权限档：对话框快捷切换优先；未传时读持久默认档（非法值回落安全默认 ask）
    let cfg = config::load();
    let model = pick_chat_model(&cfg.chat_models, &session.model_name)?;
    let mode = permission_mode
        .as_deref()
        .and_then(crate::agent::approval::PermissionMode::from_param)
        .or_else(|| crate::agent::approval::PermissionMode::from_param(&cfg.agent_permission_mode))
        .unwrap_or(crate::agent::approval::DEFAULT_PERMISSION_MODE);
    let prompt_on_dangerous = cfg.agent_prompt_on_dangerous;

    // 3) 上下文：system prompt（四段：身份/时间/准则/关于我——档案为空时该段省略）+ 按 turn 截断历史
    let about_me = Some(cfg.agent_about_me.as_str()).filter(|s| !s.trim().is_empty());
    let system_prompt = crate::agent::prompt::build_system_prompt(chrono::Local::now(), about_me);
    let history = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        crate::agent::context::build_context(&conn, session_id).map_err(err_str)?
    };

    // 4) 运行时：中断旗标 + 三个实现体（模型调用 / 审批 / 事件转发）
    // 开工即告知前端 turn_id：停止键立刻可用（不必等首个工具事件才知道 id）。
    // 先发再登记：发送失败（前端窗口已销毁）直接早退——若先 register_turn 再失败，
    // cancels 登记无人清理（turn_id 每条消息唯一，慢泄漏只增不减）
    on_event
        .send(crate::chat::ChatStreamEvent::TurnStart { turn_id: turn_id.clone() })
        .map_err(|e| e.to_string())?;
    let cancel_flag = agent.register_turn(session_id, &turn_id);
    let mut invoke = AgentStreamInvoke {
        model: model.clone(),
        turn_id: turn_id.clone(),
        ch: on_event.clone(),
        flag: cancel_flag.clone(),
    };
    let mut approver = OneshotApprover { state: agent.inner().clone(), flag: cancel_flag.clone() };
    let ev = on_event.clone();
    // 错误/中断收尾的 partial：顺手记录已流出的增量（封顶防无界）——正文本体在
    // invoke 内部的 buffer 里，这里不记的话 Error 事件的 partial 永远是空串
    let partial_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let pb = partial_buf.clone();
    let emit = move |e: crate::chat::ChatStreamEvent| {
        if let crate::chat::ChatStreamEvent::Chunk { content } = &e {
            if let Ok(mut b) = pb.lock() {
                if b.len() < 20_000 {
                    b.push_str(content);
                }
            }
        }
        ev.send(e).map_err(|x| x.to_string())
    };
    let f2 = cancel_flag.clone();
    let is_cancelled = move || f2.load(std::sync::atomic::Ordering::Relaxed);

    // 5) 循环（grants 用快照：0.9.0 修复——原先 tokio 锁跨整个回合 await 持有，
    // Ask 档等确认期间其它回合会全部卡死在取锁上；现在各回合在副本上判定，结束并回）
    let mut grants = agent.grants_snapshot();
    let result = crate::agent::turn::run_agent_turn(
        session_id,
        &turn_id,
        mode,
        &mut grants,
        prompt_on_dangerous, // 完全权限档下危险操作仍提示（子开关，设置里可开）
        crate::agent::tools::global(),
        &system_prompt,
        history,
        crate::agent::turn::MAX_ROUNDS,
        &mut invoke,
        &mut approver,
        emit,
        is_cancelled,
        || crate::db::open_tool_connection().map_err(|e| e.to_string()),
    )
    .await;
    agent.grants_merge(&grants);
    agent.finish_turn(&turn_id);

    // 6) 落库（pending 顺序即发生顺序）+ 用量累加 + 收尾事件
    // 工具改过的数据域广播刷新事件（0.9.0 修复：agent 建的待办/笔记此前
    // 不触发任何刷新事件，界面上「显示成功但看不到数据」）。事件名与
    // UI 命令一致（todos-changed 等），各窗口界面统一刷新。
    // Ok/Err 两条出口都要广播：真错误（模型调用断掉等）发生前工具可能已经写过库，
    // 只在成功路径广播会漏掉这批写入——待办已建、界面却再也不刷新。
    let broadcast_domains = |domains: &std::collections::BTreeSet<String>| {
        for domain in domains {
            let event = match domain.as_str() {
                "todos" => Some(("todos-changed", Some("todo-tags-changed"))),
                "notes" => Some(("notes-changed", None)),
                "snippets" => Some(("snippets-changed", None)),
                "stickies" => Some(("stickies-changed", None)),
                "countdowns" => Some(("countdowns-changed", None)),
                "resources" => Some(("resources-changed", None)),
                _ => None,
            };
            if let Some((primary, secondary)) = event {
                let _ = app.emit(primary, ());
                if let Some(sec) = secondary {
                    let _ = app.emit(sec, ());
                }
            }
        }
    };
    match result {
        Ok(art) => {
            let usage = art.usage;
            let tool_count = art.tool_count;
            broadcast_domains(&art.domains);
            let mut last: Option<crate::models::ChatMessage> = None;
            {
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                for p in &art.pending {
                    last = Some(
                        chat::add_message_ex(
                            &conn,
                            session_id,
                            Some(&turn_id),
                            &p.role,
                            &p.content,
                            p.tool_calls.as_deref(),
                            p.tool_call_id.as_deref(),
                            p.tool_name.as_deref(),
                            p.tool_status.as_deref(),
                        )
                        .map_err(err_str)?,
                    );
                }
                let _ = chat::add_session_usage(
                    &conn,
                    session_id,
                    usage.input,
                    usage.output,
                    usage.cache_read,
                    usage.reasoning,
                    started.elapsed().as_millis() as i64,
                );
                let _ = chat::touch_session(&conn, session_id);
            }
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            let updated = chat::get_session(&conn, session_id).map_err(err_str)?;
            drop(conn);
            match last {
                Some(message) => on_event
                    .send(crate::chat::ChatStreamEvent::TurnDone {
                        turn_id,
                        message,
                        session: updated,
                        usage,
                        tool_count,
                    })
                    .map_err(|e| e.to_string())?,
                // 中断发生在任何轮次完成之前：无内容可落库，按错误收尾（前端保留已流出部分）
                None => on_event
                    .send(crate::chat::ChatStreamEvent::Error {
                        message: "已中断".into(),
                        partial: partial_buf.lock().map(|b| b.clone()).unwrap_or_default(),
                    })
                    .map_err(|e| e.to_string())?,
            }
        }
        Err((e, art)) => {
            // 已执行的写入照常广播刷新（见 broadcast_domains 注释）；过程消息按既有口径不落库
            broadcast_domains(&art.domains);
            on_event
                .send(crate::chat::ChatStreamEvent::Error {
                    message: e,
                    partial: partial_buf.lock().map(|b| b.clone()).unwrap_or_default(),
                })
                .map_err(|e2| e2.to_string())?;
        }
    }

    Ok(())
}

/// 前端回答工具审批（Ask 档内联卡片）：once 本次允许 / always 本会话内允许 / 其他一律拒绝。
/// turn_id 用于并发回合同 call_id 撞车时精确路由（兼容后端会跨请求复用 call_0 这类 id）
#[tauri::command]
pub async fn respond_agent_approval(
    agent: State<'_, std::sync::Arc<crate::agent::state::AgentState>>,
    turn_id: String,
    call_id: String,
    answer: String,
) -> Result<(), String> {
    log::info!("[agent] 收到审批答复: {turn_id}/{call_id} answer={answer}");
    let ans = crate::agent::approval::ApproveAnswer::from_param(&answer);
    if agent.answer_approval(turn_id.trim(), &call_id, ans) {
        Ok(())
    } else {
        // 0.9.0 修复：区分「已被处理/已失效」。点停止或回合已结束都会清掉挂起的确认，
        // 这时回答会落空——提示用户该回合的状态而不是含糊报错。
        Err("该确认已失效（可能已停止或已处理），如界面仍卡住请点停止结束该回合".into())
    }
}

/// 查询某会话仍在等待确认的审批（Ask 档）：面板重开/切回会话时还原 awaiting 卡片。
/// 挂起审批不落库、只存内存（§4），这里给只读快照；答复仍走 `respond_agent_approval`。
#[tauri::command]
pub fn list_agent_approvals(
    agent: State<'_, std::sync::Arc<crate::agent::state::AgentState>>,
    session_id: i64,
) -> Result<Vec<crate::agent::turn::ApprovalAsk>, String> {
    Ok(agent.pending_approvals(session_id))
}

/// 用户中断 Agent 回合：置旗标（流式回调与循环检查点都会看到）。
/// 已执行的写操作不回滚（§4）；循环收尾后由 send_agent_message 完成落库与事件。
#[tauri::command]
pub async fn cancel_agent_turn(
    agent: State<'_, std::sync::Arc<crate::agent::state::AgentState>>,
    turn_id: String,
) -> Result<(), String> {
    agent.cancel_turn(&turn_id);
    Ok(())
}

/// 笔记 AI 深度整理：把笔记全文交给对话模型做一次**无会话**的语义重排（分组/标题/清单）。
/// 与 `send_chat_message` 的区别：不建会话、不落库、不出现在聊天记录里；模型解析**优先平台内置
/// 额度**（用户明确要求：有平台条目且已登录就固定走平台入口轮询），未登录/未开启平台时回退
/// 「新会话默认模型」同一套（`default_session_model_name` → `pick_chat_model`）。
/// 流式增量经 Channel 推送（Chunk），invoke 返回值即完整整理结果；失败返回 Err（前端可保留 partial）。
/// 注意：整理的提示词把「逐字保留 URL/密钥/账号等技术信息」作为硬约束——这类内容改一个字符就是事故。
/// 图片语法 `![说明](地址)` 同样列入硬约束：模型曾把图片压成裸地址（URL 一字不差但图片不再显示，
/// 因为 Crepe 只认 `![...](...)` 才渲染成图片），前端 `noteImageSyntax.ts` 另有按原稿的回收兜底——
/// 两层是**互补**的：提示词管「尽量别写坏」，回收管「已经写坏了也救回来」，缺一个都会复发。
/// 速记 AI 深度整理的模型解析（设置「功能 → 速记」的 `config.note_ai_model`）：
///
/// - `''`（自动）：平台条目在且已登录 → 平台负载切换；否则默认模型（历史行为）
/// - 平台入口名（`x-hub 平台`）：平台负载切换。显式选择哪怕未登录也保持平台，
///   由请求侧报错点名「需先登录」，而不是悄悄换模型
/// - `platform:<模型名>`：指定平台模型；清单里已没有（下架/改名）→ 回落自动
/// - 其余：按名字精确命中自备模型；不存在（被删）→ 回落自动
///
/// 回落而不是报错：设置里的失效值不该把整理功能卡死，与对话面板「历史会话存的
/// 模型名已删除时回落默认」同语义。
fn resolve_note_ai_model_inner(
    models: &[ChatModelConfig],
    preferred: &str,
    logged_in: bool,
) -> Result<ChatModelConfig, String> {
    let preferred = preferred.trim();
    if !preferred.is_empty() && preferred != crate::chat::PLATFORM_ENTRY_NAME {
        if let Some(name) = preferred.strip_prefix("platform:") {
            if let Some(m) = models
                .iter()
                .find(|m| crate::chat::is_platform_model(m) && (m.id == preferred || m.model == name))
            {
                return Ok(m.clone());
            }
        } else if let Some(m) = models
            .iter()
            .find(|m| !crate::chat::is_platform_model(m) && m.name == preferred)
        {
            return Ok(m.clone());
        }
        // 指定的模型已不存在：继续走下面的自动解析
    }
    // 自动档：平台额度可用 = 有平台条目且已登录（登录态就是平台请求的真实凭据，未登录时
    // 平台必然报 401，此时静默回退默认模型而不是把功能卡死在「请先登录」上）
    let session_name = if preferred == crate::chat::PLATFORM_ENTRY_NAME {
        crate::chat::PLATFORM_ENTRY_NAME.to_string()
    } else {
        let prefer_platform = models.iter().any(crate::chat::is_platform_model) && logged_in;
        if prefer_platform {
            crate::chat::PLATFORM_ENTRY_NAME.to_string()
        } else {
            default_session_model_name(models)
        }
    };
    pick_chat_model(models, &session_name)
}

/// `resolve_note_ai_model_inner` 的运行时入口：登录态现取（钥匙串会话 token）
fn resolve_note_ai_model(models: &[ChatModelConfig], preferred: &str) -> Result<ChatModelConfig, String> {
    resolve_note_ai_model_inner(models, preferred, crate::account::session_token().is_some())
}

#[tauri::command]
pub async fn ai_transform_note(
    content: String,
    on_event: tauri::ipc::Channel<crate::chat::ChatStreamEvent>,
) -> Result<String, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("笔记内容为空".into());
    }

    let cfg = config::load();
    let models = cfg.chat_models;
    let model = resolve_note_ai_model(&models, &cfg.note_ai_model)?;
    log::info!("笔记 AI 整理使用模型: {}", model.model);

    // 指令与正文合进一条 user 消息：stream_chat 的消息层只保证 user/assistant 两角色，
    // 不依赖各供应商对 system 消息的兼容度
    let instruction = "\
你是笔记整理助手。把用户提供的笔记内容重组为清晰、结构化的 Markdown：\
按主题分组，用标题与列表组织同一条目下的多项信息；\
结构重组是你的职责：鼓励调整行文与语序、合并重复内容，\
给「名称/值」这类信息补上冒号等分隔符（如 地址：https://example.com、端口：8080），\
也可以用加粗、表格等排版形式把同类信息整理得更清楚；\
但所有事实内容必须逐字保留：URL、密钥、账号、电话、邮箱、代码、数字等值本身一字不改，\
不得改写、省略或翻译（在值的前后添加冒号、空格、换行、列表与表格标记等结构性符号不受此限）；\
图片必须原样保留 Markdown 图片语法 ![说明](地址)，不得改写成链接、纯地址或直接省略，也不要改动其中的地址——\
语法一改图片就不显示（笔记图片地址形如 http://xhub-note.localhost/xxx.png，把它写成裸地址同样是错的）；\
说明文字没有就留空写成 ![](地址)；\
普通文本里的 `_`、`~`、`*` 等字符必须按原样输出，不要添加反斜杠转义（\\_、\\~ 这类都是错的）；\
空格按原样输出，禁止用 &#x20; 等 HTML 实体表示；\
原文没有的信息不要编造。只输出整理后的 Markdown 正文，不要任何解释，也不要包代码围栏。";
    let message = crate::models::ChatMessage {
        id: 0,
        session_id: 0,
        turn_id: None,
        role: "user".into(),
        content: format!("{instruction}\n\n---\n\n{content}"),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        tool_status: None,
        created_at: String::new(),
    };

    let mut reply = String::new();
    let chunk_sender = on_event.clone();
    crate::chat::stream_chat(&model, &[message], &mut reply, |delta| {
        chunk_sender
            .send(crate::chat::ChatStreamEvent::Chunk { content: delta })
            .map_err(|e| e.to_string())
    })
    .await?;

    if reply.trim().is_empty() {
        return Err("模型未返回任何内容".into());
    }
    Ok(reply)
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

    /// 迁移时只跳过可重建/一次性/会带来不一致的条目，业务数据一律跟着走
    /// （扩展/笔记图片/壁纸/市场缓存漏掉一个，用户就看到「数据消失」）
    #[test]
    fn data_root_copy_skips_only_transient_entries() {
        for skipped in [
            "app.db",
            "app.db-wal",
            "app.db-shm",
            "app.db.bak-pre-migration",
            "restore.db",
            "restore_icons",
            ".restore_pending",
            "logs",
            "updates",
            "data_path.json",
        ] {
            assert!(data_root_entry_skipped(skipped), "{skipped} 不应被复制");
        }
        for copied in [
            "extensions",
            "notes",
            "wallpapers",
            "market",
            "icons",
            "clipboard",
            "runtime",
            "app.json",
            "chat_keys.json",
            "install.mid",
        ] {
            assert!(!data_root_entry_skipped(copied), "{copied} 必须随迁移复制");
        }
    }

    /// 数据根复制要带走扩展（含各扩展的 storage/config 与跨扩展共享存储）、
    /// 笔记图片、壁纸、安装标识，同时不搬运行中的 db/wal 与日志
    #[test]
    fn copy_data_root_brings_extension_and_note_data() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("old");
        let dst = dir.path().join("new");
        std::fs::create_dir_all(src.join("extensions/com.demo/ext")).unwrap();
        std::fs::write(src.join("extensions/com.demo/manifest.json"), "{}").unwrap();
        std::fs::write(src.join("extensions/com.demo/.storage.json"), "{\"k\":1}").unwrap();
        std::fs::write(src.join("extensions/.shared-storage.json"), "{}").unwrap();
        std::fs::create_dir_all(src.join("notes/images")).unwrap();
        std::fs::write(src.join("notes/images/a.png"), b"png").unwrap();
        std::fs::create_dir_all(src.join("wallpapers")).unwrap();
        std::fs::write(src.join("wallpapers/w.jpg"), b"jpg").unwrap();
        std::fs::write(src.join("install.mid"), "0123456789abcdef0123456789abcdef").unwrap();
        std::fs::write(src.join("app.json"), "{}").unwrap();
        std::fs::write(src.join("app.db"), b"db").unwrap();
        std::fs::write(src.join("app.db-wal"), b"wal").unwrap();
        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("logs/x.log"), b"log").unwrap();

        copy_data_root(&src, &dst).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join("extensions/com.demo/.storage.json")).unwrap(),
            "{\"k\":1}"
        );
        assert!(dst.join("extensions/com.demo/manifest.json").is_file());
        assert!(dst.join("extensions/.shared-storage.json").is_file());
        assert!(dst.join("notes/images/a.png").is_file());
        assert!(dst.join("wallpapers/w.jpg").is_file());
        assert!(dst.join("app.json").is_file());
        assert!(dst.join("install.mid").is_file());
        // 运行中的数据库走在线备份，绝不直接复制；日志不随数据走
        assert!(!dst.join("app.db").exists());
        assert!(!dst.join("app.db-wal").exists());
        assert!(!dst.join("logs").exists());
    }

    /// 迁移后库内绝对路径换根：资源图标与剪贴板图片要改，根外路径原样保留
    #[test]
    fn migration_rewrites_icon_and_clipboard_paths() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE resources (id INTEGER PRIMARY KEY, icon TEXT);
                 CREATE TABLE clipboard_history (id INTEGER PRIMARY KEY, image_path TEXT);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO resources (id, icon) VALUES (1, ?1)",
                params![r"C:\old\icons\aaaa.png"],
            )
            .unwrap();
            // 根外路径（用户自选的外部图标）与空值都不该动
            conn.execute(
                "INSERT INTO resources (id, icon) VALUES (2, ?1)",
                params![r"E:\custom\bbbb.png"],
            )
            .unwrap();
            conn.execute("INSERT INTO resources (id, icon) VALUES (3, NULL)", [])
                .unwrap();
            conn.execute(
                "INSERT INTO clipboard_history (id, image_path) VALUES (1, ?1)",
                params![r"C:\old\clipboard\images\cccc.png"],
            )
            .unwrap();
        }
        rewrite_migrated_db_paths(
            &db,
            std::path::Path::new(r"C:\old"),
            std::path::Path::new(r"D:\new"),
        )
        .unwrap();

        let conn = Connection::open(&db).unwrap();
        let icon = |id: i64| -> Option<String> {
            conn.query_row(
                "SELECT icon FROM resources WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(icon(1).as_deref(), Some(r"D:\new\icons\aaaa.png"));
        assert_eq!(icon(2).as_deref(), Some(r"E:\custom\bbbb.png"));
        assert_eq!(icon(3), None);
        let clip: String = conn
            .query_row(
                "SELECT image_path FROM clipboard_history WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(clip, r"D:\new\clipboard\images\cccc.png");
    }

    /// 迁移后配置里的绝对路径换根：壁纸/开发扩展/Skills 根目录改，外部路径不动
    #[test]
    fn migration_rewrites_config_root_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        let mut cfg = AppConfig::default();
        cfg.wallpaper_path = r"C:\old\wallpapers\a.jpg".into();
        cfg.dev_extensions = vec![r"C:\old\my-ext".into(), r"E:\dev\other".into()];
        cfg.skill_roots = vec![r"C:\old\skills".into()];
        config::save_to(&cfg, &path).unwrap();

        rewrite_migrated_config_paths(&path, std::path::Path::new(r"C:\old"), std::path::Path::new(r"D:\new"));

        let loaded = config::load_from(&path);
        assert_eq!(loaded.wallpaper_path, r"D:\new\wallpapers\a.jpg");
        assert_eq!(loaded.dev_extensions, vec![r"D:\new\my-ext", r"E:\dev\other"]);
        assert_eq!(loaded.skill_roots, vec![r"D:\new\skills"]);
    }

    /// copy_note_image 的 URL 入口校验：合法 xhub-note URL 放行，
    /// 外站地址 / 尾随 query / 哈希长度不符 / 越权扩展 / 路径穿越一律拒绝
    #[test]
    fn note_image_name_from_url_validates_like_protocol() {
        assert_eq!(
            note_image_name_from_url("http://xhub-note.localhost/0123456789abcdef.png").as_deref(),
            Some("0123456789abcdef.png")
        );
        assert!(note_image_name_from_url("http://xhub-note.localhost/0123456789abcdef.webp").is_some());
        assert!(note_image_name_from_url("http://example.com/0123456789abcdef.png").is_none());
        assert!(
            note_image_name_from_url("http://xhub-note.localhost/0123456789abcdef.png?x=1")
                .is_none(),
            "尾随 query 必须整体判非法"
        );
        assert!(note_image_name_from_url("http://xhub-note.localhost/abc.png").is_none());
        assert!(
            note_image_name_from_url("http://xhub-note.localhost/0123456789abcdef.exe").is_none(),
            "白名单外的扩展名不得放行"
        );
        assert!(
            note_image_name_from_url("http://xhub-note.localhost/../../app.json").is_none(),
            "路径穿越不得放行"
        );
    }

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

    /// 图标缓存判旧的完整口径（宽度 + 垫图鬼影内容判定）：
    /// 旧 32×32 产物、v0.7.6 的 256 宽鬼影产物都要判旧重提；满幅 48/256 才可复用。
    /// 鬼影产物宽度就是 256——宽度判旧永远抓不到，这条测试锁的就是那次回归。
    #[test]
    fn icon_cache_usable_checks_width_and_padding() {
        let dir = tempfile::tempdir().unwrap();

        let save = |name: &str, img: image::RgbaImage| {
            let p = dir.path().join(name);
            img.save(&p).unwrap();
            p
        };
        let solid = |size: u32| {
            let mut v = Vec::with_capacity((size * size * 4) as usize);
            for _ in 0..size * size {
                v.extend_from_slice(&[10u8, 20, 30, 255]);
            }
            image::RgbaImage::from_raw(size, size, v).unwrap()
        };

        // 满 48：可复用；旧 PowerShell 的 32×32：判旧
        assert!(icon_cache_usable(&save("ok48.png", solid(48))));
        assert!(!icon_cache_usable(&save("old32.png", solid(32))));

        // v0.7.6 鬼影形态：256 画布中间 45px 方块（宽度达标但墨迹只占一小块）→ 判旧
        let mut ghost = vec![0u8; 256 * 256 * 4];
        for y in 100..145 {
            for x in 100..145 {
                let i = ((y * 256 + x) * 4) as usize;
                ghost[i..i + 4].copy_from_slice(&[10, 20, 30, 255]);
            }
        }
        let ghost_img = image::RgbaImage::from_raw(256, 256, ghost).unwrap();
        assert!(!icon_cache_usable(&save("ghost256.png", ghost_img)));

        // 满幅 256：可复用
        assert!(icon_cache_usable(&save("ok256.png", solid(256))));

        // 非 PNG / 文件不存在：判旧（重提自愈）
        let bin = dir.path().join("dirty.bin");
        std::fs::write(&bin, b"not a png").unwrap();
        assert!(!icon_cache_usable(&bin));
        assert!(!icon_cache_usable(&dir.path().join("absent.png")));
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

    /// 文件夹拖拽载荷的线上契约：前端 api/tauri.ts 的 reorderNoteFolders 发的是
    /// 蛇形键（对齐 NoteFolder 模型），serde 侧字段名必须逐字一致——嵌套载荷没有
    /// Tauri 顶层参数的驼峰自动转换，曾因 `rename_all = "camelCase"` 整批反序列化
    /// 失败（`sort_order` 无默认值读成缺失），文件夹拖拽完全无效果。
    #[test]
    fn note_folder_move_payload_matches_frontend_snake_case() {
        let moves: Vec<NoteFolderMove> = serde_json::from_value(serde_json::json!([
            { "id": 3, "parent_id": 7, "sort_order": 0 },
            { "id": 7, "parent_id": null, "sort_order": 1 },
        ]))
        .expect("前端蛇形载荷必须能反序列化");
        assert_eq!(moves[0].parent_id, Some(7));
        assert_eq!(moves[0].sort_order, 0);
        assert_eq!(moves[1].parent_id, None);
        assert_eq!(moves[1].sort_order, 1);
        // 驼峰键不是合法载荷（防有人把前端改回驼峰而 Rust 静默吞掉）
        assert!(serde_json::from_value::<Vec<NoteFolderMove>>(serde_json::json!([
            { "id": 3, "parentId": 7, "sortOrder": 0 }
        ]))
        .is_err());
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

    // ---- 平台模型清单对账（约定 33 自愈）：多退少补，自备供应商条目不动 ----

    #[test]
    fn reconcile_platform_models_adds_removes_and_keeps_custom() {
        let existing = vec![
            chat_model("deepseek-1", "DeepSeek", true),
            platform_chat_model("Old-A"),
            platform_chat_model("B"),
        ];
        let names = vec!["B".to_string(), "New-C".to_string()];
        let (next, added, removed) = reconcile_platform_models(existing, &names, "https://x/v1");
        assert_eq!(added, vec!["New-C".to_string()]);
        assert_eq!(removed, vec!["Old-A".to_string()]);
        // 自备条目在前原样保留（默认标记不动），平台条目按服务端顺序重列
        assert_eq!(next.len(), 3);
        assert_eq!(next[0].name, "DeepSeek");
        assert!(next[0].is_default);
        assert_eq!(next[1].id, "platform:B");
        assert_eq!(next[2].id, "platform:New-C");
        assert_eq!(next[2].base_url, "https://x/v1");
        assert!(!next[2].is_default);
    }

    #[test]
    fn reconcile_platform_models_full_swap_and_no_op() {
        // 平台条目整体换血：旧的全下、新的全上，自备条目不受影响
        let existing = vec![platform_chat_model("Only-Old"), chat_model("kimi-1", "Kimi", false)];
        let names = vec!["Only-New".to_string()];
        let (next, added, removed) = reconcile_platform_models(existing, &names, "https://x/v1");
        assert_eq!(added, vec!["Only-New".to_string()]);
        assert_eq!(removed, vec!["Only-Old".to_string()]);
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].name, "Kimi");
        assert_eq!(next[1].id, "platform:Only-New");
        // 清单与本地完全一致：增减都为空（sync 据此跳过落盘）
        let (next2, added2, removed2) = reconcile_platform_models(next, &names, "https://x/v1");
        assert!(added2.is_empty() && removed2.is_empty());
        assert_eq!(next2.len(), 2);
    }

    // ---- 速记 AI 整理的模型解析（设置「功能 → 速记」的 note_ai_model）----

    #[test]
    fn note_ai_model_auto_prefers_platform_only_when_logged_in() {
        let models = vec![
            chat_model("ds-1", "DeepSeek", true),
            platform_chat_model("LongCat-2.0"),
        ];
        // 自动 + 已登录 → 平台条目（历史行为：平台额度可用就优先走平台）
        let m = resolve_note_ai_model_inner(&models, "", true).unwrap();
        assert!(crate::chat::is_platform_model(&m));
        // 自动 + 未登录 → 默认模型（不把功能卡死在「请先登录」上）
        let m = resolve_note_ai_model_inner(&models, "", false).unwrap();
        assert_eq!(m.name, "DeepSeek");
    }

    #[test]
    fn note_ai_model_explicit_choices_hit_and_fall_back() {
        let models = vec![
            chat_model("ds-1", "DeepSeek", true),
            chat_model("kimi-1", "Kimi", false),
            platform_chat_model("LongCat-2.0"),
        ];
        // 指定自备模型 → 精确命中（哪怕它不是默认）
        let m = resolve_note_ai_model_inner(&models, "Kimi", false).unwrap();
        assert_eq!(m.id, "kimi-1");
        // 指定平台模型（platform:<名> 形态）→ 命中对应条目
        let m = resolve_note_ai_model_inner(&models, "platform:LongCat-2.0", false).unwrap();
        assert!(crate::chat::is_platform_model(&m));
        assert_eq!(m.model, "LongCat-2.0");
        // 平台入口名 → 负载切换（显式选择，未登录也保持平台，由请求侧报错点名）
        let m = resolve_note_ai_model_inner(&models, crate::chat::PLATFORM_ENTRY_NAME, false).unwrap();
        assert!(crate::chat::is_platform_model(&m));
        // 指定名已不存在（被删 / 平台下架）→ 回落自动（未登录 → 默认模型）
        let m = resolve_note_ai_model_inner(&models, "Gone", false).unwrap();
        assert_eq!(m.name, "DeepSeek");
        let m = resolve_note_ai_model_inner(&models, "platform:Gone", false).unwrap();
        assert_eq!(m.name, "DeepSeek");
        // 空配置 → 明确报错
        assert!(resolve_note_ai_model_inner(&[], "", false).is_err());
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

    // ---- 浏览器书签解析（Chromium Bookmarks JSON）----

    /// 书签树递归：只收 type=url（跳过 javascript: 与空名/空 URL），文件夹拼成 `A/B` 前缀
    #[test]
    fn bookmark_tree_walk_collects_urls_with_folder_prefix() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
              "name": "书签栏",
              "type": "folder",
              "children": [
                { "type": "url", "name": "GitHub", "url": "https://github.com/" },
                { "type": "url", "name": "空书签", "url": "" },
                { "type": "url", "name": "脚本", "url": "javascript:void(0)" },
                {
                  "type": "folder",
                  "name": "前端",
                  "children": [
                    { "type": "url", "name": "MDN", "url": "https://developer.mozilla.org/" }
                  ]
                }
              ]
            }"#,
        )
        .unwrap();

        let mut out = Vec::new();
        let mut counted = 0usize;
        let mut skipped = 0usize;
        collect_bookmark_children(&json, "书签栏", "Chrome", &mut out, &mut counted, &mut skipped);

        assert_eq!(out.len(), 2, "应只保留两条有效 URL");
        assert_eq!(counted, 2, "有效条数与保留数一致");
        assert_eq!(skipped, 2, "空 URL 与 javascript: 各计一条跳过");
        assert_eq!(out[0].name, "GitHub");
        assert_eq!(out[0].folder, "书签栏");
        assert_eq!(out[0].browser, "Chrome");
        assert_eq!(out[1].name, "MDN");
        assert_eq!(out[1].folder, "书签栏/前端");
        assert_eq!(out[1].target, "https://developer.mozilla.org/");
        // 无 children 的节点（如 workspaces_v2）应安全返回空
        let mut empty = Vec::new();
        let mut empty_counted = 0usize;
        let mut empty_skipped = 0usize;
        collect_bookmark_children(
            &serde_json::json!({ "type": "folder", "name": "x" }),
            "x",
            "Edge",
            &mut empty,
            &mut empty_counted,
            &mut empty_skipped,
        );
        assert!(empty.is_empty());
        assert_eq!(empty_counted, 0);
        assert_eq!(empty_skipped, 0);
    }

    #[test]
    fn dedupe_within_browser_keeps_cross_browser_duplicates() {
        let mk = |browser: &str, url: &str, name: &str| BrowserBookmark {
            name: name.to_string(),
            target: url.to_string(),
            folder: "书签栏".to_string(),
            browser: browser.to_string(),
        };
        // Chrome 与 Edge 各有一条同网址书签 + Chrome 自己的配置文件里重复一条
        let mut found = vec![
            mk("Chrome", "https://a.dev/", "A"),
            mk("Chrome", "https://A.dev/", "A'"), // 同浏览器同网址（大小写不敏感）
            mk("Edge", "https://a.dev/", "A"),
            mk("Edge", "https://b.dev/", "B"),
        ];
        let removed = dedupe_within_browser(&mut found, true);
        // 只合并 Chrome 内部那条；跨浏览器的同网址各自保留（否则整个浏览器会被吞掉消失）
        assert_eq!(removed, 1);
        assert_eq!(found.len(), 3);
        assert!(found.iter().filter(|b| b.browser == "Chrome").count() == 1);
        assert!(found.iter().filter(|b| b.browser == "Edge").count() == 2);
        // 关掉去重：原样保留
        let mut raw = vec![
            mk("Chrome", "https://a.dev/", "A"),
            mk("Chrome", "https://a.dev/", "A'"),
        ];
        assert_eq!(dedupe_within_browser(&mut raw, false), 0);
        assert_eq!(raw.len(), 2);
    }
}
