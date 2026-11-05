//! 把键盘焦点真正送进 webview（2026-11-05，修「打开面板要先点一下，ESC 才收得回托盘」）。
//!
//! ## 现象与根因（本机实测取证）
//!
//! 主窗被托盘 / 全局快捷键 / 悬浮球「显示 + 聚焦」之后，**窗口**确实成了前台
//! （`GetForegroundWindow` 命中的就是主窗 hwnd），但键盘焦点停在顶层窗口自己身上：
//!
//! ```text
//! 唤起后： fg = 主窗(593776)  focus = 593776  cls = "Tauri Window"     ← 焦点在顶层窗
//! 点一下： focus = Chrome_WidgetWin_1（WebView2 子窗）                  ← 键盘才真正进页面
//! ```
//!
//! 焦点不在 WebView2 子窗里，DOM 一个 `keydown` 都收不到——于是 index.vue / ChatWindow.vue
//! 里的 ESC 监听形同虚设，用户必须先拿鼠标点一下窗口。这不是「ESC 逻辑写得不对」，
//! 是**焦点根本没进去**，所以修复必须落在原生侧。
//!
//! ## 为什么不能只靠 wry 的 `WM_SETFOCUS`
//!
//! wry 在自己的父窗子类过程里收到 `WM_SETFOCUS` 会调
//! `ICoreWebView2Controller::MoveFocus(PROGRAMMATIC)`（wry-0.55.1 webview2/mod.rs:1254），
//! 但实测在「hide → show + SetForegroundWindow」这条路径上它并没有把焦点推进 webview
//! （上面取证就是唤起后的状态）。这里改用 Win32 的确定性做法：`SetFocus` 到 WebView2 的
//! 宿主子窗 `Chrome_WidgetWin_0`（Chromium 内部会再把它下移到 `Chrome_WidgetWin_1`）。
//! 实测这一下即中：焦点转到 `Chrome_WidgetWin_1`，紧接着按 Esc 主窗立刻收进托盘。
//!
//! ⚠️ 该宿主窗**与 UI 线程同线程**（`GetWindowThreadProcessId` 返回的就是 UI 线程 id，
//! 见下面 focus_now 的层级说明），所以同线程直接 SetFocus 即可，只有跨线程才需要
//! `AttachThreadInput`。把「同线程」当异常跳过 = SetFocus 一次都不执行，表现为
//! 「键盘焦点永远进不去，按 ESC/快捷键都得先点一下窗口」（2026-11-05 回修过）。

//!
//! ## 调用时机与安全边界
//!
//! - 只在「窗口刚被显示/聚焦」时调用（tray::show_window / toggle_window、主窗
//!   `WindowEvent::Focused(true)`、对话窗/贴图窗/整列表浮窗 show 之后）；
//! - **先体检再动手**：焦点已经在窗口的某个子窗里（含速达内嵌页 webview、扩展浮窗）
//!   就直接返回，绝不抢用户正在输入的 webview；焦点还在顶层窗自己身上才补一刀；
//! - 延时 60ms：窗口隐藏期间 WebView2 处于 Low 内存档，show 之后子窗口可能还在重建，
//!   太早 SetFocus 会打在旧 HWND 上；
//! - 只在 Windows 上生效，其它平台是 no-op（macOS/Linux 的 WebView2 语义不同，
//!   当前项目只发 Windows）。

use tauri::Manager;

/// 每个窗口最近一次「请求把焦点送进 webview」的序号。同一 label 短时间内被多路
/// 触发（tray show + WINDOW Focused + 各处 show 之后的显式调用）时，只有最后一次生效：
/// 否则会白起一串 60ms 线程，还可能在你已经点进输入框之后又把焦点重设一遍。
static FOCUS_GEN: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());
static FOCUS_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 登记一次焦点请求，返回本次的序号
fn next_generation(label: &str) -> u64 {
    let seq = FOCUS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if let Ok(mut list) = FOCUS_GEN.lock() {
        match list.iter_mut().find(|(l, _)| l == label) {
            Some(slot) => slot.1 = seq,
            None => list.push((label.to_string(), seq)),
        }
    }
    seq
}

/// 本次请求是否仍是最新（不是最新说明后面还有一次请求，交给它）
fn is_latest(label: &str, gen: u64) -> bool {
    FOCUS_GEN
        .lock()
        .map(|list| list.iter().any(|(l, g)| l == label && *g == gen))
        .unwrap_or(true)
}

/// 窗口显示/聚焦之后调用：把键盘焦点送进该窗口的 webview。
///
/// 幂等、廉价：焦点本来就在 webview 里时直接返回（见模块注释「安全边界」）。
pub fn focus_webview(app: &tauri::AppHandle, win_label: &str) {
    #[cfg(target_os = "windows")]
    {
        let gen = next_generation(win_label);
        let handle = app.clone();
        let label = win_label.to_string();
        // 后台线程等一拍再回主线程动手：窗口线程这会儿多半正卡在 show()/set_focus()
        // 的同步命令里，直接 run_on_main_thread 只会排在它后面，不如先睡 60ms 错开
        let _ = std::thread::Builder::new()
            .name("webview-focus".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(60));
                if !is_latest(&label, gen) {
                    return; // 期间又来了新的焦点请求，让最新那次去动手
                }
                let inner = handle.clone();
                if let Err(e) = handle.run_on_main_thread(move || focus_now(&inner, &label)) {
                    log::debug!("[focus] 投递主线程失败: {e}");
                }
            });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, win_label);
    }
}

/// 主线程上真正动手：体检焦点位置 → 需要时 AttachThreadInput + SetFocus
#[cfg(target_os = "windows")]
fn focus_now(app: &tauri::AppHandle, win_label: &str) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::AttachThreadInput;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetForegroundWindow, GetWindowThreadProcessId,
    };

    let Some(win) = app.get_window(win_label) else {
        return;
    };
    // 隐藏/最小化的窗口不抢焦点（最小化时 HWND 仍在，但屏幕上看不到）
    if !win.is_visible().unwrap_or(false) || win.is_minimized().unwrap_or(false) {
        return;
    }
    let Ok(hwnd) = win.hwnd() else { return };
    let parent = hwnd.0 as HWND;

    unsafe {
        // 只处理前台窗口：被别的应用盖住/失焦时不抢焦点（Esc 也不该在那个时刻生效）
        if GetForegroundWindow() != parent {
            return;
        }
        if focus_inside(parent) {
            return;
        }
        // 主窗自速达内嵌网页起是**多 webview**窗口：同时可见的宿主子窗可能有两个以上，
        // 「第一个可见的」不一定是主页面（内嵌页后建、Z 序在上，枚举反而先命中它，
        // 焦点被塞进网页后主界面的 ESC / 快捷键继续失灵）。所以先全收集，
        // 再按「与目标 webview 的实际 bounds 是否吻合」挑，挑不中才退回面积最大者。
        let mut children: Vec<HWND> = Vec::new();
        EnumChildWindows(
            parent,
            Some(collect_webview_children_proc),
            &mut children as *mut Vec<HWND> as LPARAM,
        );
        if children.is_empty() {
            log::debug!("[focus] {win_label}: 没找到 WebView2 子窗口，跳过");
            return;
        }
        let expected = app
            .get_webview(win_label)
            .and_then(|w| Some((w.position().ok()?, w.size().ok()?)));
        let Some(found) = pick_target_webview(parent, &children, expected) else {
            log::debug!("[focus] {win_label}: 无可用的 WebView2 子窗口，跳过");
            return;
        };
        // 焦点目标说明（2026-11-05 实测本机窗口层级）：
        //   Tauri Window → WRY_WEBVIEW(本进程) → Chrome_WidgetWin_0(本进程，tid == UI 线程)
        //                                        → Chrome_WidgetWin_1(浏览器进程，tid 不同)
        // 也就是说 Chrome_WidgetWin_0 是**同线程**窗口，SetFocus 直接生效（Chromium 再把焦点
        // 下移到 Chrome_WidgetWin_1）。因此「子窗线程 id == 本线程」是**正常主路径**，
        // 只有跨线程时才需要 AttachThreadInput。曾经把同线程当成异常直接 return，
        // 结果 SetFocus 一次都没执行 → 按 ESC 又变回「必须先点一下窗口」。
        let mut pid: u32 = 0;
        let child_tid = GetWindowThreadProcessId(found, &mut pid);
        let me = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        let mut attached = false;
        if child_tid != 0 && child_tid != me {
            // 跨线程（某些版本/形态下宿主窗属于浏览器进程）：不挂输入队列 SetFocus 无效
            if AttachThreadInput(me, child_tid, 1) == 0 {
                log::warn!("[focus] {win_label}: AttachThreadInput 失败，仍尝试 SetFocus");
            } else {
                attached = true;
            }
        }
        let prev = SetFocus(found);
        if attached {
            AttachThreadInput(me, child_tid, 0);
        }
        if prev == std::ptr::null_mut() {
            // SetFocus 返回旧焦点窗；NULL 表示调用失败或原本无焦点，两种都继续往下看结果
            log::debug!("[focus] {win_label}: SetFocus 返回空（无旧焦点或调用失败）");
        } else if prev != found {
            log::info!("[focus] {win_label}: 键盘焦点已送进 webview（原 {prev:p}）");
        } else {
            // 焦点本来就在目标窗里（每次切回前台都会走到这里）：不刷日志
            log::debug!("[focus] {win_label}: 焦点已在 webview，无需搬动");
        }
    }
}

/// 在候选子窗里挑出「本次要聚焦的那个 webview」。
///
/// 优先用目标 webview 的真实位置/尺寸（物理像素，相对窗口客户区）逐个比对：速达内嵌页
/// 与主页面同时可见时也只会命中一个。比对不中（DPI 取整 / 尚未完成布局）才退回
/// 「与客户区重叠面积最大者」——主页面通常占了窗口绝大部分，比「枚举到的第一个」可靠。
#[cfg(target_os = "windows")]
unsafe fn pick_target_webview(
    parent: windows_sys::Win32::Foundation::HWND,
    children: &[windows_sys::Win32::Foundation::HWND],
    expected: Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)>,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(parent, &mut client) == 0 {
        return children.first().copied();
    }
    let mut origin = POINT { x: 0, y: 0 };
    if ClientToScreen(parent, &mut origin) == 0 {
        return children.first().copied();
    }

    // 子窗相对客户区的矩形（物理像素）
    let rel = |h: windows_sys::Win32::Foundation::HWND| -> Option<(i32, i32, i32, i32)> {
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(h, &mut r) == 0 {
            return None;
        }
        Some((r.left - origin.x, r.top - origin.y, r.right - r.left, r.bottom - r.top))
    };

    if let Some((pos, size)) = expected {
        let target = (pos.x, pos.y, size.width as i32, size.height as i32);
        let mut best: Option<(i32, windows_sys::Win32::Foundation::HWND)> = None;
        for &h in children {
            let Some(rect) = rel(h) else { continue };
            let delta = (rect.0 - target.0).abs()
                + (rect.1 - target.1).abs()
                + (rect.2 - target.2).abs()
                + (rect.3 - target.3).abs();
            // 8px 容差：DPI 缩放下的取整误差远小于此，不同 webview 的差距远大于此
            if delta <= 32 && best.map(|(d, _)| delta < d).unwrap_or(true) {
                best = Some((delta, h));
            }
        }
        if let Some((_, h)) = best {
            return Some(h);
        }
    }

    let mut best: Option<(i64, windows_sys::Win32::Foundation::HWND)> = None;
    for &h in children {
        let Some((x, y, w, hgt)) = rel(h) else { continue };
        let overlap_w = (x + w).min(client.right).saturating_sub(x.max(0)).max(0) as i64;
        let overlap_h = (y + hgt).min(client.bottom).saturating_sub(y.max(0)).max(0) as i64;
        let area = overlap_w * overlap_h;
        if best.map(|(a, _)| area > a).unwrap_or(true) {
            best = Some((area, h));
        }
    }
    best.map(|(_, h)| h).or_else(|| children.first().copied())
}


/// 窗口的键盘焦点是否已经落在某个子窗里（含 WebView2 / 内嵌页 / 扩展 webview）。
///
/// 焦点还在顶层窗自己身上（`hwndFocus == parent`）或菜单/下拉占着焦点时返回 false。
#[cfg(target_os = "windows")]
unsafe fn focus_inside(parent: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    let mut pid: u32 = 0;
    let tid = GetWindowThreadProcessId(parent, &mut pid);
    if tid == 0 {
        return false;
    }
    let mut gti: GUITHREADINFO = std::mem::zeroed();
    gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if GetGUIThreadInfo(tid, &mut gti) == 0 {
        return false;
    }
    !gti.hwndFocus.is_null() && gti.hwndFocus != parent
}

/// EnumChildWindows 回调：把所有**可见**的 WebView2 宿主子窗（`Chrome_WidgetWin_0`）收集起来。
///
/// 主窗自速达内嵌网页起可能同时有两个可见的 webview 宿主窗，这里不做取舍（取舍见
/// `pick_target_webview`），只保证把候选收集全。返回 1 继续枚举。
#[cfg(target_os = "windows")]
unsafe extern "system" fn collect_webview_children_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, IsWindowVisible};

    let out = &mut *(lparam as *mut Vec<windows_sys::Win32::Foundation::HWND>);
    if IsWindowVisible(hwnd) != 0 {
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n > 0 {
            let name = String::from_utf16_lossy(&buf[..n as usize]);
            if name == "Chrome_WidgetWin_0" {
                out.push(hwnd);
            }
        }
    }
    1
}

