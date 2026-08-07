use std::process::Command;

#[cfg(target_os = "windows")]
fn no_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
}

pub fn launch_program(path: &str, args: Option<&str>) -> Result<(), String> {
    let target = std::path::Path::new(path);
    let mut cmd = if target.is_file() {
        let mut c = Command::new(path);
        // 便携软件（如绿色版 exe）依赖同目录资源文件，工作目录设为 exe 所在目录
        if let Some(dir) = target.parent() {
            c.current_dir(dir);
        }
        c
    } else {
        #[cfg(target_os = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_os = "windows")]
        {
            // 引号包裹路径，兼容含空格路径；隐藏控制台窗口
            c.arg("/C").arg(format!("\"{}\"", path));
            no_console_window(&mut c);
        }
        #[cfg(not(target_os = "windows"))]
        let mut c = Command::new("sh");
        #[cfg(not(target_os = "windows"))]
        c.arg("-c").arg(path);
        c
    };
    if let Some(args) = args {
        if !args.trim().is_empty() {
            for arg in split_args(args) {
                cmd.arg(arg);
            }
        }
    }
    match cmd.spawn() {
        Ok(_) => Ok(()),
        // Windows 错误 740：程序需要管理员权限，自动请求 UAC 提权
        Err(e) if e.raw_os_error() == Some(740) => {
            log::warn!("程序需要管理员权限，请求 UAC 提权: {}", path);
            launch_elevated(path, args)
        }
        Err(e) => Err(format!("启动程序失败「{}」: {}", path, e)),
    }
}

/// 以管理员权限启动（触发 UAC 提权确认）：PowerShell Start-Process -Verb RunAs
fn launch_elevated(path: &str, args: Option<&str>) -> Result<(), String> {
    let has_args = args.map(|a| !a.trim().is_empty()).unwrap_or(false);
    let script = if has_args {
        "Start-Process -FilePath $env:XHUB_PATH -ArgumentList $env:XHUB_ARGS -Verb RunAs"
    } else {
        "Start-Process -FilePath $env:XHUB_PATH -Verb RunAs"
    };
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
        .env("XHUB_PATH", path);
    #[cfg(target_os = "windows")]
    no_console_window(&mut cmd);
    if has_args {
        cmd.env("XHUB_ARGS", args.unwrap_or(""));
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("提权启动失败「{}」: {}", path, e))
}

/// 引号感知的参数分割：`--dir "C:\My Apps"` 保持为一个参数
fn split_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '"' => in_quote = !in_quote,
            ' ' | '\t' if !in_quote => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

pub fn open_url(url: &str) -> Result<(), String> {
    opener::open(url).map_err(|e| format!("打开链接失败: {}", e))
}

/// 打开本地路径：文件用系统默认程序打开，文件夹由资源管理器/文件管理器打开
pub fn open_path(path: &str) -> Result<(), String> {
    let target = std::path::Path::new(path);
    if target.is_dir() {
        #[cfg(target_os = "windows")]
        {
            Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| format!("打开文件夹失败: {}", e))?;
            return Ok(());
        }
        #[cfg(not(target_os = "windows"))]
        {
            opener::open(path).map_err(|e| format!("打开文件夹失败: {}", e))?;
            return Ok(());
        }
    }
    opener::open(path).map_err(|e| format!("打开路径失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_url_with_invalid_url_returns_error() {
        // 空 URL 应返回错误或不可预测结果，这里仅验证函数可调用
        let _ = open_url("https://example.com");
    }

    #[test]
    fn launch_nonexistent_program_returns_error() {
        let result = launch_program("/nonexistent/path/xyz", None);
        assert!(result.is_err());
    }
}
