use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{ContextMenu, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

/// 主窗口显隐的「自维护」状态位：Windows 上 WebView2 子窗口会让 `is_visible()`/`is_focused()`
/// 在部分环境（多屏/远程桌面/DPI 缩放/隐藏后）返回不准确的值，导致全局快捷键切换主窗口失效。
/// 这里以「我们自己的 show/hide 调用」为准，避免依赖系统接口。
static MAIN_WINDOW_VISIBLE: AtomicBool = AtomicBool::new(true);

/// 主窗当前显隐状态（floating_ball 等模块联动用）
pub fn is_main_window_visible() -> bool {
    MAIN_WINDOW_VISIBLE.load(Ordering::SeqCst)
}

/// 托盘 / 悬浮球右键菜单共用构造：文案改这里两处同步生效（ADR 0004「改一处两处生效」）。
/// prefix 用于区分事件来源：托盘 ""（show/hide/quit），悬浮球 "fb-"（fb-show/...），
/// 悬浮球菜单事件走 app.on_menu_event，与托盘菜单事件互不干扰。
fn build_menu(app: &AppHandle, prefix: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let show_item = MenuItem::with_id(app, format!("{prefix}show"), "显示主窗口", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, format!("{prefix}hide"), "隐藏主窗口", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, format!("{prefix}quit"), "退出", true, None::<&str>)?;
    Menu::with_items(app, &[&show_item, &hide_item, &separator, &quit_item])
}

pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let menu = build_menu(app.handle(), "")?;

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

    // 悬浮球右键菜单事件（fb- 前缀，见 build_menu / popup_context_menu）
    app.on_menu_event(|app, event| match event.id.as_ref() {
        "fb-show" => show_window(app),
        "fb-hide" => hide_window(app),
        "fb-quit" => {
            log::info!("悬浮球菜单退出，应用结束");
            app.exit(0);
        }
        _ => {}
    });
    Ok(())
}

/// 悬浮球右键菜单：弹出托盘同款菜单（floating_ball::floating_ball_context_menu 调用）
pub fn popup_context_menu(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, "fb-")?;
    if let Some(win) = app.get_webview_window(crate::floating_ball::LABEL) {
        // WebviewWindow 无公开 Window 访问器，经 AsRef<Webview>::window() 取底层窗口句柄
        menu.popup(win.as_ref().window().clone())?;
    }
    Ok(())
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        MAIN_WINDOW_VISIBLE.store(true, Ordering::SeqCst);
        crate::floating_ball::sync_with_main(app);
    }
}

pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        MAIN_WINDOW_VISIBLE.store(false, Ordering::SeqCst);
        crate::floating_ball::sync_with_main(app);
    }
}

/// 悬浮球双击主窗开关：主窗「开着」（自维护可见且未最小化）→ 隐藏；否则显示并聚焦。
/// 与 toggle_window 的区别：被其他窗口盖住时也算「开着」，双击同样收起。
pub fn toggle_main_window(app: &AppHandle) {
    let minimized = app
        .get_webview_window("main")
        .and_then(|w| w.is_minimized().ok())
        .unwrap_or(false);
    if is_main_window_visible() && !minimized {
        log::info!("[悬浮球] 双击：主窗开着，隐藏");
        hide_window(app);
    } else {
        log::info!("[悬浮球] 双击：主窗未开，显示");
        show_window(app);
    }
}

/// 切换主窗口显隐（全局快捷键 / 托盘左键共用）：
/// - 自维护状态为隐藏（已隐藏至托盘）→ 显示并聚焦
/// - 窗口最小化 → 取消最小化并聚焦
/// - 窗口可见且已聚焦 → 隐藏
/// - 窗口可见但被其他窗口盖住（未聚焦）→ 提升到前台，不隐藏
pub fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if !MAIN_WINDOW_VISIBLE.load(Ordering::SeqCst) {
            log::info!("[快捷键] toggle_window: 显示窗口");
            show_window(app);
        } else if window.is_minimized().unwrap_or(false) {
            log::info!("[快捷键] toggle_window: 取消最小化并聚焦");
            let _ = window.unminimize();
            let _ = window.set_focus();
            MAIN_WINDOW_VISIBLE.store(true, Ordering::SeqCst);
        } else if window.is_focused().unwrap_or(false) {
            log::info!("[快捷键] toggle_window: 隐藏窗口");
            hide_window(app);
        } else {
            log::info!("[快捷键] toggle_window: 窗口被盖住，提升到前台");
            let _ = window.set_focus();
        }
    } else {
        log::warn!("[快捷键] toggle_window: 未找到主窗口");
    }
}
