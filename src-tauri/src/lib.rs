mod commands;
mod config;
mod db;
mod models;
mod process;
mod repo;
mod shortcut;
mod tray;

use commands::DbState;
use rusqlite::Connection;
use tauri::{Listener, Manager};

/// 初始化数据库并返回连接
fn init_database(app: &tauri::App) -> Result<Connection, Box<dyn std::error::Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_data_dir)?;
    let db_path = app_data_dir.join("app.db");
    let conn = db::init(&db_path)?;
    Ok(conn)
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
                let _ = config::save(&cfg);
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let conn = init_database(app)?;
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
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_data,
            commands::create_group,
            commands::update_group,
            commands::delete_group,
            commands::reorder_groups,
            commands::create_resource,
            commands::update_resource,
            commands::delete_resource,
            commands::reorder_resources,
            commands::launch_resource,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::search_all,
            commands::get_config,
            commands::save_config,
            commands::save_window_state,
            commands::set_window_always_on_top,
            commands::set_always_on_top_config,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::hide_to_tray,
            commands::toggle_window_visibility,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
