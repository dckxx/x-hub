//! macOS 剪贴板历史（NSPasteboard 经 osascript/JXA 桥接，无新增原生依赖）。
//!
//! Windows 版（clipboard.rs）基于 `WM_CLIPBOARDUPDATE` 事件 + 低层鼠标钩子 + `SendInput`
//! 按键注入。macOS 无公开等价事件，且本仓不引入 objc2 全家桶——改用**系统自带工具桥接**：
//!   - 变化检测：JXA（`osascript -l JavaScript`）读 `NSPasteboard.changeCount`，实测单次
//!     ~43ms，1.2s 一跳的稳态开销可忽略；计数未变则整跳跳过（文本/图片/文件统一门控）
//!   - 图片：`public.png` / `public.tiff` 由 NSData 直写临时文件 → 落盘 PNG 快照
//!     （与 Windows 同口径：`数据根/clipboard/images/`，入库 kind=image；TIFF 转码 PNG
//!     失败则原样存 .tiff 兜底，写回剪贴板时再转）
//!   - 文件：`public.file-url`（readObjectsForClasses 读 NSURL 列表，Finder 拷贝），kind=file
//!   - 富文本：`public.html` 片段随文本一并入库；写回时同时写 text + html 两种类型
//!   - 文本读写：`pbpaste` / `pbcopy`（快路径）；来源应用：`NSWorkspace.frontmostApplication`
//!   - 回写：图片/文件经 JXA 写回 NSPasteboard；粘贴不注入按键
//!     （收起浮层后由用户按 Cmd+V，与既有 mac 设计一致）
//!   - 浮层窗口：Tauri WebviewWindow（transparent + 置顶 + 不进任务栏）
//!
//! 优先级与 Windows 一致：图片 > 文件 > 文本；入库去重 / 回声抑制 / 暂停开关同口径。

use crate::commands::DbState;
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};

/// 剪贴板浮窗窗口 label（App.vue 按此路由渲染 ClipboardOverlay）
pub const CLIPBOARD_WINDOW_LABEL: &str = "clipboard";
pub const CLIPBOARD_WIDTH: f64 = 520.0;
pub const CLIPBOARD_HEIGHT: f64 = 440.0;

/// 轮询间隔：与文本版一致；probe 单跳 ~43ms，稳态 CPU 占比 ~3.5%
const POLL_INTERVAL_MS: u64 = 1200;
/// 回声指纹有效窗口（秒）：写回剪贴板后监听循环可能晚数跳才读到
const ECHO_SUPPRESS_SECONDS: u64 = 10;

/// 跨命令共享的剪贴板浮层状态（兼容其它模块的 `ClipboardState::default()`）
pub struct ClipboardState {
    pub prev_focus: Mutex<Option<isize>>,
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self {
            prev_focus: Mutex::new(None),
        }
    }
}

/// 图片原始格式（NSPasteboard 主流两种）
#[derive(Clone, Copy, PartialEq)]
pub enum MacImageFormat {
    Png,
    Tiff,
}

// ---------- 回声指纹（自复制抑制） ----------

/// 自身写入指纹（自复制回声抑制）：文本存 content+html 哈希，图片/文件存字节/路径哈希
static LAST_SELF_SET: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

fn echo_hash(content: &str, html: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    html.unwrap_or("").hash(&mut h);
    h.finish()
}

fn mark_self_set(content: &str, html: Option<&str>) {
    if let Ok(mut g) = LAST_SELF_SET.lock() {
        *g = Some((echo_hash(content, html), Instant::now()));
    }
}

/// 清空自复制指纹（粘贴历史项后外部调用）
pub fn clear_self_set_fingerprint() {
    if let Ok(mut g) = LAST_SELF_SET.lock() {
        *g = None;
    }
}

fn is_self_set(content: &str, html: Option<&str>) -> bool {
    let Ok(g) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((h, at)) = *g else {
        return false;
    };
    h == echo_hash(content, html) && at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS
}

/// 图片字节指纹：用于回声抑制与「相同图片只留一条」去重（与 Windows 同口径）
fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// 文件列表指纹：路径 + 各文件大小（同名不同内容也能区分），用于回声抑制
fn hash_files(paths: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in paths {
        p.hash(&mut h);
        if let Ok(meta) = std::fs::metadata(p) {
            meta.len().hash(&mut h);
        }
    }
    h.finish()
}

/// 记录回声指纹（图片/文件写入后调用）
fn record_self_set_hash(hash: u64) {
    if let Ok(mut g) = LAST_SELF_SET.lock() {
        *g = Some((hash, Instant::now()));
    }
}

/// 判断给定哈希是否为「我们自己写入」的回声（图片/文件）
fn is_self_set_hash(hash: u64) -> bool {
    let Ok(g) = LAST_SELF_SET.lock() else {
        return false;
    };
    let Some((h, at)) = *g else {
        return false;
    };
    h == hash && at.elapsed().as_secs() < ECHO_SUPPRESS_SECONDS
}

// ---------- 文本快路径（pbpaste / pbcopy） ----------

/// 读剪贴板文本（`pbpaste`）
fn pbpaste() -> Option<String> {
    let out = std::process::Command::new("pbpaste").output().ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

/// 写剪贴板文本（`pbcopy`）
fn pbcopy(text: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 pbcopy 失败: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    }
    let status = child.wait().map_err(|e| format!("pbcopy 失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("写入剪贴板失败".into())
    }
}

/// 读剪贴板（文本；html 由监听循环按 probe 标志另行探取）
pub fn read_clipboard() -> Option<(String, Option<String>)> {
    pbpaste().map(|t| (t, None))
}

pub fn read_clipboard_with_retry() -> Option<(String, Option<String>)> {
    for _ in 0..2 {
        if let Some(r) = read_clipboard() {
            return Some(r);
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    None
}

// ---------- JXA 桥接（osascript -l JavaScript，脚本经 stdin 喂入） ----------

/// 执行一段 JXA 脚本，返回 stdout（失败返回 Err 带 stderr 摘要）。
/// 所有脚本约定以 JSON.stringify(...) 结尾输出，Rust 侧统一按 JSON 解析。
///
/// JXA 桥接两大坑（本文件所有脚本已规避，改动脚本时勿回退）：
///   1. 零参数 ObjC 方法（如 NSPasteboard.clearContents）按**属性访问即调用**，
///      再加 `()` 会因返回值是数字而非函数而 TypeError——写成 `pb.clearContents;`
///   2. NSInteger 属性读出（如 changeCount）经 JSON.stringify 变成**字符串**，
///      必须 Number() 显式转回数字，否则 serde 按 u64 解析失败
fn run_jxa(script: &str) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("osascript")
        .args(["-l", "JavaScript", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 osascript 失败: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("写入脚本失败: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("执行 osascript 失败: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if msg.is_empty() {
            "osascript 执行失败".into()
        } else {
            msg
        })
    }
}

/// 把任意可序列化值嵌成 JS 字面量（serde_json 输出即合法 JS 表达式）
fn js_lit(v: &impl serde::Serialize) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// 剪贴板状态探针结果
#[derive(serde::Deserialize)]
struct ProbeMeta {
    /// NSPasteboard.changeCount：系统保证每次内容变化自增
    count: u64,
    /// 是否有 public.png / public.tiff
    image: bool,
    /// 是否有 public.file-url
    file: bool,
    /// 是否有 public.html（或 Apple HTML pasteboard type）
    html: bool,
    /// 前台应用名（复制来源）
    app: String,
}

/// 探剪贴板：changeCount + 各类型有无 + 前台应用。每跳调用（~43ms）。
fn probe_meta() -> Option<ProbeMeta> {
    let script = r#"ObjC.import('AppKit');
const pb = $.NSPasteboard.generalPasteboard;
const types = ObjC.deepUnwrap(pb.types) || [];
const has = (t) => types.indexOf(t) >= 0;
let app = '';
const fr = $.NSWorkspace.sharedWorkspace.frontmostApplication;
if (fr && fr.localizedName) { app = fr.localizedName.js; }
JSON.stringify({
  // JXA 把 NSInteger 桥接成字符串（保 64 位精度），必须 Number() 显式转回数字，
  // 否则 Rust 侧 serde 按 u64 解析失败
  count: Number(pb.changeCount),
  image: has('public.png') || has('public.tiff'),
  file: has('public.file-url'),
  html: has('public.html') || has('Apple HTML pasteboard type'),
  app: app
})"#;
    let out = run_jxa(script).ok()?;
    serde_json::from_str(out.trim()).ok()
}

/// 读剪贴板图片：NSData 直写临时文件再由 Rust 读回（png 优先，tiff 兜底）。
/// 返回原始字节与格式。
fn probe_image() -> Option<(Vec<u8>, MacImageFormat)> {
    let base = std::env::temp_dir().join(format!("x-hub-clip-probe-{}", std::process::id()));
    let png_path = base.with_extension("png");
    let tiff_path = base.with_extension("tiff");
    let script = format!(
        r#"ObjC.import('AppKit');
const pb = $.NSPasteboard.generalPasteboard;
let data = pb.dataForType('public.png');
let ext = 'png';
if (!data || data.length === 0) {{ data = pb.dataForType('public.tiff'); ext = 'tiff'; }}
if (!data || data.length === 0) {{ throw new Error('no image data'); }}
const target = ext === 'png' ? {png} : {tiff};
if (!data.writeToFileAtomically(target, true)) {{ throw new Error('write temp fail'); }}
JSON.stringify(ext)"#,
        png = js_lit(&png_path.to_string_lossy()),
        tiff = js_lit(&tiff_path.to_string_lossy()),
    );
    let out = run_jxa(&script).ok()?;
    let ext: String = serde_json::from_str(out.trim()).ok()?;
    let (path, format) = if ext == "png" {
        (&png_path, MacImageFormat::Png)
    } else {
        (&tiff_path, MacImageFormat::Tiff)
    };
    let bytes = std::fs::read(path).ok()?;
    // 清理两个可能的临时落点（互斥写入，两个都扫一遍最稳）
    let _ = std::fs::remove_file(&png_path);
    let _ = std::fs::remove_file(&tiff_path);
    Some((bytes, format))
}

/// 读剪贴板文件列表：readObjectsForClasses 读 NSURL（Finder 多选拷贝的主路径），
/// 失败回退 pasteboardItems 逐个取 public.file-url。
fn probe_files() -> Option<Vec<String>> {
    let script = r#"ObjC.import('AppKit');
const pb = $.NSPasteboard.generalPasteboard;
const out = [];
try {
  const urls = pb.readObjectsForClassesOptions($.NSArray.arrayWithObject($.NSURL), $.NSDictionary.dictionary);
  if (urls) {
    for (let i = 0; i < urls.count; i++) {
      const u = urls.objectAtIndex(i);
      if (u.isFileURL && u.path) { out.push(u.path.js); }
    }
  }
} catch (e) {
  const items = pb.pasteboardItems;
  if (items) {
    for (let i = 0; i < items.count; i++) {
      const s = items.objectAtIndex(i).stringForType('public.file-url');
      if (s) {
        const p = $.NSURL.URLWithString(s).path;
        if (p) { out.push(p.js); }
      }
    }
  }
}
JSON.stringify(out)"#;
    let out = run_jxa(script).ok()?;
    serde_json::from_str(out.trim()).ok()
}

/// 读剪贴板富文本片段（public.html 优先，Apple HTML pasteboard type 兜底）。
/// 超大片段（>256K UTF-16 单元）直接丢弃：远超入库上限 MAX_HTML_LEN，防拖垮探针。
fn probe_html() -> Option<String> {
    let script = r#"ObjC.import('AppKit');
const pb = $.NSPasteboard.generalPasteboard;
let h = pb.stringForType('public.html');
if (!h) { h = pb.stringForType('Apple HTML pasteboard type'); }
if (h && h.length > 262144) { h = null; }
JSON.stringify(h ? h.js : null)"#;
    let out = run_jxa(script).ok()?;
    serde_json::from_str::<Option<String>>(out.trim()).ok()?
}

// ---------- 写入（回声指纹随写随记） ----------

/// 写剪贴板文本：带 html 时经 JXA 同写 utf8-plain-text + public.html 两种类型
/// （pbcopy 只能写纯文本）；纯文本保持 pbcopy 快路径。
pub fn set_clipboard(text: &str, html: Option<&str>) -> Result<(), String> {
    mark_self_set(text, html);
    match html {
        None => pbcopy(text),
        Some(h) => {
            let script = format!(
                r#"ObjC.import('AppKit');
const pb = $.NSPasteboard.generalPasteboard;
pb.clearContents;
const it = $.NSPasteboardItem.alloc.init;
it.setStringForType({text}, 'public.utf8-plain-text');
it.setStringForType({html}, 'public.html');
if (!pb.writeObjects($.NSArray.arrayWithObject(it))) {{ throw new Error('write fail'); }}"#,
                text = js_lit(&text),
                html = js_lit(&h),
            );
            run_jxa(&script)
                .map(|_| ())
                .map_err(|e| format!("写入剪贴板失败: {e}"))
        }
    }
}

/// 图片字节 → 目标格式转码（纯 image 解码编码，跨平台）
pub fn transcode_image_bytes(bytes: &[u8], dest_ext: &str) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("解析图片失败: {e}"))?;
    let fmt = match dest_ext.to_ascii_lowercase().as_str() {
        "png" => image::ImageFormat::Png,
        "jpeg" | "jpg" => image::ImageFormat::Jpeg,
        "webp" => image::ImageFormat::WebP,
        "gif" => image::ImageFormat::Gif,
        other => return Err(format!("不支持的图片格式: {other}")),
    };
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), fmt)
        .map_err(|e| format!("图片转码失败: {e}"))?;
    Ok(out)
}

/// 写图片剪贴板（字节入口，recorder 截图不落盘直进剪贴板）：统一按 PNG 写回，
/// 非 PNG 先转码；echo 指纹基于「实际写入的字节」——与监听端 probe 读回的一致，
/// 确保截图后粘贴/监听能被正确识别为自身写入回声。
/// 字节经临时文件中转给 JXA（避免脚本内嵌大体积 base64），用后即删。
pub fn set_clipboard_image_bytes(bytes: &[u8]) -> Result<(), String> {
    let is_png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let png_bytes = if is_png {
        bytes.to_vec()
    } else {
        transcode_image_bytes(bytes, "png")?
    };
    let tmp = std::env::temp_dir().join(format!("x-hub-clip-img-{}.png", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, &png_bytes) {
        return Err(format!("写入临时文件失败: {e}"));
    }
    let result = write_png_to_clipboard_via_jxa(&tmp.to_string_lossy());
    let _ = std::fs::remove_file(&tmp);
    result?;
    record_self_set_hash(hash_bytes(&png_bytes));
    Ok(())
}

/// 写图片剪贴板（文件入口）：读出字节后走字节入口，两处口径统一
pub fn set_clipboard_image(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片快照失败: {e}"))?;
    set_clipboard_image_bytes(&bytes)
}

/// PNG 落 NSPasteboard 的 JXA 写入（public.png 单类型；mac 应用普遍认 png，
/// 与 Windows 端 CF_DIB+PNG 双写的兼容面等价）
fn write_png_to_clipboard_via_jxa(png_path: &str) -> Result<(), String> {
    let script = format!(
        r#"ObjC.import('AppKit');
const data = $.NSData.dataWithContentsOfFile({path});
if (!data || data.length === 0) {{ throw new Error('read snapshot fail'); }}
const pb = $.NSPasteboard.generalPasteboard;
pb.clearContents;
const it = $.NSPasteboardItem.alloc.init;
it.setDataForType(data, 'public.png');
if (!pb.writeObjects($.NSArray.arrayWithObject(it))) {{ throw new Error('write fail'); }}"#,
        path = js_lit(&png_path),
    );
    run_jxa(&script)
        .map(|_| ())
        .map_err(|e| format!("写入图片剪贴板失败: {e}"))
}

/// 写文件剪贴板：NSURL 经 writeObjects 直写（只引用原路径，不拷贝文件内容）。
/// 系统会自动补 NSFilenamesPboardType 等传统类型，老应用兼容性最好（Finder 同款写法）。
/// 已不存在的路径会被剔除；全部失效则报错提示用户。
pub fn set_clipboard_files(paths: &[String]) -> Result<(), String> {
    let existing: Vec<String> = paths
        .iter()
        .filter(|p| std::path::Path::new(p).exists())
        .cloned()
        .collect();
    if existing.is_empty() {
        return Err("文件已不存在，无法粘贴".into());
    }
    let hash = hash_files(&existing);
    let script = format!(
        r#"ObjC.import('AppKit');
const PATHS = {paths};
const pb = $.NSPasteboard.generalPasteboard;
pb.clearContents;
const urls = $.NSMutableArray.array;
for (const p of PATHS) {{
  urls.addObject($.NSURL.fileURLWithPath(p));
}}
if (!pb.writeObjects(urls)) {{ throw new Error('write fail'); }}"#,
        paths = js_lit(&existing),
    );
    run_jxa(&script).map_err(|e| format!("写入文件剪贴板失败: {e}"))?;
    record_self_set_hash(hash);
    Ok(())
}

// ---------- 图片快照落盘 ----------

/// 剪贴板图片快照目录：`数据根/clipboard/images/`（与 Windows 同口径）
fn clipboard_images_dir() -> Option<std::path::PathBuf> {
    let dir = crate::paths::data_root().join("clipboard").join("images");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 图片落盘快照：PNG 原样存 .png；TIFF 转 PNG（转码失败原样存 .tiff 兜底，
/// 写回剪贴板时再转）。文件名带字节哈希前缀 + 纳秒时间戳防撞。
/// 返回快照绝对路径（失败返回 None，由调用方决定是否跳过）。
fn save_image_snapshot(bytes: &[u8], format: MacImageFormat, hash: u64) -> Option<String> {
    let dir = clipboard_images_dir()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{:016x}_{}", hash, nanos);
    let (path, data): (std::path::PathBuf, Vec<u8>) = match format {
        MacImageFormat::Png => (dir.join(format!("{name}.png")), bytes.to_vec()),
        MacImageFormat::Tiff => match transcode_image_bytes(bytes, "png") {
            Ok(png) => (dir.join(format!("{name}.png")), png),
            Err(_) => (dir.join(format!("{name}.tiff")), bytes.to_vec()),
        },
    };
    std::fs::write(&path, &data).ok()?;
    Some(path.to_string_lossy().into_owned())
}

// ---------- 浮层窗口 ----------

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
    .build()
}

/// 启动时预创建剪贴板浮层窗口（隐藏常驻）
pub fn init_overlay_window(app: &AppHandle) {
    if app.get_webview_window(CLIPBOARD_WINDOW_LABEL).is_some() {
        return;
    }
    match build_overlay_window(app) {
        Ok(_) => log::info!("剪贴板浮层窗口已预创建（隐藏常驻）"),
        Err(e) => log::warn!("剪贴板浮层窗口预创建失败: {e}"),
    }
}

/// 唤起/收起剪贴板浮层
pub fn toggle_overlay(app: &AppHandle) {
    let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        init_overlay_window(app);
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
    } else {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = app.emit_to(CLIPBOARD_WINDOW_LABEL, "clipboard-shown", ());
    }
}

pub fn activate_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn hide_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        let _ = win.hide();
    }
}

/// 粘贴到唤起前聚焦的窗口：macOS 无通用注入手段，写剪贴板已在调用方
/// （set_item_clipboard）完成，这里只收起浮层把焦点还给目标窗口，由用户按 Cmd+V。
/// 注意绝不能在这里再写剪贴板：图片/文件粘贴时 content 为空串，写空文本会覆盖
/// 刚写入的图片/文件数据。参数保留与 Windows 版同签名。
pub fn paste_to_previous_window(app: &AppHandle, _content: &str, _html: Option<&str>) {
    if let Some(win) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        let _ = win.hide();
    }
}

// ---------- 历史监听 ----------

/// 入库辅助：拿数据库连接并执行给定 repo 操作
fn insert_into_db<T>(
    app: &AppHandle,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.try_state::<DbState>().ok_or("剪贴板状态未就绪")?;
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

/// 处理一次剪贴板变化：按 图片 > 文件 > 文本 分派（优先级与 Windows 一致）。
/// 返回 () 而非 Result：失败路径各自 log，避免长 if 链再包一层。
fn handle_change(app: &AppHandle, meta: &ProbeMeta) {
    let cfg = crate::config::load();
    if cfg.clipboard_paused {
        return;
    }
    let source = if meta.app.is_empty() {
        None
    } else {
        Some(meta.app.clone())
    };

    if meta.image {
        if !cfg.clipboard_image_enabled {
            return;
        }
        let Some((bytes, format)) = probe_image() else {
            log::warn!("剪贴板图片读取失败，已跳过");
            return;
        };
        let hash = hash_bytes(&bytes);
        if is_self_set_hash(hash) {
            log::debug!("剪贴板监听：忽略自身图片写入回声");
            return;
        }
        let Some(path) = save_image_snapshot(&bytes, format, hash) else {
            log::warn!("剪贴板图片快照落盘失败，已跳过");
            return;
        };
        let dedup_key = format!("{:016x}", hash);
        let result = insert_into_db(app, |conn| {
            crate::repo::clipboard::insert_image(conn, &dedup_key, &path, source.as_deref())
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(true) => {}
            Ok(false) => {
                // 相同图片已存在（去重挪到最前），删除本次落盘的冗余快照
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => log::warn!("剪贴板历史入库失败: {e}"),
        }
        return;
    }

    if meta.file {
        if !cfg.clipboard_file_enabled {
            return;
        }
        let Some(paths) = probe_files().filter(|p| !p.is_empty()) else {
            return;
        };
        let hash = hash_files(&paths);
        if is_self_set_hash(hash) {
            log::debug!("剪贴板监听：忽略自身文件写入回声");
            return;
        }
        if let Err(e) = insert_into_db(app, |conn| {
            crate::repo::clipboard::insert_files(conn, &paths, source.as_deref())
                .map_err(|e| e.to_string())
        }) {
            log::warn!("剪贴板历史入库失败: {e}");
        }
        return;
    }

    // 文本：内容走 pbpaste 快路径，html 仅在探针报告有富文本时才取
    let Some((text, _)) = read_clipboard() else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    let html = if meta.html {
        probe_html()
    } else {
        None
    };
    // 自复制回声：粘贴/复制历史项后系统仍会置回内容，跳过自身写入
    if is_self_set(&text, html.as_deref()) {
        log::debug!("剪贴板监听：忽略自身文本写入回声");
        return;
    }
    if let Err(e) = insert_into_db(app, |conn| {
        crate::repo::clipboard::insert(conn, &text, html.as_deref(), source.as_deref())
            .map_err(|e| e.to_string())
    }) {
        log::warn!("剪贴板历史入库失败: {e}");
    }
}

/// 启动剪贴板监听线程：轮询 NSPasteboard.changeCount（1.2s/跳），变化时按
/// 图片 > 文件 > 文本 分派入库。计数未变则整跳跳过（pbpaste 都不跑）。
/// 首次探针作为种子：启动时剪贴板上已有的内容不算「变化」，不重复入库。
pub fn start_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        log::info!("剪贴板监听线程启动（macOS NSPasteboard 轮询，{}ms/跳）", POLL_INTERVAL_MS);
        let mut last_count: Option<u64> = None;
        let mut seeded = false;
        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            let Some(meta) = probe_meta() else {
                continue;
            };
            if !seeded {
                seeded = true;
                last_count = Some(meta.count);
                continue;
            }
            if last_count == Some(meta.count) {
                continue;
            }
            last_count = Some(meta.count);
            handle_change(&app, &meta);
        }
    });
}

/// macOS 无延迟窗口操作 worker 需求（Windows 版专属），空实现占位
pub fn init_win_op_worker() {}
