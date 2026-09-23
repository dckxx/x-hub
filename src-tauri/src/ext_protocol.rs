//! 扩展内容协议 `xhub-ext://`：扩展入口与其相对资源的**唯一来源**。
//!
//! 为什么不再让扩展走 asset 协议（见 `docs/adr/0008-extension-content-origin-isolation.md`）：
//! asset 协议的作用域是**全局单例**，而扩展 iframe 一旦与用户数据同源，扩展里一行 `fetch`
//! 就能取走数据根下的数据库（`xhub.db`）、`app.json` 与日志，**绕开桥 API 的权限系统**。
//! 改用独立协议后，扩展 origin 是 `xhub-ext.localhost`，与 `asset.localhost`
//! （图标 / 壁纸 / 剪贴板图片）**跨源**，扩展从此碰不到宿主数据命名空间。
//!
//! URL 形态：`http://xhub-ext.localhost/<扩展 id>/<扩展目录内相对路径>`
//! - WebView2 把自定义协议映射为 `http://<scheme>.localhost/`；非 Windows 平台为
//!   `<scheme>://localhost/`，见 [`base_url`]。
//! - 入口 HTML 由本模块**动态注入桥脚本**后返回，不再把注入结果落盘到扩展目录的
//!   `.xhpack/`（开发扩展的源码目录不该被宿主写脏）。
//!
//! 安全校验（缺一不可）：扩展 id 形状白名单；相对路径逐段 percent 解码后禁止 `..`、
//! 反斜杠、冒号（Windows 盘符 / ADS）与 NUL；解析结果 canonicalize 后必须仍在扩展目录内
//! （防符号链接逃逸）。

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::http::{Request, Response};
use tauri::Manager;

/// URL 路径段的编码集合（保留字母数字与 `-`/`_`/`.`/`~`，其余转义），
/// 与 `url` crate 的 PATH_SEGMENT 口径一致：`/` 必须转义，否则会被当成路径分隔符。
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'\\')
    .add(b'^')
    .add(b'|');

/// 所有响应都不缓存：本地读盘成本可忽略，而缓存会让「改了代码没生效」与
/// 「扩展更新后仍加载旧资源」变成排查成本极高的偶发问题。
const CACHE_CONTROL: &str = "no-store";

/// 开发扩展目录映射（扩展 id → 源码目录）。「我的扩展」登记时填充；
/// 已装扩展**不在**此表，回退 `<数据根>/extensions/<id>`。
#[derive(Default)]
pub struct DevExtensionDirs(pub Mutex<HashMap<String, PathBuf>>);

impl DevExtensionDirs {
    pub fn get(&self, id: &str) -> Option<PathBuf> {
        self.0.lock().ok().and_then(|m| m.get(id).cloned())
    }

    pub fn insert(&self, id: String, dir: PathBuf) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(id, dir);
        }
    }

    pub fn remove(&self, id: &str) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(id);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut m) = self.0.lock() {
            m.clear();
        }
    }

    /// 当前已注册的开发扩展（id, 目录）快照，供扫描合并使用
    pub fn snapshot(&self) -> Vec<(String, PathBuf)> {
        self.0
            .lock()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }
}

/// 协议 URL 前缀。WebView2（Windows）把自定义协议映射为 `http://<scheme>.localhost/`；
/// 其它平台为 `<scheme>://localhost/`。
pub fn base_url() -> &'static str {
    if cfg!(target_os = "windows") {
        "http://xhub-ext.localhost/"
    } else {
        "xhub-ext://localhost/"
    }
}

/// 把扩展 id 与扩展目录内的相对路径拼成 iframe 可加载的入口 URL。
/// `rel` 形如 `./module/index.html`（manifest 里写的是相对路径）。
pub fn entry_url(id: &str, rel: &str) -> String {
    let rel = encode_rel_path(rel);
    format!("{}{}/{}", base_url(), encode_segment(id), rel)
}

fn encode_segment(seg: &str) -> String {
    utf8_percent_encode(seg, PATH_SEGMENT).to_string()
}

fn encode_rel_path(rel: &str) -> String {
    rel.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// 解析某扩展的内容目录：优先「我的扩展」登记的源码目录，其次已装扩展根。
///
/// 返回前一律归一成普通路径（剥掉 Windows 的 `\\?\` verbatim 前缀）：调用方会把这个目录
/// 交给**外部程序**——`service.rs` 拿它拼后端脚本路径喂给 Node、`open_extension_dir` 交给
/// explorer——而 Node 的 CJS 加载器读不了带前缀的脚本路径（会 `EISDIR lstat 'A:'` 后
/// `exit 1`，即「开发目录挂的 service 扩展后端静默起不来」的根因，见 `paths::simplify_path`）。
/// 宿主内部的 fs 调用两种形式都能用，所以统一在这一个出口归一，覆盖全部调用方。
pub fn resolve_ext_dir(app: &tauri::AppHandle, id: &str) -> Result<PathBuf, String> {
    if let Some(state) = app.try_state::<DevExtensionDirs>() {
        if let Some(dir) = state.get(id) {
            if dir.is_dir() {
                return Ok(crate::paths::simplify_existing(&dir));
            }
        }
    }
    let dir = crate::extension::extensions_root(app)?.join(id);
    if dir.is_dir() {
        Ok(crate::paths::simplify_existing(&dir))
    } else {
        Err(format!("NOT_FOUND: 扩展 {id} 不存在"))
    }
}

/// 扩展 id 形状：反向域名，字符集 `[A-Za-z0-9._-]`，不以 `.` 开头（避免落到隐藏目录）。
fn is_valid_ext_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn decode_segment(seg: &str) -> Option<String> {
    percent_decode_str(seg).decode_utf8().ok().map(|s| s.into_owned())
}

/// 协议处理入口（在 `lib.rs` 的 Builder 上注册）。
pub fn handle(app: &tauri::AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let raw_path = request.uri().path().trim_start_matches('/').to_string();
    // Referer 用于兼容回退（历史扩展的 `../assets/...` 写法会丢掉扩展前缀）
    let referer = request
        .headers()
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    match serve(app, &raw_path, referer.as_deref()) {
        Ok((mime, bytes)) => Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", CACHE_CONTROL)
            .body(bytes)
            .unwrap(),
        Err(status) => Response::builder()
            .status(status)
            .header("Cache-Control", CACHE_CONTROL)
            .body(Vec::new())
            .unwrap(),
    }
}

/// 段是否危险：`..` 逃逸 / 反斜杠 / 冒号（Windows 盘符与 NTFS 数据流）/ NUL / 控制字符
fn is_unsafe_segment(seg: &str) -> bool {
    seg == ".."
        || seg.contains('\\')
        || seg.contains(':')
        || seg.contains('\0')
        || seg.chars().any(|c| c.is_control())
}

/// 把**整条**路径当相对路径解析成经过校验的段列表（供兼容回退使用）
fn parse_rel_path(raw_path: &str) -> Result<Vec<String>, u16> {
    let mut rel_parts: Vec<String> = Vec::new();
    for raw in raw_path.split('/') {
        if raw.is_empty() {
            continue;
        }
        let seg = decode_segment(raw).ok_or(400u16)?;
        if seg == "." {
            continue;
        }
        if is_unsafe_segment(&seg) {
            return Err(400);
        }
        rel_parts.push(seg);
    }
    if rel_parts.is_empty() {
        return Err(404);
    }
    Ok(rel_parts)
}

/// 把请求路径解析成 `(扩展 id, 相对路径段)`。失败返回 HTTP 状态码。
///
/// 独立成纯函数是为了单测安全边界：`..` 逃逸、反斜杠、冒号（Windows 盘符 / NTFS 数据流）、
/// NUL 与控制字符、空路径（不列目录）。这些分支任何一条漏掉，扩展就能读到扩展目录之外。
fn parse_request_path(raw_path: &str) -> Result<(String, Vec<String>), u16> {
    let mut segs = raw_path.split('/');
    let raw_id = segs.next().unwrap_or("");
    if raw_id.is_empty() {
        return Err(404);
    }
    let id = decode_segment(raw_id).ok_or(400u16)?;
    if !is_valid_ext_id(&id) {
        log::warn!("扩展协议拒绝非法 id: {raw_id}");
        return Err(400);
    }

    let mut rel_parts: Vec<String> = Vec::new();
    for raw in segs {
        if raw.is_empty() {
            continue; // 容忍重复斜杠
        }
        let seg = decode_segment(raw).ok_or(400u16)?;
        if seg == "." {
            continue;
        }
        if is_unsafe_segment(&seg) {
            log::warn!("扩展协议拒绝非法路径段 {id}: {raw}");
            return Err(400);
        }
        rel_parts.push(seg);
    }
    if rel_parts.is_empty() {
        return Err(404); // 不列目录
    }
    Ok((id, rel_parts))
}

/// 兼容回退：把"丢了扩展前缀"的请求归回它所属的扩展。
///
/// 为什么需要：**旧实现**把入口 HTML 写到 `<扩展目录>/.xhpack/<surface>.html`（下一层子目录），
/// 因此历史扩展普遍用 `../assets/x.js`、`../favicon.ico` 这类**上一级**写法引用扩展根下的资源。
/// 新协议的入口就在扩展根（`/<id>/tool.html`），浏览器把 `../assets/x.js` 规范化成 `/assets/x.js`
/// —— 第一段不再是扩展 id，按正常解析会 404，表现为「扩展白屏、但入口日志正常」。
///
/// 归因依据是 Referer：同源请求由**浏览器**设置该头，页面脚本无法伪造。
/// 安全上不扩大能力：`/<任意 id>/...` 本来就是可直接访问的（扩展之间本就互读，见 ADR 0008 的残余风险），
/// 这里只是把丢了前缀的请求接回去；`..`/反斜杠/冒号等危险段依然一律拒绝。
fn fallback_from_referer(raw_path: &str, referer: &str) -> Option<(String, Vec<String>)> {
    let base = base_url().trim_end_matches('/');
    let idx = referer.find(base)?;
    let rest = referer[idx + base.len()..].trim_start_matches('/');
    let id_raw = rest.split(['/', '?', '#']).next()?;
    let id = decode_segment(id_raw)?;
    if !is_valid_ext_id(&id) {
        return None;
    }
    let rel = parse_rel_path(raw_path).ok()?;
    Some((id, rel))
}

// ---------------- 入口 HTML 的资源引用改写 ----------------
//
// 为什么需要（线上故障，见 issue「插件安装后无法使用」）：
// 入口 URL 是 `/<id>/<rel>`。HTML 里若写 `../assets/x.js`（旧 `.xhpack/<surface>.html`
// 布局留下的写法）或根绝对路径 `/assets/x.js`，浏览器解析成 `/assets/x.js`——**丢掉 `<id>` 段**。
// `fallback_from_referer` 只能救回一层：发起者是入口文档时 Referer 首段就是扩展 id，所以
// 插件首屏打得开；但 chunk 里的 `import("./y.js")` 按**模块自身 URL** 解析成
// `/assets/y.js`，此时 Referer 是该 chunk 的 URL（首段 `assets`，不是已装扩展）→ 回退失效
// → 404 → 页面报 `Failed to fetch dynamically imported module`。
//
// 在入口 HTML 过手时（本来就为注入桥脚本读它）把这类引用改成 `/<id>/…`，其后的嵌套
// import 按模块 URL 解析就自然落回扩展内。注意顺序：**先改写、后注入桥脚本**。
//
// 只动 `src` / `href` / `poster` 三个属性的值，且逐字节保留其余内容；`#…`、带 scheme、
// `//host`、空值一律不动。深层入口（如 `module/index.html`）里的 `../x` 解析后没丢前缀，
// 保持原样。残余：写在 **JS/CSS 里**的绝对路径（`import("/assets/x.js")`、`url(/…)`）
// 改写不到，仍需 Referer 回退兜底。

/// 是否带 scheme（`http:`、`data:`、`blob:`…）：首字符字母，其后字母数字与 `+.-` 直到 `:`。
fn has_scheme(v: &str) -> bool {
    let mut chars = v.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

/// 拆出 `path` 与 `?query#fragment` 尾巴（改写只针对 path，尾巴原样接回）
fn split_query_fragment(v: &str) -> (&str, &str) {
    let idx = v.find(['?', '#']).unwrap_or(v.len());
    (&v[..idx], &v[idx..])
}

/// 把相对引用按入口所在目录解析成段列表。返回 `None` = 解析后没丢扩展前缀（保持原样）；
/// 返回 `Some(段)` = 中途越过扩展根（当前会丢前缀），按这些段改写成扩展内绝对路径。
fn resolve_relative_ref(doc_dir: &[String], path: &str) -> Option<Vec<String>> {
    let mut stack: Vec<String> = doc_dir.to_vec();
    let mut escaped = false;
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    escaped = true;
                }
            }
            s => stack.push(s.to_string()),
        }
    }
    escaped.then_some(stack)
}

/// 改写单个属性值（不是 `src`/`href`/`poster` 或不该动的引用一律原样返回）
fn rewrite_ref_value(name: &str, value: &str, id: &str, doc_dir: &[String]) -> String {
    let lower = name.to_ascii_lowercase();
    if !matches!(lower.as_str(), "src" | "href" | "poster") {
        return value.to_string();
    }
    if value.is_empty() || value.starts_with('#') || value.starts_with("//") || has_scheme(value) {
        return value.to_string();
    }
    // 两侧带空白的值（浏览器会 trim）不碰：改写要按原样保留空白，容易出岔子
    if value.trim() != value {
        return value.to_string();
    }
    let (path, tail) = split_query_fragment(value);
    if path.is_empty() {
        return value.to_string();
    }
    if path.starts_with('/') {
        return format!("/{id}{path}{tail}");
    }
    match resolve_relative_ref(doc_dir, path) {
        Some(segments) => format!("/{id}/{}{tail}", segments.join("/")),
        None => value.to_string(),
    }
}

fn is_html_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// 改写一个标签内部的引用属性。除被改写的值外**逐字节**照抄原内容。
fn rewrite_tag_refs(tag: &str, id: &str, doc_dir: &[String]) -> String {
    let bytes = tag.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(tag.len() + 32);
    // `<` + 标签名
    let mut i = 1;
    while i < len && !is_html_ws(bytes[i]) && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    out.push_str(&tag[..i]);
    while i < len {
        // 属性前的空白
        let ws_start = i;
        while i < len && is_html_ws(bytes[i]) {
            i += 1;
        }
        out.push_str(&tag[ws_start..i]);
        if i >= len || bytes[i] == b'>' || bytes[i] == b'/' {
            out.push_str(&tag[i..]);
            break;
        }
        // 属性名
        let name_start = i;
        while i < len && !is_html_ws(bytes[i]) && !matches!(bytes[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name = &tag[name_start..i];
        out.push_str(name);
        // 可选的 `= 值`
        let before_eq = i;
        while i < len && is_html_ws(bytes[i]) {
            i += 1;
        }
        if i >= len || bytes[i] != b'=' {
            i = before_eq; // 无 `=`：布尔属性，空白留给下一轮
            continue;
        }
        out.push_str(&tag[before_eq..i]);
        i += 1;
        out.push('=');
        let after_eq = i;
        while i < len && is_html_ws(bytes[i]) {
            i += 1;
        }
        out.push_str(&tag[after_eq..i]);
        if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < len && bytes[i] != quote {
                i += 1;
            }
            let value = &tag[value_start..i];
            out.push(quote as char);
            out.push_str(&rewrite_ref_value(name, value, id, doc_dir));
            out.push(quote as char);
            if i < len {
                i += 1; // 收尾引号
            }
        } else {
            let value_start = i;
            while i < len && !is_html_ws(bytes[i]) && bytes[i] != b'>' {
                i += 1;
            }
            out.push_str(&rewrite_ref_value(name, &tag[value_start..i], id, doc_dir));
        }
    }
    out
}

/// 改写入口 HTML 里会丢扩展前缀的资源引用。`rel_parts` 是该 HTML 在扩展目录内的路径段
/// （最后一段是文件名，用于算出它所在目录）。
///
/// 边界：只按「属性名」识别，不解析 HTML 结构——因此注释与内联脚本里**形如标签**的引用
/// （如 `<!-- <img src="../a.png"> -->`、`document.write('<img src="../a.png">')`）同样会被改写；
/// `<style>`/CSS 的 `url()` 不含 src/href/poster 属性，不受影响。除被改写的属性值外，其余
/// 内容逐字节保留。行为由 `rewrites_tag_shaped_refs_inside_comments_and_inline_scripts` 钉住。
pub(crate) fn rewrite_entry_refs(html: &str, id: &str, rel_parts: &[String]) -> String {
    if !html.contains("src") && !html.contains("href") && !html.contains("poster") {
        return html.to_string();
    }
    let doc_dir: Vec<String> = rel_parts[..rel_parts.len().saturating_sub(1)].to_vec();
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() + 64);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // 找标签结束（属性值里的 `>` 不算），找不到就当普通文本继续
            let mut j = i + 1;
            let mut quote: Option<u8> = None;
            while j < bytes.len() {
                let b = bytes[j];
                match quote {
                    Some(q) => {
                        if b == q {
                            quote = None;
                        }
                    }
                    None => {
                        if b == b'"' || b == b'\'' {
                            quote = Some(b);
                        } else if b == b'>' {
                            j += 1;
                            break;
                        }
                    }
                }
                j += 1;
            }
            if quote.is_none() && j <= bytes.len() && bytes[j - 1] == b'>' {
                out.push_str(&rewrite_tag_refs(&html[i..j], id, &doc_dir));
                i = j;
                continue;
            }
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 解析并读取路径：`<扩展 id>/<相对路径>`。失败时返回 HTTP 状态码。
fn serve(app: &tauri::AppHandle, raw_path: &str, referer: Option<&str>) -> Result<(String, Vec<u8>), u16> {
    let (mut id, mut rel_parts) = parse_request_path(raw_path)?;

    // 解析出的 id 必须是**已注册/已装**的扩展。不是的话，最可能的情况是历史扩展用
    // `../assets/x.js`、`../favicon.ico` 这类上一级写法引用扩展根资源，被浏览器规范化后
    // 丢掉了扩展前缀（`assets` 形状合法但不是扩展 id）——此时用 Referer 归因（见 fallback_from_referer）。
    // 注意：危险路径（`..`/反斜杠/冒号…）在 parse_request_path 就被 400 挡掉了，走不到这里。
    let root = match resolve_ext_dir(app, &id) {
        Ok(r) => r,
        Err(_) => {
            let Some((fallback_id, fallback_rel)) = referer.and_then(|r| fallback_from_referer(raw_path, r))
            else {
                return Err(404);
            };
            let Ok(r) = resolve_ext_dir(app, &fallback_id) else {
                return Err(404);
            };
            log::debug!("扩展协议兼容回退: {fallback_id} <- /{raw_path}");
            id = fallback_id;
            rel_parts = fallback_rel;
            r
        }
    };
    let _ = &id;
    let mut full = root.clone();
    for p in &rel_parts {
        full.push(p);
    }

    // canonicalize 后必须仍在扩展目录内：挡住符号链接 / junction 指到目录外的情形
    let root_canon = std::fs::canonicalize(&root).map_err(|_| 404u16)?;
    let full_canon = std::fs::canonicalize(&full).map_err(|_| 404u16)?;
    if !full_canon.starts_with(&root_canon) {
        log::warn!(
            "扩展协议拒绝越界路径 {id}: {}",
            full_canon.display()
        );
        return Err(403);
    }
    if !full_canon.is_file() {
        return Err(404);
    }

    let mut bytes = std::fs::read(&full_canon).map_err(|_| 404u16)?;
    let mime = mime_for(&full_canon);
    // 入口 HTML：动态注入桥脚本（等价于旧的 `.xhpack/<surface>.html`，但不落盘）
    //
    // ⚠️ 这里必须**按扩展名**判断，不能比较 MIME 字符串：`mime_for` 返回的是
    // `"text/html; charset=utf-8"`，与字面量 `"text/html"` 永不相等 —— 曾经的写法导致
    // 桥脚本从未注入，表现为「扩展自己的 JS 能跑、但没有 window.xhub、8 秒后判白屏」。
    if is_html(&full_canon) {
        if let Ok(html) = std::str::from_utf8(&bytes) {
            // 先改写引用、后注入桥脚本：顺序反了会连桥脚本里的字符串一起过一遍改写规则
            let rewritten = rewrite_entry_refs(html, &id, &rel_parts);
            bytes = crate::extension::inject_bridge(&rewritten, crate::extension::XHUB_BRIDGE_SCRIPT)
                .into_bytes();
        }
    }
    Ok((mime.to_string(), bytes))
}

/// 是否是需要注入桥脚本的 HTML（按扩展名，别看 MIME 字符串）
fn is_html(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "html" | "htm"
    )
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_id_shape_is_validated() {
        assert!(is_valid_ext_id("com.x-hub.ctool"));
        assert!(is_valid_ext_id("my_ext-1"));
        assert!(!is_valid_ext_id(""));
        assert!(!is_valid_ext_id(".hidden"));
        assert!(!is_valid_ext_id("a..b"));
        assert!(!is_valid_ext_id("a/b"));
        assert!(!is_valid_ext_id("扩展"));
    }

    #[test]
    fn entry_url_encodes_each_segment() {
        assert_eq!(
            entry_url("com.x-hub.ctool", "./module/index.html"),
            format!("{}com.x-hub.ctool/module/index.html", base_url())
        );
        // 空格与中文按段编码，斜杠保留
        let url = entry_url("com.x-hub.x", "./my dir/页 面.html");
        assert!(url.ends_with("/my%20dir/%E9%A1%B5%20%E9%9D%A2.html"), "{url}");
    }

    #[test]
    fn segment_encoding_roundtrips() {
        for seg in ["a b", "中文", "#x", "%y", "a+b"] {
            let enc = encode_segment(seg);
            assert_eq!(decode_segment(&enc).unwrap(), seg);
        }
    }

    #[test]
    fn falls_back_to_referer_for_legacy_asset_paths() {
        // 历史扩展用 `../assets/x.js` 引用扩展根资源 → 浏览器规范化成 `/assets/x.js`（丢了扩展前缀）
        let referer = format!("{}com.x-hub.ctool/tool.html", base_url());
        let got = fallback_from_referer("assets/tool-abc.js", &referer).expect("应能回退");
        assert_eq!(got.0, "com.x-hub.ctool");
        assert_eq!(got.1, vec!["assets", "tool-abc.js"]);

        // 绝对路径（`../favicon.ico` 越界到根）同理
        assert_eq!(
            fallback_from_referer("favicon.ico", &referer).unwrap().0,
            "com.x-hub.ctool"
        );

        // Referer 带 query 也要能解析
        let with_query = format!("{}com.x-hub.ctool/tool.html?v=1", base_url());
        assert_eq!(
            fallback_from_referer("assets/a.css", &with_query).unwrap().0,
            "com.x-hub.ctool"
        );

        // 非本协议的来源绝不回退（不把任意请求归到某个扩展）
        assert!(fallback_from_referer("assets/a.js", "https://evil.example/x").is_none());
        assert!(fallback_from_referer("assets/a.js", "http://asset.localhost/x").is_none());
        // Referer 第一段**形状非法**也不回退
        assert!(fallback_from_referer("assets/a.js", &format!("{}.hidden/x.html", base_url())).is_none());
        assert!(fallback_from_referer("assets/a.js", &format!("{}a..b/x.html", base_url())).is_none());
        // 注意分工：形状合法但**未安装**的 id（例如 referer 里的 `tool.html`）这里会返回 Some，
        // 由调用方 `serve` 用 `resolve_ext_dir(...).is_ok()` 兜住 —— 那一道是"能不能接管"的判据，
        // 本函数只负责"能不能解析出候选"。改动时别把这道检查挪掉。

        // 危险路径在回退路径上依然被拒（`..` 与反斜杠不能借回退混进来）
        assert!(fallback_from_referer("%2e%2e/secret", &referer).is_none());
        assert!(fallback_from_referer("a%5Cb.js", &referer).is_none());
    }

    #[test]
    fn detects_html_for_bridge_injection() {
        // 关键回归：判断必须按扩展名，不能比较 MIME 字符串
        //（mime_for 返回 "text/html; charset=utf-8"，与 "text/html" 永不相等）
        for name in ["index.html", "view.HTML", "a/b/module.htm"] {
            assert!(is_html(std::path::Path::new(name)), "{name} 应判为 HTML");
        }
        for name in ["app.js", "style.css", "data.json", "icon.svg", "noext"] {
            assert!(!is_html(std::path::Path::new(name)), "{name} 不应判为 HTML");
        }
        // 并且确认 mime_for 的 HTML 结果确实不是裸的 "text/html"（就是当年踩的坑）
        assert_ne!(mime_for(std::path::Path::new("index.html")), "text/html");
    }

    #[test]
    fn parses_valid_request_paths() {
        let (id, parts) = parse_request_path("com.x-hub.ctool/module/index.html").unwrap();
        assert_eq!(id, "com.x-hub.ctool");
        assert_eq!(parts, vec!["module", "index.html"]);

        // 容忍重复斜杠与 `./` 前缀（扩展里写 `./index.html` 很常见）
        let (id2, parts2) = parse_request_path("com.x-hub.ctool//.//view/index.html").unwrap();
        assert_eq!(id2, "com.x-hub.ctool");
        assert_eq!(parts2, vec!["view", "index.html"]);

        // 段内 percent 解码（中文 / 空格目录名）
        let (_, parts3) = parse_request_path("com.x-hub.ctool/%E4%B8%AD%E6%96%87/a%20b.html").unwrap();
        assert_eq!(parts3, vec!["中文", "a b.html"]);
    }

    #[test]
    fn rejects_unsafe_request_paths() {
        // 空路径 / 只要目录 → 404（不列目录）
        assert_eq!(parse_request_path("").unwrap_err(), 404);
        assert_eq!(parse_request_path("com.x-hub.ctool").unwrap_err(), 404);
        assert_eq!(parse_request_path("com.x-hub.ctool/").unwrap_err(), 404);
        assert_eq!(parse_request_path("com.x-hub.ctool/./").unwrap_err(), 404);

        // 非法 id → 400
        assert_eq!(parse_request_path(".hidden/index.html").unwrap_err(), 400);
        assert_eq!(parse_request_path("a..b/index.html").unwrap_err(), 400);
        assert_eq!(parse_request_path("%E6%89%A9%E5%B1%95/index.html").unwrap_err(), 400); // 非 ASCII id

        // 路径逃逸与非法段 → 400
        assert_eq!(parse_request_path("com.x-hub.x/../secret.txt").unwrap_err(), 400);
        assert_eq!(parse_request_path("com.x-hub.x/%2e%2e/secret.txt").unwrap_err(), 400); // 编码后的 ..
        assert_eq!(parse_request_path("com.x-hub.x/a%5Cb.js").unwrap_err(), 400); // 反斜杠 %5C
        assert_eq!(parse_request_path("com.x-hub.x/C:/Windows/win.ini").unwrap_err(), 400); // 盘符冒号
        assert_eq!(parse_request_path("com.x-hub.x/a%00b.js").unwrap_err(), 400); // NUL
        assert_eq!(parse_request_path("com.x-hub.x/a%0Ab.js").unwrap_err(), 400); // 控制字符
    }

    // ---------------- 入口 HTML 引用改写 ----------------

    fn parts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rewrites_refs_that_would_drop_the_extension_prefix() {
        let id = "com.x-hub.ctool";
        // 线上 ctool 2.9.x 的真实写法（解包自 com.x-hub.ctool-2.9.0.xhpack）
        let html = concat!(
            "<link rel=\"icon\" href=\"../favicon.ico\" type=\"image/x-ico\">\n",
            "<script type=\"module\" crossorigin src=\"../assets/tool-CAKAZgPx.js\"></script>\n",
            "<link rel=\"modulepreload\" crossorigin href=\"../assets/vendor-monaco-DSX5bDTJ.js\">\n",
            "<link rel=\"stylesheet\" crossorigin href=\"../assets/tool-DHREzKTU.css\">\n",
        );
        let got = rewrite_entry_refs(html, id, &parts(&["tool.html"]));
        assert!(got.contains(&format!("href=\"/{id}/favicon.ico\"")), "{got}");
        assert!(
            got.contains(&format!("src=\"/{id}/assets/tool-CAKAZgPx.js\"")),
            "{got}"
        );
        assert!(
            got.contains(&format!("href=\"/{id}/assets/vendor-monaco-DSX5bDTJ.js\"")),
            "{got}"
        );
        assert!(
            got.contains(&format!("href=\"/{id}/assets/tool-DHREzKTU.css\"")),
            "{got}"
        );
        // 其余内容逐字节保留（属性顺序、引号、crossorigin 都不动）
        assert!(got.contains("<script type=\"module\" crossorigin "), "{got}");
    }

    #[test]
    fn rewrites_root_absolute_refs_at_any_depth() {
        let id = "com.x-hub.x";
        // 根绝对路径在任何深度都会丢前缀
        let got = rewrite_entry_refs(
            r#"<img src="/root-abs.png"><script src='assets/ok.js'></script>"#,
            id,
            &parts(&["module", "index.html"]),
        );
        assert!(got.contains(&format!("src=\"/{id}/root-abs.png\"")), "{got}");
        // 没丢前缀的相对引用保持原样（深层入口的 `assets/ok.js` 本来就对）
        assert!(got.contains("src='assets/ok.js'"), "{got}");
    }

    #[test]
    fn leaves_non_escaping_refs_and_foreign_urls_alone() {
        let id = "com.x-hub.x";
        let html = concat!(
            "<script src=\"https://cdn.example/x.js\"></script>",
            "<script src=\"//cdn.example/y.js\"></script>",
            "<script src=\"data:text/javascript,\"></script>",
            "<script src=\"blob:abc\"></script>",
            "<a href=\"#anchor\">a</a>",
            "<img src=\"assets/local.png\">",
            "<img src=\"../escapes.png\">",
            "<div data-src=\"../assets/not-an-asset.js\">d</div>",
        );
        // 深层入口：`../assets/a.js`、`../escapes.png` 解析后是 /<id>/…，本来就没丢前缀
        let deep = rewrite_entry_refs(html, id, &parts(&["module", "index.html"]));
        assert_eq!(deep, html, "不该改的引用被动了");
        // 同一份 HTML 放在扩展根：`../…` 会丢前缀，必须改写
        let root = rewrite_entry_refs(html, id, &parts(&["tool.html"]));
        assert!(
            root.contains("data-src=\"../assets/not-an-asset.js\""),
            "非 src/href/poster 属性不该动: {root}"
        );
        assert!(root.contains(&format!("src=\"/{id}/escapes.png\"")), "{root}");
        // 扩展根下的普通相对引用本来就对，保持原样
        assert!(root.contains("src=\"assets/local.png\""), "{root}");
    }

    #[test]
    fn keeps_query_fragment_and_clamps_above_root() {
        let id = "com.x-hub.x";
        let got = rewrite_entry_refs(
            r#"<img src="../a/b.png?x=1#f"><img src="../../x.png">"#,
            id,
            &parts(&["tool.html"]),
        );
        assert!(
            got.contains(&format!("src=\"/{id}/a/b.png?x=1#f\"")),
            "query/fragment 必须保留: {got}"
        );
        assert!(
            got.contains(&format!("src=\"/{id}/x.png\"")),
            "越过扩展根的 `..` 要夹在根上: {got}"
        );
    }

    #[test]
    fn rewrite_is_byte_stable_when_nothing_to_change() {
        let html = concat!(
            "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"UTF-8\"/>",
            "<title>Ctool</title></head><body id=\"root\"><div class=\"a b\"></div>",
            "<input disabled></body></html>",
        );
        assert_eq!(
            rewrite_entry_refs(html, "com.x-hub.x", &parts(&["tool.html"])),
            html
        );
    }

    /// 边界实测（探针 `tmp_probe_comment_and_inline_script` 转正）：改写只按属性名识别，
    /// 不解析 HTML 结构，所以注释与内联脚本里「形如标签」的引用也会被改写——功能无害
    /// （这类引用在本协议下本来就写错），但确实与「逐字节保留」口径有出入，故在此钉住。
    #[test]
    fn rewrites_tag_shaped_refs_inside_comments_and_inline_scripts() {
        let id = "com.x-hub.x";
        let html = concat!(
            "<!-- <script src=\"../assets/in-comment.js\"></script> -->\n",
            "<script>document.write('<img src=\"../assets/in-js.png\">')</script>\n",
            "<style>body{background:url(../assets/bg.png)}</style>\n",
            "<a href=\"../page.html?x=1#f\">p</a>\n",
            "<base href=\"/base/\">\n",
        );
        let got = rewrite_entry_refs(html, id, &parts(&["tool.html"]));

        // 注释：改的是注释文本，对渲染无影响
        assert!(
            got.contains(&format!("src=\"/{id}/assets/in-comment.js\"")),
            "{got}"
        );
        // 内联脚本字符串里形如标签的引用同样会被改写（本例正是期望结果）
        assert!(
            got.contains(&format!("src=\"/{id}/assets/in-js.png\"")),
            "{got}"
        );
        // `<style>` 的 `url()` 不是 src/href/poster 属性 → 不受改写影响。注意它**能**用不是
        // 因为「解析后没丢前缀」：`url(../assets/bg.png)` 相对入口 `/<id>/tool.html` 会解析成
        // `/assets/bg.png`（前缀照样丢），靠的是请求 Referer 是入口文档（外链 CSS 则是 CSS 自身
        // URL）、首段=扩展 id，由 Referer 回退兜住。
        assert!(got.contains("url(../assets/bg.png)"), "{got}");
        // 普通标签：query/fragment 保留、根绝对路径补前缀
        assert!(
            got.contains(&format!("href=\"/{id}/page.html?x=1#f\"")),
            "{got}"
        );
        assert!(got.contains(&format!("href=\"/{id}/base/\"")), "{got}");
    }

    /// 回归 fixture：`tests/extensions/nested-import/`（三级模块链 + 旧 `.xhpack` 的 `../assets/…` 写法）。
    /// 用 `include_str!` 绑住真实 fixture：谁把 `../` 改成 `./`、或删掉嵌套层，测试立刻失败，
    /// 而不是等线上再出一次「点进去正常、点任意功能报 Failed to fetch dynamically imported module」。
    #[test]
    fn nested_import_fixture_refs_are_rewritten() {
        const INDEX: &str = include_str!("../../tests/extensions/nested-import/index.html");
        const ID: &str = "com.x-hub.nested-import";
        let got = rewrite_entry_refs(INDEX, ID, &parts(&["index.html"]));
        assert!(
            got.contains(&format!("src=\"/{ID}/assets/entry.js\"")),
            "{got}"
        );
        assert!(
            got.contains(&format!("href=\"/{ID}/assets/probe.css\"")),
            "{got}"
        );
        // 无目录的根级引用（`../favicon.svg`）：修复前连 Referer 回退都走不到（空 rel_parts 直接 404）
        assert!(
            got.contains(&format!("href=\"/{ID}/favicon.svg\"")),
            "{got}"
        );

        // fixture 的模块链必须真有嵌套，否则失去回归价值
        const ENTRY: &str = include_str!("../../tests/extensions/nested-import/assets/entry.js");
        const LEVEL1: &str = include_str!("../../tests/extensions/nested-import/assets/level1.js");
        assert!(ENTRY.contains("./level1.js"), "entry 必须 import level1");
        assert!(LEVEL1.contains("./level2.js"), "level1 必须 import level2");
    }
}
