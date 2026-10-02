//! 桌面悬浮球（ADR 0004）：常驻透明置顶小球，仅主窗口隐藏时显示。
//! 单击展开环形快捷菜单、双击切换主窗口（开着则收起）、右键托盘同款菜单；
//! 拖拽记忆位置，开启「贴边自动隐藏」时靠近屏幕边缘松手 → 球心落在屏边，
//! 只露出半个球体。露出/隐回由「边缘监视循环」完成（参考 tiez-clipboard 的
//! edge docking 设计）：100ms 轮询 GetCursorPos + 窗口矩形，命中屏边可见窄条
//! → 把窗口整体搬回屏内（滑出量 = 窗口半边长，整窗完全屏内 → 球体 + 粒子 +
//! 陀螺环一并露出）；光标离开滑出矩形 → 搬回半隐位。
//! 停靠身份由「记忆球心 + 窗口实际位置」双方一致才成立：窗口实际位置既不等于
//! 半隐位也不等于滑出位 → 说明球已被拖到别处，监视循环以窗口为准刷新记忆并
//! 停止干预（否则旧记忆会把球拽回屏边，表现为「拖到任意位置都弹回去」）。
//! 全程移动窗口位置、不依赖 WebView 指针事件/CSS——原生拖拽模态循环吞事件、
//! 半截屏外透明窗命中区不稳等老翻车点从根上消除。
//! 曾用「贴边吸附」（完整贴边停靠），用户反馈从未生效且不需要，已替换。
//! Windows / Linux：独立透明无边框窗口，与 countdown_window 同模式复用。
//! Linux 贴边：WM 不允许半截出屏，半隐被钳成贴边全显，悬停不再另滑一格
//! （否则窗口从光标下抽走，100ms 上下跳；见 `dock_hover_pos`）。
//!
//! 几何模型：球态窗口 = BALL_SIZE，菜单态 = MENU_SIZE，均以「球心」（窗口中心）为锚
//! 原子切换（单次 SetWindowPos）。窗口 resize 时 WebView2 内容重排滞后一帧，旧帧按
//! 旧视口渲染会让球先「跳」向窗口移动方向再弹回——前端在开合前后把窗口内容整体
//! 淡出/淡入，把跳动帧掩盖在「球化开成菜单」的过渡里（见 FloatingBallWindow 开合时序）。
//!
//! DPI 自愈（非整数缩放裁切修复）：系统缩放为非标准档位（如 110% → 106 DPI，
//! scale = 1.104166…）时，窗口物理尺寸与 WebView2 CSS 视口的换算存在取整，且悬浮球
//! 窗口大部分时间隐藏——隐藏窗口可能错过 WM_DPICHANGED（或 Win10/远程会话关闭
//! 「拖动时显示窗口内容」时 tao 显式跳过尺寸缩放），导致 tao 缓存的 scale_factor 与
//! 窗口物理尺寸都停留在旧值，而 WebView2 光栅化用窗口实时 DPI → 视口从 100 缩到
//! ~90.6 CSS px，球体最外圈陀螺环（视觉 ~94.8px）左右两侧被窗口边缘裁掉一截。
//! 对策：① 一切几何计算用 GetDpiForWindow 实时取 DPI（与 WebView2 同源，缓存过期
//! 也能算对）；② apply_geometry 用窗口实际 outer_size 推球心并按实时 scale 重设目标
//! 尺寸——任何开合/显示/动作都会顺带把失配修正回来；③ ScaleFactorChanged 事件即时
//! 重算；④ 前端 resize 失配自检兜底（floating_ball_reapply）。

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

use crate::config;

/// 悬浮球窗口 label（App.vue 按此路由到 FloatingBallWindow）
pub const LABEL: &str = "floating-ball";

/// 球态窗口尺寸（逻辑 px：48 中心球体 + 光晕/粒子/陀螺环余量，避免视觉被窗口裁切；
/// 100 = 容纳陀螺环最外圈视觉 94px + 3px 余量）
pub const BALL_SIZE: f64 = 100.0;
/// 环形菜单展开态窗口尺寸（逻辑 px：按钮轨道半径 92 + 按钮 26 → 外沿 118，中心 130 留 12px 余量；
/// 用户反馈 312 太空旷——按钮内沿距球缘 45px，收紧到 260 后空隙约 19px，8 键 hover 仍不重叠）
pub const MENU_SIZE: f64 = 260.0;
/// 球体半径（逻辑 px）：前端 .fb-ball 视觉 48px 直径的半径；用于默认初始位置留白
const BALL_R: f64 = 24.0;
/// 贴边自动隐藏触发距离（逻辑 px，球心距工作区边缘）：拖拽松手时球心距屏边在该值内
/// → 球心吸附到屏边（+PEEK），半隐。按用户实测数据定界：真贴边的松手点球心距边
/// 4~103px（103 是「靠边了但不吸附」的抱怨点，必须覆盖）；而用户明确称为
/// 「不靠边的地方」的松手点最小是 146px——200 的宽吸附带把这些位置也吞了，
/// 球从松手点滑到屏边，用户感知为「没靠边也自己挪一下/抖动」（反馈实录）。
/// 取 120：>103 覆盖贴边直觉，<146 不打扰自由放置。
/// **铁律：必须 > 滑出量（窗口半边长 BALL_SIZE/2 = 50 逻辑 px）+ 位置容差 POS_TOL 的
/// 物理换算**，否则「完整屏内、刚好贴边的自由位置」与「滑出位」在几何上无法区分，
/// 边缘监视会把用户放好的球当成停靠残留拽回屏边。
/// 注意：吸附动作本身 = 球从松手点平滑滑到屏边（slide_to ~80ms），吸附带内的
/// 「自己挪一下」是预期行为，不要当 bug 修；带外绝不移动。
const DOCK_TRIGGER: f64 = 120.0;
/// 半隐停靠位向屏内多露的距离（逻辑 px）：半隐窗口 = 停靠球心位 + dx·PEEK。
/// 球心精确压屏边时视觉只露 24px 一条弧，观感像「球直接没了」（用户反馈）；
/// 向屏内收 8px 后露出约 2/3 球体，保留「贴边藏着」语义的同时一眼能看到球。
/// 悬停滑出仍是整窗进屏（球 + 粒子 + 陀螺环全出），两态差异依旧明显
const PEEK: f64 = 8.0;
/// 停靠判定容差（物理 px）：记忆球心距屏边在该值内即视为该侧停靠态
/// （半隐位置是精确落在屏边的，容差只吸收 DPI 取整误差）
const DOCK_TOL: f64 = 6.0;
/// 位置一致容差（物理 px）：窗口实际左上角与「半隐位/滑出位」的偏差在该值内，
/// 才认为球确实停在停靠几何上——边缘监视据此决定接管还是撒手（见 edge_tick）
const POS_TOL: i32 = 10;
/// 默认初始位置留白：球缘距工作区边缘的视觉间距（逻辑 px）
const SNAP_GAP: f64 = 7.0;
/// 环形按钮上限（超过会互相重叠；保存命令与设置页双重钳制）
pub const MAX_BUTTONS: usize = 8;

/// 以下三个常量是「边缘监视循环」（tiez-clipboard 同款思路）的时序参数
/// 轮询间隔：100ms 足够跟手（悬停露出感知 ≈0.1s），CPU 成本可忽略
#[cfg(any(target_os = "windows", target_os = "linux"))]
const EDGE_POLL_MS: u64 = 100;
/// 已滑出后光标离开窗矩形多少物理 px 内不隐回（边界防抖）
#[cfg(any(target_os = "windows", target_os = "linux"))]
const EDGE_MARGIN: i32 = 12;
/// 滑出/隐回的动画帧数与帧距（5×16ms ≈ 80ms 平滑滑动，tiez 是瞬移，这里体验更好一点）
#[cfg(any(target_os = "windows", target_os = "linux"))]
const SLIDE_STEPS: i32 = 5;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const SLIDE_STEP_MS: u64 = 16;
/// 拖拽落位后的监视冷却（ms）：落位搬窗是异步 IPC 到主线程，写配置却是
/// 立即完成的——监视循环在这个间隙会读到「窗口旧位置 + 新记忆」而误判，
/// 把刚吸附的球当成位置漂移。冷却期内整跳跳过，窗口落定后监视再接管
#[cfg(any(target_os = "windows", target_os = "linux"))]
const DRAG_SETTLE_COOLDOWN_MS: u64 = 800;

/// 搬窗后等待「目标生效」的时限（ms）：超过它窗口仍不在目标上，说明 WM 不接受该
/// 坐标（Mutter/KWin 会把窗口钳回工作区）或 tao 没落实，此时**采纳窗口现位当新记忆**
/// 并停手。没有这条时限，停靠分支的「落位补齐」会每两跳重滑一次，
/// 日志实证每秒 6~10 条刷屏、球可见上下跳。
#[cfg(any(target_os = "windows", target_os = "linux"))]
const NUDGE_ADOPT_MS: u64 = 1200;

/// 最近一次拖拽落位时间戳（ms）；0 = 从未拖拽。见 DRAG_SETTLE_COOLDOWN_MS
#[cfg(any(target_os = "windows", target_os = "linux"))]
static LAST_DRAG_SETTLE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 最近一次悬停动画的搬窗时刻（ms），用于「同一目标不重复播动画」判断
#[cfg(any(target_os = "windows", target_os = "linux"))]
static LAST_NUDGE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 拖拽武装时刻（ms；0 = 未武装）：前端 `floating_ball_drag_begin`（位移超阈值、
/// 移交系统原生拖动时）记录；真正的松手由边缘监视循环检测——左键释放后的第一跳执行
/// `settle_drag`。**绝不能用 `startDragging()` 的 promise 当拖动结束信号**：实测它在
/// 拖动开始时就 resolve，松手钩子里读到的是拖动中途位置（与最终位置差几百 px），
/// 吸附/记忆全错，冷却后「落位补齐」再把球从屏边搬回中途——表现为「拖到边缘松手，
/// 球弹回屏幕中间」。**附带 TTL + cancel**：`floating_ball_drag_cancel`（startDragging
/// 启动失败）清零；武装超 TTL 未消费视为残留自动失效——否则拖拽武装后窗口被隐藏等
/// 异常链路下，用户下一次无关的左键单击松开会被当成拖拽落位（「带外绝不移动」被破坏）
#[cfg(any(target_os = "windows", target_os = "linux"))]
static DRAG_ARMED_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 拖拽武装有效期：按住左键拖动超过该时长视为异常残留（正常拖拽几秒内结束），
/// 落位标志自动作废，防无关单击被误消费
#[cfg(any(target_os = "windows", target_os = "linux"))]
const DRAG_ARMED_TTL_MS: u64 = 30_000;

/// 记忆球心进程内缓存：edge_tick 100ms 一跳，不能每跳读配置文件（同 AUTO_HIDE 缓存的
/// 理由——常态「贴边停靠/自由位驻留」下每跳 config::load() = 每秒 10 次读盘 + JSON
/// 解析的永久后台 IO）。None = 未初始化（首跳回落读盘填充）。它是配置盘上值的镜像，
/// **只在写盘成功时更新**；写点：unpop_to_inside / 救球 / settle_drag
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn memo_ball() -> &'static std::sync::Mutex<Option<(f64, f64)>> {
    static MEMO: std::sync::OnceLock<std::sync::Mutex<Option<(f64, f64)>>> =
        std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn memo_ball_get() -> Option<(f64, f64)> {
    *memo_ball().lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn memo_ball_set(x: f64, y: f64) {
    *memo_ball().lock().unwrap_or_else(|p| p.into_inner()) = Some((x, y));
}

/// 当前系统时间（ms）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 展开前的窗口位置（物理 px）：收起时恢复，靠边挪位后球能回到原吸附点。
/// 用全局 Mutex 而非 thread_local：拖拽/展开命令是 async（跑在tokio线程池），
/// 主线程的 sync_with_main 也会收拢几何，跨线程共享必须用带锁的静态。
#[cfg(any(target_os = "windows", target_os = "linux"))]
static PRE_EXPAND_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// 主窗是否处于最小化（与「隐藏到托盘」一样属于视觉不可见 → 球显示）。
/// MAIN_WINDOW_VISIBLE 状态位只覆盖托盘/快捷键的显式 show/hide，点标题栏最小化
/// 不经过那条链——由主窗 Resized 事件检测最小化变化后经 set_main_minimized 更新。
#[cfg(any(target_os = "windows", target_os = "linux"))]
static MAIN_MINIMIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 主窗最小化状态变化入口（lib.rs 主窗事件钩子调用）：更新状态并联动球显隐
pub fn set_main_minimized(app: &AppHandle, minimized: bool) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use std::sync::atomic::Ordering;
        if MAIN_MINIMIZED.swap(minimized, Ordering::SeqCst) == minimized {
            return;
        }
        sync_with_main(app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, minimized);
    }
}

/// 贴边停靠状态：球心落在某侧工作区边缘（该侧球体藏屏外一半）。
/// 露出/隐回由边缘监视循环移动窗口实现（见 edge_tick），前端不再做 CSS 平移。
#[derive(Debug, Default, Serialize, Clone, Copy)]
pub struct DockState {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

#[derive(Debug, Serialize)]
pub struct FloatingBallState {
    pub enabled: bool,
    /// 贴边自动隐藏（取代旧「贴边吸附」）
    pub auto_hide: bool,
    /// 与主窗口同时显示（主窗可见时球保持常驻）
    pub with_main: bool,
    pub buttons: Vec<String>,
    /// 记忆的球心位置（物理 px，拖拽松手后由后端记忆；None = 从未拖拽过）
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// 静止态保持转动（炫酷模式）：前端据此决定是否跳过 rings-idle 暂停与 24fps 降帧
    pub idle_spin: bool,
    /// 当前停靠边（半隐态）；露出/隐回由边缘监视循环移动窗口，前端仅只读展示
    pub dock: DockState,
    /// 球态窗口逻辑边长（前端 resize 失配自检的期望值之一）
    pub ball_size: f64,
    pub menu_size: f64,
}

/// 停靠判定：存储球心（物理 px）距所在显示器工作区边缘在 DOCK_TOL 内 → 该侧停靠
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn dock_state(win: &tauri::WebviewWindow, cx: Option<f64>, cy: Option<f64>) -> DockState {
    let (Some(cx), Some(cy)) = (cx, cy) else {
        return DockState::default();
    };
    let Ok(Some(mon)) = win.current_monitor() else {
        return DockState::default();
    };
    let wa = mon.work_area();
    let left = wa.position.x as f64;
    let top = wa.position.y as f64;
    let right = left + wa.size.width as f64;
    let bottom = top + wa.size.height as f64;
    DockState {
        left: (cx - left).abs() <= DOCK_TOL,
        right: (right - cx).abs() <= DOCK_TOL,
        top: (cy - top).abs() <= DOCK_TOL,
        bottom: (bottom - cy).abs() <= DOCK_TOL,
    }
}

// ---------- 贴边边缘监视（tiez-clipboard edge docking 同款思路）----------
// 半隐/露出全部由后台轮询「系统光标位置 + 窗口矩形」并移动窗口实现，物理像素口径，
// 不依赖 WebView pointerenter/leave 与 CSS 平移。前端 hover 判定在真实桌面上不可靠：
// 原生拖拽模态循环期间 WebView 收不到任何指针事件（hovered 卡旧值）、半截屏外的
// 透明窗口命中区随 DPI/阴影抖动——这些都曾导致「贴边隐藏时好时坏」。

/// 贴边自动隐藏开关缓存：监视循环 100ms 一跳，不能每跳读配置文件；
/// init / floating_ball_save_settings 负责写入
#[cfg(any(target_os = "windows", target_os = "linux"))]
static AUTO_HIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

// 不再维护「当前是否滑出态」的进程内布尔缓存：它一旦与窗口实际位置分叉（DPI 自愈、
// 菜单展开钳制、拖拽收尾丢失）就会把球拽到错误位置，改为每跳由窗口位置实时推导
// （见 edge_tick 的 at_hidden / at_popped）

/// 系统光标位置（物理 px）
#[cfg(target_os = "windows")]
fn cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt: POINT = unsafe { std::mem::zeroed() };
    (unsafe { GetCursorPos(&mut pt) } != 0).then_some((pt.x, pt.y))
}

/// Linux：X11 连接（边缘监视 100ms 一跳，不能每跳 `XOpenDisplay` / 起 xdotool）。
#[cfg(target_os = "linux")]
struct LinuxX11 {
    xlib: x11_dl::xlib::Xlib,
    dpy: *mut x11_dl::xlib::Display,
    root: x11_dl::xlib::Window,
}

#[cfg(target_os = "linux")]
unsafe impl Send for LinuxX11 {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LinuxX11 {}

#[cfg(target_os = "linux")]
fn linux_x11() -> Option<&'static std::sync::Mutex<LinuxX11>> {
    use std::sync::{Mutex, OnceLock};
    static STATE: OnceLock<Option<Mutex<LinuxX11>>> = OnceLock::new();
    STATE
        .get_or_init(|| {
            let xlib = x11_dl::xlib::Xlib::open().ok()?;
            unsafe {
                // 监视线程与其它路径可能并发调 Xlib
                (xlib.XInitThreads)();
                let dpy = (xlib.XOpenDisplay)(std::ptr::null());
                if dpy.is_null() {
                    return None;
                }
                let root = (xlib.XDefaultRootWindow)(dpy);
                Some(Mutex::new(LinuxX11 { xlib, dpy, root }))
            }
        })
        .as_ref()
}

/// Linux：XQueryPointer 一次取光标 + 按键掩码（失败 = 无 DISPLAY / 纯 Wayland）
#[cfg(target_os = "linux")]
fn x11_pointer() -> Option<(i32, i32, u32)> {
    let state = linux_x11()?;
    let g = state.lock().unwrap_or_else(|p| p.into_inner());
    unsafe {
        let mut root_ret: x11_dl::xlib::Window = 0;
        let mut child: x11_dl::xlib::Window = 0;
        let mut rx = 0i32;
        let mut ry = 0i32;
        let mut wx = 0i32;
        let mut wy = 0i32;
        let mut mask = 0u32;
        let ok = (g.xlib.XQueryPointer)(
            g.dpy,
            g.root,
            &mut root_ret,
            &mut child,
            &mut rx,
            &mut ry,
            &mut wx,
            &mut wy,
            &mut mask,
        );
        if ok == 0 {
            return None;
        }
        Some((rx, ry, mask))
    }
}

/// Linux：XQueryPointer 取光标（X11 / XWayland；纯 Wayland 无 DISPLAY 时失败，贴边降级）
#[cfg(target_os = "linux")]
fn cursor_pos() -> Option<(i32, i32)> {
    x11_pointer().map(|(x, y, _)| (x, y))
}

/// 左键是否按下（原生拖拽循环 / 按住操作中：禁止移动窗口抢位）
#[cfg(target_os = "windows")]
fn lmb_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // VK_LBUTTON = 0x01，高位 0x8000 = 当前按下
    unsafe { (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0 }
}

/// Linux：必须查真实按键态，**绝不能**用 `DRAG_ARMED` 冒充。
/// 旧实现把 armed 当成按下 → `edge_tick` 在 `if lmb_down() { return }` 永远提前返回，
/// `DRAG_ARMED_MS.swap(0)` / `settle_drag` 永不执行 → 拖到边松手不吸附、不半隐。
#[cfg(target_os = "linux")]
fn lmb_down() -> bool {
    match x11_pointer() {
        Some((_, _, mask)) => (mask & x11_dl::xlib::Button1Mask) != 0,
        // 无 X11：没法等松手。返回 false 让 settle 有机会跑（可能略偏早），
        // 也比「永远不 settle、贴边全废」好——纯 Wayland 场景的降级。
        None => false,
    }
}

/// 窗口所在显示器工作区矩形（l, t, r, b）物理 px——底部按工作区判定，不被任务栏吃掉。
/// `current_monitor()` 对**完全离屏**的窗口返回 None（手速快把球整个甩出屏外的
/// 死锁源：drag_end 与监视循环都拿不到矩形 → 不钳制不救球），此时退化为
/// 「中心离窗口中心最近」的显示器
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn nearest_work_rect(win: &tauri::WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    if let Some(mon) = win.current_monitor().ok().flatten() {
        let wa = mon.work_area();
        let (l, t) = (wa.position.x, wa.position.y);
        return Some((l, t, l + wa.size.width as i32, t + wa.size.height as i32));
    }
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let cx = pos.x + size.width as i32 / 2;
    let cy = pos.y + size.height as i32 / 2;
    let mut best: Option<(i64, (i32, i32, i32, i32))> = None;
    for mon in win.available_monitors().ok()? {
        let wa = mon.work_area();
        let (l, t) = (wa.position.x, wa.position.y);
        let (r, b) = (l + wa.size.width as i32, t + wa.size.height as i32);
        let d = ((cx - (l + r) / 2) as i64).pow(2) + ((cy - (t + b) / 2) as i64).pow(2);
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, (l, t, r, b)));
        }
    }
    best.map(|(_, rect)| rect)
}

/// 分步线性平滑移动窗口（仅位置不改尺寸，透明窗移动无重排开销）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn already_at(a: (i32, i32), b: (i32, i32)) -> bool {
    (a.0 - b.0).abs() <= POS_TOL && (a.1 - b.1).abs() <= POS_TOL
}

/// 停靠几何比对：单轴贴边只要求贴边轴到位。
/// 非贴边轴交给用户放置 / 合成器（Mutter 顶栏 strut、GTK CSD 会改 Y），
/// 两轴都要求 POS_TOL 时，Linux 上会每 100ms 判「漂移」再搬一次 → 上下跳。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn pos_matches_dock(pos: (i32, i32), target: (i32, i32), dx: i32, dy: i32) -> bool {
    let x_ok = (pos.0 - target.0).abs() <= POS_TOL;
    let y_ok = (pos.1 - target.1).abs() <= POS_TOL;
    match (dx != 0, dy != 0) {
        (true, false) => x_ok,
        (false, true) => y_ok,
        _ => x_ok && y_ok,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn slide_to(win: &tauri::WebviewWindow, from: (i32, i32), to: (i32, i32)) {
    if already_at(from, to) {
        return;
    }
    for i in 1..=SLIDE_STEPS {
        let x = from.0 + (to.0 - from.0) * i / SLIDE_STEPS;
        let y = from.1 + (to.1 - from.1) * i / SLIDE_STEPS;
        let _ = win.set_position(PhysicalPosition::new(x, y));
        if i != SLIDE_STEPS {
            std::thread::sleep(std::time::Duration::from_millis(SLIDE_STEP_MS));
        }
    }
}

/// 只在尚未落到目标、且没对同一格搬过时才滑。悬停路径以前每 100ms 重放 80ms 动画，
/// 窗口来回抽 = 上下跳。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn slide_once(
    win: &tauri::WebviewWindow,
    from: (i32, i32),
    to: (i32, i32),
) -> bool {
    use std::sync::atomic::Ordering;
    if already_at(from, to) {
        last_nudge_set(None);
        return false;
    }
    if last_nudge_get() == Some(to) {
        return false;
    }
    last_nudge_set(Some(to));
    LAST_NUDGE_MS.store(now_ms(), Ordering::Relaxed);
    slide_to(win, from, to);
    LAST_DRAG_SETTLE_MS.store(now_ms(), Ordering::Relaxed);
    true
}

/// 按「贴边自动隐藏」语义把球心吸附到工作区边缘（物理 px 口径，边缘监视与拖拽落位共用）：
/// 两轴独立判定，角上可双侧停靠。返回吸附后的球心与方向
/// （dx/dy：+1 = 贴左/上边，-1 = 贴右/下边，0 = 该轴未停靠）。
/// `auto_hide=false` 时不吸附，只回传原值与零方向。
#[cfg(any(target_os = "windows", target_os = "linux"))]
#[allow(clippy::too_many_arguments)]
fn dock_snap(
    cx: f64,
    cy: f64,
    l: f64,
    t: f64,
    r: f64,
    b: f64,
    scale: f64,
    auto_hide: bool,
) -> (f64, f64, i32, i32) {
    if !auto_hide {
        return (cx, cy, 0, 0);
    }
    let mut cx = cx;
    let mut cy = cy;
    let mut dx = 0i32;
    let mut dy = 0i32;
    let trigger = DOCK_TRIGGER * scale;
    let (d_left, d_right) = (cx - l, r - cx);
    let (d_top, d_bottom) = (cy - t, b - cy);
    // 球心距屏边 < trigger → 精确落在屏边、只露半个球体（滑出/隐回交 edge_tick）
    if d_left < trigger && d_left <= d_right {
        cx = l;
        dx = 1;
    } else if d_right < trigger {
        cx = r;
        dx = -1;
    }
    if d_top < trigger && d_top <= d_bottom {
        cy = t;
        dy = 1;
    } else if d_bottom < trigger {
        cy = b;
        dy = -1;
    }
    (cx, cy, dx, dy)
}

/// 关闭贴边自动隐藏时：停靠中的球从半隐位拉回完全屏内，并同步记忆球心
/// （否则关了开关球反而卡在屏边缺一半）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn unpop_to_inside(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    if !win.is_visible().unwrap_or(false) || is_expanded(&win) {
        return;
    }
    let Ok(pos) = win.outer_position() else { return };
    let Ok(size) = win.outer_size() else { return };
    let Some((l, t, r, b)) = nearest_work_rect(&win) else { return };
    let w = size.width.min(size.height) as i32;
    let nx = pos.x.clamp(l, (r - w).max(l));
    let ny = pos.y.clamp(t, (b - w).max(t));
    if (nx, ny) != (pos.x, pos.y) {
        let _ = win.set_position(PhysicalPosition::new(nx, ny));
        let _guard = config::lock();
        let mut cfg = config::load();
        cfg.floating_ball_x = Some((nx + w / 2) as f64);
        cfg.floating_ball_y = Some((ny + w / 2) as f64);
        let ok = config::save(&cfg).is_ok();
        if ok {
            memo_ball_set((nx + w / 2) as f64, (ny + w / 2) as f64);
        }
    }
}

/// 半隐停靠位的窗口左上角（物理 px）：记忆球心位 - 半边长，再向屏内多露 peek。
/// edge_tick 与 drag_end 必须共用本函数（约定 42：两处口径分叉会互相判成「位置漂移」）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn dock_hidden_pos(cx: f64, cy: f64, dx: i32, dy: i32, half: f64, peek: i32) -> (i32, i32) {
    (
        (cx - half).round() as i32 + dx * peek,
        (cy - half).round() as i32 + dy * peek,
    )
}

/// 整窗落在工作区内的左上角。Linux WM（Mutter/KWin）会拒绝半截出屏的坐标，
/// 用这个结果当半隐目标，才不会和监视循环对打。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn clamp_window_into_work(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) -> (i32, i32) {
    (
        x.clamp(l, (r - w).max(l)),
        y.clamp(t, (b - h).max(t)),
    )
}

/// Windows 允许半截出屏，原样返回；Linux 钳到工作区内（可落地的贴边目标）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn landable_window_pos(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) -> (i32, i32) {
    if cfg!(target_os = "linux") {
        clamp_window_into_work(x, y, w, h, l, t, r, b)
    } else {
        let _ = (w, h, l, t, r, b);
        (x, y)
    }
}

/// 停靠态可落点：Linux 只钳「发生停靠的轴」。
/// 左右贴边若连 Y 一起钳，靠近顶栏时半隐 Y 被拽到 work_area.top，
/// 合成器再把窗往下挤，监视循环就在两个 Y 之间跳。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn landable_dock_pos(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
    dx: i32,
    dy: i32,
) -> (i32, i32) {
    if !cfg!(target_os = "linux") {
        let _ = (w, h, l, t, r, b);
        return (x, y);
    }
    (
        if dx != 0 { x.clamp(l, (r - w).max(l)) } else { x },
        if dy != 0 { y.clamp(t, (b - h).max(t)) } else { y },
    )
}

/// 位置纠正（自由位/停靠位补齐）的去重状态：目标是哪一格 + 何时搬的。
/// **到达时不清空**——否则「WM 把窗口推开」会被判成「还没搬过」而无限重搬。
/// 只有目标换了（用户真的把球挪到别处）才重新武装；超时未到位即采纳现位。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn posfix_state() -> &'static std::sync::Mutex<(Option<(i32, i32)>, u64)> {
    static V: std::sync::OnceLock<std::sync::Mutex<(Option<(i32, i32)>, u64)>> =
        std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new((None, 0)))
}

/// 位置纠正的结果：决定调用方是「搬了/采纳了要收手」还是「本次不管」
#[cfg(any(target_os = "windows", target_os = "linux"))]
enum PosFix {
    /// 刚按目标搬了窗（或超时采纳了现位）→ 本跳收手
    Handled,
    /// 已在途中 / 已在目标上 → 本跳不搬，按当前位置继续判定
    InFlight,
}

/// 位置纠正的统一闸门：同一格目标只尝试一次；超过 `NUDGE_ADOPT_MS` 仍未到位就
/// 采纳窗口现位（WM 不接受该坐标，例如半隐位越出工作区被钳回）并停手。
/// **这是「球上下跳」的根治点**：没有它，监测循环会按冷却节奏反复 SetWindowPos。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn posfix_nudge(
    win: &tauri::WebviewWindow,
    pos: PhysicalPosition<i32>,
    half: i32,
    target: (i32, i32),
    label: &str,
    dx: i32,
    dy: i32,
) -> PosFix {
    use std::sync::atomic::Ordering;
    if pos_matches_dock((pos.x, pos.y), target, dx, dy) {
        return PosFix::InFlight;
    }
    let now = now_ms();
    let mut st = posfix_state().lock().unwrap_or_else(|e| e.into_inner());
    let (prev_target, prev_ms) = *st;
    if prev_target == Some(target) {
        if now.saturating_sub(prev_ms) < NUDGE_ADOPT_MS {
            // 搬过、还在生效窗口内：不重复搬（重复 = 每 100ms 抖一次）
            return PosFix::InFlight;
        }
        // 等够了还是没到位 → 采纳现位，记忆向现实收敛
        adopt_current_center(win, pos, half, target);
        *st = (None, 0);
        return PosFix::Handled;
    }
    // 每跳都可能命中：必须 debug 级（默认 Info 过滤下静默），否则 Linux 上会刷屏
    log::debug!(
        "[悬浮球] {label}: 窗口=({},{}) 目标=({},{}) 尺寸={:?} 内部={:?} scale={:.3}",
        pos.x,
        pos.y,
        target.0,
        target.1,
        win.outer_size().ok(),
        win.inner_size().ok(),
        window_scale(win),
    );
    *st = (Some(target), now);
    drop(st);
    LAST_DRAG_SETTLE_MS.store(now, Ordering::Relaxed);
    slide_to(win, (pos.x, pos.y), target);
    PosFix::Handled
}

/// 悬停滑出目标。Linux WM 不允许半截出屏，半隐已经贴边全显——再往里滑会把窗口
/// 从光标下抽走，下一跳又隐回。所以 Linux 滑出位 = 半隐位（贴边停住、不做悬停动画）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn dock_hover_pos(
    hidden: (i32, i32),
    dx: i32,
    dy: i32,
    off: i32,
    w: i32,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) -> (i32, i32) {
    if cfg!(target_os = "linux") {
        let _ = (dx, dy, off, w, l, t, r, b);
        hidden
    } else {
        (
            (hidden.0 + dx * off).clamp(l, (r - w).max(l)),
            (hidden.1 + dy * off).clamp(t, (b - w).max(t)),
        )
    }
}

/// 最近一次「落位补齐 / 隐回」的目标。WM 钳回后窗口到不了这个点，再搬只会跳动；
/// 记下目标，下一跳发现仍不在几何上就采纳现位并冷却。
/// **只服务悬停动画（`slide_once`）**：到达即清空，让下一次悬停能重新播动画。
/// 位置纠正（自由位/停靠位补齐）不走这里——那里到达后清空会让「WM 推开 → 标记已清
/// → 再搬」形成无限循环（实测每秒 6~10 条「落位补齐」，球可见上下跳）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn last_nudge_target() -> &'static std::sync::Mutex<Option<(i32, i32)>> {
    static V: std::sync::OnceLock<std::sync::Mutex<Option<(i32, i32)>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn last_nudge_get() -> Option<(i32, i32)> {
    *last_nudge_target().lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn last_nudge_set(v: Option<(i32, i32)>) {
    *last_nudge_target().lock().unwrap_or_else(|p| p.into_inner()) = v;
}

/// 自由位目标 = 记忆球心 − 半边长，再按平台钳进工作区（Linux WM 只接受工作区内坐标）。
/// 抽成纯函数便于回归：日志实证「记忆球心=(2317,330)、工作区底=343」时，原始目标
/// y=280 会被 WM 钳成 243，于是监视循环每 800ms 重搬一次（球上下跳）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn free_ball_pos(
    cx: f64,
    cy: f64,
    half: i32,
    w: i32,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) -> (i32, i32) {
    landable_window_pos(
        (cx - half as f64).round() as i32,
        (cy - half as f64).round() as i32,
        w,
        w,
        l,
        t,
        r,
        b,
    )
}

/// 窗口所在显示器的整屏矩形（含任务栏），用于与工作区矩形对照诊断
/// （工作区 = 整屏扣面板/struts；两者相差悬殊说明是面板占位，整屏就小说明显示器本身小）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn monitor_rect(win: &tauri::WebviewWindow) -> Option<(i32, i32, i32, i32)> {
    let mon = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())?;
    let p = mon.position();
    let s = mon.size();
    Some((
        p.x,
        p.y,
        p.x + s.width as i32,
        p.y + s.height as i32,
    ))
}

/// 目标搬不动时采纳窗口现位：把实际球心写回记忆，并打全几何以便定位根因。
/// Linux WM 会把窗口钳在工作区内，于是「记忆位/半隐位」可能是个它永不接受的点——
/// 记着它只会让监视循环按冷却节奏反复搬（日志实证每秒 6~10 条「落位补齐」，用户
/// 看到的就是球上下跳）。窗口位置是系统给的既成事实，记忆向它收敛，循环立即静止。
/// 末尾清掉 last_nudge 并武装冷却，避免采纳后又立刻被别的分支当成漂移。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn adopt_current_center(
    win: &tauri::WebviewWindow,
    pos: PhysicalPosition<i32>,
    half: i32,
    target: (i32, i32),
) {
    use std::sync::atomic::Ordering;
    let ncx = pos.x + half;
    let ncy = pos.y + half;
    let work = nearest_work_rect(win);
    // 采纳只在「目标搬不动」时发生；Linux 上每轮贴边都可能触发 → debug（默认 Info 下静默）
    log::debug!(
        "[悬浮球] 落位采纳: 窗口=({},{}) 尺寸={:?} 内部={:?} 半边长={} scale={:.3} 工作区={:?} 整屏={:?} 目标=({},{}) 未生效 → 球心改为 ({},{})",
        pos.x,
        pos.y,
        win.outer_size().ok(),
        win.inner_size().ok(),
        half,
        window_scale(win),
        work,
        monitor_rect(win),
        target.0,
        target.1,
        ncx,
        ncy
    );
    {
        let _guard = config::lock();
        let mut cur = config::load();
        cur.floating_ball_x = Some(ncx as f64);
        cur.floating_ball_y = Some(ncy as f64);
        if config::save(&cur).is_ok() {
            memo_ball_set(ncx as f64, ncy as f64);
        }
    }
    last_nudge_set(None);
    LAST_DRAG_SETTLE_MS.store(now_ms(), Ordering::Relaxed);
}

/// 边缘监视单跳：对照「半隐位/滑出位」与系统光标，决定滑出或隐回。
/// 停靠身份必须「记忆球心」与「窗口实际位置」双方一致才成立——只信记忆会把
/// 用户刚拖走的球按旧记忆拽回屏边（表现为拖到任意位置都弹回去）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn edge_tick(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let Some(win) = app.get_webview_window(LABEL) else { return };
    if !win.is_visible().unwrap_or(false) {
        return;
    }
    // 左键按下 = 原生拖拽/按住操作中，窗口位置正被模态循环接管，跳过本跳
    if lmb_down() {
        return;
    }
    // 左键已释放且拖拽标志还在 = 拖拽刚结束的第一跳：模态循环已退出、窗口位置
    // 已稳定，在这里统一做钳制/吸附/落位/写记忆（DRAG_ARMED_MS 注释：为什么松手
    // 检测必须在这里而不是 startDragging 的 promise；超 TTL 的残留标志作废不消费）
    let armed = DRAG_ARMED_MS.swap(0, Ordering::Relaxed);
    if armed > 0 && now_ms().saturating_sub(armed) < DRAG_ARMED_TTL_MS {
        settle_drag(app);
        return;
    }
    let Ok(pos) = win.outer_position() else { return };
    let Ok(size) = win.outer_size() else { return };
    let w = size.width.min(size.height) as i32;
    let half = w / 2;
    // 救球（无条件，AUTO_HIDE 关闭也生效）：球心被甩出工作区 = 手速快把球整个拖出
    // 屏外的死锁场景——窗口完全离屏时 current_monitor 返回 None，drag_end 的钳制
    // 整段被跳过、记忆也写成屏外坐标，球点不到也拖不回。钳回工作区 + 写记忆。
    // 半隐/角落停靠的球心恰在边缘线上（含边界），不会误触发
    if let Some((l, t, r, b)) = nearest_work_rect(&win) {
        let (cx, cy) = (pos.x + half, pos.y + half);
        // 只把「整球飞出工作区」的轴钳回来。贴边轴上球心本就在边上；
        // 非贴边轴被顶栏 strut / CSD 挤出几像素不能当飞出，否则左右贴边会上下跳。
        let overflow_x = cx < l - POS_TOL || cx > r + POS_TOL;
        let overflow_y = cy < t - POS_TOL || cy > b + POS_TOL;
        if overflow_x || overflow_y {
            let ncx = if overflow_x { cx.clamp(l, r.max(l)) } else { cx };
            let ncy = if overflow_y { cy.clamp(t, b.max(t)) } else { cy };
            let (nx, ny) = landable_dock_pos(
                ncx - half,
                ncy - half,
                w,
                w,
                l,
                t,
                r,
                b,
                i32::from(overflow_x),
                i32::from(overflow_y),
            );
            // 已经在可落点：多半是 WM 把半隐钳回来了，再搬就是和它对打。
            if already_at((pos.x, pos.y), (nx, ny)) {
                return;
            }
            let _ = win.set_position(PhysicalPosition::new(nx, ny));
            {
                let _guard = config::lock();
                let mut cur = config::load();
                cur.floating_ball_x = Some(ncx as f64);
                cur.floating_ball_y = Some(ncy as f64);
                if config::save(&cur).is_ok() {
                    memo_ball_set(ncx as f64, ncy as f64);
                }
            }
            LAST_DRAG_SETTLE_MS.store(now_ms(), Ordering::Relaxed);
            log::info!(
                "[悬浮球] 救球: 窗口=({},{}) 球心=({},{}) 出工作区 → 钳回 ({},{})",
                pos.x,
                pos.y,
                cx,
                cy,
                ncx,
                ncy
            );
            return;
        }
    }
    if !AUTO_HIDE.load(Ordering::Relaxed) {
        return;
    }
    // 菜单展开态：窗口几何归 expand/收起流程管，监视不插队
    if is_expanded(&win) {
        return;
    }
    // 拖拽刚落位：窗口搬移（异步 IPC）可能还没被主线程处理完，此时读到的位置是旧的，
    // 任何判定都会失真——冷却期内整跳跳过（见 DRAG_SETTLE_COOLDOWN_MS）
    if now_ms().saturating_sub(LAST_DRAG_SETTLE_MS.load(Ordering::Relaxed))
        < DRAG_SETTLE_COOLDOWN_MS
    {
        return;
    }
    // 记忆球心走进程内缓存（100ms 一跳不能每跳读盘）；未初始化时回落读盘填充一次
    let (cx, cy) = match memo_ball_get() {
        Some(v) => v,
        None => {
            let cfg = config::load();
            match (cfg.floating_ball_x, cfg.floating_ball_y) {
                (Some(x), Some(y)) => {
                    memo_ball_set(x, y);
                    (x, y)
                }
                _ => return,
            }
        }
    };
    let Some((l, t, r, b)) = nearest_work_rect(&win) else { return };
    let scale = window_scale(&win);
    // 停靠边（可同时双侧=角落斜隐）：记忆球心落在该侧工作区边缘容差内
    let dock_left = (cx - l as f64).abs() <= DOCK_TOL;
    let dock_right = (r as f64 - cx).abs() <= DOCK_TOL;
    let dock_top = (cy - t as f64).abs() <= DOCK_TOL;
    let dock_bottom = (b as f64 - cy).abs() <= DOCK_TOL;
    if !(dock_left || dock_right || dock_top || dock_bottom) {
        // 自由位记忆：窗口被搬丢（落位 IPC 丢失/被后续消息覆盖）时补齐到记忆位，
        // 记忆不动——球被拖走必经 drag_end 重写记忆，轮询期间记忆不可能过期。
        // Linux：目标先钳进工作区（WM 会把越界坐标直接钳回来，拿它当目标就是每跳
        // 对打一次——日志实证「记忆球心=(2317,330) 窗口=(2267,243) → 搬到 (2267,280)」
        // 每秒刷一条，窗口下沿被 WM 钉在工作区底=343，则 280 永远到不了）。
        let (mx, my) = free_ball_pos(cx, cy, half, w, l, t, r, b);
        // 统一闸门：同一格只搬一次，超时未到位则采纳现位（见 posfix_nudge）
        let _ = posfix_nudge(&win, pos, half, (mx, my), "落位补齐", 0, 0);
        return;
    }
    let dx = i32::from(dock_left) - i32::from(dock_right);
    let dy = i32::from(dock_top) - i32::from(dock_bottom);
    // 滑出量 = 窗口半边长：滑出后整窗完全落在屏内，球体 + 粒子云 + 最外圈陀螺环
    // （视觉半径 47 逻辑 px）一并完整露出。曾按球半径 BALL_R 只平移 24px，窗口仍有
    // 26px 留在屏外，悬停只露出球体一小半、粒子永远看不见（用户反馈）。
    // 滑出位向对侧 clamp（贴角时不至于整窗越出工作区）
    let off = half;
    // 半隐位 = 记忆球心位 - 半边长，再向屏内多露 PEEK（见常量注释：纯压边只露 24px 弧）
    let peek = (PEEK * scale).round() as i32;
    let hidden_raw = dock_hidden_pos(cx, cy, dx, dy, half as f64, peek);
    let hidden = landable_dock_pos(hidden_raw.0, hidden_raw.1, w, w, l, t, r, b, dx, dy);
    let popped = dock_hover_pos(hidden, dx, dy, off, w, l, t, r, b);
    let at_hidden = pos_matches_dock((pos.x, pos.y), hidden, dx, dy);
    let at_popped = pos_matches_dock((pos.x, pos.y), popped, dx, dy);
    let mut treat_as_hidden = at_hidden;
    if !at_hidden && !at_popped {
        // 窗口不在停靠几何上 = drag_end 的落位搬窗丢了（异步 IPC 在拖动刚结束的
        // ~200ms 内被吞/被覆盖，日志实证：补搬一次后纠偏读到的仍是旧位置）。
        // **记忆是唯一真相**（球被拖走必经 drag_end 重写记忆），窗口向记忆收敛：
        // 光标在窗口屏内可见区（松手时通常正停在球上）→ 直接落滑出位，否则落半隐位。
        // 绝不反向改记忆——旧逻辑「以窗口为准改记」会把刚吸附的位置改漂
        // （日志曾见记忆 y 在 446→435→416→365 间乱跳）
        let Some((px, py)) = cursor_pos() else { return };
        let inside = px >= pos.x.max(l)
            && px < (pos.x + w).min(r)
            && py >= pos.y.max(t)
            && py < (pos.y + w).min(b);
        let target = if inside { popped } else { hidden };
        let label = if inside { "落位补齐(滑出位)" } else { "落位补齐(半隐位)" };
        match posfix_nudge(&win, pos, half, target, label, dx, dy) {
            PosFix::Handled => return,
            // 已在途中 / 已在目标上：当半隐态处理，本跳不重复搬（否则就是 100ms 抖）
            PosFix::InFlight => treat_as_hidden = true,
        }
    } else {
        // 到位则清空悬停动画去重标记（下一次悬停要能重新播动画）
        last_nudge_set(None);
    }
    let popped_now = !treat_as_hidden && at_popped;
    let Some((px, py)) = cursor_pos() else { return };
    if !popped_now {
        let inside = px >= pos.x.max(l)
            && px < (pos.x + w).min(r)
            && py >= pos.y.max(t)
            && py < (pos.y + w).min(b);
        if inside {
            log::debug!(
                "[悬浮球] 悬停滑出: 窗口=({},{}) 滑出位=({},{}) 光标=({},{}) 停靠方向=({},{})",
                pos.x,
                pos.y,
                popped.0,
                popped.1,
                px,
                py,
                dx,
                dy
            );
            slide_once(&win, (pos.x, pos.y), popped);
        }
    } else {
        let outside = px < pos.x - EDGE_MARGIN
            || px >= pos.x + w + EDGE_MARGIN
            || py < pos.y - EDGE_MARGIN
            || py >= pos.y + w + EDGE_MARGIN;
        if outside {
            slide_once(&win, (pos.x, pos.y), hidden);
        }
    }
}

/// 启动边缘监视线程（进程内仅一次；开关由 AUTO_HIDE 原子量控制，循环常驻空转成本可忽略）
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn start_edge_watch(app: &AppHandle) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    let handle = app.clone();
    STARTED.call_once(move || {
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(EDGE_POLL_MS));
            edge_tick(&handle);
        });
    });
}

/// 窗口实时 DPI 缩放系数。
/// Windows：GetDpiForWindow（与 WebView2 同源）；Linux：tao 的 scale_factor。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn window_scale(win: &tauri::WebviewWindow) -> f64 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
        if let Ok(hwnd) = win.hwnd() {
            let dpi = unsafe { GetDpiForWindow(hwnd.0) };
            if dpi > 0 {
                return dpi as f64 / 96.0;
            }
        }
    }
    win.scale_factor().unwrap_or(1.0)
}

// ---------- 生命周期 ----------

/// 启动时预创建悬浮球窗口并隐藏常驻（与 clipboard 浮层同模式，见 ADR 0004「非惰性创建」）。
/// 停用/启用只切窗口显隐，绝不运行时销毁重建——运行时现场创建/销毁 WebView2 窗口
/// 是主线程长任务，曾与悬浮球窗口操作交错导致整窗未响应（WebView2 controller 创建挂起，
/// 见 clipboard.rs::init_overlay_window 与 lib.rs 启动注释的同款坑）。
/// 必须在 autostart-hidden 的 tray::hide_window 之后调用，显隐联动才正确。
pub fn init(app: &AppHandle) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use std::sync::atomic::Ordering;
        if let Err(e) = ensure_window(app) {
            log::warn!("悬浮球窗口创建失败: {}", e);
            return;
        }
        let cfg = config::load();
        AUTO_HIDE.store(cfg.floating_ball_auto_hide, Ordering::Relaxed);
        // 几何基线一次性落日志：排查「球停在意外位置 / 反复搬窗」时先看这条
        // （工作区远小于整屏 = 面板 struts 占位；记忆球心落在工作区外 = WM 不接受它）
        if let Some(win) = app.get_webview_window(LABEL) {
            log::info!(
                "[悬浮球] 几何基线(build={}): 工作区={:?} 整屏={:?} scale={:.3} 窗口尺寸={:?} 内部={:?} 位置={:?} 记忆球心=({:?},{:?})",
                option_env!("XHUB_BUILD_TAG").unwrap_or("unknown"),
                nearest_work_rect(&win),
                monitor_rect(&win),
                window_scale(&win),
                win.outer_size().ok(),
                win.inner_size().ok(),
                win.outer_position().ok(),
                cfg.floating_ball_x,
                cfg.floating_ball_y
            );
        }
        start_edge_watch(app);
        if !cfg.floating_ball_enabled {
            // 停用状态：窗口隐藏常驻，设置启用时直接 show 即可
            if let Some(win) = app.get_webview_window(LABEL) {
                let _ = win.hide();
            }
            return;
        }
        sync_with_main(app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn ensure_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let mut builder =
        // 轻量入口 ball.html（P2）：只渲染球体，不加载完整 SPA（内存优化，见 src/light/ball.ts）
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("ball.html".into()))
            .title("悬浮球")
            .inner_size(BALL_SIZE, BALL_SIZE)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(true)
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);
    #[cfg(not(target_os = "windows"))]
    let builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("ball.html".into()))
            .title("悬浮球")
            .inner_size(BALL_SIZE, BALL_SIZE)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(true)
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);
    // 透明窗口不能同时启用系统阴影：Windows 会画黑边；GTK 阴影会撑大
    // outer_size（实测 +37px），贴边监视把尺寸差当成位置漂移而跳动。
    let builder = builder.shadow(false);
    let win = builder.build()?;

    // 彻底不进任务栏：tao 的 skip_taskbar 只是一次性 DeleteTab，窗口仍带 WS_EX_APPWINDOW，
    // 会在 hide/show 后重新长出任务栏按钮（见 win_taskbar 模块注释）
    crate::win_taskbar::apply(&win);

    place_initial(app, &win);

    // 悬浮球不响应关闭请求（Alt+F4 等）：隐藏即可，窗口常驻复用
    // （非惰性创建，避免每次唤出的窗口创建延迟，见 ADR 0004）
    let handle = app.clone();
    win.on_window_event(move |event| {
        match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(w) = handle.get_webview_window(LABEL) {
                    let _ = w.hide();
                }
            }
            // DPI 变化：tao 会按建议矩形缩放窗口，但悬浮球常驻隐藏，隐藏期间可能错过
            // WM_DPICHANGED（或 Win10/远程会话关闭「拖动时显示窗口内容」时 tao 跳过
            // 尺寸缩放）——收到本事件立即按当前态重算几何，把物理尺寸拉回实时 DPI
            // 对应值（失配会让球体陀螺环被窗口边缘裁掉一截，见 apply_geometry 注释）
            tauri::WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = handle.get_webview_window(LABEL) {
                    apply_geometry(&w, is_expanded(&w), None);
                }
            }
            _ => {}
        }
    });

    log::info!("悬浮球窗口已创建");
    Ok(())
}

/// 初始位置：优先用记忆的球心（物理 px），否则主显示器右下角（球缘距屏边 SNAP_GAP，
/// 与吸附同语义）。球态窗口位置 = 球心 - 半边长。
/// 注意：floating_ball_x/y 存球心坐标（旧版本存的是窗口左上角，升级后首次
/// 位置会偏移一次，拖动一下即按新语义记忆）。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn place_initial(app: &AppHandle, win: &tauri::WebviewWindow) {
    let scale = window_scale(win);
    let half = (BALL_SIZE * scale / 2.0).round() as i32;
    let cfg = config::load();
    if let (Some(x), Some(y)) = (cfg.floating_ball_x, cfg.floating_ball_y) {
        if crate::is_position_on_screen(x, y) {
            let _ = win.set_position(PhysicalPosition::new(
                (x - half as f64).round() as i32,
                (y - half as f64).round() as i32,
            ));
            return;
        }
    }
    if let Ok(Some(mon)) = app.primary_monitor() {
        let gap = (SNAP_GAP * scale).round() as i32;
        let ball_r = (BALL_R * scale).round() as i32;
        // 默认右下角：按工作区（扣除任务栏）定位，球缘距工作区右/下各 SNAP_GAP，
        // 否则任务栏在底部时默认位会被任务栏盖住
        let wa = mon.work_area();
        let cx = wa.position.x + wa.size.width as i32 - ball_r - gap;
        let cy = wa.position.y + wa.size.height as i32 - ball_r - gap;
        let _ = win.set_position(PhysicalPosition::new(cx - half, cy - half));
    }
}

/// 与主窗口显隐联动：主窗显示且未开「同显」→ 球隐藏；其余情况 → 球显示。
/// 主窗的所有 show/hide 都走 tray::show_window / hide_window，统一钩到这里。
pub fn sync_with_main(app: &AppHandle) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let cfg = config::load();
        if !cfg.floating_ball_enabled {
            return;
        }
        let Some(win) = app.get_webview_window(LABEL) else {
            return;
        };
        // 主窗视觉不可见 = 隐藏到托盘 ∨ 最小化
        let main_visible = crate::tray::is_main_window_visible()
            && !MAIN_MINIMIZED.load(std::sync::atomic::Ordering::SeqCst);
        // 球保持显示：主窗不可见 ∨ 设置开启「与主窗口同时显示」
        let keep_ball = !main_visible || cfg.floating_ball_with_main;
        if keep_ball {
            // 曾以菜单态被隐藏时先回到球态几何再显示（隐藏期间收拢，跳动不可见）
            apply_geometry(&win, false, None);
            // 显示后必须重新摘掉任务栏按钮（tao 每次 VISIBLE 变化都会重建 ex-style，
            // 把 WS_EX_APPWINDOW 加回来——见 win_taskbar 模块注释）
            crate::webview_mem::on_shown(app, LABEL);
            crate::win_taskbar::show(&win);
            // 通知页面复位菜单态（几何已在上面收拢）
            use tauri::Emitter;
            let _ = app.emit_to(LABEL, "floating-ball-shown", ());
        } else {
            let _ = win.hide();
            crate::webview_mem::on_hidden(app, LABEL);
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
    }
}

// ---------- 几何管理 ----------

/// 当前是否处于菜单展开态（按窗口实际尺寸判断，免维护额外状态）
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn is_expanded(win: &tauri::WebviewWindow) -> bool {
    let scale = window_scale(win);
    win.outer_size()
        .map(|sz| sz.width as f64 > BALL_SIZE * scale * 1.5)
        .unwrap_or(false)
}

/// 以「球心」（窗口中心）为锚调整窗口几何：球态 = BALL_SIZE，菜单态 = MENU_SIZE。
/// 展开时钳制到所在显示器内（空间自适应挪位，收起后自然回到吸附位置）。
///
/// DPI 失配自愈（见模块注释）：球心一律按窗口「实际」outer_size 的一半推算（视觉
/// 球心 = 窗口中心，与换算方式无关），目标尺寸按实时 DPI 重算——窗口物理尺寸偏离
/// 期望（隐藏期间错过 WM_DPICHANGED 等）时，任何一次开合/显示都会被本函数拉回：
/// SetWindowPos 同时修正尺寸 → WM_SIZE → wry 同步 WebView2 bounds → 视口恢复。
/// 换用 tao 缓存 scale 的话，缓存过期时目标尺寸永远算成旧值，失配无法自愈。
/// `scale_override`：前端视口实测 DPI（窗口物理宽 / CSS 视口宽）。窗口 DPI 上下文
/// 过期（隐藏期间错过 WM_DPICHANGED 且 GetDpiForWindow 仍返回旧值）时，
/// window_scale 算出的目标物理尺寸依然偏小 → WebView2 按真实光栅 DPI 渲染，
/// 视口 < 逻辑尺寸，菜单按钮外圈被窗口边缘裁掉。此时以「视口实测」为唯一真相，
/// 前端 checkViewportSync 失配时携带 clientWidth 调 floating_ball_reapply 自愈。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn apply_geometry(win: &tauri::WebviewWindow, expanded: bool, scale_override: Option<f64>) {
    let Ok(pos) = win.outer_position() else { return };
    let scale = scale_override.unwrap_or_else(|| window_scale(win));
    let was_expanded = is_expanded(win);

    // 当前球心 = 窗口实际中心；取不到实际尺寸（极端）才退回逻辑换算
    let cur_half = match win.outer_size() {
        Ok(sz) if sz.width > 0 && sz.height > 0 => {
            (sz.width as f64 + sz.height as f64) / 4.0
        }
        _ => (if was_expanded { MENU_SIZE } else { BALL_SIZE } / 2.0) * scale,
    };
    let cx = pos.x as f64 + cur_half;
    let cy = pos.y as f64 + cur_half;

    let new_size = ((if expanded { MENU_SIZE } else { BALL_SIZE }) * scale).round();
    let new_half = new_size / 2.0;
    let mut nx = cx - new_half;
    let mut ny = cy - new_half;

    // 展开时记住原球心；收起时优先恢复（展开被钳制挪位后，球回到原吸附点而非漂移）。
    // 存球心而非窗口左上角：恢复时按目标半边长重新定位，与窗口实际尺寸/DPI 无关
    // （DPI 在展开期间变化时按左上角恢复会让球心漂移半边长差）。
    // 已处于展开态时跳过记录（收回动画期间重复 expand 不覆盖原始吸附点）
    let mut pre = PRE_EXPAND_POS.lock().unwrap_or_else(|e| e.into_inner());
    if expanded {
        if !was_expanded {
            *pre = Some((cx.round() as i32, cy.round() as i32));
        }
    } else if was_expanded {
        // 只有「确实刚从展开态收拢」才恢复展开前球心。窗口本来就是球态时，
        // pre 可能残留自上一次展开（如收起后隐藏、再显示触发本函数）——那时
        // 按它定位会把球挪到与记忆无关的位置，随后监视循环又照记忆搬回来，
        // 两者相差半边长（日志实证：窗口反复停在 243 与 280 之间 = 上下跳）。
        if let Some((px, py)) = pre.take() {
            nx = px as f64 - new_half;
            ny = py as f64 - new_half;
        }
    } else {
        *pre = None;
    }
    drop(pre);

    // 钳制只在菜单展开态做（保证大圆完整显示）。球态**不可钳**：贴边半隐位的窗口
    // 左上角本来就在屏幕外（x 为负或超出右缘），钳回屏内会让收起菜单后的球离开半隐位，
    // 边缘监视随即判成「位置漂移」再搬回去——表现为收起菜单后球弹一下
    if expanded {
        // 钳到工作区（扣任务栏）而非整屏：贴底边停靠时展开的菜单下缘不得伸进任务栏
        // （6 点方向按钮会被任务栏遮挡）——与 dock_snap/救球/落位同口径（约定 42）；
        // 球态不可钳（见上），钳制只发生在展开态
        if let Some((wl, wt, wr, wb)) = nearest_work_rect(win) {
            let m = 4.0 * scale;
            let min_x = wl as f64 + m;
            let min_y = wt as f64 + m;
            // 小屏保护：max 可能小于 min（f64::clamp 在 min > max 时 panic）
            let max_x = (wr as f64 - new_size - m).max(min_x);
            let max_y = (wb as f64 - new_size - m).max(min_y);
            nx = nx.clamp(min_x, max_x);
            ny = ny.clamp(min_y, max_y);
        }
    }

    // 原子应用尺寸+位置（单次 SetWindowPos）：拆成 set_size + set_position 会让窗口
    // 先单向长大再挪回（球心瞬移），WebView2 还要做两次重排——展开卡顿的一部分
    let expect =
        ((if was_expanded { MENU_SIZE } else { BALL_SIZE }) * scale).round() as i32;
    if let Ok(sz) = win.outer_size() {
        let actual = sz.width as i32;
        // 失配修正诊断日志：物理尺寸偏离「逻辑×实时DPI」说明经历过 DPI 失配，
        // 本次调用即自愈（用户反馈「两侧被遮盖」时先查这条日志）
        if (actual - expect).abs() > 1 {
            log::info!(
                "[悬浮球] DPI 失配自愈: 窗口物理 {} → 期望 {} (scale {:.4})",
                actual,
                expect,
                scale
            );
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
            };
            unsafe {
                SetWindowPos(
                    hwnd.0,
                    std::ptr::null_mut(),
                    nx.round() as i32,
                    ny.round() as i32,
                    new_size as i32,
                    new_size as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            return;
        }
    }
    let _ = win.set_size(PhysicalSize::new(new_size as u32, new_size as u32));
    let _ = win.set_position(PhysicalPosition::new(nx.round() as i32, ny.round() as i32));
}

/// 设置变更后的应用：启用则确保窗口存在并联动显隐；停用只隐藏、不销毁窗口。
/// 窗口由启动 init 预创建后常驻——运行时 destroy/rebuild WebView2 与悬浮球窗口
/// 操作交错会卡死整窗（见 init 注释），与 clipboard 浮层「预创建隐藏常驻」同款约束
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn apply_enabled(app: &AppHandle, enabled: bool) {
    if enabled {
        // 正常路径窗口启动时已预创建；此处 ensure 仅兜底极少见的缺失场景
        if let Err(e) = ensure_window(app) {
            log::warn!("悬浮球窗口创建失败: {}", e);
            return;
        }
        sync_with_main(app);
    } else if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
        log::info!("悬浮球已停用（窗口隐藏常驻）");
    }
}

// ---------- Tauri 命令 ----------

/// 命令名 = 函数名（Tauri v2 注册规则），前端统一以 `floating_ball_*` 调用，
/// 故函数名带模块前缀，与 src/api/tauri.ts 的 invoke 名一一对应。
#[tauri::command]
pub fn floating_ball_get_state(app: tauri::AppHandle) -> FloatingBallState {
    let cfg = config::load();
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let dock = app
        .get_webview_window(LABEL)
        .map(|w| dock_state(&w, cfg.floating_ball_x, cfg.floating_ball_y))
        .unwrap_or_default();
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let dock = {
        let _ = &app;
        DockState::default()
    };
    FloatingBallState {
        enabled: cfg.floating_ball_enabled,
        auto_hide: cfg.floating_ball_auto_hide,
        with_main: cfg.floating_ball_with_main,
        buttons: cfg.floating_ball_buttons,
        x: cfg.floating_ball_x,
        y: cfg.floating_ball_y,
        idle_spin: cfg.floating_ball_idle_spin,
        dock,
        ball_size: BALL_SIZE,
        menu_size: MENU_SIZE,
    }
}

/// 保存设置并立即生效（窗口创建/销毁、显隐联动；按钮列表去重 + 钳制上限）。
/// 独立于 save_config：位置等 Rust 侧字段不经过前端快照，避免互相覆盖。
#[tauri::command]
pub fn floating_ball_save_settings(
    app: AppHandle,
    enabled: bool,
    auto_hide: bool,
    with_main: bool,
    buttons: Vec<String>,
    idle_spin: bool,
) -> Result<(), String> {
    let auto_hide = config::floating_ball_auto_hide_for(auto_hide, cfg!(target_os = "linux"));
    // 去重保序 + 截断上限
    let mut seen = std::collections::HashSet::new();
    let buttons: Vec<String> = buttons
        .into_iter()
        .filter(|b| seen.insert(b.clone()))
        .take(MAX_BUTTONS)
        .collect();

    {
        let _guard = config::lock();
        let mut cfg = config::load();
        cfg.floating_ball_enabled = enabled;
        cfg.floating_ball_auto_hide = auto_hide;
        cfg.floating_ball_with_main = with_main;
        cfg.floating_ball_buttons = buttons;
        cfg.floating_ball_idle_spin = idle_spin;
        config::save(&cfg)?;
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use std::sync::atomic::Ordering;
        AUTO_HIDE.store(auto_hide, Ordering::Relaxed);
        if !auto_hide {
            // 关闭贴边自动隐藏：停靠中的球拉回完全屏内并同步记忆球心
            unpop_to_inside(&app);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    apply_enabled(&app, enabled);

    // 通知球窗口重拉状态（按钮集/自动隐藏/停靠边一并刷新）
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if enabled {
        use tauri::Emitter;
        let _ = app.emit_to(LABEL, "floating-ball-config-changed", ());
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = (&app, auto_hide);

    log::info!(
        "悬浮球设置已保存: enabled={} auto_hide={} with_main={}",
        enabled,
        auto_hide,
        with_main
    );
    Ok(())
}

/// 拖拽开始：前端在位移超阈值、移交系统原生拖动（startDragging）时调用，仅记录
/// 武装时刻。真正的松手由边缘监视循环检测（左键释放后的第一跳 → settle_drag）——
/// startDragging 的 promise 在拖动开始时就 resolve，不能当结束信号（见 DRAG_ARMED_MS 注释）。
#[tauri::command]
pub fn floating_ball_drag_begin() {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    DRAG_ARMED_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// 拖拽取消：startDragging 启动失败（回退指针收尾路径）时清落位标志，
/// 防止残留标志把用户下一次无关的左键单击松开当成拖拽落位（「带外绝不移动」）
#[tauri::command]
pub fn floating_ball_drag_cancel() {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    DRAG_ARMED_MS.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// 拖拽落位（松手后由 edge_tick 在监视线程调用，此时窗口位置已稳定）：
/// 球心钳在工作区内 + 可选贴边自动隐藏（球心落到屏边、半隐，见模块注释）+ 记忆球心到配置。
/// 拖拽只发生在球态，窗口位置 = 球心 - 球态半边长。
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn settle_drag(app: &AppHandle) {
    use tauri::Emitter;
    use std::sync::atomic::Ordering;
        // 先武装监视冷却再动手：下面 set_position 是异步 IPC，配置却是立即写盘，
        // 监视循环若在间隙轮询会读到旧窗口位置而误判（见 DRAG_SETTLE_COOLDOWN_MS）
        LAST_DRAG_SETTLE_MS.store(now_ms(), Ordering::Relaxed);
        let Some(win) = app.get_webview_window(LABEL) else { return };
        // 松手由本函数检测（模态循环已退出、位置已稳定），无需再等待
        let Ok(pos) = win.outer_position() else { return };
        let scale = window_scale(&win);
        // 半边长优先取窗口「实际」外框尺寸的一半（与 edge_tick 同口径；DPI 失配时
        // 逻辑×scale 会偏，两处不同口径会让彼此判成「位置漂移」）
        let half = match win.outer_size() {
            Ok(sz) if sz.width.min(sz.height) > 0 => sz.width.min(sz.height) as f64 / 2.0,
            _ => (BALL_SIZE * scale / 2.0).round(),
        };
        // 拖拽结束时的球心（球态窗口中心即球心）
        let mut cx = pos.x as f64 + half;
        let mut cy = pos.y as f64 + half;
        // 停靠方向（+1 = 贴左/上边，-1 = 贴右/下边，0 = 未停靠），落位与监视循环共用口径
        let mut dx = 0i32;
        let mut dy = 0i32;
        let mut wa_rect: Option<(i32, i32, i32, i32)> = None;
        // nearest_work_rect：完全离屏时 current_monitor 返回 None（球被甩出屏外的
        // 死锁源），兜底取「中心最近」的显示器，钳制/吸附/落位永远有工作区可用
        if let Some((wl, wt, wr, wb)) = nearest_work_rect(&win) {
            // 以工作区为界（扣除任务栏）：球心落在工作区边缘外会被任务栏盖住
            let mx = wl as f64;
            let my = wt as f64;
            let mr = wr as f64;
            let mb = wb as f64;
            // 球心钳在显示器内（整个球不出屏，否则拖不回来）
            cx = cx.clamp(mx, mr.max(mx));
            cy = cy.clamp(my, mb.max(my));
            wa_rect = Some((wl, wt, wr, wb));
            // 贴边判定与边缘监视共用 dock_snap：球心距屏边 < DOCK_TRIGGER 才吸附，
            // 超过就保持原位——用户「放」下的球不该被拽回屏边
            let auto_hide = config::load().floating_ball_auto_hide;
            let (free_cx, free_cy) = (cx, cy);
            let (sx, sy, sdx, sdy) = dock_snap(cx, cy, mx, my, mr, mb, scale, auto_hide);
            cx = sx;
            cy = sy;
            dx = sdx;
            dy = sdy;
            // 诊断（用户反馈「拖到边上不吸附」时先看这条）：吸附只取决于松手点球心
            // 到工作区边缘的距离是否 < trigger（= DOCK_TRIGGER × scale，物理 px）
            // 每次拖拽松手都会记录（含吸附判定依据）；属诊断信息，debug 级
            log::debug!(
                "[悬浮球] 拖拽松手: 球心=({:.0},{:.0}) 工作区=[{:.0},{:.0},{:.0},{:.0}] scale={:.4} \
                 trigger={:.0} 距边 左{:.0}/右{:.0}/上{:.0}/下{:.0} auto_hide={} → 吸附后球心=({:.0},{:.0}) 方向=({},{})",
                free_cx,
                free_cy,
                mx,
                my,
                mr,
                mb,
                scale,
                DOCK_TRIGGER * scale,
                free_cx - mx,
                mr - free_cx,
                free_cy - my,
                mb - free_cy,
                auto_hide,
                cx,
                cy,
                dx,
                dy
            );
        }
        // 落位：停靠时若光标仍停在球体半隐可见区内，直接落在滑出位（松手先闪半隐
        // 再滑出的观感很怪），由监视循环在光标移开后隐回；其余落半隐停靠位。
        // 半隐位与 edge_tick 同口径（dock_hidden_pos）
        let peek = (PEEK * scale).round() as i32;
        let size_px = (half * 2.0).round() as i32;
        let (raw_nx, raw_ny) = dock_hidden_pos(cx, cy, dx, dy, half, peek);
        let (dock_nx, dock_ny) = if let Some((ml, mt, mr, mb)) = wa_rect {
            landable_dock_pos(raw_nx, raw_ny, size_px, size_px, ml, mt, mr, mb, dx, dy)
        } else {
            (raw_nx, raw_ny)
        };
        let mut popped_now = false;
        if let (Some((ml, mt, mr, mb)), true) = (wa_rect, dx != 0 || dy != 0) {
            let (vx0, vy0) = (dock_nx.max(ml), dock_ny.max(mt));
            let (vx1, vy1) = ((dock_nx + size_px).min(mr), (dock_ny + size_px).min(mb));
            popped_now = cursor_pos()
                .map(|(x, y)| x >= vx0 && x < vx1 && y >= vy0 && y < vy1)
                .unwrap_or(false);
        }
        // 滑出量 = 窗口半边长（整窗完全屏内，球体与周围粒子一并露出），并与边缘监视
        // 同口径向对侧 clamp——两处算法必须一致，否则监视循环会判成「位置漂移」而撒手
        let off = half.round() as i32;
        let (fx, fy) = if popped_now {
            if let Some((ml, mt, mr, mb)) = wa_rect {
                (
                    (dock_nx + dx * off).clamp(ml, (mr - size_px).max(ml)),
                    (dock_ny + dy * off).clamp(mt, (mb - size_px).max(mt)),
                )
            } else {
                (dock_nx + dx * off, dock_ny + dy * off)
            }
        } else {
            (dock_nx, dock_ny)
        };
        // 先写盘成功再搬窗：写盘失败（磁盘满/占用）时窗口与记忆保持一致（都在旧位），
        // 拖拽整体未生效且有日志——否则 set_position（异步 IPC）成功 + save 失败会让
        // 「记忆=唯一真相」在 800ms 后按旧记忆把球拉回，整次拖拽等于没发生且无线索
        let save_ok = {
            let _guard = config::lock();
            let mut cfg = config::load();
            // 记忆的一律是「半隐停靠球心」：滑出态只是窗口临时偏移，不写回
            cfg.floating_ball_x = Some(cx.round());
            cfg.floating_ball_y = Some(cy.round());
            config::save(&cfg).is_ok()
        };
        if save_ok {
            // memo 是盘上值的镜像，只在写盘成功时更新
            memo_ball_set(cx.round(), cy.round());
            if (fx, fy) != (pos.x, pos.y) {
                // set_position 是异步 IPC：主线程忙导致丢失时不必在此补搬——冷却结束后
                // edge_tick 的「落位补齐」会按记忆位收敛（窗口向记忆）
                let _ = win.set_position(PhysicalPosition::new(fx, fy));
            }
        } else {
            log::warn!("[悬浮球] 拖拽落位写配置失败，球保持原位");
        }
        // 通知前端重拉状态（停靠边决定菜单/滑出方向）——旧流程由前端在
        // startDragging 的 promise then 里 refreshState，现在落位时机移到
        // 监视循环，改用事件驱动。只发给悬浮球窗（广播会无谓唤醒主窗等全部窗口）
        let _ = app.emit_to(LABEL, "floating-ball-settled", ());
}

/// 展开/收起环形菜单：以球心为锚切换窗口几何（球态 100 ↔ 菜单态 260，一次原子
/// SetWindowPos）。WebView2 重排滞后帧由前端开合淡出掩盖（见 FloatingBallWindow）。
/// async 与 drag_end 同理（窗口操作离开主线程）。
#[tauri::command]
pub async fn floating_ball_expand(app: AppHandle, expanded: bool) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if let Some(win) = app.get_webview_window(LABEL) {
        apply_geometry(&win, expanded, None);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = (app, expanded);
}

/// 环形菜单/双击动作分发：view:*/act:search/act:note 作用于主窗口（先显示再派发事件）；
/// act:clipboard 直呼剪贴板浮层（不弹主窗）；act:main 双击切换主窗口（开着则收起）
#[tauri::command]
pub fn floating_ball_trigger(app: AppHandle, id: String) {
    if id == "act:clipboard" {
        crate::clipboard::toggle_overlay(&app);
        return;
    }
    if id == "act:main" {
        crate::tray::toggle_main_window(&app);
        return;
    }
    // AI 对话：设置开启「独立窗口」形态时直接唤起对话小窗（主窗不必出现），
    // 否则仍走主窗路径（显示 + 派发事件展开内嵌抽屉）——两种形态互斥，入口行为跟随设置
    if id == "view:chat" && crate::chat_window::mode_enabled() {
        crate::chat_window::toggle(&app);
        return;
    }
    crate::tray::show_window(&app);
    if id != "act:main" {
        use tauri::Emitter;
        let _ = app.emit_to("main", "floating-ball-action", id);
    }
}

/// 右键菜单：托盘同款（tray.rs 统一定义文案，事件 id 带 fb- 前缀）
#[tauri::command]
pub fn floating_ball_context_menu(app: AppHandle) -> Result<(), String> {
    crate::tray::popup_context_menu(&app).map_err(|e| e.to_string())
}

/// 前端失配自检兜底：视口尺寸（CSS clientWidth）偏离期望逻辑尺寸时调用。
/// 携带 clientWidth 让 Rust 反推 WebView2 实际光栅缩放（窗口物理宽 ÷ 视口宽）——
/// 窗口 DPI 上下文过期时 GetDpiForWindow 也会返回旧值，只有视口实测值不会骗人，
/// 据此重设物理尺寸即可把视口拉回期望逻辑尺寸（118% 等自定义缩放下菜单按钮
/// 外圈被裁的根治路径）。与 expand 的区别：不切换态，只把当前态几何拉回正确值。
/// async 与 expand 同理（窗口操作离开主线程）。
#[tauri::command]
pub async fn floating_ball_reapply(app: AppHandle, viewport_w: f64) {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if let Some(win) = app.get_webview_window(LABEL) {
        let override_scale = win.outer_size().ok().and_then(|sz| {
            let s = sz.width as f64 / viewport_w;
            // 合理性区间外的值不可信（视口未就绪/异常），退回 GetDpiForWindow
            (viewport_w > 20.0 && (0.8..=4.0).contains(&s)).then_some(s)
        });
        apply_geometry(&win, is_expanded(&win), override_scale);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = (app, viewport_w);
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::*;

    /// 1080p 工作区（左上角为原点，物理 px 口径下 scale=1）
    const WA: (f64, f64, f64, f64) = (0.0, 0.0, 1920.0, 1040.0);

    fn snap(cx: f64, cy: f64, auto_hide: bool) -> (f64, f64, i32, i32) {
        dock_snap(cx, cy, WA.0, WA.1, WA.2, WA.3, 1.0, auto_hide)
    }

    /// 关键不变式（AGENTS.md 约定 42 / ADR 0004）：吸附触发距离必须大于
    /// 「滑出量（窗口半边长）+ 位置一致容差」，否则「刚好贴边的自由位置」与
    /// 「滑出位」几何上无法区分，边缘监视会把用户放好的球拽回屏边
    #[test]
    fn dock_trigger_exceeds_pop_offset_plus_tolerance() {
        let half = BALL_SIZE / 2.0;
        assert!(
            DOCK_TRIGGER > half + POS_TOL as f64,
            "DOCK_TRIGGER {DOCK_TRIGGER} 必须 > 半边长 {half} + POS_TOL {POS_TOL}"
        );
    }

    #[test]
    fn snaps_center_onto_edge_within_trigger() {
        // 球心距左缘在 trigger 内 → 精确落在屏边、方向 +1（向屏内滑出）
        let (cx, cy, dx, dy) = snap(DOCK_TRIGGER - 1.0, 500.0, true);
        assert_eq!((cx.round() as i32, dx, dy), (0, 1, 0));
        assert_eq!(cy.round() as i32, 500);
    }

    /// 用户反馈的主 bug：吸附过一次之后，把球拖到屏幕中间必须保持原位、不再弹回
    #[test]
    fn keeps_free_position_beyond_trigger() {
        let (cx, _cy, dx, _dy) = snap(DOCK_TRIGGER + 1.0, 500.0, true);
        assert_eq!((cx.round() as i32, dx), ((DOCK_TRIGGER + 1.0) as i32, 0));
        // 屏幕正中更不可能被吸走
        let (mx, my, mdx, mdy) = snap(960.0, 520.0, true);
        assert_eq!((mx.round() as i32, my.round() as i32, mdx, mdy), (960, 520, 0, 0));
    }

    #[test]
    fn snaps_corner_on_both_axes() {
        let (cx, cy, dx, dy) = snap(10.0, 1035.0, true);
        assert_eq!((dx, dy), (1, -1));
        assert_eq!((cx.round() as i32, cy.round() as i32), (0, 1040));
    }

    #[test]
    fn picks_the_nearest_edge_per_axis() {
        // 距右缘更近（20 < 40）→ 吸附右缘，方向 -1
        let (cx, _cy, dx, _dy) = snap(1900.0, 500.0, true);
        assert_eq!((cx.round() as i32, dx), (1920, -1));
    }

    #[test]
    fn auto_hide_off_never_snaps() {
        let (cx, cy, dx, dy) = snap(3.0, 3.0, false);
        assert_eq!((cx, cy, dx, dy), (3.0, 3.0, 0, 0));
    }

    /// 半隐停靠位（dock_hidden_pos）：贴边后向屏内多露 PEEK，贴左向右偏、贴右向左偏
    #[test]
    fn hidden_pos_peeks_inward() {
        // 贴左（dx=1）：窗口左上 = 0 - 50 + 8 = -42（屏内可见 58px，球露约 2/3）
        assert_eq!(dock_hidden_pos(0.0, 500.0, 1, 0, 50.0, 8), (-42, 450));
        // 贴右（dx=-1）：窗口左上 = 1920 - 50 - 8 = 1862
        assert_eq!(dock_hidden_pos(1920.0, 500.0, -1, 0, 50.0, 8), (1862, 450));
        // 角落双侧（贴左 + 贴底）：y 轴向屏内 = 向上收 8
        assert_eq!(dock_hidden_pos(0.0, 1040.0, 1, -1, 50.0, 8), (-42, 982));
    }

    /// 贴顶半隐钳回工作区后：Windows 仍要半隐/滑出分开；Linux 滑出位必须等于半隐位
    /// （贴边全显、不做悬停滑出），否则窗口从光标下抽走，100ms 上下跳。
    #[test]
    fn dock_hover_matches_platform_policy() {
        let half = 50i32;
        let w = 100;
        let (l, t, r, b) = (0, 0, 1920, 1040);
        let hidden_raw = dock_hidden_pos(500.0, 0.0, 0, 1, 50.0, 8);
        assert_eq!(hidden_raw, (450, -42));
        let hidden = clamp_window_into_work(hidden_raw.0, hidden_raw.1, w, w, l, t, r, b);
        assert_eq!(hidden, (450, 0));
        let popped = dock_hover_pos(hidden, 0, 1, half, w, l, t, r, b);
        if cfg!(target_os = "linux") {
            assert_eq!(popped, hidden);
        } else {
            assert_eq!(popped, (450, 50));
            assert!(
                (hidden.1 - popped.1).abs() > POS_TOL,
                "Windows 钳回后半隐/滑出必须能区分: hidden={} popped={}",
                hidden.1,
                popped.1
            );
        }
    }

    #[test]
    fn clamp_window_keeps_fully_on_screen() {
        assert_eq!(
            clamp_window_into_work(-42, 450, 100, 100, 0, 0, 1920, 1040),
            (0, 450)
        );
        assert_eq!(
            clamp_window_into_work(1862, 982, 100, 100, 0, 0, 1920, 1040),
            (1820, 940)
        );
    }

    #[test]
    fn already_at_uses_pos_tolerance() {
        assert!(already_at((0, 0), (0, 9)));
        assert!(!already_at((0, 0), (0, 11)));
    }

    /// 左右贴边只认 X：Y 被顶栏挤偏仍算半隐，避免监视循环上下跳。
    #[test]
    fn side_dock_ignores_free_axis_drift() {
        let hidden = dock_hidden_pos(0.0, 40.0, 1, 0, 50.0, 8);
        assert_eq!(hidden, (-42, -10));
        let work = (0, 32, 1920, 1040);
        let landed = landable_dock_pos(hidden.0, hidden.1, 100, 100, work.0, work.1, work.2, work.3, 1, 0);
        assert_eq!(landed.0, if cfg!(target_os = "linux") { 0 } else { -42 });
        assert_eq!(landed.1, hidden.1); // 不得把 Y 钳到顶栏
        assert!(pos_matches_dock((-42, 8), hidden, 1, 0));
        assert!(!pos_matches_dock((40, -10), hidden, 1, 0));
    }

    /// 自由位目标必须钳进工作区：日志实证「记忆球心=(2317,330)、工作区底=343」时，
    /// 原始目标 y=280（窗口下沿 380 越界）会被 WM 钳成 243，监视循环于是每 800ms
    /// 重搬一次 = 球上下跳。Linux 必须直接给出 243（已到位的坐标 → 不再搬）；
    /// Windows 允许半截出屏，保持原目标。
    #[test]
    fn free_pos_is_clamped_into_work_area() {
        let (x, y) = free_ball_pos(2317.0, 330.0, 50, 100, 0, 0, 2560, 343);
        assert_eq!(x, 2267);
        if cfg!(target_os = "linux") {
            assert_eq!(y, 243);
        } else {
            assert_eq!(y, 280);
        }
    }

    /// 半隐位（球心压屏边）与滑出位（球心向屏内一个半边长）必须落在两个可区分的
    /// 位置，且滑出位整窗在屏内 —— 悬停才能露出完整球体与周围粒子
    #[test]
    fn popped_offset_keeps_whole_window_on_screen() {
        let half = (BALL_SIZE / 2.0) as i32;
        let (cx, _, dx, _) = snap(0.0, 500.0, true);
        assert_eq!(dx, 1);
        let cx = cx as i32; // 半隐球心 = 左边缘
        assert_eq!(cx, 0);
        // 半隐位窗口左上角在屏外一个半边长；滑出位整窗进入屏内
        assert_eq!(cx - half, -half);
        assert_eq!((cx - half) + half, 0);
    }
}
