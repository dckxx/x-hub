//! Windows 系统通知（真 Toast：右下角横幅 + 操作中心）
//!
//! 历史演进：便携版（无安装器）下 tauri-plugin-notification 的 toast 需要已注册的
//! AUMID，否则静默失败，项目曾改用 Win32 `Shell_NotifyIconW` 托盘气泡。但 Win11
//! 24H2+（build 26200 实测）不再把气泡转成系统通知——NIM_ADD / NIM_MODIFY 均返回
//! 成功却不显示，气泡路线整体失效。
//! 现回归 tauri-plugin-notification 真 Toast，并在首次发通知前把应用 AUMID 注册进
//! `HKCU\Software\Classes\AppUserModelId`（用户级注册表、免安装器、便携版可用）。
//! 注册失败不影响弹出：dev 构建（target 目录内）插件自动回退 PowerShell AUMID。

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// AUMID 键路径：与 tauri.conf.json 的 identifier（"x-hub"）一致，
/// 插件在非 target 目录运行时以此身份调用 CreateToastNotifierWithId
#[cfg(windows)]
const AUMID_KEY_PATH: &str = r"HKCU\Software\Classes\AppUserModelId\x-hub";
/// 通知横幅里显示的应用名
#[cfg(windows)]
const DISPLAY_NAME: &str = "X-Hub";

/// AUMID 注册成功标记：同一进程只写一次注册表（每条通知都 spawn reg 进程得不偿失；
/// 失败时不置位，下次通知重试）
#[cfg(windows)]
static AUMID_REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 隐藏 reg 命令的控制台窗口（避免闪黑框，同 autostart.rs）
#[cfg(windows)]
fn no_console_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

/// 注册 AUMID（幂等）：键默认值=显示名（通知横幅的应用名），IconUri=exe 路径（通知图标）。
/// portable 版没有安装器写这些，运行时补一次，失败只记日志（插件会回退 PowerShell AUMID）。
#[cfg(windows)]
fn ensure_aumid_registered() {
    use std::process::Command;
    use std::sync::atomic::Ordering;

    if AUMID_REGISTERED.load(Ordering::Relaxed) {
        return;
    }

    let mut cmd = Command::new("reg");
    cmd.args(["add", AUMID_KEY_PATH, "/ve", "/t", "REG_SZ", "/d", DISPLAY_NAME, "/f"]);
    no_console_window(&mut cmd);
    let name_ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);

    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let icon_ok = if exe.is_empty() {
        false
    } else {
        let mut cmd = Command::new("reg");
        cmd.args(["add", AUMID_KEY_PATH, "/v", "IconUri", "/t", "REG_SZ", "/d", &exe, "/f"]);
        no_console_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    };

    if name_ok && icon_ok {
        AUMID_REGISTERED.store(true, Ordering::Relaxed);
    } else {
        // 不阻断发通知：AUMID 缺图标/显示名只影响观感；键完全缺失时 dev 回退 PowerShell AUMID
        log::warn!("AUMID 注册未完全成功: name={name_ok} icon={icon_ok}");
    }
}

/// 显示系统通知（Win10/11 渲染为右下角横幅 + 操作中心系统级通知）。
pub fn show_system_notification(app: &AppHandle, title: &str, body: &str) {
    #[cfg(windows)]
    {
        ensure_aumid_registered();
        match app.notification().builder().title(title).body(body).show() {
            Ok(()) => log::info!("已发送系统通知: {title}"),
            Err(e) => log::error!("系统通知发送失败: {e}"),
        }
    }
    #[cfg(not(windows))]
    let _ = (app, title, body);
}
