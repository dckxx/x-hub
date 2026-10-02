//! 网页图标（favicon）抓取：导入书签 / 网页资源时自动补齐站点图标。
//!
//! 策略（不依赖 Google/DuckDuckGo 等第三方 favicon 服务——国内网络不可达）：
//! ① `GET {origin}/favicon.ico`（跟随重定向，CDN 跳转很常见）→ 魔数校验图片类型后落盘；
//! ② 失败再 `GET {origin}/` 首页（截断 128KB，`<link rel=icon>` 几乎都在 `<head>` 里）
//!    解析出图标地址再抓一次。
//! 产物按内容哈希存 `数据根/icons/fav-{hash16}.{ext}`（与拖拽/扫描图标同一 asset 白名单
//! 目录、同为绝对路径口径），前端 `<img>` 经 convertFileSrc 渲染；ico/png/svg/gif/webp/
//! jpg/bmp WebView2 都能直接显示，无需解码转换。
//!
//! 批量入口 [`fetch_favicons`]：按域名去重后并发抓取（信号量 8、单请求 8s 超时）。
//! 任何失败都只返回 None、绝不报错中断——图标是锦上添花，不是导入链路的一环。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

const PER_REQUEST_TIMEOUT_SECS: u64 = 8;
const CONNECT_TIMEOUT_SECS: u64 = 5;
/// 图标尺寸上限：超过视为异常（大图/误把网页当下发物），按失败处理
const MAX_ICON_BYTES: usize = 512 * 1024;
/// 首页 HTML 只读前 128KB
const MAX_HTML_BYTES: usize = 128 * 1024;
/// 并发抓取上限：一批书签几十个域名很常见，不限并发会瞬间打满连接
const CONCURRENCY: usize = 8;
const USER_AGENT: &str = concat!(
    "x-hub/",
    env!("CARGO_PKG_VERSION"),
    " (local-first desktop dashboard)"
);

/// 从 URL 提取 origin（scheme://authority，scheme 归一为小写、剥掉 userinfo）。
/// 非 http/https（javascript:/file:/chrome: 等书签残留）一律 None。
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    // 剥 userinfo（http://user@host/ 形态），host 大小写不敏感一并归一
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// 按魔数判定图片类型（Content-Type 只做 SVG 的兜底参考）。
/// 刻意不信任 Content-Type：站点把 PNG 命名成 .ico、把 404/首页 HTML 回 200 的都不罕见。
fn sniff_image_ext(bytes: &[u8], content_type: &str) -> Option<&'static str> {
    if bytes.is_empty() {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("jpg");
    }
    if bytes.len() >= 4 && bytes[0..4] == [0, 0, 1, 0] {
        return Some("ico");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("bmp");
    }
    // SVG 没有稳定魔数（可能带 BOM / <?xml 前缀）：看开头一小段是否 <svg 且不是 HTML 页面
    let head_len = bytes.len().min(512);
    let head = String::from_utf8_lossy(&bytes[..head_len]).to_ascii_lowercase();
    if head.contains("<svg") && !head.contains("<html") {
        return Some("svg");
    }
    // 罕见：xml 声明/注释很长把 <svg 顶出了 512 字节，信一次 Content-Type
    if content_type.contains("svg") && !head.contains("<html") {
        return Some("svg");
    }
    None
}

/// 从标签原文里取属性值：在 ASCII 小写副本上定位 `name=`（避免 data-href 之类误命中），
/// 再按同偏移回原文切片——ASCII 小写不改变字节长度，偏移两串通用，且保住 URL 本身的大小写。
fn attr_value(raw_tag: &str, lower_tag: &str, name: &str) -> Option<String> {
    let lb = lower_tag.as_bytes();
    let rb = raw_tag.as_bytes();
    debug_assert_eq!(lb.len(), rb.len());
    let nb = name.as_bytes();
    if lb.len() < nb.len() {
        return None;
    }
    for i in 0..=(lb.len() - nb.len()) {
        if &lb[i..i + nb.len()] != nb {
            continue;
        }
        // 前一个字符必须是空白或引号（排除 data-href / href-x 这类伪属性名）
        let prev_ok = i == 0
            || lb[i - 1].is_ascii_whitespace()
            || lb[i - 1] == b'"'
            || lb[i - 1] == b'\'';
        if !prev_ok {
            continue;
        }
        let mut j = i + nb.len();
        while j < lb.len() && lb[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= lb.len() || lb[j] != b'=' {
            continue;
        }
        j += 1;
        while j < lb.len() && lb[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= lb.len() {
            return None;
        }
        if lb[j] == b'"' || lb[j] == b'\'' {
            let quote = lb[j];
            let vstart = j + 1;
            let vlen = lb[vstart..].iter().position(|&c| c == quote)?;
            let vend = vstart + vlen;
            return Some(raw_tag[vstart..vend].to_string());
        }
        // 未加引号的值：取到下一个空白
        let vstart = j;
        let vend = lb[vstart..]
            .iter()
            .position(|c| c.is_ascii_whitespace())
            .map(|n| vstart + n)
            .unwrap_or(lb.len());
        return Some(raw_tag[vstart..vend].to_string());
    }
    None
}

/// 在首页 HTML 里找 `<link rel=…icon… href=…>`。大小写不敏感；apple-touch-icon 往往更高清、
/// 一并接受；stylesheet 明确排除（rel 里偶带 icon 字样的样式声明）。
fn find_icon_href(html: &str) -> Option<String> {
    // to_ascii_lowercase 逐字节等长变换：索引可双向通用（to_lowercase 会变长度，不能用）
    let lower = html.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(p) = lower[search_from..].find("<link") {
        let tag_start = search_from + p;
        let Some(tag_end_rel) = lower[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_rel;
        let tag = &lower[tag_start..tag_end];
        if tag.contains("icon") && !tag.contains("stylesheet") {
            if let Some(href) = attr_value(&html[tag_start..tag_end], tag, "href") {
                return Some(href);
            }
        }
        search_from = tag_end + 1;
    }
    None
}

/// 相对 href → 绝对 URL（只处理 //、/、相对根三种形态，覆盖站点实际写法）。
/// `data:`/`javascript:` 等非 http 形态直接放弃。
fn resolve_href(origin: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with("data:")
        || href.starts_with("javascript:")
        || href.starts_with('#')
    {
        return None;
    }
    if let Some(rest) = href.strip_prefix("https://") {
        return Some(format!("https://{rest}"));
    }
    if let Some(rest) = href.strip_prefix("http://") {
        return Some(format!("http://{rest}"));
    }
    if let Some(rest) = href.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if href.starts_with('/') {
        return Some(format!("{origin}{href}"));
    }
    Some(format!("{origin}/{href}"))
}

/// 落盘：内容哈希命名（同图标天然去重，已存在即复用），返回绝对路径
fn save_icon(bytes: &[u8], ext: &str) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    let dir = crate::paths::data_root().join("icons");
    std::fs::create_dir_all(&dir).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let path = dir.join(format!("fav-{:016x}.{ext}", hasher.finish()));
    if !path.exists() {
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path.to_string_lossy().into_owned())
}

/// 抓取并落盘一个图标/页面（读 body 时封顶：超限按失败处理，不落半截文件）
async fn fetch_bytes_capped(
    client: &reqwest::Client,
    url: &str,
    cap: usize,
) -> Option<(Vec<u8>, String)> {
    let mut resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    while let Some(chunk) = resp.chunk().await.ok()? {
        if buf.len() + chunk.len() > cap {
            return None;
        }
        buf.extend_from_slice(&chunk);
    }
    if buf.is_empty() {
        return None;
    }
    Some((buf, ct))
}

/// 单域名抓取：/favicon.ico → 失败再首页 <link rel=icon>
async fn fetch_origin_favicon(client: &reqwest::Client, origin: &str) -> Option<String> {
    if let Some((bytes, ct)) =
        fetch_bytes_capped(client, &format!("{origin}/favicon.ico"), MAX_ICON_BYTES).await
    {
        if let Some(path) = sniff_image_ext(&bytes, &ct).and_then(|ext| save_icon(&bytes, ext)) {
            return Some(path);
        }
    }
    if let Some((html, _)) = fetch_bytes_capped(client, &format!("{origin}/"), MAX_HTML_BYTES).await
    {
        let text = String::from_utf8_lossy(&html);
        if let Some(url) = find_icon_href(&text).and_then(|href| resolve_href(origin, &href)) {
            if let Some((bytes, ct)) = fetch_bytes_capped(client, &url, MAX_ICON_BYTES).await {
                if let Some(path) = sniff_image_ext(&bytes, &ct).and_then(|ext| save_icon(&bytes, ext)) {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// 批量抓取：返回「原样 target → 图标绝对路径（抓不到为 None）」。同域名只抓一次；
/// 整体永不失败（客户端构建失败也回全 None），调用方无需 try。
pub async fn fetch_favicons(targets: Vec<String>) -> HashMap<String, Option<String>> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(PER_REQUEST_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[favicon] HTTP 客户端初始化失败: {e}");
            return targets.into_iter().map(|t| (t, None)).collect();
        }
    };

    let mut origins: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut target_origin: Vec<(String, Option<String>)> = Vec::with_capacity(targets.len());
    for t in targets {
        let o = origin_of(&t);
        if let Some(o) = &o {
            if seen.insert(o.clone()) {
                origins.push(o.clone());
            }
        }
        target_origin.push((t, o));
    }

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let futures = origins.into_iter().map(|origin| {
        let client = client.clone();
        let sem = sem.clone();
        async move {
            let _permit = sem.acquire().await;
            let icon = fetch_origin_favicon(&client, &origin).await;
            if icon.is_none() {
                log::debug!("[favicon] 未取到: {origin}");
            }
            (origin, icon)
        }
    });
    let fetched: HashMap<String, Option<String>> =
        futures_util::future::join_all(futures).await.into_iter().collect();

    let mut out = HashMap::with_capacity(target_origin.len());
    for (t, o) in target_origin {
        let icon = o.and_then(|o| fetched.get(&o).cloned().flatten());
        out.insert(t, icon);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_extracts_and_normalizes() {
        assert_eq!(
            origin_of("https://GitHub.com/a/b?x=1#f").as_deref(),
            Some("https://github.com")
        );
        assert_eq!(
            origin_of("HTTP://User:Pass@Example.com:8080/").as_deref(),
            Some("http://example.com:8080")
        );
        assert_eq!(origin_of("javascript:void(0)"), None);
        assert_eq!(origin_of("ftp://a.com/x"), None);
        assert_eq!(origin_of("not-a-url"), None);
    }

    #[test]
    fn sniff_by_magic_over_content_type() {
        assert_eq!(sniff_image_ext(&[0x89, b'P', b'N', b'G', 1, 2], "image/x-icon"), Some("png"));
        assert_eq!(sniff_image_ext(&[0, 0, 1, 0, 4, 0], ""), Some("ico"));
        assert_eq!(sniff_image_ext(&[0xFF, 0xD8, 0xFF, 0xE0], "text/html"), Some("jpg"));
        assert_eq!(sniff_image_ext(b"RIFF\x00\x00\x00\x00WEBPVP8", ""), Some("webp"));
        assert_eq!(sniff_image_ext(b"GIF89a......", ""), Some("gif"));
        assert_eq!(sniff_image_ext(b"BM\x00\x00", ""), Some("bmp"));
        // HTML 页面（404 兜底页）绝不当图标
        assert_eq!(sniff_image_ext(b"<html><head></head>", "image/vnd.microsoft.icon"), None);
        // SVG 开头 / 长 xml 前缀靠 Content-Type 兜底
        assert_eq!(sniff_image_ext(b"<svg xmlns=\"...\"></svg>", ""), Some("svg"));
        assert_eq!(
            sniff_image_ext(b"<?xml version=\"1.0\"?>\n<!-- long comment -->", "image/svg+xml"),
            Some("svg")
        );
        assert_eq!(sniff_image_ext(b"", "image/png"), None);
    }

    #[test]
    fn link_scan_finds_icon_href() {
        let html = r#"<head><LINK REL="shortcut icon" HREF="/static/Fav.Icon.png"><link rel="stylesheet" href="a.css"></head>"#;
        assert_eq!(find_icon_href(html).as_deref(), Some("/static/Fav.Icon.png"));
        // 属性顺序颠倒 + 单引号 + apple-touch-icon
        let html2 = r#"<link href='/apple-touch-icon.png' rel='apple-touch-icon' sizes='180x180'>"#;
        assert_eq!(find_icon_href(html2).as_deref(), Some("/apple-touch-icon.png"));
        // 未加引号的 href
        assert_eq!(find_icon_href(r#"<link rel=icon href=/f.ico>"#).as_deref(), Some("/f.ico"));
        // 无 icon 声明
        assert_eq!(find_icon_href(r#"<head><link rel="stylesheet" href="x.css"></head>"#), None);
        // data-href 不误命中
        assert_eq!(find_icon_href(r#"<link rel="icon" data-href="nope">"#), None);
    }

    #[test]
    fn href_resolution() {
        let o = "https://a.com";
        assert_eq!(resolve_href(o, "https://b.com/f.ico").as_deref(), Some("https://b.com/f.ico"));
        assert_eq!(resolve_href(o, "//cdn.a.com/f.ico").as_deref(), Some("https://cdn.a.com/f.ico"));
        assert_eq!(resolve_href(o, "/f.ico").as_deref(), Some("https://a.com/f.ico"));
        assert_eq!(resolve_href(o, "f.ico").as_deref(), Some("https://a.com/f.ico"));
        assert_eq!(resolve_href(o, "data:image/png;base64,xxx"), None);
        assert_eq!(resolve_href(o, "  "), None);
    }
}
