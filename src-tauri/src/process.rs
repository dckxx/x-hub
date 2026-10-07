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
/// 两条入口：launch_program 撞错误 740（程序清单要求提权）时的自动兜底，
/// 与 launch_resource_as_admin（速达右键「以管理员身份运行」）的用户显式提权。
pub(crate) fn launch_elevated(path: &str, args: Option<&str>) -> Result<(), String> {
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
/// **只处理「可见或最小化」的窗口**。托盘隐藏的主窗口（微信/QQ 等点「关闭」后）不能靠
/// 外部 `SW_SHOW` 拉起——应用内部仍认为窗口是隐藏的：微信 4.x（Qt）拉起后点击无响应、
/// Electron 系（WorkBuddy）拉起后停留在隐藏前的最后一帧不再重绘（用户看到一片空白），
/// 且隐藏态进程里常并存带标题的大面积辅助窗（微信 `WxTrayIconMessageWindow`）极易选错。
/// 所以窗口全部隐藏时返回 false，调用方照常启动 exe——托盘类应用自带的单实例逻辑会把
/// 主窗正规地唤出来（微信/WorkBuddy 均实测通过）。
///
/// 只有「目标确实是一个可执行文件、且找得到一个像主窗口的可见顶层窗口」才返回 true；
/// 非 Windows 平台恒返回 false。
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

/// 候选窗口判定（抽出为纯函数便于回归测试）：**可见**（含最小化——最小化窗口的
/// `WS_VISIBLE` 仍置位）+ 有标题 + 无 `WS_EX_NOACTIVATE` + 面积为正。
///
/// 隐藏窗口一律不是候选：托盘隐藏的主窗不能靠外部 `SW_SHOW` 拉起，应用内部仍认为
/// 窗口是隐藏的——微信 4.x（Qt）拉起后点击无响应，Electron 系（WorkBuddy）拉起后
/// 停留在隐藏前的最后一帧不再重绘（一片空白）；隐藏态还常并存带标题的大面积辅助窗
/// （微信 `WxTrayIconMessageWindow`，实测 1440×753 比真主窗还大），打分极易选错。
/// 隐藏态返回 false 让调用方重启 exe、走应用自带单实例的正规唤起路径（两例均实测通过）。
fn is_focus_candidate(visible: bool, title_len: i32, ex_style: u32, area: i64) -> bool {
    // WS_EX_NOACTIVATE（Win32 固定值）以内联常量代替 windows_sys 导入：本函数是纯判定、
    // 唯一调用方虽在 Windows 门控内，但回归测试不应依赖平台（Linux cargo test 也要能编译）
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    visible && title_len > 0 && ex_style & WS_EX_NOACTIVATE == 0 && area > 0
}

/// 在给定 PID 集合里挑一个**可见（含最小化）**的顶层主窗口并调度到前台。
///
/// 按候选判定 [`is_focus_candidate`] 过滤后打分：可见 ≫ 非工具窗口 ≫ 面积大。
/// 一个可见候选都没有（进程在跑但窗口全隐藏，典型如托盘挂后台）时返回 false，
/// 调用方照常启动 exe 让应用自己的单实例逻辑唤起主窗。
#[cfg(target_os = "windows")]
fn focus_windows_of(pids: &[u32]) -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE,
        SW_MINIMIZE, SW_RESTORE, SW_SHOW, WS_EX_TOOLWINDOW,
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
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if !is_focus_candidate(
            IsWindowVisible(hwnd) != 0,
            GetWindowTextLengthW(hwnd),
            ex_style,
            area,
        ) {
            return TRUE;
        }
        // 量级差保证优先级：非工具窗 > 工具窗 > 面积大（候选已全部可见，无需再加可见分）
        let mut score = area;
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
        // 最小化的窗口靠 SW_RESTORE 还原并激活（候选恒可见，SW_SHOW 只是防御性补一拍）
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

    #[test]
    fn focus_candidate_rejects_hidden_windows() {
        // 2026-09-29 微信 4.1.13「关窗挂托盘」态实测值（同一进程并存的窗口）：
        // 真主窗「微信」——隐藏；外部 SW_SHOW 能拉出来但点击无响应（Qt 认为窗仍隐藏），
        // 必须返回 false 走「重启 exe → 单实例唤起」路径 → 不是候选
        assert!(!is_focus_candidate(false, 2, 0x0000_0100, 941 * 688));
        // 托盘消息辅助窗——隐藏、带标题、面积 1440×753 比真主窗还大，同样不是候选
        assert!(!is_focus_candidate(false, 23, 0x0000_0100, 1440 * 753));
        // Electron（WorkBuddy 5.3）隐藏主窗：外部拉起停留在最后一帧不再重绘 → 不是候选
        assert!(!is_focus_candidate(false, 9, 0x0000_0100, 1532 * 923));

        // 可见/最小化（最小化窗口 WS_VISIBLE 仍置位）照常调度到前台
        assert!(is_focus_candidate(true, 2, 0x0000_0100, 941 * 688));
        // WorkBuddy 最小化实测：可见（iconic）、160×28 的最小化占位矩形
        assert!(is_focus_candidate(true, 9, 0x0000_0100, 160 * 28));

        // 可见但无激活（点击穿透的提示气泡一类）与零面积、无标题窗口照旧排除
        assert!(!is_focus_candidate(true, 5, 0x0800_0100, 10_000));
        assert!(!is_focus_candidate(true, 5, 0x0000_0100, 0));
        assert!(!is_focus_candidate(true, 0, 0x0000_0100, 10_000));
    }
}
