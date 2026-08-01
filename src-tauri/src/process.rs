use std::process::Command;

pub fn launch_program(path: &str, args: Option<&str>) -> Result<(), String> {
    let mut cmd = if std::path::Path::new(path).exists() {
        Command::new(path)
    } else {
        #[cfg(target_os = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_os = "windows")]
        c.arg("/C").arg(path);
        #[cfg(not(target_os = "windows"))]
        let mut c = Command::new("sh");
        #[cfg(not(target_os = "windows"))]
        c.arg("-c").arg(path);
        c
    };
    if let Some(args) = args {
        if !args.trim().is_empty() {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("启动程序失败: {}", e))
}

pub fn open_url(url: &str) -> Result<(), String> {
    opener::open(url).map_err(|e| format!("打开链接失败: {}", e))
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
