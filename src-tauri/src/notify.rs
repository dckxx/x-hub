//! 自定义右下角通知弹窗（跨 Win10 / Win11 一致，替代系统 WinRT Toast）。
//!
//! 为什么不用系统通知：Windows 10 对「非安装包 / 便携版」应用有额外硬门槛——AUMID 必须能在
//! 开始菜单里找到对应 .lnk，否则 `CreateToastNotifier` 判定应用不具备通知能力，Toast 被静默丢弃
//! （表现就是 Win11 正常、Win10 完全无弹窗）。与其去维护快捷方式，不如自绘一个右下角小窗，
//! 两个版本表现一致、样式可控。
//!
//! 架构：一个透明、置顶、跳过任务栏、不抢焦点（WS_EX_NOACTIVATE + SW_SHOWNA）的独立 WebView 窗
//! （label = `notice`，前端 App.vue 路由到 NoticeOverlay 渲染卡片堆叠）。后端只做：建窗/隐藏常驻、
//! 把一条通知 emit 给前端、以及按前端上报的内容高度把窗口锚定到「当前显示器工作区」右下角（避让
//! 任务栏）。卡片的入/出场动画、逐条自动淡出计时、点击关闭都由前端负责，后端无状态。
//!
//! 启动时 `init` 预创建隐藏窗口（同剪贴板浮层/悬浮球的约定：运行时现场建 WebView2 窗口易与并发
//! 窗口操作交错导致整窗未响应）。推送时后端**先**把窗口定位+无激活显示，再 emit 内容——避免依赖
//! 隐藏态 WebView2 是否及时处理 IPC 事件的不确定性。

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// 通知窗 label（App.vue 按此路由渲染 NoticeOverlay）
pub const NOTICE_LABEL: &str = "notice";
/// 通知窗宽度（逻辑 px）
const NOTICE_WIDTH: f64 = 360.0;
/// 距工作区右 / 下边留白（逻辑 px）：8 + 前端 stack 内衬 8 = 卡片离任务栏/屏幕右沿各 16px
const NOTICE_MARGIN: f64 = 8.0;
/// 窗口默认高度（首帧定位用；随后前端按实际内容高度回调 notice_layout 校正）
const NOTICE_DEFAULT_HEIGHT: f64 = 104.0;

/// 前端通知窗监听是否就绪：notice webview 启动需要一两秒，期间倒计时/待办线程
/// 可能已推送提醒——`emit_to` 不缓存，事件会静默丢失，而提醒的 remind_fired 已置位、
/// 倒计时已顺延，丢了不可恢复。就绪前暂存（容量上限），`notice_ready` 时重放。
static NOTICE_READY: AtomicBool = AtomicBool::new(false);

/// 未就绪期间暂存的通知（kind, title, body）。见 NOTICE_READY。
fn pending_notices() -> &'static std::sync::Mutex<Vec<(String, String, String)>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<Vec<(String, String, String)>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// 暂存队列上限：超出丢最旧（启动首秒积压超过 8 条通知本身就不正常）
const PENDING_CAP: usize = 8;

/// 启动时预创建通知窗（隐藏常驻）。只在启动期（setup 主线程）建窗；
/// 运行时（show_notice）仅取预创建实例——现场 build WebView2 窗口会与并发
/// 窗口操作交错挂起整窗（铁律，见 clipboard.rs / floating_ball.rs 同款注释）。
///
/// **⚠️ 这是全工程唯一的通知窗 build 调用点，勿删**：v0.5.3 发版审查批次（2147bd9）
/// 以「运行时禁止现场建窗」为由删 ensure_window 的 build 兜底时把它一并删了——结果
/// 窗口从此不存在、notice_ready 永不触发、所有通知卡进暂存队列，弹窗整体静默失联
/// （单测/类型检查全绿，只有实机暴露）。该铁律的正解 = build 恰留这一处、在 init。
pub fn init(app: &AppHandle) {
    // Linux：自绘通知窗在部分 X11 环境（mutter + 虚拟显卡）下创建时隐藏 →
    // 首次 show 无法映射（WM_STATE 停在 Withdrawn，见 show_notice 注释），暂不预创建。
    #[cfg(not(target_os = "windows"))]
    {
        log::info!("Linux 下禁用自绘通知弹窗，跳过通知窗预创建");
        return;
    }
    #[cfg(target_os = "windows")]
    if app.get_webview_window(NOTICE_LABEL).is_some() {
        return;
    }
    #[cfg(target_os = "windows")]
    match build_window(app) {
        Ok(_) => log::info!("通知窗已预创建（隐藏常驻）"),
        Err(e) => log::warn!("通知窗预创建失败: {e}"),
    }
}

fn build_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    let mut builder =
        // 轻量入口 notice.html（P2）：只渲染通知卡片，不加载完整 SPA（内存优化，见 src/light/notice.ts）
        WebviewWindowBuilder::new(app, NOTICE_LABEL, WebviewUrl::App("notice.html".into()))
            .title("通知")
            .inner_size(NOTICE_WIDTH, NOTICE_DEFAULT_HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .background_color(tauri::window::Color(0, 0, 0, 0))
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 透明窗口在 Windows 上开系统阴影会渲染成黑描边；卡片自带 CSS 阴影，关掉 OS 阴影（同倒计时/剪贴板浮窗）
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    builder.build().map_err(|e| e.to_string())
}

/// 推送一条通知：确保窗口存在 → 先定位并无激活显示 → emit `notice-new` 给前端渲染。
/// `kind` 供前端选图标/配色（"countdown" | "todo" | "info" ...）。
pub fn show_notice(app: &AppHandle, kind: &str, title: &str, body: &str) {
    // Linux：自绘通知窗在部分 X11 环境（mutter + 虚拟显卡）下创建时隐藏 → 首次 show
    // 无法映射（WM_STATE 停在 Withdrawn，实测 2026-10-07），弹窗不可用，暂时禁用。
    // 提醒不丢：todo-remind / countdown-fired 事件照常发出，主窗可见时以应用内 toast 呈现。
    #[cfg(not(target_os = "windows"))]
    {
        log::info!("[通知·Linux 禁用弹窗] [{kind}] {title}: {body}");
        return;
    }
    let win = match ensure_window(app) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("通知窗不可用，丢弃一条通知（{title}）: {e}");
            return;
        }
    };
    // 前端未就绪：暂存等 notice_ready 重放（直接 emit 会丢事件，且丢的是不可恢复的提醒）。
    // 同时把（还空着的）窗口先显示出来：隐藏态 WebView2 可能推迟脚本/渲染，
    // notice_ready 依赖页面 onMounted 触发——不显示的话「暂存等就绪」会自锁死，
    // 弹窗整体失联（v0.5.3 发版批次实测踩坑）。空窗完全透明，多显示一两秒无感
    if !NOTICE_READY.load(Ordering::Relaxed) {
        {
            #[cfg(target_os = "windows")]
            anchor_default_and_show(&win);
            #[cfg(not(target_os = "windows"))]
            {
                let _ = win.show();
            }
        }
        let mut q = pending_notices().lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= PENDING_CAP {
            q.remove(0);
        }
        q.push((kind.to_string(), title.to_string(), body.to_string()));
        log::info!("通知窗前端未就绪，暂存通知: [{kind}] {title}");
        // 复查竞态：上面的窗口显示是跨线程派发（毫秒级），期间前端可能恰好 notice_ready——
        // ready 的 swap+take 发生在本条入队之前就拿不到它，而 ready 已置位、不会再有重放，
        // 这条提醒会永久滞留队列（remind_fired 已置位不可恢复 → 静默丢失）。
        // 入队后复查：已就绪就取回本条落回下方就绪路径直接推送；找不到说明已被
        // 并发的 notice_ready 重放取走，无需重复推。
        if !NOTICE_READY.load(Ordering::Relaxed) {
            return;
        }
        let mut q = pending_notices().lock().unwrap_or_else(|p| p.into_inner());
        match q.iter().rposition(|(k, t, b)| k == kind && t == title && b == body) {
            Some(pos) => {
                q.remove(pos);
            }
            None => return,
        }
        drop(q);
        log::info!("通知在暂存瞬间前端恰好就绪，直接推送: [{kind}] {title}");
    }
    // 已在展示堆叠：只保证显示，不动尺寸/位置——重置回默认高度会让已展示的卡片
    // 被瞬时裁切、再经一次 IPC 往返校正回来（肉眼可见跳动）；高度交给 notice_layout 增量校正
    if win.is_visible().unwrap_or(false) {
        #[cfg(target_os = "windows")]
        show_no_activate(&win);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = win.show();
        }
    } else {
        // 首帧：先定位 + 显示（不依赖隐藏 WebView2 处理事件的时序），高度随后校正
        anchor_default_and_show(&win);
    }
    // 驻留时长随每条通知一起下发（设置 → 常规可调）：通知窗是启动期预创建、常驻不重启的，
    // 若让前端自己读一次配置，改了设置也得等下次启动才生效
    let duration_ms = crate::config::load().notice_duration_ms.clamp(1000, 60_000);
    let payload = serde_json::json!({
        "kind": kind,
        "title": title,
        "body": body,
        "durationMs": duration_ms
    });
    if let Err(e) = app.emit_to(NOTICE_LABEL, "notice-new", &payload) {
        log::warn!("通知事件投递失败: {e}");
    }
    log::info!("已推送右下角通知: [{kind}] {title}");
}

/// 窗口只取预创建实例。**不做运行时 build 兜底**：预创建失败说明启动期建窗已出问题，
/// ticker 线程运行时 build WebView2 会与并发窗口操作交错挂起整窗（铁律：运行时禁止
/// 现场 build/destroy WebView2 窗口）——直接丢弃该条通知并让调用方日志可见。
fn ensure_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(NOTICE_LABEL)
        .ok_or_else(|| "通知窗未预创建（启动期建窗失败），已丢弃通知".to_string())
}

/// 前端通知窗监听就绪：置就绪标志并重放启动初期暂存的通知。
#[tauri::command]
pub fn notice_ready(app: AppHandle) {
    if NOTICE_READY.swap(true, Ordering::Relaxed) {
        return; // 已就绪过（前端重载等场景），不重复重放
    }
    let drained: Vec<(String, String, String)> = std::mem::take(
        &mut *pending_notices().lock().unwrap_or_else(|p| p.into_inner()),
    );
    let replayed = drained.len();
    for (kind, title, body) in drained {
        show_notice(&app, &kind, &title, &body);
    }
    if replayed > 0 {
        log::info!("通知窗前端就绪，重放 {replayed} 条启动期暂存通知");
    }
}

/// 用默认高度把窗口锚到右下角并无激活显示（供推送首帧；精确高度随后由 notice_layout 校正）。
fn anchor_default_and_show(win: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        apply_size(win, NOTICE_WIDTH, NOTICE_DEFAULT_HEIGHT);
        anchor_bottom_right(win, NOTICE_WIDTH, NOTICE_DEFAULT_HEIGHT);
        show_no_activate(win);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.set_resizable(true);
        let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            NOTICE_WIDTH,
            NOTICE_DEFAULT_HEIGHT,
        )));
        let _ = win.set_resizable(false);
        simple_bottom_right(win, NOTICE_WIDTH, NOTICE_DEFAULT_HEIGHT);
        let _ = win.show();
    }
}

/// 前端上报实际内容高度 → 设尺寸并重新锚定右下角（保持底边贴住工作区下沿，卡片向上生长）。
#[tauri::command]
pub fn notice_layout(app: AppHandle, height: f64) -> Result<(), String> {
    let win = app
        .get_webview_window(NOTICE_LABEL)
        .ok_or_else(|| "通知窗不存在".to_string())?;
    let h = height.clamp(1.0, 1600.0);
    #[cfg(target_os = "windows")]
    {
        apply_size(&win, NOTICE_WIDTH, h);
        anchor_bottom_right(&win, NOTICE_WIDTH, h);
        show_no_activate(&win);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            NOTICE_WIDTH,
            h,
        )));
        simple_bottom_right(&win, NOTICE_WIDTH, h);
        let _ = win.show();
    }
    Ok(())
}

/// 队列为空时前端收起窗口（仅隐藏，窗口常驻复用）。
#[tauri::command]
pub fn notice_dismiss_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(NOTICE_LABEL) {
        hide_window(&win);
    }
    Ok(())
}

// ---------- Windows：工作区右下角 + 不抢焦点显示 ----------

#[cfg(target_os = "windows")]
fn apply_size(win: &WebviewWindow, width_logical: f64, height_logical: f64) {
    // resizable(false) 会把 min/max 锁死在当前尺寸，后续 set_size 被 WM_GETMINMAXINFO 钳制而静默失效
    // （表现为窗口一直保持默认 104 高、卡片下方多出一条透明空隙）——临时解锁 → 改尺寸 → 再锁回。
    let _ = win.set_resizable(true);
    let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        width_logical,
        height_logical,
    )));
    let _ = win.set_resizable(false);
}

/// 计算并设置物理像素位置：光标所在显示器的工作区（rcWork，避让任务栏）右下角内缩 margin。
/// 取光标所在屏（而非窗口所在屏）：通知应出现在用户正在操作的那块屏上（多屏体验一致）。
#[cfg(target_os = "windows")]
fn anchor_bottom_right(win: &WebviewWindow, width_logical: f64, height_logical: f64) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let Ok(hwnd) = win.hwnd() else {
        return;
    };
    unsafe {
        let dpi = GetDpiForWindow(hwnd.0);
        let scale = if dpi > 0 { dpi as f64 / 96.0 } else { 1.0 };
        let w = (width_logical * scale).round() as i32;
        let h = (height_logical * scale).round() as i32;
        let m = (NOTICE_MARGIN * scale).round() as i32;

        // 光标所在显示器；取不到光标（异常）则退回窗口所在显示器
        let monitor = {
            let mut pt: POINT = std::mem::zeroed();
            if GetCursorPos(&mut pt) != 0 {
                MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
            } else {
                windows_sys::Win32::Graphics::Gdi::MonitorFromWindow(
                    hwnd.0,
                    MONITOR_DEFAULTTONEAREST,
                )
            }
        };
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        let work = info.rcWork;
        // 工作区比窗口还小的极端情况用 max 兜底，避免负坐标跑出屏幕
        let x = (work.right - w - m).max(work.left);
        let y = (work.bottom - h - m).max(work.top);
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
    }
}

/// 无激活显示：加 WS_EX_NOACTIVATE 后 SW_SHOWNA——通知不该抢走用户当前输入焦点。
#[cfg(target_os = "windows")]
fn show_no_activate(win: &WebviewWindow) {
    // 先恢复内存级别再显示（webview_mem：Low 态缓存已吐，通知首帧前回 Normal）
    crate::webview_mem::on_shown(win.app_handle(), win.label());
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_SHOWNA, WS_EX_NOACTIVATE,
    };
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd.0, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE as isize);
            ShowWindow(hwnd.0, SW_SHOWNA);
        }
        // 通知窗同样是 skip_taskbar 浮窗：显示时摘掉任务栏按钮（见 win_taskbar 模块注释）
        crate::win_taskbar::apply(win);
        return;
    }
    crate::win_taskbar::show(win);
}

#[cfg(target_os = "windows")]
fn hide_window(win: &WebviewWindow) {
    // 隐藏后把常驻 renderer 的内存目标级别降到 Low（webview_mem，轮询兜底）
    crate::webview_mem::on_hidden(win.app_handle(), win.label());
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            ShowWindow(hwnd.0, SW_HIDE);
        }
        return;
    }
    let _ = win.hide();
}

#[cfg(not(target_os = "windows"))]
fn hide_window(win: &WebviewWindow) {
    crate::webview_mem::on_hidden(win.app_handle(), win.label());
    let _ = win.hide();
}

// ---------- 非 Windows 兜底：用 Tauri 显示器逻辑（无工作区，仅整屏右下） ----------

#[cfg(not(target_os = "windows"))]
fn simple_bottom_right(win: &WebviewWindow, width_logical: f64, height_logical: f64) {
    let scale = win.scale_factor().unwrap_or(1.0);
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let size = m.size(); // 物理 px
    let origin = m.position();
    let w = (width_logical * scale).round() as i32;
    let h = (height_logical * scale).round() as i32;
    let m2 = (NOTICE_MARGIN * scale).round() as i32;
    let x = origin.x + size.width as i32 - w - m2;
    let y = origin.y + size.height as i32 - h - m2;
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
}
