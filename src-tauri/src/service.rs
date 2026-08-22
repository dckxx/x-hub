//! service 扩展托管：后端进程启动 / 动态端口 / 探活 / 停止（spec §5）。
//!
//! 一期范围（MVP）：
//! - 懒启动：service 扩展首次打开（read_extension_entry）时启动后端，动态分配 127.0.0.1 空闲端口；
//! - 运行时：检测系统 Node（`node --version`，主版本 ≥ backend.engine.minVersion），复用系统 Node；
//!   按需下载内置运行时依赖扩展市场（§12.7），后续实现；
//! - 探活：TcpStream connect 轮询（未做 HTTP 健康检查路径，后续补）；
//! - 代理转发：非流式走桥 API `service.request`（xhub_api 内 reqwest 转发）；`/svc/<extId>/*`
//!   反向代理与 WebSocket 流式后续实现；
//! - 停止：卸载时调用 `stop_service`（卸载 UI 在 §12.7 接入）。

use crate::extension::{extensions_root, read_manifest, ExtensionManifest};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Manager;

/// 单个 service 扩展的运行时状态
pub struct ServiceRuntime {
    pub port: u16,
    pub ready: bool,
    pub child: Option<std::process::Child>,
}

/// 全局 service 运行时注册表（ext_id → ServiceRuntime）
pub struct ServiceState(pub Mutex<HashMap<String, ServiceRuntime>>);

impl Default for ServiceState {
    fn default() -> Self {
        ServiceState(Mutex::new(HashMap::new()))
    }
}

/// 分配 127.0.0.1 空闲端口（bind 0 让 OS 挑，拿到后释放；本地单用户场景竞态可忽略）
fn alloc_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

// 运行时解析（系统 Node 优先 + 内置兜底下载）见 runtime.rs

/// 探活：轮询 connect 端口直到成功或超时
fn probe_ready(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// 启动 service 扩展后端，返回端口。幂等：已启动则直接复用。
pub fn start_service(
    app: &tauri::AppHandle,
    ext_id: &str,
) -> Result<u16, String> {
    let state = app.state::<ServiceState>();
    {
        let map = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(rt) = map.get(ext_id) {
            return Ok(rt.port);
        }
    }

    let dir = extensions_root(app)?.join(ext_id);
    let manifest: ExtensionManifest = read_manifest(&dir)?;
    let backend = manifest
        .backend
        .as_ref()
        .ok_or_else(|| format!("扩展 {ext_id} 未声明 backend（非 service 扩展）"))?;

    let engine_type = backend
        .engine
        .as_ref()
        .map(|e| e.engine_type.as_str())
        .unwrap_or("node");
    if engine_type != "node" {
        return Err(format!("不支持的运行时引擎类型: {engine_type}"));
    }
    let min_version = backend
        .engine
        .as_ref()
        .and_then(|e| e.min_version.as_deref());
    let strategy = crate::config::load().runtime_strategy;
    let node_exe = crate::runtime::resolve_node(app, min_version, &strategy)?;

    let entry = dir.join(&backend.entry);
    if !entry.is_file() {
        return Err(format!("后端入口不存在: {}", backend.entry));
    }
    let cwd = backend.cwd.as_ref().map(|c| dir.join(c)).unwrap_or_else(|| dir.clone());
    let port = alloc_port()?;

    let mut cmd = std::process::Command::new(node_exe);
    cmd.arg(&entry)
        .current_dir(&cwd)
        .env("PORT", port.to_string())
        .env("XHUB_EXT_ID", ext_id)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn().map_err(|e| format!("启动后端失败: {e}"))?;

    let ready = probe_ready(port, Duration::from_secs(10));

    {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.insert(
            ext_id.to_string(),
            ServiceRuntime {
                port,
                ready,
                child: Some(child),
            },
        );
    }
    log::info!("service 扩展已启动: {ext_id} port={port} ready={ready}");
    Ok(port)
}

/// 停止并清理 service 扩展后端进程（卸载 / 宿主退出时调用）
pub fn stop_service(app: &tauri::AppHandle, ext_id: &str) {
    let mut rt = match app.state::<ServiceState>().0.lock() {
        Ok(mut map) => map.remove(ext_id),
        Err(_) => return,
    };
    if let Some(mut child) = rt.as_mut().and_then(|r| r.child.take()) {
        let _ = child.kill();
        let _ = child.wait();
    }
    log::info!("service 扩展已停止: {ext_id}");
}

/// 宿主退出时停止所有 service 后端进程，避免残留
pub fn stop_all(app: &tauri::AppHandle) {
    let state = app.state::<ServiceState>();
    let mut map = match state.0.lock() {
        Ok(m) => m,
        Err(_) => return,
    };
    let ids: Vec<String> = map.keys().cloned().collect();
    for id in ids {
        if let Some(mut rt) = map.remove(&id) {
            if let Some(mut child) = rt.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    log::info!("宿主退出，已停止所有 service 后端进程");
}

/// 已启动 service 的端口（未启动返回 None）
pub fn service_port(app: &tauri::AppHandle, ext_id: &str) -> Option<u16> {
    let state = app.state::<ServiceState>();
    let map = state.0.lock().ok()?;
    map.get(ext_id).map(|rt| rt.port)
}

/// service 是否就绪
pub fn service_ready(app: &tauri::AppHandle, ext_id: &str) -> bool {
    let state = app.state::<ServiceState>();
    let map = state.0.lock().ok();
    map.and_then(|m| m.get(ext_id).map(|rt| rt.ready))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_port_returns_valid_port() {
        let port = alloc_port().unwrap();
        assert!(port > 0);
    }
}
