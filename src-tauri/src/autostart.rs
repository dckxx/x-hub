//! 开机自启动管理：支持「普通」与「以管理员身份」两种启动方式。
//!
//! - 普通模式：写注册表 Run 键 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! - 管理员模式：注册计划任务（ONLOGON + 最高权限），登录后静默提权启动，
//!   避免普通 Run 键在需要管理员权限时被 UAC 拦下。因最高权限任务需以管理员身份
//!   注册，首次启用会触发一次 UAC 授权（随后登录静默，不再弹窗）。
//!
//! 两种方式互斥：切换时会先清掉另一套，防止重复启动；关闭时两套都清理。

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
#[cfg(target_os = "windows")]
const TASK_NAME: &str = "x-hub-autostart";

/// 当前 exe 的完整路径（Run 键 / 计划任务都需要绝对路径）
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

// ---------- 管理员模式：计划任务（可提权注册） ----------

/// 构建 schtasks /Create 完整命令行（写进临时 bat 交给提权进程执行，
/// 避免经 PowerShell ArgumentList 时引号/空格被二次解析）
#[cfg(target_os = "windows")]
fn schtasks_create_line() -> String {
    format!(
        "schtasks /Create /TN \"{}\" /TR \"{}\" /SC ONLOGON /RL HIGHEST /F",
        TASK_NAME,
        launch_command_line()
    )
}

/// 直接运行已构造好的命令行（cmd /C，隐藏控制台）
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

/// 任务是否已存在（schtasks /Query 退出码 0 表示存在）
#[cfg(target_os = "windows")]
fn task_exists() -> bool {
    let mut cmd = Command::new("schtasks");
    cmd.args(["/Query", "/TN", TASK_NAME]);
    no_console_window(&mut cmd);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// 注册最高权限计划任务：先直接尝试（适用于宿主已提权），
/// 失败则写入临时 bat 并请求 UAC 提权执行；最后校验任务是否真正建立。
#[cfg(target_os = "windows")]
fn write_scheduled_task() -> Result<(), String> {
    let line = schtasks_create_line();
    if run_cmd_line(&line) && task_exists() {
        log::info!("已注册开机自启动计划任务（最高权限）");
        return Ok(());
    }

    // 直接失败（通常因权限不足 /RL HIGHEST）：走 UAC 提权
    let bat = std::env::temp_dir().join("x-hub-autostart-task.bat");
    if std::fs::write(&bat, line).is_err() {
        return Err("无法写入临时提权脚本".into());
    }
    let elevated_ok = run_bat_elevated(&bat, "创建计划任务");
    let _ = std::fs::remove_file(&bat);
    if elevated_ok && task_exists() {
        log::info!("已通过 UAC 授权注册最高权限自启动任务");
        Ok(())
    } else {
        Err((if elevated_ok {
            "提权完成但任务校验失败"
        } else {
            "创建管理员自启动需要授权：请在弹窗中点击「是」"
        })
        .into())
    }
}

#[cfg(target_os = "windows")]
fn remove_scheduled_task() {
    // 普通创建的任务直接删；以管理员身份创建的最高权限任务，删除同样需要提权
    if task_exists() {
        let line = format!("schtasks /Delete /TN \"{}\" /F", TASK_NAME);
        if !run_cmd_line(&line) {
            let bat = std::env::temp_dir().join("x-hub-autostart-task-del.bat");
            let _ = std::fs::write(&bat, line);
            let _ = run_bat_elevated(&bat, "删除计划任务");
            let _ = std::fs::remove_file(&bat);
        }
    }
}

// ---------- 对外接口 ----------

/// 应用自启动状态：`enabled` 总开关；`admin` 是否以管理员身份启动（仅启用时生效）。
///
/// 先清掉当前存在的两套注册（普通 + 管理员），再按需写入目标方式，
/// 保证任何时候至多存在一种启动方式。
pub fn apply(enabled: bool, admin: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        remove_run_key();
        remove_scheduled_task();
        if enabled {
            if admin {
                write_scheduled_task()?
            } else {
                write_run_key()?
            }
        }
        Ok(())
    }
    // 非 Windows 平台：自启动仅支持当前平台时返回错误提示
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, admin);
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

    #[test]
    fn apply_disabled_is_ok() {
        // 关闭自启动不应报错（静默清理）
        let _ = apply(false, false);
    }
}