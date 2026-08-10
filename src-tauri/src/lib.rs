mod commands;
mod config;
mod db;
mod models;
mod process;
mod repo;
mod shortcut;
mod sysmon;
mod tray;
mod usage;

use commands::DbState;
use rusqlite::Connection;
use tauri::{Listener, Manager};

/// 初始化数据库并返回连接
fn init_database(app: &tauri::App) -> Result<Connection, Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data_dir)?;

    // 启动时应用待恢复的数据（恢复命令只暂存，重启后替换）
    apply_pending_restore(&app_data_dir);

    let db_path = app_data_dir.join("app.db");
    let conn = db::init(&db_path)?;
    log::info!("数据库初始化完成: {}", db_path.display());
    Ok(conn)
}

/// 应用待恢复的数据：将 restore.db / restore_icons 替换为正式数据
fn apply_pending_restore(app_data: &std::path::Path) {
    let flag = app_data.join(".restore_pending");
    if !flag.exists() {
        return;
    }
    let db = app_data.join("app.db");
    let restore = app_data.join("restore.db");
    if restore.exists() {
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(app_data.join("app.db-wal"));
        let _ = std::fs::remove_file(app_data.join("app.db-shm"));
        let _ = std::fs::copy(&restore, &db);
        let _ = std::fs::remove_file(&restore);
    }
    let restore_icons = app_data.join("restore_icons");
    if restore_icons.exists() {
        let _ = std::fs::remove_dir_all(app_data.join("icons"));
        let _ = std::fs::rename(&restore_icons, app_data.join("icons"));
    }
    let _ = std::fs::remove_file(&flag);
    log::info!("已应用待恢复的数据");
}

/// 一次性迁移旧目录（com.workbench.desktop）数据到新目录（x-hub）
fn migrate_legacy_data(app: &tauri::App) {
    let new_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    if new_dir.exists() {
        return;
    }
    let Some(legacy_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    if !legacy_dir.exists() {
        return;
    }
    if std::fs::create_dir_all(&new_dir).is_err() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            let dst = new_dir.join(entry.file_name());
            if src.is_dir() {
                let _ = copy_dir(&src, &dst);
            } else {
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
    log::info!("已从旧目录迁移数据: {}", legacy_dir.display());
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// 将数据库中旧目录（com.workbench.desktop）的图标路径替换为当前目录（x-hub）
/// 幂等：重复执行无副作用，每次启动调用
fn fix_icon_paths(conn: &Connection, app: &tauri::App) {
    let Ok(new_dir) = app.path().app_data_dir() else {
        return;
    };
    let Some(old_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    let old = old_dir.to_string_lossy().into_owned();
    let new = new_dir.to_string_lossy().into_owned();
    match conn.execute(
        "UPDATE resources SET icon = replace(icon, ?1, ?2) WHERE icon LIKE ?1 || '%'",
        rusqlite::params![old, new],
    ) {
        Ok(n) if n > 0 => log::info!("已修复 {} 条图标路径为 x-hub 目录", n),
        _ => {}
    }
}

/// 验证窗口位置是否在任意可用的显示器内
fn is_position_on_screen(x: f64, y: f64) -> bool {
    // Tauri 2 没有直接枚举显示器的 API，使用一个合理的边界检查
    // 允许负坐标（多显示器配置），但限制在合理范围内
    x >= -10000.0 && x <= 10000.0 && y >= -10000.0 && y <= 10000.0
}

/// 应用启动时恢复上次保存的窗口位置、尺寸与置顶状态
fn restore_window_state(app: &tauri::App) {
    let config = config::load();
    if let Some(window) = app.get_webview_window("main") {
        let ws = &config.window;
        let _ = window.set_size(tauri::LogicalSize::new(ws.width, ws.height));
        if let (Some(x), Some(y)) = (ws.x, ws.y) {
            if is_position_on_screen(x, y) {
                let _ = window.set_position(tauri::LogicalPosition::new(x, y));
            }
        }
        if ws.always_on_top {
            let _ = window.set_always_on_top(true);
        }
        log::info!(
            "恢复窗口状态: {}x{} @ ({:?},{:?}) 置顶={}",
            ws.width,
            ws.height,
            ws.x,
            ws.y,
            ws.always_on_top
        );
    }
}

/// 保存窗口位置与尺寸到配置
fn persist_window_state(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        if let Ok(pos) = window.outer_position() {
            if let Ok(size) = window.inner_size() {
                let mut cfg = config::load();
                cfg.window.x = Some(pos.x as f64);
                cfg.window.y = Some(pos.y as f64);
                cfg.window.width = size.width as f64;
                cfg.window.height = size.height as f64;
                match config::save(&cfg) {
                    Ok(()) => log::debug!("窗口状态已保存: {}x{} @ ({},{})", size.width, size.height, pos.x, pos.y),
                    Err(e) => log::warn!("窗口状态保存失败: {}", e),
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    // 文件日志写到 x-hub 数据目录（默认 app_log_dir 是 %LOCALAPPDATA%\logs，不符合统一目录约定）
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                        path: dirs::data_dir()
                            .map(|d| d.join("x-hub").join("logs"))
                            .unwrap_or_default(),
                        file_name: Some("x-hub".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            log::info!("========== x-hub 启动 ==========");

            // 旧版本（com.workbench.desktop 标识）数据迁移到 x-hub 目录
            migrate_legacy_data(app);

            let conn = init_database(app)?;
            fix_icon_paths(&conn, app);
            app.manage(DbState(std::sync::Mutex::new(conn)));

            tray::setup(app)?;
            shortcut::setup(app)?;

            restore_window_state(app);

            // 关闭事件：拦截默认关闭，改为隐藏至托盘
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        log::info!("收到关闭请求：保存状态并隐藏至托盘");
                        persist_window_state(&app_handle);
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }

            // 全局快捷键事件：切换主窗口显示/隐藏
            let app_handle = app.handle().clone();
            app.listen("global-shortcut-toggle", move |_| {
                if let Some(window) = app_handle.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        log::debug!("全局快捷键：隐藏窗口");
                        let _ = window.hide();
                    } else {
                        log::debug!("全局快捷键：显示窗口");
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            });

            log::info!("x-hub 启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_data,
            commands::create_resource,
            commands::update_resource,
            commands::delete_resource,
            commands::reorder_resources,
            commands::launch_resource,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::list_todos,
            commands::create_todo,
            commands::toggle_todo,
            commands::update_todo,
            commands::delete_todo,
            commands::list_stickies,
            commands::save_sticky,
            commands::search_all,
            commands::save_config,
            commands::set_window_always_on_top,
            commands::set_always_on_top_config,
            commands::get_global_shortcut,
            commands::set_global_shortcut,
            commands::log_client_error,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::hide_to_tray,
            commands::parse_dropped_path,
            commands::import_icon_file,
            commands::inspect_path,
            commands::list_tags,
            commands::create_tag,
            commands::delete_tag,
            commands::get_note_tags,
            commands::set_note_tags,
            commands::list_note_tags,
            commands::backup_data,
            commands::restore_data,
            commands::sync_ai_usage,
            commands::get_usage_summary,
            commands::get_usage_detail,
            sysmon::get_system_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
