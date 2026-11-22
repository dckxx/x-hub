//! 全局搜索浮窗（label `search`）：热键直达的独立小窗，**不唤起主窗**。
//!
//! ## 为什么要有这个模块
//!
//! 搜索 UI（GlobalSearch.vue）原先挂在主窗的遮罩层里，于是「显示搜索框」在结构上
//! 等价于「显示主窗」——旧实现 `lib.rs` 收到 `search-shortcut` 先 `tray::show_window`
//! 再由主窗前端把弹窗打开，用户看到的就是「先弹软件、再弹搜索框」。
//! 挪进独立窗后热键只动这一个窗口，主窗隐藏就继续隐藏。
//!
//! ## 生命周期：按需建窗 + 建好后隐藏常驻
//!
//! 与 `chat_window.rs` 同款姿势（**必须**照抄，原因见那里的事故注释）：首次唤起时
//! `BUILDING` 单飞 + `std::thread::spawn` 建窗——在主线程直接 build WebView2 会在
//! 当前调用栈里跑 controller 创建，和 IPC/菜单回调嵌套时把消息泵卡死。建好后绝不销毁，
//! 关闭只 hide，下次唤起是 ShowWindow 级快操作。
//!
//! 建窗后必须等页面 `PageLoadEvent::Finished` 再 present（口径同
//! `recorder::ensure_pin_window`）：调用方紧接着 emit `search-window-shown`，
//! 页面监听还没注册就发会丢事件，表现为「窗口出来了但输入框没聚焦」。
//!
//! ## 结果路由（本模块的核心收益）
//!
//! 搜索结果选中后**不再无脑弹主窗**：程序/文件/系统浏览器打开的资源由后端直接启动，
//! 主窗可以一直藏着；只有「结构上离不开主窗」的三种才亮主窗——软件内面板模式的网页
//! （`suda-panel` 是主窗的子 webview）、速记、待办。

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow};

use crate::commands::DbState;
use crate::models::ResourceKind;

/// 搜索浮窗 label（前端 SearchWindow.vue / capabilities / webview_mem 三处对齐）
pub const LABEL: &str = "search";

/// 窗口物理尺寸（逻辑 px）：卡片本身 560 宽，四周留 ~20px 给 CSS 落影，
/// 否则阴影会被窗口边界切掉（透明窗的常见破相）。
const WIDTH: f64 = 600.0;
const HEIGHT: f64 = 520.0;

/// 防连点重复建窗（口径同 chat_window::BUILDING）
static BUILDING: AtomicBool = AtomicBool::new(false);

/// 窗口原生可见性：`is_visible()` 在 Windows 上对 WebView2 子窗口判定不准
/// （隐藏后仍可能报 true，见 tray.rs / clipboard.rs 同款注释），会让 toggle 误入
/// 「收起」分支。直查 Win32 IsWindowVisible。
#[cfg(target_os = "windows")]
fn native_visible(win: &WebviewWindow) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    match win.hwnd() {
        Ok(hwnd) => unsafe { IsWindowVisible(hwnd.0) != 0 },
        Err(_) => win.is_visible().unwrap_or(false),
    }
}

#[cfg(not(target_os = "windows"))]
fn native_visible(win: &WebviewWindow) -> bool {
    win.is_visible().unwrap_or(false)
}

/// 纯几何：在给定工作区矩形内算出「水平 + 垂直双居中」的窗口左上角（物理 px）。
///
/// 夹紧到工作区内，保证副屏 / 任务栏在顶部的布局也不会弹到屏幕外。
pub fn centered_position(work: (i32, i32, i32, i32), w: i32, h: i32) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let ww = (right - left).max(1);
    let wh = (bottom - top).max(1);
    let x = left + (ww - w) / 2;
    let y = top + (wh - h) / 2;
    // 下限优先：工作区比窗口还小时，left/top 才是唯一合法落点
    (
        x.clamp(left, (right - w).max(left)),
        y.clamp(top, (bottom - h).max(top)),
    )
}

/// 当前鼠标所在显示器的工作区（物理 px）+ 该点的坐标。
/// 返回 None 表示取不到光标/显示器信息，调用方回退主显示器。
#[cfg(target_os = "windows")]
fn cursor_work_area() -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
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
        let rc = info.rcWork;
        Some((rc.left, rc.top, rc.right, rc.bottom))
    }
}

#[cfg(not(target_os = "windows"))]
fn cursor_work_area() -> Option<(i32, i32, i32, i32)> {
    None
}

/// 主显示器工作区兜底（取不到光标所在屏时用）
#[cfg(target_os = "windows")]
fn primary_work_area() -> Option<(i32, i32, i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    unsafe {
        let pt = POINT { x: 0, y: 0 };
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let rc = info.rcWork;
        Some((rc.left, rc.top, rc.right, rc.bottom))
    }
}

#[cfg(not(target_os = "windows"))]
fn primary_work_area() -> Option<(i32, i32, i32, i32)> {
    None
}

/// 建窗（visible=false；显隐由 show_window/hide_window 负责）
fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // 等页面加载完成的通道：见模块注释「建窗后必须等 PageLoadEvent::Finished」
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let tx = std::sync::Mutex::new(Some(tx));

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        LABEL,
        // 轻量入口 search.html：只挂 SearchWindow，不加载完整 SPA（见 src/light/search.ts）
        WebviewUrl::App("search.html".into()),
    )
    .title("全局搜索")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
    .on_page_load(move |_win, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            if let Ok(mut guard) = tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
            }
        }
    });

    // 透明窗口在 Windows 上启用系统阴影会把边缘渲染成黑色描边（黑边），
    // 卡片自带 CSS 阴影，OS 层阴影关闭即可（与便签/剪贴板浮窗一致）
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;
    crate::lockdown_webview(&win);
    crate::win_taskbar::apply(&win);

    // 3s 兜底足够（单卡片页面）；超时也照常返回，最多丢一次 shown 事件
    let _ = rx.recv_timeout(std::time::Duration::from_secs(3));
    log::info!("搜索浮窗已创建（隐藏常驻）");

    // 关闭 == 收起：绝不销毁（销毁会迫使下次唤起现场重建 WebView2，见模块注释）
    let handle = app.clone();
    win.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_window(&handle);
        }
        // 失焦自动收起的第一道闸（Windows 上真正的兜底是 start_focus_watchdog：
        // 焦点进 WebView2 子窗口后父窗口收不到 WM_KILLFOCUS，本分支多在
        // show→set_focus 交接瞬间误触发一次，宽限就是为它留的）。
        // ⚠️ 必须给宽限：show() 与 set_focus() 之间会先来一次 Focused(false)，
        // 无宽限会把刚弹出的窗口瞬间收掉（表现为「按热键闪一下没了」）。
        tauri::WindowEvent::Focused(false) => {
            if shown_recently() {
                return;
            }
            hide_window(&handle);
        }
        _ => {}
    });

    Ok(win)
}

/// 最近一次显示的时刻（失焦收起宽限用）
static LAST_SHOWN_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 距离上次显示是否还在宽限窗口内（200ms）
fn shown_recently() -> bool {
    let last = LAST_SHOWN_MS.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    now.saturating_sub(last) < 200
}

/// 失焦看门狗是否在跑（防止重复起线程；线程自灭时自清）
static WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

/// 起失焦看门狗（Windows）：窗口可见期间轮询前台窗口，一旦易主就收起。
///
/// 为什么不能只靠 `WindowEvent::Focused(false)`：present() 经
/// `webview_focus::focus_webview` 把键盘焦点送进 WebView2 子窗口，此后用户点别的
/// 应用时失焦的是**子窗口**，而 tao 只在父窗口收到 WM_KILLFOCUS 时才发
/// Focused(false)（WM_ACTIVATE 不参与，tao 0.35 event_loop.rs 实证）——父窗口
/// 自 show→set_focus 交接那次之后就永远等不到第二次 Focused(false)，表现为
/// 「点哪儿搜索框都赖着不走」。看门狗直接对账 GetForegroundWindow（激活归属），
/// 不依赖键盘焦点落在哪个子窗口。
#[cfg(target_os = "windows")]
fn start_focus_watchdog(app: &AppHandle) {
    if WATCHDOG_RUNNING.swap(true, Ordering::SeqCst) {
        return; // 已有看门狗在跑（重复唤起不叠加线程）
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let Some(win) = handle.get_webview_window(LABEL) else {
                break;
            };
            if !native_visible(&win) {
                break; // 已被其它路径收起，看门狗随之退场
            }
            if shown_recently() {
                // 唤起宽限：show → set_focus 之间前台可能还没落位到浮窗
                continue;
            }
            match foreground_is_ours(&win) {
                // 前台暂不可得（系统过渡态）按「别动」处理，下个周期再看
                Some(true) | None => continue,
                Some(false) => {}
            }
            hide_window(&handle);
            break;
        }
        WATCHDOG_RUNNING.store(false, Ordering::SeqCst);
    });
}

#[cfg(not(target_os = "windows"))]
fn start_focus_watchdog(_app: &AppHandle) {}

/// 前台窗口是否仍是搜索浮窗自己。None = 前台暂不可得（系统过渡态）。
#[cfg(target_os = "windows")]
fn foreground_is_ours(win: &WebviewWindow) -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() {
        return None;
    }
    let self_hwnd = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    Some(self_hwnd != 0 && fg as isize == self_hwnd)
}

/// 显示（或聚焦）搜索浮窗：窗口不存在则先按需创建，再 show + focus。
///
/// ⚠️ 首次建窗必须丢到**后台线程**：Tauri 的建窗消息在「主线程」上会直接在当前
/// 调用栈里执行 WebView2 controller 创建，和 IPC/菜单回调嵌套时会把消息泵卡死
/// （chat_window.rs 有同款事故记录）。后台线程调用会走 send_user_message 投递到
/// 主事件循环、等主线程空闲时创建。
pub fn show_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        present(app, &win);
        return;
    }
    if BUILDING.swap(true, Ordering::SeqCst) {
        return; // 连点去重
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let result = build(&handle);
        BUILDING.store(false, Ordering::SeqCst);
        match result {
            Ok(win) => present(&handle, &win),
            Err(e) => log::warn!("搜索浮窗按需创建失败: {e}"),
        }
    });
}

/// 已建好窗口后的显示逻辑（按需创建线程与常规唤起共用）
fn present(app: &AppHandle, win: &WebviewWindow) {
    // 记录唤起前的前台窗口——必须在 show/set_focus 之前，否则记到的是浮窗自己
    remember_prev_focus(win);
    // 每次唤起都重新定位：跟随鼠标所在显示器双居中（副屏用户不该被拽回主屏中心）。
    // 拖拽只影响当前这次展示——收起后下次唤起回到全局居中，位置不做跨唤起记忆。
    let scale = win.scale_factor().unwrap_or(1.0);
    let (w, h) = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
    if let Some(work) = cursor_work_area().or_else(primary_work_area) {
        let (x, y) = centered_position(work, w, h);
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
    LAST_SHOWN_MS.store(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        Ordering::Relaxed,
    );
    // 先恢复内存级别再显示（webview_mem：Low 态缓存已吐，首帧前回 Normal）
    crate::webview_mem::on_shown(app, LABEL);
    if !native_visible(win) {
        // 浮窗显示一律走 win_taskbar::show（tao 每次 show 都会重建 ex-style，
        // 直调 win.show() 会让任务栏冒出图标，见该模块注释 / 约定 48）
        crate::win_taskbar::show(win);
    }
    let _ = win.set_focus();
    // 窗口成为前台 ≠ 键盘焦点进了 webview：不补这一刀，↑↓/Esc 都得先点一下
    // 窗口才生效（见 webview_focus.rs 的取证）
    crate::webview_focus::focus_webview(app, LABEL);
    let _ = app.emit_to(LABEL, "search-window-shown", ());
    start_focus_watchdog(app);
}

/// 唤起前的前台窗口句柄（收起时归还焦点）。
///
/// 必须在**显示之前**记录：等 hide 时再 `GetForegroundWindow` 拿到的就是浮窗自己
/// （它一直握着前台），照那个值「归还」等于什么都不做——用户 Esc 收起后会卡在一个
/// 已隐藏的窗口上，得先点一下原应用才能继续打字。口径与 `clipboard::prev_focus` 一致。
static PREV_FOCUS: std::sync::Mutex<Option<isize>> = std::sync::Mutex::new(None);

/// 记录唤起前的前台窗口（仅当它不属于本浮窗自身时）
#[cfg(target_os = "windows")]
fn remember_prev_focus(win: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() {
        return;
    }
    let self_hwnd = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    // 前台已经是浮窗自己（重复唤起 / 已被聚焦）时保留原记录，别把它覆盖成本窗口
    if fg as isize == self_hwnd {
        return;
    }
    if let Ok(mut guard) = PREV_FOCUS.lock() {
        *guard = Some(fg as isize);
    }
}

#[cfg(not(target_os = "windows"))]
fn remember_prev_focus(_win: &WebviewWindow) {}

/// 收起：只隐藏，窗口常驻复用
pub fn hide_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    if !native_visible(&win) {
        return;
    }
    // 取出唤起前记录的前台窗口；取出即清（一次唤起只归还一次）
    let prev_fg = PREV_FOCUS.lock().ok().and_then(|mut g| g.take());

    // 是否该归还焦点：只有「用户主动关（Esc / 点卡片空白）」才还——那时前台仍是
    // 浮窗自己。若前台已经变成别的应用，说明是点击外部触发的失焦收起，这时再
    // SetForegroundWindow 会把焦点从用户刚点的应用手里抢回去。
    #[cfg(target_os = "windows")]
    let should_restore = unsafe {
        let fg = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        let self_hwnd = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        fg.is_null() || fg as isize == self_hwnd || self_hwnd == 0
    };

    let _ = win.hide();
    crate::win_taskbar::apply(&win);
    crate::webview_mem::on_hidden(app, LABEL);
    let _ = app.emit_to(LABEL, "search-window-hidden", ());

    #[cfg(target_os = "windows")]
    if should_restore {
        if let Some(prev) = prev_fg {
            // 迟一拍：hide 之后 tao 还会重建 ex-style，立刻抢焦点会被它覆盖回去
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(60));
                crate::clipboard::force_focus_window(prev as *mut core::ffi::c_void);
            });
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = prev_fg;
    log::info!("搜索浮窗已收起（常驻复用）");
}

/// 唤起/收起（全局热键、标题栏按钮、悬浮球「全局搜索」共用）
pub fn toggle(app: &AppHandle) {
    match app.get_webview_window(LABEL) {
        Some(win) if native_visible(&win) => hide_window(app),
        _ => show_window(app),
    }
}

// ---------------- 搜索结果路由 ----------------

/// 网页资源的打开方式（纯函数，便于单测）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRoute {
    /// 主窗内嵌面板（`suda-panel` 是主窗的子 webview：必须亮主窗）
    Panel,
    /// 独立应用内浏览器窗池
    Window,
    /// 系统默认浏览器
    System,
}

/// 决定一条网页资源结果该怎么打开。
///
/// 只有 http/https 才谈得上「应用内」；smb/ftp 等远程协议 webview 导航不了，
/// 一律交系统（口径同 store.launchResource）。
pub fn web_route(target: &str, open_mode: &str) -> WebRoute {
    let http = {
        let t = target.trim().to_ascii_lowercase();
        t.starts_with("http://") || t.starts_with("https://")
    };
    if !http {
        return WebRoute::System;
    }
    match open_mode {
        "window" => WebRoute::Window,
        "system" => WebRoute::System,
        // 默认/未知一律按面板（与 settings 的默认值 panel 对齐）
        _ => WebRoute::Panel,
    }
}

/// 打开一条速达资源结果。
///
/// 返回 `true` 表示「已经就地打开、不要亮主窗」；`false` 表示需要主窗接手
/// （面板模式网页——内嵌面板挂在主窗上）。
fn open_resource_without_main(
    app: &AppHandle,
    state: &State<'_, DbState>,
    id: i64,
) -> Result<bool, String> {
    let (kind, target) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let res = crate::repo::resource::get(&conn, id).map_err(|e| e.to_string())?;
        (res.kind, res.target)
    };
    match kind {
        ResourceKind::Web => {
            let cfg = crate::config::load();
            match web_route(&target, &cfg.suda_web_open_mode) {
                WebRoute::Panel => Ok(false), // 主窗接手
                WebRoute::Window => {
                    crate::suda_browser::open_in_pool_value(app, &target)?;
                    // 与 suda_browser_open / 面板 / 系统三条路径同口径打最近使用——
                    // 独立窗路径漏打点会让该资源不进「最近使用」通栏排序
                    let conn = state.0.lock().map_err(|e| e.to_string())?;
                    let _ = crate::repo::resource::touch(&conn, id);
                    Ok(true)
                }
                WebRoute::System => {
                    crate::process::open_url(&target)?;
                    let conn = state.0.lock().map_err(|e| e.to_string())?;
                    let _ = crate::repo::resource::touch(&conn, id);
                    Ok(true)
                }
            }
        }
        // 程序 / 文件：走与 launch_resource 完全一致的既有路径（含「已在运行则调度到前台」）
        _ => {
            crate::commands::launch_resource_inner(state, id)?;
            Ok(true)
        }
    }
}

#[tauri::command]
pub fn search_window_toggle(app: AppHandle) {
    toggle(&app);
}

#[tauri::command]
pub fn search_window_hide(app: AppHandle) {
    hide_window(&app);
}

/// 前端就绪后拉取浮窗当前可见性（推拉双保险的「拉」）：
/// 建窗完成那一刻 present() 发出的 search-window-shown 可能比页面监听注册早
/// （首次建窗 ~1s 后页面才挂载完），推丢了就表现为「窗口出来了但卡片没渲染」
/// ——透明空窗看起来像没呼出，用户得再按好几次热键。页面挂载完成后主动拉一次，
/// 丢掉的 shown 就地补上；之后仍以 shown/hidden 推送为准。
#[tauri::command]
pub fn search_window_is_visible(app: AppHandle) -> bool {
    match app.get_webview_window(LABEL) {
        Some(win) => native_visible(&win),
        None => false,
    }
}

/// 打开一条搜索结果资源（程序 / 文件 / 网页，路由见 `web_route` 与
/// `open_resource_without_main`）。需要主窗接手的（软件内面板模式网页）在这里
/// 亮主窗并派发 `open-suda-web-panel`，主窗 index.vue 收到后走既有面板打开链路。
///
/// **必须 async**（口径同 `suda_browser_open` / `suda_browser_open_url`）：Tauri 同步命令
/// 跑在主线程，而「独立窗模式网页」的首次打开会经 `ensure_pool_windows` 在调用线程内联
/// build 池窗口——主线程同步建 WebView2 会卡死消息泵（铁律，见 suda_browser.rs 注释）；
/// async 命令跑在异步运行时线程，建窗消息投递回主循环执行。
#[tauri::command]
pub async fn search_open_resource(
    app: AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<(), String> {
    if open_resource_without_main(&app, &state, id)? {
        return Ok(());
    }
    // 面板模式网页：内嵌面板是主窗的子 webview，结构上离不开主窗
    let (name, url) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let res = crate::repo::resource::get(&conn, id).map_err(|e| e.to_string())?;
        let _ = crate::repo::resource::touch(&conn, id);
        (res.name, res.target)
    };
    crate::tray::show_window(&app);
    let _ = app.emit_to(
        "main",
        "open-suda-web-panel",
        serde_json::json!({ "id": id, "url": url, "name": name }),
    );
    Ok(())
}

/// 打开一条速记结果：亮主窗并交给主窗切视图 + 打开该笔记
#[tauri::command]
pub fn search_open_note(app: AppHandle, id: i64) {
    crate::tray::show_window(&app);
    let _ = app.emit_to("main", "open-search-note", id);
}

/// 打开一条待办结果：亮主窗并交给主窗高亮该待办（回工作台）
#[tauri::command]
pub fn search_open_todo(app: AppHandle, id: i64) {
    crate::tray::show_window(&app);
    let _ = app.emit_to("main", "open-search-todo", id);
}

/// 打开一条设置项搜索结果：亮主窗并交给主窗切到对应设置分区
/// （分区 id 来自构建期生成的 settingsIndex，前端落地走 SettingsView 的 initialSection 深链）
#[tauri::command]
pub fn search_open_settings(app: AppHandle, section: String) {
    crate::tray::show_window(&app);
    let _ = app.emit_to("main", "open-search-settings", section);
}

/// 打开一条扩展搜索结果：亮主窗并交给主窗按该扩展的「打开方式」打开
/// （view/window/drawer 分流在主窗前端，与扩展中心行 ▶ 同一条链路）
#[tauri::command]
pub fn search_open_extension(app: AppHandle, id: String) {
    crate::tray::show_window(&app);
    let _ = app.emit_to("main", "open-search-extension", id);
}

#[cfg(test)]
mod tests {
    use super::{centered_position, web_route, WebRoute};

    /// 1080p 工作区（物理 px 口径，任务栏在底部）
    const WORK: (i32, i32, i32, i32) = (0, 0, 1920, 1040);

    /// 水平 + 垂直双居中
    #[test]
    fn centered_position_centers_on_both_axes() {
        let (x, y) = centered_position(WORK, 600, 520);
        assert_eq!(x, (1920 - 600) / 2, "水平居中");
        assert_eq!(y, (1040 - 520) / 2, "垂直居中");
    }

    /// 副屏 / 负坐标：偏移工作区内依旧双居中
    #[test]
    fn centered_position_respects_offset_work_area() {
        // 左侧副屏：工作区原点为负
        let work = (-1920, 0, 0, 1040);
        let (x, y) = centered_position(work, 600, 520);
        assert_eq!(x, -1920 + (1920 - 600) / 2);
        assert_eq!(y, (1040 - 520) / 2);
    }

    /// 工作区比窗口还小时钳到左上角，绝不弹到屏外
    #[test]
    fn centered_position_clamps_when_area_too_small() {
        let tiny = (100, 50, 500, 300); // 400×250，窗口 600×520
        let (x, y) = centered_position(tiny, 600, 520);
        assert_eq!((x, y), (100, 50));
    }

    /// 网页路由：仅 http/https 才谈得上应用内打开，其余交系统
    #[test]
    fn web_route_picks_mode_only_for_http() {
        assert_eq!(web_route("https://a.com", "panel"), WebRoute::Panel);
        assert_eq!(web_route("http://a.com", "window"), WebRoute::Window);
        assert_eq!(web_route("https://a.com", "system"), WebRoute::System);
        // 未知模式回退面板（与设置默认值一致）
        assert_eq!(web_route("https://a.com", "unknown"), WebRoute::Panel);
        // 远程协议 / 本地路径：webview 导航不了，一律系统打开
        assert_eq!(web_route("smb://host/share", "panel"), WebRoute::System);
        assert_eq!(web_route("ftp://host/x", "window"), WebRoute::System);
        assert_eq!(web_route("  HTTPS://A.COM  ", "window"), WebRoute::Window);
    }
}
