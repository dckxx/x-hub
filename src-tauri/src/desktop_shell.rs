//! 桌面层挂靠（Windows 桌面宿主模式，最小可用版）
//!
//! ## 目标
//!
//! 让**整个 X-HUB 主窗**成为「桌面层」的一部分，而不是一个普通应用窗口：
//!
//! - `SetParent` 到桌面窗口 `Progman`，并置 `WS_CHILD`（子窗不属于「顶层窗口」集合，
//!   Win+D / 「显示桌面」枚举不到它，因此不会被最小化）
//! - `WS_EX_TOOLWINDOW` + 清 `WS_EX_APPWINDOW`（任务栏无入口、不进 Alt+Tab）
//! - 铺满当前显示器工作区（默认全屏桌面样式；无边框是 tauri.conf 既有配置）
//! - 启动/唤起用 `SW_SHOWNA` 无激活显示（不抢焦点）
//!
//! ## 为什么不能只靠 `win_taskbar::apply`
//!
//! 工具窗样式只解决「任务栏/Alt+Tab」，Win+D 仍会把顶层窗口最小化；真正让窗口
//! 属于桌面层的是 **父窗关系 + `WS_CHILD`**（桌面窗口的子窗不参与 Show Desktop）。
//!
//! ## 为什么需要监视线程
//!
//! tao 每次 hide → show 都会重建窗口 flags（`WindowFlags::apply_diff` 重写
//! `GWL_EXSTYLE`/`GWL_STYLE`），会把 `WS_CHILD` / `WS_EX_TOOLWINDOW` 覆盖回去；
//! Explorer 刷新桌面时也可能把图标层重新抬到子窗 z 序前面。所以桌面层模式下跑一个
//! 800ms 的轻量纠偏线程：样式、父窗、全屏几何、z 序（仅可见时）逐项收敛。
//! 该线程只在模式开启时工作，关闭后只是空转（每 800ms 一次原子读）。
//!
//! ## 入口与状态（v0.9.9 定稿）
//!
//! 两个入口，职责分离：
//!
//! - **设置 → 常规 →「桌面层模式」开关** = **启动默认模式**（`config.desktop_shell_mode_enabled`）：
//!   只决定下次启动是否自动挂靠，[`init`] 在 `restore_window_state` 之后执行；点了不立刻切窗。
//! - **主窗标题栏「桌面模式」按钮** = **本次会话切换**（`TitleBar.vue` → `set_desktop_shell_mode`）：
//!   纯运行期状态 [`ACTIVE`]，**不落盘**，退出也不改设置。
//!
//! ## 回退
//!
//! 关闭时走 [`restore_normal_window`]：解除父窗、逐位还原原始样式、最后补回「窗口置顶」。
//! 桌面层模式下 [`crate::persist_window_state`] 不写盘，所以全屏尺寸不会被记成用户几何。

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "windows")]
/// 桌面层模式**运行期**状态：只有真正挂上桌面才为 true（不落盘）
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// 监视线程单例（重复 enable/disable 不重复起线程）
static WATCHDOG: AtomicBool = AtomicBool::new(false);

/// 平台能力：桌面挂靠是 Windows 专属；其他平台不显示入口（命令层直接报错）
pub fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

/// 当前是否处于桌面层模式（运行期状态位）
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// 配置里的「启动默认模式」开关
pub fn configured() -> bool {
    crate::config::load().desktop_shell_mode_enabled
}

/// 启动时按配置挂靠（失败只落日志、退回普通窗口，不阻断启动）。
///
/// 调用点固定在 `lib.rs` 的 `restore_window_state` 之后——先恢复常规几何，再铺满工作区。
pub fn init(app: &tauri::AppHandle) {
    if !is_supported() || !configured() {
        return;
    }
    match attach_to_desktop(app) {
        Ok(()) => log::info!("[桌面层] 启动即挂靠（设置里的启动默认模式已开启）"),
        Err(e) => log::warn!("[桌面层] 启动挂靠失败，退回普通窗口: {e}"),
    }
}

/// 还原普通窗口几何：**取配置里的值**，
fn restore_geometry(win: &tauri::Window, hwnd: isize) {
    let cfg = crate::config::load();
    let (width, height) = (cfg.window.width, cfg.window.height);
    // 位置没记过（首次运行）：只定尺寸，位置留在当前
    let pos = match (cfg.window.x, cfg.window.y) {
        (Some(x), Some(y)) => Some((x as i32, y as i32)),
        _ => None,
    };
    #[cfg(target_os = "windows")]
    {
        if let Some((x, y)) = pos {
            let scale = win.scale_factor().unwrap_or(1.0);
            let w = (width * scale).round().max(1.0) as i32;
            let h = (height * scale).round().max(1.0) as i32;
            imp::set_rect(hwnd as _, x, y, w, h);
        } else {
            let _ = win.set_size(tauri::LogicalSize::new(width, height));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        let _ = win.set_size(tauri::LogicalSize::new(width, height));
        if let Some((x, y)) = pos {
            let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
}

/// 挂靠到桌面层：父窗 = Progman + 子窗样式 + 工具窗样式 + 全屏工作区
fn attach_to_desktop(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let win = crate::main_window(app).ok_or_else(|| "未找到主窗口".to_string())?;
        let hwnd = win.hwnd().map_err(|e| e.to_string())?;
        // 配置里保留的是最后一次**常规**几何。
        crate::persist_window_state(app);
        // 桌面层是子窗：`WS_MAXIMIZE` 与 `WS_CHILD` 摆在一起会互相打架（子窗的位置
        // 与尺寸归父窗管，最大化态下铺不满或尺寸乱跳），所以挂靠前必须退出最大化。
        let was_visible = imp::is_visible(hwnd.0);
        imp::neutralize_restore_rect(hwnd.0);
        let _ = win.unmaximize();
        imp::attach(hwnd.0)?;
        if !was_visible {
            imp::hide(hwnd.0);
        }
        // 任务栏按钮兜底（tao 的 DeleteTab）：子窗本就没有按钮，这里只防样式被重建的间隙
        let _ = win.set_skip_taskbar(true);
        ACTIVE.store(true, Ordering::SeqCst);
        start_watchdog(app.clone());
        log::info!("[桌面层] 主窗已挂靠到 Windows 桌面层");
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("桌面模式仅支持 Windows".to_string())
    }
}

/// 退出桌面层：解除父窗、逐位还原原始样式，按**配置里的常规几何**恢复普通窗口
/// （**不回到最大化态**）并补回置顶
pub fn restore_normal_window(app: &tauri::AppHandle) -> Result<(), String> {
    ACTIVE.store(false, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let win = crate::main_window(app).ok_or_else(|| "未找到主窗口".to_string())?;
        let hwnd = win.hwnd().map_err(|e| e.to_string())?;
        // 退出前记下真实可见性
        let was_visible = imp::is_visible(hwnd.0);
        imp::detach(hwnd.0);
        let _ = win.set_skip_taskbar(false);
        restore_geometry(&win, hwnd.0 as isize);
        // 桌面层是 Progman 的子窗，置顶样式在那里没有意义；退出后按配置补回来
        let _ = win.set_always_on_top(crate::config::load().window.always_on_top);
        let _ = win.show();
        if was_visible {
            imp::ensure_visible(hwnd.0);
        }
        let _ = win.set_focus();
        log::info!(
            "[桌面层] 已退出桌面层，恢复普通窗口（可见={}）",
            imp::is_visible(hwnd.0)
        );
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(())
    }
}

/// 切换入口（命令 `set_desktop_shell_mode` 调用）：纯运行期动作，不落盘。
///
/// 失败时窗口保持原状（`ACTIVE` 只在挂靠成功后置位），前端按返回的状态位纠正按钮。
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if !is_supported() {
        return Err("桌面模式仅支持 Windows".to_string());
    }
    if enabled {
        attach_to_desktop(app)
    } else {
        restore_normal_window(app)
    }
}

/// 桌面层模式下的主窗显示：**无激活**显示（不抢焦点），并立即补齐挂靠与全屏几何。
/// 普通模式不要调用它（那会让窗口显示但不激活），走 `tray::show_window` 的常规分支。
///
/// 实现要点：显示动作必须在**主线程**上做（`run_on_main_thread`），因为 tao 的
/// `show()` 会把窗口样式按它自己缓存的 flags 重建——那一步会抹掉 `WS_CHILD` /
/// `WS_EX_TOOLWINDOW`（约定 48 同源机理），所以「show → 立即补挂靠」必须在同一个
/// 主线程回调里串行完成，中间不能留给别的线程插入。
pub fn show_main(app: &tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        // 先恢复内存级别再显示（webview_mem：Low 态 renderer 缓存已吐，首帧前回 Normal）
        crate::webview_mem::on_shown(app, "main");
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(win) = crate::main_window(&handle) else {
                return;
            };
            let Ok(hwnd) = win.hwnd() else {
                log::warn!("[桌面层] 取主窗句柄失败");
                return;
            };
            if let Err(e) = imp::attach(hwnd.0) {
                log::warn!("[桌面层] 显示前补齐挂靠失败: {e}");
            }
            let _ = win.show();
            if let Err(e) = imp::attach(hwnd.0) {
                log::warn!("[桌面层] 显示后补齐挂靠失败: {e}");
            }
            let _ = win.set_skip_taskbar(true);
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 800ms 纠偏线程：只做幂等的 Win32 样式/父窗/几何收敛，不碰 tauri 窗口生命周期
fn start_watchdog(app: tauri::AppHandle) {
    if WATCHDOG.swap(true, Ordering::SeqCst) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(800));
            if !is_active() {
                continue;
            }
            let Some(win) = crate::main_window(&app) else {
                continue;
            };
            let Ok(hwnd) = win.hwnd() else {
                continue;
            };
            imp::ensure(hwnd.0);
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetParent, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
        IsWindowVisible, SetParent, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos,
        ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WS_CHILD,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZE, WS_MINIMIZE, WS_POPUP, WS_VISIBLE,
        SW_HIDE,
    };

    /// 挂靠前主窗的原始 `GWL_STYLE` / `GWL_EXSTYLE`：回退时逐位还原，不靠猜。
    /// 只在首次挂靠时记录一次（watchdog / 每次 show 的补齐挂靠不会覆盖它）。
    static SAVED_STYLE: AtomicIsize = AtomicIsize::new(0);
    static SAVED_EXSTYLE: AtomicIsize = AtomicIsize::new(0);
    static STYLES_SAVED: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 桌面窗口（Progman）：Shell 的桌面顶层窗，`SHELLDLL_DefView`（图标层）是它的子窗。
    /// 把主窗挂到它下面 = 成为桌面层的一部分。
    fn desktop_window() -> Option<HWND> {
        let class = wide("Progman");
        let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if hwnd.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }

    /// 桌面层所需样式：子窗（Win+D 不最小化）+ 工具窗（无任务栏、不进 Alt+Tab）。
    fn fix_styles(hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let want = (style | WS_CHILD as isize)
                & !(WS_POPUP as isize | WS_MAXIMIZE as isize | WS_MINIMIZE as isize);
            if want != style {
                SetWindowLongPtrW(hwnd, GWL_STYLE, want);
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let want_ex = (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
            if want_ex != ex {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want_ex);
            }
        }
    }

    /// 把窗口的「还原矩形」压成它**当前的矩形**。
    pub fn neutralize_restore_rect(hwnd: HWND) {
        unsafe {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(hwnd, &mut wp) == 0 {
                return;
            }
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return;
            }
            wp.rcNormalPosition = rect;
            let _ = SetWindowPlacement(hwnd, &wp);
        }
    }

    pub fn set_rect(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                width.max(1),
                height.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Win32 口径的真实可见性（`IsWindowVisible`）。
    pub fn is_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    /// 原生隐藏（不经过 tao）：挂靠过程中若把本该隐藏的窗口顶了出来，用它复位。
    pub fn hide(hwnd: HWND) {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    }

    /// 兜底可见：样式还原后如果窗口仍是隐藏态，无激活地显示一次。
    pub fn ensure_visible(hwnd: HWND) {
        if !is_visible(hwnd) {
            unsafe {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    /// 退出时写回的 `GWL_STYLE`：**原始样式**剔除 `WS_MAXIMIZE` / `WS_MINIMIZE`
    pub(super) fn restored_style(saved: isize, current: isize) -> isize {
        let base = saved
            & !(WS_MAXIMIZE as isize | WS_MINIMIZE as isize | WS_VISIBLE as isize);
        base | (current & WS_VISIBLE as isize)
    }

    pub fn attach(hwnd: HWND) -> Result<(), String> {
        let parent = desktop_window().ok_or_else(|| "未找到桌面窗口（Progman）".to_string())?;
        unsafe {
            // 0) 首次挂靠记下原始样式（回退时逐位还原）
            if !STYLES_SAVED.swap(true, Ordering::SeqCst) {
                SAVED_STYLE.store(GetWindowLongPtrW(hwnd, GWL_STYLE), Ordering::SeqCst);
                SAVED_EXSTYLE.store(GetWindowLongPtrW(hwnd, GWL_EXSTYLE), Ordering::SeqCst);
            }
            // 1) 先改样式再改父窗：顶层窗 SetParent 不会自动加 WS_CHILD
            fix_styles(hwnd);
            // 2) 挂到桌面层
            if GetParent(hwnd) != parent {
                SetParent(hwnd, parent);
            }
            // 3) 默认全屏桌面样式：铺满当前显示器工作区（隐藏态也先摆好，显示时无跳变）
            fill_work_area(hwnd, parent);
            // 4) 抬到桌面图标层之上（Progman 的子窗 z 序顶部）
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    pub fn detach(hwnd: HWND) {
        unsafe {
            let current = GetWindowLongPtrW(hwnd, GWL_STYLE);
            // 1) 先解除父窗（子窗 → 顶层窗）
            if !GetParent(hwnd).is_null() {
                SetParent(hwnd, std::ptr::null_mut());
            }
            // 2) 还原原始样式（任务栏入口 / Alt+Tab / 焦点行为一并回来）。
            if STYLES_SAVED.swap(false, Ordering::SeqCst) {
                let style = restored_style(SAVED_STYLE.load(Ordering::SeqCst), current);
                SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, SAVED_EXSTYLE.load(Ordering::SeqCst));
            } else {
                // 异常路径（没记到原始样式）：至少保证是普通顶层窗
                SetWindowLongPtrW(
                    hwnd,
                    GWL_STYLE,
                    (current | WS_POPUP as isize) & !(WS_CHILD as isize),
                );
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    (ex | WS_EX_APPWINDOW as isize) & !(WS_EX_TOOLWINDOW as isize),
                );
            }
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }

    /// 幂等纠偏：样式 → 父窗 → （可见时）全屏几何与 z 序
    pub fn ensure(hwnd: HWND) {
        let Some(parent) = desktop_window() else {
            return;
        };
        unsafe {
            fix_styles(hwnd);
            if GetParent(hwnd) != parent {
                SetParent(hwnd, parent);
            }
            if IsWindowVisible(hwnd) != 0 {
                fill_work_area(hwnd, parent);
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// 铺满工作区。子窗坐标相对父窗客户区原点，所以减去 Progman 客户区原点
    /// （主屏下就是 0,0；副屏/负坐标多屏时靠 `ClientToScreen` 换算）。
    fn fill_work_area(hwnd: HWND, parent: HWND) {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return;
            }
            let work = info.rcWork;
            let mut origin = POINT { x: 0, y: 0 };
            ClientToScreen(parent, &mut origin);
            let w = (work.right - work.left).max(1);
            let h = (work.bottom - work.top).max(1);
            SetWindowPos(
                hwnd,
                HWND_TOP,
                work.left - origin.x,
                work.top - origin.y,
                w,
                h,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}
