use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};

/// 提示词 / 待办浮窗的窗口 label（整列表浮窗，各只有一个实例）
pub const PROMPT_FLOAT_LABEL: &str = "prompt-float";
pub const TODO_FLOAT_LABEL: &str = "todo-float";

/// 计算浮窗初始位置：主窗口中心（向右下轻微偏移，与便签浮窗一致，避免完全盖住来源）。
/// 主窗口不可见或取不到位置时返回 None（交给系统默认位置）。
fn centered_position(app: &AppHandle, width: f64, height: f64) -> Option<(f64, f64)> {
    let main = app.get_webview_window("main")?;
    if !main.is_visible().unwrap_or(false) {
        return None;
    }
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    let x = pos.x as f64 + size.width as f64 / 2.0 - width / 2.0 + 40.0;
    let y = pos.y as f64 + size.height as f64 / 2.0 - height / 2.0 + 24.0;
    Some((x, y))
}

/// 创建（或聚焦）浮窗。已存在同 label 窗口时复用（show + focus）。
/// 固定尺寸、无边框、透明、置顶、不进任务栏，与便签/倒计时浮窗风格一致。
pub fn create_or_focus(
    app: &AppHandle,
    label: &str,
    title: &str,
    width: f64,
    height: f64,
) -> tauri::Result<WebviewWindow> {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(win);
    }

    let mut builder = tauri::WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(width, height)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(true)
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 首次弹出时定位到主窗口中心（与便签浮窗一致）；复用已有窗口则保持用户拖到的位置
    if let Some((px, py)) = centered_position(app, width, height) {
        builder = builder.position(px, py);
    }

    // 透明窗口在 Windows 上不能同时启用系统阴影（黑边），与便签浮窗一致
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;
    log::info!("浮窗已创建: {} {}", label, title);
    Ok(win)
}

/// 关闭并销毁浮窗
pub fn destroy(app: &AppHandle, label: &str) {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.close();
    }
}

/// 是否已存在且可见
pub fn is_visible(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label)
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false)
}
