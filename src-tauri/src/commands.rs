use crate::config::AppConfig;
use crate::models::{Note, Resource, ResourceKind, SearchResult, Tag, Todo};
use crate::process;
use crate::repo::{note, resource, tag};
use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{Manager, State};

pub struct DbState(pub Mutex<Connection>);

#[tauri::command]
pub fn get_initial_data(state: State<'_, DbState>) -> Result<InitialData, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::list_all(&conn).map_err(err_str)?;
    let notes = note::list(&conn).map_err(err_str)?;
    let tags = tag::list(&conn).map_err(err_str)?;
    let config = crate::config::load();
    log::info!(
        "初始化数据加载完成: resources={} notes={} tags={} todos={}",
        resources.len(),
        notes.len(),
        tags.len(),
        0
    );
    Ok(InitialData {
        resources,
        notes,
        tags,
        todos: Vec::new(),
        config,
    })
}

#[derive(serde::Serialize)]
pub struct InitialData {
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub tags: Vec<Tag>,
    pub todos: Vec<Todo>,
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

// ---------- 全局搜索 ----------

#[tauri::command]
pub fn search_all(state: State<'_, DbState>, keyword: String) -> Result<SearchResult, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let resources = resource::search(&conn, &keyword).map_err(err_str)?;
    let notes = note::search(&conn, &keyword).map_err(err_str)?;
    log::debug!(
        "全局搜索「{}」: 资源 {} 条, 笔记 {} 条",
        keyword,
        resources.len(),
        notes.len()
    );
    Ok(SearchResult {
        resources,
        notes,
        todos: Vec::new(),
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
    let (name, target) = match ext.as_str() {
        "exe" => {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("本地应用")
                .to_string();
            (name, path.clone())
        }
        "lnk" => {
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("快捷方式")
                .to_string();
            let target = resolve_lnk_target(&path)?;
            (name, target)
        }
        _ => {
            log::warn!("拖入文件不支持: {}", path);
            return Err("仅支持 .exe 文件或 .lnk 快捷方式".into());
        }
    };
    let icon = extract_app_icon(&app, &target);
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

/// 通过 PowerShell COM（WScript.Shell）解析 .lnk 快捷方式的目标路径
/// 注意：-Command 模式下 $args 参数传递不可靠，路径通过环境变量传入
fn resolve_lnk_target(lnk_path: &str) -> Result<String, String> {
    let script = "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:XHUB_LNK).TargetPath; [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Write-Output $s";
    let output = powershell()
        .args(["-NoProfile", "-Command", script])
        .env("XHUB_LNK", lnk_path)
        .output()
        .map_err(|e| {
            log::error!("解析快捷方式失败（PowerShell 执行错误）: {}", e);
            format!("解析快捷方式失败: {}", e)
        })?;
    let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if target.is_empty() {
        log::error!("解析快捷方式失败（目标为空）: {}", lnk_path);
        Err("无法解析快捷方式目标路径".into())
    } else {
        Ok(target)
    }
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
