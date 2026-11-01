//! 枚举本机已安装浏览器
//!
//! Windows 标准做法：读注册表 `SOFTWARE\Clients\StartMenuInternet`（HKLM + HKCU，
//! 另查 Wow6432Node 兼容 32 位注册项）——系统「默认应用」列表即来源于此，
//! Chrome/Edge/Firefox/Brave/360/QQ 等浏览器都会注册。每个子键为一条浏览器注册项：
//! 默认值是显示名，`shell\open\command` 是启动命令（首段引号内即 exe 路径）。
//! 按 exe 路径去重后按显示名排序。
//!
//! macOS 无注册表等价物：扫 /Applications、/System/Applications、~/Applications
//! 下的 *.app，`plutil -convert json` 读 Info.plist，CFBundleURLTypes[].CFBundleURLSchemes
//! 含 http scheme 的即浏览器（Safari/Chrome/Edge/Firefox 都注册 http；只认 https 的
//! 大多是应用回跳 URL——微信/QQ 等注册 https 做登录回跳，不能算浏览器）。
//! exe 字段存 .app 目录路径，打开时走 `open -a`（见 process::open_with_browser）。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledBrowser {
    pub name: String,
    pub exe: String,
}

#[cfg(target_os = "windows")]
pub fn list_installed() -> Vec<InstalledBrowser> {
    use std::collections::HashSet;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut result: Vec<InstalledBrowser> = Vec::new();
    let mut seen = HashSet::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let root = RegKey::predef(root);
        for path in [
            "SOFTWARE\\Clients\\StartMenuInternet",
            "SOFTWARE\\Wow6432Node\\Clients\\StartMenuInternet",
        ] {
            let Ok(clients) = root.open_subkey(path) else {
                continue;
            };
            for key_name in clients.enum_keys().flatten() {
                let Ok(client) = clients.open_subkey(&key_name) else {
                    continue;
                };
                let Ok(command) = client
                    .open_subkey("shell\\open\\command")
                    .and_then(|k| k.get_value::<String, _>(""))
                else {
                    continue;
                };
                let Some(exe) = extract_exe(&command) else {
                    continue;
                };
                // 注册表残留（浏览器已卸载）过滤
                if !std::path::Path::new(&exe).is_file() {
                    continue;
                }
                if !seen.insert(exe.to_lowercase()) {
                    continue;
                }
                let name = display_name(&client, &key_name, &exe);
                result.push(InstalledBrowser { name, exe });
            }
        }
    }
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// macOS：扫标准应用目录的 *.app，plutil 读 Info.plist 判 http scheme。
/// 逐个 spawn plutil ~15ms、几十个应用串行要一秒多，用 10 个并行 worker 提速。
#[cfg(not(target_os = "windows"))]
pub fn list_installed() -> Vec<InstalledBrowser> {
    use std::path::PathBuf;

    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    let mut apps: Vec<PathBuf> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // 只认目录形态的 .app（排除同名文件/别名）
            if path.is_dir() && path.extension().and_then(|e| e.to_str()) == Some("app") {
                apps.push(path);
            }
        }
    }

    let mut found: Vec<InstalledBrowser> = {
        let chunk = (apps.len() / 10).max(1);
        let mut result: Vec<InstalledBrowser> = Vec::new();
        std::thread::scope(|s| {
            let handles: Vec<_> = apps
                .chunks(chunk)
                .map(|group| s.spawn(move || {
                    group
                        .iter()
                        .filter_map(|p| browser_from_app(p))
                        .collect::<Vec<InstalledBrowser>>()
                }))
                .collect();
            for h in handles {
                result.extend(h.join().unwrap_or_default());
            }
        });
        result
    };

    // 路径去重（多目录可能撞名）+ 按显示名排序，与 Windows 版同口径
    let mut seen = std::collections::HashSet::new();
    found.retain(|b| seen.insert(b.exe.to_lowercase()));
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

/// 单个 .app 的浏览器判定：plutil 读 Info.plist，CFBundleURLTypes 里声明 http
/// scheme 才算浏览器；显示名取 CFBundleDisplayName / CFBundleName，兜底目录名。
#[cfg(not(target_os = "windows"))]
fn browser_from_app(app_path: &std::path::Path) -> Option<InstalledBrowser> {
    let plist = app_path.join("Contents").join("Info.plist");
    if !plist.is_file() {
        return None;
    }
    let out = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let is_browser = json
        .get("CFBundleURLTypes")
        .and_then(|t| t.as_array())
        .map(|types| {
            types.iter().any(|t| {
                t.get("CFBundleURLSchemes")
                    .and_then(|s| s.as_array())
                    .map(|schemes| schemes.iter().any(|s| s.as_str() == Some("http")))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !is_browser {
        return None;
    }
    let name = ["CFBundleDisplayName", "CFBundleName"]
        .iter()
        .find_map(|k| json.get(k).and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            app_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })?;
    Some(InstalledBrowser {
        name,
        exe: app_path.to_string_lossy().into_owned(),
    })
}

/// 从启动命令中解析 exe 路径：`"C:\...\chrome.exe" --single-argument %1` → 引号内路径
#[cfg(target_os = "windows")]
fn extract_exe(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        let path = &rest[..end];
        return if path.is_empty() {
            None
        } else {
            Some(path.to_string())
        };
    }
    let path = trimmed.split_whitespace().next()?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// 显示名：优先注册表默认值 / LocalizedString；间接字符串（@...）或缺失时按 exe 文件名兜底
#[cfg(target_os = "windows")]
fn display_name(client: &winreg::RegKey, key_name: &str, exe: &str) -> String {
    let from_registry = client
        .get_value::<String, _>("")
        .ok()
        .or_else(|| client.get_value::<String, _>("LocalizedString").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('@'));
    from_registry.unwrap_or_else(|| {
        fallback_name(exe)
            .map(str::to_string)
            .unwrap_or_else(|| key_name.to_string())
    })
}

/// 常见浏览器 exe 文件名 → 显示名兜底表
#[cfg(target_os = "windows")]
fn fallback_name(exe: &str) -> Option<&'static str> {
    let file = std::path::Path::new(exe)
        .file_name()?
        .to_str()?
        .to_lowercase();
    const FALLBACKS: &[(&str, &str)] = &[
        ("chrome.exe", "Google Chrome"),
        ("msedge.exe", "Microsoft Edge"),
        ("firefox.exe", "Firefox"),
        ("brave.exe", "Brave"),
        ("vivaldi.exe", "Vivaldi"),
        ("opera.exe", "Opera"),
        ("360se.exe", "360安全浏览器"),
        ("360chrome.exe", "360极速浏览器"),
        ("qqbrowser.exe", "QQ浏览器"),
        ("sogouexplorer.exe", "搜狗浏览器"),
        ("maxthon.exe", "傲游浏览器"),
    ];
    FALLBACKS
        .iter()
        .find(|(name, _)| *name == file)
        .map(|(_, display)| *display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn extract_exe_parses_quoted_command() {
        assert_eq!(
            extract_exe(
                r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --single-argument %1"#
            ),
            Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string())
        );
        assert_eq!(
            extract_exe(r"C:\Browsers\firefox.exe %1"),
            Some(r"C:\Browsers\firefox.exe".to_string())
        );
        assert_eq!(extract_exe(r#""#), None);
    }

    #[test]
    fn list_installed_runs_without_panic() {
        // 依赖机器环境（无浏览器时应返回空列表），仅保证枚举过程不 panic
        let _ = list_installed();
    }
}
