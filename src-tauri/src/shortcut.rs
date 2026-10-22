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
        // 旧键已反注册，任何失败都必须把它装回去，否则用户改坏一个键会连带丢掉原来那个。
        // （旧实现靠 `mapped != e` 判断「是否认得这个错误」，认不出时就漏了回滚）
        if let Err(restore_err) = register_toggle_shortcut(app, previous) {
            log::warn!("[快捷键] 回滚旧键 {previous} 失败: {restore_err}");
        }
        return Err(format_shortcut_error(&e));
    }
    Ok(())
}

/// 是否「主键解析失败」类错误。判定必须先于冲突判定：插件把解析错误也写成
/// `Couldn't recognize "X" as a valid HotKey Code …`——含 "HotKey" 字样，
/// 若先判冲突就会把「键名不认识」误报成「快捷键冲突」（实测踩过）。
pub fn is_parse_error(err: &str) -> bool {
    err.contains("Couldn't recognize")
        || err.contains("valid HotKey Code")
        || err.contains("Unexpected hotkey string format")
        || err.contains("hotkey should have the modifiers first")
        || err.contains("Unexpected empty token")
        || err.contains("UnsupportedKey")
        || err.contains("InvalidFormat")
        || err.contains("EmptyToken")
}

pub fn format_shortcut_error(err: &str) -> String {
    if is_parse_error(err) {
        "快捷键格式无法识别，请重新录入（修饰键在前、单个主键，如 Ctrl+Shift+K）".to_string()
    } else if is_conflict_error(err) {
        "快捷键冲突".to_string()
    } else {
        err.to_string()
    }
}

pub fn is_conflict_error(err: &str) -> bool {
    if is_parse_error(err) {
        return false;
    }
    err.contains("already registered") || err.contains("already") || err.contains("occupied")
}

/// 允许无修饰键单键作快捷键的键：只放行功能键。
/// 空格/字母/数字单键一旦注册就全局劫持——空格最狠，全系统输入框都打不出空格，
/// 用户连回设置里改回来都困难（实测确认），故在写盘/注册前就拦下。
fn is_standalone_key_allowed(key: &str) -> bool {
    let k = key.trim();
    let f = |s: &str| {
        s.strip_prefix('F')
            .and_then(|n| n.parse::<u32>().ok())
            .map(|n| (1..=24).contains(&n))
            .unwrap_or(false)
    };
    f(k)
}

/// 校验一个自定义快捷键的键值是否可保存（与前端 utils/shortcutKeys.ts 同口径）。
/// 手工输入、老配置回填、IPC 直接调用都经这里，不能只靠前端录制器拦。
pub fn validate_custom_shortcut(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("快捷键不能为空".into());
    }
    let parsed = Shortcut::from_str(value)
        .map_err(|_| "快捷键格式无法识别，请重新录入（修饰键在前、单个主键，如 Ctrl+Shift+K）".to_string())?;
    // mods 为空 = 无修饰键的单键快捷键
    if parsed.mods.is_empty() && !is_standalone_key_allowed(&parsed.key.to_string()) {
        let key = parsed.key.to_string();
        if key == "Space" {
            return Err(
                "空格不能单独作快捷键（会让所有程序都打不出空格）；请加修饰键，如 Ctrl+Shift+Space"
                    .into(),
            );
        }
        return Err(format!(
            "「{key}」不能单独作快捷键（会劫持所有程序的这个键）；请加修饰键，如 Ctrl+Shift+{key}"
        ));
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    // ---- 错误串判定：解析失败必须先于冲突判定 ----
    // 插件把「键名不认识」也写成 `Couldn't recognize "X" as a valid HotKey Code …`，
    // 含 "HotKey" 字样；旧实现先判冲突，于是 `Ctrl+Process` 这类坏值被误报成「快捷键冲突」。
    const REAL_PARSE_ERR: &str = "Couldn't recognize \"Process\" as a valid key for hotkey, if you feel like it should be, please report this to https://github.com/tauri-apps/muda";

    #[test]
    fn parse_error_is_not_reported_as_conflict() {
        assert!(is_parse_error(REAL_PARSE_ERR));
        assert!(!is_conflict_error(REAL_PARSE_ERR));
        let msg = format_shortcut_error(REAL_PARSE_ERR);
        assert!(msg.contains("格式无法识别"), "实际: {msg}");
        assert!(!msg.contains("冲突"), "解析失败不该说成冲突: {msg}");
    }

    #[test]
    fn real_conflict_error_still_mapped_to_conflict() {
        let err = "HotKey already registered: HotKey { mods: CONTROL, key: Space }";
        assert!(is_conflict_error(err));
        assert!(!is_parse_error(err));
        assert_eq!(format_shortcut_error(err), "快捷键冲突");
    }

    // ---- 单键劫持校验：空格/字母不许单独作快捷键，功能键放行 ----
    #[test]
    fn bare_space_is_rejected_with_actionable_hint() {
        let msg = validate_custom_shortcut("Space").unwrap_err();
        assert!(msg.contains("空格"), "实际: {msg}");
        assert!(msg.contains("Ctrl+Shift+Space"), "要给出可照抄的例子: {msg}");
    }

    #[test]
    fn bare_letter_is_rejected() {
        assert!(validate_custom_shortcut("K").is_err());
        assert!(validate_custom_shortcut("1").is_err());
    }

    #[test]
    fn function_keys_may_stand_alone() {
        for k in ["F1", "F9", "F12", "F24"] {
            assert!(validate_custom_shortcut(k).is_ok(), "{k} 应放行");
        }
        // F25 不在插件支持范围内
        assert!(validate_custom_shortcut("F25").is_err());
    }

    #[test]
    fn space_with_modifiers_is_accepted() {
        for k in ["Ctrl+Space", "Ctrl+Shift+Space", "Alt+Space"] {
            assert!(validate_custom_shortcut(k).is_ok(), "{k} 应放行");
        }
    }

    #[test]
    fn malformed_input_rejected() {
        assert!(validate_custom_shortcut("").is_err());
        assert!(validate_custom_shortcut("Ctrl+").is_err());
        assert!(validate_custom_shortcut("Ctrl+K+Shift").is_err());
        assert!(validate_custom_shortcut("Ctrl+Process").is_err());
    }
}
