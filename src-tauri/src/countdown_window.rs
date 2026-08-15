use crate::commands::DbState;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

/// 倒计时浮窗 label 前缀；完整 label 为 `countdown-{id}`
pub const COUNTDOWN_WINDOW_PREFIX: &str = "countdown-";

/// 浮窗固定尺寸（v0.1.14 进一步缩小并增强透明感：150×170 → 120×132）
pub const COUNTDOWN_WIDTH: f64 = 120.0;
pub const COUNTDOWN_HEIGHT: f64 = 132.0;

pub fn window_label(id: i64) -> String {
    format!("{}{}", COUNTDOWN_WINDOW_PREFIX, id)
}

fn parse_id(label: &str) -> Option<i64> {
    label
        .trim_start_matches(COUNTDOWN_WINDOW_PREFIX)
        .parse::<i64>()
        .ok()
}

/// 计算浮窗初始位置：主窗口中心附近偏移
fn initial_position(app: &AppHandle) -> Option<(f64, f64)> {
    let main = app.get_webview_window("main")?;
    if !main.is_visible().unwrap_or(false) {
        return None;
    }
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    let x = pos.x as f64 + size.width as f64 / 2.0 - COUNTDOWN_WIDTH / 2.0 + 40.0;
    let y = pos.y as f64 + size.height as f64 / 2.0 - COUNTDOWN_HEIGHT / 2.0 + 24.0;
    Some((x, y))
}

/// 创建（或聚焦）倒计时浮窗。已存在同 label 窗口时复用。
/// 传入已保存的位置；未保存则出现在主窗口中心附近。
pub fn create_or_focus(
    app: &AppHandle,
    id: i64,
    x: Option<f64>,
    y: Option<f64>,
) -> tauri::Result<WebviewWindow> {
    let label = window_label(id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(win);
    }

    let (pos_x, pos_y) = match (x, y) {
        (Some(px), Some(py)) => (px, py),
        _ => initial_position(app).unwrap_or((240.0, 200.0)),
    };

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App("index.html".into()),
    )
    .title("倒计时")
    .inner_size(COUNTDOWN_WIDTH, COUNTDOWN_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .position(pos_x, pos_y)
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;

    // 移动时持久化位置
    let app_handle = app.clone();
    let moved_win = win.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(_) = event {
            if let Some(id) = parse_id(&moved_win.label()) {
                if let Ok(pos) = moved_win.outer_position() {
                    if let Some(state) = app_handle.try_state::<DbState>() {
                        if let Ok(conn) = state.0.lock() {
                            let _ = crate::repo::countdown::update_position(
                                &conn,
                                id,
                                pos.x as f64,
                                pos.y as f64,
                            );
                        }
                    }
                }
            }
        }
    });

    Ok(win)
}

/// 关闭浮窗并销毁窗口
pub fn destroy(app: &AppHandle, id: i64) {
    if let Some(win) = app.get_webview_window(&window_label(id)) {
        let _ = win.close();
    }
}

/// 启动时恢复所有已浮起的倒计时
pub fn restore_all(app: &AppHandle, countdowns: &[crate::models::Countdown]) {
    for c in countdowns {
        if let Err(e) = create_or_focus(app, c.id, c.float_x, c.float_y) {
            log::warn!("恢复倒计时浮窗 id={} 失败: {}", c.id, e);
        } else {
            log::info!(
                "已恢复倒计时浮窗: id={} name={} @ ({:?},{:?})",
                c.id,
                c.name,
                c.float_x,
                c.float_y
            );
        }
    }
}
