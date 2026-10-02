//! 扩展发布（客户端侧）：把开发中的扩展打包上传到平台，并查看自己的提交。
//!
//! 流程：`pack_dir_to_archive`（与本地打包同一实现，保证产物结构一致）→ multipart 上传 →
//! 服务端跑关卡 → 返回逐项结论（未通过时前端直接展示"哪里不合格"）。
//!
//! 两条纪律：
//! - **打包在临时目录进行**，绝不往开发者的源码目录里写任何东西（见 ADR 0005）。
//! - 客户端**不内置任何审核规则**，只展示服务端返回的结论（PRD 附录 A 红线）。

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 关卡单项结论（与 `admin-web` 的展示口径一致：给人看的 label/detail）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateItem {
    pub id: String,
    pub label: String,
    pub ok: bool,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    /// 服务端的提交 id
    pub id: i64,
    /// pending_review / gate_failed
    pub status: String,
    pub gate_passed: bool,
    pub gate_items: Vec<GateItem>,
    pub ext_id: String,
    pub version: String,
    /// 剩余配额（服务端只回剩余次数，不回上限）
    pub quota: Option<Value>,
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 扩展 id 里的点号等字符不能进临时文件名
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

/// 打包某个扩展（已装或开发中）到临时文件，返回 (路径, id, version)
fn pack_to_temp(app: &tauri::AppHandle, id: &str) -> Result<(std::path::PathBuf, String, String), String> {
    let dir = crate::ext_protocol::resolve_ext_dir(app, id)?;
    let out = std::env::temp_dir().join(format!("xhpack-{}-{}.xhpack", sanitize(id), nanos()));
    let (pack_id, version, _size, _sha) = crate::market::pack_dir_to_archive(&dir, &out)?;
    Ok((out, pack_id, version))
}

/// 发布版本号校验：`x.y.z` 三段纯数字（与服务端关卡同口径，`1.0.0-beta` / `1.2` 这类不合法），
/// 且须能被 semver 解析（拒绝 `01.2.3` 这类前导零，保证 manifest 里始终是规范 semver，
/// 后续 version_cmp 走 semver 路径而不是数字回退）。
fn validate_new_version(v: &str) -> Result<(), String> {
    let parts: Vec<&str> = v.split('.').collect();
    let shape_ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !shape_ok || semver::Version::parse(v).is_err() {
        return Err(format!("版本号必须是 x.y.z 三段纯数字（如 0.2.1），当前填的是「{v}」"));
    }
    Ok(())
}

/// 把新版本号写回扩展目录的 manifest.json（发布弹窗「发布版本」的落盘动作），返回写回前的旧版本。
///
/// - 只改顶层 `version` 字段：serde_json 开了 `preserve_order`（见 Cargo.toml），键序保持原样，
///   开发者眼里的 diff 只有版本那一行；其余字段（含嵌套对象里碰巧也叫 version 的键）一律不动。
/// - 必须严格大于当前版本（`version_cmp`，semver 语义）——服务端关卡要求版本递增。
/// - 原子落盘：先写 `.` 开头的临时文件再改名覆盖——`pack_dir_to_archive` 排除 `.` 开头项、
///   `dev_extensions_stamp` 的目录树哈希也跳过它们，写一半崩溃既不会把半截文件打进包，也不触发热重载。
pub fn bump_manifest_in_dir(dir: &Path, new_version: &str) -> Result<String, String> {
    validate_new_version(new_version)?;
    let current = crate::extension::read_manifest(dir)?.version;
    if crate::market::version_cmp(new_version, &current) != Ordering::Greater {
        return Err(format!("新版本 {new_version} 必须大于当前 manifest 版本 {current}"));
    }
    let path = dir.join("manifest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("IO_ERROR: 读取 manifest.json 失败 {e}"))?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|e| format!("manifest 解析失败：{e}"))?;
    match value.get_mut("version") {
        Some(Value::String(s)) => *s = new_version.to_string(),
        _ => return Err("manifest.json 缺少 version 字符串字段".into()),
    }
    let mut out = serde_json::to_string_pretty(&value).map_err(|e| format!("manifest 序列化失败 {e}"))?;
    if raw.ends_with('\n') {
        out.push('\n');
    }
    let tmp = dir.join(".manifest.json.bump-tmp");
    std::fs::write(&tmp, out).map_err(|e| format!("IO_ERROR: 写入 manifest 失败 {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("IO_ERROR: 替换 manifest.json 失败 {e}"))?;
    Ok(current)
}

/// 把作者署名写进扩展目录 manifest.json 的顶层 `author` 字段（发布弹窗「作者署名」的落盘动作）。
///
/// - 值与当前相同则不动文件（返回 `None`）；发生了回写返回 `Some(旧值)`（旧值可为空串 = 原本没填）。
/// - 与 `bump_manifest_in_dir` 同一条「Value 改字段 + `.` 开头临时文件原子替换」路径：键序保持原样，
///   其余字段一律不动；临时文件既不会被打进包，也不触发热重载。
pub fn set_author_in_dir(dir: &Path, author: &str) -> Result<Option<String>, String> {
    let path = dir.join("manifest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("IO_ERROR: 读取 manifest.json 失败 {e}"))?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|e| format!("manifest 解析失败：{e}"))?;
    let old = value
        .get("author")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if old == author {
        return Ok(None);
    }
    value["author"] = Value::String(author.to_string());
    let mut out = serde_json::to_string_pretty(&value).map_err(|e| format!("manifest 序列化失败 {e}"))?;
    if raw.ends_with('\n') {
        out.push('\n');
    }
    let tmp = dir.join(".manifest.json.author-tmp");
    std::fs::write(&tmp, out).map_err(|e| format!("IO_ERROR: 写入 manifest 失败 {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("IO_ERROR: 替换 manifest.json 失败 {e}"))?;
    Ok(Some(old))
}

/// 上传打包产物：multipart（package 文件 + 展示字段），返回服务端结论
async fn upload(
    base: &str,
    token: &str,
    pkg: &Path,
    changelog: &str,
    min_app_version: &str,
    homepage: &str,
    screenshots: &[String],
) -> Result<Value, String> {
    let bytes = std::fs::read(pkg).map_err(|e| format!("IO_ERROR: 读取安装包失败 {e}"))?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name("ext.xhpack")
        .mime_str("application/zip")
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new().part("package", part);
    if !changelog.is_empty() {
        form = form.text("changelog", changelog.to_string());
    }
    if !min_app_version.is_empty() {
        form = form.text("min_app_version", min_app_version.to_string());
    }
    if !homepage.is_empty() {
        form = form.text("homepage", homepage.to_string());
    }
    // 截图：字段名必须是 `screenshots[]` —— 服务端 Hono 的 parseBody **默认只保留同名键的最后一个值**，
    // 只有 `key[]` 形态才会被收集成数组（实测踩过：不带 [] 时传 6 张只到 1 张，且静默通过）。
    for path in screenshots {
        let p = Path::new(path);
        let shot = std::fs::read(p).map_err(|e| format!("IO_ERROR: 读取截图失败 {}：{e}", p.display()))?;
        let mime = match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        };
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot")
            .to_string();
        let part = reqwest::multipart::Part::bytes(shot)
            .file_name(name)
            .mime_str(mime)
            .map_err(|e| e.to_string())?;
        form = form.part("screenshots[]", part);
    }

    // 上传可能较慢（几 MB 包），给足超时；目标是平台服务端（国内）→ 强制直连（见 crate::net）
    let client = crate::net::direct()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}{}", base.trim_end_matches('/'), crate::api_spec::submit_extension_path()))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("NETWORK_ERROR: 上传失败 {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::account::api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

/// 发布当前开发中的扩展（可选先回写新版本号 → 打包 → 上传 → 返回关卡结论）
#[tauri::command]
pub async fn dev_submit(
    app: tauri::AppHandle,
    id: String,
    changelog: Option<String>,
    min_app_version: Option<String>,
    homepage: Option<String>,
    screenshots: Option<Vec<String>>,
    // 发布弹窗「发布版本」：非空时先把该版本写回 manifest.json 再打包（须大于当前版本）——
    // 包内 manifest 带上新版本号，服务端的版本递增关卡才认。空 = 按 manifest 当前版本发布。
    new_version: Option<String>,
    // 发布弹窗「作者署名」：非空且与 manifest 当前值不同时回写 `author` 字段再打包——
    // 服务端发布时从包内 manifest 读署名写进市场清单，市场卡片与详情页展示的就是它。
    // 空 = 不动 manifest（保留现有署名）。
    author: Option<String>,
) -> Result<SubmitResult, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先在「设置 → 账号」登录")?;
    let base = crate::account::base_url();

    // 先落盘再打包：打包读的就是 manifest.json，顺序反了包里还是旧版本。
    // 与当前版本相同 = 原号重发（撤回 / 被驳回 / 关卡未过 / 已撤销后的重新发布），跳过回写直接打包；
    // 低于当前版本仍报错（填错的兜底，弹窗侧已先行校验）。
    if let Some(v) = new_version.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let dir = crate::ext_protocol::resolve_ext_dir(&app, &id)?;
        let current = crate::extension::read_manifest(&dir)?.version;
        if crate::market::version_cmp(v, &current) == Ordering::Equal {
            log::info!("发布版本与 manifest 当前版本一致（{v}），跳过回写");
        } else {
            let old = bump_manifest_in_dir(&dir, v)?;
            log::info!("发布版本回写: {id} {old} -> {v}");
        }
    }

    // 作者署名与发布版本同一条「先落盘再打包」顺序：打包读的就是 manifest.json。
    if let Some(a) = author.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let dir = crate::ext_protocol::resolve_ext_dir(&app, &id)?;
        if let Some(old) = set_author_in_dir(&dir, a)? {
            log::info!("作者署名回写: {id}「{old}」->「{a}」");
        }
    }

    let (pkg, pack_id, version) = pack_to_temp(&app, &id)?;
    log::info!("发布打包完成: {pack_id} v{version} -> {}", pkg.display());

    let shots = screenshots.unwrap_or_default();
    let result = upload(
        &base,
        &token,
        &pkg,
        changelog.unwrap_or_default().trim(),
        min_app_version.unwrap_or_default().trim(),
        homepage.unwrap_or_default().trim(),
        &shots,
    )
    .await;
    // 无论成败都清掉临时包（源码目录从不被写入）
    let _ = std::fs::remove_file(&pkg);
    let v = result?;

    let gate = v.get("gate").cloned().unwrap_or(Value::Null);
    let items: Vec<GateItem> = gate
        .get("items")
        .and_then(|x| serde_json::from_value(x.clone()).ok())
        .unwrap_or_default();
    let gate_passed = gate.get("passed").and_then(|x| x.as_bool()).unwrap_or(false);
    let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
    let failed = items.iter().filter(|i| !i.ok).count();
    log::info!(
        "发布提交完成: {pack_id} v{version} status={status} 关卡={} 未过项={failed}",
        if gate_passed { "通过" } else { "未通过" }
    );

    Ok(SubmitResult {
        id: v.get("id").and_then(|x| x.as_i64()).unwrap_or(0),
        status,
        gate_passed,
        gate_items: items,
        ext_id: pack_id,
        version,
        quota: v.get("quota").cloned(),
    })
}

/// 读取本地图片为 data URL —— **只用于发布弹窗的截图缩略图预览**。
///
/// 作者选中的图片在任意目录，不在资产协议白名单里（见约定 44），`convertFileSrc` 会被拒；
/// 所以在这里读成 base64 回传。**限制大小与类型**，别让它变成任意文件读取通道。
#[tauri::command]
pub fn read_image_data_url(path: String) -> Result<String, String> {
    const MAX_BYTES: u64 = 2 * 1024 * 1024;
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| format!("IO_ERROR: 读取失败 {e}"))?;
    if !meta.is_file() {
        return Err("INVALID_ARGUMENT: 不是文件".into());
    }
    if meta.len() > MAX_BYTES {
        return Err("TOO_LARGE: 图片超过 2 MB".into());
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return Err("INVALID_ARGUMENT: 仅支持 PNG / JPG / WebP".into()),
    };
    let bytes = std::fs::read(p).map_err(|e| format!("IO_ERROR: 读取失败 {e}"))?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// 按文件头嗅探图片类型（与发布弹窗的 png/jpg/webp 白名单一致；服务端截图关卡同样按文件头判，
/// 本地先挡一道，清单 URL 失效或被换成非图片时不必白传一次再被打回）
fn sniff_image_ext(bytes: &[u8]) -> Option<&'static str> {
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.starts_with(&PNG_MAGIC) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// 分块读取响应体并限长（清单 URL 由市场侧下发，content-length 缺失或虚报时不至把内存读爆）
async fn download_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length() {
        if len as usize > cap {
            return Err(format!("TOO_LARGE: 图片超过 {} MB", cap / 1024 / 1024));
        }
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(resp.content_length().unwrap_or(0).min(8 * 1024 * 1024) as usize);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("NETWORK_ERROR: 下载失败 {e}"))?
    {
        if bytes.len() + chunk.len() > cap {
            return Err(format!("TOO_LARGE: 图片超过 {} MB", cap / 1024 / 1024));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// 「引用上一版截图」：把市场清单里该扩展当前已上架版本的截图 URL 下载成本地临时文件，
/// 返回路径列表；前端把它们当手选图片一样塞进发布表单，后续上传走既有本地路径链路——
/// **服务端契约零改动**（截图本就按内容哈希存储，同图重传不占新空间）。
///
/// 防线：仅接受 http(s)；单张 ≤ 2MB（与发布弹窗同限）；按文件头嗅探类型；
/// 临时目录由本命令独占、每次调用先清空，引用残留不会无限堆积（不含用户手选的文件，不能拿到处清理）。
#[tauri::command]
pub async fn fetch_remote_screenshots(urls: Vec<String>) -> Result<Vec<String>, String> {
    const MAX_BYTES: usize = 2 * 1024 * 1024;
    if urls.is_empty() {
        return Ok(Vec::new());
    }

    let dir = std::env::temp_dir().join("x-hub-pub-shots");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("IO_ERROR: 创建临时目录失败 {e}"))?;

    let client = crate::net::direct()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let mut saved: Vec<String> = Vec::new();
    for url in &urls {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            log::warn!("引用截图跳过非 http(s) 地址: {url}");
            continue;
        }
        let resp = match client.get(url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                log::warn!("引用截图下载失败 {url}: HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                log::warn!("引用截图下载失败 {url}: {e}");
                continue;
            }
        };
        let bytes = match download_capped(resp, MAX_BYTES).await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("引用截图下载失败 {url}: {e}");
                continue;
            }
        };
        let Some(ext) = sniff_image_ext(&bytes) else {
            log::warn!("引用截图跳过非图片内容: {url}");
            continue;
        };
        // 内容哈希命名：同图天然去重（与市场截图桶的存储口径一致）
        let name = format!("{}.{}", &crate::market::to_hex(&Sha256::digest(&bytes))[..16], ext);
        if saved.iter().any(|p| p.ends_with(&name)) {
            continue;
        }
        let path = dir.join(&name);
        if let Err(e) = std::fs::write(&path, &bytes) {
            log::warn!("引用截图落盘失败 {name}: {e}");
            continue;
        }
        saved.push(path.to_string_lossy().to_string());
    }

    if saved.is_empty() {
        return Err("NETWORK_ERROR: 上一版截图全部下载失败（可能已失效），请手动选择图片".into());
    }
    Ok(saved)
}

/// 我的提交列表（含剩余配额：服务端只回剩余次数）
#[tauri::command]
pub async fn dev_list_submissions(page: Option<u32>, page_size: Option<u32>) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let query = format!(
        "?page={}&page_size={}",
        page.unwrap_or(1).max(1),
        page_size.unwrap_or(20).clamp(1, 200)
    );
    crate::account::get_json(
        &format!("{}{query}", crate::api_spec::my_submissions_path()),
        &token,
    )
    .await
}

/// 单个提交详情（含关卡逐项结论；**不含** AI 预审报告——那是给审核者看的）
#[tauri::command]
pub async fn dev_get_submission(id: i64) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    crate::account::get_json(&crate::api_spec::submission_detail_path(id), &token).await
}

/// 撤回自己的提交（审核结束前可用）
#[tauri::command]
pub async fn dev_withdraw_submission(id: i64) -> Result<Value, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let base = crate::account::base_url();
    let (url, body) = crate::api_spec::withdraw_submission(&base, id);
    crate::account::post_json(&url, Some(&token), body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Duration;

    fn header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }

    fn content_length(head: &str) -> usize {
        head.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.eq_ignore_ascii_case("content-length") {
                    v.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    /// 极简 mock 服务端：接一次请求，把「请求行 + 头 + body」原样回传，再返回固定 JSON。
    /// 用它是为了验证**客户端发出去的契约**（路径、鉴权头、multipart 字段名），
    /// 这是只有联调才会暴露、而单测最容易漏掉的一层。
    fn spawn_mock(resp_body: &'static str) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 16384];
                loop {
                    let n = stream.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = header_end(&buf) {
                        let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                        let need = pos + 4 + content_length(&head);
                        while buf.len() < need {
                            let n = stream.read(&mut tmp).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        break;
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn upload_sends_the_contract_the_server_expects() {
        let (base, rx) = spawn_mock(
            r#"{"id":7,"status":"pending_review","gate":{"passed":true,"items":[]},"quota":{"drafts_remaining":3}}"#,
        );
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("com.example.x-1.0.0.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04 not-a-real-zip-but-fine-for-contract").unwrap();

        let v = upload(
            &base,
            "tok-test",
            &pkg,
            "首版说明",
            "0.5.5",
            "https://example.com/x",
            &[],
        )
        .await
        .expect("上传应当成功");

        assert_eq!(v.get("id").and_then(|x| x.as_i64()), Some(7));
        assert_eq!(
            v.get("status").and_then(|x| x.as_str()),
            Some("pending_review")
        );

        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert!(
            raw.starts_with("POST /api/v1/dev/submissions "),
            "路径必须与服务端路由一致：{raw}"
        );
        // HTTP 头名大小写不敏感（hyper 实际发出的是小写），这里统一按小写比对
        let lower = raw.to_lowercase();
        assert!(
            lower.contains("authorization: bearer tok-test"),
            "必须带 Bearer 鉴权头：{raw}"
        );
        // 字段名是服务端 parseBody 读取的键，拼错就整单失败
        for field in ["package", "changelog", "min_app_version", "homepage"] {
            assert!(
                raw.contains(&format!("name=\"{field}\"")),
                "缺少 multipart 字段 {field}：{raw}"
            );
        }
        assert!(raw.contains("filename=\"ext.xhpack\""), "包文件名不符");
        assert!(
            raw.contains("application/zip"),
            "包的 Content-Type 应为 application/zip"
        );
        assert!(raw.contains("首版说明"), "更新说明未随请求发出");
        assert!(raw.contains("0.5.5"), "宿主最低版本未随请求发出");
    }

    #[tokio::test]
    async fn upload_omits_empty_optional_fields() {
        let (base, rx) = spawn_mock(r#"{"id":1,"status":"gate_failed","gate":{"passed":false,"items":[]}}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();

        upload(&base, "t", &pkg, "", "", "", &[]).await.expect("上传应当成功");
        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert!(raw.contains("name=\"package\""));
        for field in ["changelog", "min_app_version", "homepage"] {
            assert!(
                !raw.contains(&format!("name=\"{field}\"")),
                "空的可选字段不应发送：{field}"
            );
        }
    }

    #[tokio::test]
    async fn upload_surfaces_server_error_code() {
        let (base, _rx) = spawn_mock(r#"{"error":"draft_quota_exceeded","message":"待处理的提交过多"}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();

        // mock 一律返回 200，这里只验「错误码能被解析出来」的路径；
        // 4xx 分支由服务端 smoke 覆盖（服务端返回的是同样的 JSON 形状）
        let v = upload(&base, "t", &pkg, "", "", "", &[]).await.expect("200 时应当成功解析");
        assert_eq!(
            v.get("error").and_then(|x| x.as_str()),
            Some("draft_quota_exceeded")
        );
    }

    /// 截图的字段名必须是 `screenshots[]`。
    /// 服务端 Hono 的 `parseBody()` 默认只保留**同名键的最后一个值**，只有 `key[]` 形态会被收集成数组 ——
    /// 不带 [] 时多张截图只到 1 张，而且**服务端不会报错**（实测踩过）。
    #[tokio::test]
    async fn upload_sends_screenshots_with_array_field_name() {
        let (base, rx) = spawn_mock(r#"{"id":9,"status":"pending_review","gate":{"passed":true,"items":[]}}"#);
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("p.xhpack");
        std::fs::write(&pkg, b"PK\x03\x04x").unwrap();
        let shot1 = dir.path().join("shot1.png");
        let shot2 = dir.path().join("shot2.png");
        // 这里只验字段名与 Content-Type；图片类型由服务端按文件头判定
        std::fs::write(&shot1, b"\x89PNG\r\n\x1a\nnot-a-real-png").unwrap();
        std::fs::write(&shot2, b"\x89PNG\r\n\x1a\nnot-a-real-png").unwrap();

        upload(
            &base,
            "t",
            &pkg,
            "",
            "",
            "",
            &[
                shot1.to_string_lossy().into_owned(),
                shot2.to_string_lossy().into_owned(),
            ],
        )
        .await
        .expect("上传应当成功");

        let raw = rx.recv_timeout(Duration::from_secs(5)).expect("mock 未收到请求");
        assert_eq!(
            raw.matches("name=\"screenshots[]\"").count(),
            2,
            "两张截图应各发一个 screenshots[] part：{raw}"
        );
        assert!(
            !raw.contains("name=\"screenshots\""),
            "不要用不带 [] 的字段名（服务端只会保留最后一个）：{raw}"
        );
        assert!(raw.contains("image/png"), "截图 Content-Type 应为 image/png");
        assert!(
            raw.contains("filename=\"shot1.png\"") && raw.contains("filename=\"shot2.png\""),
            "截图文件名应随请求发出"
        );
    }

    /// 最小 manifest：键序故意非字母序，且嵌套对象里放一个同名 version 键——
    /// 回写只许动顶层 version，其余字段与键序必须原样保留。
    fn write_bump_fixture(dir: &Path, version: &str) {
        let raw = format!(
            r#"{{
  "name": "示例扩展",
  "version": "{version}",
  "id": "com.example.dev",
  "config": {{
    "version": "internal-marker"
  }}
}}
"#
        );
        std::fs::write(dir.join("manifest.json"), raw).unwrap();
    }

    #[test]
    fn bump_writes_new_version_preserving_order_and_nested_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_bump_fixture(dir.path(), "0.1.0");

        let old = bump_manifest_in_dir(dir.path(), "0.2.0").unwrap();
        assert_eq!(old, "0.1.0");

        let raw = std::fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["version"], "0.2.0");
        // 嵌套对象里碰巧同名的键不能被误改
        assert_eq!(v["config"]["version"], "internal-marker");
        // 键序保持原样（serde_json preserve_order），开发者的 diff 只有 version 一行
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["name", "version", "id", "config"]);
        // 末尾换行风格保留；临时文件不残留
        assert!(raw.ends_with('\n'));
        assert!(!dir.path().join(".manifest.json.bump-tmp").exists());
        // 类型化读取（与服务端关卡同一读取口径）拿到新版本
        let m = crate::extension::read_manifest(dir.path()).unwrap();
        assert_eq!(m.version, "0.2.0");
    }

    #[test]
    fn bump_rejects_non_greater_or_malformed_versions() {
        let dir = tempfile::tempdir().unwrap();
        write_bump_fixture(dir.path(), "0.2.0");

        // 相同 / 更小 / 非法格式（非 x.y.z、prerelease、缺段、前导零、带 v 前缀、空串）
        for bad in ["0.2.0", "0.1.9", "1.0.0-beta", "1.2", "v1.0.1", "01.2.4", ""] {
            let err = bump_manifest_in_dir(dir.path(), bad).unwrap_err();
            assert!(!err.is_empty(), "应拒绝「{bad}」");
        }
        // 拒绝后 manifest 原样未动
        let m = crate::extension::read_manifest(dir.path()).unwrap();
        assert_eq!(m.version, "0.2.0");
    }

    #[test]
    fn bump_rejects_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert!(bump_manifest_in_dir(dir.path(), "1.0.0").is_err());
    }

    #[test]
    fn author_write_back_sets_field_and_preserves_rest() {
        let dir = tempfile::tempdir().unwrap();
        write_bump_fixture(dir.path(), "0.1.0");

        // 原本没填 author：发生了回写（旧值为空串），字段落盘
        let old = set_author_in_dir(dir.path(), "张三").unwrap();
        assert_eq!(old, Some(String::new()));

        let raw = std::fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["author"], "张三");
        // 其余字段与键序原样保留，author 追加在末尾（开发者的 diff 只有新增的一行）
        assert_eq!(v["config"]["version"], "internal-marker");
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["name", "version", "id", "config", "author"]);
        assert!(raw.ends_with('\n'));
        assert!(!dir.path().join(".manifest.json.author-tmp").exists());

        // 值相同：不动文件
        assert_eq!(set_author_in_dir(dir.path(), "张三").unwrap(), None);
        // 改署名：返回旧值
        assert_eq!(set_author_in_dir(dir.path(), "李四").unwrap(), Some("张三".into()));
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("manifest.json")).unwrap()).unwrap();
        assert_eq!(v["author"], "李四");

        // manifest 缺失：报错而不是静默成功
        let empty = tempfile::tempdir().unwrap();
        assert!(set_author_in_dir(empty.path(), "张三").is_err());
    }

    #[test]
    fn sniff_image_ext_by_magic_bytes() {
        // PNG：8 字节签名
        assert_eq!(sniff_image_ext(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]), Some("png"));
        // JPEG：FF D8 FF
        assert_eq!(sniff_image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        // WebP：RIFF....WEBP（长度不足 12 字节不认）
        assert_eq!(
            sniff_image_ext(b"RIFF\x24\x00\x00\x00WEBPVP8 "),
            Some("webp")
        );
        assert_eq!(sniff_image_ext(b"RIFF\x24\x00WEBP"), None);
        // 非图片 / 空内容：拒（清单 URL 失效被换成 HTML 错误页的场景）
        assert_eq!(sniff_image_ext(b"<!DOCTYPE html>"), None);
        assert_eq!(sniff_image_ext(&[]), None);
    }
}
