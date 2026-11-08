//! 桌面层挂靠（Windows 桌面宿主模式）
//!
//! 让主窗成为桌面层的一部分，`Win+D` / 「显示桌面」不最小化它。
//!
//! ## 方案
//!
//! `SetParent(hwnd, SHELLDLL_DefView)` 将主窗挂到桌面图标层下面：
//! - `SHELLDLL_DefView` 是 Explorer 管理的桌面图标列表窗口，作为 `WorkerW` 的子窗存在。
//! - Explorer 不允许第三方窗口直接成为 `WorkerW` 的子窗（`SetParent` 返回错误 87）。
//! - `SetParent` 会正确处理窗口状态转换（自动管理 `WS_CHILD` 标志）。
//! - 配合 `WS_EX_TOOLWINDOW` + 清 `WS_EX_APPWINDOW` 实现无任务栏、不进 Alt+Tab。
//! - Win10/11 的 `Win+D` 不操作 `SHELLDLL_DefView` 下的子窗。
//!
//! ## 为什么需要纠偏线程
//!
//! tao 的 `WindowFlags::apply_diff` 每次 hide → show 都会重建窗口 flags，
//! 会覆盖 `WS_CHILD` / `WS_EX_TOOLWINDOW`。800ms 线程做幂等收敛。
//!
//! ## 入口
//!
//! - 设置 → 常规 →「桌面层模式」= 启动默认（`config.desktop_shell_mode_enabled`）
//! - 标题栏「桌面模式」按钮 = 本次会话切换（`ACTIVE` 运行期状态，不落盘）

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "windows")]
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WATCHDOG: AtomicBool = AtomicBool::new(false);

pub fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

pub fn configured() -> bool {
    crate::config::load().desktop_shell_mode_enabled
}

/// 启动时按配置挂靠（失败不阻断启动）
pub fn init(app: &tauri::AppHandle) {
    if !is_supported() || !configured() {
        return;
    }
    match attach_to_desktop(app) {
        Ok(()) => log::info!("[桌面层] 启动即挂靠（启动默认模式已开启）"),
        Err(e) => log::warn!("[桌面层] 启动挂靠失败，退回普通窗口: {e}"),
    }
}

/// 还原普通窗口几何
fn restore_geometry(win: &tauri::Window, hwnd: isize) {
    let cfg = crate::config::load();
    let (width, height) = (cfg.window.width, cfg.window.height);
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

fn attach_to_desktop(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let win = crate::main_window(app).ok_or_else(|| "未找到主窗口".to_string())?;
        let hwnd = win.hwnd().map_err(|e| e.to_string())?;
        crate::persist_window_state(app);
        // 子窗模式下 `WS_MAXIMIZE` 与 `WS_CHILD` 冲突，挂前退出最大化
        let was_visible = imp::is_visible(hwnd.0);
        imp::neutralize_restore_rect(hwnd.0);
        let _ = win.unmaximize();
        imp::attach(hwnd.0)?;
        if !was_visible {
            imp::hide(hwnd.0);
        }
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

/// 退出桌面层：解除父窗、还原原始样式、按配置恢复几何并补回置顶
pub fn restore_normal_window(app: &tauri::AppHandle) -> Result<(), String> {
    ACTIVE.store(false, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let win = crate::main_window(app).ok_or_else(|| "未找到主窗口".to_string())?;
        let hwnd = win.hwnd().map_err(|e| e.to_string())?;
        let was_visible = imp::is_visible(hwnd.0);
        imp::detach(hwnd.0);
        let _ = win.set_skip_taskbar(false);
        restore_geometry(&win, hwnd.0 as isize);
        let _ = win.set_always_on_top(crate::config::load().window.always_on_top);
        let _ = win.show();
        if was_visible {
            imp::ensure_visible(hwnd.0);
        }
        let _ = win.set_focus();
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(())
    }
}

/// 纯运行期切换，不落盘
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

/// 桌面层模式下显示主窗：无激活显示 + 立即补齐挂靠与全屏几何。
/// 必须在主线程做（tao 的 `show()` 会重建 flags，抹掉 `WS_CHILD` / `WS_EX_TOOLWINDOW`）
pub fn show_main(app: &tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
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
                log::warn!("[桌面层] 补齐挂靠失败: {e}");
            }
            let _ = win.show();
            if let Err(e) = imp::attach(hwnd.0) {
                log::warn!("[桌面层] 补齐挂靠失败: {e}");
            }
            let _ = win.set_skip_taskbar(true);
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 800ms 纠偏线程：幂等样式/父窗/几何收敛，不碰 tauri 窗口生命周期
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
        FindWindowExW, FindWindowW, GetParent,
        GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsWindowVisible, SetParent,
        SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WS_CHILD,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZE, WS_MINIMIZE, WS_POPUP, WS_VISIBLE,
        SW_HIDE,
    };

    /// 回退时逐位还原的原始样式
    static SAVED_STYLE: AtomicIsize = AtomicIsize::new(0);
    static SAVED_EXSTYLE: AtomicIsize = AtomicIsize::new(0);
    static STYLES_SAVED: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 查找 `SHELLDLL_DefView`：先找 Progman 下的，再找 WorkerW 下的
    fn find_desktop_icon_view() -> Option<HWND> {
        unsafe {
            let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
            if !progman.is_null() {
                let def_view = FindWindowExW(progman, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
                if !def_view.is_null() {
                    return Some(def_view);
                }
            }
            let workerw = FindWindowW(wide("WorkerW").as_ptr(), std::ptr::null());
            if !workerw.is_null() {
                let def_view = FindWindowExW(workerw, std::ptr::null_mut(), wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
                if !def_view.is_null() {
                    return Some(def_view);
                }
            }
            None
        }
    }

    /// 子窗样式 + 工具窗样式
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

    /// 把「还原矩形」压成当前矩形，防止退出桌面层时跳回错误尺寸
    pub fn neutralize_restore_rect(hwnd: HWND) {
        unsafe {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(hwnd, &mut wp) == 0 { return; }
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 { return; }
            wp.rcNormalPosition = rect;
            let _ = SetWindowPlacement(hwnd, &wp);
        }
    }

    pub fn set_rect(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            SetWindowPos(
                hwnd, std::ptr::null_mut(),
                x, y, width.max(1), height.max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub fn is_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    /// 原生隐藏（挂靠中防止顶出本该隐藏的窗口）
    pub fn hide(hwnd: HWND) {
        unsafe { ShowWindow(hwnd, SW_HIDE); }
    }

    /// 样式还原后兜底可见
    pub fn ensure_visible(hwnd: HWND) {
        if !is_visible(hwnd) {
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE); }
        }
    }

    /// 退出时还原的 `GWL_STYLE`
    pub(super) fn restored_style(saved: isize, current: isize) -> isize {
        let base = saved & !(WS_MAXIMIZE as isize | WS_MINIMIZE as isize | WS_VISIBLE as isize);
        base | (current & WS_VISIBLE as isize)
    }

    pub fn attach(hwnd: HWND) -> Result<(), String> {
        let icon_view = find_desktop_icon_view()
            .ok_or_else(|| "未找到桌面图标层".to_string())?;
        unsafe {
            if !STYLES_SAVED.swap(true, Ordering::SeqCst) {
                SAVED_STYLE.store(GetWindowLongPtrW(hwnd, GWL_STYLE), Ordering::SeqCst);
                SAVED_EXSTYLE.store(GetWindowLongPtrW(hwnd, GWL_EXSTYLE), Ordering::SeqCst);
            }
            fix_styles(hwnd);
            let old_parent = GetParent(hwnd);
            if old_parent != icon_view {
                SetParent(hwnd, icon_view);
                if GetParent(hwnd) != icon_view {
                    return Err("挂靠失败".to_string());
                }
            }
            fill_work_area(hwnd, icon_view);
            SetWindowPos(
                hwnd, HWND_TOP,
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    pub fn detach(hwnd: HWND) {
        unsafe {
            let current = GetWindowLongPtrW(hwnd, GWL_STYLE);
            if !GetParent(hwnd).is_null() {
                SetParent(hwnd, std::ptr::null_mut());
            }
            if STYLES_SAVED.swap(false, Ordering::SeqCst) {
                let style = restored_style(SAVED_STYLE.load(Ordering::SeqCst), current);
                SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, SAVED_EXSTYLE.load(Ordering::SeqCst));
            } else {
                SetWindowLongPtrW(hwnd, GWL_STYLE,
                    (current | WS_POPUP as isize) & !(WS_CHILD as isize));
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE,
                    (ex | WS_EX_APPWINDOW as isize) & !(WS_EX_TOOLWINDOW as isize));
            }
            SetWindowPos(
                hwnd, HWND_TOP,
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }

    /// 幂等纠偏：样式 → 父窗 → （可见时）全屏几何与 z 序
    pub fn ensure(hwnd: HWND) {
        let Some(icon_view) = find_desktop_icon_view() else { return; };
        unsafe {
            fix_styles(hwnd);
            if GetParent(hwnd) != icon_view {
                SetParent(hwnd, icon_view);
            }
            if IsWindowVisible(hwnd) != 0 {
                fill_work_area(hwnd, icon_view);
                SetWindowPos(
                    hwnd, HWND_TOP,
                    0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// 铺满工作区。子窗坐标相对父窗客户区原点，需要减去父窗屏幕偏移
    fn fill_work_area(hwnd: HWND, parent: HWND) {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 { return; }
            let work = info.rcWork;
            let mut origin = POINT { x: 0, y: 0 };
            ClientToScreen(parent, &mut origin);
            let w = (work.right - work.left).max(1);
            let h = (work.bottom - work.top).max(1);
            SetWindowPos(
                hwnd, HWND_TOP,
                work.left - origin.x, work.top - origin.y,
                w, h,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}
