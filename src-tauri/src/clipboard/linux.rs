//! Linux / 非 Windows 剪贴板历史实现。
//!
//! 能力对齐 Windows 版对外 API：读写剪贴板、轮询监听入库、浮层唤起/收起、粘贴回前台。
//! 粘贴通过 `xdotool`/`ydotool`（若可用）注入 Ctrl+V；不可用时仅写入剪贴板。

use crate::commands::DbState;
use arboard::Clipboard;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};

pub const CLIPBOARD_WINDOW_LABEL: &str = "clipboard";
pub const CLIPBOARD_WIDTH: f64 = 520.0;
pub const CLIPBOARD_HEIGHT: f64 = 440.0;

const ECHO_SUPPRESS_SECONDS: u64 = 10;
const SETTLE_MS: u64 = 80;
const POLL_MS: u64 = 500;

pub struct ClipboardState {
    /// Linux 上暂存「唤起前」标记；粘贴不依赖 HWND
    pub prev_focus: Mutex<Option<isize>>,
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self {
            prev_focus: Mutex::new(None),
        }
    }
}

static LAST_SELF_SET: Mutex<Option<(u64, Instant)>> = Mutex::new(None);
static LAST_SEEN_HASH: Mutex<Option<u64>> = Mutex::new(None);

pub enum ClipboardPayload {
    Text { content: String, html: Option<String> },
    Image { bytes: Vec<u8>, format: ImageFormat },
    Files { paths: Vec<String> },
}

#[derive(Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Png,
    Dib,
}

fn content_hash(text: &str, html: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    html.unwrap_or("").hash(&mut h);
    h.finish()
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

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

fn is_self_set(text: &str, html: Option<&str>) -> bool {
    is_self_set_hash(content_hash(text, html))
}

fn is_self_set_hash(hash: u64) -> bool {
    let Ok(guard) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((h, at)) = guard.as_ref() else {
        return false;
    };
    at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS && *h == hash
}

fn record_self_set_hash(hash: u64) {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = Some((hash, Instant::now()));
    }
}

pub fn clear_self_set_fingerprint() {
    if let Ok(mut guard) = LAST_SELF_SET.lock() {
        *guard = None;
    }
}

pub fn init_win_op_worker() {
    // Linux 无 Win32 延迟操作队列
}

fn open_clipboard() -> Result<Clipboard, String> {
    Clipboard::new().map_err(|e| format!("打开系统剪贴板失败: {e}"))
}

pub fn read_clipboard() -> Option<(String, Option<String>)> {
    let mut cb = open_clipboard().ok()?;
    let text = cb.get_text().ok()?;
    if text.trim().is_empty() {
        return None;
    }
    Some((text, None))
}

pub fn read_clipboard_with_retry() -> Option<(String, Option<String>)> {
    for delay in [0u64, 40, 80, 140] {
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }
        if let Some(v) = read_clipboard() {
            return Some(v);
        }
    }
    None
}

/// RGBA → PNG（最小实现，避免额外 image 依赖：用简单未压缩 PNG 太大，改存 raw 不合适）
/// 使用 `image` 会增依赖；这里用 arboard 读到的 RGBA 经 `png` crate… 项目未引入。
/// 折中：把 RGBA 写成未压缩 BMP（自绘文件头），格式标为 Dib。
fn rgba_to_bmp(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return None;
    }
    // BMP 每行 4 字节对齐，像素为 BGRA、自底向上
    let row_stride = (width * 3 + 3) & !3;
    let pixel_size = row_stride * height;
    let file_size = 14 + 40 + pixel_size;
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel offset
    out.extend_from_slice(&40u32.to_le_bytes()); // DIB header size
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // compression
    out.extend_from_slice(&(pixel_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..height).rev() {
        let mut row = Vec::with_capacity(row_stride);
        for x in 0..width {
            let i = (y * width + x) * 4;
            row.push(rgba[i + 2]); // B
            row.push(rgba[i + 1]); // G
            row.push(rgba[i]); // R
        }
        while row.len() < row_stride {
            row.push(0);
        }
        out.extend_from_slice(&row);
    }
    Some(out)
}

pub fn read_clipboard_payload() -> Option<ClipboardPayload> {
    let mut cb = open_clipboard().ok()?;

    if let Ok(img) = cb.get_image() {
        let w = img.width;
        let h = img.height;
        if let Some(bmp) = rgba_to_bmp(w, h, &img.bytes) {
            // 去掉 14 字节文件头，存 DIB（与 Windows 路径一致）
            let dib = if bmp.len() > 14 { bmp[14..].to_vec() } else { bmp };
            return Some(ClipboardPayload::Image {
                bytes: dib,
                format: ImageFormat::Dib,
            });
        }
    }

    let text = cb.get_text().ok()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(ClipboardPayload::Text {
        content: text,
        html: None,
    })
}

pub fn set_clipboard(text: &str, html: Option<&str>) -> Result<(), String> {
    let mut cb = open_clipboard()?;
    cb.set_text(text.to_string())
        .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    let _ = html;
    record_self_set_hash(content_hash(text, html));
    Ok(())
}

fn clipboard_images_dir() -> Option<std::path::PathBuf> {
    let dir = crate::paths::data_root().join("clipboard").join("images");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

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
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(off_bits as u32).to_le_bytes());
    out.extend_from_slice(dib);
    out
}

fn save_image_snapshot(bytes: &[u8], format: ImageFormat, hash: u64) -> Option<String> {
    let dir = clipboard_images_dir()?;
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

pub fn set_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片失败: {e}"))?;
    let hash = hash_bytes(&bytes);
    // Linux：尽量写文本占位路径；完整图片写回依赖 RGBA 解码，此处写路径文本兜底
    // 若是 BMP，尝试粗略解析宽高后写 image（过复杂则仅文本）
    let mut cb = open_clipboard()?;
    if let Some((w, h, rgba)) = bmp_to_rgba(&bytes) {
        let img = arboard::ImageData {
            width: w,
            height: h,
            bytes: rgba.into(),
        };
        cb.set_image(img)
            .map_err(|e| format!("写入剪贴板图片失败: {e}"))?;
    } else {
        cb.set_text(path.to_string())
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    }
    record_self_set_hash(hash);
    Ok(())
}

/// 极简 BMP(24bpp) → RGBA
fn bmp_to_rgba(bmp: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bmp.len() < 54 || &bmp[0..2] != b"BM" {
        return None;
    }
    let dib = &bmp[14..];
    if dib.len() < 40 {
        return None;
    }
    let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]) as usize;
    let height_i = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    let height = height_i.unsigned_abs() as usize;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]);
    if bpp != 24 || width == 0 || height == 0 {
        return None;
    }
    let row_stride = (width * 3 + 3) & !3;
    let pixel_offset = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]) as usize;
    if bmp.len() < pixel_offset + row_stride * height {
        return None;
    }
    let mut rgba = vec![0u8; width * height * 4];
    for y in 0..height {
        let src_y = if height_i > 0 { height - 1 - y } else { y };
        let row = &bmp[pixel_offset + src_y * row_stride..];
        for x in 0..width {
            let s = x * 3;
            let d = (y * width + x) * 4;
            rgba[d] = row[s + 2];
            rgba[d + 1] = row[s + 1];
            rgba[d + 2] = row[s];
            rgba[d + 3] = 255;
        }
    }
    Some((width, height, rgba))
}

pub fn set_clipboard_files(paths: &[String]) -> Result<(), String> {
    // Linux 无统一「文件列表」剪贴板格式（URI list 因桌面环境而异）；退化为换行文本
    let text = paths.join("\n");
    set_clipboard(&text, None)?;
    record_self_set_hash(hash_files(paths));
    Ok(())
}

fn try_inject_paste() {
    // 优先 xdotool（X11），其次 ydotool（需权限）
    let attempts = [
        ("xdotool", vec!["key", "--clearmodifiers", "ctrl+v"]),
        ("ydotool", vec!["key", "29:1", "47:1", "47:0", "29:0"]),
    ];
    for (bin, args) in attempts {
        if std::process::Command::new(bin)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return;
        }
    }
    log::info!("未找到 xdotool/ydotool，已写入剪贴板，请手动 Ctrl+V 粘贴");
}

pub fn paste_to_previous_window(app: &AppHandle, content: &str, html: Option<&str>) {
    let _ = app;
    if !content.is_empty() {
        if let Err(e) = set_clipboard(content, html) {
            log::warn!("粘贴前写入剪贴板失败: {e}");
            return;
        }
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(80));
        try_inject_paste();
    });
}

fn build_overlay_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    tauri::WebviewWindowBuilder::new(
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
    .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
    .build()
}

pub fn init_overlay_window(app: &AppHandle) {
    if app.get_webview_window(CLIPBOARD_WINDOW_LABEL).is_some() {
        return;
    }
    match build_overlay_window(app) {
        Ok(_) => log::info!("剪贴板浮层窗口已预创建（隐藏常驻）"),
        Err(e) => log::warn!("剪贴板浮层窗口预创建失败: {e}"),
    }
}

fn overlay_visible(win: &tauri::WebviewWindow) -> bool {
    win.is_visible().unwrap_or(false)
}

fn place_near_center(win: &tauri::WebviewWindow) {
    if let Ok(Some(m)) = win.current_monitor() {
        let size = m.size();
        let pos = m.position();
        let scale = m.scale_factor();
        let w = (CLIPBOARD_WIDTH * scale) as i32;
        let h = (CLIPBOARD_HEIGHT * scale) as i32;
        let x = pos.x + (size.width as i32 - w) / 2;
        let y = pos.y + (size.height as i32 - h) / 3;
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )));
    }
}

fn show_ready_overlay(win: &tauri::WebviewWindow, app: &AppHandle) {
    crate::webview_mem::on_shown(app, CLIPBOARD_WINDOW_LABEL);
    place_near_center(win);
    crate::win_taskbar::show(win);
    let _ = app.emit_to(CLIPBOARD_WINDOW_LABEL, "clipboard-shown", ());
}

pub fn toggle_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        if overlay_visible(&win) {
            hide_overlay(app);
            return;
        }
    }
    if let Ok(mut guard) = app.state::<ClipboardState>().prev_focus.lock() {
        *guard = Some(1);
    }
    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        if let Ok(win) = build_overlay_window(app) {
            show_ready_overlay(&win, app);
        }
        return;
    };
    show_ready_overlay(&win, app);
}

pub fn activate_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        let _ = win.set_focus();
    }
}

pub fn hide_overlay(app: &AppHandle) {
    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        return;
    };
    crate::webview_mem::on_hidden(win.app_handle(), win.label());
    let _ = win.hide();
}

fn insert_into_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.try_state::<DbState>().ok_or("剪贴板状态未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

fn handle_clipboard_update(app: &AppHandle) {
    let cfg = crate::config::load();
    if cfg.clipboard_paused {
        return;
    }

    let payload = read_clipboard_payload().or_else(|| {
        std::thread::sleep(Duration::from_millis(120));
        read_clipboard_payload()
    });
    let Some(payload) = payload else {
        return;
    };

    let source: Option<String> = None;
    let result: Result<(), String> = match payload {
        ClipboardPayload::Text { content, html } => {
            if content.trim().is_empty() {
                return;
            }
            let hash = content_hash(&content, html.as_deref());
            if let Ok(mut seen) = LAST_SEEN_HASH.lock() {
                if seen.as_ref() == Some(&hash) {
                    return;
                }
                *seen = Some(hash);
            }
            if is_self_set(&content, html.as_deref()) {
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert(conn, &content, html.as_deref(), source.as_deref())
                    .map_err(|e| e.to_string())
            })
            .map(|_| ())
        }
        ClipboardPayload::Image { bytes, format } => {
            if !cfg.clipboard_image_enabled {
                return;
            }
            let hash = hash_bytes(&bytes);
            if let Ok(mut seen) = LAST_SEEN_HASH.lock() {
                if seen.as_ref() == Some(&hash) {
                    return;
                }
                *seen = Some(hash);
            }
            if is_self_set_hash(hash) {
                return;
            }
            let Some(path) = save_image_snapshot(&bytes, format, hash) else {
                return;
            };
            let dedup_key = format!("{:016x}", hash);
            match insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_image(conn, &dedup_key, &path, source.as_deref())
                    .map_err(|e| e.to_string())
            }) {
                Ok(true) => Ok(()),
                Ok(false) => {
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
                return;
            }
            insert_into_db(app, |conn| {
                crate::repo::clipboard::insert_files(conn, &paths, source.as_deref())
                    .map_err(|e| e.to_string())
            })
            .map(|_| ())
        }
    };

    if let Err(e) = result {
        log::warn!("剪贴板历史入库失败: {e}");
    }
}

pub fn start_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        log::info!("剪贴板监听线程启动（Linux 轮询 {POLL_MS}ms）");
        loop {
            std::thread::sleep(Duration::from_millis(POLL_MS));
            std::thread::sleep(Duration::from_millis(SETTLE_MS));
            handle_clipboard_update(&app);
        }
    });
}
