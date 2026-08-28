//! 应用自动升级（方案 A：自研 update.json + 摘要签名 + rename 自替换）。
//!
//! 升级清单 `releases/update.json` 紧随市场清单之后扩展：发布侧用同一把
//! Ed25519 私钥对原始字节做分离签名（`.sig` 文本文件并列上传），客户端以
//! 内嵌公钥验签（`signing::verify_detached`），通过才信任清单内容——
//! 清单是唯一安全根，下载物（新版本 zip）的 sha256 均由签名清单背书。
//!
//! 流程（对应文档 §6.2）：
//!   ① `check_for_update`：拉取 update.json + .sig → 验签 → semver 比较 +
//!      `minimumUpgradable` 跳级保护 → 平台匹配（便携版优先 portableUrl）→
//!      广播 `update-available`（版本/说明/大小）。
//!   ② `download_update`：按清单下载新版本 zip → 边下边算 sha256（与清单
//!      比对）→ 落 `data_root()/updates/<version>/x-hub.zip` →
//!      写 `data_root()/updates/.pending.json` 标记，广播 `update-ready`。
//!   ③ `apply_pending_update`：每次启动早期调用（幂等）。无标记直接跳过；
//!      有标记 → 解包 → `exe → exe.old` / `新 exe → exe` 两步 rename
//!      （Windows 允许 rename 正在运行的 exe）→ 校验新 exe 具名 → 删 .old。
//!      任一步失败回滚并保留标记，下次启动重试。
//!
//! 事件：`update-available`、`update-download-progress`、`update-ready`。

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

/// 更新清单 schema 版本。
const SCHEMA_VERSION: u32 = 1;

/// 待应用更新标记文件：`data_root()/updates/.pending.json`
const PENDING_FILE: &str = ".pending.json";

/// 下载根目录：`data_root()/updates/<version>/`
fn updates_root() -> Result<PathBuf, String> {
    Ok(crate::paths::data_root().join("updates"))
}

fn pending_file() -> Result<PathBuf, String> {
    Ok(updates_root()?.join(PENDING_FILE))
}

/// 更新清单（远端 `update.json`），字段与文档 §6.1 一致。
#[derive(Debug, Clone, Deserialize)]
struct UpdateManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    /// 新版本号（对比当前版本决定是否可更新）
    #[serde(default)]
    version: String,
    /// 可升级的最低版本下限（跳级保护）
    #[serde(default, rename = "minimumUpgradable")]
    minimum_upgradable: String,
    /// 更新说明摘要（下载前给用户看）
    #[serde(default)]
    notes: String,
    /// 平台条目：`windows-x86_64` → 下载信息
    #[serde(default)]
    platforms: std::collections::HashMap<String, PlatformEntry>,
}

impl Default for UpdateManifest {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            version: String::new(),
            minimum_upgradable: String::new(),
            notes: String::new(),
            platforms: std::collections::HashMap::new(),
        }
    }
}

/// 单个平台的下载信息。
#[derive(Debug, Clone, Deserialize)]
struct PlatformEntry {
    /// 标准版下载地址（zip）
    #[serde(default)]
    url: String,
    /// 便携版下载地址（zip，便携版优先）
    #[serde(default, rename = "portableUrl")]
    portable_url: String,
    /// 标准版 zip 的 sha256（hex 小写）
    #[serde(default)]
    sha256: String,
    /// 便携版 zip 的 sha256（hex 小写）
    #[serde(default, rename = "portableSha256")]
    portable_sha256: String,
    /// zip 字节大小（0 = 未知，仅作进度参考）
    #[serde(default)]
    size: u64,
    /// 便携版 zip 字节大小（0 = 未知）
    #[serde(default, rename = "portableSize")]
    portable_size: u64,
}

impl Default for PlatformEntry {
    fn default() -> Self {
        Self {
            url: String::new(),
            portable_url: String::new(),
            sha256: String::new(),
            portable_sha256: String::new(),
            size: 0,
            portable_size: 0,
        }
    }
}

/// 前端查询/收到的更新信息（`get_update_status` / `update-available` 负载）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 是否有可用更新（已命中版本且未下载）
    pub available: bool,
    /// 目标版本号（空 = 无目标版本）
    pub version: String,
    /// 更新说明摘要
    pub notes: String,
    /// 本次更新 zip 大小（0 = 未知）
    pub size: u64,
    /// 该更新是否为便携版专属（决定下载哪个 URL / 校验哪个 sha256）
    pub portable: bool,
    /// 是否已就绪待重启应用（下载完成并写好标记）
    pub ready: bool,
    /// 当前应用的版本号
    pub current: String,
}

impl UpdateInfo {
    fn none(current: &str) -> Self {
        Self {
            available: false,
            version: String::new(),
            notes: String::new(),
            size: 0,
            portable: false,
            ready: false,
            current: current.to_string(),
        }
    }
}

fn current_version(app: &tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 解析更新清单。
fn parse_manifest(bytes: &[u8]) -> Result<UpdateManifest, String> {
    let m: UpdateManifest = serde_json::from_slice(bytes).map_err(|e| format!("清单解析失败: {e}"))?;
    if m.schema_version > SCHEMA_VERSION {
        return Err(format!(
            "升级清单 schemaVersion={} 高于宿主支持的 v{SCHEMA_VERSION}",
            m.schema_version
        ));
    }
    if m.version.is_empty() {
        return Err("升级清单缺少 version 字段".to_string());
    }
    Ok(m)
}

/// 一次性拉取 URL 内容（字节），非 2xx 视为失败。
async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("通信失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("读取响应失败: {e}"))?;
    Ok(bytes.to_vec())
}

/// 脱敏更新源错误信息：不向用户暴露具体 URL。
fn sanitize_update_error(mut msg: String, endpoint: &str, sig_url: &str) -> String {
    for url in [endpoint, sig_url] {
        msg = msg.replace(url, "(更新源地址)");
    }
    msg
}

/// 更新清单地址（用户可配置覆盖）。
fn update_endpoint() -> String {
    let cfg = crate::config::load();
    if cfg.update_endpoint.trim().is_empty() {
        crate::config::DEFAULT_UPDATE_ENDPOINT.to_string()
    } else {
        cfg.update_endpoint.trim().to_string()
    }
}

/// 拉取更新清单并验签（未验签通过一律不信任）。返回 `(清单, 原字节)`。
async fn fetch_manifest(client: &reqwest::Client) -> Result<(UpdateManifest, Vec<u8>), String> {
    let endpoint = update_endpoint();
    let sig_url = format!("{endpoint}.sig");
    let content = fetch_bytes(client, &endpoint)
        .await
        .map_err(|e| sanitize_update_error(format!("拉取更新清单失败：{e}"), &endpoint, &sig_url))?;
    let sig = fetch_bytes(client, &sig_url)
        .await
        .map_err(|e| sanitize_update_error(format!("拉取清单签名失败：{e}"), &endpoint, &sig_url))?;
    let sig = String::from_utf8_lossy(&sig).into_owned();
    crate::signing::verify_detached(&content, &sig)
        .map_err(|e| format!("更新清单验签失败：{e}"))?;
    let manifest = parse_manifest(&content)?;
    Ok((manifest, content))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    crate::market::version_cmp(a, b)
}

/// 解析平台条目：取 `windows-x86_64`；便携版优先 portableUrl。
fn platform_entry(manifest: &UpdateManifest) -> Option<(PlatformEntry, bool)> {
    let entry = manifest.platforms.get("windows-x86_64")?;
    let portable = crate::paths::is_portable();
    let available = if portable {
        !entry.portable_url.is_empty()
    } else {
        !entry.url.is_empty()
    };
    if !available {
        return None;
    }
    Some((entry.clone(), portable))
}

/// 判断候选版本是否可比当前版本更新（semver；含 minimumUpgradable 跳级保护）。
fn is_newer(manifest: &UpdateManifest, current: &str) -> bool {
    if manifest.version.is_empty() {
        return false;
    }
    if version_cmp(&manifest.version, current) != std::cmp::Ordering::Greater {
        return false;
    }
    if !manifest.minimum_upgradable.is_empty()
        && version_cmp(current, &manifest.minimum_upgradable) == std::cmp::Ordering::Less
    {
        log::info!(
            "更新源版本 v{} 要求最低可升级 v{}，当前 v{}，跳级保护拦截",
            manifest.version,
            manifest.minimum_upgradable,
            current
        );
        return false;
    }
    true
}

/// 检查是否有可用更新。验签失败 / 通信失败时**静默**返回"无更新"
/// （记日志不打扰用户），只有真正命中才广播 `update-available`。
///
/// `manual`：是否由用户手动触发（About 页「检查更新」）。手动检查时
/// **忽略**「跳过此版本」记录——用户主动查看，应能再次看到该版本。
#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
    manual: Option<bool>,
) -> Result<UpdateInfo, String> {
    let manual = manual.unwrap_or(false);
    let current = current_version(&app);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let (manifest, _) = match fetch_manifest(&client).await {
        Ok(m) => m,
        Err(e) => {
            log::warn!("更新检查失败（静默）: {e}");
            // 手动检查（About 页）应向上报错，让前端提示失败原因；
            // 仅自动检查静默降级为"无更新"
            if manual {
                return Err(e);
            }
            return Ok(UpdateInfo::none(&current));
        }
    };

    if !is_newer(&manifest, &current) {
        log::info!("已是最新版本（当前 v{current}，源 v{}）", manifest.version);
        return Ok(UpdateInfo::none(&current));
    }
    // 用户「跳过此版本」：与清单目标版本一致时不再提示（记录到 config）。
    // 仅自动检查时生效——手动检查更新应能再次看到并选择升级。
    if !manual && crate::config::load().skipped_update_version == manifest.version {
        log::info!("版本 v{} 已被用户跳过，不再提示", manifest.version);
        return Ok(UpdateInfo::none(&current));
    }
    let (entry, portable) = match platform_entry(&manifest) {
        Some(p) => p,
        None => {
            log::warn!("更新源无当前平台条目（windows-x86_64）");
            return Ok(UpdateInfo::none(&current));
        }
    };

    let info = UpdateInfo {
        available: true,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        size: if portable { entry.portable_size } else { entry.size },
        portable,
        ready: false,
        current,
    };
    log::info!(
        "发现新版本 v{}（{}，{:.1} MB，便携版={}）",
        manifest.version,
        if portable { "portable" } else { "standard" },
        (if portable { entry.portable_size } else { entry.size }) as f64 / 1048576.0,
        portable
    );
    let _ = app.emit("update-available", &info);
    Ok(info)
}

/// 下载可用更新（`check_for_update` 命中后调用）。
/// 重新拉取清单并验签（保证下载依据仍是最新签名清单）→ 取对应平台条目 →
/// 流式下载 → 边下边算 sha256 校验 → 写 `updates/<version>/x-hub.zip` →
/// 写 `.pending.json` → 广播 `update-ready`。
///
/// 前端传入 `version` 仅为目标版本校验：下载时若清单中的目标版本与请求不符
/// 则中止（防并发/竞态下下载旧条目）。
#[tauri::command]
pub async fn download_update(
    app: tauri::AppHandle,
    version: String,
) -> Result<UpdateInfo, String> {
    let current = current_version(&app);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let (manifest, _) = fetch_manifest(&client).await?;
    if !manifest.version.eq(&version) {
        return Err(format!(
            "更新源已变更（目标 v{version}，清单 v{}），请重新检查更新",
            manifest.version
        ));
    }
    let (entry, portable) = platform_entry(&manifest).ok_or_else(|| {
        format!("更新源无当前平台条目（windows-x86_64，版本 v{version}）")
    })?;
    let (url, sha256) = if portable {
        (entry.portable_url.clone(), entry.portable_sha256.clone())
    } else {
        (entry.url.clone(), entry.sha256.clone())
    };
    if url.is_empty() {
        return Err("更新清单缺少下载地址".to_string());
    }

    // 目录准备：updates/<version>/
    let ver_dir = updates_root()?.join(&version);
    std::fs::create_dir_all(&ver_dir).map_err(|e| format!("创建更新目录失败: {e}"))?;
    let zip_path = ver_dir.join("x-hub.zip");
    let tmp_zip = ver_dir.join("x-hub.zip.tmp");

    // 流式下载到临时文件，边下边算 sha256，节流广播进度（≥256KB 一次）。
    // 复用上面带 timeout 的 client（reqwest::get 会新建默认客户端绕过超时）
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let total = {
        let manifest_size = if portable { entry.portable_size } else { entry.size };
        if manifest_size > 0 { Some(manifest_size) } else { resp.content_length() }
    };
    let mut file = std::fs::File::create(&tmp_zip).map_err(|e| format!("创建文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    let mut last_emit: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        received += chunk.len() as u64;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .map_err(|e| format!("写入失败: {e}"))?;
        if received - last_emit >= 262_144 || received == total.unwrap_or(0) {
            last_emit = received;
            let _ = app.emit("update-download-progress", crate::market::DownloadProgress {
                id: "x-hub".to_string(),
                received,
                total,
            });
        }
    }
    file.flush().map_err(|e| format!("落盘失败: {e}"))?;
    drop(file);
    if let Some(t) = total {
        if t != 0 && received != t {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err(format!("下载不完整: 收到 {received} 字节，预期 {t} 字节"));
        }
    }
    // 校验完整性（清单背书）
    if !sha256.is_empty() {
        let actual = to_hex(&hasher.finalize());
        if !actual.eq_ignore_ascii_case(&sha256) {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err(format!(
                "下载校验失败（sha256 不匹配）\n期望: {sha256}\n实际: {actual}\n更新包可能被篡改或损坏，已中止。"
            ));
        }
    }
    // 就位（rename 原子）
    std::fs::rename(&tmp_zip, &zip_path).map_err(|e| format!("更新包就位失败: {e}"))?;

    // 写待应用标记
    let pending = serde_json::json!({
        "version": manifest.version.clone(),
        "zipPath": zip_path.to_string_lossy().to_string(),
        "portable": portable,
    });
    std::fs::write(pending_file()?, serde_json::to_vec_pretty(&pending).unwrap_or_default())
        .map_err(|e| format!("写入更新标记失败: {e}"))?;

    log::info!(
        "更新已下载就绪: v{} -> v{}（{} 字节，便携版={}）",
        current,
        manifest.version,
        received,
        portable
    );
    let info = UpdateInfo {
        available: true,
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        size: if portable { entry.portable_size } else { entry.size },
        portable,
        ready: true,
        current,
    };
    let _ = app.emit("update-ready", &info);
    Ok(info)
}

/// 当前更新状态（不发起网络请求）：读取本地标记与配置，供前端展示。
#[tauri::command]
pub fn get_update_status(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = current_version(&app);
    let mut info = UpdateInfo::none(&current);
    if let Some(pending) = read_pending() {
        info.available = true;
        info.version = pending.version;
        info.portable = pending.portable;
        info.ready = true;
    }
    Ok(info)
}

/// 记录用户「跳过此版本」：将该版本号持久化到 config，后续检查更新时不再提示该版本。
#[tauri::command]
pub fn skip_update_version(version: String) -> Result<(), String> {
    let version = version.trim().to_string();
    if version.is_empty() {
        return Err("版本号不能为空".to_string());
    }
    let _guard = crate::config::lock();
    let mut config = crate::config::load();
    config.skipped_update_version = version.clone();
    crate::config::save(&config)?;
    log::info!("已跳过版本 v{}，后续检查不再提示", version);
    Ok(())
}

/// 待应用更新标记。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
    version: String,
    #[serde(default)]
    zip_path: String,
    #[serde(default)]
    portable: bool,
}

fn read_pending() -> Option<PendingUpdate> {
    let path = pending_file().ok()?;
    if !path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<PendingUpdate>(&content).ok()
}

/// 应用待更新版本（每次启动早期调用，幂等）。
/// Windows 允许 rename 正在运行的 exe：`exe → exe.old`、`新 exe → exe`。
/// 任一步失败回滚（.old 还原）并保留标记，下次启动重试。
/// 成功则删除标记 + 旧版本剩余文件。
pub fn apply_pending_update() {
    let Some(pending) = read_pending() else {
        return;
    };
    let zip_path = PathBuf::from(&pending.zip_path);
    if !zip_path.is_file() {
        log::warn!("待应用更新包不存在（{}），清理标记", zip_path.display());
        let _ = remove_pending_file();
        return;
    }

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("无法定位当前 exe（{e}），跳过本次应用更新");
            return;
        }
    };
    let exe_dir = match exe_path.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            log::warn!("无法定位 exe 目录，跳过本次应用更新");
            return;
        }
    };

    // 解包新版本到同一目录下的隐藏暂存目录
    let version_dir = match updates_root() {
        Ok(d) => d.join(&pending.version),
        Err(_) => return,
    };
    let staging = version_dir.join(".staging");
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    let staging_tmp = version_dir.join(".staging.tmp");
    if staging_tmp.exists() {
        let _ = std::fs::remove_dir_all(&staging_tmp);
    }
    if let Err(e) = crate::market::extract_zip_read(&zip_path, &staging_tmp) {
        log::warn!("更新包解包失败（{}），跳过本次应用更新，稍后重试", e);
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }

    // 解包结果里找到真正可执行的 exe（根或一层子目录；Windows 平台产物名不定，
    // 统一取 `.exe` 后缀且体积最大的候选；无则失败回滚）
    let new_exe = locate_new_exe(&staging_tmp);
    let Some(new_exe) = new_exe else {
        log::warn!("更新包内未找到可执行文件（.exe），跳过应用更新");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    };
    // 校验新 exe 不是当前正在运行的自己（防更新包误打包旧版）
    if new_exe == exe_path {
        log::warn!("更新包内 exe 与当前一致，疑似打包异常，跳过");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }

    if let Err(e) = std::fs::rename(&staging_tmp, &staging) {
        log::warn!("更新暂存目录就位失败（{e}），跳过本次应用更新");
        let _ = std::fs::remove_dir_all(&staging_tmp);
        return;
    }
    // 新 exe 相对暂存根目录的偏移（可能在子目录），rename 后同样相对 staging 拼接
    let rel = new_exe
        .strip_prefix(&staging_tmp)
        .unwrap_or_else(|_| std::path::Path::new(new_exe.file_name().unwrap_or_default()));
    let new_in_staging = staging.join(rel);

    // 自替换：两步 rename
    // 1) exe → exe.old（可能已存在 .old 残留：先清掉旧残留）
    let old_path = exe_dir.join(exe_file_name_with_old(&exe_path));
    let _ = std::fs::remove_file(&old_path);
    if let Err(e) = std::fs::rename(&exe_path, &old_path) {
        log::warn!("备份当前 exe 失败（{e}），跳过本次应用更新");
        let _ = std::fs::remove_dir_all(&staging);
        return;
    }
    // 2) 新 exe → exe；失败则回滚
    if let Err(e) = std::fs::rename(&new_in_staging, &exe_path) {
        log::error!("安装新版本失败（{e}），回滚到旧版本");
        let _ = std::fs::rename(&old_path, &exe_path);
        let _ = std::fs::remove_dir_all(&staging);
        return;
    }
    // 3) 清理 .old 与新包残余
    let _ = std::fs::remove_file(&old_path);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&zip_path);
    let _ = remove_pending_file();

    log::info!("应用已自替换升级到 v{}（重启后生效）", pending.version);
}

fn remove_pending_file() -> std::io::Result<()> {
    if let Ok(p) = pending_file() {
        let _ = std::fs::remove_file(&p);
    }
    Ok(())
}

fn exe_file_name_with_old(exe: &Path) -> std::ffi::OsString {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    name
}

/// 在解包目录（根或一层）里找 `.exe` 文件；有多个时取体积最大的（防误取只读小工具）。
fn locate_new_exe(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, u64)> = None;
    let mut consider = |p: &Path| {
        if let Some(name) = p.file_name() {
            let lower = name.to_string_lossy().to_lowercase();
            if lower.ends_with(".exe") && std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false) {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                if best.as_ref().map(|(_, s)| size > *s).unwrap_or(true) {
                    best = Some((p.to_path_buf(), size));
                }
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            consider(&p);
            if p.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&p) {
                    for ie in inner.flatten() {
                        consider(&ie.path());
                    }
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pending_camel_case() {
        // download_update 写入的是 camelCase 的 zipPath，读取必须能映射回 zip_path
        let json = serde_json::json!({
            "version": "0.3.1",
            "zipPath": "C:/x/updates/0.3.1/x-hub.zip",
            "portable": false,
        });
        let p: PendingUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(p.version, "0.3.1");
        assert_eq!(p.zip_path, "C:/x/updates/0.3.1/x-hub.zip");
        assert!(!p.portable);
    }

    #[test]
    fn parses_manifest() {
        let json = serde_json::json!({
            "schemaVersion": 1,
            "version": "0.4.0",
            "minimumUpgradable": "0.1.0",
            "notes": "v0.4.0: 新增更新中心",
            "platforms": {
                "windows-x86_64": {
                    "url": "https://dist/x-hub-0.4.0.zip",
                    "portableUrl": "https://dist/x-hub-0.4.0-portable.zip",
                    "sha256": "abc",
                    "portableSha256": "def",
                    "size": 1024
                }
            }
        });
        let m = parse_manifest(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.version, "0.4.0");
        assert_eq!(m.minimum_upgradable, "0.1.0");
        let e = m.platforms.get("windows-x86_64").expect("有平台条目");
        assert_eq!(e.portable_url, "https://dist/x-hub-0.4.0-portable.zip");
    }

    #[test]
    fn rejects_future_schema() {
        let json = serde_json::json!({
            "schemaVersion": 99,
            "version": "0.4.0",
            "platforms": {}
        });
        assert!(parse_manifest(&serde_json::to_vec(&json).unwrap()).is_err());
    }

    #[test]
    fn rejects_missing_version() {
        let json = serde_json::json!({ "schemaVersion": 1, "platforms": {} });
        assert!(parse_manifest(&serde_json::to_vec(&json).unwrap()).is_err());
    }

    #[test]
    fn is_newer_guards_jump() {
        let mut m = UpdateManifest::default();
        m.version = "0.4.0".to_string();
        // 正常更新
        assert!(is_newer(&m, "0.3.0"));
        // 同版本/低版本不更新
        assert!(!is_newer(&m, "0.4.0"));
        assert!(!is_newer(&m, "0.5.0"));
        // 跳级保护：当前低于可升级下限
        m.minimum_upgradable = "0.3.0".to_string();
        assert!(!is_newer(&m, "0.2.0"));
        assert!(is_newer(&m, "0.3.0"));
    }

    #[test]
    fn locate_exe_picks_largest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        std::fs::write(dir.path().join("x-hub.exe"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("helper.exe"), vec![0u8; 10]).unwrap();
        let found = locate_new_exe(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "x-hub.exe");
    }

    #[test]
    fn locate_exe_finds_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("res");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("x-hub.exe"), vec![0u8; 50]).unwrap();
        let found = locate_new_exe(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap().to_string_lossy(), "x-hub.exe");
    }
}