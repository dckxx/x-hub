mod about;
mod autostart;
mod browsers;
mod chat;
mod clipboard;
mod commands;
mod config;
mod countdown_ticker;
mod countdown_window;
mod db;
mod extension;
mod floating_ball;
mod float_window;
pub mod market;
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
pub mod signing;
mod sticky_window;
mod sysmon;
mod todo_reminder;
mod tray;
pub mod updater;
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
pub(crate) fn is_position_on_screen(x: f64, y: f64) -> bool {
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
            crate::tray::show_window(app);
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
        // 笔记图片协议：笔记 Markdown 内嵌 http://xhub-note.localhost/<hash>.<ext>，
        // 按「数据根/notes/images/<文件名>」读取（URL 不含数据根绝对路径，迁数据目录后仍有效）；
        // 文件名严格校验为 16 位十六进制哈希 + 白名单扩展名，杜绝路径穿越
        .register_uri_scheme_protocol("xhub-note", |_ctx, request| {
            let name = request.uri().path().trim_start_matches('/');
            let valid = {
                let mut parts = name.split('.');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(hash), Some(ext), None) => {
                        hash.len() == 16
                            && hash.chars().all(|c| c.is_ascii_hexdigit())
                            && matches!(
                                ext.to_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
                            )
                    }
                    _ => false,
                }
            };
            if !valid {
                return tauri::http::Response::builder()
                    .status(400)
                    .body(Vec::new())
                    .unwrap();
            }
            let mime = match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "gif" => "image/gif",
                _ => "application/octet-stream",
            };
            match std::fs::read(crate::paths::data_root().join("notes").join("images").join(name))
            {
                Ok(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", mime)
                    .header("Access-Control-Allow-Origin", "*")
                    // 内容哈希命名 ⇒ 同名即同内容，可长缓存
                    .header("Cache-Control", "public, max-age=31536000, immutable")
                    .body(bytes)
                    .unwrap(),
                Err(_) => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
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

            // 升级自替换非常早期执行：必须在数据库/其他句柄持有 exe 相关资源前，
            // 且仅在真正待应用时才做替换（幂等）。失败只记日志不阻断启动。
            updater::apply_pending_update(&app.package_info().version.to_string());

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

            // 自启动静默模式（--autostart-hidden）：主窗不显示、直接驻留托盘。
            // 必须经 tray::hide_window 更新自维护的显隐状态位，否则 MAIN_WINDOW_VISIBLE
            // 保持默认 true，全局快捷键 toggle_window 会误判「窗口可见」，只 set_focus
            // 隐藏窗口上无效调用，导致快捷键无法呼出主窗口。
            if crate::autostart::is_hidden_launch() {
                crate::tray::hide_window(app.handle());
            }

            // 桌面悬浮球（ADR 0004）：须在 autostart-hidden 的 hide_window 之后初始化，
            // 显隐联动（主窗隐藏 → 球显示）才能覆盖静默启动场景
            floating_ball::init(app.handle());

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

            // 工作台倒计时卡片可见性门控：默认不可见，待前端按已提交布局上报后放开，
            // 避免启动早期（前端未就绪/卡片实际不在布局中）倒计时抢跑到点
            app.manage(countdown_ticker::CardVisible(
                std::sync::atomic::AtomicBool::new(false),
            ));

            // 启动倒计时后台驱动线程（每秒扫描到期项，托盘/隐藏时不受 WebView 节流影响）
            countdown_ticker::start(app.handle().clone());

            // 启动待办提醒后台线程（remind_at 到点发系统通知 + 前端 toast）
            todo_reminder::start(app.handle().clone());

            // 启动剪贴板监听线程（启动零加载历史，仅剪贴板变化时落库）
            clipboard::start_monitor(app.handle().clone());

            // 启动剪贴板延迟窗口操作 worker（粘贴/归还焦点统一串行执行，避免频繁 spawn 短命线程）
            clipboard::init_win_op_worker();

            // 预创建剪贴板浮层窗口（隐藏常驻）：运行时现场创建 WebView2 窗口曾与
            // 悬浮球操作交错导致整窗未响应（见 clipboard.rs::init_overlay_window 注释）
            clipboard::init_overlay_window(app.handle());

            // 关闭事件：拦截默认关闭，改为隐藏至托盘
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let win_for_events = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            log::info!("收到关闭请求：保存状态并隐藏至托盘");
                            persist_window_state(&app_handle);
                            api.prevent_close();
                            crate::tray::hide_window(&app_handle);
                        }
                        // 最小化/还原联动悬浮球：最小化也是主窗「视觉不可见」，
                        // 球应出现。Windows 上最小化状态变化伴随 Resized 事件，借此
                        // 检测（MAIN_WINDOW_VISIBLE 状态位只覆盖托盘/快捷键显隐链）
                        tauri::WindowEvent::Resized(_) => {
                            let minimized = win_for_events.is_minimized().unwrap_or(false);
                            crate::floating_ball::set_main_minimized(&app_handle, minimized);
                        }
                        _ => {}
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

            // 静默检查更新：启动 5s 后一次，此后按配置间隔（默认 4h）循环。
            // 受 auto_update_enabled 开关控制；检查失败静默（updater 内部记日志）。
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    loop {
                        {
                            let cfg = crate::config::load();
                            if cfg.auto_update_enabled {
                                let _ = updater::check_for_update(handle.clone(), None).await;
                            }
                        }
                        let hours = crate::config::load().update_interval_hours.max(1);
                        tokio::time::sleep(std::time::Duration::from_secs(hours * 3600)).await;
                    }
                });
            }

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
            commands::list_installed_browsers,
            commands::open_url_with_browser,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::list_notes,
            commands::list_todos,
            commands::create_todo,
            commands::toggle_todo,
            commands::update_todo,
            commands::delete_todo,
            commands::schedule_todo,
            commands::reorder_todo_orders,
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
            commands::set_countdown_card_visible,
            commands::list_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet,
            commands::toggle_snippet_pin,
            commands::record_snippet_copy,
            commands::toggle_prompt_float,
            commands::toggle_todo_float,
            commands::toggle_float_pin,
            commands::search_all,
            commands::save_config,
            commands::set_window_always_on_top,
            commands::set_always_on_top_config,
            commands::get_global_shortcut,
            commands::set_global_shortcut,
            commands::get_run_at_startup,
            commands::set_run_at_startup,
            commands::get_startup_hidden,
            commands::log_client_error,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::hide_to_tray,
            commands::parse_dropped_path,
            commands::import_icon_file,
            commands::import_wallpaper,
            commands::cleanup_wallpapers,
            commands::import_note_image,
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
            commands::set_chat_panel_side,
            commands::get_app_info,
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
            extension::extensions_stamp,
            extension::read_extension_entry,
            extension::open_extension_window,
            extension::uninstall_extension,
            extension::get_extension_permissions,
            extension::set_extension_permission,
            market::get_market_registry,
            market::refresh_market_registry,
            market::install_from_market,
            market::install_local_archive,
            market::update_extension,
            updater::check_for_update,
            updater::download_update,
            updater::get_update_status,
            updater::skip_update_version,
            process::open_external,
            xhub_api::xhub_call,
            commands::check_connectivity,
            commands::get_weather,
            commands::get_quote,
            commands::set_weather_city,
            commands::locate_weather_by_ip,
            floating_ball::floating_ball_get_state,
            floating_ball::floating_ball_save_settings,
            floating_ball::floating_ball_drag_end,
            floating_ball::floating_ball_expand,
            floating_ball::floating_ball_trigger,
            floating_ball::floating_ball_context_menu,
            commands::get_theme_config,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 宿主退出：停止所有 service 后端进程，避免 Node 子进程残留
            if let tauri::RunEvent::Exit = event {
                service::stop_all(app);
                // Windows 上 WebView2 子窗口/托盘销毁偶发把退出流程拖死（托盘点「退出」
                // 后进程不消失），清理完成后直接结束进程，保证退出 100% 生效
                std::process::exit(0);
            }
        });
}
