//! WebView2 内存目标级别联动（「隐藏窗口降低内存占用」，内存优化 P0）。
//!
//! 原理：WebView2 的 `ICoreWebView2_19::SetMemoryUsageTargetLevel`（微软
//! MemoryUsageTargetLevel 规范）让 renderer 在 Low 档主动弃缓存、把可换页内存
//! 压出去——**脚本继续跑、事件照常收**（区别于 TrySuspend 冻结渲染进程），
//! 与约定 41「浮窗启动预创建 + 隐藏常驻」完全兼容：窗口生命周期一根手指不碰，
//! 只在窗口隐藏时把常驻 renderer 的内存吐出来一部分，显示前恢复 Normal。
//!
//! 双保险结构：
//! - 快路径：各显隐函数（tray / clipboard / chat / ball / notify / suda）在
//!   show/hide 调用点直接翻转级别，保证「显示前先 Normal」的首帧体验；
//! - 轮询纠偏：300ms 兜底线程按窗口实际可见性收敛，覆盖漏接线的路径、任务栏
//!   还原最小化主窗等绕过 Rust 显隐函数的路径。状态表去重，级别没变不打 COM。
//!
//! 范围：主窗 / 对话 / 剪贴板 / 通知 / 悬浮球 / 提示词与待办浮窗 / 速达池窗口
//! （chrome+content 双 webview）/ 扩展独立窗（ext-*）。sticky-\*、countdown-\*
//! 属于低频瞬态窗（用完即毁），不参与。suda-panel 是主窗子 webview，与主窗
//! 共享 renderer 的可能性未证实，**故意不设**——若与主窗同 renderer，在主窗
//! 可见时把它设 Low 会拖累整个主窗。
//!
//! ⚠️ 本模块只做内存级别翻转，禁止出现任何 build/destroy（约定 41）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::Manager;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Normal,
    Low,
}

/// 已设置的级别（webview label → level）。快路径与轮询共用：级别没变就不重复
/// Set（SetMemoryUsageTargetLevel 是属性写入，也不该 300ms 一次地反复打）。
static LEVELS: Mutex<Option<HashMap<String, Level>>> = Mutex::new(None);

/// 旧 WebView2 Runtime 没有 ICoreWebView2_19 / COM 调用失败：只告警一次，
/// 别按 300ms 轮询频率刷日志
static WARNED: AtomicBool = AtomicBool::new(false);

pub fn init(app: &tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        let handle = app.clone();
        let _ = std::thread::Builder::new()
            .name("webview-mem".into())
            .spawn(move || poll_loop(handle));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

#[cfg(target_os = "windows")]
fn poll_loop(app: tauri::AppHandle) {
    // 开关缓存：每 30 跳（约 9s）重读一次配置，改设置后无需重启即生效（有迟滞）
    let mut enabled = true;
    let mut tick: u32 = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        tick = tick.wrapping_add(1);
        if tick % 30 == 1 {
            enabled = crate::config::load().webview_mem_low_on_hide;
        }
        for (win_label, webviews) in managed_targets(&app) {
            // 最小化的主窗按「视觉不可见」处理（与悬浮球联动同口径）。
            // ⚠️ 必须用 get_window：主窗自 suda-panel 起是多 webview 窗口，
            // get_webview_window("main") 恒为 None（约定 66），用它会把可见中的
            // 主窗判成不可见 → 每 300ms 把快路径恢复的 Normal 打回 Low
            let visible = app
                .get_window(&win_label)
                .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
                .unwrap_or(false);
            apply(&app, &webviews, enabled && !visible);
        }
    }
}

/// 参与联动的窗口：可见性取自窗口本身（win），级别设到它的 webview 上
/// （单 webview 窗两者同 label；速达池窗口额外覆盖 chrome/content 两个子 webview，
/// 同窗口的 webview 大概率共享 renderer，翻转一次即覆盖）。
#[cfg(target_os = "windows")]
fn managed_targets(app: &tauri::AppHandle) -> Vec<(String, Vec<String>)> {
    let mut targets: Vec<(String, Vec<String>)> = vec![
        ("main".to_string(), vec!["main".to_string()]),
        (
            crate::chat_window::LABEL.to_string(),
            vec![crate::chat_window::LABEL.to_string()],
        ),
        (
            crate::clipboard::CLIPBOARD_WINDOW_LABEL.to_string(),
            vec![crate::clipboard::CLIPBOARD_WINDOW_LABEL.to_string()],
        ),
        (
            crate::notify::NOTICE_LABEL.to_string(),
            vec![crate::notify::NOTICE_LABEL.to_string()],
        ),
        (
            crate::floating_ball::LABEL.to_string(),
            vec![crate::floating_ball::LABEL.to_string()],
        ),
        (
            crate::float_window::PROMPT_FLOAT_LABEL.to_string(),
            vec![crate::float_window::PROMPT_FLOAT_LABEL.to_string()],
        ),
        (
            crate::float_window::TODO_FLOAT_LABEL.to_string(),
            vec![crate::float_window::TODO_FLOAT_LABEL.to_string()],
        ),
    ];
    for i in 0..crate::suda_browser::POOL_SIZE {
        targets.push((
            crate::suda_browser::win_label(i),
            vec![
                crate::suda_browser::chrome_label(i),
                crate::suda_browser::content_label(i),
            ],
        ));
    }
    // 扩展独立窗（ext-{id}，按需创建）：从全量 webview 表按前缀捞，新开的自动纳入
    for label in app.webview_windows().into_keys() {
        if label.starts_with("ext-") {
            targets.push((label.clone(), vec![label]));
        }
    }
    targets
}

/// 把一组 webview 的内存级别收敛到目标值（带去重；窗口不存在的 label 清出状态表）
#[cfg(target_os = "windows")]
fn apply(app: &tauri::AppHandle, labels: &[String], low: bool) {
    let want = if low { Level::Low } else { Level::Normal };
    let mut guard = match LEVELS.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let map = guard.get_or_insert_with(HashMap::new);
    for label in labels {
        if map.get(label) == Some(&want) {
            continue;
        }
        match app.get_webview(label) {
            Some(wv) => {
                set_level(&wv, low);
                log::info!("[webview-mem] {label} → {}", if low { "Low" } else { "Normal" });
                map.insert(label.clone(), want);
            }
            None => {
                // 窗口尚未创建（按需窗）或已销毁：清掉旧状态，出现后下次再设
                map.remove(label);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_level(wv: &tauri::Webview, low: bool) {
    let _ = wv.with_webview(move |platform| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            ICoreWebView2_19,
        };
        use windows_core::Interface;

        let controller = platform.controller();
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(c) => c,
            Err(e) => {
                warn_once(&format!("取 CoreWebView2 失败: {e}"));
                return;
            }
        };
        // 旧 WebView2 Runtime 无 ICoreWebView2_19：cast 失败 = 本机不支持，静默降级
        let core19 = match core.cast::<ICoreWebView2_19>() {
            Ok(c) => c,
            Err(_) => {
                warn_once("WebView2 Runtime 过旧（无 ICoreWebView2_19），内存级别联动不可用");
                return;
            }
        };
        let level = if low {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        if let Err(e) = unsafe { core19.SetMemoryUsageTargetLevel(level) } {
            warn_once(&format!("SetMemoryUsageTargetLevel 失败: {e}"));
        }
    });
}

fn warn_once(msg: &str) {
    if !WARNED.swap(true, Ordering::Relaxed) {
        log::warn!("[webview-mem] {msg}");
    }
}

// ---------- 快路径（各显隐函数调用；非 Windows 下为 no-op） ----------

/// 窗口即将显示：在 `win.show()` **之前**调用，先恢复 Normal 保证首帧不软
pub fn on_shown(app: &tauri::AppHandle, label: &str) {
    #[cfg(target_os = "windows")]
    apply(app, &[label.to_string()], false);
    #[cfg(not(target_os = "windows"))]
    let _ = (app, label);
}

/// 窗口已隐藏：在 `win.hide()` 之后调用
pub fn on_hidden(app: &tauri::AppHandle, label: &str) {
    #[cfg(target_os = "windows")]
    {
        if crate::config::load().webview_mem_low_on_hide {
            apply(app, &[label.to_string()], true);
        } else {
            apply(app, &[label.to_string()], false);
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, label);
}

/// 速达池槽位：chrome/content 双 webview 跟随窗口显隐一起翻转
pub fn on_slot(app: &tauri::AppHandle, i: usize, shown: bool) {
    #[cfg(target_os = "windows")]
    {
        let labels = vec![
            crate::suda_browser::chrome_label(i),
            crate::suda_browser::content_label(i),
        ];
        let low = !shown && crate::config::load().webview_mem_low_on_hide;
        apply(app, &labels, low);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, i, shown);
}

#[cfg(test)]
mod tests {
    /// P3 守卫：Rust 常量与 tauri.conf.json 主窗的 additionalBrowserArgs 必须逐字
    /// 一致——两处都是真相源，不一致时 WebView2 按参数差分裂 environment，
    /// 多出一整棵浏览器进程树（内存不降反暴增）。
    /// macOS 移植说明：主窗配置**有意去掉**了该 WebView2 专属字段（WKWebView 不
    /// 消费它），守卫只在 Windows 生效，防止平台差异被误「修」回来。
    #[cfg(target_os = "windows")]
    #[test]
    fn additional_browser_args_match_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let args = conf["app"]["windows"][0]["additionalBrowserArgs"]
            .as_str()
            .expect("tauri.conf.json 主窗必须声明 additionalBrowserArgs");
        assert_eq!(args, crate::ADDITIONAL_BROWSER_ARGS);
    }
}
