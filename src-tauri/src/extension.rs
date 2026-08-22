//! 扩展系统：manifest 解析 + 本地目录扫描注册表（spec §12 第 1 步）。
//!
//! 第 1 步只做「扫描 + 解析 + 列出」，不涉及安装 / 卸载 / 打开 / service 进程托管。
//! service 托管（启动 / 端口 / 代理 / 健康检查 / 运行时提供）见 spec §5，后续步骤实现。
//!
//! 目录约定：`app_data_dir()/extensions/<extId>/manifest.json`。
//! 每个子目录是一个已安装扩展；manifest 损坏或缺失的目录会被标记为 invalid 返回，
//! 让扩展中心能展示「此扩展不可用」而非静默消失。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tauri::Manager;

/// 扩展运行时类型：web（纯前端）/ service（带后端服务）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionRuntime {
    Web,
    Service,
}

impl Default for ExtensionRuntime {
    fn default() -> Self {
        ExtensionRuntime::Web
    }
}

/// service 扩展的后端声明（manifest.backend）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSpec {
    /// 后端入口（相对扩展目录的路径）
    pub entry: String,
    /// 运行时引擎要求（宿主据此判断复用系统运行时或下载内置）
    pub engine: Option<EngineSpec>,
    /// 后端工作目录（相对扩展目录）
    pub cwd: Option<String>,
    /// 0 = 动态分配；固定端口需冲突检测
    pub port: Option<u16>,
    /// 健康检查路径（可选）
    pub health: Option<String>,
}

/// 后端运行时引擎要求（backend.engine）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpec {
    #[serde(rename = "type")]
    pub engine_type: String,
    #[serde(rename = "minVersion")]
    pub min_version: Option<String>,
}

/// window / drawer 建议尺寸（manifest.minSize）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinSize {
    pub w: f64,
    pub h: f64,
}

/// 扩展 manifest（manifest.json），对齐 spec §4。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// 唯一标识，反向域名
    pub id: String,
    pub name: String,
    pub version: String,
    /// 运行时：web（默认）| service
    #[serde(default)]
    pub runtime: ExtensionRuntime,
    /// 默认 / 主形态：module / view / window / drawer
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 声明支持哪些形态
    #[serde(default)]
    pub surfaces: Vec<String>,
    /// app 形态下支持哪些打开方式（manifest 字段为 openIn）
    #[serde(default, rename = "openIn")]
    pub open_in: Vec<String>,
    /// 各形态入口：形态 → 相对扩展目录的路径
    #[serde(default)]
    pub entry: HashMap<String, String>,
    /// 能力申请，按需授权
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 图标（相对扩展目录的路径）
    pub icon: Option<String>,
    /// window / drawer 建议尺寸（manifest 字段为 minSize）
    #[serde(default, rename = "minSize")]
    pub min_size: Option<MinSize>,
    /// service 专属：受托管的后端服务声明
    pub backend: Option<BackendSpec>,
    /// 一句话描述（列表展示用；spec §4 未列，作为可选补充字段）
    #[serde(default)]
    pub description: String,
}

fn default_kind() -> String {
    "view".to_string()
}

/// 已安装扩展的注册表项（返回给前端的展示结构）。
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "web" | "service"
    pub runtime: String,
    /// 默认 / 主形态
    pub kind: String,
    pub surfaces: Vec<String>,
    pub open_in: Vec<String>,
    pub permissions: Vec<String>,
    pub description: String,
    /// 图标文件绝对路径（存在时才非空）
    pub icon: Option<String>,
    /// 扩展目录绝对路径
    pub dir: String,
    /// manifest 缺失 / 解析失败时为 true
    pub invalid: bool,
    /// invalid 时的原因（供扩展中心友好展示）
    pub error: Option<String>,
}

fn runtime_str(r: &ExtensionRuntime) -> &'static str {
    match r {
        ExtensionRuntime::Web => "web",
        ExtensionRuntime::Service => "service",
    }
}

/// 扩展根目录：`app_data_dir()/extensions/`
pub fn extensions_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("extensions"))
}

/// 扫描扩展根目录，解析每个子目录的 manifest.json。
/// 单个目录解析失败不影响整体扫描，只标记该目录为 invalid。
pub fn scan_extensions(app: &tauri::AppHandle) -> Result<Vec<ExtensionEntry>, String> {
    let root = extensions_root(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let dirs = std::fs::read_dir(&root).map_err(|e| e.to_string())?;
    for entry in dirs.flatten() {
        let path = entry.path();
        if path.is_dir() {
            entries.push(load_extension(&path));
        }
    }
    // 按名称排序（中文按 Unicode 码点），保证列表稳定
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// 加载单个扩展目录为注册表项（永不 panic，损坏时返回 invalid 项）。
fn load_extension(dir: &Path) -> ExtensionEntry {
    let dir_str = dir.to_string_lossy().into_owned();
    let fallback_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir_str.clone());

    let invalid = |error: String| ExtensionEntry {
        id: fallback_name.clone(),
        name: fallback_name,
        version: String::new(),
        runtime: "web".to_string(),
        kind: "view".to_string(),
        surfaces: Vec::new(),
        open_in: Vec::new(),
        permissions: Vec::new(),
        description: String::new(),
        icon: None,
        dir: dir_str.clone(),
        invalid: true,
        error: Some(error),
    };

    let manifest_path = dir.join("manifest.json");
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(e) => return invalid(format!("读取 manifest.json 失败：{e}")),
    };
    let manifest: ExtensionManifest = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => return invalid(format!("manifest 解析失败：{e}")),
    };

    let icon = manifest.icon.as_ref().and_then(|rel| {
        let p = dir.join(rel);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    });

    ExtensionEntry {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        runtime: runtime_str(&manifest.runtime).to_string(),
        kind: manifest.kind,
        surfaces: manifest.surfaces,
        open_in: manifest.open_in,
        permissions: manifest.permissions,
        description: manifest.description,
        icon,
        dir: dir_str,
        invalid: false,
        error: None,
    }
}

/// 列出已安装扩展（Tauri 命令）。
#[tauri::command]
pub fn list_extensions(app: tauri::AppHandle) -> Result<Vec<ExtensionEntry>, String> {
    let entries = scan_extensions(&app)?;
    log::info!("扩展注册表扫描完成：{} 个扩展", entries.len());
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_manifest(root: &Path, id: &str, manifest: serde_json::Value) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn parses_web_manifest() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.apidebug",
            serde_json::json!({
                "id": "com.x-hub.apidebug",
                "name": "API 调试助手",
                "version": "1.0.0",
                "runtime": "web",
                "kind": "view",
                "surfaces": ["module", "view"],
                "openIn": ["view", "window"],
                "entry": { "view": "./app/index.js" },
                "permissions": ["clipboard"],
                "icon": "./icon.svg"
            }),
        );

        let entry = load_extension(&dir.path().join("com.x-hub.apidebug"));
        assert!(!entry.invalid);
        assert_eq!(entry.id, "com.x-hub.apidebug");
        assert_eq!(entry.name, "API 调试助手");
        assert_eq!(entry.runtime, "web");
        assert_eq!(entry.kind, "view");
        assert_eq!(entry.surfaces, vec!["module", "view"]);
        assert_eq!(entry.open_in, vec!["view", "window"]);
        assert_eq!(entry.permissions, vec!["clipboard"]);
    }

    #[test]
    fn parses_service_manifest_with_backend() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.dsh",
            serde_json::json!({
                "id": "com.x-hub.dsh",
                "name": "DeepSeek Harness",
                "version": "0.1.1",
                "runtime": "service",
                "kind": "view",
                "surfaces": ["view"],
                "openIn": ["view", "window"],
                "entry": { "view": "./entry/index.html" },
                "backend": {
                    "entry": "./service/index.js",
                    "engine": { "type": "node", "minVersion": "22" },
                    "cwd": "./service",
                    "port": 0,
                    "health": "/healthz"
                },
                "permissions": ["network", "fs", "process"]
            }),
        );

        let entry = load_extension(&dir.path().join("com.x-hub.dsh"));
        assert!(!entry.invalid);
        assert_eq!(entry.runtime, "service");

        // 直接解析 manifest，校验 backend 字段
        let raw = std::fs::read_to_string(dir.path().join("com.x-hub.dsh/manifest.json")).unwrap();
        let manifest: ExtensionManifest = serde_json::from_str(&raw).unwrap();
        let backend = manifest.backend.unwrap();
        assert_eq!(backend.entry, "./service/index.js");
        assert_eq!(backend.port, Some(0));
        assert_eq!(backend.health.as_deref(), Some("/healthz"));
        let engine = backend.engine.unwrap();
        assert_eq!(engine.engine_type, "node");
        assert_eq!(engine.min_version.as_deref(), Some("22"));
    }

    #[test]
    fn runtime_defaults_to_web_and_kind_to_view() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.minimal",
            serde_json::json!({
                "id": "com.x-hub.minimal",
                "name": "最小扩展",
                "version": "0.0.1"
            }),
        );

        let raw = std::fs::read_to_string(dir.path().join("com.x-hub.minimal/manifest.json")).unwrap();
        let manifest: ExtensionManifest = serde_json::from_str(&raw).unwrap();
        assert_eq!(manifest.runtime, ExtensionRuntime::Web);
        assert_eq!(manifest.kind, "view");
    }

    #[test]
    fn broken_manifest_is_invalid_not_panic() {
        let dir = tempdir().unwrap();
        let ext = dir.path().join("com.x-hub.broken");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::write(ext.join("manifest.json"), "not valid json {{{").unwrap();

        let entry = load_extension(&ext);
        assert!(entry.invalid);
        assert!(entry.error.is_some());
        assert_eq!(entry.id, "com.x-hub.broken");
    }

    #[test]
    fn missing_manifest_is_invalid() {
        let dir = tempdir().unwrap();
        let ext = dir.path().join("com.x-hub.nomanifest");
        std::fs::create_dir_all(&ext).unwrap();

        let entry = load_extension(&ext);
        assert!(entry.invalid);
        assert!(entry.error.unwrap().contains("manifest.json"));
    }
}
