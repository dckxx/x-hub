//! 截图 + GIF 区域录制：xcap 屏幕采集（Windows 走 WGC，GPU 加速）+ gif/color_quant 编码。
//!
//! 两个功能共用同一张全屏透明「选区层」（recorder-overlay）：拖拽框选，Esc 取消。
//! - 截图（shot）：框选松手直接采单帧存 PNG 到 数据根\captures\ 并复制剪贴板（不叠光标）；
//! - GIF 录制（record）：框选后设帧率/时长，逐帧采集写 PNG 临时文件，停止后两遍编码
//!   （NeuQuant 训练 256 色全局调色板 → 逐帧索引写出），光标经 Win32 GDI 逐帧叠加。
//!
//! 性能/内存要点（自 gif-recorder 分支移植时保留）：
//! - 录制帧写 PNG 临时文件（比裸 RGBA 省 90%+ 磁盘），内存只留帧路径表和有上限的调色板采样；
//! - 仅依赖 xcap/gif/color_quant 三个新增 crate，不捆绑 ffmpeg。
//!
//! 窗口约定（AGENTS.md 约定 41）：两窗启动期 init 预创建隐藏常驻，运行期只重定位 + 显隐，
//! 绝不运行期 build/destroy；快捷键与命令都必须经 async 命令或 app.listen 回调进入
//! （同步命令跑在主线程，现场建窗/操作窗口会互相阻塞死锁，与倒计时浮窗同一坑）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

/// 选区窗口 label（覆盖在目标显示器上，拖拽框选；截图与 GIF 录制共用）
pub const RECORDER_OVERLAY_LABEL: &str = "recorder-overlay";
/// 录制控制条窗口 label（录制中悬浮的计时 + 停止条）
pub const RECORDER_CONTROL_LABEL: &str = "recorder-control";
/// 贴图窗口 label 前缀（pin-shot-0..PIN_WINDOW_COUNT-1，盯住功能）
pub const PIN_LABEL_PREFIX: &str = "pin-shot-";

/// 屏幕区域（物理像素、屏幕坐标）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RecRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 选区模式：截图（框选松手直接出图）/ GIF 录制（框选后设参数再录）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Shot,
    Record,
}

impl OverlayMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "shot" => Ok(Self::Shot),
            "record" => Ok(Self::Record),
            _ => Err("未知的选区模式".into()),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Shot => "shot",
            Self::Record => "record",
        }
    }
}

/// 选区层唤起事件载荷：目标显示器物理原点/尺寸 + 本次模式 + 初始预选框。
/// 前端用它把 CSS 坐标换算成物理像素，按模式切换交互；hover 有值时选框直接
/// 预选为光标下的窗口（窗口自动识别，用户可调整/重选）。
#[derive(Debug, Clone, Serialize)]
struct OverlayShownPayload {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    mode: String,
    hover: Option<RecRegion>,
}

/// GIF 录制结果（返回给控制条窗口）
#[derive(Debug, Clone, Serialize)]
pub struct RecorderResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub size_bytes: u64,
}

/// 截图结果（图已复制剪贴板，不落盘）
#[derive(Debug, Clone, Serialize)]
pub struct CaptureShotResult {
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
}

/// 截图标注：一笔/一形 = 归一化坐标（相对选区左上角 0..1）。
/// - stroke：points 为轨迹点列（画笔）
/// - rect / ellipse：x/y/w/h 为包围盒（矩形 / 圆形标注）
/// 前端实时渲染预览，确认/钉住时交后端合成进成图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureAnnotation {
    /// "stroke" | "rect" | "ellipse"
    pub kind: String,
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub w: f32,
    #[serde(default)]
    pub h: f32,
}

/// 贴图结果（图同时在剪贴板 + 钉屏展示）
#[derive(Debug, Clone, Serialize)]
pub struct PinShotResult {
    /// 贴图窗口序号（pin-shot-{slot}）
    pub slot: usize,
    /// 落盘路径（captures/pins，贴图窗口经 asset 协议渲染；关闭贴图时删除）
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// 贴图窗口池：预创建 PIN_WINDOW_COUNT 个隐藏常驻（约定 41 运行期绝不建窗），
/// 每张贴图占一个 slot，关闭即释放。全满时报错提示先关闭（不做自动回收最旧）。
pub const PIN_WINDOW_COUNT: usize = 3;

#[derive(Default)]
pub struct PinState {
    /// 每个 slot 的贴图文件路径（None = 空闲）
    slots: Mutex<Vec<Option<String>>>,
}

/// GIF 录制会话状态（全局托管）
#[derive(Default)]
pub struct RecorderState {
    pub inner: Mutex<Option<ActiveRecording>>,
    /// 窗口自动识别跟随锁：用户首次交互（拖框/调整/移动/标注）后置 true，
    /// hover 跟随线程看到即停（选框交还给用户手动控制）
    pub hover_locked: AtomicBool,
}

pub struct ActiveRecording {
    pub stop_flag: Arc<AtomicBool>,
    pub worker: Option<std::thread::JoinHandle<()>>,
    pub region: RecRegion,
    pub fps: u32,
    pub tmp_dir: PathBuf,
    pub frame_files: Arc<Mutex<Vec<PathBuf>>>,
    /// 每帧采集完成时刻（相对录制开始的 ms，与 frame_files 同序）：
    /// GIF delay 按相邻帧**实际间隔**写——WGC 采集耗时不可控，按理论帧率写 delay
    /// 会让播放速度被压缩几十倍（2026-10-30 实测 30s 录出 2s 的快放）
    pub frame_times: Arc<Mutex<Vec<u64>>>,
    pub samples: Arc<Mutex<Vec<u8>>>,
    pub started: Instant,
    pub frames: Arc<std::sync::atomic::AtomicU64>,
}

/// 截图 / 录屏产物目录：`recorder_save_dir`（设置 → 功能可改，空 = 默认 数据根\captures\）。
/// 自定义目录失效（被删/无权限）时回退默认，绝不因目录失效丢功能。
fn captures_dir() -> Result<PathBuf, String> {
    let configured = crate::config::load().recorder_save_dir;
    let dir = PathBuf::from(configured.trim());
    let dir = if configured.trim().is_empty() {
        crate::paths::data_root().join("captures")
    } else {
        match std::fs::create_dir_all(&dir) {
            Ok(()) => dir,
            Err(e) => {
                log::warn!("[GIF 录制] 自定义保存目录不可用（{e}），回退默认目录");
                let fallback = crate::paths::data_root().join("captures");
                std::fs::create_dir_all(&fallback).map_err(|e| format!("创建录制目录失败: {e}"))?;
                fallback
            }
        }
    };
    if dir == crate::paths::data_root().join("captures") {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建录制目录失败: {e}"))?;
    }
    Ok(dir)
}

/// 调色板采样上限（像素数）。NeuQuant 在 samplefac=10 下足够；防止超长录制撑爆内存。
const SAMPLE_PIXEL_CAP: usize = 2_000_000;

// ---------------------------------------------------------------------------
// 窗口生命周期：启动预创建，运行期只重定位 + 显隐
// ---------------------------------------------------------------------------

/// 预创建选区层与控制条（lib.rs setup 调用，隐藏常驻；运行期绝不建窗，见约定 41）
pub fn init(app: &AppHandle) {
    if app.get_webview_window(RECORDER_OVERLAY_LABEL).is_none() {
        match tauri::WebviewWindowBuilder::new(
            app,
            RECORDER_OVERLAY_LABEL,
            // 轻量入口 recorder.html（P2 续，2026-10-31）：只挂 RecorderOverlay，
            // 不加载完整 SPA（内存优化，见 src/light/recorder.ts）
            tauri::WebviewUrl::App("recorder.html".into()),
        )
        .title("截图 / 录屏选区")
        .inner_size(800.0, 600.0) // 占位；每次唤起按目标显示器物理尺寸重设
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .shadow(false)
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
        .build()
        {
            Ok(win) => {
                crate::lockdown_webview(&win);
                crate::win_taskbar::apply(&win);
            }
            Err(e) => log::warn!("[截图/录屏] 预创建选区层失败: {e}"),
        }
    }
    if app.get_webview_window(RECORDER_CONTROL_LABEL).is_none() {
        match tauri::WebviewWindowBuilder::new(
            app,
            RECORDER_CONTROL_LABEL,
            tauri::WebviewUrl::App("recorder.html".into()),
        )
        .title("GIF 录制")
        .inner_size(200.0, 64.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .shadow(false)
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
        .build()
        {
            Ok(win) => {
                crate::lockdown_webview(&win);
                crate::win_taskbar::apply(&win);
            }
            Err(e) => log::warn!("[截图/录屏] 预创建控制条失败: {e}"),
        }
    }
    // 贴图窗口池（盯住功能）：每张钉屏贴图占一个窗口，预创建隐藏常驻复用
    for i in 0..PIN_WINDOW_COUNT {
        let label = format!("{}{}", PIN_LABEL_PREFIX, i);
        if app.get_webview_window(&label).is_some() {
            continue;
        }
        match tauri::WebviewWindowBuilder::new(
            app,
            &label,
            // 轻量入口 pin.html（P2 续，2026-10-31）：只挂 PinShotWindow，
            // 不加载完整 SPA（三个贴图窗启动即隐藏常驻，见 src/light/pin.ts）
            tauri::WebviewUrl::App("pin.html".into()),
        )
        .title("贴图")
        .inner_size(320.0, 240.0) // 占位；每次贴图按选区物理尺寸重设
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .shadow(false)
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
        .build()
        {
            Ok(win) => {
                crate::lockdown_webview(&win);
                crate::win_taskbar::apply(&win);
            }
            Err(e) => log::warn!("[截图/录屏] 预创建贴图窗口 {label} 失败: {e}"),
        }
    }
}

/// 收掉选区层：恢复点击（录制中曾开启穿透）+ 隐藏 + 反注册 Esc 热键 + 内存级别降档。
/// 同时 emit `recorder-overlay-hidden` 让前端立刻清掉选框/标注状态——窗口复用，
/// 若等下次 show 时的 shown 事件才清，旧状态会先渲染出来（闪一下上次的框）。
pub fn hide_overlay(app: &AppHandle) {
    unregister_esc_hotkey(app);
    if let Some(win) = app.get_webview_window(RECORDER_OVERLAY_LABEL) {
        let _ = win.set_ignore_cursor_events(false);
        let _ = win.hide();
    }
    let _ = app.emit_to(RECORDER_OVERLAY_LABEL, "recorder-overlay-hidden", ());
    crate::webview_mem::on_hidden(app, RECORDER_OVERLAY_LABEL);
}

// ---------------------------------------------------------------------------
// Esc 快捷取消（全局热键，动态注册）
// ---------------------------------------------------------------------------

/// 选区层显示期间全局注册 Esc：唤起选区层的 SetForegroundWindow 在前台锁定下
/// 可能没把键盘焦点真正落到 webview，前端 keydown 收不到 Esc（2026-10-31 实测）；
/// RegisterHotKey 不依赖焦点，且系统授予热键接收进程前台权，显示期间劫持 Esc
/// 正是截图流程的预期语义。收窗（hide_overlay）与开始录制（进入录制态，Esc 不再
/// 退出）时反注册，绝不常驻劫持。
///
/// ⚠️ 注册/反注册**必须脱离同步 emit 链**（spawn 到 async runtime）：global_shortcut
/// 的 register/unregister 要把请求发给插件的热键事件循环线程并等它完成，而
/// open_overlay/hide_overlay 常从 with_handler → emit → listen 的同步回调链进入——
/// 事件循环线程正阻塞在 handler 里等 emit 返回，再等它处理注册请求 = 互相等死
/// （2026-10-31 实测：按快捷键唤起选区层整应用卡死）。
const ESC_HOTKEY: &str = "Escape";

fn register_esc_hotkey(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // 已注册（重复唤起）幂等跳过；失败仅落日志（前端 keydown 仍是兜底）
        if crate::shortcut::is_shortcut_registered(&app, ESC_HOTKEY) {
            return;
        }
        if let Err(e) = crate::shortcut::register_toggle_shortcut(&app, ESC_HOTKEY) {
            log::warn!("[截图/录屏] 注册 Esc 快捷取消失败（前端按键兜底仍可用）: {e}");
        }
    });
}

fn unregister_esc_hotkey(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // 快速连按场景：hide 的反注册还在路上、选区层已被重新唤起——跳过，别把
        // 刚注册的 Esc 拆掉
        let visible = app
            .get_webview_window(RECORDER_OVERLAY_LABEL)
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if visible {
            return;
        }
        if crate::shortcut::is_shortcut_registered(&app, ESC_HOTKEY) {
            if let Err(e) = crate::shortcut::unregister_toggle_shortcut(&app, ESC_HOTKEY) {
                log::warn!("[截图/录屏] 反注册 Esc 失败: {e}");
            }
        }
    });
}

/// 收掉控制条（复用窗口，只 hide 不 destroy）
fn hide_control(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(RECORDER_CONTROL_LABEL) {
        let _ = win.hide();
    }
    crate::webview_mem::on_hidden(app, RECORDER_CONTROL_LABEL);
}

// ---------------------------------------------------------------------------
// 选区层唤起
// ---------------------------------------------------------------------------

/// 唤起选区覆盖层（覆盖光标所在显示器；截图与 GIF 录制共用，按 mode 切换前端交互）。
/// 必须 async 命令或 app.listen 回调进入：同步命令跑在主线程，窗口操作会互相阻塞死锁。
pub fn open_overlay(app: &AppHandle, mode: OverlayMode) -> Result<(), String> {
    {
        let state = app.state::<RecorderState>();
        let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            // 死会话自愈：采集线程已退出（到限/panic/找不到显示器等）而 stop 没被调用的
            // 残留——worker 已 finished 就地清掉，别让「正在录制中」把快捷键永久堵死
            //（到限主收尾已移入采集线程，这里是 panic 等异常路径的兜底）
            let worker_done = guard
                .as_ref()
                .and_then(|rec| rec.worker.as_ref())
                .map(|w| w.is_finished())
                .unwrap_or(true);
            if worker_done {
                if let Some(rec) = guard.take() {
                    if let Some(w) = rec.worker {
                        let _ = w.join();
                    }
                    let _ = std::fs::remove_dir_all(&rec.tmp_dir);
                    log::warn!("[截图/录屏] 检测到未收尾的死录制会话，已自动清理");
                }
            } else {
                return Err("正在录制中，请先停止".to_string());
            }
        }
    }

    // 目标显示器：光标所在显示器；取不到光标则退回主显示器
    let mon = monitor_under_cursor()
        .or_else(primary_monitor)
        .ok_or("未找到可用显示器")?;
    let mx = mon.x().unwrap_or(0);
    let my = mon.y().unwrap_or(0);
    let mw = mon.width().unwrap_or(1920);
    let mh = mon.height().unwrap_or(1080);

    let Some(win) = app.get_webview_window(RECORDER_OVERLAY_LABEL) else {
        return Err("选区窗口未就绪，请重启应用".to_string());
    };
    // 先按逻辑坐标创建（init），再改用物理坐标铺满目标显示器（xcap 返回的是物理像素，
    // HiDPI 下 builder 的 inner_size/position 是逻辑值，直接传会错位）。
    // resizable(false) 会把 min/max 锁死、后续 set_size 被 WM_GETMINMAXINFO 静默钳制
    // （notify.rs::apply_size 同坑）：临时解锁 → 物理尺寸 → 锁回。
    let _ = win.set_resizable(true);
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(mx, my)));
    let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(mw, mh)));
    let _ = win.set_resizable(false);
    // 通知选区窗口目标显示器原点/尺寸与本次模式（前端换算 CSS↔物理、切换交互）。
    // **先 emit 再 show**：组件收到即重置选框状态，窗口可见时渲染的就是干净状态
    //（show 在前的话，事件到达前的间隙会把上次会话的旧选框渲染出来——闪框）。
    // hover = 光标下窗口的初始预选框（窗口自动识别）
    let hover = window_rect_under_cursor(app);
    let _ = app.emit_to(
        RECORDER_OVERLAY_LABEL,
        "recorder-overlay-shown",
        OverlayShownPayload {
            x: mx,
            y: my,
            width: mw,
            height: mh,
            mode: mode.as_str().to_string(),
            hover,
        },
    );
    crate::webview_mem::on_shown(app, RECORDER_OVERLAY_LABEL);
    crate::win_taskbar::show(&win);
    let _ = win.set_focus();
    // 全局注册 Esc 快捷取消（收窗/开录时反注册，见 register_esc_hotkey 注释）
    register_esc_hotkey(app);
    // 窗口自动识别跟随：用户首次交互前，光标切到别的窗口时选框自动跟随
    {
        let state = app.state::<RecorderState>();
        state.hover_locked.store(false, Ordering::Relaxed);
    }
    spawn_hover_follow(app.clone(), RecRegion { x: mx, y: my, width: mw, height: mh });
    log::info!(
        "[截图/录屏] 选区层已显示（{}）：{}x{} @ ({},{})",
        mode.as_str(),
        mw,
        mh,
        mx,
        my
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 唤起选区层（快捷键事件与标题栏按钮共用；mode = "shot" | "record"）
#[tauri::command]
pub async fn recorder_open_overlay(app: AppHandle, mode: String) -> Result<(), String> {
    open_overlay(&app, OverlayMode::parse(&mode)?)
}

/// 收起选区层（Esc 取消；只是隐藏，窗口复用）
#[tauri::command]
pub async fn recorder_close_overlay(app: AppHandle) -> Result<(), String> {
    hide_overlay(&app);
    Ok(())
}

/// 选区裁剪到所在显示器边界（截图与录制共用）：xcap 采集参数不允许越界
fn clamp_and_clip_region(region: RecRegion) -> Option<RecRegion> {
    let width = region.width.clamp(16, 7680);
    let height = region.height.clamp(16, 7680);
    let mon = find_monitor(region.x, region.y)?;
    let mx = mon.x().unwrap_or(0);
    let my = mon.y().unwrap_or(0);
    let mw = mon.width().unwrap_or(1920);
    let mh = mon.height().unwrap_or(1080);
    let rx = (region.x - mx).max(0);
    let ry = (region.y - my).max(0);
    let w = (width as i32).min(mw as i32 - rx).max(16) as u32;
    let h = (height as i32).min(mh as i32 - ry).max(16) as u32;
    Some(RecRegion { x: mx + rx, y: my + ry, width: w, height: h })
}

/// 采屏内核：收窗 → 等 DWM → 采单帧 → 合成标注 → PNG 字节。
/// 截图与贴图共用（两者都是「此刻的屏幕 + 标注」成图进剪贴板）。
fn capture_png_bytes(app: &AppHandle, region: RecRegion, annotations: &[CaptureAnnotation]) -> Result<(Vec<u8>, u32, u32), String> {
    let region = clamp_and_clip_region(region).ok_or("选区不在任何显示器内")?;

    hide_overlay(app);
    std::thread::sleep(Duration::from_millis(120));

    let mut rgba = gdi_capture_region(&region)?;
    draw_annotations(&mut rgba, region.width, region.height, annotations);

    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(region.width, region.height, rgba)
        .ok_or("图像缓冲区尺寸不匹配")?
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| format!("编码 PNG 失败: {e}"))?;
    Ok((png.into_inner(), region.width, region.height))
}

/// 采屏后复制剪贴板的公共尾（截图/贴图共用；清自写指纹让它正常进剪贴板历史）
fn copy_png_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    crate::clipboard::set_clipboard_image_bytes(png_bytes)?;
    crate::clipboard::clear_self_set_fingerprint();
    Ok(())
}

/// 截图：框选区域采单帧（不叠光标，画面干净），标注合成进成图，
/// 内存编码 PNG **直接复制剪贴板，不落盘**。
#[tauri::command]
pub async fn recorder_capture_shot(
    app: AppHandle,
    region: RecRegion,
    annotations: Vec<CaptureAnnotation>,
) -> Result<CaptureShotResult, String> {
    let (png_bytes, width, height) = capture_png_bytes(&app, region, &annotations)?;
    let size_bytes = png_bytes.len() as u64;

    // 复制剪贴板（不落盘——截图是一次性传递产物，钉住走 pins 另有其文件）
    let clip_err = match copy_png_to_clipboard(&png_bytes) {
        Ok(()) => None,
        Err(e) => {
            log::warn!("[截图] 复制剪贴板失败: {e}");
            Some(e)
        }
    };

    // 反馈走主窗轻 toast（不再右下角系统通知）；主窗隐藏时 toast 不可见，剪贴板已就位即可
    let message = match &clip_err {
        None => "截图已复制到剪贴板".to_string(),
        Some(e) => format!("截图复制失败：{e}"),
    };
    let _ = app.emit_to(
        "main",
        "shot-done",
        serde_json::json!({
            "message": message,
            "size_bytes": size_bytes,
            "width": width,
            "height": height,
        }),
    );
    log::info!(
        "[截图] {} {}x{} {:.1} KB（{} 个标注）",
        if clip_err.is_some() { "剪贴板失败" } else { "已复制" },
        width,
        height,
        size_bytes as f64 / 1024.0,
        annotations.len()
    );
    Ok(CaptureShotResult {
        width,
        height,
        size_bytes,
    })
}

/// 贴图（盯住）：截图同款采屏 + 复制剪贴板，图**落盘 captures/pins**（asset 协议渲染，
/// 关闭贴图时删除）并钉到选区原位置置顶显示，可拖动、随时参考。
#[tauri::command]
pub async fn recorder_pin_shot(
    app: AppHandle,
    region: RecRegion,
    annotations: Vec<CaptureAnnotation>,
) -> Result<PinShotResult, String> {
    let pinned = clamp_and_clip_region(region).ok_or("选区不在任何显示器内")?;
    let (png_bytes, width, height) = capture_png_bytes(&app, region, &annotations)?;

    // 找空闲 slot（全满报错，提示先关闭）
    let state = app.state::<PinState>();
    let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
    if slots.len() < PIN_WINDOW_COUNT {
        slots.resize_with(PIN_WINDOW_COUNT, || None);
    }
    let Some(slot) = slots.iter().position(|s| s.is_none()) else {
        return Err(format!("贴图已达 {PIN_WINDOW_COUNT} 张上限，先关闭一些再试"));
    };

    // 落盘 pins（asset 协议渲染源；关闭贴图时删除）
    let pins_dir = crate::paths::data_root().join("captures").join("pins");
    std::fs::create_dir_all(&pins_dir).map_err(|e| format!("创建贴图目录失败: {e}"))?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let path = pins_dir.join(format!("pin_{stamp}_{slot}.png"));
    std::fs::write(&path, &png_bytes).map_err(|e| format!("写入贴图文件失败: {e}"))?;
    slots[slot] = Some(path.to_string_lossy().into_owned());

    // 定位窗口到选区原位置（物理像素；resizable(false) 的 set_size 钳制坑同选区层）
    let label = format!("{}{}", PIN_LABEL_PREFIX, slot);
    let Some(win) = app.get_webview_window(&label) else {
        // 窗口池异常：回滚 slot 占用并删除落盘文件
        if let Some(s) = slots.get_mut(slot) {
            *s = None;
        }
        drop(slots);
        let _ = std::fs::remove_file(&path);
        return Err("贴图窗口未就绪，请重启应用".into());
    };
    drop(slots);
    let _ = win.set_resizable(true);
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        pinned.x,
        pinned.y,
    )));
    let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(width, height)));
    let _ = win.set_resizable(false);

    crate::webview_mem::on_shown(&app, &label);
    crate::win_taskbar::show(&win);
    // 钉住是用户主动动作：给焦点让贴图能直接收 Esc 关闭（点别处焦点自然让出，不影响对照录入）
    let _ = win.set_focus();
    // 通知组件拉取本 slot 的图（窗口复用、组件只在启动时挂载一次，**必须**靠事件
    // 触发重新拉取——onMounted 里的初次拉取发生在启动时，slot 还是空的）
    let _ = app.emit_to(&label, "pin-shot-shown", ());
    log::info!(
        "[贴图] 已钉屏 slot {slot}: {}x{} @ ({},{}) {:.1} KB",
        width,
        height,
        pinned.x,
        pinned.y,
        png_bytes.len() as f64 / 1024.0
    );
    Ok(PinShotResult {
        slot,
        path: path.to_string_lossy().into_owned(),
        width,
        height,
    })
}

/// 贴图窗口取图：PinShotWindow 挂载后按自己的窗口拉取（拉取模式防 emit 早于监听）。
/// 返回**文件系统绝对路径**（前端 convertFileSrc 转换，勿在 Rust 侧拼 asset URL——
/// Windows 的 asset 协议是 http://asset.localhost/<encoded>，手写 asset:// 必坏）；
/// slot 引用保留到关闭贴图时统一删文件
#[tauri::command]
pub async fn recorder_pin_take(window: tauri::WebviewWindow) -> Result<String, String> {
    let app = window.app_handle().clone();
    let slot = window
        .label()
        .strip_prefix(PIN_LABEL_PREFIX)
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| format!("窗口 {} 不是贴图窗口", window.label()))?;
    if slot >= PIN_WINDOW_COUNT {
        return Err("贴图序号越界".into());
    }
    let state = app.state::<PinState>();
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    let path = slots
        .get(slot)
        .cloned()
        .unwrap_or_default()
        .ok_or("该贴图窗口没有待显示的图")?;
    drop(slots);
    Ok(path)
}

/// 关闭贴图：清 slot + 删文件 + 收窗
#[tauri::command]
pub async fn recorder_pin_close(window: tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle().clone();
    let slot = match window
        .label()
        .strip_prefix(PIN_LABEL_PREFIX)
        .and_then(|s| s.parse::<usize>().ok())
    {
        Some(s) if s < PIN_WINDOW_COUNT => s,
        _ => return Ok(()),
    };
    let state = app.state::<PinState>();
    let mut slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(Some(path)) = slots.get_mut(slot).map(|s| s.take()) {
        let _ = std::fs::remove_file(&path);
    }
    drop(slots);
    let _ = window.hide();
    crate::webview_mem::on_hidden(&app, window.label());
    log::info!("[贴图] 已关闭 slot {slot}");
    Ok(())
}

/// 标注合成：红色圆头笔画/形状画进 RGBA 帧。坐标是归一化（相对选区），按实际尺寸缩放；
/// 线宽随选区大小自适应（小区域不至于糊成一团，4K 全屏不至于细成发丝）。
/// kind：stroke = 画笔折线；rect = 矩形边框；ellipse = 圆形边框（参数方程步进）。
fn draw_annotations(rgba: &mut [u8], width: u32, height: u32, annotations: &[CaptureAnnotation]) {
    if annotations.is_empty() {
        return;
    }
    let (w, h) = (width as f32, height as f32);
    let lw = (w.min(h) / 220.0).round().clamp(2.0, 8.0) as i32; // 线宽（物理像素）
    let r = lw / 2;
    let (cr, cg, cb) = (0xF5u8, 0x45u8, 0x45u8); // 红

    let put_pixel = |rgba: &mut [u8], x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return;
        }
        let fi = ((y as u32 * width + x as u32) * 4) as usize;
        // 不透明覆盖（屏幕内容本就 alpha=255）
        rgba[fi] = cr;
        rgba[fi + 1] = cg;
        rgba[fi + 2] = cb;
        rgba[fi + 3] = 255;
    };
    // 圆头端点/接点：以点为圆心填 r 半径实心圆
    let fill_dot = |rgba: &mut [u8], x: f32, y: f32| {
        let (cx, cy) = (x.round() as i32, y.round() as i32);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    put_pixel(rgba, cx + dx, cy + dy);
                }
            }
        }
    };
    // 两点间沿直线铺圆头点（stroke 线段 / 矩形边 / 椭圆弧共用）
    let line = |rgba: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32| {
        fill_dot(rgba, x1, y1);
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            fill_dot(rgba, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        }
    };

    for a in annotations {
        match a.kind.as_str() {
            "stroke" => {
                let pts: Vec<(f32, f32)> = a
                    .points
                    .iter()
                    .map(|p| (p[0].clamp(0.0, 1.0) * w, p[1].clamp(0.0, 1.0) * h))
                    .collect();
                if pts.is_empty() {
                    continue;
                }
                // 单点 = 点一下，也画圆头
                fill_dot(rgba, pts[0].0, pts[0].1);
                for pair in pts.windows(2) {
                    let (x0, y0) = pair[0];
                    let (x1, y1) = pair[1];
                    line(rgba, x0, y0, x1, y1);
                }
            }
            "rect" | "ellipse" => {
                let x0 = a.x.clamp(0.0, 1.0) * w;
                let y0 = a.y.clamp(0.0, 1.0) * h;
                let x1 = (a.x + a.w).clamp(0.0, 1.0) * w;
                let y1 = (a.y + a.h).clamp(0.0, 1.0) * h;
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                if a.kind == "rect" {
                    // 四条边
                    line(rgba, x0, y0, x1, y0);
                    line(rgba, x1, y0, x1, y1);
                    line(rgba, x1, y1, x0, y1);
                    line(rgba, x0, y1, x0, y0);
                } else {
                    // 椭圆边框：参数方程步进，步数按周长取够密（相邻点距 ≤ r 保线条连续）
                    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
                    let steps = ((std::f32::consts::PI * (rx + ry)).ceil() as i32).clamp(64, 4096);
                    let mut prev = (cx + rx, cy);
                    for i in 1..=steps {
                        let t = i as f32 / steps as f32 * std::f32::consts::TAU;
                        let pt = (cx + rx * t.cos(), cy + ry * t.sin());
                        line(rgba, prev.0, prev.1, pt.0, pt.1);
                        prev = pt;
                    }
                }
            }
            _ => {}
        }
    }
}

/// 开始录制：选区层切录制态（红色边框 + 点击穿透）、后台采集线程、悬浮控制条。
/// fps 传 None 时读配置默认值（recorder_fps，设置 → 功能可改）——选区层不再逐次选择。
#[tauri::command]
pub async fn recorder_start(
    app: AppHandle,
    region: RecRegion,
    fps: Option<u32>,
    max_seconds: u32,
) -> Result<(), String> {
    // ---- 参数校验 ----
    let fps = fps
        .unwrap_or_else(|| crate::config::load().recorder_fps)
        .clamp(1, 60);
    let max_seconds = max_seconds.clamp(1, 300);
    let region = clamp_and_clip_region(region).ok_or("选区不在任何显示器内")?;

    let state = app.state::<RecorderState>();
    // ---- 第一段 lock：校验 + 建临时目录 + 组会话数据（不 spawn）----
    let (stop_flag, frame_files, frame_times, samples, sample_full, frames, tmp_dir) = {
        let guard = state.inner.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Err("正在录制中，请先停止".to_string());
        }

        // ---- 临时文件 ----
        let rec_dir = captures_dir()?;
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let tmp_dir = rec_dir.join(format!(".tmp_{stamp}"));
        std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;

        (
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(Vec::<PathBuf>::new())),
            Arc::new(Mutex::new(Vec::<u64>::new())),
            Arc::new(Mutex::new(Vec::<u8>::new())),
            Arc::new(AtomicBool::new(false)),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            tmp_dir,
        )
    };

    // ---- 通知前端切录制态（红框外置 4px 显示，屏幕可见、GIF 不入画）----
    // 选区层**保持显示**（用户要始终看得到录制范围）+ 点击穿透（不挡被录应用）；
    // 遮罩/把手/面板由前端 recording=true 隐藏。等 120ms 让红框外置渲染完成 +
    // DWM 合成，第一帧 BitBlt 才不会抓到跨界压在选区边缘上的旧框。
    let _ = app.emit_to(RECORDER_OVERLAY_LABEL, "recorder-started", region);
    if let Some(win) = app.get_webview_window(RECORDER_OVERLAY_LABEL) {
        let _ = win.set_ignore_cursor_events(true);
    }
    std::thread::sleep(Duration::from_millis(120));
    // 录制态 Esc 不再退出选区层（停止走控制条）：反注册全局 Esc，还键于系统
    unregister_esc_hotkey(&app);
    // 录制态不再跟随窗口识别
    state.hover_locked.store(true, Ordering::Relaxed);

    // ---- 第二段 lock：spawn worker + 置会话（started 从此起算）----
    // 会话（rec）与采集线程必须持有**同一个 stop_flag Arc**（stop 命令置旗标、线程读它），
    // 其余帧表/临时目录同理共享；spawn 侧全部用 clone，原件进 rec。
    let started_at = Instant::now();
    let app_handle = app.clone();
    let th_region = region;
    let th_fps = fps;
    let th_max = max_seconds;
    let th_stop_flag = stop_flag.clone();
    let th_tmp_dir = tmp_dir.clone();
    let th_frame_files = frame_files.clone();
    let th_frame_times = frame_times.clone();
    let th_samples = samples.clone();
    let th_sample_full = sample_full.clone();
    let th_frames = frames.clone();
    let worker = std::thread::Builder::new()
        .name("gif-recorder".into())
        .spawn(move || {
            capture_loop(
                app_handle,
                th_region,
                th_fps,
                th_max,
                th_stop_flag,
                th_tmp_dir,
                th_frame_files,
                th_frame_times,
                th_samples,
                th_sample_full,
                th_frames,
                started_at,
            );
        })
        .map_err(|e| format!("启动采集线程失败: {e}"))?;
    {
        let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
        // 防御性二次校验（两段锁之间有 120ms 窗口，前端 ▶ 双击可并发进两次 start）：
        // worker 已经 spawn 出去了——立即叫停并清理，别留一个跑到 max_seconds 的
        // 孤儿采集线程（白采几百 MB 临时帧）+ 泄漏临时目录
        if guard.is_some() {
            stop_flag.store(true, Ordering::SeqCst);
            let _ = worker.join();
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err("正在录制中，请先停止".to_string());
        }
        let rec = ActiveRecording {
            stop_flag,
            worker: Some(worker),
            region,
            fps,
            tmp_dir,
            frame_files,
            frame_times,
            samples,
            started: started_at,
            frames,
        };
        *guard = Some(rec);
    }

    // 控制条（预创建复用）：重定位到选区外侧后显示，并下发新会话重置事件
    // （含 max_seconds，控制条据此在到达时长上限时自动停止——旧实现只 break 不收尾，
    // 计时器继续走、状态停在录制中，用户还得手点一次停止）
    show_control_window(&app, &region);
    let _ = app.emit_to(
        RECORDER_CONTROL_LABEL,
        "recorder-control-start",
        serde_json::json!({ "max_seconds": max_seconds }),
    );

    log::info!(
        "GIF 录制开始: {}x{} @ ({},{}) fps={} max={}s",
        region.width,
        region.height,
        region.x,
        region.y,
        fps,
        max_seconds
    );
    Ok(())
}

/// 停止录制：停止采集线程，两遍编码 GIF，返回结果
#[tauri::command]
pub async fn recorder_stop(app: AppHandle) -> Result<RecorderResult, String> {
    let state = app.state::<RecorderState>();
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let Some(rec) = guard.take() else {
        return Err("当前没有正在进行的录制".to_string());
    };
    // 会话已 take 出来，状态锁必须**在 join 前释放**：到限自停的 finish_from_capture_thread
    // 正在等这把锁，若持锁 join 等它退出就是互相等死（join 等线程、线程等锁）
    drop(guard);

    // 结束录制边框：恢复点击并隐藏选区窗口
    hide_overlay(&app);

    rec.stop_flag.store(true, Ordering::SeqCst);
    if let Some(worker) = rec.worker {
        let _ = worker.join();
    }

    let duration_ms = rec.started.elapsed().as_millis() as u64;
    let frames_count = rec.frames.load(Ordering::Relaxed);

    if frames_count == 0 {
        let _ = std::fs::remove_dir_all(&rec.tmp_dir);
        return Err("未采集到任何帧，请重新录制".to_string());
    }

    // 收集数据（状态锁已在 join 前释放；编码耗时可观，不持任何锁）
    let samples = rec.samples.lock().map_err(|e| e.to_string())?.clone();
    let frame_files = rec.frame_files.lock().map_err(|e| e.to_string())?.clone();
    let frame_times = rec.frame_times.lock().map_err(|e| e.to_string())?.clone();
    let rec_dir = captures_dir()?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let mut out_path = rec_dir.join(format!("gif_{stamp}.gif"));
    let mut n = 2;
    while out_path.exists() {
        out_path = rec_dir.join(format!("gif_{stamp}-{n}.gif"));
        n += 1;
    }

    // 两遍编码（PNG 逐帧解码，内存占用恒定）；delay 按每帧实际采集间隔写，速度忠实
    encode_gif(&frame_files, &frame_times, &rec.region, rec.fps, &samples, &out_path)?;

    let size_bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_dir_all(&rec.tmp_dir);

    let result = RecorderResult {
        path: out_path.to_string_lossy().into_owned(),
        width: rec.region.width,
        height: rec.region.height,
        frames: frames_count,
        duration_ms,
        size_bytes,
    };

    // 主窗轻 toast（带「打开文件夹」）承担完成反馈；控制条立即收起——
    // 已保存态的控制条与 toast 冗余（用户拍板去掉），系统通知同理不再发
    let _ = app.emit_to("main", "recorder-done", result.clone());
    hide_control(&app);
    log::info!(
        "GIF 录制完成: {} 帧, {:?}, {:.1} KB",
        frames_count,
        out_path,
        size_bytes as f64 / 1024.0
    );
    Ok(result)
}

/// 取消录制：丢弃数据，清理临时文件，收掉选区层与控制条
#[tauri::command]
pub async fn recorder_cancel(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RecorderState>();
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let Some(rec) = guard.take() else {
        return Ok(());
    };
    // 同 recorder_stop：join 前必须释放状态锁（防与到限自停收尾互等死锁）
    drop(guard);

    hide_overlay(&app);

    rec.stop_flag.store(true, Ordering::SeqCst);
    if let Some(worker) = rec.worker {
        let _ = worker.join();
    }
    let _ = std::fs::remove_dir_all(&rec.tmp_dir);
    hide_control(&app);
    log::info!("GIF 录制已取消，临时文件已清理");
    Ok(())
}

/// 在资源管理器中显示结果文件
#[tauri::command]
pub fn recorder_reveal(path: String) -> Result<(), String> {
    opener::reveal(&path).map_err(|e| format!("打开所在文件夹失败: {e}"))
}

// ---------------------------------------------------------------------------
// 采集循环
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn capture_loop(
    app: AppHandle,
    region: RecRegion,
    fps: u32,
    max_seconds: u32,
    stop_flag: Arc<AtomicBool>,
    tmp_dir: PathBuf,
    frame_files: Arc<Mutex<Vec<PathBuf>>>,
    frame_times: Arc<Mutex<Vec<u64>>>,
    samples: Arc<Mutex<Vec<u8>>>,
    sample_full: Arc<AtomicBool>,
    frames: Arc<std::sync::atomic::AtomicU64>,
    started: Instant,
) {
    // xcap Monitor 仅非 Windows 采集路径用（Windows 走 GDI BitBlt，见 gdi_capture_region）
    #[cfg(not(target_os = "windows"))]
    let mon = match find_monitor(region.x, region.y) {
        Some(m) => m,
        None => {
            log::error!("GIF 录制：找不到选区所在显示器，终止");
            return;
        }
    };
    #[cfg(not(target_os = "windows"))]
    let (rx, ry) = {
        let mx = mon.x().unwrap_or(0);
        let my = mon.y().unwrap_or(0);
        (region.x - mx, region.y - my)
    };

    let frame_interval = Duration::from_millis((1000 / fps.max(1)) as u64);
    let max_duration = Duration::from_secs(max_seconds as u64);
    let mut last_progress = Instant::now();

    while !stop_flag.load(Ordering::Relaxed) {
        if started.elapsed() >= max_duration {
            log::info!("GIF 录制达到最大时长，自动结束");
            // 到限收尾**在本线程内直接完成**（take 会话 → 编码 → emit → 收窗），
            // 不等控制条调 stop——控制条窗口隐藏/失焦时定时器被节流，可能永远不调，
            // 会话状态 Some 残留，之后每次快捷键都被「正在录制中」拦死（实测踩过）。
            finish_from_capture_thread(
                &app,
                region,
                fps,
                started,
                &tmp_dir,
                &frame_files,
                &frame_times,
                &samples,
                &frames,
                &stop_flag,
            );
            return;
        }
        let t0 = Instant::now();

        // 采集：Windows 走 GDI BitBlt（几 ms/帧）；非 Windows 走 xcap。失败跳过本帧不中断
        let frame_rgba: Result<Vec<u8>, String> = {
            #[cfg(target_os = "windows")]
            { gdi_capture_region(&region) }
            #[cfg(not(target_os = "windows"))]
            {
                mon.capture_region(rx as u32, ry as u32, region.width, region.height)
                    .map(|img| img.into_raw())
                    .map_err(|e| format!("屏幕采集失败: {e}"))
            }
        };
        match frame_rgba {
            Ok(mut rgba) => {
                // 叠加鼠标光标（Windows）
                #[cfg(target_os = "windows")]
                draw_cursor(&mut rgba, region.width, region.height, region.x, region.y);

                // 调色板采样：隔 8 个像素取 1 个，封顶后停止
                if !sample_full.load(Ordering::Relaxed) {
                    let mut s = samples.lock().unwrap_or_else(|e| e.into_inner());
                    for px in rgba.chunks_exact(4).step_by(8) {
                        if s.len() >= SAMPLE_PIXEL_CAP * 4 {
                            sample_full.store(true, Ordering::Relaxed);
                            break;
                        }
                        s.extend_from_slice(&px[..4]);
                    }
                }

                // 帧写 PNG（比裸 RGBA 省 90%+ 磁盘，内存恒定）。
                // 同时记录该帧的真实时刻：编码时按相邻帧实际间隔写 delay，
                // 采集再慢播放速度也忠实于录制的操作节奏
                let n = frames.fetch_add(1, Ordering::Relaxed) + 1;
                let path = tmp_dir.join(format!("{n:06}.png"));
                let save_ok = image::RgbaImage::from_raw(region.width, region.height, rgba)
                    .map(|img| {
                        img.save_with_format(&path, image::ImageFormat::Png).is_ok()
                    })
                    .unwrap_or(false);
                if save_ok {
                    if let Ok(mut ff) = frame_files.lock() {
                        ff.push(path);
                    }
                    if let Ok(mut ft) = frame_times.lock() {
                        // 必须是**从录制开始的绝对毫秒**（started.elapsed），编码时按相邻帧
                        // 差值算 delay；此前误写成 t0.elapsed（本帧采集耗时，每帧都是相近
                        // 的几百 ms），相邻差 ≈ 0 恒回退理论帧率，加速修复从未生效
                        ft.push(started.elapsed().as_millis() as u64);
                    }
                } else {
                    log::warn!("GIF 录制：写帧 PNG 失败");
                }
            }
            Err(e) => log::warn!("GIF 录制：采集帧失败: {e}"),
        }

        // 进度事件（约每 500ms）。带 max_seconds：控制条到达上限时自动触发停止
        if last_progress.elapsed() >= Duration::from_millis(500) {
            last_progress = Instant::now();
            let _ = app.emit_to(
                RECORDER_CONTROL_LABEL,
                "recorder-progress",
                serde_json::json!({
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                    "frames": frames.load(Ordering::Relaxed),
                    "fps": fps,
                    "max_seconds": max_seconds,
                }),
            );
        }

        // 帧率节拍
        let elapsed = t0.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
    log::info!("GIF 录制采集线程结束：{} 帧", frames.load(Ordering::Relaxed));
}

/// 到限自停的线程内收尾：take 会话 → 两遍编码 → emit done → 收选区层与控制条。
/// **不能 join 自己**（会死锁）——worker handle 留在已 take 出的会话里随它去（线程即将 return）。
/// 若 take 落空（用户恰在此刻点了停止、stop 命令先拿走会话）：直接返回，由 stop 命令
/// 负责收尾（它 join 本线程——我们马上 return，无死锁）。
#[allow(clippy::too_many_arguments)]
fn finish_from_capture_thread(
    app: &AppHandle,
    region: RecRegion,
    fps: u32,
    started: Instant,
    tmp_dir: &std::path::Path,
    frame_files: &Arc<Mutex<Vec<PathBuf>>>,
    frame_times: &Arc<Mutex<Vec<u64>>>,
    samples: &Arc<Mutex<Vec<u8>>>,
    frames: &Arc<std::sync::atomic::AtomicU64>,
    stop_flag: &Arc<AtomicBool>,
) {
    // 抢会话：只清自己的（Arc 指针比对，防误伤并发中的新会话）
    {
        let state = app.state::<RecorderState>();
        let Ok(mut guard) = state.inner.lock() else { return };
        match guard.as_ref() {
            Some(rec) if Arc::ptr_eq(&rec.stop_flag, stop_flag) => {
                guard.take();
            }
            _ => return,
        }
    }

    let frames_count = frames.load(Ordering::Relaxed);
    let duration_ms = started.elapsed().as_millis() as u64;
    if frames_count == 0 {
        let _ = std::fs::remove_dir_all(tmp_dir);
        hide_overlay(app);
        hide_control(app);
        log::warn!("[GIF 录制] 到限自停但 0 帧，已清理");
        return;
    }
    let samples = samples.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let frame_files = frame_files.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let frame_times = frame_times.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let Ok(rec_dir) = captures_dir() else {
        // 保存目录不可用（磁盘满/权限）：会话已 take，必须照常收尾——否则选区层
        // 停在点击穿透的录制态、Esc 已反注册，用户无从退出
        let _ = std::fs::remove_dir_all(tmp_dir);
        hide_overlay(app);
        hide_control(app);
        crate::notify::show_notice(app, "info", "GIF 录制失败", "保存目录不可用，本次录制已丢弃");
        return;
    };
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let mut out_path = rec_dir.join(format!("gif_{stamp}.gif"));
    let mut n = 2;
    while out_path.exists() {
        out_path = rec_dir.join(format!("gif_{stamp}-{n}.gif"));
        n += 1;
    }
    if let Err(e) = encode_gif(&frame_files, &frame_times, &region, fps, &samples, &out_path) {
        log::error!("[GIF 录制] 到限自停编码失败: {e}");
        let _ = std::fs::remove_dir_all(tmp_dir);
        hide_overlay(app);
        hide_control(app);
        crate::notify::show_notice(app, "info", "GIF 录制失败", &e);
        return;
    }
    let size_bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_dir_all(tmp_dir);

    let result = RecorderResult {
        path: out_path.to_string_lossy().into_owned(),
        width: region.width,
        height: region.height,
        frames: frames_count,
        duration_ms,
        size_bytes,
    };
    // 主窗轻 toast（带「打开文件夹」）承担完成反馈；控制条立即收起，系统通知不再发
    let _ = app.emit_to("main", "recorder-done", result.clone());
    hide_overlay(app);
    hide_control(app);
    log::info!(
        "[GIF 录制] 到限自停完成: {} 帧, {:?}, {:.1} KB",
        frames_count,
        out_path,
        size_bytes as f64 / 1024.0
    );
}

// ---------------------------------------------------------------------------
// GIF 两遍编码
// ---------------------------------------------------------------------------

/// 相邻帧间隔(ms) → GIF delay（单位 1/100 秒）。GIF delay 字段上限 255（2.55s），
/// 超长间隔（卡顿/截图间隙）由调用方拆成重复帧铺完——按 250 一片，保证总时长忠实。
fn delay_cs_for_interval(interval_ms: u64) -> u16 {
    ((interval_ms as f64 / 10.0).round() as u64).clamp(1, 250) as u16
}

#[cfg(test)]
mod delay_tests {
    use super::delay_cs_for_interval;

    #[test]
    fn delay_matches_real_interval() {
        assert_eq!(delay_cs_for_interval(100), 10); // 10fps
        assert_eq!(delay_cs_for_interval(33), 3); // 30fps
        assert_eq!(delay_cs_for_interval(7), 1); // 下限 1
        assert_eq!(delay_cs_for_interval(0), 1);
    }

    #[test]
    fn long_interval_capped_for_repeat_split() {
        // 2s → 200cs（>2.55s 的由调用方拆重复帧，这里只验 250 封顶）
        assert_eq!(delay_cs_for_interval(2_000), 200);
        assert_eq!(delay_cs_for_interval(9_999), 250);
    }
}

fn encode_gif(
    frame_files: &[PathBuf],
    frame_times: &[u64],
    region: &RecRegion,
    fps: u32,
    samples: &[u8],
    out_path: &std::path::Path,
) -> Result<(), String> {
    let w = region.width as u16;
    let h = region.height as u16;

    if frame_files.is_empty() {
        return Err("没有可编码的帧".to_string());
    }
    if samples.is_empty() {
        return Err("没有调色板采样数据".to_string());
    }

    // 第一遍：NeuQuant 训练全局 256 色板
    let nq = color_quant::NeuQuant::new(10, 256, samples);
    let palette = nq.color_map_rgb();

    let out_file = std::fs::File::create(out_path).map_err(|e| format!("创建 GIF 文件失败: {e}"))?;
    let mut enc = gif::Encoder::new(out_file, w, h, &palette).map_err(|e| format!("初始化 GIF 编码器失败: {e}"))?;
    enc.set_repeat(gif::Repeat::Infinite).map_err(|e| format!("设置循环失败: {e}"))?;

    // RGB -> 调色板索引缓存（屏幕内容颜色重复度高，命中率高）。
    // 上限防极端花屏/噪声画面把唯一色数撑到无界（每条 ~48B，百万色 ≈ 上百 MB）：
    // 超限整表清空重建，内存恒定；正常录屏唯一色数千~万级，远摸不到
    const INDEX_CACHE_CAP: usize = 262_144;
    let mut cache: std::collections::HashMap<u32, u8> = std::collections::HashMap::with_capacity(65536);

    // 每帧 delay = 与下一帧的**实际间隔**（时间戳缺项/错位时回退理论帧率，宁快不错乱）。
    // 间隔超 2.5s 拆重复帧铺完（GIF 单帧 delay 上限 2.55s），总时长与录制节奏一致
    let fallback_cs = delay_cs_for_interval((1000 / fps.max(1)) as u64);

    for (i, path) in frame_files.iter().enumerate() {
        let img = image::open(path)
            .map_err(|e| format!("读取帧图片失败 {}: {e}", path.display()))?
            .into_rgba8();
        let buf = img.as_raw();

        let mut indices = Vec::with_capacity(buf.len() / 4);
        for px in buf.chunks_exact(4) {
            // 忽略 alpha（屏幕内容不透明），按 RGB 缓存索引
            let key = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32;
            let idx = match cache.get(&key) {
                Some(&v) => v,
                None => {
                    if cache.len() >= INDEX_CACHE_CAP {
                        cache.clear();
                    }
                    let v = nq.index_of(px) as u8;
                    cache.insert(key, v);
                    v
                }
            };
            indices.push(idx);
        }

        // 本帧与前帧的间隔(ms)；GIF 语义是「显示完本帧后停留 delay」——
        // 用「下一帧时刻 − 本帧时刻」，最后一帧用理论帧率收尾
        let this_ms = frame_times.get(i).copied();
        let next_ms = frame_times.get(i + 1).copied();
        let mut delay_cs = match (this_ms, next_ms) {
            (Some(a), Some(b)) if b > a => delay_cs_for_interval(b - a),
            _ => fallback_cs,
        };

        let mut frame = gif::Frame::from_indexed_pixels(w, h, indices.clone(), None);
        frame.dispose = gif::DisposalMethod::Any;
        // 重复帧铺超长间隔：同一画面反复写，每片 ≤250cs
        loop {
            let slice = delay_cs.min(250);
            frame.delay = slice;
            enc.write_frame(&frame).map_err(|e| format!("写入帧失败: {e}"))?;
            delay_cs -= slice;
            if delay_cs == 0 {
                break;
            }
        }
    }

    enc.into_inner().map_err(|e| format!("结束 GIF 编码失败: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 显示器 / 光标
// ---------------------------------------------------------------------------

fn monitor_under_cursor() -> Option<xcap::Monitor> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } == 0 {
            return None;
        }
        xcap::Monitor::from_point(pt.x, pt.y).ok()
    }
    #[cfg(not(target_os = "windows"))]
    None
}

fn primary_monitor() -> Option<xcap::Monitor> {
    xcap::Monitor::all()
        .ok()?
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
}

fn find_monitor(x: i32, y: i32) -> Option<xcap::Monitor> {
    xcap::Monitor::from_point(x, y).ok().or_else(primary_monitor)
}

// ---------------------------------------------------------------------------
// 窗口自动识别（框选预选 + hover 跟随）
// ---------------------------------------------------------------------------

/// 最小可识别窗口边（物理像素）：再小的悬浮条/气泡框选意义不大
const HOVER_MIN_SIZE: i32 = 40;

/// 光标下「可截取的顶层窗口」可见边界（物理像素，DWM 扩展框架 = 去窗口阴影的真实矩形）。
/// 枚举顶层窗口按 Z 序取第一个命中者；用枚举而非 WindowFromPoint——全屏透明选区层
/// 自己就是最顶层窗口，WindowFromPoint 恒命中它；枚举法把本应用工具窗直接排除。
#[cfg(target_os = "windows")]
fn window_rect_under_cursor(app: &AppHandle) -> Option<RecRegion> {
    use std::collections::HashSet;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows_sys::Win32::Graphics::Gdi::PtInRect;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetCursorPos, GetWindowRect, IsIconic, IsWindowVisible, GA_ROOT,
    };

    // 本应用工具窗不作为识别目标（选区层/控制条/贴图/通知/剪贴板浮层——半透明或全屏，
    // 框到它们没有意义）；主窗与便签等正常窗口保留（可以截自己）
    let mut skip: HashSet<isize> = HashSet::new();
    for (label, win) in app.webview_windows() {
        if label.starts_with("recorder-")
            || label.starts_with("pin-shot-")
            || label == "notice"
            || label == "clipboard"
        {
            if let Ok(h) = win.hwnd() {
                skip.insert(h.0 as isize);
            }
        }
    }

    struct Ctx {
        pt: POINT,
        skip: HashSet<isize>,
        hit: Option<RECT>,
    }
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        if ctx.hit.is_some() {
            return 1;
        }
        if ctx.skip.contains(&(hwnd as isize)) {
            return 1;
        }
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        if ctx.skip.contains(&(root as isize)) {
            return 1;
        }
        // UWP 挂起/虚拟桌面隐藏的窗口 cloaked，不可见但 IsWindowVisible 为真
        let mut cloaked: u32 = 0;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return 1;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut r) == 0 {
            return 1;
        }
        if r.right - r.left < HOVER_MIN_SIZE || r.bottom - r.top < HOVER_MIN_SIZE {
            return 1;
        }
        if PtInRect(&r, ctx.pt) == 0 {
            return 1;
        }
        // DWM 扩展框架：去掉左右下阴影的真实可见边界
        let mut f = r;
        DwmGetWindowAttribute(
            root,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut f as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if f.right - f.left < HOVER_MIN_SIZE || f.bottom - f.top < HOVER_MIN_SIZE {
            return 1;
        }
        if PtInRect(&f, ctx.pt) == 0 {
            return 1;
        }
        ctx.hit = Some(f);
        1
    }

    let mut pt = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        return None;
    }
    let mut ctx = Ctx { pt, skip, hit: None };
    unsafe { EnumWindows(Some(enum_cb), &mut ctx as *mut Ctx as LPARAM) };
    ctx.hit.map(|f| RecRegion {
        x: f.left,
        y: f.top,
        width: (f.right - f.left).max(1) as u32,
        height: (f.bottom - f.top).max(1) as u32,
    })
}

#[cfg(not(target_os = "windows"))]
fn window_rect_under_cursor(_app: &AppHandle) -> Option<RecRegion> {
    None
}

/// hover 跟随线程：选区层显示期间轮询光标下窗口，矩形变化即 emit 给选区层预选。
/// 用户首次交互（前端置 hover_locked）或选区层隐藏后线程退出。
fn spawn_hover_follow(app: AppHandle, mon: RecRegion) {
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_millis(120));
        let state = app.state::<RecorderState>();
        if state.hover_locked.load(Ordering::Relaxed) {
            return;
        }
        let visible = app
            .get_webview_window(RECORDER_OVERLAY_LABEL)
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if !visible {
            return;
        }
        // 只跟选区层所在显示器（选区层只铺这块屏；跨屏的窗口框画不到）
        let Some(rect) = window_rect_under_cursor(&app) else { continue };
        let mw = mon.width as i32;
        let mh = mon.height as i32;
        if rect.x + rect.width as i32 <= mon.x
            || rect.y + rect.height as i32 <= mon.y
            || rect.x >= mon.x + mw
            || rect.y >= mon.y + mh
        {
            continue;
        }
        let _ = app.emit_to(RECORDER_OVERLAY_LABEL, "recorder-hover-window", rect);
    });
}

/// 用户开始手动交互（拖框/调整/移动/标注）：停掉窗口自动识别跟随，选框交还用户
#[tauri::command]
pub async fn recorder_hover_lock(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RecorderState>();
    state.hover_locked.store(true, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// 屏幕采集（GDI BitBlt）
// ---------------------------------------------------------------------------

/// GDI BitBlt 抓取屏幕指定区域（物理坐标）→ RGBA。
/// **为什么不用 xcap 的 capture_region 逐帧采**：xcap 每次调用都重建 WGC 会话
/// （实测几百 ms/帧），10fps 只能采到 2~3 帧——GIF 一卡一卡的根因。BitBlt 只做
/// 一次 DC 拷贝，选区级只要几 ms。BitBlt 不含鼠标光标，由 draw_cursor 照常叠加。
#[cfg(target_os = "windows")]
fn gdi_capture_region(region: &RecRegion) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, SRCCOPY, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };

    unsafe {
        let hdc_screen = GetDC(std::ptr::null_mut());
        if hdc_screen.is_null() {
            return Err("获取屏幕 DC 失败".into());
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.is_null() {
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("创建内存 DC 失败".into());
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: region.width as i32,
                biHeight: -(region.height as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..std::mem::zeroed()
            },
            ..std::mem::zeroed()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if hbmp.is_null() || bits.is_null() {
            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("创建 DIB 失败".into());
        }
        let old = SelectObject(hdc_mem, hbmp);
        let ok = BitBlt(
            hdc_mem,
            0,
            0,
            region.width as i32,
            region.height as i32,
            hdc_screen,
            region.x,
            region.y,
            SRCCOPY,
        );
        SelectObject(hdc_mem, old);
        if ok == 0 {
            DeleteObject(hbmp);
            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            return Err("BitBlt 屏幕拷贝失败".into());
        }

        // DIB 位是 BGRA（屏幕内容 alpha 无意义），逐像素换序成 RGBA、alpha 置 255
        let len = (region.width as usize) * (region.height as usize) * 4;
        let src = std::slice::from_raw_parts(bits as *const u8, len);
        let mut rgba = vec![0u8; len];
        for (d, s) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = 255;
        }
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);
        Ok(rgba)
    }
}

#[cfg(not(target_os = "windows"))]
fn gdi_capture_region(region: &RecRegion) -> Result<Vec<u8>, String> {
    let mon = find_monitor(region.x, region.y).ok_or("未找到可用显示器")?;
    let mx = mon.x().unwrap_or(0);
    let my = mon.y().unwrap_or(0);
    let img = mon
        .capture_region((region.x - mx) as u32, (region.y - my) as u32, region.width, region.height)
        .map_err(|e| format!("屏幕采集失败: {e}"))?;
    Ok(img.into_raw())
}

// ---------------------------------------------------------------------------
// 鼠标光标叠加（Win32 GDI）
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn draw_cursor(rgba: &mut [u8], width: u32, height: u32, origin_x: i32, origin_y: i32) {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DrawIconEx, GetCursorInfo, GetCursorPos, GetIconInfo, GetSystemMetrics, CURSORINFO, CURSOR_SHOWING,
        DI_NORMAL, ICONINFO, SM_CXCURSOR, SM_CYCURSOR,
    };

    unsafe {
        let mut ci: CURSORINFO = std::mem::zeroed();
        ci.cbSize = size_of::<CURSORINFO>() as u32;
        if GetCursorInfo(&mut ci) == 0 {
            return;
        }
        if ci.flags & CURSOR_SHOWING == 0 {
            return;
        }
        let mut pos: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pos) == 0 {
            return;
        }
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(ci.hCursor, &mut ii) == 0 {
            return;
        }

        // 光标尺寸：优先从位图取，回退系统光标尺寸
        let (mut cw, mut ch) = (GetSystemMetrics(SM_CXCURSOR) as i32, GetSystemMetrics(SM_CYCURSOR) as i32);
        let mut bm: windows_sys::Win32::Graphics::Gdi::BITMAP = std::mem::zeroed();
        if !ii.hbmColor.is_null()
            && windows_sys::Win32::Graphics::Gdi::GetObjectW(
                ii.hbmColor,
                size_of::<windows_sys::Win32::Graphics::Gdi::BITMAP>() as i32,
                &mut bm as *mut _ as *mut c_void,
            ) != 0
            && bm.bmWidth > 0
            && bm.bmHeight > 0
        {
            cw = bm.bmWidth;
            ch = bm.bmHeight;
        }
        if cw <= 0 || ch <= 0 || cw > 256 || ch > 256 {
            let _ = DeleteObject(ii.hbmColor);
            let _ = DeleteObject(ii.hbmMask);
            return;
        }

        // 目标位图（frame 中的区域）
        let left = pos.x - origin_x - ii.xHotspot as i32;
        let top = pos.y - origin_y - ii.yHotspot as i32;
        let (iwidth, iheight) = (width as i32, height as i32);
        if left + cw <= 0 || top + ch <= 0 || left >= iwidth || top >= iheight {
            let _ = DeleteObject(ii.hbmColor);
            let _ = DeleteObject(ii.hbmMask);
            return;
        }

        // DIB 段：DrawIconEx 后直接读位（top-down 32bpp）
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        if hdc.is_null() {
            let _ = DeleteObject(ii.hbmColor);
            let _ = DeleteObject(ii.hbmMask);
            return;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: cw,
            biHeight: -ch,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            ..std::mem::zeroed()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if hbmp.is_null() || bits.is_null() {
            let _ = DeleteDC(hdc);
            let _ = DeleteObject(ii.hbmColor);
            let _ = DeleteObject(ii.hbmMask);
            return;
        }
        // DIB 段内存未初始化：DrawIconEx 只写它实际着色的像素，掩码型光标的透明像素
        // 会留下内存垃圾（随机色 + 随机 alpha）混进 GIF 光标区域——应用图标链路踩过
        // 同款坑（约定 14「整图 alpha 全 0」）。先整体清零：未覆盖像素 alpha=0，
        // 下方混合循环 a==0 直接跳过
        std::ptr::write_bytes(bits as *mut u8, 0, (cw * ch * 4) as usize);
        let old = SelectObject(hdc, hbmp);
        DrawIconEx(hdc, 0, 0, ci.hCursor, cw, ch, 0, std::ptr::null_mut(), DI_NORMAL);
        SelectObject(hdc, old);

        // 读回并 alpha 混合
        let cursor = std::slice::from_raw_parts(bits as *const u8, (cw * ch * 4) as usize);
        let x0 = left.max(0);
        let y0 = top.max(0);
        let x1 = (left + cw).min(iwidth);
        let y1 = (top + ch).min(iheight);
        for fy in y0..y1 {
            for fx in x0..x1 {
                let sx = fx - left;
                let sy = fy - top;
                let ci = (sy * cw + sx) as usize;
                let a = cursor[ci * 4 + 3] as u32;
                if a == 0 {
                    continue;
                }
                // DrawIconEx 写入 32bpp DIB 的像素是预乘 alpha，直接做「源 + 目标×(1-α)」合成
                let r = cursor[ci * 4 + 2] as u32;
                let g = cursor[ci * 4 + 1] as u32;
                let b = cursor[ci * 4] as u32;
                let inv = 255u32.wrapping_sub(a);
                let fi = ((fy * iwidth + fx) * 4) as usize;
                rgba[fi] = (r + ((rgba[fi] as u32) * inv) / 255) as u8;
                rgba[fi + 1] = (g + ((rgba[fi + 1] as u32) * inv) / 255) as u8;
                rgba[fi + 2] = (b + ((rgba[fi + 2] as u32) * inv) / 255) as u8;
            }
        }

        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(ii.hbmColor);
        let _ = DeleteObject(ii.hbmMask);
    }
}

#[cfg(not(target_os = "windows"))]
fn draw_cursor(_rgba: &mut [u8], _width: u32, _height: u32, _origin_x: i32, _origin_y: i32) {}

// ---------------------------------------------------------------------------
// 悬浮控制条窗口
// ---------------------------------------------------------------------------

fn show_control_window(app: &AppHandle, region: &RecRegion) {
    // 放到选区外侧：优先右侧，其次下侧；放不下就放屏幕左上角
    let (mut cx, mut cy) = (region.x + region.width as i32 + 8, region.y);
    let cw = 200i32;
    let ch = 64i32;
    let mon = find_monitor(region.x, region.y);
    if let Some(m) = mon {
        let mx = m.x().unwrap_or(0);
        let my = m.y().unwrap_or(0);
        let mw = m.width().unwrap_or(1920) as i32;
        let mh = m.height().unwrap_or(1080) as i32;
        if cx + cw > mx + mw {
            cx = region.x;
            cy = region.y + region.height as i32 + 8;
        }
        if cy + ch > my + mh {
            cx = mx;
            cy = my;
        }
    }

    let Some(win) = app.get_webview_window(RECORDER_CONTROL_LABEL) else {
        log::warn!("[截图/录屏] 控制条窗口未就绪");
        return;
    };
    // 物理坐标定位（HiDPI 安全）；尺寸在 init 时已固定，无需重设
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(cx, cy)));
    // 不抢焦点：录制的是用户正在操作的应用，控制条点按无需焦点
    crate::webview_mem::on_shown(app, RECORDER_CONTROL_LABEL);
    crate::win_taskbar::show(&win);
}
