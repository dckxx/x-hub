use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+Space";

pub fn register_toggle_shortcut(app: &AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .register(TOGGLE_SHORTCUT)
        .map_err(|e| format!("全局快捷键注册失败: {}", e))
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

    // 注册默认快捷键，注册失败仅记录日志不阻塞启动
    if let Err(e) = register_toggle_shortcut(&handle) {
        log::warn!("{}", e);
    }
    Ok(())
}
