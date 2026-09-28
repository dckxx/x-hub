use std::process::Command;

/// 让子进程不弹控制台窗口（链式：`Command::new("x").no_console_window()`）。
///
/// 宿主是 GUI 子系统进程（`main.rs` 的 `windows_subsystem = "windows"`），自身没有控制台；
/// 此时拉起控制台子系统程序（`node.exe` / `cmd` / `powershell` / `netsh` / `reg`）若不带
/// `CREATE_NO_WINDOW`，Windows 会**为子进程新建一个控制台窗口**——表现为「闪一下黑窗」。
///
/// **全工程唯一实现**：`autostart` / `runtime` / `service` / `commands` 都从这里取，
/// 新增子进程调用点直接挂 `.no_console_window()`，不要再抄一份 `creation_flags(0x08000000)`。
pub(crate) trait NoConsoleWindow {
    fn no_console_window(&mut self) -> &mut Self;
}

#[cfg(target_os = "windows")]
impl NoConsoleWindow for Command {
    fn no_console_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x08000000) // CREATE_NO_WINDOW
    }
}

#[cfg(not(target_os = "windows"))]
impl NoConsoleWindow for Command {
    fn no_console_window(&mut self) -> &mut Self {
        self
    }
}

fn looks_like_fs_path(path: &str) -> bool {
    path.contains('/') || path.contains('\\') || path.starts_with('~')
}

pub fn launch_program(path: &str, args: Option<&str>) -> Result<(), String> {
    let target = std::path::Path::new(path);
    let mut cmd = if target.is_file() {
        let mut c = Command::new(path);
        // 便携软件（如绿色版 exe）依赖同目录资源文件，工作目录设为 exe 所在目录
        if let Some(dir) = target.parent() {
            c.current_dir(dir);
        }
        // 宿主是 GUI 子系统进程（无控制台），直接启动 CLI 工具/bat 时若不指定
        // CREATE_NO_WINDOW，Windows 会为子进程新建控制台窗口（闪黑窗）
        c.no_console_window();
        c
    } else if looks_like_fs_path(path) {
        // 带路径分隔符却不是文件：不要丢给 cmd / sh——外壳总能 spawn 成功，
        // 调用方会以为启动成功，CI 在 Linux 上也会误判。
        return Err(format!("启动程序失败「{}」: 文件不存在", path));
    } else {
        #[cfg(target_os = "windows")]
        let mut c = Command::new("cmd");
        #[cfg(target_os = "windows")]
        {
            // 引号包裹路径，兼容含空格路径；隐藏控制台窗口
            c.arg("/C").arg(format!("\"{}\"", path));
            c.no_console_window();
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

/// 以管理员权限启动。
/// Windows：PowerShell Start-Process -Verb RunAs；Linux：pkexec。
fn launch_elevated(path: &str, args: Option<&str>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let has_args = args.map(|a| !a.trim().is_empty()).unwrap_or(false);
        let script = if has_args {
            "Start-Process -FilePath $env:XHUB_PATH -ArgumentList $env:XHUB_ARGS -Verb RunAs"
        } else {
            "Start-Process -FilePath $env:XHUB_PATH -Verb RunAs"
        };
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
            .env("XHUB_PATH", path);
        cmd.no_console_window();
        if has_args {
            cmd.env("XHUB_ARGS", args.unwrap_or(""));
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("提权启动失败「{}」: {}", path, e))
    }
    #[cfg(target_os = "linux")]
    {
        let mut cmd = std::process::Command::new("pkexec");
        cmd.arg(path);
        if let Some(args) = args {
            if !args.trim().is_empty() {
                for arg in split_args(args) {
                    cmd.arg(arg);
                }
            }
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("提权启动失败「{}」: {}（需要 pkexec）", path, e))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (path, args);
        Err("当前平台不支持提权启动".into())
    }
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

/// 用指定浏览器打开 URL（browser_exe 必须存在；URL 仅放行 http/https，由调用方校验）
pub fn open_with_browser(browser_exe: &str, url: &str) -> Result<(), String> {
    let path = std::path::Path::new(browser_exe);
    if !path.is_file() {
        return Err(format!("浏览器不存在: {}", browser_exe));
    }
    // 浏览器已经在运行：先把它的窗口调度到前台。主流浏览器收到 URL 会复用现有实例、
    // 以新标签页打开，这里再置前一次，避免用户以为「点了没反应」。
    let _ = activate_existing(browser_exe);
    let mut cmd = Command::new(path);
    cmd.arg(url);
    cmd.no_console_window();
    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("启动浏览器失败「{}」: {}", browser_exe, e)),
    }
}

/// 若已有与 `exe_path` 同名的进程在运行，把它的顶层窗口还原并调度到前台，返回 true。
///
/// 用途：速达里点击一个**已经在运行**的应用/浏览器时，不再拉起第二个实例（很多程序不自己
/// 复用实例，会再开一个窗口/进程），而是把已有窗口直接拉出来。
///
/// 托盘类应用（微信/QQ 等）点「关闭」只是把主窗口**隐藏**起来（常带 `WS_EX_TOOLWINDOW`），
/// 所以窗口搜索不要求可见、也不排除工具窗口，隐藏窗口用 `SW_SHOW` 拉出来。
///
/// 只有「目标确实是一个可执行文件、且找得到一个像主窗口的顶层窗口」才返回 true；进程没在
/// 跑、或主窗口已被销毁（只剩托盘图标，典型如部分 IM）时返回 false，调用方应照常启动——
/// 这类程序自带单实例逻辑，再启动一次就会把已有窗口唤出来。非 Windows 平台恒返回 false。
pub fn activate_existing(exe_path: &str) -> bool {
    if !std::path::Path::new(exe_path).is_file() {
        return false;
    }
    match exe_file_name(exe_path) {
        Some(name) => activate_existing_by_name(&name),
        None => false,
    }
}

/// 取可执行文件名（按进程名匹配用；输入可以是完整路径）。
/// 同时认 `/` 与 `\`：配置里可能是 Windows 路径，Linux 上 `Path::file_name` 不会切反斜杠。
fn exe_file_name(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(target_os = "windows")]
fn activate_existing_by_name(exe_name: &str) -> bool {
    use sysinfo::{ProcessesToUpdate, System};

    // 同名进程可能有一堆（浏览器的渲染/GPU 子进程等）：先收集 PID，再看谁的窗口是主窗口
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let self_pid = std::process::id();
    let mut pids: Vec<u32> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| {
            pid.as_u32() != self_pid && p.name().to_string_lossy().eq_ignore_ascii_case(exe_name)
        })
        .map(|(pid, _)| pid.as_u32())
        .collect();
    pids.sort_unstable();
    pids.dedup();
    if pids.is_empty() {
        return false;
    }
    focus_windows_of(&pids)
}

/// 在给定 PID 集合里挑一个最像「主窗口」的顶层窗口并调度到前台。
///
/// 托盘应用的主窗口被隐藏时仍然存在，所以不要求可见、也不排除工具窗口，改为按
/// 「有标题 + 面积」打分：可见 ≫ 非工具窗口 ≫ 面积大。无标题、零面积、`WS_EX_NOACTIVATE`
/// 的窗口（IPC/消息窗、提示气泡）直接排除，避免把辅助窗拉出来。
#[cfg(target_os = "windows")]
fn focus_windows_of(pids: &[u32]) -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE,
        SW_MINIMIZE, SW_RESTORE, SW_SHOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    struct Ctx {
        pids: Vec<u32>,
        best: Option<(HWND, i64)>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if !ctx.pids.contains(&pid) {
            return TRUE;
        }
        // 无标题的顶层窗口几乎都是隐藏的辅助窗（IPC/消息窗），不是主窗口
        if GetWindowTextLengthW(hwnd) <= 0 {
            return TRUE;
        }
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        // 无激活浮层（提示气泡一类）不抢焦点，也不该被当成主窗口
        if ex_style & WS_EX_NOACTIVATE != 0 {
            return TRUE;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return TRUE;
        }
        let area = (rect.right - rect.left).max(0) as i64 * (rect.bottom - rect.top).max(0) as i64;
        if area <= 0 {
            return TRUE;
        }
        // 量级差保证优先级：可见 > 隐藏，非工具窗 > 工具窗（托盘主窗常是工具窗，兜底仍可用）
        let mut score = area;
        if IsWindowVisible(hwnd) != 0 {
            score += 1_000_000_000;
        }
        if ex_style & WS_EX_TOOLWINDOW == 0 {
            score += 100_000_000;
        }
        if ctx.best.map_or(true, |(_, s)| score > s) {
            ctx.best = Some((hwnd, score));
        }
        TRUE
    }

    let mut ctx = Ctx {
        pids: pids.to_vec(),
        best: None,
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut ctx as *mut Ctx as LPARAM);
    }
    let Some((hwnd, _)) = ctx.best else {
        return false;
    };
    unsafe {
        // 隐藏窗口（托盘应用）要 SW_SHOW 才会露出来；最小化的窗口靠 SW_RESTORE 还原
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        if SetForegroundWindow(hwnd) == 0 {
            // 非前台进程调用会被系统拒绝：最小化再还原一次是通行的绕行写法
            ShowWindow(hwnd, SW_MINIMIZE);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
    true
}

#[cfg(not(target_os = "windows"))]
fn activate_existing_by_name(_exe_name: &str) -> bool {
    false
}

/// 打开外部链接（仅供前端调用的安全命令：只放行 http/https，防止任意 scheme 注入）。
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("只能打开 http/https 链接".to_string());
    }
    open_url(&url)
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
    fn launch_nonexistent_program_returns_error() {
        // 带路径分隔符且不是文件 → 所有平台都应直接失败（不再丢给 cmd/sh）
        let result = launch_program("/nonexistent/path/xyz", None);
        assert!(result.is_err());
        let result = launch_program(r"C:\nonexistent\app.exe", None);
        assert!(result.is_err());
    }

    #[test]
    fn exe_file_name_extracts_basename() {
        assert_eq!(
            exe_file_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe").as_deref(),
            Some("chrome.exe")
        );
        assert_eq!(exe_file_name("/usr/bin/firefox").as_deref(), Some("firefox"));
        assert_eq!(exe_file_name("chrome.exe").as_deref(), Some("chrome.exe"));
        assert_eq!(exe_file_name(""), None);
    }

    #[test]
    fn activate_existing_ignores_non_file_targets() {
        // 目标不是文件（命令行 / URL）时不该去匹配进程，恒 false
        assert!(!activate_existing("not-a-real-file-xyz"));
        assert!(!activate_existing("https://example.com"));
    }
}
