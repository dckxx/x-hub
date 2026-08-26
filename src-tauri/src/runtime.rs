//! service 后端运行时解析（spec §5.2）：系统 Node 优先，失败自动降级到内置运行时，
//! 内置未缓存则按需下载（Node 官方分发）。返回用于启动后端的可执行文件路径。

use std::path::PathBuf;

/// 内置 Node 版本（Node 官方 LTS）
const NODE_VERSION: &str = "v24.9.0";

/// 解析 Node 版本号的主版本（"v22.11.0" → 22）
fn node_major(version: &str) -> u32 {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0)
}

/// 检测系统 Node 是否可用且主版本 ≥ min_version（min_version 形如 "22"）。
/// 成功返回版本号（如 "v22.11.0"）。
fn check_system_node(min_version: Option<&str>) -> Result<String, String> {
    let out = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map_err(|_| "未检测到 Node.js".to_string())?;
    if !out.status.success() {
        return Err("Node.js 不可用".to_string());
    }
    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if let Some(min) = min_version {
        let min_major = node_major(min);
        let cur_major = node_major(&ver);
        if cur_major < min_major {
            return Err(format!("系统 Node {ver} 低于要求 ≥ {min}"));
        }
    }
    Ok(ver)
}

/// 内置 Node 缓存目录：`data_root()/runtime/node`
/// 必须用 `paths::data_root()`（便携版跟随 exe 目录\data），不能用 `app_data_dir()`。
fn builtin_node_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let _ = app;
    Ok(crate::paths::data_root().join("runtime").join("node"))
}

/// 内置 Node 可执行文件路径（已缓存则 Some）
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn builtin_node_exe(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = builtin_node_dir(app).ok()?;
    let exe = dir
        .join(format!("node-{NODE_VERSION}-win-x64"))
        .join("node.exe");
    exe.is_file().then_some(exe)
}

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
fn builtin_node_exe(_app: &tauri::AppHandle) -> Option<PathBuf> {
    None
}

/// 下载并解压内置 Node，返回可执行文件路径。
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
async fn download_builtin_node(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let url = format!("https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-win-x64.zip");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    let dir = builtin_node_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::market::extract_zip(&bytes, &dir)?;

    let exe = dir
        .join(format!("node-{NODE_VERSION}-win-x64"))
        .join("node.exe");
    if !exe.is_file() {
        return Err("解压后未找到 node.exe".to_string());
    }
    log::info!("内置 Node 运行时已就绪: {}", exe.display());
    Ok(exe)
}

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
async fn download_builtin_node(_app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Err("当前平台暂不支持内置运行时下载，请安装 Node.js".to_string())
}

/// 解析用于启动后端的 node 可执行文件。策略（config.runtime_strategy）：
/// - `system`：始终用系统 Node，不降级、不下载；
/// - `builtin`：始终用内置（已缓存直接用，未缓存下载）；
/// - `auto`（默认）：系统优先，版本不符/缺失降级内置，未缓存则下载。
pub fn resolve_node(
    app: &tauri::AppHandle,
    min_version: Option<&str>,
    strategy: &str,
) -> Result<PathBuf, String> {
    match strategy {
        "system" => {
            check_system_node(min_version).map_err(|e| {
                format!(
                    "{e}（运行时策略为「始终系统」，请安装 Node ≥ {}）",
                    min_version.unwrap_or("22")
                )
            })?;
            Ok(PathBuf::from("node"))
        }
        "builtin" => {
            if let Some(exe) = builtin_node_exe(app) {
                return Ok(exe);
            }
            log::info!("运行时策略「始终内置」，下载内置运行时…");
            tauri::async_runtime::block_on(download_builtin_node(app))
        }
        _ => {
            let system_err = match check_system_node(min_version) {
                Ok(_ver) => return Ok(PathBuf::from("node")),
                Err(e) => e,
            };

            if let Some(exe) = builtin_node_exe(app) {
                log::info!("系统 Node 不可用（{system_err}），降级使用内置运行时");
                return Ok(exe);
            }

            log::info!("系统 Node 不可用，下载内置运行时（首次，一次性）…");
            match tauri::async_runtime::block_on(download_builtin_node(app)) {
                Ok(exe) => Ok(exe),
                Err(e) => Err(format!(
                    "系统 Node 不可用（{system_err}），内置运行时下载失败（{e}）。请安装 Node ≥ {}",
                    min_version.unwrap_or("22")
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_major_parses() {
        assert_eq!(node_major("v22.11.0"), 22);
        assert_eq!(node_major("v18.20.4"), 18);
        assert_eq!(node_major("22"), 22);
        assert_eq!(node_major("bogus"), 0);
    }
}
