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

/// 固定常驻窗清单（不含按需窗 ext-*，那批由调用方按前缀补）。
///
/// ⚠️ 遗漏即白烧内存（2026-10-31 实测）：录屏三窗 + 贴图池**启动即隐藏、可能整场
/// 会话都不显示**，全靠轮询纠偏才会落 Low——`on_hidden` 快路径永远不会为它们触发。
/// 此前它们不在表里，3 个加载完整 SPA 的贴图 renderer 常年停在 Normal 档。
///
/// 刻意排除的两类：
/// - `sticky-*` / `countdown-*`：低频瞬态窗，用完即毁，没有常驻代价；
/// - `export-render`：按需渲染表面，`render_pdf` 语义是「**隐藏但活跃**」（先 hide
///   再 on_shown 恢复 Normal 去跑打印管线）。可见性在这里不是忙碌信号，纳入本表
///   会被轮询每 300ms 打回 Low，与它自己的 on_shown/on_hidden 语义打架。
#[cfg(target_os = "windows")]
fn resident_targets() -> Vec<(String, Vec<String>)> {
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
        // 截图/录屏：选区层 + 控制条
        (
            crate::recorder::RECORDER_OVERLAY_LABEL.to_string(),
            vec![crate::recorder::RECORDER_OVERLAY_LABEL.to_string()],
        ),
        (
            crate::recorder::RECORDER_CONTROL_LABEL.to_string(),
            vec![crate::recorder::RECORDER_CONTROL_LABEL.to_string()],
        ),
    ];
    // 贴图窗口池（pin-shot-{0..N-1}）：启动预创建、隐藏常驻，可能整场会话都不显示
    for i in 0..crate::recorder::PIN_WINDOW_COUNT {
        let label = format!("{}{}", crate::recorder::PIN_LABEL_PREFIX, i);
        targets.push((label.clone(), vec![label]));
    }
    targets
}

/// 参与联动的窗口：可见性取自窗口本身（win），级别设到它的 webview 上
/// （单 webview 窗两者同 label；速达池窗口额外覆盖 chrome/content 两个子 webview，
/// 同窗口的 webview 大概率共享 renderer，翻转一次即覆盖）。
#[cfg(target_os = "windows")]
fn managed_targets(app: &tauri::AppHandle) -> Vec<(String, Vec<String>)> {
    let mut targets = resident_targets();
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
    /// 守卫：所有「启动即隐藏常驻、可能整场会话都不显示」的窗口都必须在常驻表里
    /// ——它们的 `on_hidden` 快路径永远不会触发，漏登记 = 完整 SPA 的 renderer 常年
    /// 停在 Normal 档白烧内存（2026-10-31 实测贴图池 + 录屏三窗全漏）。
    #[cfg(target_os = "windows")]
    #[test]
    fn resident_targets_cover_startup_hidden_windows() {
        use super::resident_targets;
        let labels: Vec<String> = resident_targets().into_iter().map(|(w, _)| w).collect();
        let mut expected = vec![
            crate::recorder::RECORDER_OVERLAY_LABEL.to_string(),
            crate::recorder::RECORDER_CONTROL_LABEL.to_string(),
            crate::export_render::WINDOW_LABEL.to_string(),
        ];
        for i in 0..crate::recorder::PIN_WINDOW_COUNT {
            expected.push(format!("{}{}", crate::recorder::PIN_LABEL_PREFIX, i));
        }
        for label in expected {
            // export-render 是刻意排除项（见 resident_targets 注释），只断言不存在
            if label == crate::export_render::WINDOW_LABEL {
                assert!(
                    !labels.contains(&label),
                    "{label} 是「隐藏但活跃」的渲染表面，不进常驻表"
                );
                continue;
            }
            assert!(labels.contains(&label), "{label} 必须进常驻表（启动即隐藏）");
        }
        // 每个窗口的 webview label 必须自带一份（apply 按 webview label 设级别）
        for (win, webviews) in resident_targets() {
            assert!(
                webviews.contains(&win),
                "{win} 的 webview 列表必须包含自身 label"
            );
        }
    }

    /// P3 守卫：Rust 常量与 tauri.conf.json 主窗的 additionalBrowserArgs 必须逐字
    /// 一致——两处都是真相源，不一致时 WebView2 按参数差分裂 environment，
    /// 多出一整棵浏览器进程树（内存不降反暴增）。
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
