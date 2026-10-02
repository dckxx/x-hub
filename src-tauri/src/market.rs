//! 扩展市场（spec §11 安装页「市场」tab）。
//!
//! 数据源为**远端市场清单**：内置地址 `config::market_registry_url()`
//! （`https://x-hub.xfactor.top/api/v1/market/registry`，v0.6.1 起走平台服务端接口——
//! 服务端代理 COS 并把清单里的下载/图标地址改写成服务端地址后再用发布私钥重签；
//! 此前是配置项 `market_endpoint`，直连 COS 桶，已废弃）。客户端拉取清单后做
//! Ed25519 验签（`signing` 模块），通过才原子缓存到 `data_root()/market/registry.json`；
//! 离线 / 验签失败时回退本地缓存，市场仍可浏览（带警示）。
//!
//! 安装流程：下载 `downloadUrl`（zip 包）→ 边下边算 sha256（与清单比对，防篡改）
//! → 解包 → 定位 manifest.json 所在目录 → 复制到 `extensions/<id>/`。
//! 下载进度以 `market-download-progress` 事件广播给前端（进度条）。

use crate::extension::{copy_dir_recursive, extensions_root, read_manifest};
use futures_util::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

/// 市场清单里的一条扩展（v2：远端清单格式，向后兼容原字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub author: String,
    /// 下载地址（zip 包）
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
    /// zip 包 sha256（hex 小写）；空串 = 旧清单未提供（安装时跳过校验并告警）
    #[serde(default)]
    pub sha256: String,
    /// zip 包字节大小（0 = 未知，仅作进度参考）
    #[serde(default)]
    pub size: u64,
    /// 市场卡片图标（https URL，前端 `<img>` 直接加载）
    #[serde(default)]
    pub icon: String,
    /// 宿主最低版本门槛（如 "0.3.0"）
    #[serde(default, rename = "minAppVersion")]
    pub min_app_version: String,
    /// 本版本更新说明
    #[serde(default)]
    pub changelog: String,
    /// 项目主页
    #[serde(default)]
    pub homepage: String,
    /// 官方内置扩展标记
    #[serde(default)]
    pub required: bool,
    /// 该扩展申请的权限（发布时由服务端从 manifest 写入；老清单缺此字段 = 空，
    /// 客户端据此在安装前分级告知，见 PRD §7.2 ②）
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 截图（展示物料，完整 URL；发布时由作者上传、服务端写入清单）。
    /// 老清单没有这个字段 → 空数组 → 详情页显示「作者未提供截图」
    #[serde(default)]
    pub screenshots: Vec<String>,
}

/// 远端清单顶层结构。
#[derive(Debug, Clone, Deserialize, Default)]
struct MarketRegistry {
    #[serde(default, rename = "schemaVersion")]
    schema_version: u32,
    #[serde(default, rename = "updatedAt")]
    updated_at: String,
    #[serde(default)]
    extensions: Vec<MarketExtension>,
    /// 撤销列表（条目形如 `id@version`）：平台下架 / 撤回的版本。
    /// 追加字段、**不抬 schemaVersion** —— 老客户端会忽略它（因此老客户端对下架
    /// 只有"包下载不到"这一层保护），新客户端据此警示已装扩展并停止其自动更新。
    #[serde(default)]
    revoked: Vec<String>,
}

/// 市场状态（get_market_registry / refresh_market_registry 的返回）。
#[derive(Debug, Clone, Serialize)]
pub struct MarketStatus {
    pub extensions: Vec<MarketExtension>,
    /// 清单更新时间（远端 `updatedAt` 透传，空 = 本地缓存无此信息）
    pub last_updated: String,
    /// 数据来源：remote（刚才刷新成功）/ cache（回退本地缓存）
    pub source: String,
    /// 拉取 / 验签失败原因（source=cache 时非空，前端黄色警示）
    pub error: Option<String>,
    /// 撤销列表（`id@version`）：前端用于警示已装扩展并阻止其自动更新
    pub revoked: Vec<String>,
}

/// 下载进度事件负载（`market-download-progress`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub received: u64,
    pub total: Option<u64>,
}

fn status(
    extensions: Vec<MarketExtension>,
    last_updated: String,
    revoked: Vec<String>,
    source: &str,
    error: Option<String>,
) -> MarketStatus {
    MarketStatus {
        extensions,
        last_updated,
        revoked,
        source: source.to_string(),
        error,
    }
}

/// 判断某扩展的某版本是否已被撤销（大小写不敏感，容忍 `id@version` 两侧空白）
pub fn is_revoked(revoked: &[String], id: &str, version: &str) -> bool {
    let key = format!("{}@{}", id.trim(), version.trim());
    revoked
        .iter()
        .any(|r| r.trim().eq_ignore_ascii_case(&key))
}

/// 市场清单缓存路径：`data_root()/market/registry.json`
/// 必须用 `paths::data_root()`（便携版跟随 exe 目录\data），不能用 `app_data_dir()`。
fn registry_path() -> Result<PathBuf, String> {
    Ok(crate::paths::data_root().join("market").join("registry.json"))
}

/// 读取市场清单（同步读本地缓存；不存在或损坏返回空列表 + 提示）。
#[tauri::command]
pub fn get_market_registry() -> Result<MarketStatus, String> {
    let path = registry_path()?;
    if let Ok(content) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<MarketRegistry>(&content) {
            Ok(r) => Ok(status(r.extensions, r.updated_at, r.revoked, "cache", None)),
            Err(_) => Ok(status(
                Vec::new(),
                String::new(),
                Vec::new(),
                "cache",
                Some("本地市场缓存损坏，请尝试刷新".to_string()),
            )),
        }
    } else {
        Ok(status(
            Vec::new(),
            String::new(),
            Vec::new(),
            "cache",
            Some("尚未拉取过市场清单（离线或首次使用），请点击刷新".to_string()),
        ))
    }
}

/// 一次性拉取 URL 内容（字节），非 2xx 视为失败。
async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("通信失败：{e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // 清单/签名都走平台服务端（v0.6.1 起）：4xx/5xx 是 JSON `{error, message}`，
        // 优先带出服务端的可读 message，别只给「HTTP 503」；非 JSON（不会出现）退回状态码
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(msg);
    }
    let bytes = resp.bytes().await.map_err(|e| format!("读取响应失败: {e}"))?;
    Ok(bytes.to_vec())
}

/// 脱敏市场源错误信息：不向用户暴露具体 URL（reqwest 错误串会带 endpoint）
fn sanitize_market_error(mut msg: String, endpoint: &str, sig_url: &str) -> String {
    // 直接整串替换最容易命中；reqwest 错误常形如 "…for url (https://…)"
    for url in [endpoint, sig_url] {
        msg = msg.replace(url, "(市场源地址)");
    }
    msg
}

/// 拉取远端市场清单：fetch 原始字节 + `.sig` → Ed25519 验签 → 校验 schema
/// → 原子落缓存。任何一步失败都回退本地缓存并携带原因（不阻塞浏览）。
#[tauri::command]
pub async fn refresh_market_registry() -> Result<MarketStatus, String> {
    // 地址是内置常量（平台服务端接口，服务端再代理 COS）：v0.6.1 起不再读配置
    let endpoint = crate::config::market_registry_url();
    let sig_url = format!("{endpoint}.sig");
    log::info!("刷新市场清单: {endpoint}");

    // 市场源是平台服务端（国内）：强制直连，别被用户本地代理带沟里（见 crate::net）
    let client = crate::net::direct()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let content = match fetch_bytes(&client, &endpoint).await {
        Ok(c) => c,
        Err(e) => {
            let msg = sanitize_market_error(format!("拉取市场清单失败：{e}"), &endpoint, &sig_url);
            return Ok(fallback_cache(msg));
        }
    };
    // 签名拉取失败也禁止放行：未验签的清单一律不信任（宁可回退缓存）
    let sig = match fetch_bytes(&client, &sig_url).await {
        Ok(s) => s,
        Err(e) => {
            let msg = sanitize_market_error(format!("拉取清单签名失败：{e}"), &endpoint, &sig_url);
            return Ok(fallback_cache(msg));
        }
    };
    let sig = String::from_utf8_lossy(&sig).into_owned();
    if let Err(e) = crate::signing::verify_detached(&content, &sig) {
        return Ok(fallback_cache(format!("市场清单验签失败：{e}")));
    }

    let registry: MarketRegistry = match serde_json::from_slice(&content) {
        Ok(r) => r,
        Err(e) => return Ok(fallback_cache(format!("市场清单解析失败：{e}"))),
    };
    if registry.schema_version > 2 {
        return Ok(fallback_cache(format!(
            "市场清单 schemaVersion={} 高于宿主支持的 v2，请升级 x-hub",
            registry.schema_version
        )));
    }

    // 原子写缓存（临时文件 + rename）
    let path = registry_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &content).map_err(|e| format!("写入市场缓存失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("更新市场缓存失败: {e}"))?;

    log::info!(
        "市场清单刷新成功：{} 个扩展（schema v{}，updatedAt={}）",
        registry.extensions.len(),
        registry.schema_version,
        if registry.updated_at.is_empty() { "无" } else { &registry.updated_at }
    );
    Ok(status(
        registry.extensions,
        registry.updated_at.clone(),
        registry.revoked.clone(),
        "remote",
        None,
    ))
}

/// 回退本地缓存（refresh 失败时的降级路径，详情作为 error 透出）
fn fallback_cache(reason: String) -> MarketStatus {
    match get_market_registry() {
        Ok(mut s) => {
            s.source = "cache".to_string();
            s.error = Some(reason);
            s
        }
        Err(_) => status(Vec::new(), String::new(), Vec::new(), "cache", Some(reason)),
    }
}

/// 解包 zip 到目标目录（enclosed_name 防 zip-slip 路径逃逸）
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    extract_zip_from_reader(reader, dest)
}

/// 从磁盘文件解包 zip 到目标目录（大包无需整体读入内存，专用作更新包解压）。
pub fn extract_zip_read(zip_file: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_file).map_err(|e| format!("打开安装包失败: {e}"))?;
    extract_zip_from_reader(file, dest)
}

fn extract_zip_from_reader<R: std::io::Read + std::io::Seek>(
    reader: R,
    dest: &Path,
) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("解包失败: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| "安装包含非法路径".to_string())?;
        let out = dest.join(&name);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 定位解包目录中 manifest.json 所在目录（根或一层子目录）
fn find_manifest_dir(dir: &Path) -> Result<PathBuf, String> {
    if dir.join("manifest.json").is_file() {
        return Ok(dir.to_path_buf());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        if p.is_dir() && p.join("manifest.json").is_file() {
            return Ok(p);
        }
    }
    Err("安装包中未找到 manifest.json".to_string())
}

/// 生成唯一临时目录名
fn temp_extract_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("xhub-ext-{}-{}", std::process::id(), nanos))
}

/// 字节流转小写 hex（发布截图「引用上一版」也用内容哈希命名，crate 内共用）
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 校验字节流 sha256（expected 为空串则跳过，兼容旧清单）
fn verify_sha256(data: &[u8], expected: &str) -> Result<(), String> {
    if expected.is_empty() {
        return Ok(());
    }
    let actual = to_hex(&Sha256::digest(data));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "下载内容校验失败（sha256 不匹配）\n期望: {expected}\n实际: {actual}\n安装包可能被篡改或损坏，已中止安装。"
        ));
    }
    Ok(())
}

/// 流式下载扩展 zip 包：边下边累计，进度以 `market-download-progress` 事件节流广播（≥256KB 一次）。
async fn download_with_progress(
    app: &tauri::AppHandle,
    ext: &MarketExtension,
) -> Result<Vec<u8>, String> {
    let resp = reqwest::get(&ext.download_url)
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let total = if ext.size > 0 { Some(ext.size) } else { resp.content_length() };
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut received: u64 = 0;
    let mut last_emit: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        received += chunk.len() as u64;
        buf.extend_from_slice(&chunk);
        if received - last_emit >= 262_144 || received == total.unwrap_or(0) {
            last_emit = received;
            let _ = app.emit(
                "market-download-progress",
                DownloadProgress {
                    id: ext.id.clone(),
                    received,
                    total,
                },
            );
        }
    }
    if let Some(t) = total {
        if t != 0 && received != t {
            return Err(format!("下载不完整: 收到 {received} 字节，预期 {t} 字节"));
        }
    }
    Ok(buf)
}

/// 从市场下载并安装扩展：下载 → sha256 校验 → 解包 → 校验 manifest → 复制到 extensions/<id>。
#[tauri::command]
pub async fn install_from_market(
    app: tauri::AppHandle,
    extension: MarketExtension,
) -> Result<String, String> {
    if extension.sha256.is_empty() {
        log::warn!("市场条目 {} 未提供 sha256，本次安装跳过完整性校验", extension.id);
    }
    let bytes = download_with_progress(&app, &extension).await?;
    verify_sha256(&bytes, &extension.sha256)?;

    let tmp = temp_extract_dir();
    extract_zip(&bytes, &tmp)?;
    let manifest_dir = find_manifest_dir(&tmp)?;
    let manifest = read_manifest(&manifest_dir)?;
    let id = manifest.id.clone();

    let root = extensions_root(&app)?;
    let dest = root.join(&id);
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("扩展 {id} 已安装，请先卸载再重装"));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    copy_dir_recursive(&manifest_dir, &dest).map_err(|e| format!("复制扩展失败: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);

    log::info!(
        "扩展已从市场安装: {id} v{} <- {}",
        manifest.version,
        extension.download_url
    );
    Ok(id)
}

/// 从本地压缩包文件（.xhpack，zip 格式；兼容旧 .zip）安装扩展：
/// 读文件 → 解包 → 定位 manifest.json → 复制到 extensions/<id>。
/// 与 install_from_market 共用同一条解包/校验链路，只是数据源换成本地文件。
#[tauri::command]
pub fn install_local_archive(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err(format!("INVALID_ARGUMENT: 安装包文件不存在：{path}"));
    }
    let bytes = std::fs::read(&src).map_err(|e| format!("读取安装包失败: {e}"))?;

    let tmp = temp_extract_dir();
    extract_zip(&bytes, &tmp)?;
    let manifest_dir = find_manifest_dir(&tmp)?;
    let manifest = read_manifest(&manifest_dir)?;
    let id = manifest.id.clone();

    let root = extensions_root(&app)?;
    let dest = root.join(&id);
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("扩展 {id} 已安装，请先卸载再重装"));
    }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    copy_dir_recursive(&manifest_dir, &dest).map_err(|e| format!("复制扩展失败: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);

    log::info!("扩展已从本地包安装: {id} <- {}", src.display());
    Ok(id)
}

/// 打包结果（前端展示与上传用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedArchive {
    /// 产物绝对路径（`.xhpack`，zip 格式）
    pub path: String,
    pub id: String,
    pub version: String,
    pub size: u64,
    /// 产物 sha256（hex 小写）；上传后服务端会重算比对
    pub sha256: String,
}

/// 打包核心（不依赖 app handle，便于单测）：把扩展目录打成 zip 包。
/// 返回 `(id, version, size, sha256)`。
pub fn pack_dir_to_archive(
    dir: &Path,
    out: &Path,
) -> Result<(String, String, u64, String), String> {
    let manifest = crate::extension::read_manifest(dir)?;
    if manifest.version.trim().is_empty() {
        return Err("INVALID_ARGUMENT: manifest 缺少 version".to_string());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // 检查并完成压缩后才写目标，敏感文件检查失败时不留下半包或覆盖已有产物。
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    add_dir_to_zip(&mut zip, dir, dir, options)?;
    let bytes = zip.finish().map_err(|e| format!("写入安装包失败: {e}"))?.into_inner();
    std::fs::write(out, &bytes).map_err(|e| format!("创建安装包失败: {e}"))?;
    let sha256 = to_hex(&Sha256::digest(&bytes));
    Ok((
        manifest.id,
        manifest.version,
        bytes.len() as u64,
        sha256,
    ))
}

/// 把扩展目录打成 `.xhpack`（zip 格式，**manifest.json 必须在包根**）。
/// 已装扩展与开发扩展都可打包（按 id 解析目录，「我的扩展」直挂的源码目录同样适用）。
/// 排除 `node_modules` 与所有 `.` 开头的文件/目录（含 `.xhpack`、`.git`、用户数据点文件）。
#[tauri::command]
pub fn pack_extension_archive(
    app: tauri::AppHandle,
    id: String,
    out_path: Option<String>,
) -> Result<PackedArchive, String> {
    let dir = crate::ext_protocol::resolve_ext_dir(&app, &id)?;
    let out = match out_path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => temp_extract_dir().with_extension("xhpack"),
    };
    let (id, version, size, sha256) = pack_dir_to_archive(&dir, &out)?;
    log::info!(
        "扩展已打包: {id} v{version} -> {}（{size} 字节）",
        out.display()
    );
    Ok(PackedArchive {
        path: out.to_string_lossy().into_owned(),
        id,
        version,
        size,
        sha256,
    })
}

/// 递归把目录写进 zip：包内路径统一用 `/`，跳过 `node_modules` 与 `.` 开头的项
fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let is_link = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let is_link = metadata.file_type().is_symlink();
        if is_link {
            return Err(format!("打包已中止：不允许符号链接或 junction（{name}）"));
        }
        if sensitive_package_file(&name) || (metadata.is_dir() && name.eq_ignore_ascii_case("backups")) {
            return Err(format!("打包已中止：发现可能包含私有数据的文件或目录 {name}，请在干净发布目录中打包"));
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            zip.add_directory(format!("{rel}/"), options)
                .map_err(|e| e.to_string())?;
            add_dir_to_zip(zip, root, &path, options)?;
        } else {
            // 无扩展名的 LICENSE（如包根的 `LICENSE`）在包内改名加 `.txt`：服务端发布关卡按
            // 扩展名白名单校验包内文件，无扩展名一律拒绝；改名而非删除——MIT 等协议要求
            // 分发时保留版权与许可声明，只改 zip 内条目名，不动开发者源码目录。
            let entry = if !name.contains('.') && name.eq_ignore_ascii_case("license") {
                format!("{rel}.txt")
            } else {
                rel
            };
            zip.start_file(entry, options).map_err(|e| e.to_string())?;
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 发布与资源协议共用的敏感文件识别；未命中不代表内容已经过秘密扫描。
pub(crate) fn sensitive_package_file(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let stem = name.split('.').next().unwrap_or("");
    matches!(stem, "credentials" | "secrets" | "secret" | "token" | "tokens" | "chat_keys" | "account_token")
        || [".db", ".db-wal", ".db-shm", ".sqlite", ".sqlite3", ".pem", ".key", ".pfx", ".p12", ".env", ".xhpack"]
            .iter().any(|suffix| name.ends_with(suffix))
}

/// 扩展升级需保留的用户数据点文件（随扩展卸载可清除，升级时必须保留）。
const EXT_DOTFILES: [&str; 4] = [
    ".permissions.json",
    ".config.json",
    ".storage.json",
    ".deploy-config.json",
];

/// 版本比较：优先 semver 语义；非 semver（如 "v1"、"1.0"）回退到逐节数字比较。
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => {
            let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
            let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
            for i in 0..pa.len().max(pb.len()) {
                let x = pa.get(i).copied().unwrap_or(0);
                let y = pb.get(i).copied().unwrap_or(0);
                if x != y {
                    return x.cmp(&y);
                }
            }
            Ordering::Equal
        }
    }
}

/// 原子替换扩展目录：备份旧目录 → 新内容就位 → 从备份恢复用户点文件。
/// 任何一步失败即回滚（删新内容、还原旧目录），成功则清理备份。
/// - `dest`：`extensions/<id>`（已存在）
/// - `content`：新版本内容（已组装在 `extensions/.tmp-update/<id>`）
/// - `backup`：备份目标（`extensions/.backup/<id>-<ts>`）
pub fn replace_extension_dir(dest: &Path, content: &Path, backup: &Path) -> Result<(), String> {
    // 1) 备份旧目录
    if dest.exists() {
        if let Some(p) = backup.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::rename(dest, backup).map_err(|e| format!("备份旧版本失败: {e}"))?;
    }
    // 2) 就位新目录（rename 原子，同盘）
    if let Err(e) = std::fs::rename(content, dest) {
        let _ = std::fs::rename(backup, dest); // 回滚：还原旧目录
        return Err(format!("替换新版本失败: {e}"));
    }
    // 3) 从备份恢复用户点文件到新目录
    for pf in EXT_DOTFILES {
        let sp = backup.join(pf);
        if sp.is_file() {
            if let Err(e) = std::fs::copy(&sp, &dest.join(pf)) {
                // 回滚整棵：删新内容，还原旧目录
                let _ = std::fs::remove_dir_all(dest);
                let _ = std::fs::rename(backup, dest);
                return Err(format!("恢复 {pf} 失败: {e}"));
            }
        }
    }
    // 4) 成功：清理备份
    if backup.exists() {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(())
}

/// 更新市场扩展：下载 → sha256 校验 → 版本比较（新 > 旧、宿主门槛）→ 备份旧
/// → 原子替换 → 恢复用户点文件；失败回滚到旧版本。
/// 完成后 `extensions_stamp` 自然变化，前端轮询即感知。
#[tauri::command]
pub async fn update_extension(
    app: tauri::AppHandle,
    extension: MarketExtension,
) -> Result<String, String> {
    if extension.sha256.is_empty() {
        log::warn!("市场条目 {} 未提供 sha256，本次更新跳过完整性校验", extension.id);
    }
    let bytes = download_with_progress(&app, &extension).await?;
    verify_sha256(&bytes, &extension.sha256)?;

    let tmp = temp_extract_dir();
    extract_zip(&bytes, &tmp)?;
    let manifest_dir = find_manifest_dir(&tmp)?;
    let new_manifest = read_manifest(&manifest_dir)?;
    let id = new_manifest.id.clone();

    let root = extensions_root(&app)?;
    let dest = root.join(&id);
    // 前置校验
    if !dest.is_dir() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("扩展 {id} 尚未安装，无法更新"));
    }
    let old_manifest = read_manifest(&dest)?;
    if old_manifest.id != id {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err("更新包 manifest.id 与已装扩展不一致，已中止".to_string());
    }
    if version_cmp(&new_manifest.version, &old_manifest.version) != Ordering::Greater {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!(
            "新版本 {} 未高于已装版本 {}，无可更新内容",
            new_manifest.version, old_manifest.version
        ));
    }
    if !extension.min_app_version.is_empty() {
        let host = app.package_info().version.to_string();
        if version_cmp(&host, &extension.min_app_version) == Ordering::Less {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!(
                "该更新要求宿主 v{}，当前 v{}",
                extension.min_app_version, host
            ));
        }
    }

    // service 扩展先停后端进程（重开后懒启动），避免旧进程持有文件
    crate::service::stop_service(&app, &id);

    // 组装新内容到扩展树内的隐藏暂存（`.` 开头，scan/stamp 均跳过）
    let hidden_root = root.join(".tmp-update");
    std::fs::create_dir_all(&hidden_root).map_err(|e| e.to_string())?;
    let pre = hidden_root.join(&id);
    if pre.exists() {
        let _ = std::fs::remove_dir_all(&pre);
    }
    copy_dir_recursive(&manifest_dir, &pre).map_err(|e| format!("复制新版本内容失败: {e}"))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = root.join(".backup").join(format!("{id}-{ts}"));
    let result = replace_extension_dir(&dest, &pre, &backup);

    // 清理临时与暂存目录
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&hidden_root);

    result?;
    log::info!(
        "扩展已更新: {id} v{} -> v{}（{src}）",
        old_manifest.version,
        new_manifest.version,
        src = extension.download_url
    );
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_manifest_dir_detects_root_and_subdir() {
        let dir = tempfile::tempdir().unwrap();
        // 根目录有 manifest.json
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        assert_eq!(find_manifest_dir(dir.path()).unwrap(), dir.path());

        // 一层子目录有 manifest.json
        let dir2 = tempfile::tempdir().unwrap();
        let sub = dir2.path().join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("manifest.json"), "{}").unwrap();
        assert_eq!(find_manifest_dir(dir2.path()).unwrap(), sub);

        // 无 manifest.json
        let dir3 = tempfile::tempdir().unwrap();
        assert!(find_manifest_dir(dir3.path()).is_err());
    }

    #[test]
    fn sha256_verifies_and_rejects() {
        let data = b"hello x-hub market";
        let digest = to_hex(&Sha256::digest(data));
        assert!(verify_sha256(data, &digest).is_ok());
        // 大小写不敏感
        assert!(verify_sha256(data, &digest.to_uppercase()).is_ok());
        // 内容被篡改 → 拒绝
        assert!(verify_sha256(b"hello x-hub markeT", &digest).is_err());
        // 空期望 = 跳过校验
        assert!(verify_sha256(data, "").is_ok());
        // 长度不符预期字符串报错
        assert!(verify_sha256(data, "abc").is_err());
    }

    #[test]
    fn parses_v2_registry_with_defaults() {
        let json = serde_json::json!({
            "schemaVersion": 2,
            "updatedAt": "2026-08-26T12:00:00Z",
            "extensions": [{
                "id": "com.x-hub.ctool",
                "name": "C 工具集",
                "version": "1.2.0",
                "downloadUrl": "https://dist/packages/com.x-hub.ctool/1.2.0/com.x-hub.ctool-1.2.0.xhpack",
                "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                "size": 1024,
                "minAppVersion": "0.3.0"
            }]
        });
        let r: MarketRegistry = serde_json::from_value(json).unwrap();
        assert_eq!(r.schema_version, 2);
        assert_eq!(r.extensions.len(), 1);
        let e = &r.extensions[0];
        assert_eq!(e.id, "com.x-hub.ctool");
        assert_eq!(e.sha256, "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
        assert_eq!(e.size, 1024);
        assert_eq!(e.min_app_version, "0.3.0");
        // 未提供字段回落默认
        assert_eq!(e.icon, "");
        assert!(!e.required);
        assert_eq!(e.runtime, "");
    }

    #[test]
    fn parses_registry_with_revoked_and_matches_keys() {
        let json = serde_json::json!({
            "schemaVersion": 2,
            "updatedAt": "2026-09-12T00:00:00Z",
            "revoked": ["com.x-hub.bad@1.0.0", " com.x-hub.other@2.0.0 "],
            "extensions": []
        });
        let r: MarketRegistry = serde_json::from_value(json).unwrap();
        assert_eq!(r.revoked.len(), 2);
        assert!(is_revoked(&r.revoked, "com.x-hub.bad", "1.0.0"));
        // 大小写不敏感 + 容忍条目两侧空白
        assert!(is_revoked(&r.revoked, "COM.X-HUB.BAD", "1.0.0"));
        assert!(is_revoked(&r.revoked, "com.x-hub.other", "2.0.0"));
        // 版本不同不算撤销
        assert!(!is_revoked(&r.revoked, "com.x-hub.bad", "1.0.1"));
        assert!(!is_revoked(&r.revoked, "com.x-hub.good", "1.0.0"));
    }

    #[test]
    fn registry_without_revoked_still_parses() {
        // 追加字段的向后兼容：老清单（无 revoked）必须照常解析
        let json = serde_json::json!({ "extensions": [] });
        let r: MarketRegistry = serde_json::from_value(json).unwrap();
        assert!(r.revoked.is_empty());
    }

    #[test]
    fn pack_archive_keeps_manifest_at_root_and_skips_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let ext = tmp.path().join("ext");
        std::fs::create_dir_all(ext.join("view")).unwrap();
        std::fs::create_dir_all(ext.join("node_modules").join("dep")).unwrap();
        std::fs::create_dir_all(ext.join(".xhpack")).unwrap();
        std::fs::write(
            ext.join("manifest.json"),
            r#"{"id":"com.x-hub.pack","name":"P","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(ext.join("view").join("index.html"), "<html></html>").unwrap();
        std::fs::write(ext.join("node_modules").join("dep").join("a.js"), "x").unwrap();
        std::fs::write(ext.join(".xhpack").join("old.html"), "x").unwrap();
        std::fs::write(ext.join(".storage.json"), "{}").unwrap();

        let out = tmp.path().join("out.xhpack");
        let (id, version, size, sha) = pack_dir_to_archive(&ext, &out).unwrap();
        assert_eq!(id, "com.x-hub.pack");
        assert_eq!(version, "1.0.0");
        assert!(size > 0);
        assert_eq!(sha.len(), 64);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        assert!(names.iter().any(|n| n == "manifest.json"), "{names:?}");
        assert!(names.iter().any(|n| n == "view/index.html"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("node_modules")), "{names:?}");
        assert!(!names.iter().any(|n| n.contains(".xhpack")), "{names:?}");
        assert!(!names.iter().any(|n| n.contains(".storage.json")), "{names:?}");

        // 打包产物必须能被既有安装链路解出 manifest 所在目录
        let unpack = tempfile::tempdir().unwrap();
        extract_zip(&std::fs::read(&out).unwrap(), unpack.path()).unwrap();
        assert!(find_manifest_dir(unpack.path()).is_ok());

        for name in ["credentials.json", "history.sqlite", "private.pem", "a.xhpack"] {
            let previous = std::fs::read(&out).unwrap();
            std::fs::write(ext.join(name), "测试私有数据").unwrap();
            assert!(pack_dir_to_archive(&ext, &out).is_err(), "{name} 不得进入发布包");
            assert_eq!(std::fs::read(&out).unwrap(), previous, "失败不能覆盖已有产物");
            std::fs::remove_file(ext.join(name)).unwrap();
        }

        // 2026-09-23 放宽（dckxx 拍板）：data/ 资产与 .zip/.log 不再阻断打包
        std::fs::create_dir_all(ext.join("data")).unwrap();
        std::fs::write(ext.join("data").join("seed.json"), "[]").unwrap();
        std::fs::write(ext.join("demo.zip"), "x").unwrap();
        std::fs::write(ext.join("debug.log"), "x").unwrap();
        assert!(pack_dir_to_archive(&ext, &out).is_ok(), "data/ 资产与 .zip/.log 应可打包");
    }

    #[test]
    fn pack_archive_renames_extensionless_license_to_txt() {
        // 服务端发布关卡按扩展名白名单拒绝无扩展名文件：包内 `LICENSE` 必须自动改成
        // `LICENSE.txt`（源码目录不动），带扩展名的许可文件（LICENSE.md）不受影响。
        let tmp = tempfile::tempdir().unwrap();
        let ext = tmp.path().join("ext");
        std::fs::create_dir_all(ext.join("docs")).unwrap();
        std::fs::write(
            ext.join("manifest.json"),
            r#"{"id":"com.x-hub.lic","name":"L","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(ext.join("LICENSE"), "MIT License").unwrap();
        std::fs::write(ext.join("docs").join("LICENSE"), "nested MIT").unwrap();
        std::fs::write(ext.join("LICENSE.md"), "# License").unwrap();

        let out = tmp.path().join("out.xhpack");
        pack_dir_to_archive(&ext, &out).unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            names.push(archive.by_index(i).unwrap().name().to_string());
        }
        assert!(names.iter().any(|n| n == "LICENSE.txt"), "{names:?}");
        assert!(names.iter().any(|n| n == "docs/LICENSE.txt"), "{names:?}");
        assert!(names.iter().any(|n| n == "LICENSE.md"), "{names:?}");
        assert!(
            !names.iter().any(|n| n.split('/').last().map_or(false, |f| !f.contains('.') && f.eq_ignore_ascii_case("license"))),
            "包内不得残留无扩展名的 LICENSE：{names:?}"
        );
    }

    #[test]
    fn parses_legacy_v1_registry() {
        // 老格式（无 schemaVersion / sha256 等）仍然可解析
        let json = serde_json::json!({
            "extensions": [{
                "id": "com.x-hub.legacy",
                "name": "老扩展",
                "version": "0.1.0",
                "description": "legacy",
                "runtime": "web",
                "author": "x",
                "downloadUrl": "https://example.com/a.zip"
            }]
        });
        let r: MarketRegistry = serde_json::from_value(json).unwrap();
        assert_eq!(r.schema_version, 0);
        assert_eq!(r.extensions[0].id, "com.x-hub.legacy");
        assert_eq!(r.extensions[0].sha256, "");
    }

    #[test]
    fn version_cmp_semver_and_fallback() {
        // 正规 semver
        assert_eq!(version_cmp("1.2.0", "1.1.9"), Ordering::Greater);
        assert_eq!(version_cmp("0.1.0", "0.1.0"), Ordering::Equal);
        assert_eq!(version_cmp("0.1.0", "0.2.0"), Ordering::Less);
        assert_eq!(version_cmp("0.10.0", "0.9.9"), Ordering::Greater);
        // 非标准 semver 回退逐节数字
        assert_eq!(version_cmp("1.2", "1.10"), Ordering::Less);
        assert_eq!(version_cmp("2", "1.9"), Ordering::Greater);
    }
}
