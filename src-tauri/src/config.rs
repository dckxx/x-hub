use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::models::ChatModelConfig;

/// 全局配置写锁：串行化所有「读-改-写」配置命令。
/// 防止并发下旧快照互相覆盖——典型事故：`save_chat_models` 刚把新模型写入 app.json，
/// 另一个命令用启动时读到的旧 `chat_models`（空/过期）整体覆写，导致配置的供应商「消失」。
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// 获取配置写锁（所有读-改-写配置的调用点都必须持有它）
pub fn lock() -> MutexGuard<'static, ()> {
    CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub always_on_top: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1400.0,
            height: 900.0,
            x: None,
            y: None,
            always_on_top: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 主题模式：light / dark / system（旧配置中的 `theme` 字段自动映射到此字段）
    #[serde(alias = "theme")]
    pub theme_mode: String,
    /// 主题预设：indigo / green / morandi / midnight
    pub theme_preset: String,
    /// 强调色（hex，如 #5B5BF5）；null 表示跟随预设推荐强调色
    pub accent_color: Option<String>,
    /// 侧边栏展开/收缩功能开关（默认关闭）
    pub sidebar_toggle: bool,
    pub window: WindowState,
    pub global_shortcut: String,
    /// 主页面「中上区块」显示内容：
    /// countdown(默认倒计时) / token(Token 统计) / notes(速记统计) / todo(待办概览) / resources(速达数量)
    pub dashboard_mid_content: String,
    /// 工作台自定义布局（placements JSON 数组字符串；空串 = 未自定义，回退推荐布局）
    #[serde(default)]
    pub dashboard_layout: String,
    /// 倒计时到点提示音（默认关闭）
    pub countdown_sound: bool,
    /// 时钟卡片语录（工作台时间卡片下方显示的一句话，空串时回退默认）
    pub clock_quote: String,
    /// 联网功能总开关（默认开启）：有网显示在线内容、无网自动隐藏；关闭后完全不发起网络请求
    #[serde(default = "default_true")]
    pub online_enabled: bool,
    /// 天气城市展示名（空串 = 未配置，天气卡不显示）
    #[serde(default)]
    pub weather_city: String,
    /// 天气经纬度缓存（geocoding / IP 定位后写入；0 表示未配置）
    #[serde(default)]
    pub weather_lat: f64,
    #[serde(default)]
    pub weather_lng: f64,
    /// 名言来源：online（在线 hitokoto，离线回退本地语料）/ local（仅本地语料）
    #[serde(default = "default_quote_source")]
    pub quote_source: String,
    /// AI 对话自定义模型配置（不绑定厂商，统一 OpenAI 兼容协议；api_key 不落盘）
    pub chat_models: Vec<ChatModelConfig>,
    /// AI 对话右侧面板宽度（320–640px，持久化用户拖拽结果）
    pub chat_panel_width: f64,
    /// AI 对话右侧面板是否展开
    pub chat_panel_open: bool,
    /// AI 对话右侧面板透明度（0.5–1.0，可在设置中调整）
    #[serde(default = "default_chat_panel_opacity")]
    pub chat_panel_opacity: f64,
    /// 升级后弹窗显示更新说明（默认开启）
    #[serde(default = "default_true")]
    pub whats_new_enabled: bool,
    /// 上次已记录「更新说明」的版本号（用于升级检测；空串表示首次运行）
    pub last_seen_version: String,
    /// 剪贴板历史全局呼出快捷键（默认 Ctrl+Alt+V，可配置）
    pub clipboard_shortcut: String,
    /// 剪贴板历史最大条数（含置顶；置顶豁免自动清理但计入上限）
    pub clipboard_max_items: i64,
    /// 非置顶记录的保留天数
    pub clipboard_ttl_days: i64,
    /// 是否暂停记录（暂停期间复制内容不写入历史）
    pub clipboard_paused: bool,
    /// 粘贴快捷键方式：auto(自动检测终端) / ctrl_v / ctrl_shift_v / shift_insert
    #[serde(default = "default_paste_method")]
    pub clipboard_paste_method: String,
    /// 记录剪贴板图片（复制图片时落盘快照进历史，默认开启）
    #[serde(default = "default_true")]
    pub clipboard_image_enabled: bool,
    /// 记录剪贴板文件（复制文件时记录路径进历史，默认开启）
    #[serde(default = "default_true")]
    pub clipboard_file_enabled: bool,
    /// 全局字体缩放系数（0.85–1.30，默认 1.0）
    #[serde(default = "one")]
    pub font_scale: f64,
    /// 便签模块字体缩放系数（相对全局的额外缩放，默认 1.0）
    #[serde(default = "one")]
    pub font_sticky: f64,
    /// 速记模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_notes: f64,
    /// 提示词模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_prompt: f64,
    /// 待办模块字体缩放系数（默认 1.0）
    #[serde(default = "one")]
    pub font_todo: f64,
    /// service 扩展运行时策略：auto（自动检测，默认）/ builtin（始终内置）/ system（始终系统）
    #[serde(default = "default_runtime_strategy")]
    pub runtime_strategy: String,
    /// 固定到左侧栏的扩展 id 列表（点击侧栏菜单即在主区打开对应扩展）
    #[serde(default)]
    pub sidebar_extensions: Vec<String>,
    /// 扩展「默认打开方式」映射：extId → view / window / drawer（未设置时默认 view）
    #[serde(default)]
    pub extension_open_modes: std::collections::HashMap<String, String>,
    /// 市场清单远端地址（空 = 用默认值）
    #[serde(default = "default_market_endpoint")]
    pub market_endpoint: String,
}

fn one() -> f64 {
    1.0
}

fn default_paste_method() -> String {
    "auto".to_string()
}

fn default_chat_panel_opacity() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_quote_source() -> String {
    "online".to_string()
}

fn default_runtime_strategy() -> String {
    "auto".to_string()
}

/// 默认市场清单远端地址（Cloudflare R2 公开桶 + 自定义域名）
pub const DEFAULT_MARKET_ENDPOINT: &str = "https://r2.dckxx.com/extensions/registry.json";

fn default_market_endpoint() -> String {
    DEFAULT_MARKET_ENDPOINT.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme_mode: "light".to_string(),
            theme_preset: "indigo".to_string(),
            accent_color: None,
            sidebar_toggle: false,
            window: WindowState::default(),
            global_shortcut: crate::shortcut::DEFAULT_TOGGLE_SHORTCUT.to_string(),
            dashboard_mid_content: "countdown".to_string(),
            dashboard_layout: String::new(),
            countdown_sound: false,
            clock_quote: String::new(),
            online_enabled: true,
            weather_city: String::new(),
            weather_lat: 0.0,
            weather_lng: 0.0,
            quote_source: "online".to_string(),
            chat_models: default_chat_models(),
            chat_panel_width: 420.0,
            chat_panel_open: false,
            chat_panel_opacity: 1.0,
            whats_new_enabled: true,
            last_seen_version: String::new(),
            clipboard_shortcut: crate::shortcut::DEFAULT_CLIPBOARD_SHORTCUT.to_string(),
            clipboard_max_items: 500,
            clipboard_ttl_days: 7,
            clipboard_paused: false,
            clipboard_paste_method: "auto".to_string(),
            clipboard_image_enabled: true,
            clipboard_file_enabled: true,
            font_scale: 1.0,
            font_sticky: 1.0,
            font_notes: 1.0,
            font_prompt: 1.0,
            font_todo: 1.0,
            runtime_strategy: "auto".to_string(),
            sidebar_extensions: Vec::new(),
            extension_open_modes: std::collections::HashMap::new(),
            market_endpoint: default_market_endpoint(),
        }
    }
}

/// 预置一条 DeepSeek 官方配置作为开箱即用示例（用户可删可改可加）
pub fn default_chat_models() -> Vec<ChatModelConfig> {
    vec![ChatModelConfig {
        id: "deepseek-default".to_string(),
        name: "DeepSeek".to_string(),
        provider_name: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com/v1".to_string(),
        model: "deepseek-v4-flash".to_string(),
        api_key: String::new(),
        is_default: true,
        has_api_key: false,
    }]
}

pub fn config_dir() -> PathBuf {
    // 配置与数据库同挂数据根：更改数据目录后 app.json 也随数据走，
    // U 盘便携时配置一并继承（数据根解析见 paths.rs）
    crate::paths::data_root().to_path_buf()
}

pub fn config_file() -> PathBuf {
    config_dir().join("app.json")
}

pub fn load() -> AppConfig {
    load_from(&config_file())
}

pub fn load_from(path: &Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => {
                let mut config = config;
                normalize(&mut config);
                config
            }
            Err(_) => {
                // 配置文件损坏：备份并回退默认
                let _ = fs::copy(path, path.with_extension("json.bak"));
                let default = AppConfig::default();
                let _ = save_to(&default, path);
                default
            }
        },
        Err(_) => AppConfig::default(),
    }
}

/// 旧默认语录「日拱一卒」迁移：v0.1.19 起语录改为随机名言金句，
/// 旧默认值视为「未自定义」，置空以启用随机金句。
fn normalize(config: &mut AppConfig) {
    if config.clock_quote == "日拱一卒，功不唐捐。" {
        config.clock_quote = String::new();
    }
}

pub fn save(config: &AppConfig) -> Result<(), String> {
    save_to(config, &config_file())
}

pub fn save_to(config: &AppConfig, path: &Path) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "配置目录无效".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    // 原子写入：临时文件 + rename
    let mut tmp = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
    tmp.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    tmp.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = AppConfig::default();
        assert_eq!(c.theme_mode, "light");
        assert_eq!(c.theme_preset, "indigo");
        assert!(c.accent_color.is_none());
        assert!(!c.sidebar_toggle);
        assert_eq!(c.window.width, 1400.0);
        assert!(!c.window.always_on_top);
        assert_eq!(c.global_shortcut, crate::shortcut::DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(c.dashboard_mid_content, "countdown");
    }

    #[test]
    fn save_to_and_load_from_roundtrip() {
        let mut config = AppConfig::default();
        config.theme_mode = "dark".to_string();
        config.theme_preset = "midnight".to_string();
        config.accent_color = Some("#8b8bff".to_string());
        config.sidebar_toggle = true;
        config.window.width = 1280.0;
        config.window.x = Some(100.0);
        config.window.always_on_top = true;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        save_to(&config, &path).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "dark");
        assert_eq!(loaded.theme_preset, "midnight");
        assert_eq!(loaded.accent_color.as_deref(), Some("#8b8bff"));
        assert!(loaded.sidebar_toggle);
        assert_eq!(loaded.window.width, 1280.0);
        assert_eq!(loaded.window.x, Some(100.0));
        assert!(loaded.window.always_on_top);
    }

    #[test]
    fn corrupted_config_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, "not valid json {{{").unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "light");
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn missing_config_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "light");
    }

    #[test]
    fn old_theme_field_migrates_to_theme_mode() {
        // 旧版配置格式：只有 `theme` 字段（light/dark），
        // 依赖 serde `alias = "theme"` 自动映射到 theme_mode
        let old_json = serde_json::json!({
            "theme": "dark",
            "window": {
                "width": 1400.0,
                "height": 900.0,
                "x": null,
                "y": null,
                "always_on_top": false
            },
            "global_shortcut": "Ctrl+Shift+Space",
            "dashboard_mid_content": "countdown",
            "countdown_sound": false
        })
        .to_string();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, old_json).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.theme_mode, "dark");
        assert_eq!(loaded.theme_preset, "indigo");
        assert!(loaded.accent_color.is_none());
    }
}
