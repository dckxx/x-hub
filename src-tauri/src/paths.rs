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

/// 剥掉 Windows verbatim（`\\?\`）前缀，返回等价的可交付路径。
///
/// **为什么必须有这个函数**：`std::fs::canonicalize` 在 Windows 上返回 verbatim 形式
/// （`\\?\A:\x-hub\publish-src\lan-share`）。Rust 自己的 fs 调用接受它，但**一交给外部程序就出事**：
/// Node 的 CJS 加载器处理不了这个前缀，会把 `\\?\A:\…\backend\server.js` 拆错，
/// 跑去 `lstat` 盘符 `A:` 得 `EISDIR` 直接 `exit 1`（service 扩展后端「静默起不来」的根因）；
/// `ShellExecute`（`opener::open` / `explorer`）同样不认。所以凡是**要落盘、要展示、
/// 要交给外部进程**的路径都必须先过这里。
///
/// 保守策略（与 `dunce::simplified` 同口径，只有确知语义不变才剥）：
///   - `\\?\C:\...`（盘符形式）→ `C:\...`；
///   - `\\?\UNC\server\share\...` → `\\server\share\...`；
///   - 剥完含 `.` / `..` 组件的不剥（Win32 会规范化这些组件，剥了语义就变了）；
///   - `\\?\Volume{...}` / `\\?\GLOBALROOT` 等设备路径不剥。
/// 非 Windows 平台原样返回。
pub fn simplify_path(p: &Path) -> PathBuf {
    if !cfg!(windows) {
        return p.to_path_buf();
    }
    let s = p.to_string_lossy();
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        // 仅盘符形式（`X:` 开头）可剥；`Volume{...}` 等设备路径保持原样
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => rest.to_string(),
            _ => return p.to_path_buf(),
        }
    } else {
        return p.to_path_buf();
    };
    let out = PathBuf::from(&stripped);
    if out
        .components()
        .any(|c| matches!(c, std::path::Component::CurDir | std::path::Component::ParentDir))
    {
        return p.to_path_buf();
    }
    out
}

/// [`simplify_path`] 的存在性安全网：简化后的路径**确实可访问**才采用，否则原样返回。
/// 兜住极端情形——总长超过 260 且系统未开启长路径支持时，verbatim 是唯一能访问的形式。
pub fn simplify_existing(p: &Path) -> PathBuf {
    let s = simplify_path(p);
    if s.as_path() != p && s.exists() {
        s
    } else {
        p.to_path_buf()
    }
}

/// `p` 是否位于 `root` 之下（`root` 本身不算）。
///
/// 只认 `root` 后紧跟目录分隔符，防止 `C:\data` 误判 `C:\data2`；
/// Windows 上前缀按 ASCII 大小写不敏感比较，`/` 与 `\` 视为同一分隔符。
/// 用于「数据目录迁移后，库/配置里的绝对路径是否还指着旧根」的判定。
pub fn path_within_root(p: &str, root: &Path) -> bool {
    let root_s = trimmed_root(root);
    if p.len() <= root_s.len() {
        return false;
    }
    let Some(head) = p.get(..root_s.len()) else {
        return false;
    };
    if !head
        .replace('/', "\\")
        .eq_ignore_ascii_case(&root_s.replace('/', "\\"))
    {
        return false;
    }
    matches!(
        p.as_bytes().get(root_s.len()).copied(),
        Some(b'\\' | b'/')
    )
}

/// 把位于 `old_root` 下的绝对路径改写为 `new_root` 下的同相对路径；
/// 不在 `old_root` 下（或等于根本身）时返回 `None`，调用方保持原值。
pub fn relink_root_path(p: &str, old_root: &Path, new_root: &Path) -> Option<String> {
    if !path_within_root(p, old_root) {
        return None;
    }
    let old_len = trimmed_root(old_root).len();
    let rest = p.get(old_len..)?;
    Some(format!("{}{}", trimmed_root(new_root), rest))
}

/// 根路径去掉尾部分隔符（`/` 与 `\` 都算）：保留原分隔符风格，只用于长度与拼接。
/// 盘符根 `C:\` 会变成 `C:`，随后「根后必须紧跟分隔符」的判定正好把它补回来。
fn trimmed_root(root: &Path) -> String {
    let s = root.to_string_lossy().into_owned();
    let trimmed = s.trim_end_matches(|c: char| c == '/' || c == '\\');
    if trimmed.is_empty() {
        s
    } else {
        trimmed.to_string()
    }
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

    #[test]
    fn simplify_strips_verbatim_prefix() {
        if !cfg!(windows) {
            // 非 Windows 平台一律原样返回
            assert_eq!(
                simplify_path(Path::new("/tmp/x-hub/ext")),
                PathBuf::from("/tmp/x-hub/ext")
            );
            return;
        }
        // 盘符形式：canonicalize 在 Windows 上的真实产物
        assert_eq!(
            simplify_path(Path::new(r"\\?\A:\x-hub\publish-src\1.3.0\lan-share")),
            PathBuf::from(r"A:\x-hub\publish-src\1.3.0\lan-share")
        );
        // UNC 形式：还原成 `\\server\share`
        assert_eq!(
            simplify_path(Path::new(r"\\?\UNC\srv\share\ext")),
            PathBuf::from(r"\\srv\share\ext")
        );
        // 普通路径不动
        assert_eq!(
            simplify_path(Path::new(r"C:\x-hub\extensions\com.a.b")),
            PathBuf::from(r"C:\x-hub\extensions\com.a.b")
        );
        // 设备路径不剥（剥了会变成不存在的盘符路径）
        assert_eq!(
            simplify_path(Path::new(r"\\?\Volume{8f0e1c2a}\x")),
            PathBuf::from(r"\\?\Volume{8f0e1c2a}\x")
        );
        // 含 `..` 组件不剥：Win32 会规范化，语义会变
        assert_eq!(
            simplify_path(Path::new(r"\\?\C:\a\..\b")),
            PathBuf::from(r"\\?\C:\a\..\b")
        );
    }

    #[test]
    fn path_within_root_requires_separator_boundary() {
        let root = Path::new(r"C:\Users\me\AppData\Roaming\x-hub");
        assert!(path_within_root(
            r"C:\Users\me\AppData\Roaming\x-hub\icons\0123.png",
            root
        ));
        // 只是字符串前缀（`x-hub2`）不算在根下
        assert!(!path_within_root(
            r"C:\Users\me\AppData\Roaming\x-hub2\icons\0123.png",
            root
        ));
        assert!(!path_within_root(r"C:\Users\me\AppData\Roaming\x-hub", root));
        // Windows 前缀大小写不敏感 + 正斜杠混用
        assert!(path_within_root(
            "c:/Users/me/AppData/Roaming/X-HUB/icons/0123.png",
            root
        ));
        // 完全不相干的路径
        assert!(!path_within_root(r"D:\icons\0123.png", root));
        // 根带尾部分隔符 / 盘符根也应正确判定
        assert!(path_within_root(
            r"C:\data\icons\a.png",
            Path::new(r"C:\data\")
        ));
        assert!(!path_within_root(r"C:\data2\icons\a.png", Path::new(r"C:\data\")));
        assert!(path_within_root(r"C:\icons\a.png", Path::new(r"C:\")));
    }

    #[test]
    fn relink_root_path_swaps_prefix_only() {
        let old = Path::new(r"C:\old-data");
        let new = Path::new(r"D:\new-data");
        assert_eq!(
            relink_root_path(r"C:\old-data\icons\a.png", old, new).as_deref(),
            Some(r"D:\new-data\icons\a.png")
        );
        // 新根带尾部分隔符时不能拼出双分隔符
        assert_eq!(
            relink_root_path(r"C:\old-data\icons\a.png", old, Path::new(r"D:\new-data\"))
                .as_deref(),
            Some(r"D:\new-data\icons\a.png")
        );
        // 不在旧根下 / 只差一个字符的兄弟目录 / 远程 URL 都原样保留
        assert_eq!(relink_root_path(r"C:\old-data2\icons\a.png", old, new), None);
        assert_eq!(relink_root_path("https://example.com/a.png", old, new), None);
        assert_eq!(relink_root_path(r"C:\old-data", old, new), None);
    }

    #[test]
    fn simplify_existing_keeps_usable_path() {
        let dir = tempfile::tempdir().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        if cfg!(windows) {
            assert!(
                canon.to_string_lossy().starts_with(r"\\?\"),
                "前提不成立：Windows 的 canonicalize 应当返回 verbatim 形式"
            );
        }
        let plain = simplify_existing(&canon);
        assert!(plain.is_dir(), "简化后的路径必须仍可访问");
        assert!(!plain.to_string_lossy().starts_with(r"\\?\"));
        // 已简化 / 不存在的路径原样返回
        assert_eq!(simplify_existing(&plain), plain);
        let ghost = canon.join("__not_here__");
        assert_eq!(simplify_existing(&ghost), ghost);
    }
}
