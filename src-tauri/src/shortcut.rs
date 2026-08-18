use std::str::FromStr;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+Space";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+Space";

/// 剪贴板历史浮层默认呼出快捷键（避开 Ctrl+Shift+V「无格式粘贴」等高频组合）
#[cfg(target_os = "macos")]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "CommandOrControl+Alt+V";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "Ctrl+Alt+V";

/// 判断两个快捷键字符串是否代表同一个物理按键组合
/// （如 Windows 上 CommandOrControl 与 Ctrl 是同一个键，仅写法不同）
pub fn same_hotkey(a: &str, b: &str) -> bool {
    match (Shortcut::from_str(a), Shortcut::from_str(b)) {
        (Ok(x), Ok(y)) => x.id() == y.id(),
        _ => false,
    }
}

pub fn register_toggle_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| format!("{}", e))
}

pub fn unregister_toggle_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("{}", e))
}

pub fn is_shortcut_registered(app: &AppHandle, shortcut: &str) -> bool {
    app.global_shortcut()
        .is_registered(shortcut)
}

pub fn setup(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut: &Shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    // 按当前配置分发：剪贴板快捷键 → 剪贴板浮层；其余 → 主窗口显隐
                    let clip = crate::config::load().clipboard_shortcut;
                    if same_hotkey(&clip, &shortcut.to_string()) {
                        let _ = app.emit("clipboard-toggle", ());
                    } else {
                        let _ = app.emit("global-shortcut-toggle", ());
                    }
                }
            })
            .build(),
    )?;

    let config = crate::config::load();
    // 注册默认快捷键，注册失败仅记录日志不阻塞启动
    if let Err(e) = register_toggle_shortcut(&handle, &config.global_shortcut) {
        log::warn!("{}", e);
    }
    if let Err(e) = register_toggle_shortcut(&handle, &config.clipboard_shortcut) {
        log::warn!("{}", e);
    }
    Ok(())
}

pub fn format_shortcut_error(err: &str) -> String {
    if err.contains("HotKey") || err.contains("already") || err.contains("occupied") {
        "快捷键冲突".to_string()
    } else if err.contains("UnsupportedKey") || err.contains("InvalidFormat") || err.contains("EmptyToken") {
        "快捷键格式无效，请重新录入（修饰键在前、单个主键，如 Ctrl+Shift+K）".to_string()
    } else {
        err.to_string()
    }
}

pub fn is_conflict_error(err: &str) -> bool {
    err.contains("HotKey") || err.contains("already") || err.contains("occupied")
}
