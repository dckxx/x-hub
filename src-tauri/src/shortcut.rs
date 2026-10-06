use std::str::FromStr;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "CommandOrControl+Shift+Space";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+Space";

/// 剪贴板历史浮层默认呼出快捷键。
/// Windows 用 Ctrl+`（Backquote）：避开 Ctrl+Shift+V「无格式粘贴」等高频组合。
#[cfg(target_os = "macos")]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "CommandOrControl+Alt+V";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CLIPBOARD_SHORTCUT: &str = "Ctrl+`";

/// 全局搜索默认呼出快捷键。
#[cfg(target_os = "macos")]
pub const DEFAULT_SEARCH_SHORTCUT: &str = "CommandOrControl+K";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SEARCH_SHORTCUT: &str = "Ctrl+K";

/// AI 对话默认呼出快捷键。
#[cfg(target_os = "macos")]
pub const DEFAULT_CHAT_SHORTCUT: &str = "CommandOrControl+Shift+K";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CHAT_SHORTCUT: &str = "Ctrl+Shift+K";

/// 速记默认呼出快捷键（docs/speednote-plan.md §5.4）：唤起主窗 → 切速记视图 → 聚焦新建。
#[cfg(target_os = "macos")]
pub const DEFAULT_NOTES_SHORTCUT: &str = "CommandOrControl+Shift+N";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_NOTES_SHORTCUT: &str = "Ctrl+Shift+N";

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
                    // 诊断：确认全局热键事件是否到达本进程（用于定位「录入成功但呼出/隐藏不生效」）
                    log::info!("[快捷键] 事件触发: {} state={:?}", shortcut, event.state);
                    // 按当前配置分发四个全局快捷键（搜索 / AI 对话事件由主窗前端监听处理，
                    // 见 index.vue；剪贴板与主窗显隐由 lib.rs 的 app.listen 处理）
                    let cfg = crate::config::load();
                    let pressed = shortcut.to_string();
                    if same_hotkey(&cfg.clipboard_shortcut, &pressed) {
                        let _ = app.emit("clipboard-toggle", ());
                    } else if same_hotkey(&cfg.search_shortcut, &pressed) {
                        let _ = app.emit("search-shortcut", ());
                    } else if same_hotkey(&cfg.chat_shortcut, &pressed) {
                        let _ = app.emit("chat-shortcut", ());
                    } else if same_hotkey(&cfg.notes_shortcut, &pressed) {
                        let _ = app.emit("notes-shortcut", ());
                    } else {
                        let _ = app.emit("global-shortcut-toggle", ());
                    }
                }
            })
            .build(),
    )?;

    let config = crate::config::load();
    // 注册默认快捷键，注册失败仅记录日志不阻塞启动。
    // 各键可在设置里单独「禁用」（config.*_shortcut_enabled = false）：禁用即不注册但保留键值。
    if !config.global_shortcut_enabled {
        log::info!("[快捷键] 主窗口快捷键已禁用，跳过注册");
    } else if let Err(e) = register_toggle_shortcut(&handle, &config.global_shortcut) {
        log::warn!("[快捷键] 注册主窗口快捷键失败: {}", e);
    } else {
        log::info!("[快捷键] 已注册主窗口快捷键: {}", config.global_shortcut);
    }
    if !config.clipboard_shortcut_enabled {
        log::info!("[快捷键] 剪贴板快捷键已禁用，跳过注册");
    } else if let Err(e) = register_toggle_shortcut(&handle, &config.clipboard_shortcut) {
        log::warn!("[快捷键] 注册剪贴板快捷键失败: {}", e);
    } else {
        log::info!("[快捷键] 已注册剪贴板快捷键: {}", config.clipboard_shortcut);
    }
    if !config.search_shortcut_enabled {
        log::info!("[快捷键] 搜索快捷键已禁用，跳过注册");
    } else if let Err(e) = register_toggle_shortcut(&handle, &config.search_shortcut) {
        log::warn!("[快捷键] 注册搜索快捷键失败: {}", e);
    } else {
        log::info!("[快捷键] 已注册搜索快捷键: {}", config.search_shortcut);
    }
    if !config.chat_shortcut_enabled {
        log::info!("[快捷键] AI 对话快捷键已禁用，跳过注册");
    } else if let Err(e) = register_toggle_shortcut(&handle, &config.chat_shortcut) {
        log::warn!("[快捷键] 注册 AI 对话快捷键失败: {}", e);
    } else {
        log::info!("[快捷键] 已注册 AI 对话快捷键: {}", config.chat_shortcut);
    }
    if !config.notes_shortcut_enabled {
        log::info!("[快捷键] 速记快捷键已禁用，跳过注册");
    } else if let Err(e) = register_toggle_shortcut(&handle, &config.notes_shortcut) {
        log::warn!("[快捷键] 注册速记快捷键失败: {}", e);
    } else {
        log::info!("[快捷键] 已注册速记快捷键: {}", config.notes_shortcut);
    }
    Ok(())
}

/// 把「旧快捷键 → 新快捷键」的改绑一次做完：冲突预检、反注册旧的、注册新的，
/// 注册失败时回滚旧键。五个可自定义快捷键（主窗/剪贴板/搜索/AI 对话/速记）的
/// set_*_shortcut 命令共用这一份逻辑，只是各自读写配置里自己的字段。
pub fn rebind_shortcut(app: &AppHandle, previous: &str, next: &str) -> Result<(), String> {
    if crate::shortcut::is_shortcut_registered(app, next) {
        return Err("快捷键冲突".into());
    }
    if let Err(e) = unregister_toggle_shortcut(app, previous) {
        if !is_conflict_error(&e) {
            return Err(e);
        }
        return Err("快捷键冲突".into());
    }
    if let Err(e) = register_toggle_shortcut(app, next) {
        let mapped = format_shortcut_error(&e);
        if !mapped.eq(&e) {
            let _ = register_toggle_shortcut(app, previous);
        }
        return Err(mapped);
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
