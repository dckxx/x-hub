//! 扩展市场（spec §11 安装页「市场」tab）。
//!
//! 数据源一期为**本地市场清单文件**：`app_data_dir/market/registry.json`，
//! 格式 `{ "extensions": [{ "id", "name", "version", "description", "runtime", "author", "downloadUrl" }] }`。
//! 后续接远端市场时，把「读本地文件」换成「fetch 清单 URL」即可，机制不变。
//!
//! 安装流程：下载 `downloadUrl`（zip 包）→ 解包 → 定位 manifest.json 所在目录 → 复制到 `extensions/<id>/`。

use crate::extension::{copy_dir_recursive, extensions_root, read_manifest};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// 市场清单里的一条扩展
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub author: String,
    /// 下载地址（zip 包）
    #[serde(rename = "downloadUrl")]
    pub download_url: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct MarketRegistry {
    #[serde(default)]
    extensions: Vec<MarketExtension>,
}

/// 市场清单文件路径：`app_data_dir/market/registry.json`
fn registry_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("market")
        .join("registry.json"))
}

/// 读取市场清单（不存在或损坏返回空列表）
#[tauri::command]
pub fn get_market_registry(app: tauri::AppHandle) -> Result<Vec<MarketExtension>, String> {
    let path = registry_path(&app)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let registry: MarketRegistry = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(registry.extensions)
}

/// 解包 zip 到目标目录（enclosed_name 防 zip-slip 路径逃逸）
fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
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

/// 从市场下载并安装扩展：下载 zip → 解包 → 校验 manifest → 复制到 extensions/<id>。
#[tauri::command]
pub async fn install_from_market(
    app: tauri::AppHandle,
    download_url: String,
) -> Result<String, String> {
    let resp = reqwest::get(&download_url)
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

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

    log::info!("扩展已从市场安装: {id} <- {download_url}");
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
}
