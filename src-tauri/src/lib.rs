mod about;
mod chat;
mod clipboard;
mod commands;
mod config;
mod countdown_ticker;
mod countdown_window;
mod db;
mod extension;
mod market;
mod models;
mod notify;
mod online;
mod paths;
mod process;
mod proxy;
mod repo;
mod runtime;
mod service;
mod shortcut;
mod sticky_window;
mod sysmon;
mod tray;
mod usage;
mod xhub_api;

/// WebView2 附加浏览器参数（主窗/倒计时浮窗/便签浮窗必须完全一致，
/// 同一 user data folder 下不同参数的环境创建会失败）。
/// 保留 wry 默认的 --disable-features 前缀 + 唯一有官方背书的
/// --disable-background-timer-throttling（禁用后台定时器节流，见 WebView2 浏览器标志文档）。
pub const ADDITIONAL_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-timer-throttling";

use commands::DbState;
use rusqlite::Connection;
use tauri::{Listener, Manager};

/// 初始化数据库并返回连接
fn init_database() -> Result<Connection, Box<dyn std::error::Error>> {
    let app_data_dir = crate::paths::data_root().to_path_buf();
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
fn migrate_legacy_data() {
    let new_dir = crate::paths::data_root().to_path_buf();
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
fn fix_icon_paths(conn: &Connection) {
    let new_dir = crate::paths::data_root();
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
                let _guard = config::lock();
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
        // 单实例：重复双击 exe 时不另起新进程（避免再冷启动一个 WebView2、窗口出现在后台），
        // 而是直接把已运行实例的主窗口唤起并置前
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    // 文件日志写到数据根目录（默认 app_log_dir 是 %LOCALAPPDATA%\logs，不符合统一目录约定）
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                        path: crate::paths::data_root().join("logs"),
                        file_name: Some("x-hub".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            log::info!("========== x-hub 启动 ==========");

            // 把数据根动态加入 asset 协议作用域：图标 / 剪贴板图片经 convertFileSrc 渲染，
            // 当数据目录被改到 %APPDATA% 之外（自定义目录 / U 盘便携）时仍能正常访问
            let _ = app
                .asset_protocol_scope()
                .allow_directory(crate::paths::data_root(), true);

            // 系统通知权限：Prompt 时请求一次（Windows 通常直接放行）
            {
                use tauri::plugin::PermissionState;
                use tauri_plugin_notification::NotificationExt;
                if matches!(
                    app.notification().permission_state(),
                    Ok(PermissionState::Prompt) | Ok(PermissionState::PromptWithRationale)
                ) {
                    let _ = app.notification().request_permission();
                }
            }

            // 旧版本（com.workbench.desktop 标识）数据迁移到 x-hub 目录
            migrate_legacy_data();

            let conn = init_database()?;
            fix_icon_paths(&conn);
            app.manage(DbState(std::sync::Mutex::new(conn)));
            app.manage(clipboard::ClipboardState::default());
            app.manage(service::ServiceState::default());

            // 启动扩展反向代理（/svc/<extId>/* → 127.0.0.1:<service 端口>，统一加 CORS 头）
            let proxy_port = tauri::async_runtime::block_on(proxy::start(app.handle().clone()))
                .unwrap_or_else(|e| {
                    log::warn!("扩展反向代理启动失败: {e}");
                    0
                });
            app.manage(proxy::ProxyState(proxy_port));

            tray::setup(app)?;
            shortcut::setup(app)?;

            restore_window_state(app);

            // 主窗口启动时隐藏（tauri.conf.json visible:false），等前端内容可绘制后再 show，
            // 避免 WebView2 冷启动期间出现空白/白屏等待窗口；这里先铺上主题底色，
            // 若 show 早于首帧绘制，也只会闪主题色而非纯白
            // 主题三态：dark，或 system 且系统偏好深色 → 暗色底色；light / 系统浅色 → 亮色
            let config = config::load();
            let mut dark = config.theme_mode == "dark";
            if config.theme_mode == "system" {
                dark = matches!(
                    app.get_webview_window("main").and_then(|w| w.theme().ok()),
                    Some(tauri::Theme::Dark)
                );
            }
            let bg = if dark {
                tauri::window::Color(18, 19, 27, 255) // --bg-page 暗色 #12131b
            } else {
                tauri::window::Color(236, 239, 246, 255) // --bg-page 亮色 #eceff6
            };
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(bg));
                // 启动即前台：避免窗口偶尔出现在其他窗口后面；稍等再补一次焦点，
                // 绕开 Windows 前台锁定的瞬时限制
                let _ = window.set_focus();
                let handle = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let _ = handle.set_focus();
                });
            }

            // 恢复上次已脱离的浮窗便签（位置/置顶/内容均持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let detached = crate::repo::detached_sticky::list(&conn).unwrap_or_default();
                drop(conn);
                sticky_window::restore_all(app.handle(), &detached);
            }

            // 恢复上次已浮起的倒计时浮窗（浮起状态 + 位置持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let floated = crate::repo::countdown::list_floated(&conn).unwrap_or_default();
                drop(conn);
                countdown_window::restore_all(app.handle(), &floated);
            }

            // 启动倒计时后台驱动线程（每秒扫描到期项，托盘/隐藏时不受 WebView 节流影响）
            countdown_ticker::start(app.handle().clone());

            // 启动剪贴板监听线程（启动零加载历史，仅剪贴板变化时落库）
            clipboard::start_monitor(app.handle().clone());

            // 启动剪贴板延迟窗口操作 worker（粘贴/归还焦点统一串行执行，避免频繁 spawn 短命线程）
            clipboard::init_win_op_worker();

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
                crate::tray::toggle_window(&app_handle);
            });

            // 剪贴板快捷键事件：唤起/收起剪贴板历史浮层
            let app_handle = app.handle().clone();
            app.listen("clipboard-toggle", move |_| {
                crate::clipboard::toggle_overlay(&app_handle);
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
            commands::list_notes,
            commands::list_todos,
            commands::create_todo,
            commands::toggle_todo,
            commands::update_todo,
            commands::delete_todo,
            commands::list_stickies,
            commands::save_sticky,
            commands::get_detached_stickies,
            commands::detach_sticky,
            commands::focus_detached_sticky,
            commands::save_detached_sticky,
            commands::toggle_detached_sticky_pin,
            commands::restore_detached_sticky,
            commands::delete_detached_sticky,
            commands::list_countdowns,
            commands::create_countdown,
            commands::update_countdown,
            commands::delete_countdown,
            commands::pause_countdown,
            commands::resume_countdown,
            commands::float_countdown,
            commands::unfloat_countdown,
            commands::list_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet,
            commands::toggle_snippet_pin,
            commands::record_snippet_copy,
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
            commands::scan_installed_apps,
            commands::get_running_processes,
            commands::list_tags,
            commands::create_tag,
            commands::delete_tag,
            commands::get_note_tags,
            commands::set_note_tags,
            commands::list_note_tags,
            commands::backup_data,
            commands::restore_data,
            commands::get_data_path,
            commands::change_data_dir,
            commands::restart_app,
            commands::sync_ai_usage,
            commands::get_usage_summary,
            commands::get_usage_detail,
            commands::list_chat_sessions,
            commands::create_chat_session,
            commands::delete_chat_session,
            commands::rename_chat_session,
            commands::set_chat_session_model,
            commands::list_chat_messages,
            commands::send_chat_message,
commands::get_chat_models,
commands::save_chat_models,
commands::fetch_chat_provider_models,
commands::get_chat_api_key,
commands::set_chat_panel,
            commands::get_chat_panel,
            commands::get_app_info,
            commands::check_whats_new,
            commands::clipboard_list,
            commands::clipboard_copy,
            commands::clipboard_paste,
            commands::clipboard_toggle_pin,
            commands::clipboard_delete,
            commands::clipboard_clear,
            commands::clipboard_set_paused,
            commands::clipboard_activate,
            commands::clipboard_hide,
            commands::set_clipboard_paste_method,
            commands::set_clipboard_media_enabled,
            commands::clipboard_export_image,
            commands::clipboard_get_info,
            commands::set_clipboard_shortcut,
            commands::set_clipboard_retention,
            sysmon::get_system_info,
            extension::list_extensions,
            extension::read_extension_entry,
            extension::open_extension_window,
            extension::uninstall_extension,
            extension::install_extension,
            extension::get_extension_permissions,
            extension::set_extension_permission,
            market::get_market_registry,
            market::install_from_market,
            xhub_api::xhub_call,
            commands::check_connectivity,
            commands::get_weather,
            commands::get_quote,
            commands::set_weather_city,
            commands::locate_weather_by_ip,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 宿主退出：停止所有 service 后端进程，避免 Node 子进程残留
            if let tauri::RunEvent::Exit = event {
                service::stop_all(app);
            }
        });
}
