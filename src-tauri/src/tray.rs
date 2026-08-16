use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "隐藏主窗口", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &hide_item, &separator, &quit_item])?;

    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        log::warn!("未找到托盘图标，使用默认图标");
        tauri::Error::AssetNotFound("default_window_icon".into())
    })?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("x-hub")
        .on_menu_event(|app, event| {
            log::info!("托盘菜单点击: {}", event.id.as_ref());
            match event.id.as_ref() {
                "show" => {
                    show_window(app);
                }
                "hide" => {
                    hide_window(app);
                }
                "quit" => {
                    log::info!("托盘退出，应用结束");
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;
    log::info!("系统托盘初始化完成");
    Ok(())
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// 切换主窗口显隐（全局快捷键 / 托盘左键共用）：
/// - 窗口不可见（已隐藏至托盘）→ 显示并聚焦
/// - 窗口最小化 → 取消最小化并聚焦
/// - 窗口可见且已聚焦 → 隐藏
/// - 窗口可见但被其他窗口盖住（未聚焦）→ 提升到前台，不隐藏
pub fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if !window.is_visible().unwrap_or(false) {
            log::debug!("toggle_window: 显示窗口");
            show_window(app);
        } else if window.is_minimized().unwrap_or(false) {
            log::debug!("toggle_window: 取消最小化并聚焦");
            let _ = window.unminimize();
            let _ = window.set_focus();
        } else if window.is_focused().unwrap_or(false) {
            log::debug!("toggle_window: 隐藏窗口");
            hide_window(app);
        } else {
            log::debug!("toggle_window: 窗口被盖住，提升到前台");
            let _ = window.set_focus();
        }
    }
}
