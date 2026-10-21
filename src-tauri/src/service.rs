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

use crate::extension::{read_manifest, BackendSpec, ExtensionManifest};
use crate::process::NoConsoleWindow;
use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
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

/// 分配空闲端口。按 host 绑定：
/// - host = 127.0.0.1（默认）：本机回环，安全默认
/// - host = 0.0.0.0 / 局域网地址：对外监听（需 network 权限，由 start_service 校验）
/// bind 0 让 OS 挑端口；固定端口时直接试绑该端口。
fn alloc_port(host: &str, fixed: Option<u16>) -> Result<u16, String> {
    let bind_addr = match fixed {
        Some(p) => format!("{host}:{p}"),
        None => format!("{host}:0"),
    };
    let listener = std::net::TcpListener::bind(&bind_addr).map_err(|e| {
        format!("端口绑定失败 {bind_addr}: {e}")
    })?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

// 运行时解析（系统 Node 优先 + 内置兜底下载）见 runtime.rs

/// 后端启动路径：`(entry 脚本, cwd)`。
///
/// **一律归一成普通路径**（剥掉 Windows 的 `\\?\` verbatim 前缀）：Node 的 CJS 加载器读不了
/// verbatim 形式的**脚本路径**（argv[1] 带前缀即 `Error: EISDIR: ... lstat 'A:'` 后 `exit 1`），
/// 于是后端在模块初始化阶段就死；`current_dir` 带前缀本身无害，但两者统一口径，
/// 避免以后只改一处。`ext_protocol::resolve_ext_dir` 出口已归一，这里是第二道保险。
fn backend_paths(dir: &std::path::Path, backend: &BackendSpec) -> (PathBuf, PathBuf) {
    let dir = crate::paths::simplify_path(dir);
    let entry = crate::paths::simplify_path(&dir.join(&backend.entry));
    let cwd = match backend.cwd.as_ref() {
        Some(c) => crate::paths::simplify_path(&dir.join(c)),
        None => dir.clone(),
    };
    (entry, cwd)
}

/// 后端日志文件名：扩展 id 形状白名单是 `[A-Za-z0-9._-]`（manifest 校验），
/// 这里再兜一次，防任何来源的 id 拼出路径穿越。
fn service_log_file_name(ext_id: &str) -> String {
    let safe: String = ext_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}.log")
}

/// service 后端 stdout/stderr 的落盘路径：`<数据根>/logs/service/<扩展 id>.log`。
///
/// 为什么必须落盘：这两个流原本是 `Stdio::null()`，后端启动期崩溃（脚本路径 Node 读不了、
/// 端口占用、依赖缺失……）时宿主侧只剩一个 `ready=false` 布尔值——前端只能提示
/// 「首次使用会自动下载 Node」，与实际原因（Node 早已就绪、后端一启动就退出）完全错位，
/// 排查成本几乎全部来自「错误被静默丢弃」。
fn service_log_path(ext_id: &str) -> Option<PathBuf> {
    let dir = crate::paths::data_root().join("logs").join("service");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(service_log_file_name(ext_id)))
}

/// 单份后端日志上限：超限就删档重来（后端崩溃重启循环不该把磁盘写满）
const SERVICE_LOG_MAX_BYTES: u64 = 1024 * 1024;

/// 打开后端日志（追加写）。超限先删档，避免无限增长。
fn open_service_log(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    if std::fs::metadata(path)
        .map(|m| m.len() > SERVICE_LOG_MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(path);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // 每次启动留一条分隔标记：日志是追加式的，排查时要能分清哪几行属于哪一次启动
    let _ = writeln!(
        file,
        "----- service 启动 {} -----",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    Ok(file)
}

/// 读日志尾部若干行（探活失败时打进宿主日志，让 `x-hub.log` 里直接有线索）
fn tail_text(path: &std::path::Path, max_lines: usize) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// 探活：轮询 connect 端口直到成功或超时。
/// host 为通配地址（0.0.0.0/::）时探回环；为具体地址（如局域网 IP）时探该地址——
/// 后端只在该地址监听，盲探 127.0.0.1 会永远失败（探活超时 → serviceReady 恒 false）。
fn probe_ready(port: u16, host: &str, timeout: Duration) -> bool {
    let probe_host = match host {
        "0.0.0.0" | "::" => "127.0.0.1",
        _ => host,
    };
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect((probe_host, port)).is_ok() {
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
    if !crate::extension::permission_granted(app, ext_id, "service:execute")
        || !crate::extension::permission_granted(app, ext_id, "network") {
        return Err("PERMISSION_DENIED: 本地后端未获信任或网络权限已关闭，请检查扩展权限设置".into());
    }
    let state = app.state::<ServiceState>();
    {
        let map = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(rt) = map.get(ext_id) {
            return Ok(rt.port);
        }
    }

    // 已装扩展与开发扩展共用解析路径（开发扩展由「我的扩展」直挂源码目录）
    let dir = crate::ext_protocol::resolve_ext_dir(app, ext_id)?;
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

    let (entry, cwd) = backend_paths(&dir, backend);
    if !entry.is_file() {
        return Err(format!("后端入口不存在: {}", backend.entry));
    }

    // 监听主机解析 + 对外监听的安全门控：
    // - 默认/127.0.0.1 = 本机回环，任何 service 扩展可用（现状不变）
    // - 0.0.0.0/局域网地址 = 对外开放，必须声明 network 权限且未被用户关闭，否则拒绝启动
    let listen_host = backend.listen_host();
    let external = backend.is_external();
    if external {
        if !manifest.permissions.iter().any(|p| p == "network") {
            return Err(format!(
                "PERMISSION_DENIED: 扩展 {ext_id} 对外监听（host={listen_host}）需要声明 network 权限"
            ));
        }
        if !crate::extension::permission_granted(app, ext_id, "network") {
            return Err(format!(
                "PERMISSION_DENIED: 扩展 {ext_id} 的 network 权限已被用户关闭，无法对外监听"
            ));
        }
    }

    let port = alloc_port(&listen_host, backend.port)?;

    let mut cmd = std::process::Command::new(&node_exe);
    // 不把宿主继承的云凭据、代理认证与 NODE_OPTIONS 等环境泄露给扩展。
    cmd.env_clear();
    for key in ["SystemRoot", "windir", "PATH", "PATHEXT", "TEMP", "TMP", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "COMSPEC"] {
        if let Some(value) = std::env::var_os(key) { cmd.env(key, value); }
    }
    cmd.arg(&entry)
        .current_dir(&cwd)
        .env("PORT", port.to_string())
        .env("XHUB_LISTEN_HOST", &listen_host)
        .env("XHUB_EXT_ID", ext_id)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // 后端输出落盘（`<数据根>/logs/service/<扩展 id>.log`）：后端启动期崩溃时这是唯一能
    // 自证的线索。打不开就退回 null（只记录日志，不阻断启动）
    let log_path = service_log_path(ext_id);
    if let Some(path) = log_path.as_ref() {
        match open_service_log(path) {
            Ok(file) => {
                let cloned = file.try_clone();
                cmd.stdout(std::process::Stdio::from(file));
                match cloned {
                    Ok(f) => {
                        cmd.stderr(std::process::Stdio::from(f));
                    }
                    Err(e) => log::warn!("后端 stderr 落盘失败（{ext_id}）：{e}"),
                }
            }
            Err(e) => log::warn!("后端日志文件打开失败（{ext_id}）：{e}（后端输出将被丢弃）"),
        }
    }
    cmd.no_console_window();

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("启动后端失败: {e}"));
        }
    };

    // 先以 ready=false 入库并立即返回端口：防火墙放行与探活（最长 10s）
    // 都不占用命令线程（read_extension_entry 懒启动路径），后台线程完成后回填 ready
    {
        let mut map = state.0.lock().map_err(|e| e.to_string())?;
        map.insert(
            ext_id.to_string(),
            ServiceRuntime {
                port,
                ready: false,
                child: Some(child),
            },
        );
    }

    let bg_app = app.clone();
    let bg_ext = ext_id.to_string();
    let bg_host = listen_host.clone();
    let bg_program = node_exe.to_string_lossy().to_string();
    let bg_log = log_path.clone();
    std::thread::spawn(move || {
        // 对外监听时放行 Windows 防火墙（非对外不触碰，避免无谓 UAC 提示）
        if external {
            ensure_firewall_rule(&bg_ext, port, &bg_program);
        }
        let ready = probe_ready(port, &bg_host, Duration::from_secs(10));
        // 探活失败时顺手取子进程退出码：后端在模块初始化阶段就崩时这是最快的判据
        let mut exit_code = None;
        if let Ok(mut map) = bg_app.state::<ServiceState>().0.lock() {
            if let Some(rt) = map.get_mut(&bg_ext) {
                if !ready {
                    exit_code = rt
                        .child
                        .as_mut()
                        .and_then(|c| c.try_wait().ok().flatten())
                        .map(|s| s.code());
                }
                rt.ready = ready;
            }
        }
        log::info!("service 探活完成: {bg_ext} port={port} ready={ready}");
        // 未就绪时把「后端自己说了什么」带进宿主日志：本类故障的自证线索全在这里
        if !ready {
            let log_desc = bg_log
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "（未落盘）".to_string());
            let tail = bg_log
                .as_ref()
                .map(|p| tail_text(p, 20))
                .unwrap_or_default();
            let tail = tail.trim();
            if tail.is_empty() {
                log::warn!("service 后端未就绪: {bg_ext} port={port} exit={exit_code:?} 日志={log_desc}");
            } else {
                log::warn!(
                    "service 后端未就绪: {bg_ext} port={port} exit={exit_code:?} 日志={log_desc}\n{tail}"
                );
            }
        }
    });

    log::info!("service 扩展已启动: {ext_id} port={port}（防火墙/探活后台进行中）");
    Ok(port)
}

/// 停止并清理 service 扩展后端进程（卸载 / 宿主退出时调用）
pub fn stop_service(app: &tauri::AppHandle, ext_id: &str) {
    if let Some(proxy) = app.try_state::<crate::proxy::ProxyState>() { proxy.revoke(ext_id); }
    let mut rt = match app.state::<ServiceState>().0.lock() {
        Ok(mut map) => map.remove(ext_id),
        Err(_) => return,
    };
    if let Some(child) = rt.as_mut().and_then(|r| r.child.take()) {
        kill_and_reap(child);
    }
    // 同步移除对外监听放行的防火墙规则，避免规则永久残留
    remove_firewall_rule(ext_id);
    log::info!("service 扩展已停止: {ext_id}");
}

/// kill + 有界回收：TerminateProcess 后子进程应立即终止，但为防异常进程把
/// 宿主退出流程拖死，轮询 try_wait 至多 1s，超时则放弃（进程句柄由系统回收）
fn kill_and_reap(mut child: std::process::Child) {
    let _ = child.kill();
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => break,
        }
    }
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
            if let Some(child) = rt.child.take() {
                kill_and_reap(child);
            }
        }
        remove_firewall_rule(&id);
    }
    log::info!("宿主退出，已停止所有 service 后端进程");
}

/// 已启动 service 的端口（未启动返回 None）
pub fn service_port(app: &tauri::AppHandle, ext_id: &str) -> Option<u16> {
    let state = app.state::<ServiceState>();
    let map = state.0.lock().ok()?;
    map.get(ext_id).map(|rt| rt.port)
}

/// 防火墙规则名（不含端口：同扩展重启换端口时先删后加幂等覆盖，停止时按名可删）
fn firewall_rule_name(ext_id: &str) -> String {
    format!("x-hub extension {ext_id}")
}

/// Windows 防火墙「入站放行」规则管理：走 COM（`INetFwPolicy2`），不再拉起 `netsh.exe`。
///
/// 2026-09-30 改：旧实现 `Command::new("netsh")` 增删规则，开机自启 / 关机退出时恰好赶上
/// 控制台子系统尚未就绪或正在拆除，`netsh.exe` 以 `0xC0000142`（初始化失败）弹系统错误框；
/// 且非管理员时 netsh 加规则必然失败。改 COM 后：无子进程、无控制台、不再有该弹窗。
#[cfg(target_os = "windows")]
mod firewall_com {
    use windows::core::{BSTR, IUnknown};
    use windows::Win32::Foundation::VARIANT_TRUE;
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NetFwPolicy2, NetFwRule, NET_FW_ACTION_ALLOW,
        NET_FW_IP_PROTOCOL_TCP, NET_FW_PROFILE2_PRIVATE, NET_FW_RULE_DIR_IN,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };

    /// 规则不存在时 `Remove` 会报 HRESULT `0x80070002`（ERROR_FILE_NOT_FOUND）——与删成功同义
    const HRESULT_FILE_NOT_FOUND: i32 = 0x8007_0002u32 as i32;

    /// 进入本线程的 COM 单元；返回是否需要配对 `CoUninitialize`。
    /// `RPC_E_CHANGED_MODE`（线程已在别的单元，如 MTA）复用现成单元、不配对。
    fn enter_com() -> bool {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() }
    }

    fn with_policy<T>(
        f: impl FnOnce(&INetFwPolicy2) -> windows::core::Result<T>,
    ) -> windows::core::Result<T> {
        let entered = enter_com();
        let result = (|| {
            let policy: INetFwPolicy2 =
                unsafe { CoCreateInstance(&NetFwPolicy2, None::<&IUnknown>, CLSCTX_INPROC_SERVER)? };
            f(&policy)
        })();
        if entered {
            unsafe { CoUninitialize() };
        }
        result
    }

    pub fn add_rule(rule_name: &str, port: u16, program: &str) -> windows::core::Result<()> {
        with_policy(|policy| {
            let rules = unsafe { policy.Rules()? };
            let name = BSTR::from(rule_name);
            // 幂等：同名先删后加（动态端口每次启动都新增会无限累积）
            let _ = unsafe { rules.Remove(&name) };
            let rule: INetFwRule =
                unsafe { CoCreateInstance(&NetFwRule, None::<&IUnknown>, CLSCTX_INPROC_SERVER)? };
            unsafe {
                rule.SetName(&name)?;
                rule.SetDescription(&BSTR::from(
                    "x-hub service 扩展对外监听放行（自动管理，可随时删除）",
                ))?;
                rule.SetDirection(NET_FW_RULE_DIR_IN)?;
                rule.SetAction(NET_FW_ACTION_ALLOW)?;
                rule.SetProtocol(NET_FW_IP_PROTOCOL_TCP.0)?;
                rule.SetLocalPorts(&BSTR::from(port.to_string()))?;
                rule.SetApplicationName(&BSTR::from(program))?;
                rule.SetProfiles(NET_FW_PROFILE2_PRIVATE.0)?;
                rule.SetEnabled(VARIANT_TRUE)?;
                rules.Add(&rule)?;
            }
            Ok(())
        })
    }

    pub fn remove_rule(rule_name: &str) -> windows::core::Result<()> {
        with_policy(|policy| {
            let rules = unsafe { policy.Rules()? };
            match unsafe { rules.Remove(&BSTR::from(rule_name)) } {
                Ok(()) => Ok(()),
                Err(e) if e.code().0 == HRESULT_FILE_NOT_FOUND => Ok(()),
                Err(e) => Err(e),
            }
        })
    }
}

/// 移除扩展的防火墙放行规则（停止/卸载/启动失败时调用），失败仅记录日志
pub(crate) fn remove_firewall_rule(ext_id: &str) {
    #[cfg(target_os = "windows")]
    {
        let rule_name = firewall_rule_name(ext_id);
        match firewall_com::remove_rule(&rule_name) {
            Ok(()) => log::info!("已移除防火墙规则: {rule_name}"),
            Err(e) => log::warn!("移除防火墙规则失败（{rule_name}）：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ext_id;
    }
}

/// 为对外监听的 service 扩展放行 Windows 防火墙（入站 TCP，按端口 + 程序定向）。
/// 失败不阻塞启动：仅记录日志（可能因非管理员权限无法写入，提示用户手动放行）。
fn ensure_firewall_rule(ext_id: &str, port: u16, program: &str) {
    #[cfg(target_os = "windows")]
    {
        let rule_name = firewall_rule_name(ext_id);
        match firewall_com::add_rule(&rule_name, port, program) {
            Ok(()) => log::info!("已放行防火墙: {rule_name} (tcp {port}, program={program})"),
            Err(e) => log::warn!(
                "防火墙放行失败（{ext_id} 端口 {port}）：{e}。如需局域网访问请手动放行该端口。"
            ),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (ext_id, port, program);
    }
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

    fn backend(entry: &str, cwd: Option<&str>) -> BackendSpec {
        BackendSpec {
            entry: entry.to_string(),
            engine: None,
            cwd: cwd.map(|c| c.to_string()),
            port: None,
            host: None,
            health: None,
        }
    }

    /// 回归：Node 的 CJS 加载器读不了 `\\?\` 前缀的脚本路径（后端静默 exit 1 的根因），
    /// 所以喂给 Node 的 entry 必须是普通路径——哪怕目录来自 canonicalize。
    #[test]
    fn backend_paths_strip_verbatim_prefix() {
        if !cfg!(windows) {
            let (entry, cwd) = backend_paths(std::path::Path::new("/tmp/ext"), &backend("backend/server.js", None));
            assert_eq!(entry, PathBuf::from("/tmp/ext/backend/server.js"));
            assert_eq!(cwd, PathBuf::from("/tmp/ext"));
            return;
        }
        let dev_dir = std::path::Path::new(r"\\?\A:\x-hub\publish-src\1.3.0\lan-share");
        let (entry, cwd) = backend_paths(dev_dir, &backend("backend/server.js", None));
        assert_eq!(
            entry,
            PathBuf::from(r"A:\x-hub\publish-src\1.3.0\lan-share\backend\server.js")
        );
        assert_eq!(cwd, PathBuf::from(r"A:\x-hub\publish-src\1.3.0\lan-share"));
        assert!(!entry.to_string_lossy().contains(r"\\?\"));

        // manifest 显式声明 cwd 时同样归一
        let (_, cwd) = backend_paths(
            std::path::Path::new(r"\\?\A:\ext"),
            &backend("server.js", Some("backend")),
        );
        assert_eq!(cwd, PathBuf::from(r"A:\ext\backend"));
    }

    #[test]
    fn service_log_file_name_is_path_safe() {
        assert_eq!(
            service_log_file_name("com.dept.lan-share"),
            "com.dept.lan-share.log"
        );
        // 任何越界字符都被替换：日志文件名绝不能拼出路径穿越
        let evil = service_log_file_name("..\\..\\evil");
        assert!(!evil.contains('/') && !evil.contains('\\'));
        assert!(evil.ends_with(".log"));
    }

    #[test]
    fn tail_text_keeps_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("svc.log");
        std::fs::write(&path, "l1\nl2\nl3\nl4\n").unwrap();
        assert_eq!(tail_text(&path, 2), "l3\nl4");
        assert_eq!(tail_text(&path, 99), "l1\nl2\nl3\nl4");
        // 文件不存在时静默返回空串（探活失败路径不该因日志读取再报错）
        assert_eq!(tail_text(&dir.path().join("nope.log"), 5), "");
    }

    /// 端到端回归（模拟「我的扩展 → 添加源码目录」的真实链路）：目录 canonicalize 得到
    /// verbatim 形式，经 `backend_paths` 归一后交给**真实 Node** 执行——脚本必须跑得起来。
    /// 这正是本 bug 的现场：修复前喂给 Node 的是 `\\?\A:\…\server.js`，Node 把路径拆错
    /// （`EISDIR: lstat 'A:'`）后 exit 1，后端在模块初始化阶段就死，宿主侧只见 `ready=false`。
    /// 机器上没有 Node 时跳过（本测试依赖真实运行时）。
    #[test]
    fn backend_entry_runs_under_real_node() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("probe.js"), "console.log('XHUB_PROBE_OK');").unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let (entry, cwd) = backend_paths(&canon, &backend("probe.js", None));
        if cfg!(windows) {
            assert!(
                canon.to_string_lossy().starts_with(r"\\?\"),
                "前提不成立：Windows 的 canonicalize 应当返回 verbatim 形式"
            );
            assert!(!entry.to_string_lossy().starts_with(r"\\?\"));
        }
        let out = match std::process::Command::new("node")
            .arg(&entry)
            .current_dir(&cwd)
            .output()
        {
            Ok(o) => o,
            Err(_) => return, // 无 Node：跳过
        };
        assert!(
            out.status.success(),
            "Node 应能执行归一后的脚本路径 {}：{}",
            entry.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("XHUB_PROBE_OK"));
    }

    #[test]
    fn alloc_port_binds_loopback_and_external() {
        let port = alloc_port("127.0.0.1", None).unwrap();
        assert!(port > 0);
        // 对外监听也能分配
        let ext = alloc_port("0.0.0.0", None).unwrap();
        assert!(ext > 0);
    }

    #[test]
    fn alloc_port_fixed_port_binds_that_port() {
        let p = alloc_port("127.0.0.1", None).unwrap();
        // 占住该端口后，同一端口重复绑定应失败（冲突检测）
        let _guard = std::net::TcpListener::bind(("127.0.0.1", p)).unwrap();
        assert!(alloc_port("127.0.0.1", Some(p)).is_err());
    }

    #[test]
    fn backend_spec_external_detection() {
        let mk = |host: Option<String>| crate::extension::BackendSpec {
            entry: "s.js".into(),
            engine: None,
            cwd: None,
            port: None,
            host,
            health: None,
        };
        assert!(!mk(None).is_external());
        assert!(!mk(Some("127.0.0.1".into())).is_external());
        assert!(!mk(Some("localhost".into())).is_external());
        assert!(mk(Some("0.0.0.0".into())).is_external());
        assert!(mk(Some("192.168.1.5".into())).is_external());
        assert_eq!(mk(None).listen_host(), "127.0.0.1");
        assert_eq!(mk(Some("0.0.0.0".into())).listen_host(), "0.0.0.0");
    }
}
