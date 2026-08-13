use crate::commands::DbState;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

/// 浮窗便签窗口的 label 前缀；完整 label 为 `sticky-{slot}`
pub const STICKY_WINDOW_PREFIX: &str = "sticky-";

/// 浮窗固定尺寸（设计定稿：约 260×280，不可缩放）
pub const STICKY_WIDTH: f64 = 260.0;
pub const STICKY_HEIGHT: f64 = 280.0;

pub fn window_label(slot: i64) -> String {
    format!("{}{}", STICKY_WINDOW_PREFIX, slot)
}

/// 计算浮窗初始位置：主窗口中心附近错开一点（避免完全盖住来源卡）。
/// 主窗口不可见或取不到位置时返回 None（交给系统默认位置）。
fn initial_position(app: &AppHandle) -> Option<(f64, f64)> {
    let main = app.get_webview_window("main")?;
    if !main.is_visible().unwrap_or(false) {
        return None;
    }
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    // 主窗口中心附近，向右下偏移半张浮窗尺寸 + 少量留白
    let x = pos.x as f64 + size.width as f64 / 2.0 - STICKY_WIDTH / 2.0 + 40.0;
    let y = pos.y as f64 + size.height as f64 / 2.0 - STICKY_HEIGHT / 2.0 + 24.0;
    Some((x, y))
}

/// 创建（或重建）便签浮窗。已存在同 label 窗口时先复用。
/// 传入已保存的位置；未保存则默认出现在主窗口中心附近。
pub fn create_or_focus(
    app: &AppHandle,
    slot: i64,
    x: Option<f64>,
    y: Option<f64>,
    always_on_top: bool,
) -> tauri::Result<WebviewWindow> {
    let label = window_label(slot);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(win);
    }

    let (pos_x, pos_y) = match (x, y) {
        (Some(px), Some(py)) => (px, py),
        _ => initial_position(app).unwrap_or((200.0, 200.0)),
    };

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App("index.html".into()),
    )
    .title("便签")
    .inner_size(STICKY_WIDTH, STICKY_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(always_on_top)
    .skip_taskbar(true)
    .visible(true)
    .position(pos_x, pos_y);

    // 透明窗口在 Windows 上不能同时启用 resizable 与阴影拉伸，
    // 保持固定尺寸，窗口内内容即卡片本体
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;

    // 移动时持久化位置（存逻辑坐标，与恢复时的 position 一致）
    let app_handle = app.clone();
    let moved_win = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(_) = event {
            let slot = moved_win
                .label()
                .trim_start_matches(STICKY_WINDOW_PREFIX)
                .parse::<i64>()
                .unwrap_or(0);
            if let Ok(pos) = moved_win.outer_position() {
                if let Some(state) = app_handle.try_state::<DbState>() {
                    if let Ok(conn) = state.0.lock() {
                        let _ = crate::repo::detached_sticky::update_position(
                            &conn,
                            slot,
                            pos.x as f64,
                            pos.y as f64,
                        );
                    }
                }
            }
        }
    });

    Ok(win)
}

/// 聚焦已存在的浮窗（脱离 icon 再次点击时）
pub fn focus(app: &AppHandle, slot: i64) -> bool {
    let Some(win) = app.get_webview_window(&window_label(slot)) else {
        return false;
    };
    let _ = win.show();
    let _ = win.set_focus();
    true
}

/// 关闭浮窗并销毁窗口
pub fn destroy(app: &AppHandle, slot: i64) {
    if let Some(win) = app.get_webview_window(&window_label(slot)) {
        let _ = win.close();
    }
}

/// 启动时恢复所有已脱离的浮窗
pub fn restore_all(app: &AppHandle, stickies: &[crate::models::DetachedSticky]) {
    for s in stickies {
        if let Err(e) = create_or_focus(app, s.slot, s.x, s.y, s.always_on_top) {
            log::warn!("恢复浮窗便签 slot={} 失败: {}", s.slot, e);
        } else {
            log::info!("已恢复浮窗便签: slot={} @ ({:?},{:?})", s.slot, s.x, s.y);
        }
    }
}
