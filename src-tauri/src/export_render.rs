//! 静默 PDF 导出（2026-10-22，速记笔记导出）：
//! 用 WebView2 自带的 `ICoreWebView2_16::PrintToPdf`（Edge 打印管线）把渲染好的
//! HTML 直出矢量 PDF——文字可选中、可搜索，Obsidian（Electron printToPDF）同款效果。
//!
//! 约定 41 铁律：承载渲染的 webview **启动期预创建、隐藏常驻**（label `export-render`，
//! 挂轻量 export.html，不加载主 SPA），运行期只做 navigate / show / hide / COM 调用，
//! 绝不现场 build/destroy。
//!
//! 页面加载检测不用脚本（隐藏态 WebView2 可能不跑脚本，见 notify.rs 教训）：
//! 纯静态 HTML 零脚本，靠 builder 的 `on_page_load(Finished)` 原生回调 + 通道等待。
//!
//! WebView2 Runtime 过旧（< 1.0.1108，无 ICoreWebView2_16）时回落系统打印对话框
//! （`print()`，用户在对话框里选「另存为 PDF」），保证功能可用性。

use std::path::Path;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub const WINDOW_LABEL: &str = "export-render";

/// 单次页面加载的等待通道：render 前置入 sender，on_page_load(Finished) 取走并放行
#[derive(Default)]
pub struct PageLoadWait(Mutex<Option<mpsc::Sender<()>>>);

const LOAD_TIMEOUT: Duration = Duration::from_secs(10);
const PRINT_TIMEOUT: Duration = Duration::from_secs(30);

/// 启动期预创建隐藏常驻窗口（约定 41；见模块注释）
pub fn init(app: &tauri::AppHandle) {
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return;
    }
    app.manage(PageLoadWait::default());
    let builder = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App("export.html".into()),
    )
    .title("x-hub 导出")
    .inner_size(960.0, 1280.0)
    .resizable(false)
    .decorations(false)
    .skip_taskbar(true)
    .visible(false)
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS);

    let built = builder
        .on_page_load(|win, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                if let Some(state) = win.app_handle().try_state::<PageLoadWait>() {
                    if let Some(tx) = state.0.lock().ok().and_then(|mut g| g.take()) {
                        let _ = tx.send(());
                    }
                }
            }
        })
        .build();
    if let Err(e) = built {
        log::warn!("导出渲染窗预创建失败（本次会话 PDF 导出不可用）: {e}");
        return;
    }
    log::info!("导出渲染窗已预创建（隐藏常驻）");
}

/// 把渲染好的 HTML 直出为 PDF（静默、矢量文字）。
/// 从命令线程调用：navigate / with_webview 内部自行派发到主线程。
pub fn render_pdf(app: &tauri::AppHandle, html: &str, out: &Path) -> Result<(), String> {
    let win = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "导出渲染窗不可用（应用启动中，稍后再试）".to_string())?;
    // 回落路径可能把窗口留成可见：每次正式导出先归位隐藏
    let _ = win.hide();
    // 渲染/打印前恢复 Normal 内存档（隐藏常驻被 webview_mem 压在 Low，见 webview_mem.rs）
    crate::webview_mem::on_shown(app, WINDOW_LABEL);

    let temp = temp_html_path();
    std::fs::write(&temp, html).map_err(|e| format!("写入临时渲染文件失败: {e}"))?;
    let _guard = TempFileGuard(temp.clone());

    // 等页面加载完成（原生 Finished 事件，不依赖脚本）
    let (load_tx, load_rx) = mpsc::channel::<()>();
    {
        let state: State<PageLoadWait> = app.state();
        *state
            .0
            .lock()
            .map_err(|_| "导出渲染窗状态不可用".to_string())? = Some(load_tx);
    }
    let url = tauri::Url::from_file_path(&temp).map_err(|_| "临时渲染路径不合法".to_string())?;
    win.navigate(url).map_err(|e| format!("加载渲染页面失败: {e}"))?;
    if load_rx.recv_timeout(LOAD_TIMEOUT).is_err() {
        return Err("渲染页面加载超时".into());
    }

    // 主线程上只「发起」PrintToPdf；完成回调要靠主线程泵消息才能派发，
    // 所以等待必须回到命令线程（0.9.2 实测：在 with_webview 里阻塞等完成 =
    // 阻死泵 → 回调永不送达 → 必超时，而浏览器进程照常把文件写出来了）
    let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
    let out_path = out.to_path_buf();
    let begin_tx = done_tx.clone();
    win.with_webview(move |pv| {
        let _ = begin_tx.send(print_begin(pv, &out_path, done_tx));
    })
    .map_err(|e| format!("调度导出任务失败: {e}"))?;
    // 第一条消息 = 发起结果（COM 同步调用，秒级内回；Runtime 过旧在这报 UNSUPPORTED）
    match done_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            crate::webview_mem::on_hidden(app, WINDOW_LABEL);
            return Err(e);
        }
        Err(_) => {
            crate::webview_mem::on_hidden(app, WINDOW_LABEL);
            return Err("发起 PDF 生成失败（主线程无响应）".into());
        }
    }
    // 第二条消息 = 完成回调；命令线程阻塞不影响主线程泵消息
    let result = match done_rx.recv_timeout(PRINT_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("PDF 生成超时".to_string()),
    };
    crate::webview_mem::on_hidden(app, WINDOW_LABEL);
    result
}

/// WebView2 Runtime 过旧时的回落：临时文件渲染 + 显示窗口 + 系统打印对话框
/// （用户在对话框里选「另存为 PDF」）。窗口留在可见态供预览，下次静默导出会归位。
pub fn render_pdf_via_dialog(app: &tauri::AppHandle, html: &str) -> Result<(), String> {
    let win = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "导出渲染窗不可用".to_string())?;
    crate::webview_mem::on_shown(app, WINDOW_LABEL);
    let temp = temp_html_path();
    std::fs::write(&temp, html).map_err(|e| format!("写入临时渲染文件失败: {e}"))?;
    let _guard = TempFileGuard(temp.clone());

    let (load_tx, load_rx) = mpsc::channel::<()>();
    {
        let state: State<PageLoadWait> = app.state();
        *state.0.lock().map_err(|_| "状态不可用".to_string())? = Some(load_tx);
    }
    let url = tauri::Url::from_file_path(&temp).map_err(|_| "临时渲染路径不合法".to_string())?;
    win.navigate(url).map_err(|e| format!("加载渲染页面失败: {e}"))?;
    if load_rx.recv_timeout(LOAD_TIMEOUT).is_err() {
        return Err("渲染页面加载超时".into());
    }
    let _ = win.show();
    let _ = win.set_focus();
    win.print().map_err(|e| format!("打开打印对话框失败: {e}"))
}

fn temp_html_path() -> std::path::PathBuf {
    // 同进程内多次导出唯一化（毫秒时间戳足够）
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("xhub-export-{}-{ts}.html", std::process::id()))
}

struct TempFileGuard(std::path::PathBuf);
impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(target_os = "windows")]
mod win_pdf {
    use super::*;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_16, ICoreWebView2Environment6, ICoreWebView2PrintSettings,
        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
    };
    use windows_core::{Interface, PCWSTR};

    /// 必须在主线程调用（with_webview 回调内）：COM 调 PrintToPdf，A4 竖版 + 10mm 边距。
    /// **只发起、绝不阻塞**：完成回调经主线程泵消息派发，这里一旦等回调就是死锁
    /// （0.9.2 实测：文件能写出来、回调永远收不到 → 必报超时）。
    /// 发起结果直接返回；完成结果经 `done` 通道送回命令线程。
    /// Runtime 过旧（cast 失败）时返回 super::UNSUPPORTED 供上层回落打印对话框。
    /// 完成回调直接用 webview2-com 自带的 PrintToPdfCompletedHandler（其内部经
    /// completed_callback 宏实现 COM 对象，避免自己 #[implement] 引入 windows-core
    /// 多版本 trait 不匹配问题）。
    pub fn print_begin(
        pv: tauri::webview::PlatformWebview,
        out: &Path,
        done: mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        unsafe {
            let controller = pv.controller();
            let wv = controller.CoreWebView2().map_err(|e| e.to_string())?;
            let wv16: ICoreWebView2_16 =
                wv.cast().map_err(|_| super::UNSUPPORTED.to_string())?;
            let env6: ICoreWebView2Environment6 = pv
                .environment()
                .cast()
                .map_err(|_| super::UNSUPPORTED.to_string())?;
            let settings: ICoreWebView2PrintSettings = env6
                .CreatePrintSettings()
                .map_err(|e| format!("创建打印设置失败: {e}"))?;
            settings
                .SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)
                .map_err(|e| e.to_string())?;
            // A4 = 8.27 × 11.69 英寸；边距 10mm ≈ 0.394 英寸
            settings.SetPageWidth(8.27).map_err(|e| e.to_string())?;
            settings.SetPageHeight(11.69).map_err(|e| e.to_string())?;
            settings.SetMarginTop(0.394).map_err(|e| e.to_string())?;
            settings.SetMarginBottom(0.394).map_err(|e| e.to_string())?;
            settings.SetMarginLeft(0.394).map_err(|e| e.to_string())?;
            settings.SetMarginRight(0.394).map_err(|e| e.to_string())?;
            settings
                .SetShouldPrintBackgrounds(true)
                .map_err(|e| e.to_string())?;
            settings
                .SetShouldPrintHeaderAndFooter(false)
                .map_err(|e| e.to_string())?;
            let handler = webview2_com::PrintToPdfCompletedHandler::create(Box::new(
                move |hr, ok| {
                    let _ = done.send(if ok && hr.is_ok() {
                        Ok(())
                    } else {
                        Err("PDF 生成失败（目标文件可能被占用）".into())
                    });
                    Ok(())
                },
            ));
            let mut wide: Vec<u16> = out.as_os_str().to_string_lossy().encode_utf16().collect();
            wide.push(0);
            wv16
                .PrintToPdf(PCWSTR(wide.as_ptr()), &settings, &handler)
                .map_err(|e| format!("发起 PDF 生成失败: {e}"))?;
        }
        Ok(())
    }
}

/// Runtime 缺 ICoreWebView2_16 时的哨兵错误：render_pdf_with_fallback 据此回落打印对话框
pub const UNSUPPORTED: &str = "SYSTEM_PRINT_FALLBACK";

#[cfg(target_os = "windows")]
use win_pdf::print_begin;

#[cfg(not(target_os = "windows"))]
#[allow(unused)]
fn print_begin(
    _pv: tauri::webview::PlatformWebview,
    _out: &Path,
    _done: mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    Err(UNSUPPORTED.to_string())
}

/// 非 Windows / Runtime 过旧时的统一入口：尝试静默，不支持时走打印对话框
pub fn render_pdf_with_fallback(
    app: &tauri::AppHandle,
    html: &str,
    out: &Path,
) -> Result<(), String> {
    match render_pdf(app, html, out) {
        Ok(()) => Ok(()),
        Err(e) if e == UNSUPPORTED => {
            log::warn!("WebView2 不支持静默 PDF，回落系统打印对话框");
            render_pdf_via_dialog(app, html)
        }
        Err(e) => Err(e),
    }
}

/// 进度事件（文件夹批量导出时逐条 emit 到主窗）
pub fn emit_progress(app: &tauri::AppHandle, done: usize, total: usize, title: &str) {
    let _ = app.emit(
        "note-export-progress",
        serde_json::json!({ "done": done, "total": total, "title": title }),
    );
}
