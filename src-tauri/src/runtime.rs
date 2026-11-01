//! service 后端运行时解析（spec §5.2）：系统 Node 优先，失败自动降级到内置运行时，
//! 内置未缓存则按需下载（Node 官方分发）。返回用于启动后端的可执行文件路径。

use crate::process::NoConsoleWindow;
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
    let node = find_system_node().ok_or_else(|| "未检测到 Node.js".to_string())?;
    let mut cmd = std::process::Command::new(&node);
    cmd.arg("--version");
    cmd.no_console_window();
    let out = cmd
        .output()
        .map_err(|_| "Node.js 不可用".to_string())?;
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

/// 解析系统 node 可执行文件：PATH 优先；macOS 上 PATH 找不到时再探测常见安装位置。
///
/// **为什么 macOS 要探测**：从 Finder/Dock 启动的 GUI 进程不继承登录 shell 的
/// PATH（launchd 默认只有 `/usr/bin:/bin:/usr/sbin:/sbin`），Homebrew / 官方 pkg /
/// nvm / volta 以及本机 AI 助手捆绑的 node 运行时都装在这些默认路径之外——
/// 只依赖 PATH 时表现为「终端里明明有 node、应用里却报未检测到」。
/// 返回绝对路径（后续 spawn 后端不再依赖 PATH）；PATH 命中时返回字面量 `node`。
fn find_system_node() -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("node");
    cmd.arg("--version");
    cmd.no_console_window();
    if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
        return Some(PathBuf::from("node"));
    }
    #[cfg(target_os = "macos")]
    {
        for p in node_candidate_paths() {
            if !p.is_file() {
                continue;
            }
            let mut c = std::process::Command::new(&p);
            c.arg("--version");
            c.no_console_window();
            if c.output().map(|o| o.status.success()).unwrap_or(false) {
                log::info!("[运行时] 已定位系统 Node: {}", p.display());
                return Some(p);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ();
    None
}

/// macOS 上常见的 node 安装位置（PATH 兜底探测顺序）。
#[cfg(target_os = "macos")]
fn node_candidate_paths() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/opt/homebrew/bin/node"), // Homebrew（Apple Silicon）
        PathBuf::from("/usr/local/bin/node"),    // Homebrew（Intel）/ 官方 pkg
    ];
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".volta/bin/node")); // volta
        // 本机 AI 助手捆绑的 node 运行时（桌面应用看不到 PATH 时的实际来源）
        out.push(home.join(".local/share/TeleAgent/runtimes/node/bin/node"));
        // nvm：取版本目录里语义化版本最高的那个
        if let Some(best) = nvm_best_node(&home) {
            out.push(best);
        }
    }
    out
}

/// nvm 默认安装位置里语义化版本最高的 node 可执行文件（`~/.nvm/versions/node/vX.Y.Z/bin/node`）
#[cfg(target_os = "macos")]
fn nvm_best_node(home: &std::path::Path) -> Option<PathBuf> {
    let root = home.join(".nvm/versions/node");
    let mut best: Option<((u32, u32, u32), PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()? .flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(ver) = name.strip_prefix('v') else { continue };
        let mut it = ver.split('.');
        let v = (
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        );
        if best.as_ref().map_or(true, |(bv, _)| v > *bv) {
            best = Some((v, entry.path().join("bin").join("node")));
        }
    }
    best.filter(|(_, p)| p.is_file()).map(|(_, p)| p)
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
            // 探测出的绝对路径（GUI 启动无用户 PATH，spawn 后端时字面量 "node" 不可靠）
            Ok(find_system_node().unwrap_or_else(|| PathBuf::from("node")))
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
                Ok(_) => {
                    // 探测出的绝对路径（GUI 启动无用户 PATH，spawn 后端时字面量 "node" 不可靠）
                    return Ok(find_system_node().unwrap_or_else(|| PathBuf::from("node")));
                }
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
