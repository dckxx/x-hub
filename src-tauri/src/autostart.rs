//! 开机自启动管理：注册表 Run 键方式（登录时静默拉起，主窗不弹出、驻留托盘）。
//!
//! 历史版本曾提供「计划任务 + 最高权限」的管理员启动模式；因 Windows UIPI 隔离，
//! 管理员权限进程无法从资源管理器接收文件拖放（速达拖拽导入失效），该模式已移除。
//! `apply` 时会顺带清理旧版残留的计划任务。

use std::process::Command;

/// 自启动在命令行里追加的隐藏启动参数（主窗不弹出、直接驻留托盘）
pub const HIDDEN_ARG: &str = "--autostart-hidden";

/// 判断本次进程是否由自启动拉起（命令行含 HIDDEN_ARG）。
/// 供前端决定是否主动显示主窗口：自启动时不打扰用户，直接驻留托盘。
pub fn is_hidden_launch() -> bool {
    std::env::args().any(|a| a == HIDDEN_ARG)
}

// ---- Windows 常量 ----
#[cfg(target_os = "windows")]
const RUN_KEY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_VALUE_NAME: &str = "x-hub";
/// 旧版管理员自启动注册的计划任务名（仅用于清理，不再创建）
#[cfg(target_os = "windows")]
const LEGACY_TASK_NAME: &str = "x-hub-autostart";

/// 当前 exe 的完整路径（Run 键需要绝对路径）
#[cfg(target_os = "windows")]
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 自启动命令行：`"C:\...\x-hub.exe" --autostart-hidden`
#[cfg(target_os = "windows")]
fn launch_command_line() -> String {
    format!("\"{}\" {}", exe_path(), HIDDEN_ARG)
}

/// 隐藏控制台窗口（reg / schtasks 是命令行工具，避免闪黑框）
#[cfg(target_os = "windows")]
fn no_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

// ---------- 普通模式：注册表 Run 键 ----------

#[cfg(target_os = "windows")]
fn write_run_key() -> Result<(), String> {
    let mut cmd = Command::new("reg");
    cmd.args([
        "add",
        RUN_KEY_PATH,
        "/v",
        RUN_VALUE_NAME,
        "/t",
        "REG_SZ",
        "/d",
        &launch_command_line(),
        "/f",
    ]);
    no_console_window(&mut cmd);
    let status = cmd.status().map_err(|e| format!("reg add 失败: {e}"))?;
    if status.success() {
        log::info!("已写入开机自启动 Run 键");
        Ok(())
    } else {
        Err(format!("reg add 退出码 {}", status.code().unwrap_or(-1)))
    }
}

#[cfg(target_os = "windows")]
fn remove_run_key() {
    let mut cmd = Command::new("reg");
    cmd.args(["delete", RUN_KEY_PATH, "/v", RUN_VALUE_NAME, "/f"]);
    no_console_window(&mut cmd);
    let _ = cmd.status();
}

// ---------- 旧版管理员模式残留清理 ----------

/// 隐藏控制台地运行一行 cmd 命令，返回是否成功
#[cfg(target_os = "windows")]
fn run_cmd_line(line: &str) -> bool {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", line]);
    no_console_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 提权运行临时 bat（触发一次 UAC 授权），等待完成
#[cfg(target_os = "windows")]
fn run_bat_elevated(bat_path: &std::path::Path, log_tag: &str) -> bool {
    let script = format!(
        "Start-Process -Wait -Verb RunAs -WindowStyle Hidden -FilePath \"{}\"",
        bat_path.to_string_lossy()
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    no_console_window(&mut cmd);
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    if ok {
        log::info!("[自启动] {} 提权操作完成", log_tag);
    } else {
        log::warn!("[自启动] {} 提权操作被取消或失败", log_tag);
    }
    ok
}

/// 旧版最高权限计划任务是否仍存在（schtasks /Query 退出码 0 表示存在）
#[cfg(target_os = "windows")]
fn legacy_task_exists() -> bool {
    let mut cmd = Command::new("schtasks");
    cmd.args(["/Query", "/TN", LEGACY_TASK_NAME]);
    no_console_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 清理旧版管理员自启动残留的计划任务（最高权限任务普通权限删不动时走一次 UAC 授权）
#[cfg(target_os = "windows")]
fn remove_legacy_task() {
    if !legacy_task_exists() {
        return;
    }
    let line = format!("schtasks /Delete /TN \"{}\" /F", LEGACY_TASK_NAME);
    if run_cmd_line(&line) {
        log::info!("[自启动] 已清理旧版管理员自启动计划任务");
        return;
    }
    let bat = std::env::temp_dir().join("x-hub-autostart-task-del.bat");
    if std::fs::write(&bat, &line).is_ok() && run_bat_elevated(&bat, "清理旧版自启动任务") {
        log::info!("[自启动] 已通过 UAC 授权清理旧版管理员自启动计划任务");
    }
    let _ = std::fs::remove_file(&bat);
}

// ---------- 对外接口 ----------

/// 应用自启动开关：先清掉已有注册（Run 键 + 旧版任务残留），启用时写入 Run 键。
pub fn apply(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        remove_run_key();
        remove_legacy_task();
        if enabled {
            write_run_key()?;
        }
        Ok(())
    }
    // 非 Windows 平台：自启动仅支持当前平台时返回错误提示
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Err("当前平台不支持开机自启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_line_contains_hidden() {
        // 命令行必须始终带隐藏参数（自启动时不打扰用户，驻留托盘）
        #[cfg(target_os = "windows")]
        {
            let cli = launch_command_line();
            assert!(cli.contains(HIDDEN_ARG));
            // 路径带空格时必须有引号包裹
            assert!(cli.starts_with('"'));
        }
    }

    // 注意：不要在测试里调用 apply()——它会真实操作系统注册表与计划任务，
    // cargo test 会把本机已注册的自启动 Run 键删掉。
}
