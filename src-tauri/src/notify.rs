//! Windows 系统托盘气泡通知
//!
//! 便携版（无安装器，bundle.active=false）下 tauri-plugin-notification 的系统 toast
//! 底层用 `ToastNotificationManager::CreateToastNotifierWithId(AUMID)`，要求「开始菜单
//! 快捷方式」已注册对应 AUMID，否则静默失败、不弹任何通知。
//! 这里改用 Win32 `Shell_NotifyIconW` 气泡：无需安装器，Win10/11 会在操作中心将其
//! 渲染为系统级通知，便携版可稳定弹出。

use tauri::AppHandle;

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use windows_sys::core::GUID;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_GUID, NIF_ICON, NIF_INFO, NIIF_INFO, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, IMAGE_ICON, LoadImageW, RegisterClassExW, WNDCLASSEXW,
        WS_POPUP,
    };

    /// 通知图标 GUID（唯一标识，避免与托盘图标互相干扰）
    const NOTIFY_GUID: GUID = GUID {
        data1: 0x8F3A4B2C,
        data2: 0x1E6D,
        data3: 0x4A92,
        data4: [0x9B, 0xC5, 0x2D, 0xE4, 0x7A, 0x10, 0x63, 0xF8],
    };

    const CLASS_NAME: &str = "xhub_balloon_window";

    /// 承载通知图标的隐藏窗口（进程生命周期内只建一次）
    /// 用 usize 存储 HWND，避免裸指针无法跨线程（static 需 Sync）
    static HIDDEN_WINDOW: OnceLock<usize> = OnceLock::new();
    /// 图标是否已注册（首次 NIM_ADD，之后 NIM_MODIFY 复用）
    static ICON_ADDED: AtomicBool = AtomicBool::new(false);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: *mut core::ffi::c_void,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// 惰性创建隐藏窗口
    unsafe fn ensure_hidden_window() -> *mut core::ffi::c_void {
        if let Some(&hwnd) = HIDDEN_WINDOW.get() {
            return hwnd as *mut core::ffi::c_void;
        }
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if !hwnd.is_null() {
            let _ = HIDDEN_WINDOW.set(hwnd as usize);
        }
        hwnd
    }

    /// 取 exe 内嵌图标（资源 ID 1）作为通知图标
    unsafe fn app_icon() -> *mut core::ffi::c_void {
        let hinstance = GetModuleHandleW(std::ptr::null());
        LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 32, 32, 0)
    }

    pub(super) unsafe fn show_balloon(title: &str, body: &str) {
        let hwnd = ensure_hidden_window();
        if hwnd.is_null() {
            log::error!("系统通知：承载窗口创建失败");
            return;
        }

        let info = wide(body);
        let info_title = wide(title);
        let mut info_arr = [0u16; 256];
        let mut title_arr = [0u16; 64];
        let n = info.len().min(255);
        info_arr[..n].copy_from_slice(&info[..n]);
        let m = info_title.len().min(63);
        title_arr[..m].copy_from_slice(&info_title[..m]);

        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_GUID | NIF_INFO;
        nid.guidItem = NOTIFY_GUID;
        let hicon = app_icon();
        if !hicon.is_null() {
            nid.hIcon = hicon;
            nid.uFlags |= NIF_ICON;
        }
        nid.szInfo = info_arr;
        nid.szInfoTitle = title_arr;
        nid.dwInfoFlags = NIIF_INFO;
        nid.Anonymous.uTimeout = 5_000;

        // 首次注册图标，之后复用（每次置 NIF_INFO 即弹新气泡）
        let added = ICON_ADDED.load(Ordering::SeqCst);
        let msg = if added { NIM_MODIFY } else { NIM_ADD };
        let ok = Shell_NotifyIconW(msg, &nid) != 0;
        if !added {
            ICON_ADDED.store(true, Ordering::SeqCst);
        }
        if !ok {
            log::error!(
                "系统通知：Shell_NotifyIcon 失败, 错误={}",
                std::io::Error::last_os_error()
            );
        }
    }
}

/// 显示系统通知（Win10/11 渲染为操作中心里的系统级通知）。
/// 统一派发到主线程，避免 Win32 窗口线程归属问题。
pub fn show_system_notification(app: &AppHandle, title: &str, body: &str) {
    #[cfg(windows)]
    {
        let title = title.to_owned();
        let body = body.to_owned();
        let _ = app.run_on_main_thread(move || unsafe {
            imp::show_balloon(&title, &body);
        });
    }
    #[cfg(not(windows))]
    let _ = (app, title, body);
}
