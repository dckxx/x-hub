//! 数据根目录解析。
//!
//! 数据根是 x-hub 所有持久化数据的统一挂载点：数据库 `app.db`、配置 `app.json`、
//! 图标 `icons/`、剪贴板图片 `clipboard/`、日志 `logs/`、`chat_keys.json` 全部位于其下。
//!
//! 标准版与便携版的判定互不串扰：
//!   - 便携版（exe 同目录有 `portable` 标志）：数据**固定**跟随 `exe 目录\data\`，
//!     不支持改路径（绝对路径无法跨电脑/盘符迁移，改路径会破坏"随身带"的语义）；
//!   - 标准版（无标志）：数据默认 `%APPDATA%\x-hub`，可在设置中改到任意目录，
//!     改过的路径记录在 `%APPDATA%\x-hub\data_path.json`。
//!
//! 启动时按以下优先级解析一次并缓存：
//!   1. 便携标志存在 → `exe 目录\data\`；
//!   2. 便携标志不存在（标准版）：
//!      a. `%APPDATA%` 引导文件记录了非默认路径（用户改过）→ 用它；
//!      b. 否则默认 `%APPDATA%\x-hub`（首次运行写入引导文件）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 便携标志文件名：exe 同目录存在该文件即启用便携版
pub const PORTABLE_MARKER: &str = "portable";

/// 便携版数据子目录名（数据落在 exe 目录下的 data/ 内）
pub const PORTABLE_DATA_DIR: &str = "data";

/// 引导文件名（记录用户改过的数据根路径；仅标准版使用，位于固定锚点目录）
const BOOTSTRAP_FILE: &str = "data_path.json";

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 默认数据根（标准版，`%APPDATA%\x-hub`）
pub fn default_data_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("x-hub")
}

/// 标准版引导文件锚点：`%APPDATA%\x-hub\data_path.json`
fn bootstrap_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("x-hub")
        .join(BOOTSTRAP_FILE)
}

/// exe 所在目录
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

/// 是否便携版：exe 同目录存在 portable 标志文件
pub fn is_portable() -> bool {
    exe_dir()
        .map(|d| d.join(PORTABLE_MARKER).exists())
        .unwrap_or(false)
}

/// 读取指定引导文件记录的绝对路径；文件不存在或非绝对路径时返回 None
fn read_bootstrap_at(file: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(file).ok()?;
    let p = content.trim();
    if p.is_empty() {
        return None;
    }
    let path = PathBuf::from(p);
    if path.is_absolute() { Some(path) } else { None }
}

/// 解析数据根（不缓存；供惰性初始化与测试使用）
pub fn resolve_data_root() -> PathBuf {
    // 1. 便携版：exe 旁有标志 → 数据固定跟随 exe\data（忽略任何改路径记录）
    if let Some(dir) = exe_dir() {
        if dir.join(PORTABLE_MARKER).exists() {
            return dir.join(PORTABLE_DATA_DIR);
        }
    }

    // 2. 标准版：用户改过的路径（%APPDATA% 引导文件，非默认值）
    if let Some(path) = read_bootstrap_at(&bootstrap_file()) {
        if path != default_data_root() {
            return path;
        }
    }

    // 3. 默认：用户级目录，并首次初始化引导文件
    let default = default_data_root();
    let _ = write_bootstrap_at(&bootstrap_file(), &default);
    default
}

/// 数据根（惰性解析并缓存，进程内只解析一次）
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(resolve_data_root).as_path()
}

/// 数据路径信息：`(路径, 模式)`，模式 = default / custom / portable
pub fn data_path_info() -> (String, &'static str) {
    let root = data_root();
    // 便携版：固定跟随 exe\data
    if is_portable() {
        return (root.to_string_lossy().into_owned(), "portable");
    }
    // 标准版：引导文件非默认 → 用户改过
    if let Some(path) = read_bootstrap_at(&bootstrap_file()) {
        if path != default_data_root() {
            return (root.to_string_lossy().into_owned(), "custom");
        }
    }
    (root.to_string_lossy().into_owned(), "default")
}

/// 更新引导文件指向新的数据根（仅标准版改路径时调用，重启后生效）
pub fn set_data_root(path: &Path) -> Result<(), String> {
    write_bootstrap_at(&bootstrap_file(), path)
}

/// 原子写入引导文件（临时文件 + rename）
fn write_bootstrap_at(file: &Path, value: &Path) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, value.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_is_absolute() {
        assert!(default_data_root().is_absolute());
    }

    #[test]
    fn portable_detection_returns_bool() {
        // 仅验证函数可调用且返回布尔值（测试二进制目录下通常无 portable 标记）
        let _ = is_portable();
    }
}
