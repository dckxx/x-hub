//! AI 对话独立窗口（label `chat`）：设置「AI 助手 → 以独立窗口打开 AI 对话」开启后，
//! 对话不再是主窗内嵌抽屉，而是这个可缩放、可置顶、无边框 + 自制标题栏的独立小窗。
//! 与主窗内嵌形态**互斥**：开关决定唯一形态，标题栏按钮 / Ctrl+Shift+K / 悬浮球
//! 「AI 对话」入口都按该开关分流。
//!
//! 窗口生命周期（约定 41 铁律：运行时禁止现场创建/销毁 WebView2 窗口，**零例外**）：
//! - 启动期 `init` **无条件**预创建 + 隐藏常驻，与悬浮球/剪贴板/通知窗同口径。曾有过的
//!   「按配置惰性建窗」把建窗推到了设置开关从关切到开的那一刻（apply_mode 现场 build），
//!   实测整 app 挂死：窗口打不开、主线程卡死后同步命令（供应商列表）永不返回、托盘退出
//!   无反应（v0.5.5 用户实机踩中）。「用户在设置页没有并发窗口操作」是伪论证——悬浮球边缘
//!   监视循环 100ms 一次搬窗、通知/剪贴板隐藏窗常驻，任何运行期 build 都在赌窗口操作不并存；
//! - 关闭按钮 / Alt+F4 只 `prevent_close` + 隐藏，窗口常驻复用，**绝不 destroy**；
//! - 形态开关（chat_window_save_mode/apply_mode）只改内存镜像 + 显隐，运行期绝不 build。

use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow};

use crate::config;

/// 对话独立窗口 label（App.vue 按此路由到 ChatWindow）
pub const LABEL: &str = "chat";

/// 最小尺寸（逻辑 px）：再小就放不下「标题栏 + 输入区 + 若干条消息」
const MIN_WIDTH: f64 = 360.0;
const MIN_HEIGHT: f64 = 320.0;
/// 尺寸持久化上限（逻辑 px）：吸收异常值（如被拖到超宽双屏的极端尺寸）避免下次启动铺满桌面
const MAX_WIDTH: f64 = 2560.0;
const MAX_HEIGHT: f64 = 1600.0;

/// 几何持久化节流计数：Moved/Resized 在拖拽过程中高频触发，只在最后一次事件
/// `GEOMETRY_DEBOUNCE_MS` 之后落盘一次
static GEOMETRY_TICK: AtomicU64 = AtomicU64::new(0);
/// 去抖线程单飞标志：拖动中 Moved 每秒可触发上百次，逐事件 spawn 线程纯属浪费，
/// 同一时刻只允许一个在飞线程负责「等事件流稳定 → 落盘」。
/// 只承载「单飞资格」本身、不承载其他数据同步，Relaxed 足够
static PERSIST_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 独立窗形态开关的内存镜像：init/save_mode/apply_mode 三个写点维护，悬浮球触发等
/// 高频读方免每次全量读盘反序列化 app.json（磁盘仍是唯一权威，手改配置文件需重启生效）
static MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 独立窗形态是否开启（内存镜像，见 MODE 注释）
pub fn mode_enabled() -> bool {
    MODE.load(Ordering::Relaxed)
}
const GEOMETRY_DEBOUNCE_MS: u64 = 600;

/// 窗口原生可见性：`is_visible()` 在 Windows 上对 WebView2 子窗口判定不准（隐藏后仍可能
/// 报 true，见 clipboard.rs / tray.rs 同款注释），会让 toggle 误入「收起」分支——直查
/// Win32 IsWindowVisible。
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

/// 几何落盘（尺寸取逻辑 px、位置取物理 px——与悬浮球/倒计时浮窗的位置约定一致）
fn persist_geometry(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    // 最小化时 Windows 会把窗口搬到 (-32000,-32000) 并触发 Moved 事件——把该坐标
    // 落盘会让下次恢复跑到屏幕外（show 的 unminimize 也救不回），跳过本次记忆
    if win.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
        return;
    };
    let scale = win.scale_factor().unwrap_or(1.0);
    let width = (size.width as f64 / scale).clamp(MIN_WIDTH, MAX_WIDTH).round();
    let height = (size.height as f64 / scale).clamp(MIN_HEIGHT, MAX_HEIGHT).round();
    let (x, y) = (pos.x as f64, pos.y as f64);

    let _guard = config::lock();
    let mut cfg = config::load();
    if (cfg.chat_window_x, cfg.chat_window_y) == (Some(x), Some(y))
        && cfg.chat_window_width == width
        && cfg.chat_window_height == height
    {
        return;
    }
    cfg.chat_window_width = width;
    cfg.chat_window_height = height;
    cfg.chat_window_x = Some(x);
    cfg.chat_window_y = Some(y);
    if let Err(e) = config::save(&cfg) {
        log::warn!("对话独立窗几何落盘失败: {e}");
    }
}

/// 拖拽/缩放结束后防抖落盘（spawn 短命线程等待窗口静止，不占用消息线程）
fn schedule_persist(app: &AppHandle) {
    GEOMETRY_TICK.fetch_add(1, Ordering::Relaxed);
    // 已有在飞线程：只 bump tick 即可，它醒来发现 tick 变化会续睡等事件流稳定
    if PERSIST_IN_FLIGHT.swap(true, Ordering::Relaxed) {
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut seen = GEOMETRY_TICK.load(Ordering::Relaxed);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(GEOMETRY_DEBOUNCE_MS));
            let now = GEOMETRY_TICK.load(Ordering::Relaxed);
            if now != seen {
                seen = now; // 期间又有新事件，续睡一轮
                continue;
            }
            persist_geometry(&handle);
            // persist（含落盘 fsync）期间可能又有新事件 bump tick——此刻在飞的只有本线程，
            // 直接退出会让那次移动永不落盘，复查一遍：有新事件就续睡接管
            let latest = GEOMETRY_TICK.load(Ordering::Relaxed);
            if latest == seen {
                PERSIST_IN_FLIGHT.store(false, Ordering::Relaxed);
                return;
            }
            seen = latest;
        }
    });
}

/// 首次建窗（尚无记忆位置）的落点：主窗中央略偏右下（与便签/整列表浮窗同口径）。
/// 主窗不可见或取不到几何时返回 None，交给系统默认级联位置。
/// 入参为逻辑尺寸，返回物理 px 坐标（与 `set_position(PhysicalPosition)` 对齐）。
fn initial_center(app: &AppHandle, width: f64, height: f64) -> Option<(i32, i32)> {
    let main = crate::main_window(app)?;
    if !main.is_visible().unwrap_or(false) {
        return None;
    }
    let pos = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    let scale = main.scale_factor().unwrap_or(1.0);
    // outer_size 是物理 u32、position 是物理 i32，统一到 i32 再算
    let (sw, sh) = (size.width as i32, size.height as i32);
    let w = (width * scale) as i32;
    let h = (height * scale) as i32;
    Some((
        pos.x + (sw - w) / 2 + 40,
        pos.y + (sh - h) / 2 + 24,
    ))
}

/// 建窗（一律隐藏常驻，唤起/收起由 show_window/hide_window 做显隐；启动 init 唯一调用点）
fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let cfg = config::load();
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
            .title("AI 对话")
            .inner_size(
                cfg.chat_window_width.clamp(MIN_WIDTH, MAX_WIDTH),
                cfg.chat_window_height.clamp(MIN_HEIGHT, MAX_HEIGHT),
            )
            .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
            .resizable(true)
            .decorations(false)
            .transparent(true)
            .always_on_top(cfg.chat_window_pinned)
            // 独立对话窗是正常窗体：进任务栏，便于 Alt+Tab 唤回（主窗隐藏时也能找到它）
            .skip_taskbar(false)
            .visible(false)
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    // 透明窗口在 Windows 上启用系统阴影会把边缘渲染成黑色描边（便签/剪贴板浮窗同款坑），
    // 面板自带 CSS 阴影，OS 层阴影关闭
    #[cfg(target_os = "windows")]
    {
        builder = builder.shadow(false);
    }

    let win = builder.build()?;

    // 位置：有记忆用记忆（物理 px），首次落在主窗中央（builder.position 是逻辑 px
    // 口径，故统一在建好后用 PhysicalPosition 设置）
    let target = match (cfg.chat_window_x, cfg.chat_window_y) {
        (Some(x), Some(y)) => Some((x as i32, y as i32)),
        _ => initial_center(app, cfg.chat_window_width, cfg.chat_window_height),
    };
    if let Some((x, y)) = target {
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    attach_events(app, &win);
    log::info!("对话独立窗口已创建（隐藏常驻）");
    Ok(win)
}

/// 挂窗口事件：关闭 → 隐藏常驻（绝不销毁）；拖拽/缩放 → 防抖落盘几何
fn attach_events(app: &AppHandle, win: &WebviewWindow) {
    let handle = app.clone();
    win.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_window(&handle);
        }
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
            schedule_persist(&handle);
        }
        _ => {}
    });
}

/// 启动期预创建：**无条件**建窗 + 隐藏常驻（同悬浮球/剪贴板/通知窗口径）。
/// 不做「按配置惰性建窗、开启时运行期补建」——运行期现场 build WebView2 会挂死主线程
/// （v0.5.4 实测事故，见模块头注释）。未开启形态多付一份隐藏 renderer 内存 = 铁律
/// 已接受的既定代价。
pub fn init(app: &AppHandle) {
    MODE.store(config::load().chat_window_mode, Ordering::Relaxed);
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    match build(app) {
        Ok(_) => log::info!("对话独立窗口已预创建（隐藏常驻）"),
        Err(e) => log::warn!("对话独立窗口预创建失败: {e}"),
    }
}

/// 显示（或聚焦）独立对话窗：常驻窗口只做 show + focus。
/// **不做运行时 build 兜底**（约定 41 / notify.rs 同口径）：预创建失败时只记日志、
/// 等下次启动 init 重试——运行期现场建 WebView2 窗口会与悬浮球等窗口操作交错挂死整窗。
pub fn show_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        log::warn!("对话独立窗口不存在（预创建失败？），跳过本次唤起");
        return;
    };
    // 建窗已提前到启动期（主窗可能隐藏/未就绪，拿不到「主窗中央」落点）：
    // 尚无位置记忆时在首次唤起（主窗必然可见）补一次落位，Moved 事件会随之持久化
    {
        let cfg = config::load();
        if (cfg.chat_window_x, cfg.chat_window_y) == (None, None) {
            if let Some((x, y)) = initial_center(app, cfg.chat_window_width, cfg.chat_window_height)
            {
                let _ = win.set_position(PhysicalPosition::new(x, y));
            }
        }
    }
    if win.is_minimized().unwrap_or(false) {
        let _ = win.unminimize();
    }
    // 先恢复内存级别再显示（webview_mem：Low 态缓存已吐，首帧前回 Normal）
    crate::webview_mem::on_shown(app, LABEL);
    if !native_visible(&win) {
        let _ = win.show();
    }
    let _ = win.set_focus();
    // 通知窗内页面：重新拉一次会话/模型（期间主窗可能改过配置或发过消息）
    let _ = app.emit_to(LABEL, "chat-window-shown", ());
}

/// 收起：只隐藏，窗口常驻复用（关闭按钮 / Alt+F4 / 模式切换共用此路径）
pub fn hide_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    if native_visible(&win) {
        let _ = win.hide();
        crate::webview_mem::on_hidden(app, LABEL);
        // 通知窗内页面卸载会话活堆（内存优化）：消息 DOM 随聊天增长，Low 吐不掉
        // 活数据；会话在 SQLite，chat-window-shown 时 ChatPanel.refresh() 重拉
        let _ = app.emit_to(LABEL, "chat-window-hidden", ());
        // 落盘投给后台线程：persist 含配置锁 + fsync，CloseRequested 回调在主线程上，
        // 同步等慢盘会有可感卡顿（窗口 hide 先行，几何在隐藏后读取不受影响）
        let handle = app.clone();
        std::thread::spawn(move || persist_geometry(&handle));
        log::info!("对话独立窗口已隐藏（常驻复用）");
    }
    let _ = app.emit_to("main", "chat-window-visibility", false);
}

/// 唤起/收起（标题栏按钮、Ctrl+Shift+K、悬浮球「AI 对话」入口共用）。
/// 门禁：内嵌形态（mode=false）下入口一律忽略——独立窗虽常驻隐藏，未开启时也不许被唤起。
pub fn toggle(app: &AppHandle) {
    if !mode_enabled() {
        return;
    }
    match app.get_webview_window(LABEL) {
        Some(win) if native_visible(&win) => hide_window(app),
        _ => show_window(app),
    }
}

/// 是否可见（主窗同步标题栏按钮态用）
pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL)
        .map(|w| native_visible(&w))
        .unwrap_or(false)
}

/// 模式切换落地：只改内存镜像 + 显隐。窗口由启动 init 常驻，**这里绝不 build**
/// （约定 41：v0.5.4 在此现场建窗挂死整 app 的事故实录见模块头）。
/// 开启 → 无事（等入口按 mode_enabled 唤起）；关闭 → 隐藏常驻（不 destroy）。
pub fn apply_mode(app: &AppHandle, enabled: bool) {
    MODE.store(enabled, Ordering::Relaxed);
    if enabled {
        if app.get_webview_window(LABEL).is_none() {
            log::warn!("对话独立窗口不存在（启动期建窗失败），本次开启无法恢复，重启后可用");
        }
        return;
    }
    hide_window(app);
}

/// 供 commands::save_config 以磁盘为准保留的字段集（独立窗几何/开关均由后端管理，
/// 防止主窗旧快照把拖拽后的位置覆盖回去）
pub fn preserve_disk_fields(merged: &mut config::AppConfig, disk: &config::AppConfig) {
    merged.chat_window_mode = disk.chat_window_mode;
    merged.chat_window_width = disk.chat_window_width;
    merged.chat_window_height = disk.chat_window_height;
    merged.chat_window_x = disk.chat_window_x;
    merged.chat_window_y = disk.chat_window_y;
    merged.chat_window_pinned = disk.chat_window_pinned;
}

#[derive(serde::Serialize)]
pub struct ChatWindowState {
    pub mode: bool,
    pub pinned: bool,
    pub visible: bool,
}

/// 查询独立窗状态（ChatWindow 挂载时自取置顶初值）
#[tauri::command]
pub fn chat_window_get_state(app: AppHandle) -> ChatWindowState {
    let cfg = config::load();
    ChatWindowState {
        mode: cfg.chat_window_mode,
        pinned: cfg.chat_window_pinned,
        visible: is_visible(&app),
    }
}

/// 唤起 / 收起独立对话窗（前端标题栏按钮与快捷键在独立模式下的入口）
#[tauri::command]
pub fn chat_window_toggle(app: AppHandle) {
    toggle(&app);
}

/// 收起独立对话窗（ChatWindow 自制标题栏的关闭钮：隐藏而非销毁）
#[tauri::command]
pub fn chat_window_close(app: AppHandle) {
    hide_window(&app);
}

/// 切换置顶并持久化
#[tauri::command]
pub fn chat_window_set_pinned(app: AppHandle, pinned: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(LABEL) {
        win.set_always_on_top(pinned).map_err(|e| e.to_string())?;
    }
    let _guard = config::lock();
    let mut cfg = config::load();
    cfg.chat_window_pinned = pinned;
    config::save(&cfg)
}

/// 设置页开关：切换「以独立窗口打开 AI 对话」并落盘 + 落地窗口形态。
/// 通知主窗同步形态（内嵌抽屉此时收起，避免两种形态同屏）。
#[tauri::command]
pub fn chat_window_save_mode(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let _guard = config::lock();
        let mut cfg = config::load();
        if cfg.chat_window_mode != enabled {
            cfg.chat_window_mode = enabled;
            config::save(&cfg).map_err(|e| e.to_string())?;
        }
    }
    apply_mode(&app, enabled);
    let _ = app.emit_to("main", "chat-window-mode", enabled);
    log::info!("AI 对话独立窗口模式: {}", enabled);
    Ok(())
}

/// 独立窗内点「模型设置」/积分不足「去兑换」：唤出主窗并定位到设置页对应分区。
/// section 为空 = 「providers」（供应商，模型设置的落点）；"account" = 账号页（积分徽标 / 429 分流卡片用）
#[tauri::command]
pub fn chat_window_open_settings(app: AppHandle, section: Option<String>) {
    crate::tray::show_window(&app);
    use tauri::Emitter;
    let _ = app.emit_to("main", "open-chat-settings", section.unwrap_or_else(|| "providers".into()));
}
