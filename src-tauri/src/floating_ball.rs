//! 桌面悬浮球（ADR 0004）：常驻透明置顶小球，仅主窗口隐藏时显示。
//! 单击展开环形快捷菜单、双击显示主窗口、右键托盘同款菜单；
//! 拖拽记忆位置，靠近屏幕边缘自动吸附——停靠姿态为「完整贴边」：球缘距屏边
//! SNAP_GAP 停住、球体完整可见（与交互原型一致；曾实现为「半挂」球心贴屏边、
//! 半个球藏屏外，用户实测觉得吸附没用且难拖回，已改回完整贴边）。
//! Windows-only：独立透明无边框窗口，与 countdown_window 同模式复用。
//!
//! 几何模型：球态窗口 = BALL_SIZE，菜单态 = MENU_SIZE，均以「球心」（窗口中心）为锚
//! 原子切换（单次 SetWindowPos）。窗口 resize 时 WebView2 内容重排滞后一帧，旧帧按
//! 旧视口渲染会让球先「跳」向窗口移动方向再弹回——前端在开合前后把窗口内容整体
//! 淡出/淡入，把跳动帧掩盖在「球化开成菜单」的过渡里（见 FloatingBallWindow 开合时序）。

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

use crate::config;

/// 悬浮球窗口 label（App.vue 按此路由到 FloatingBallWindow）
pub const LABEL: &str = "floating-ball";

/// 球态窗口尺寸（逻辑 px：48 中心球体 + 光晕/粒子/陀螺环余量，避免视觉被窗口裁切；
/// 100 = 容纳陀螺环最外圈视觉 94px + 3px 余量）
pub const BALL_SIZE: f64 = 100.0;
/// 环形菜单展开态窗口尺寸（逻辑 px：按钮轨道半径 118 + 按钮 26 → 外沿 144，中心 156 留 12px 余量）
pub const MENU_SIZE: f64 = 312.0;
/// 球体半径（逻辑 px）：前端 .fb-ball 视觉 48px 直径的半径；
/// 用于默认初始位置与吸附停靠的贴边定位（球心距屏边 = BALL_R + SNAP_GAP）
const BALL_R: f64 = 24.0;
/// 吸附触发距离（逻辑 px）：拖拽松手时球心距屏幕边缘小于该值 → 完整贴边停靠。
/// 60 = 与交互原型 NEAR 一致：球缘距屏边约 36px 内松手即吸附
const SNAP_TRIGGER: f64 = 60.0;
/// 贴边停靠留白：默认初始位置与吸附停靠共用——球缘距屏幕边缘的视觉间距（逻辑 px），
/// 球整体完整可见地停在屏内（不做「半挂」藏球，用户实测半挂觉得吸附没用）
const SNAP_GAP: f64 = 7.0;
/// 环形按钮上限（超过会互相重叠；保存命令与设置页双重钳制）
pub const MAX_BUTTONS: usize = 8;

/// 展开前的窗口位置（物理 px）：收起时恢复，靠边挪位后球能回到原吸附点。
/// 用全局 Mutex 而非 thread_local：拖拽/展开命令是 async（跑在tokio线程池），
/// 主线程的 sync_with_main 也会收拢几何，跨线程共享必须用带锁的静态。
#[cfg(target_os = "windows")]
static PRE_EXPAND_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// 主窗是否处于最小化（与「隐藏到托盘」一样属于视觉不可见 → 球显示）。
/// MAIN_WINDOW_VISIBLE 状态位只覆盖托盘/快捷键的显式 show/hide，点标题栏最小化
/// 不经过那条链——由主窗 Resized 事件检测最小化变化后经 set_main_minimized 更新。
#[cfg(target_os = "windows")]
static MAIN_MINIMIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 主窗最小化状态变化入口（lib.rs 主窗事件钩子调用）：更新状态并联动球显隐
pub fn set_main_minimized(app: &AppHandle, minimized: bool) {
    #[cfg(target_os = "windows")]
    {
        use std::sync::atomic::Ordering;
        if MAIN_MINIMIZED.swap(minimized, Ordering::SeqCst) == minimized {
            return;
        }
        sync_with_main(app);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, minimized);
    }
}

#[derive(Debug, Serialize)]
pub struct FloatingBallState {
    pub enabled: bool,
    pub snap: bool,
    /// 与主窗口同时显示（主窗可见时球保持常驻）
    pub with_main: bool,
    pub buttons: Vec<String>,
    /// 记忆的球心位置（物理 px，拖拽松手后由后端记忆；None = 从未拖拽过）
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub menu_size: f64,
}

// ---------- 生命周期 ----------

/// 启动时预创建悬浮球窗口并隐藏常驻（与 clipboard 浮层同模式，见 ADR 0004「非惰性创建」）。
/// 停用/启用只切窗口显隐，绝不运行时销毁重建——运行时现场创建/销毁 WebView2 窗口
/// 是主线程长任务，曾与悬浮球窗口操作交错导致整窗未响应（WebView2 controller 创建挂起，
/// 见 clipboard.rs::init_overlay_window 与 lib.rs 启动注释的同款坑）。
/// 必须在 autostart-hidden 的 tray::hide_window 之后调用，显隐联动才正确。
pub fn init(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Err(e) = ensure_window(app) {
            log::warn!("悬浮球窗口创建失败: {}", e);
            return;
        }
        if !config::load().floating_ball_enabled {
            // 停用状态：窗口隐藏常驻，设置启用时直接 show 即可
            if let Some(win) = app.get_webview_window(LABEL) {
                let _ = win.hide();
            }
            return;
        }
        sync_with_main(app);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

#[cfg(target_os = "windows")]
fn ensure_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("index.html".into()))
            .title("悬浮球")
            .inner_size(BALL_SIZE, BALL_SIZE)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(true)
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);
    // 透明窗口在 Windows 上不能同时启用系统阴影（黑边），与便签/倒计时浮窗一致
    builder = builder.shadow(false);
    let win = builder.build()?;

    place_initial(app, &win);

    // 悬浮球不响应关闭请求（Alt+F4 等）：隐藏即可，窗口常驻复用
    // （非惰性创建，避免每次唤出的窗口创建延迟，见 ADR 0004）
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window(LABEL) {
                let _ = w.hide();
            }
        }
    });

    log::info!("悬浮球窗口已创建");
    Ok(())
}

/// 初始位置：优先用记忆的球心（物理 px），否则主显示器右下角（球缘距屏边 SNAP_GAP，
/// 与吸附同语义）。球态窗口位置 = 球心 - 半边长。
/// 注意：floating_ball_x/y 存球心坐标（旧版本存的是窗口左上角，升级后首次
/// 位置会偏移一次，拖动一下即按新语义记忆）。
#[cfg(target_os = "windows")]
fn place_initial(app: &AppHandle, win: &tauri::WebviewWindow) {
    let scale = win.scale_factor().unwrap_or(1.0);
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
        // 默认右下角：球缘距屏右/下各 SNAP_GAP
        let cx = mon.position().x + mon.size().width as i32 - ball_r - gap;
        let cy = mon.position().y + mon.size().height as i32 - ball_r - gap;
        let _ = win.set_position(PhysicalPosition::new(cx - half, cy - half));
    }
}

/// 与主窗口显隐联动：主窗显示且未开「同显」→ 球隐藏；其余情况 → 球显示。
/// 主窗的所有 show/hide 都走 tray::show_window / hide_window，统一钩到这里。
pub fn sync_with_main(app: &AppHandle) {
    #[cfg(target_os = "windows")]
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
            apply_geometry(&win, false);
            let _ = win.show();
            // 通知页面复位菜单态（几何已在上面收拢）
            use tauri::Emitter;
            let _ = app.emit_to(LABEL, "floating-ball-shown", ());
        } else {
            let _ = win.hide();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

// ---------- 几何管理 ----------

/// 当前是否处于菜单展开态（按窗口实际尺寸判断，免维护额外状态）
#[cfg(target_os = "windows")]
fn is_expanded(win: &tauri::WebviewWindow) -> bool {
    let scale = win.scale_factor().unwrap_or(1.0);
    win.outer_size()
        .map(|sz| sz.width as f64 > BALL_SIZE * scale * 1.5)
        .unwrap_or(false)
}

/// 以「球心」（窗口中心）为锚调整窗口几何：球态 = BALL_SIZE，菜单态 = MENU_SIZE。
/// 展开时钳制到所在显示器内（空间自适应挪位，收起后自然回到吸附位置）。
#[cfg(target_os = "windows")]
fn apply_geometry(win: &tauri::WebviewWindow, expanded: bool) {
    let Ok(pos) = win.outer_position() else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let was_expanded = is_expanded(win);

    // 当前球心 = 当前模式窗口中心；目标窗口 = 球心 ± 目标半边长
    let cur_half = (if was_expanded { MENU_SIZE } else { BALL_SIZE } / 2.0) * scale;
    let cx = pos.x as f64 + cur_half;
    let cy = pos.y as f64 + cur_half;

    let new_size = ((if expanded { MENU_SIZE } else { BALL_SIZE }) * scale).round();
    let new_half = new_size / 2.0;
    let mut nx = cx - new_half;
    let mut ny = cy - new_half;

    // 展开时记住原位置；收起时优先恢复（展开被钳制挪位后，球回到原吸附点而非漂移）
    // 已处于展开态时跳过记录（收回动画期间重复 expand 不覆盖原始吸附点）
    let mut pre = PRE_EXPAND_POS.lock().unwrap_or_else(|e| e.into_inner());
    if expanded {
        if !was_expanded {
            *pre = Some((pos.x, pos.y));
        }
    } else if let Some((px, py)) = pre.take() {
        nx = px as f64;
        ny = py as f64;
    }
    drop(pre);

    if let Ok(Some(mon)) = win.current_monitor() {
        let m = if expanded { 4.0 * scale } else { 0.0 };
        let min_x = mon.position().x as f64 + m;
        let min_y = mon.position().y as f64 + m;
        let max_x = (mon.position().x + mon.size().width as i32) as f64 - new_size - m;
        let max_y = (mon.position().y + mon.size().height as i32) as f64 - new_size - m;
        // 小屏保护：max 可能小于 min（f64::clamp 在 min > max 时 panic）
        let (max_x, max_y) = (max_x.max(min_x), max_y.max(min_y));
        nx = nx.clamp(min_x, max_x);
        ny = ny.clamp(min_y, max_y);
    }

    // 原子应用尺寸+位置（单次 SetWindowPos）：拆成 set_size + set_position 会让窗口
    // 先单向长大再挪回（球心瞬移），WebView2 还要做两次重排——展开卡顿的一部分
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
    } else {
        let _ = win.set_size(PhysicalSize::new(new_size as u32, new_size as u32));
        let _ = win.set_position(PhysicalPosition::new(nx.round() as i32, ny.round() as i32));
    }
}

/// 设置变更后的应用：启用则确保窗口存在并联动显隐；停用只隐藏、不销毁窗口。
/// 窗口由启动 init 预创建后常驻——运行时 destroy/rebuild WebView2 与悬浮球窗口
/// 操作交错会卡死整窗（见 init 注释），与 clipboard 浮层「预创建隐藏常驻」同款约束
#[cfg(target_os = "windows")]
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
pub fn floating_ball_get_state() -> FloatingBallState {
    let cfg = config::load();
    FloatingBallState {
        enabled: cfg.floating_ball_enabled,
        snap: cfg.floating_ball_snap,
        with_main: cfg.floating_ball_with_main,
        buttons: cfg.floating_ball_buttons,
        x: cfg.floating_ball_x,
        y: cfg.floating_ball_y,
        menu_size: MENU_SIZE,
    }
}

/// 保存设置并立即生效（窗口创建/销毁、显隐联动；按钮列表去重 + 钳制上限）。
/// 独立于 save_config：位置等 Rust 侧字段不经过前端快照，避免互相覆盖。
#[tauri::command]
pub fn floating_ball_save_settings(
    app: AppHandle,
    enabled: bool,
    snap: bool,
    with_main: bool,
    buttons: Vec<String>,
) -> Result<(), String> {
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
        cfg.floating_ball_snap = snap;
        cfg.floating_ball_with_main = with_main;
        cfg.floating_ball_buttons = buttons;
        config::save(&cfg)?;
    }

    #[cfg(target_os = "windows")]
    apply_enabled(&app, enabled);

    // 通知球窗口刷新按钮集（未启用/非 Windows 时窗口不存在，跳过）
    #[cfg(target_os = "windows")]
    if enabled {
        use tauri::Emitter;
        let cfg = config::load();
        let _ = app.emit_to(
            LABEL,
            "floating-ball-config-changed",
            serde_json::json!({ "buttons": cfg.floating_ball_buttons, "snap": cfg.floating_ball_snap }),
        );
    }

    #[cfg(not(target_os = "windows"))]
    let _ = (&app, snap);

    log::info!(
        "悬浮球设置已保存: enabled={} snap={} with_main={}",
        enabled,
        snap,
        with_main
    );
    Ok(())
}

/// 拖拽结束：球心钳在显示器内（不能整个出屏，否则拖不回来）+ 可选贴边吸附
/// （球缘距屏边 SNAP_GAP 完整停靠，见模块注释）+ 记忆球心到配置。
/// 拖拽只发生在球态，窗口位置 = 球心 - 球态半边长。
/// async：配置读写是文件 IO，必须离开主线程（前端在系统拖动循环结束后调用）。
#[tauri::command]
pub async fn floating_ball_drag_end(app: AppHandle) {
    #[cfg(target_os = "windows")]
    {
        let Some(win) = app.get_webview_window(LABEL) else { return };
        let Ok(pos) = win.outer_position() else { return };
        let scale = win.scale_factor().unwrap_or(1.0);
        let half = (BALL_SIZE * scale / 2.0).round();
        // 拖拽结束时的球心（球态窗口中心即球心）
        let mut cx = pos.x as f64 + half;
        let mut cy = pos.y as f64 + half;
        if let Ok(Some(mon)) = win.current_monitor() {
            let mx = mon.position().x as f64;
            let my = mon.position().y as f64;
            let mw = mon.size().width as f64;
            let mh = mon.size().height as f64;
            // 球心只能钳在显示器内（球整体不能出屏，否则拖不回来）
            cx = cx.clamp(mx, (mx + mw).max(mx));
            cy = cy.clamp(my, (my + mh).max(my));
            if config::load().floating_ball_snap {
                let trigger = SNAP_TRIGGER * scale;
                let d_left = cx - mx;
                let d_right = mx + mw - cx;
                let d_top = cy - my;
                let d_bottom = my + mh - cy;
                // 完整贴边停靠：球心距屏边 < trigger（球基本贴到屏边）→ 吸附到
                // 「球缘距屏边 SNAP_GAP」的停靠位，球体完整可见——不做半挂藏球
                // （曾实现为球心贴屏边、半个球在屏外，用户实测觉得吸附没用且难拖回）
                let edge = (BALL_R + SNAP_GAP) * scale; // 贴边后球心距屏边距离
                if d_left < trigger && d_left <= d_right {
                    cx = mx + edge;
                } else if d_right < trigger {
                    cx = mx + mw - edge;
                }
                if d_top < trigger && d_top <= d_bottom {
                    cy = my + edge;
                } else if d_bottom < trigger {
                    cy = my + mh - edge;
                }
            }
        }
        let nx = (cx - half).round() as i32;
        let ny = (cy - half).round() as i32;
        if (nx, ny) != (pos.x, pos.y) {
            let _ = win.set_position(PhysicalPosition::new(nx, ny));
        }
        let _guard = config::lock();
        let mut cfg = config::load();
        cfg.floating_ball_x = Some(cx.round());
        cfg.floating_ball_y = Some(cy.round());
        let _ = config::save(&cfg);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = app;
}

/// 展开/收起环形菜单：以球心为锚切换窗口几何（球态 100 ↔ 菜单态 312，一次原子
/// SetWindowPos）。WebView2 重排滞后帧由前端开合淡出掩盖（见 FloatingBallWindow）。
/// async 与 drag_end 同理（窗口操作离开主线程）。
#[tauri::command]
pub async fn floating_ball_expand(app: AppHandle, expanded: bool) {
    #[cfg(target_os = "windows")]
    if let Some(win) = app.get_webview_window(LABEL) {
        apply_geometry(&win, expanded);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, expanded);
}

/// 环形菜单/双击动作分发：view:*/act:search/act:note 作用于主窗口（先显示再派发事件）；
/// act:clipboard 直呼剪贴板浮层（不弹主窗）；act:main 仅显示主窗口
#[tauri::command]
pub fn floating_ball_trigger(app: AppHandle, id: String) {
    if id == "act:clipboard" {
        crate::clipboard::toggle_overlay(&app);
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
