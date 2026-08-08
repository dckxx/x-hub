use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const DEFAULT_TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+Space";

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
            .with_handler(move |app, _shortcut: &Shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = app.emit("global-shortcut-toggle", ());
                }
            })
            .build(),
    )?;

    let shortcut = crate::config::load().global_shortcut;
    // 注册默认快捷键，注册失败仅记录日志不阻塞启动
    if let Err(e) = register_toggle_shortcut(&handle, &shortcut) {
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
