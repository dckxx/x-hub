//! 速达「应用内打开网页」（ADR 0011）。
//!
//! 两个载体（双载体并行）：
//! - **主窗内嵌面板**：main 窗口的子 webview（label `suda-panel`），单实例、单页无 tab；
//!   工具栏（地址栏/前进后退/系统浏览器出口/关闭）是主窗 DOM，网页内容区是这层 webview，
//!   边界由前端按内容区矩形上报、运行期经 `set_bounds` 跟随。
//! - **独立浏览器窗口池**：1 个窗口（`suda-web-0`，2026-09-25 内存优化 4→2→1，
//!   全部页面收进 tab；ADR 0011 修订节同口径），每窗两个子 webview——
//!   content（外站页面，`suda-web-{i}-content`，**先建、垫底**）+ chrome（SPA 顶栏，
//!   `suda-web-{i}-chrome`，**后建、盖在上层**）。tab 条、地址栏、系统浏览器出口都在 chrome 页里；
//!   content 先建 chrome 后建是刻意的：chrome 高度变化（tab 条出现/收起）到 content 边界
//!   跟随之间有短暂交叠，让顶栏盖住内容比内容盖住顶栏观感好。
//!
//! ⚠️ 铁律（AGENTS.md 约定 41）：所有 webview 一律在启动期（setup）预创建、隐藏常驻，
//! 运行期只做 show/hide/navigate/set_bounds/eval，绝不 build/destroy。
//!
//! 权限口径：`suda-panel` 与 `suda-web-*-content` **故意不进** capabilities——它们只加载
//! 外站页面，不给 IPC；chrome 页是本地 SPA（label 命中 capabilities 的 `suda-web-*` 通配）。
//!
//! tab 模型（ADR 0011 拍板）：同地址复用（完整 URL 精确匹配）、轻量 tab 条（≥2 个才出现，
//! 由 chrome 页自行渲染）、**不做同域合并**；池=1 时选路总能落到唯一窗口
//! （可见开新 tab / 隐藏即空闲），`POOL_FULL` 实际不可达，保留作池扩容后的兜底，
//! 不做 LRU 自动杀。

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Url, Webview,
    WebviewBuilder, WebviewUrl,
};
use tauri::webview::NewWindowResponse;

use crate::commands::DbState;
use crate::models::ResourceKind;
use crate::repo::resource;

/// 主窗内嵌面板的 webview label（不进 capabilities：外站页面无 IPC）
pub const PANEL_LABEL: &str = "suda-panel";
/// 独立浏览器窗口池大小（ADR 0011：开满提示先关一个，不做 LRU 自动杀；
/// 2026-09-25 内存优化 4→2→1：轻量入口后每扇常驻仅 ~17-34MB，最终保留单窗、
/// 全部页面收进 tab——池选路对 1 扇窗天然成立（可见开新 tab / 隐藏即空闲））
pub const POOL_SIZE: usize = 1;
/// 窗口 label 前缀（chrome/content webview 共用此前缀，capabilities 用 `suda-web-*` 通配）
const WIN_PREFIX: &str = "suda-web";
/// chrome 顶栏默认高度（逻辑 px；chrome 页挂载后会实测上报）
const DEFAULT_CHROME_HEIGHT: f64 = 48.0;
const DEFAULT_WIN_W: f64 = 1024.0;
const DEFAULT_WIN_H: f64 = 720.0;

struct SlotState {
    tabs: Vec<String>,
    active: usize,
    chrome_height: f64,
    last_shown_ms: u64,
}

impl SlotState {
    fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            chrome_height: DEFAULT_CHROME_HEIGHT,
            last_shown_ms: 0,
        }
    }
}

/// 窗口池状态。锁纪律：持锁期间只做纯内存读写，窗口操作（show/navigate/bounds）
/// 一律在锁外进行——窗口事件回调（CloseRequested 清 tab）也会拿这把锁。
static SLOTS: Mutex<Vec<SlotState>> = Mutex::new(Vec::new());

fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn win_label(i: usize) -> String {
    format!("{WIN_PREFIX}-{i}")
}

pub(crate) fn chrome_label(i: usize) -> String {
    format!("{WIN_PREFIX}-{i}-chrome")
}

pub(crate) fn content_label(i: usize) -> String {
    format!("{WIN_PREFIX}-{i}-content")
}

fn ensure_slots() {
    let mut slots = SLOTS.lock().unwrap();
    while slots.len() < POOL_SIZE {
        slots.push(SlotState::new());
    }
}

/// 只放行 http/https（沿用资源打开链路的白名单口径，见 commands.rs::open_url_with_browser）
fn validate_web_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("只能打开 http/https 链接".to_string());
    }
    Ok(url.to_string())
}

fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank 是合法 URL")
}

// ---------------------------------------------------------------------------
// 启动期预创建（铁律：全工程唯一的 suda 浏览器 webview 创建点）
// ---------------------------------------------------------------------------

/// lib.rs setup 调用：预创建面板 webview + POOL_SIZE 个池窗口（全部隐藏常驻）
pub fn init(app: &AppHandle) {
    ensure_slots();
    init_panel(app);
    for i in 0..POOL_SIZE {
        init_pool_window(app, i);
    }
}

fn init_panel(app: &AppHandle) {
    if app.get_webview(PANEL_LABEL).is_some() {
        return;
    }
    let Some(main) = crate::main_window(app) else {
        log::warn!("速达网页面板预创建跳过：主窗口不存在");
        return;
    };
    let nav_app = app.clone();
    let nw_app = app.clone();
    let builder = WebviewBuilder::new(PANEL_LABEL, WebviewUrl::External(about_blank()))
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            // 顶层导航（含重定向）同步给主窗，驱动面板地址栏
            let _ = nav_app.emit_to("main", "suda-panel-nav", url.as_str());
            true
        })
        .on_new_window(move |url, _features| {
            // wry 不注册 handler 时会对 NewWindowRequested 静默拒绝（extension.rs 踩过）；
            // 这里注册为宿主接管：面板单页无 tab，新窗口请求交主窗处理（默认原地导航）
            let _ = nw_app.emit_to("main", "suda-panel-newwindow", url.as_str());
            NewWindowResponse::Deny
        });
    match main.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(200.0, 120.0),
    ) {
        Ok(wv) => {
            let _ = wv.hide();
            #[cfg(target_os = "linux")]
            {
                // 收缩对象必须精确到「新加入的面板」：setup 阶段主 webview 同样处于
                // 未可见状态，按可见性过滤会误伤主视图（实测整窗空白，2026-10-07）。
                let before = vbox_webview_ptrs(&main);
                shrink_hidden_panel_out_of_layout(&main, &before);
            }
            log::info!("速达网页面板 webview 预创建完成");
        }
        Err(e) => log::warn!("速达网页面板 webview 预创建失败: {e}"),
    }
}

/// 收集主窗 vbox 里所有 WebKitWebView 子组件的原始指针（用于新增前后对比）。
#[cfg(target_os = "linux")]
fn vbox_webview_ptrs(main: &tauri::Window<tauri::Wry>) -> Vec<usize> {
    use gtk::prelude::*;

    let mut out = Vec::new();
    let Ok(gwin) = main.gtk_window() else {
        return out;
    };
    for container in gwin.children() {
        let Some(box_) = container.dynamic_cast::<gtk::Box>().ok() else {
            continue;
        };
        for child in box_.children() {
            if child.type_().name() == "WebKitWebView" {
                out.push(child.as_ptr() as usize);
            }
        }
    }
    out
}

/// Linux：隐藏的速达面板 webview 仍参与主窗 vbox 布局——GtkBox 在可见子组件间平分空间，
/// 且主窗 show 流程的 `show_all` 会把 `hide()` 过的组件重新显示，导致主界面只占窗口上半
/// （面板占走另一半）。创建后立即把面板从布局中除名：取消 expand、尺寸请求压到 0，
/// 主 webview 恢复满高。代价：应用内打开网页的面板在 Linux 上暂不可见（已知差异，
/// 待 wry/tauri 修复子 webview 在 X11 的布局归属后恢复，参见 tauri#15656 一族）。
#[cfg(target_os = "linux")]
fn shrink_hidden_panel_out_of_layout(main: &tauri::Window<tauri::Wry>, before: &[usize]) {
    use gtk::prelude::*;

    let Ok(gwin) = main.gtk_window() else {
        return;
    };
    for container in gwin.children() {
        let Some(box_) = container.dynamic_cast::<gtk::Box>().ok() else {
            continue;
        };
        for child in box_.children() {
            // 只收缩 add_child 新增的那一个 webview：用调用前后指针差集识别，
            // 不能按可见性过滤——setup 阶段主 webview 同样未可见，会误伤
            if child.type_().name() == "WebKitWebView"
                && !before.contains(&(child.as_ptr() as usize))
            {
                box_.set_child_packing(&child, false, false, 0, gtk::PackType::Start);
                child.set_size_request(0, 0);
                log::info!("已将隐藏的速达面板 webview 移出主窗 vbox 布局");
            }
        }
    }
}

fn init_pool_window(app: &AppHandle, i: usize) {
    if app.get_window(&win_label(i)).is_some() {
        return;
    }
    // 独立浏览器窗保留系统边框：拖动/缩放/最小化/最大化交给 OS，
    // 应用内顶栏只负责 tab 条 + 地址栏 + 出口按钮
    let win = match tauri::WindowBuilder::new(app, win_label(i))
        .title("应用内浏览器")
        .inner_size(DEFAULT_WIN_W, DEFAULT_WIN_H)
        .min_inner_size(420.0, 300.0)
        .resizable(true)
        .visible(false)
        .build()
    {
        Ok(w) => w,
        Err(e) => {
            log::warn!("应用内浏览器窗口 {} 预创建失败: {e}", win_label(i));
            return;
        }
    };

    let nav_app = app.clone();
    let nw_app = app.clone();
    let title_app = app.clone();
    let content = win.add_child(
        WebviewBuilder::new(content_label(i), WebviewUrl::External(about_blank()))
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
            .disable_drag_drop_handler()
            .on_navigation(move |url| {
                if url.as_str() == "about:blank" {
                    return true;
                }
                // 页内跳转（用户点链接/重定向）也要回写 tabs[active]：tab 真源 = 当前页
                // URL，只靠命令路径更新的话「用系统浏览器打开」会导出进入时的旧地址
                if let Ok(mut slots) = SLOTS.lock() {
                    if let Some(s) = slots.get_mut(i) {
                        if !s.tabs.is_empty() {
                            s.tabs[s.active] = url.to_string();
                        }
                    }
                }
                emit_tabs(&nav_app, i);
                let _ = nav_app.emit_to(
                    chrome_label(i),
                    "suda-browser-nav",
                    json!({ "slot": i, "url": url.as_str() }),
                );
                true
            })
            .on_new_window(move |url, _features| {
                // 宿主接管进 tab 条（ADR 0011：新窗口请求不能指望页面自己弹窗）；
                // 不在回调里直接 navigate（重入风险），发给 chrome 页由它回 open_tab 命令
                let _ = nw_app.emit_to(
                    chrome_label(i),
                    "suda-browser-newwindow",
                    json!({ "slot": i, "url": url.as_str() }),
                );
                NewWindowResponse::Deny
            })
            .on_document_title_changed(move |_wv, title| {
                let _ = title_app.emit_to(
                    chrome_label(i),
                    "suda-browser-title",
                    json!({ "slot": i, "title": title }),
                );
            }),
        LogicalPosition::new(0.0, DEFAULT_CHROME_HEIGHT),
        LogicalSize::new(DEFAULT_WIN_W, DEFAULT_WIN_H - DEFAULT_CHROME_HEIGHT),
    );
    match &content {
        Ok(wv) => {
            let _ = wv.hide();
        }
        Err(e) => log::warn!("应用内浏览器 content webview 预创建失败: {e}"),
    }

    // chrome 创建即顶部条尺寸（不是全窗）：首次显示到 chrome 页上报高度之间，
    // 它若以全窗尺寸盖在 content 上方，不透明页面底色会白闪遮挡（实测踩过）；
    // 运行期边界统一由 apply_content_bounds 维护（chrome 页挂载后会上报真实高度修正）
    let chrome = win.add_child(
        // 轻量入口 chrome.html（P1）：只渲染顶栏，不加载完整 SPA（内存优化，见 src/light/chrome.ts）
        WebviewBuilder::new(chrome_label(i), WebviewUrl::App("chrome.html".into()))
            .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(DEFAULT_WIN_W, DEFAULT_CHROME_HEIGHT),
    );
    if let Err(e) = chrome {
        log::warn!("应用内浏览器 chrome webview 预创建失败: {e}");
    }

    let ev_app = app.clone();
    win.on_window_event(move |event| match event {
        // 关闭 = 隐藏 + 清空 tab + 复位空白页（铁律：不销毁窗口，槽位随之空闲）。
        // 必须走 hide_slot 与 chrome 页关闭同一条清理路径：只清 tab 不导航回
        // about:blank 的话，最后访问页面的 DOM/JS 活堆会一直驻留 renderer
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_slot(&ev_app, i);
        }
        tauri::WindowEvent::Resized(_) => {
            apply_content_bounds(&ev_app, i);
        }
        _ => {}
    });
}

// ---------------------------------------------------------------------------
// 窗口/内容操作辅助
// ---------------------------------------------------------------------------

fn slot_visible(app: &AppHandle, i: usize) -> bool {
    app.get_window(&win_label(i))
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

fn content_webview(app: &AppHandle, i: usize) -> Result<Webview, String> {
    app.get_webview(&content_label(i))
        .ok_or_else(|| "应用内浏览器未初始化".to_string())
}

fn navigate_content(app: &AppHandle, i: usize, url: &str) -> Result<(), String> {
    let wv = content_webview(app, i)?;
    let parsed = Url::parse(url).map_err(err_str)?;
    wv.navigate(parsed).map_err(err_str)
}

fn eval_content(app: &AppHandle, i: usize, js: &str) -> Result<(), String> {
    let wv = content_webview(app, i)?;
    wv.eval(js).map_err(err_str)
}

/// 池窗口内部布局：chrome webview = 顶部条（0,0 ~ 全宽×chrome 高度），
/// content webview = 顶部条以下其余区域（逻辑 px）。
/// ⚠️ 两个子 webview 都要维护：chrome 创建时给的是全窗尺寸，若不在此收缩，
/// 它不透明的页面底色会盖死下方真正加载网页的 content（实测踩过：独立窗口打开一片空白）。
fn apply_content_bounds(app: &AppHandle, i: usize) {
    let Some(win) = app.get_window(&win_label(i)) else {
        return;
    };
    let Ok(scale) = win.scale_factor() else {
        return;
    };
    let Ok(size) = win.inner_size() else {
        return;
    };
    let w = size.width as f64 / scale;
    let h = size.height as f64 / scale;
    let ch = SLOTS
        .lock()
        .ok()
        .and_then(|slots| slots.get(i).map(|s| s.chrome_height))
        .unwrap_or(DEFAULT_CHROME_HEIGHT);
    // chrome：顶部条（宽度必须随窗口实时取，否则窗口拉伸后顶栏不跟随）
    if let Some(chrome) = app.get_webview(&chrome_label(i)) {
        let _ = chrome.set_bounds(tauri::Rect {
            position: tauri::Position::Logical(LogicalPosition::new(0.0, 0.0)),
            size: tauri::Size::Logical(LogicalSize::new(w.max(1.0), ch)),
        });
    }
    // content：顶部条以下
    if let Ok(wv) = content_webview(app, i) {
        let rest = (h - ch).max(1.0);
        let _ = wv.set_bounds(tauri::Rect {
            position: tauri::Position::Logical(LogicalPosition::new(0.0, ch)),
            size: tauri::Size::Logical(LogicalSize::new(w.max(1.0), rest)),
        });
    }
}

fn emit_tabs(app: &AppHandle, i: usize) {
    let payload = {
        let Ok(slots) = SLOTS.lock() else {
            return;
        };
        let Some(s) = slots.get(i) else { return };
        json!({ "slot": i, "tabs": s.tabs, "active": s.active })
    };
    let _ = app.emit_to(chrome_label(i), "suda-browser-tabs", payload);
}

fn show_slot(app: &AppHandle, i: usize) {
    let Some(win) = app.get_window(&win_label(i)) else {
        return;
    };
    apply_content_bounds(app, i);
    // 先恢复内存级别再显示（webview_mem：chrome/content 双 webview 一起回 Normal）
    crate::webview_mem::on_slot(app, i, true);
    if let Ok(wv) = content_webview(app, i) {
        let _ = wv.show();
    }
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
    if let Ok(mut slots) = SLOTS.lock() {
        if let Some(s) = slots.get_mut(i) {
            s.last_shown_ms = now_ms();
        }
    }
}

fn hide_slot(app: &AppHandle, i: usize) {
    if let Some(win) = app.get_window(&win_label(i)) {
        let _ = win.hide();
    }
    if let Ok(wv) = content_webview(app, i) {
        let _ = wv.hide();
        // 复位到空白页：访问过的页面若留在 webview 里，其 DOM/JS 活堆会一直占着
        // renderer 内存（Low 只吐缓存吐不掉活堆）。tab 状态反正已清、下次打开必然
        // 重新 navigate，这里卸掉页面让 renderer 回到空白地板。navigate 属于
        // 约定 41 允许的运行期操作（show/hide/navigate/set_bounds/eval）
        let _ = wv.navigate(about_blank());
    }
    // 隐藏后把 chrome/content 双 webview 的内存目标级别降到 Low（webview_mem）
    crate::webview_mem::on_slot(app, i, false);
    if let Ok(mut slots) = SLOTS.lock() {
        if let Some(s) = slots.get_mut(i) {
            s.tabs.clear();
            s.active = 0;
        }
    }
    emit_tabs(app, i);
}

fn current_tab_url(i: usize) -> Option<String> {
    let slots = SLOTS.lock().ok()?;
    let s = slots.get(i)?;
    s.tabs.get(s.active).cloned()
}

// ---------------------------------------------------------------------------
// 独立窗口池命令
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct OpenResult {
    pub slot: usize,
    pub index: usize,
    pub reused: bool,
}

/// 打开速达网页资源到独立应用内浏览器（URL 取自数据库：仅 Web 类型 + http/https）
#[tauri::command]
pub fn suda_browser_open(
    app: AppHandle,
    state: State<'_, DbState>,
    id: i64,
) -> Result<OpenResult, String> {
    let url = {
        let conn = state.0.lock().map_err(err_str)?;
        let res = resource::get(&conn, id).map_err(err_str)?;
        if !matches!(res.kind, ResourceKind::Web) {
            return Err("仅网页资源支持应用内打开".to_string());
        }
        let url = validate_web_url(&res.target)?;
        let _ = resource::touch(&conn, id);
        url
    };
    open_in_pool(&app, &url)
}

/// 按 URL 打开（面板「转独立窗口」、chrome 页新窗口接管等入口）
#[tauri::command]
pub fn suda_browser_open_url(app: AppHandle, url: String) -> Result<OpenResult, String> {
    let url = validate_web_url(&url)?;
    open_in_pool(&app, &url)
}

/// 池选路：① 同地址复用 → ② 最近显示的可见窗口开新 tab → ③ 空闲窗口 → ④ POOL_FULL
fn open_in_pool(app: &AppHandle, url: &str) -> Result<OpenResult, String> {
    ensure_slots();

    // ① 同地址复用（完整 URL 精确匹配起步，ADR 0011 可逆默认）
    let reuse = {
        let slots = SLOTS.lock().map_err(err_str)?;
        slots.iter().enumerate().find_map(|(i, s)| {
            s.tabs
                .iter()
                .position(|t| t == url)
                .map(|idx| (i, idx))
        })
    };
    if let Some((i, idx)) = reuse {
        if let Ok(mut slots) = SLOTS.lock() {
            if let Some(s) = slots.get_mut(i) {
                s.active = idx;
            }
        }
        show_slot(app, i);
        emit_tabs(app, i);
        return Ok(OpenResult {
            slot: i,
            index: idx,
            reused: true,
        });
    }

    // ② 已有可见窗口 → 作为新 tab 落进最近显示的那扇
    let mut candidates: Vec<(u64, usize)> = {
        let slots = SLOTS.lock().map_err(err_str)?;
        slots
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.tabs.is_empty())
            .map(|(i, s)| (s.last_shown_ms, i))
            .collect()
    };
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let tab_target = candidates
        .into_iter()
        .map(|(_, i)| i)
        .find(|&i| slot_visible(app, i));
    if let Some(i) = tab_target {
        let index = {
            let mut slots = SLOTS.lock().map_err(err_str)?;
            let s = slots.get_mut(i).ok_or("无效的窗口槽位")?;
            s.tabs.push(url.to_string());
            s.active = s.tabs.len() - 1;
            s.active
        };
        show_slot(app, i);
        navigate_content(app, i, url)?;
        emit_tabs(app, i);
        return Ok(OpenResult {
            slot: i,
            index,
            reused: false,
        });
    }

    // ③ 空闲窗口（无 tab；兜底：隐藏但残留 tab 的陈旧槽位也回收复用）
    let (free, stale) = {
        let slots = SLOTS.lock().map_err(err_str)?;
        (
            slots.iter().position(|s| s.tabs.is_empty()),
            slots
                .iter()
                .enumerate()
                .find(|(i, s)| !s.tabs.is_empty() && !slot_visible(app, *i))
                .map(|(i, _)| i),
        )
    };
    let slot = match free.or(stale) {
        Some(i) => i,
        None => {
            return Err(
                format!("POOL_FULL:应用内浏览器窗口已开满（{POOL_SIZE} 个），请先关闭一个"),
            )
        }
    };
    {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        s.tabs = vec![url.to_string()];
        s.active = 0;
    }
    show_slot(app, slot);
    navigate_content(app, slot, url)?;
    emit_tabs(app, slot);
    Ok(OpenResult {
        slot,
        index: 0,
        reused: false,
    })
}

/// 新窗口请求接管：chrome 页把 target=_blank 的 URL 交回来，落成当前窗口的新 tab
#[tauri::command]
pub fn suda_browser_open_tab(app: AppHandle, slot: usize, url: String) -> Result<(), String> {
    let url = validate_web_url(&url)?;
    let index = {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        s.tabs.push(url.clone());
        s.active = s.tabs.len() - 1;
        s.active
    };
    show_slot(&app, slot);
    navigate_content(&app, slot, &url)?;
    emit_tabs(&app, slot);
    log::info!("应用内浏览器新 tab 接管：slot={slot} index={index} url={url}");
    Ok(())
}

#[tauri::command]
pub fn suda_browser_activate_tab(app: AppHandle, slot: usize, index: usize) -> Result<(), String> {
    let url = {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        if index >= s.tabs.len() {
            return Err("无效的 tab 序号".to_string());
        }
        s.active = index;
        s.tabs[index].clone()
    };
    show_slot(&app, slot);
    navigate_content(&app, slot, &url)?;
    emit_tabs(&app, slot);
    Ok(())
}

#[tauri::command]
pub fn suda_browser_close_tab(app: AppHandle, slot: usize, index: usize) -> Result<(), String> {
    let remaining = {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        if index >= s.tabs.len() {
            return Err("无效的 tab 序号".to_string());
        }
        s.tabs.remove(index);
        if s.active >= s.tabs.len() {
            s.active = s.tabs.len().saturating_sub(1);
        }
        s.tabs.len()
    };
    if remaining == 0 {
        hide_slot(&app, slot);
    } else {
        if let Some(url) = current_tab_url(slot) {
            navigate_content(&app, slot, &url)?;
        }
        emit_tabs(&app, slot);
    }
    Ok(())
}

/// 关闭整扇窗口（chrome 页 X / 主窗入口）：隐藏 + 清空 tab，窗口回池
#[tauri::command]
pub fn suda_browser_close(app: AppHandle, slot: usize) -> Result<(), String> {
    hide_slot(&app, slot);
    Ok(())
}

/// 地址栏导航（替换当前 tab 的 URL）
#[tauri::command]
pub fn suda_browser_navigate(app: AppHandle, slot: usize, url: String) -> Result<(), String> {
    let url = validate_web_url(&url)?;
    {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        if s.tabs.is_empty() {
            return Err("窗口没有打开的标签".to_string());
        }
        s.tabs[s.active] = url.clone();
    }
    navigate_content(&app, slot, &url)?;
    emit_tabs(&app, slot);
    Ok(())
}

#[tauri::command]
pub fn suda_browser_back(app: AppHandle, slot: usize) -> Result<(), String> {
    eval_content(&app, slot, "history.back()")
}

#[tauri::command]
pub fn suda_browser_forward(app: AppHandle, slot: usize) -> Result<(), String> {
    eval_content(&app, slot, "history.forward()")
}

#[tauri::command]
pub fn suda_browser_reload(app: AppHandle, slot: usize) -> Result<(), String> {
    eval_content(&app, slot, "location.reload()")
}

/// 顶栏「用系统浏览器打开」出口：当前 tab URL 交系统默认浏览器，窗口保持不动
#[tauri::command]
pub fn suda_browser_open_system(slot: usize) -> Result<(), String> {
    let url = current_tab_url(slot).ok_or("窗口没有打开的标签")?;
    crate::process::open_url(&url)
}

/// chrome 页挂载/重渲染后上报顶栏实测高度，content 边界随之收缩
#[tauri::command]
pub fn suda_browser_chrome_height(app: AppHandle, slot: usize, height: f64) -> Result<(), String> {
    {
        let mut slots = SLOTS.lock().map_err(err_str)?;
        let s = slots.get_mut(slot).ok_or("无效的窗口槽位")?;
        s.chrome_height = height.clamp(32.0, 160.0);
    }
    apply_content_bounds(&app, slot);
    Ok(())
}

#[derive(Serialize)]
pub struct SlotSnapshot {
    pub slot: usize,
    pub tabs: Vec<String>,
    pub active: usize,
}

/// chrome 页挂载时拉取初始状态（显示前的事件不补发，与 clipboard-shown 同款约定）
#[tauri::command]
pub fn suda_browser_state(slot: usize) -> Result<SlotSnapshot, String> {
    ensure_slots();
    let slots = SLOTS.lock().map_err(err_str)?;
    let s = slots.get(slot).ok_or("无效的窗口槽位")?;
    Ok(SlotSnapshot {
        slot,
        tabs: s.tabs.clone(),
        active: s.active,
    })
}

#[derive(Serialize)]
pub struct SlotSummary {
    pub slot: usize,
    pub tabs: Vec<String>,
    pub active: usize,
    pub visible: bool,
}

/// 窗口池概览（主窗侧展示「哪些窗口开着」；池满提示的后续管理用）
#[tauri::command]
pub fn suda_browser_slots(app: AppHandle) -> Result<Vec<SlotSummary>, String> {
    ensure_slots();
    let slots = SLOTS.lock().map_err(err_str)?;
    Ok(slots
        .iter()
        .enumerate()
        .map(|(i, s)| SlotSummary {
            slot: i,
            tabs: s.tabs.clone(),
            active: s.active,
            visible: slot_visible(&app, i),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// 主窗内嵌面板命令
// ---------------------------------------------------------------------------

fn panel_webview(app: &AppHandle) -> Result<Webview, String> {
    app.get_webview(PANEL_LABEL)
        .ok_or_else(|| "速达网页面板未初始化".to_string())
}

fn set_panel_bounds(wv: &Webview, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    wv.set_bounds(tauri::Rect {
        position: tauri::Position::Logical(LogicalPosition::new(x.max(0.0), y.max(0.0))),
        size: tauri::Size::Logical(LogicalSize::new(w.max(80.0), h.max(80.0))),
    })
    .map_err(err_str)
}

/// 打开速达网页资源到内嵌面板（URL 取自数据库：仅 Web 类型 + http/https）
#[tauri::command]
pub fn suda_panel_show(
    app: AppHandle,
    state: State<'_, DbState>,
    id: i64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let url = {
        let conn = state.0.lock().map_err(err_str)?;
        let res = resource::get(&conn, id).map_err(err_str)?;
        if !matches!(res.kind, ResourceKind::Web) {
            return Err("仅网页资源支持应用内打开".to_string());
        }
        let url = validate_web_url(&res.target)?;
        let _ = resource::touch(&conn, id);
        url
    };
    let wv = panel_webview(&app)?;
    wv.navigate(Url::parse(&url).map_err(err_str)?)
        .map_err(err_str)?;
    set_panel_bounds(&wv, x, y, w, h)?;
    wv.show().map_err(err_str)
}

/// 面板内容区边界跟随（主窗缩放/布局变化时由前端 ResizeObserver 上报，逻辑 px）
#[tauri::command]
pub fn suda_panel_bounds(app: AppHandle, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    let wv = panel_webview(&app)?;
    set_panel_bounds(&wv, x, y, w, h)
}

#[tauri::command]
pub fn suda_panel_hide(app: AppHandle) -> Result<(), String> {
    let wv = panel_webview(&app)?;
    wv.hide().map_err(err_str)?;
    // 复位到空白页卸掉已访问页面（同 hide_slot：下次打开必然重新 navigate）
    wv.navigate(about_blank()).map_err(err_str)
}

/// 面板地址栏导航（无资源 id，不写最近使用）
#[tauri::command]
pub fn suda_panel_navigate(app: AppHandle, url: String) -> Result<(), String> {
    let url = validate_web_url(&url)?;
    let wv = panel_webview(&app)?;
    wv.navigate(Url::parse(&url).map_err(err_str)?)
        .map_err(err_str)
}

#[tauri::command]
pub fn suda_panel_back(app: AppHandle) -> Result<(), String> {
    let wv = panel_webview(&app)?;
    wv.eval("history.back()").map_err(err_str)
}

#[tauri::command]
pub fn suda_panel_forward(app: AppHandle) -> Result<(), String> {
    let wv = panel_webview(&app)?;
    wv.eval("history.forward()").map_err(err_str)
}

#[tauri::command]
pub fn suda_panel_reload(app: AppHandle) -> Result<(), String> {
    let wv = panel_webview(&app)?;
    wv.eval("location.reload()").map_err(err_str)
}

#[cfg(test)]
mod tests {
    use super::validate_web_url;

    #[test]
    fn web_url_whitelist() {
        assert!(validate_web_url("https://example.com").is_ok());
        assert!(validate_web_url(" http://example.com ").is_ok());
        assert!(validate_web_url("file:///C:/Windows").is_err());
        assert!(validate_web_url("javascript:alert(1)").is_err());
        assert!(validate_web_url("example.com").is_err());
    }
}
