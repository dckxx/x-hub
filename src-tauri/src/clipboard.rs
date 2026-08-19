use crate::commands::DbState;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};
use windows_sys::Win32::Foundation::{GlobalFree, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard,
    GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    RemoveClipboardFormatListener, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, DROPFILES, HDROP};
use windows_sys::Win32::System::Threading::AttachThreadInput;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, MapVirtualKeyW,
    VK_CONTROL, VK_ESCAPE, VK_INSERT, VK_LMENU, VK_LCONTROL, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW,
    RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, SetWindowsHookExW, SetWindowPos,
    ShowWindow, TranslateMessage, UnhookWindowsHookEx, UnregisterClassW, GWL_EXSTYLE,
    HWND_MESSAGE, HWND_TOPMOST, MSG, MSLLHOOKSTRUCT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, SW_RESTORE, WH_MOUSE_LL, WNDCLASSW, WS_EX_NOACTIVATE,
    WM_APP, WM_CLIPBOARDUPDATE, WM_HOTKEY, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
};

/// 剪贴板浮窗窗口 label（App.vue 按此路由渲染 ClipboardOverlay）
pub const CLIPBOARD_WINDOW_LABEL: &str = "clipboard";
pub const CLIPBOARD_WIDTH: f64 = 520.0;
pub const CLIPBOARD_HEIGHT: f64 = 440.0;

/// 相同内容去重：内容一致的记录直接挪到最前，不新增重复条目
/// 自复制回声抑制窗口（粘贴/复制历史项后，事件监听会再次触发，用 hash 识别并跳过）
const ECHO_SUPPRESS_SECONDS: u64 = 10;
/// 富文本读取递增重试（Office 等分阶段写入剪贴板：先文本后 HTML）
const HTML_RETRY_DELAYS_MS: [u64; 7] = [0, 40, 80, 140, 220, 360, 560];
/// 剪贴板事件去抖：一次复制可能触发多次 WM_CLIPBOARDUPDATE（多格式逐步写入）
const SETTLE_MS: u64 = 80;
/// 剪贴板浮层「延迟回收」等待时间：收起后窗口先隐藏，超过该时长仍未再次唤起才销毁，
/// 期间再次唤起直接复用窗口（无 WebView2 冷启动），兼顾体感与内存回收。
const OVERLAY_RECYCLE_SECS: u64 = 30;

/// 跨命令共享的剪贴板浮层状态
pub struct ClipboardState {
    /// 唤起浮层前聚焦的窗口句柄（粘贴时先恢复焦点再注入 Ctrl+V）
    pub prev_focus: Mutex<Option<isize>>,
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self {
            prev_focus: Mutex::new(None),
        }
    }
}

/// 最近一次「我们自己写入」的剪贴板内容指纹（文本+HTML 哈希 + 时间戳），
/// 用于抑制监听回声：粘贴/复制历史项会再次触发剪贴板事件，不应重复入库。
static LAST_SELF_SET: Mutex<Option<(u64, std::time::Instant)>> = Mutex::new(None);

/// 全局鼠标钩子句柄（浮层可见时启用，点击浮层外部即收起）
static MOUSE_HOOK: Mutex<Option<isize>> = Mutex::new(None);

/// 浮层窗口句柄缓存：钩子回调运行在监听线程，禁止跨线程 Tauri 调用，
/// 显示浮层时写入，钩子回调只读它做矩形判断。
static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// 延迟回收代际计数：每次隐藏/销毁都会递增，延迟销毁任务启动时记下当前代际，
/// 到期后若代际已变（说明期间被再次唤起/重建），则不再执行销毁，避免误杀复用中的窗口。
static OVERLAY_RECYCLE_GEN: Mutex<u64> = Mutex::new(0);

/// 一次延迟窗口操作任务：粘贴后恢复焦点+注入按键，或收起后归还焦点。
/// 统一交给单一 worker 线程串行执行，避免频繁 spawn 短命线程造成线程数波动。
enum DelayedWinOp {
    /// 粘贴到唤起前窗口：先释放修饰键、必要时恢复目标窗口焦点，再按目标应用注入粘贴快捷键
    Paste {
        hwnd: isize,
        paste_method: &'static str,
    },
    /// 收起浮层后归还焦点给唤起前窗口（若前台仍在本进程）
    RestoreFocus { hwnd: isize },
}

/// 延迟窗口操作 worker：所有粘贴/归还焦点在同一个线程上串行执行
static WIN_OP_TX: Mutex<Option<std::sync::mpsc::Sender<DelayedWinOp>>> = Mutex::new(None);

/// 提交一次延迟窗口操作（粘贴或归还焦点）
fn submit_win_op(op: DelayedWinOp) {
    let tx = WIN_OP_TX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tx) = tx.as_ref() {
        let _ = tx.send(op);
    }
}

/// 初始化延迟窗口操作 worker（应用启动时调用一次）
pub fn init_win_op_worker() {
    let (tx, rx) = std::sync::mpsc::channel::<DelayedWinOp>();
    if let Ok(mut guard) = WIN_OP_TX.lock() {
        *guard = Some(tx);
    }
    std::thread::spawn(move || {
        while let Ok(op) = rx.recv() {
            match op {
                DelayedWinOp::Paste { hwnd, paste_method } => {
                    // 先给系统一点时间完成窗口隐藏与焦点转移
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    release_modifier_keys();
                    // 前台若仍是我们的窗口（浮层刚隐藏/被激活过），先恢复目标窗口焦点；
                    // 浮层从未抢焦点时（无激活显示）前台始终是目标应用，无需恢复。
                    let fg = unsafe { GetForegroundWindow() } as isize;
                    let own_pid = std::process::id();
                    let mut fg_pid: u32 = 0;
                    unsafe {
                        GetWindowThreadProcessId(fg as *mut core::ffi::c_void, &mut fg_pid);
                    }
                    log::info!(
                        "剪贴板粘贴：隐藏后前台 hwnd={:#x} pid={}，本进程 pid={}，目标 hwnd={:#x}",
                        fg,
                        fg_pid,
                        own_pid,
                        hwnd
                    );
                    if fg_pid == own_pid {
                        force_focus_window(hwnd as *mut core::ffi::c_void);
                        log::info!("剪贴板粘贴：前台仍在本进程，已恢复目标窗口焦点");
                    }
                    // 沉降：等焦点真正移交到目标窗口
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let fg_after = unsafe { GetForegroundWindow() } as isize;
                    let visible = unsafe { IsWindowVisible(hwnd as *mut core::ffi::c_void) } != 0;
                    log::info!(
                        "剪贴板粘贴：沉降后前台 hwnd={:#x}，目标可见={}",
                        fg_after,
                        visible
                    );
                    // 验证目标窗口仍可见（可能已关闭/最小化），避免输入注入到错误窗口
                    if !visible {
                        log::warn!("剪贴板粘贴：目标窗口不可见，跳过输入注入");
                        continue;
                    }
                    log::info!("剪贴板粘贴：采用粘贴方式 {:?}，注入按键", paste_method);
                    send_paste_keystroke(paste_method);
                }
                DelayedWinOp::RestoreFocus { hwnd } => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let fg = unsafe { GetForegroundWindow() } as isize;
                    let mut pid: u32 = 0;
                    unsafe {
                        GetWindowThreadProcessId(fg as *mut core::ffi::c_void, &mut pid);
                    }
                    if pid == std::process::id() {
                        force_focus_window(hwnd as *mut core::ffi::c_void);
                    }
                }
            }
        }
    });
}

fn content_hash(text: &str, html: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    html.hash(&mut h);
    h.finish()
}

fn is_self_set(text: &str, html: Option<&str>) -> bool {
    let Ok(guard) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((hash, at)) = guard.as_ref() else {
        return false;
    };
    at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS && *hash == content_hash(text, html)
}

/// 恢复记录时清除自复制指纹：暂停期间从浮窗复制/粘贴会写入指纹，
/// 恢复后短时间内复制相同内容会被误判为回声丢弃，这里重置为全新状态。
pub fn clear_self_set_fingerprint() {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = None;
    }
}

/// 剪贴板读取结果（优先级：图片 > 文件 > 文本）
pub enum ClipboardPayload {
    Text { content: String, html: Option<String> },
    Image { bytes: Vec<u8>, format: ImageFormat },
    Files { paths: Vec<String> },
}

/// 图片原始格式（落盘快照时的编码依据）
#[derive(Clone, Copy, PartialEq)]
pub enum ImageFormat {
    /// PNG 剪贴板格式（现代应用主流），原样存 .png
    Png,
    /// CF_DIB / CF_DIBV5 位图，包装 14 字节文件头存 .bmp
    Dib,
}

/// 图片/文件字节指纹：用于回声抑制与「相同内容只留一条」去重
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// 文件列表指纹：路径 + 各文件大小（同名不同内容也能区分），用于回声抑制
fn hash_files(paths: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in paths {
        p.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(p) {
            meta.len().hash(&mut h);
        }
    }
    h.finish()
}

/// 判断给定哈希是否为「我们自己写入」的回声（图片/文件路径）
fn is_self_set_hash(hash: u64) -> bool {
    let Ok(guard) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((h, at)) = guard.as_ref() else {
        return false;
    };
    at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS && *h == hash
}

/// 记录回声指纹（图片/文件写入后调用）
fn record_self_set_hash(hash: u64) {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = Some((hash, std::time::Instant::now()));
    }
}

// ---- 剪贴板读取（Windows） ----

fn read_utf16_null_terminated(ptr: *const u16, max_chars: usize) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    unsafe {
        while len < max_chars && *ptr.add(len) != 0 {
            len += 1;
        }
    }
    if len == 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf16_lossy(slice)
}

/// 从 CF_HTML 数据中提取实际片段（StartFragment/EndFragment 偏移优先，标签兜底）
fn extract_cf_html_fragment(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    let parse = |key: &str| -> Option<usize> {
        let i = s.find(key)?;
        let digits: String = s[i + key.len()..]
            .chars()
            .take(10)
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse::<usize>().ok()
    };
    if let (Some(a), Some(b)) = (parse("StartFragment:"), parse("EndFragment:")) {
        if b > a && b <= s.len() {
            return Some(s[a..b].to_string());
        }
    }
    let a = s.find("<!--StartFragment-->")?;
    let b = s.find("<!--EndFragment-->")?;
    let start = a + "<!--StartFragment-->".len();
    if b > start {
        Some(s[start..b].to_string())
    } else {
        None
    }
}

/// 读取剪贴板纯文本与可选 HTML 片段（失败返回 None）
pub fn read_clipboard() -> Option<(String, Option<String>)> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }

        let mut text: Option<String> = None;
        let handle = GetClipboardData(CF_UNICODETEXT as u32);
        if !handle.is_null() {
            let ptr = GlobalLock(handle);
            if !ptr.is_null() {
                let t = read_utf16_null_terminated(ptr as *const u16, 1_000_000);
                if !t.trim().is_empty() {
                    text = Some(t);
                }
                GlobalUnlock(handle);
            }
        }

        let html = read_html_fragment_unlocked();

        CloseClipboard();
        if text.is_none() && html.is_none() {
            None
        } else {
            Some((text.unwrap_or_default(), html))
        }
    }
}

/// 读取剪贴板 HTML 片段（调用方需已 OpenClipboard）
unsafe fn read_html_fragment_unlocked() -> Option<String> {
    let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
    if fmt == 0 {
        return None;
    }
    let h = GetClipboardData(fmt);
    if h.is_null() {
        return None;
    }
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 2_000_000 && *(ptr as *const u8).add(len) != 0 {
        len += 1;
    }
    let html = if len > 0 {
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
        extract_cf_html_fragment(bytes)
    } else {
        None
    };
    GlobalUnlock(h);
    html
}

/// 读取剪贴板：优先拿文本；仅当剪贴板已注册 HTML 格式（内容可能仍在分阶段写入）时，
/// 才按递增延迟重试，避免只存到纯文本丢失富文本格式。
/// 之前用 clipboard_format_count() > 1 判断「是否富文本」，但纯文本复制（记事本等）也常带
/// CF_TEXT/CF_OEMTEXT/CF_LOCALE 等多个格式，导致每次复制都空跑 ~1.4s 的重试、入库明显延迟。
pub fn read_clipboard_with_retry() -> Option<(String, Option<String>)> {
    let (text, html) = read_clipboard()?;
    if html.is_some() || text.trim().is_empty() {
        return Some((text, html));
    }
    let has_html_format = unsafe {
        let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
        fmt != 0 && IsClipboardFormatAvailable(fmt) != 0
    };
    if !has_html_format {
        return Some((text, html));
    }
    for &d in &HTML_RETRY_DELAYS_MS[1..] {
        if d > 0 {
            std::thread::sleep(std::time::Duration::from_millis(d));
        }
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let html = read_html_fragment_unlocked();
                CloseClipboard();
                if let Some(h) = html {
                    if !h.trim().is_empty() {
                        return Some((text, Some(h)));
                    }
                }
            }
        }
    }
    Some((text, html))
}

/// 读取剪贴板图片（调用方需已 OpenClipboard）：
/// 优先 PNG 格式（浏览器/微信/QQ/截图工具均放置），否则 CF_DIBV5 → CF_DIB。
/// 返回原始字节与格式（DIB 无文件头，落盘时补 BMP 头）。
unsafe fn read_image_unlocked() -> Option<(Vec<u8>, ImageFormat)> {
    let png_fmt = RegisterClipboardFormatW(windows_sys::core::w!("PNG"));
    if png_fmt != 0 {
        let h = GetClipboardData(png_fmt);
        if !h.is_null() {
            if let Some(bytes) = read_global_bytes(h) {
                return Some((bytes, ImageFormat::Png));
            }
        }
    }
    for fmt in [CF_DIBV5 as u32, CF_DIB as u32] {
        let h = GetClipboardData(fmt);
        if !h.is_null() {
            if let Some(bytes) = read_global_bytes(h) {
                return Some((bytes, ImageFormat::Dib));
            }
        }
    }
    None
}

/// 读取剪贴板文件列表（CF_HDROP，调用方需已 OpenClipboard）
unsafe fn read_files_unlocked() -> Option<Vec<String>> {
    let h = GetClipboardData(CF_HDROP as u32);
    if h.is_null() {
        return None;
    }
    let hdrop = h as HDROP;
    let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, std::ptr::null_mut(), 0);
    if count == 0 {
        return None;
    }
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        // 先取长度，再取内容（宽字符路径）
        let len = DragQueryFileW(hdrop, i, std::ptr::null_mut(), 0) as usize;
        if len == 0 {
            continue;
        }
        let mut buf = vec![0u16; len + 1];
        let got = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), (len + 1) as u32) as usize;
        if got > 0 {
            paths.push(String::from_utf16_lossy(&buf[..got]));
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// 读取 Global 内存块为字节（GlobalSize 取准确长度，调用方需已 OpenClipboard）
unsafe fn read_global_bytes(h: windows_sys::Win32::Foundation::HGLOBAL) -> Option<Vec<u8>> {
    let ptr = GlobalLock(h);
    if ptr.is_null() {
        return None;
    }
    let size = GlobalSize(h);
    let bytes = if size > 0 {
        std::slice::from_raw_parts(ptr as *const u8, size).to_vec()
    } else {
        Vec::new()
    };
    GlobalUnlock(h);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// 尝试读取图片（自带 Open/Close 剪贴板）
fn try_read_image() -> Option<(Vec<u8>, ImageFormat)> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let result = read_image_unlocked();
        CloseClipboard();
        result
    }
}

/// 尝试读取文件列表（自带 Open/Close 剪贴板）
fn try_read_files() -> Option<Vec<String>> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let result = read_files_unlocked();
        CloseClipboard();
        result
    }
}

/// 读取剪贴板并按类型分派：图片 > 文件 > 文本。
/// 文本复用 read_clipboard_with_retry（含 Office 等分阶段写 HTML 的重试）。
pub fn read_clipboard_payload() -> Option<ClipboardPayload> {
    if let Some((bytes, format)) = try_read_image() {
        return Some(ClipboardPayload::Image { bytes, format });
    }
    if let Some(paths) = try_read_files() {
        return Some(ClipboardPayload::Files { paths });
    }
    let (text, html) = read_clipboard_with_retry()?;
    Some(ClipboardPayload::Text { content: text, html })
}

// ---- 剪贴板写入 ----

/// 构造 CF_HTML 数据（含 StartHTML/EndHTML/StartFragment/EndFragment 偏移）
fn build_cf_html(fragment: &str) -> Vec<u8> {
    let body = format!(
        "<html><body><!--StartFragment-->{}</body>",
        fragment
    );
    // 头部四段偏移各占 10 位零填充数字：先用占位 0 求出真实头长，再据此算各偏移。
    // 之前硬编码 108 与实际头长（105 字节）不符，导致 StartFragment/EndFragment 整体错位，
    // 粘贴富文本时片段首尾错乱，且读回片段与写入片段不一致使回声抑制失效。
    let prefix_len = format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
        0usize, 0usize, 0usize, 0usize
    )
    .len();
    let start_frag = prefix_len + "<html><body><!--StartFragment-->".len();
    let end_frag = start_frag + fragment.len();
    let end_html = prefix_len + body.len();
    let header = format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",
        prefix_len, end_html, start_frag, end_frag
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// 写入剪贴板：纯文本 + 可选 HTML（粘贴回原处时优先还原富文本格式）。
/// 写入成功会记录内容指纹，抑制监听线程的回声重复入库。
pub fn set_clipboard(text: &str, html: Option<&str>) -> Result<(), String> {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }

        // CF_UNICODETEXT
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        buf.push(0);
        let mem = GlobalAlloc(GMEM_MOVEABLE, buf.len() * 2);
        if mem.is_null() {
            CloseClipboard();
            return Err("剪贴板内存分配失败".into());
        }
        let dst = GlobalLock(mem);
        if dst.is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("剪贴板内存锁定失败".into());
        }
        std::ptr::copy_nonoverlapping(buf.as_ptr(), dst as *mut u16, buf.len());
        GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT as u32, mem).is_null() {
            GlobalFree(mem);
            CloseClipboard();
            return Err("写入剪贴板失败".into());
        }

        // CF_HTML（可选，失败不阻塞纯文本写入）
        if let Some(h) = html {
            let fmt = RegisterClipboardFormatW(windows_sys::core::w!("HTML Format"));
            if fmt != 0 {
                let bytes = build_cf_html(h);
                let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len());
                if !mem.is_null() {
                    let dst = GlobalLock(mem);
                    if !dst.is_null() {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
                        GlobalUnlock(mem);
                        if SetClipboardData(fmt, mem).is_null() {
                            GlobalFree(mem);
                        }
                    } else {
                        GlobalFree(mem);
                    }
                }
            }
        }

        CloseClipboard();

        // 记录回声指纹（文本+HTML），供监听线程 10s 内跳过自身写入
        if let Ok(mut guard) = LAST_SELF_SET.lock() {
            *guard = Some((content_hash(text, html), std::time::Instant::now()));
        }
        Ok(())
    }
}

/// 把 CF_DIB 数据（BITMAPINFOHEADER + 调色板 + 像素，无文件头）包装成 BMP 文件字节。
/// 只补 14 字节 BITMAPFILEHEADER，不做像素重编码。
fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    if dib.len() < 40 {
        return dib.to_vec();
    }
    let bi_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let clr_used = if dib.len() >= 36 {
        u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize
    } else {
        0
    };
    let pal_entries = if clr_used != 0 {
        clr_used
    } else if bit_count <= 8 {
        1usize << bit_count
    } else {
        0
    };
    let off_bits = 14 + bi_size + pal_entries * 4;
    let file_size = 14 + dib.len();
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(off_bits as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

/// 把 BMP 文件字节剥掉 14 字节文件头，得到 CF_DIB 数据（写回剪贴板用）
fn bmp_to_dib(bmp: &[u8]) -> &[u8] {
    if bmp.len() > 14 && &bmp[0..2] == b"BM" {
        &bmp[14..]
    } else {
        bmp
    }
}

/// 剪贴板图片快照目录：`app_data_dir/clipboard/images/`
fn clipboard_images_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("clipboard").join("images");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 图片落盘快照：PNG 原样存 .png，DIB 补文件头存 .bmp。
/// 返回快照绝对路径（失败返回 None，由调用方决定是否降级为文本）。
fn save_image_snapshot(
    app: &AppHandle,
    bytes: &[u8],
    format: ImageFormat,
    hash: u64,
) -> Option<String> {
    let dir = clipboard_images_dir(app)?;
    let ext = match format {
        ImageFormat::Png => "png",
        ImageFormat::Dib => "bmp",
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{:016x}_{}.{}", hash, nanos, ext);
    let path = dir.join(name);
    let data = match format {
        ImageFormat::Png => bytes.to_vec(),
        ImageFormat::Dib => dib_to_bmp(bytes),
    };
    std::fs::write(&path, &data).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// 分配 Global 内存并写入字节、SetClipboardData（调用方需已 OpenClipboard 且 EmptyClipboard）
unsafe fn set_global_data(format: u32, data: &[u8]) -> Result<(), String> {
    let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data.len());
    if mem.is_null() {
        return Err("剪贴板内存分配失败".into());
    }
    let dst = GlobalLock(mem);
    if dst.is_null() {
        GlobalFree(mem);
        return Err("剪贴板内存锁定失败".into());
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), dst as *mut u8, data.len());
    GlobalUnlock(mem);
    if SetClipboardData(format, mem).is_null() {
        GlobalFree(mem);
        return Err("写入剪贴板失败".into());
    }
    Ok(())
}

/// 写入图片到剪贴板：BMP 快照写 CF_DIB，PNG 快照写 PNG 剪贴板格式。
/// 写回后记录回声指纹，抑制监听线程重复入库。
pub fn set_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片快照失败: {}", e))?;
    let is_bmp = bytes.len() > 2 && &bytes[0..2] == b"BM";
    let data: Vec<u8> = if is_bmp {
        bmp_to_dib(&bytes).to_vec()
    } else {
        bytes.clone()
    };
    // 回声指纹基于「写回剪贴板的实际字节」（PNG 原始字节 / 剥掉文件头的 DIB），
    // 与监听端读到的一致，确保粘贴后能被正确识别为自身写入回声。
    let hash = hash_bytes(&data);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }
        let result = if is_bmp {
            set_global_data(CF_DIB as u32, &data)
        } else {
            let fmt = RegisterClipboardFormatW(windows_sys::core::w!("PNG"));
            if fmt == 0 {
                Err("无法注册 PNG 格式".into())
            } else {
                set_global_data(fmt, &data)
            }
        };
        CloseClipboard();
        result?;
    }
    record_self_set_hash(hash);
    Ok(())
}

/// 构造 CF_HDROP 数据：DROPFILES 结构 + 宽字符路径表（每条以 null 结尾，末尾额外 null）
fn build_hdrop(paths: &[&str]) -> Vec<u8> {
    let header_size = std::mem::size_of::<DROPFILES>();
    let mut payload: Vec<u16> = Vec::new();
    for p in paths {
        payload.extend(p.encode_utf16());
        payload.push(0);
    }
    payload.push(0); // 列表结束标记

    let total = header_size + payload.len() * 2;
    let mut out = vec![0u8; total];
    let df = DROPFILES {
        pFiles: header_size as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: 0,
        fWide: 1,
    };
    let df_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(&df as *const DROPFILES as *const u8, header_size)
    };
    out[..header_size].copy_from_slice(df_bytes);
    for (i, u) in payload.iter().enumerate() {
        let b = u.to_le_bytes();
        out[header_size + i * 2..header_size + i * 2 + 2].copy_from_slice(&b);
    }
    out
}

/// 写入文件列表到剪贴板（CF_HDROP）：只引用原路径，不拷贝文件内容。
/// 已不存在的路径会被剔除；全部失效则报错提示用户。
pub fn set_clipboard_files(paths: &[String]) -> Result<(), String> {
    let existing: Vec<&str> = paths
        .iter()
        .map(|s| s.as_str())
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    if existing.is_empty() {
        return Err("文件已不存在，无法粘贴".into());
    }
    let owned: Vec<String> = existing.iter().map(|s| s.to_string()).collect();
    let hash = hash_files(&owned);
    let data = build_hdrop(&existing);
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开系统剪贴板".into());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("无法清空系统剪贴板".into());
        }
        let result = set_global_data(CF_HDROP as u32, &data);
        CloseClipboard();
        result?;
    }
    record_self_set_hash(hash);
    Ok(())
}

// ---- 粘贴注入（恢复焦点 + 发送粘贴快捷键） ----

/// 发送一次按键（基于扫描码，比虚拟键码更兼容各类应用/终端）
unsafe fn send_key(scan: u16, flags: u32) {
    let mut input: INPUT = std::mem::zeroed();
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: 0,
        wScan: scan,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
    };
    SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
}

/// 发送粘贴快捷键，method 支持：
/// - ctrl_v：Ctrl+V（常规应用）
/// - ctrl_shift_v：Ctrl+Shift+V（部分终端/命令行只支持无格式粘贴）
/// - shift_insert：Shift+Insert（终端通用粘贴）
fn send_paste_keystroke(method: &str) {
    unsafe {
        let ctrl = MapVirtualKeyW(VK_CONTROL as u32, MAPVK_VK_TO_VSC) as u16;
        let shift = MapVirtualKeyW(VK_SHIFT as u32, MAPVK_VK_TO_VSC) as u16;
        let v = MapVirtualKeyW('V' as u32, MAPVK_VK_TO_VSC) as u16;
        let insert = MapVirtualKeyW(VK_INSERT as u32, MAPVK_VK_TO_VSC) as u16;
        match method {
            "ctrl_shift_v" => {
                send_key(ctrl, KEYEVENTF_SCANCODE);
                send_key(shift, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(shift, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(ctrl, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
            "shift_insert" => {
                send_key(shift, KEYEVENTF_SCANCODE);
                send_key(insert, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY);
                send_key(insert, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
                send_key(shift, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
            _ => {
                // ctrl_v
                send_key(ctrl, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE);
                send_key(v, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
                send_key(ctrl, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
            }
        }
    }
}

/// 释放所有修饰键（Ctrl/Shift/Alt/Win 的左右键），
/// 避免用户还按着 Shift 时 Ctrl+V 变成 Ctrl+Shift+V 无格式粘贴。
fn release_modifier_keys() {
    unsafe {
        let keys = [
            VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU,
            VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN,
        ];
        for k in keys {
            keybd_event(k as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

/// 强行把目标窗口带到前台并聚焦（AttachThreadInput 绕开前台锁定 + 还原最小化窗口）。
fn force_focus_window(hwnd: *mut core::ffi::c_void) {
    unsafe {
        if hwnd.is_null() || IsWindowVisible(hwnd) == 0 {
            return;
        }
        let should_restore = IsIconic(hwnd) != 0;
        let fg = GetForegroundWindow();
        if fg as isize != hwnd as isize {
            let mut fg_pid: u32 = 0;
            let mut target_pid: u32 = 0;
            let fg_tid = GetWindowThreadProcessId(fg, &mut fg_pid);
            let target_tid = GetWindowThreadProcessId(hwnd, &mut target_pid);
            if fg_tid != 0 && target_tid != 0 && fg_tid != target_tid {
                AttachThreadInput(fg_tid, target_tid, 1);
                SetForegroundWindow(hwnd);
                if should_restore {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                BringWindowToTop(hwnd);
                AttachThreadInput(fg_tid, target_tid, 0);
            } else {
                SetForegroundWindow(hwnd);
                if should_restore {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                BringWindowToTop(hwnd);
            }
        }
    }
}

/// 前台进程名（给定窗口句柄），用于识别终端等特殊粘贴目标
fn process_name_of_hwnd(hwnd: isize) -> Option<String> {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd as *mut core::ffi::c_void, &mut pid) };
    if pid == 0 {
        return None;
    }
    process_name_of_pid(pid)
}

/// 已知终端/命令行窗口进程名（这类窗口通常不支持 Ctrl+V，需要 Ctrl+Shift+V 或 Shift+Insert）
const TERMINAL_NAMES: [&str; 24] = [
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "openconsole.exe",
    "windowsterminal.exe",
    "wt.exe",
    "wezterm.exe",
    "alacritty.exe",
    "kitty.exe",
    "mintty.exe",
    "hyper.exe",
    "conemu64.exe",
    "cmder.exe",
    "tabby.exe",
    "warp.exe",
    "ghostty.exe",
    "mobaxterm.exe",
    "tabs.exe",
    "terminus.exe",
    "fluentterminal.exe",
    "ssh.exe",
    "sshd.exe",
    "windows terminal",
];

fn is_terminal_name(name: &str) -> bool {
    TERMINAL_NAMES
        .iter()
        .any(|t| name.eq_ignore_ascii_case(t))
}

/// 解析最终粘贴方式：auto 时按目标窗口是否终端自动选择 Ctrl+Shift+V / Ctrl+V
fn resolve_paste_method(cfg: &str, target_hwnd: isize) -> &'static str {
    match cfg {
        "ctrl_shift_v" => "ctrl_shift_v",
        "shift_insert" => "shift_insert",
        "ctrl_v" => "ctrl_v",
        _ => {
            let name = process_name_of_hwnd(target_hwnd).unwrap_or_default();
            if is_terminal_name(&name) {
                "ctrl_shift_v"
            } else {
                "ctrl_v"
            }
        }
    }
}

/// 判断唤起浮层前聚焦的窗口是否为本应用主窗口：
/// 主窗口输入框在 WebView2 内，浮层抢焦点会让其失去 DOM 焦点，
/// 恢复焦点 + 注入 Ctrl+V 依赖窗口激活/焦点时序，实际不可靠，
/// 改为直接向主窗口 JS 派发内容、由其插回原输入框。
fn is_main_window(app: &AppHandle, prev_hwnd: isize) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize == prev_hwnd)
        .unwrap_or(false)
}

/// 粘贴到唤起浮层前聚焦的窗口：
/// - 本应用主窗口：隐藏浮层后直接向主窗口 JS 派发内容，由其插回原输入框（WebView2 焦点时序不可靠）
/// - 外部窗口：隐藏浮层 →（若前台被我们抢占则恢复目标窗口焦点）→ 按目标应用发送粘贴快捷键
pub fn paste_to_previous_window(app: &AppHandle, content: &str, html: Option<&str>) {
    unregister_esc_hotkey();
    uninstall_mouse_hook();
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        hide_overlay_window(&win);
    }
    let prev = app
        .state::<ClipboardState>()
        .prev_focus
        .lock()
        .map(|g| *g)
        .unwrap_or(None);
    let Some(hwnd) = prev else {
        log::warn!("剪贴板粘贴：未记录到唤起前窗口，仅写入剪贴板");
        return;
    };
    log::info!("剪贴板粘贴：唤起前窗口 hwnd={:#x}", hwnd);

    // 本应用主窗口 + 文本内容：直接派发内容给主窗口 JS 插入（浮层已隐藏，无需恢复焦点）。
    // 图片/文件（content 为空）不在此分支——走下方 Ctrl+V 注入，目标应用读剪贴板对应格式。
    if !content.is_empty() && is_main_window(app, hwnd) {
        let payload = serde_json::json!({
            "content": content,
            "html": html,
        });
        if let Err(e) = app.emit_to("main", "clipboard-paste-request", payload) {
            log::warn!("剪贴板粘贴：向主窗口派发插入请求失败: {}", e);
        }
        return;
    }

    // 提交到常驻 worker 串行执行（粘贴时序依赖窗口隐藏/焦点转移，延迟需按序进行）
    let paste_method = resolve_paste_method(&crate::config::load().clipboard_paste_method, hwnd);
    submit_win_op(DelayedWinOp::Paste { hwnd, paste_method });
}

// ---- 浮层窗口 ----

/// 剪贴板监听消息窗口句柄（Esc 兜底热键注册到该窗口，由监听线程消息循环分发 WM_HOTKEY）
static LISTENER_HWND: Mutex<Option<isize>> = Mutex::new(None);
/// Esc 兜底关闭热键 ID
const ESC_HOTKEY_ID: i32 = 0x5C43;

/// 低层鼠标钩子回调：浮层可见时，点击落在浮层窗口矩形之外 → 收起浮层。
/// 浮层以无激活方式显示、不持有焦点，靠此钩子感知「点击外部」（含点击本应用主窗口）。
/// 钩子回调运行在安装钩子的线程上，这里只做矩形判断，随后向监听消息窗口
/// PostMessage 一个自定义消息，由监听线程统一收口处理（避免在钩子回调里直接做窗口操作）。
const WM_XHUB_CLIPBOARD_DISMISS: u32 = WM_APP + 0x5C01;
/// 监听线程专用消息：安装/卸载低层鼠标钩子（钩子必须由拥有消息循环的线程安装）
const WM_XHUB_INSTALL_MOUSE_HOOK: u32 = WM_APP + 0x5C02;
const WM_XHUB_UNINSTALL_MOUSE_HOOK: u32 = WM_APP + 0x5C03;

/// 低层鼠标钩子回调：浮层可见时，点击落在浮层窗口矩形之外 → 收起浮层。
/// 回调运行在「安装钩子的线程」上，这里只做纯 Win32 矩形判断（禁止任何 Tauri 调用，
/// 否则会经 send_user_message 阻塞并拖垮该线程消息循环），随后向监听消息窗口
/// PostMessage 一个自定义消息，由监听线程统一收口处理。
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let msg = wparam as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
            let clicked_outside = {
                let Some(hwnd) = *OVERLAY_HWND.lock().unwrap_or_else(|e| e.into_inner()) else {
                    return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                };
                if IsWindowVisible(hwnd as *mut core::ffi::c_void) == 0 {
                    return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                }
                let st = lparam as *const MSLLHOOKSTRUCT;
                let pt = (*st).pt;
                let mut rc: RECT = std::mem::zeroed();
                GetWindowRect(hwnd as *mut core::ffi::c_void, &mut rc);
                // 5px 边距兜底，贴近边缘的点击不算「外部」
                pt.x < rc.left - 5 || pt.x > rc.right + 5 || pt.y < rc.top - 5 || pt.y > rc.bottom + 5
            };
            if clicked_outside {
                if let Some(hwnd) = *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
                    let _ = PostMessageW(
                        hwnd as *mut core::ffi::c_void,
                        WM_XHUB_CLIPBOARD_DISMISS,
                        0,
                        0,
                    );
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// 请求监听线程安装低层鼠标钩子（浮层可见时启用，隐藏时关闭）。
/// 钩子必须由监听线程（拥有 GetMessageW 消息循环）安装，才能由该线程驱动回调，
/// 避免挂在全局快捷键线程上导致回调阻塞拖死 Esc 链路。
fn install_mouse_hook() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        PostMessageW(hwnd as *mut core::ffi::c_void, WM_XHUB_INSTALL_MOUSE_HOOK, 0, 0);
    }
}

/// 请求监听线程卸载低层鼠标钩子（浮层隐藏时调用）
fn uninstall_mouse_hook() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        PostMessageW(hwnd as *mut core::ffi::c_void, WM_XHUB_UNINSTALL_MOUSE_HOOK, 0, 0);
    }
}

/// 监听线程实际安装低层鼠标钩子（由消息循环调用，保证钩子挂在本线程）
fn install_mouse_hook_impl() {
    let Ok(mut guard) = MOUSE_HOOK.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let hmod = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0) };
    if hook.is_null() {
        log::warn!("剪贴板浮层：注册低层鼠标钩子失败");
        return;
    }
    *guard = Some(hook as isize);
    log::info!("剪贴板浮层：低层鼠标钩子已启用（监听线程）");
}

/// 监听线程实际卸载低层鼠标钩子（由消息循环调用）
fn uninstall_mouse_hook_impl() {
    let Ok(mut guard) = MOUSE_HOOK.lock() else {
        return;
    };
    if let Some(hook) = guard.take() {
        unsafe {
            UnhookWindowsHookEx(hook as *mut core::ffi::c_void);
        }
    }
}

/// 注册「浮层可见期间的 Esc 兜底关闭」全局热键：
/// 浮层以无激活方式显示时自身收不到键盘事件，靠 RegisterHotKey 拦截 Esc，
/// 由剪贴板监听线程把 WM_HOTKEY 转成关闭动作（隐藏后自动注销，避免吞掉全局 Esc）。
fn register_esc_hotkey() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    let ok = unsafe {
        RegisterHotKey(hwnd as *mut core::ffi::c_void, ESC_HOTKEY_ID, 0, VK_ESCAPE as u32) != 0
    };
    if !ok {
        log::warn!("剪贴板浮层：注册 Esc 兜底热键失败（可能被其他程序占用）");
    }
}

/// 注销 Esc 兜底热键（浮层隐藏时调用）
fn unregister_esc_hotkey() {
    let hwnd = match *LISTENER_HWND.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        UnregisterHotKey(hwnd as *mut core::ffi::c_void, ESC_HOTKEY_ID);
    }
}

/// 唤起/收起剪贴板浮层（全局快捷键触发）：
/// 唤起前记录当前前台窗口（粘贴还原目标），浮层为独立置顶小窗。
/// 浮层以「无激活」方式显示（不抢走当前输入框焦点），用户点击搜索框时才激活。
pub fn toggle_overlay(app: &AppHandle) {
    // 已存在且可见：收起走统一 hide_overlay（注销热键/钩子 + 归还唤起前窗口焦点）。
    // 不要在此覆盖 prev_focus——浮层被激活后前台是浮层自身，覆盖会把还原目标错写成浮层。
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        if win.is_visible().unwrap_or(false) {
            hide_overlay(app);
            return;
        }
    }

    // 显示（含首次创建）：记录唤起前的前台窗口，供关闭/粘贴时还原焦点
    let prev = unsafe { GetForegroundWindow() } as isize;
    if let Ok(mut guard) = app.state::<ClipboardState>().prev_focus.lock() {
        *guard = Some(prev);
    }

    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        // 取消任何挂起的延迟回收：该窗口即将被重新显示，不能让回收任务销毁它
        bump_overlay_recycle_gen();
        // 每次唤起都重新定位到鼠标附近（窗口可能被拖走过、或显示器布局变化）
        if let Some((px, py)) = cursor_anchor_position() {
            let _ = win
                .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(px, py)));
        }
        show_overlay_no_activate(&win);
        register_esc_hotkey();
        install_mouse_hook();
        let _ = app.emit_to(CLIPBOARD_WINDOW_LABEL, "clipboard-shown", ());
        return;
    }

    let (px, py) = cursor_anchor_position().unwrap_or((200, 200));
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        CLIPBOARD_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("剪贴板历史")
    .inner_size(CLIPBOARD_WIDTH, CLIPBOARD_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .background_color(tauri::window::Color(0, 0, 0, 0))
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 透明窗口在 Windows 上启用系统阴影会把边缘渲染成黑色描边（黑边），
    // 面板自带 CSS 阴影，OS 层阴影关闭即可（与便签浮窗一致）
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    match builder.build() {
        Ok(win) => {
            // 创建后先按物理像素定位（HiDPI 安全）再显示，避免闪现默认位置
            let _ = win
                .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(px, py)));
            show_overlay_no_activate(&win);
            register_esc_hotkey();
            install_mouse_hook();
            let _ = app.emit_to(CLIPBOARD_WINDOW_LABEL, "clipboard-shown", ());
            log::info!("剪贴板浮层窗口已创建");
        }
        Err(e) => log::warn!("剪贴板浮层窗口创建失败: {}", e),
    }
}

/// 无激活显示浮层：附加 WS_EX_NOACTIVATE 后以 SW_SHOWNA 显示并置顶，
/// 不抢占前台焦点（原输入框保持焦点，外部应用可直接粘贴）。
fn show_overlay_no_activate(win: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            if let Ok(mut guard) = OVERLAY_HWND.lock() {
                *guard = Some(hwnd.0 as isize);
            }
            unsafe {
                let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE as isize);
                ShowWindow(hwnd.0, SW_SHOWNA);
                SetWindowPos(
                    hwnd.0,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
            }
            return;
        }
        let _ = win.show();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = win.show();
}

/// 激活浮层：清除 WS_EX_NOACTIVATE 并强制前台（用户点击搜索框开始键盘操作时调用）
pub fn activate_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        #[cfg(target_os = "windows")]
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE as isize));
            }
            let _ = win.set_focus();
            force_focus_window(hwnd.0);
        } else {
            let _ = win.set_focus();
        }
        #[cfg(not(target_os = "windows"))]
        let _ = win.set_focus();
    }
}

/// 收起浮层：先立即隐藏（视觉立刻消失），再启动「延迟回收」——等待 OVERLAY_RECYCLE_SECS
/// 后若期间未被再次唤起（代际未变）才真正 destroy 释放 WebView2 renderer 内存。
/// 纯隐藏时 renderer 常驻（约 60–120MB），立即销毁则每次唤起都要 WebView2 冷启动（数百 ms），
/// 延迟回收在两者之间取平衡：短时间连续唤起零冷启动，长期不用才回收内存。
fn hide_overlay_window(win: &tauri::WebviewWindow) {
    // 先清掉浮层窗口句柄缓存，避免低层鼠标钩子残留句柄对已隐藏窗口做矩形判断
    if let Ok(mut guard) = OVERLAY_HWND.lock() {
        *guard = None;
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                ShowWindow(hwnd.0, SW_HIDE);
            }
            schedule_overlay_recycle(win);
            return;
        }
        let _ = win.hide();
        schedule_overlay_recycle(win);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.hide();
        schedule_overlay_recycle(win);
    }
}

/// 记录一次「收起/唤起」代际变更，使任何已排期的延迟回收作废。
/// 在 toggle_overlay 重新显示浮层时调用，避免延迟任务误销毁复用中的窗口。
fn bump_overlay_recycle_gen() {
    if let Ok(mut guard) = OVERLAY_RECYCLE_GEN.lock() {
        *guard = guard.wrapping_add(1);
    }
}

/// 排期延迟回收：等待 OVERLAY_RECYCLE_SECS 后，若代际未变且窗口仍处于隐藏态，
/// 则销毁该窗口释放 WebView2 renderer。期间被再次唤起（代际已变）则不执行。
fn schedule_overlay_recycle(win: &tauri::WebviewWindow) {
    let app = win.app_handle().clone();
    let label = win.label().to_string();
    let gen = *OVERLAY_RECYCLE_GEN.lock().unwrap_or_else(|e| e.into_inner());
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(OVERLAY_RECYCLE_SECS));
        let current = *OVERLAY_RECYCLE_GEN.lock().unwrap_or_else(|e| e.into_inner());
        if current != gen {
            return; // 期间被再次唤起/重建，作废本次回收
        }
        let Some(win) = app.get_webview_window(&label) else {
            return;
        };
        if !win.is_visible().unwrap_or(false) {
            log::info!("剪贴板浮层：延迟回收销毁（{}s 未再唤起）", OVERLAY_RECYCLE_SECS);
            let _ = win.destroy();
        }
    });
}

/// 收起浮层：隐藏后若前台仍在本应用（浮层/主窗口），把焦点还给唤起前的窗口
pub fn hide_overlay(app: &AppHandle) {
    unregister_esc_hotkey();
    uninstall_mouse_hook();
    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        return;
    };
    let was_visible = win.is_visible().unwrap_or(false);
    hide_overlay_window(&win);
    if !was_visible {
        return;
    }
    let prev = app
        .state::<ClipboardState>()
        .prev_focus
        .lock()
        .map(|g| *g)
        .unwrap_or(None);
    let Some(hwnd) = prev else {
        return;
    };
    // 提交到常驻 worker 串行执行：延迟 100ms 后归还焦点
    submit_win_op(DelayedWinOp::RestoreFocus { hwnd });
}

/// 计算浮层初始位置：光标附近（工作区范围内），避免覆盖点击来源
/// 返回物理像素坐标（GetCursorPos/MonitorFromPoint 均为物理像素），
/// 放置窗口时使用 Position::Physical，避免 HiDPI 缩放下坐标错位。
fn cursor_anchor_position() -> Option<(i32, i32)> {
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let rc: RECT = info.rcWork;
        let w = CLIPBOARD_WIDTH as i32;
        let h = CLIPBOARD_HEIGHT as i32;
        let x = (pt.x - w / 2).clamp(rc.left, rc.right - w);
        let y = (pt.y - 30).clamp(rc.top, rc.bottom - h);
        Some((x, y))
    }
}

// ---- 事件驱动监听（AddClipboardFormatListener + WM_CLIPBOARDUPDATE） ----

/// 监听消息窗口类名（进程内唯一）
const LISTENER_CLASS: *const u16 = windows_sys::core::w!("XHubClipboardListenerWnd");

/// 消息窗口过程：事件由 GetMessageW 循环按消息类型分发，这里走默认处理
unsafe extern "system" fn listener_wndproc(
    hwnd: *mut core::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 取前台窗口所属进程名（作为历史来源应用，随剪贴板变化时读取）
fn foreground_app_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        process_name_of_pid(pid)
    }
}

/// 进程名缓存容量上限：常驻托盘应用长期运行会累积大量 PID 条目（每个前台进程复制都新增一条），
/// 超过上限时清空重建，避免缓存无限增长（缓慢泄漏）与 PID 复用后返回陈旧进程名。
const MAX_PROCESS_CACHE: usize = 256;

/// 按 PID 取进程名（带缓存）。`remove_dead_processes=false` 避免每次刷新都全量枚举所有进程，
/// 否则一次复制会卡数百毫秒甚至数秒，拖慢入库并阻塞监听线程消息循环。
/// 同一前台应用连续复制时 PID 不变，缓存命中后跳过 sysinfo 查询，进一步降低入库延迟。
fn process_name_of_pid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<u32, String>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(cached) = cache.lock() {
        if let Some(name) = cached.get(&pid) {
            return Some(name.clone());
        }
    }
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        false,
    );
    let name = sys
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().to_string());
    if let Some(name) = &name {
        if let Ok(mut cached) = cache.lock() {
            if cached.len() >= MAX_PROCESS_CACHE {
                cached.clear();
            }
            cached.insert(pid, name.clone());
        }
    }
    name
}

/// 入库辅助：拿数据库连接并执行给定 repo 操作（监听线程跨线程访问 DbState）
fn insert_into_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.try_state::<DbState>().ok_or("剪贴板状态未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

/// 处理一次剪贴板变化事件：读取（含 HTML 重试）→ 回声抑制 → 入库。
/// 由独立 worker 线程调用（去抖已在 worker 完成），这里不做额外 sleep。
fn handle_clipboard_update(app: &AppHandle) {
    // 暂停记录时跳过（暂停前已入库的数据保留）
    let cfg = crate::config::load();
    if cfg.clipboard_paused {
        return;
    }

    let payload = read_clipboard_payload().or_else(|| {
        // 读取失败（复制方仍占用剪贴板）：稍等重试同一条，避免内容被锁定时静默丢失
        std::thread::sleep(std::time::Duration::from_millis(120));
        read_clipboard_payload()
    });
    let Some(payload) = payload else {
        return;
    };

    let source = foreground_app_name();
    let result: Result<(), String> = match payload {
        ClipboardPayload::Text { content, html } => {
            if content.trim().is_empty() {
                return;
            }
            // 自复制回声：粘贴/复制历史项后系统会再次触发事件，跳过自身写入
            if is_self_set(&content, html.as_deref()) {
                log::debug!("剪贴板监听：忽略自身文本写入回声");
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert(conn, &content, html.as_deref(), source.as_deref())
                    .map_err(|e| e.to_string())
            })
        }
        ClipboardPayload::Image { bytes, format } => {
            if !cfg.clipboard_image_enabled {
                return;
            }
            let hash = hash_bytes(&bytes);
            if is_self_set_hash(hash) {
                log::debug!("剪贴板监听：忽略自身图片写入回声");
                return;
            }
            let Some(path) = save_image_snapshot(app, &bytes, format, hash) else {
                log::warn!("剪贴板图片快照落盘失败，已跳过");
                return;
            };
            let dedup_key = format!("{:016x}", hash);
            match insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_image(conn, &dedup_key, &path, source.as_deref())
                    .map_err(|e| e.to_string())
            }) {
                Ok(true) => Ok(()),
                Ok(false) => {
                    // 相同图片已存在（去重挪到最前），删除本次落盘的冗余快照
                    let _ = std::fs::remove_file(&path);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        ClipboardPayload::Files { paths } => {
            if !cfg.clipboard_file_enabled {
                return;
            }
            let hash = hash_files(&paths);
            if is_self_set_hash(hash) {
                log::debug!("剪贴板监听：忽略自身文件写入回声");
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_files(conn, &paths, source.as_deref())
                    .map_err(|e| e.to_string())
            })
        }
    };

    if let Err(e) = result {
        log::warn!("剪贴板历史入库失败: {}", e);
    }
}

/// 启动剪贴板事件驱动监听线程（Q8：启动零加载，不碰历史数据；仅在剪贴板变化时落库）。
/// 取代原来的 500ms 轮询：AddClipboardFormatListener 注册消息窗口，
/// 剪贴板一变化即收到 WM_CLIPBOARDUPDATE，实时且无轮询开销。
pub fn start_monitor(app: AppHandle) {
    // 剪贴板内容读取 + 入库放到独立 worker 线程：去抖沉降后只处理最新状态。
    // 监听线程的消息循环必须保持空闲，才能及时处理 Esc 热键、外部点击收起
    // 与鼠标钩子装卸消息；若在循环里同步读剪贴板，一旦复制方短暂占用剪贴板
    // （重试 + sysinfo 查询），整条消息队列都会被拖住，导致浮层关不掉。
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let worker_app = app.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // 一次复制可能触发多次 WM_CLIPBOARDUPDATE（多格式逐步写入），
            // 沉降期内持续吞掉新事件，只保留最后一次再处理。
            std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
            while rx.try_recv().is_ok() {}
            handle_clipboard_update(&worker_app);
        }
    });

    std::thread::spawn(move || {
        log::info!("剪贴板监听线程启动（事件驱动 WM_CLIPBOARDUPDATE）");
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(listener_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: LISTENER_CLASS,
            };
            if RegisterClassW(&class) == 0 {
                let err = std::io::Error::last_os_error();
                log::warn!("注册剪贴板监听窗口类失败: {}", err);
            }
            let hwnd = CreateWindowExW(
                0,
                LISTENER_CLASS,
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                log::warn!("创建剪贴板监听消息窗口失败");
                return;
            }
            if AddClipboardFormatListener(hwnd) == 0 {
                log::warn!("AddClipboardFormatListener 注册失败");
                DestroyWindow(hwnd);
                return;
            }
            log::info!("剪贴板格式监听已注册");
            if let Ok(mut guard) = LISTENER_HWND.lock() {
                *guard = Some(hwnd as isize);
            }

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                if msg.message == WM_CLIPBOARDUPDATE {
                    let _ = tx.send(());
                } else if msg.message == WM_HOTKEY && msg.wParam as i32 == ESC_HOTKEY_ID {
                    // Esc 兜底关闭：浮层以无激活方式显示时自身收不到键盘事件，
                    // 热键把 Esc 转发到这里，若浮层仍可见则收起
                    if app
                        .get_webview_window(CLIPBOARD_WINDOW_LABEL)
                        .map(|w| w.is_visible().unwrap_or(false))
                        .unwrap_or(false)
                    {
                        hide_overlay(&app);
                    }
                } else if msg.message == WM_XHUB_INSTALL_MOUSE_HOOK {
                    install_mouse_hook_impl();
                } else if msg.message == WM_XHUB_UNINSTALL_MOUSE_HOOK {
                    uninstall_mouse_hook_impl();
                } else if msg.message == WM_XHUB_CLIPBOARD_DISMISS {
                    // 低层鼠标钩子：点击落在浮层窗口矩形之外 → 收起
                    hide_overlay(&app);
                } else {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if let Ok(mut guard) = LISTENER_HWND.lock() {
                *guard = None;
            }
            RemoveClipboardFormatListener(hwnd);
            DestroyWindow(hwnd);
            UnregisterClassW(LISTENER_CLASS, hinstance);
        }
    });
}
