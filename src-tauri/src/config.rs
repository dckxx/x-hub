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
    /// 应用壁纸：主窗口背景图片的绝对路径（空 = 未设置，回退主题渐变背景）
    #[serde(default)]
    pub wallpaper_path: String,
    /// 壁纸整屏静态模糊（ADR 0002：模糊作用于壁纸层整体，非卡片局部 backdrop）
    #[serde(default = "default_true")]
    pub wallpaper_blur: bool,
    /// 壁纸蒙版：主题底色罩层不透明度（0–0.85，默认 0.3），在壁纸鲜亮度与文字对比度间取平衡
    #[serde(default = "default_wallpaper_veil")]
    pub wallpaper_veil: f64,
    /// 沉浸模式：卡片改用真毛玻璃 backdrop-filter 局部取景模糊（ADR 0003 受控例外，默认关）
    #[serde(default)]
    pub wallpaper_immersive: bool,
    /// 卡片玻璃透明度（0.4–1.0，1.0 = 默认不透明观感）
    #[serde(default = "one")]
    pub glass_opacity: f64,
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
    /// AI 对话面板方位：left / right / top / bottom（默认右侧）
    #[serde(default = "default_chat_panel_side")]
    pub chat_panel_side: String,
    /// AI 对话面板在「顶部/底部」方位时的高度（320–640px 之外的拖拽会钳制）
    #[serde(default = "default_chat_panel_height")]
    pub chat_panel_height: f64,
    /// AI 对话右侧面板透明度（0.5–1.0，可在设置中调整）
    #[serde(default = "default_chat_panel_opacity")]
    pub chat_panel_opacity: f64,
    /// AI 对话是否以独立窗口打开（true = 独立小窗，false = 主窗内嵌抽屉，默认）
    #[serde(default)]
    pub chat_window_mode: bool,
    /// AI 对话独立窗口宽度（逻辑 px，由窗口缩放拖拽记忆）
    #[serde(default = "default_chat_window_width")]
    pub chat_window_width: f64,
    /// AI 对话独立窗口高度（逻辑 px）
    #[serde(default = "default_chat_window_height")]
    pub chat_window_height: f64,
    /// AI 对话独立窗口位置（物理 px，拖动松手后由后端记忆，与悬浮球/倒计时浮窗同约定）
    #[serde(default)]
    pub chat_window_x: Option<f64>,
    #[serde(default)]
    pub chat_window_y: Option<f64>,
    /// AI 对话独立窗口是否置顶（自制标题栏的图钉按钮切换）
    #[serde(default)]
    pub chat_window_pinned: bool,
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
    /// 速记编辑器模式：wysiwyg（实时预览，默认）/ split（分屏预览）/ source（源码）。
    /// 按用户记住，不按单篇笔记。非法值由前端读入时归一为 wysiwyg。
    #[serde(default = "default_note_editor_mode")]
    pub note_editor_mode: String,
    /// service 扩展运行时策略：auto（自动检测，默认）/ builtin（始终内置）/ system（始终系统）
    #[serde(default = "default_runtime_strategy")]
    pub runtime_strategy: String,
    /// 固定到左侧栏的扩展 id 列表（点击侧栏菜单即在主区打开对应扩展）
    #[serde(default)]
    pub sidebar_extensions: Vec<String>,
    /// 扩展「默认打开方式」映射：extId → view / window / drawer（未设置时默认 view）
    #[serde(default)]
    pub extension_open_modes: std::collections::HashMap<String, String>,
    /// ⚠️ **已废弃、不再被读取**（v0.6.1）：市场清单地址的唯一真相源是内置常量
    /// [`market_registry_url`]——从 v0.6.1 起客户端**不再直连对象存储**，清单/包/截图一律走
    /// 平台服务端接口（`x-hub-server` 的 `src/modules/market`，服务端再代理 COS）。
    /// 字段保留只为兼容旧 `app.json`：读到即被 [`migrate_legacy_endpoints`] 归一为服务端地址并落盘。
    /// 不要再从它读地址（要恢复「自建分发可配置」时，请连同设置入口一起加回来）。
    #[serde(default)]
    pub market_endpoint: String,
    /// ⚠️ **已废弃、不再被读取**（v0.6.x）：曾经是「开发者模式」总开关，现在**登记即加载**——
    /// 加进「我的扩展」的本机源码目录一律直挂（见 docs/adr/0005 的 v0.6.x 修订）。
    /// 字段保留只为兼容旧 `app.json` 里残留的 false，读到即忽略；不要再恢复读取。
    #[serde(default)]
    pub dev_mode_enabled: bool,
    /// ⚠️ **已废弃、不再被读取**：x-hub 平台服务端地址的唯一真相源是内置常量
    /// `DEFAULT_SERVER_URL`（v0.5.6 起平台域名正式启用，设置页不再提供地址入口，见约定 52）。
    /// 字段保留只为兼容旧 `app.json`（其中可能残留开发期的临时地址），读到即忽略——
    /// 若哪天又需要可配置，请连同设置入口一起加回来，不要只恢复读取。
    #[serde(default)]
    pub server_url: String,
    /// 「我的扩展」：本机扩展源码目录列表（绝对路径，目录须含 manifest.json）。
    /// 登记即加载（无需开关）；⚠️ 这些目录会被动态加入资产协议作用域，只暴露给扩展内容协议；
    /// 不要添加敏感目录。见 docs/adr/0005-developer-mode-local-source-mount.md
    #[serde(default)]
    pub dev_extensions: Vec<String>,
    /// 扩展开发技能包（Skills）的自定义安装根目录（自动探测的助手目录之外的 skills 根）。
    /// 安装到自定义目录时登记，供设置 →「扩展 → Skills」列出；只解除登记时见 `remove_skill_root`。
    #[serde(default)]
    pub skill_roots: Vec<String>,
    /// 开机自启动（登录 Windows 时自动驻留托盘）
    #[serde(default)]
    pub run_at_startup: bool,
    /// ⚠️ **已废弃、不再被读取**（v0.6.1）：升级清单地址的唯一真相源是内置常量
    /// [`update_manifest_url`]（同样走平台服务端接口，见 `market_endpoint` 字段注释）。
    /// 字段保留只为兼容旧 `app.json`，读到即被 [`migrate_legacy_endpoints`] 归一并落盘。
    #[serde(default)]
    pub update_endpoint: String,
    /// 自动升级总开关（默认开启）：关闭后不再发起版本检查
    #[serde(default = "default_true")]
    pub auto_update_enabled: bool,
    /// 静默检查更新频率（小时，默认 4）
    #[serde(default = "default_update_interval_hours")]
    pub update_interval_hours: u64,
    /// 用户「跳过此版本」记录的版本号（空 = 未跳过）；check 命中时若与清单版本一致则不再提示
    #[serde(default)]
    pub skipped_update_version: String,
    /// 桌面悬浮球总开关（ADR 0004，默认开启）：主窗口隐藏时在桌面显示悬浮球
    #[serde(default = "default_true")]
    pub floating_ball_enabled: bool,
    /// 悬浮球贴边自动隐藏：拖到屏幕边缘附近松手 → 球心落在屏边，只露出半个球体；
    /// 悬停时球体完整滑出。取代旧「贴边吸附」（用户反馈吸附从未生效，改为本交互）。
    /// alias：v0.5.2 及更早字段名为 floating_ball_snap，用户显式关闭过的偏好经别名
    /// 自动迁移（同 theme→theme_mode 先例），否则升级后被丢弃回落 default_true
    #[serde(default = "default_true", alias = "floating_ball_snap")]
    pub floating_ball_auto_hide: bool,
    /// 与主窗口同时显示：默认 false = 球仅在主窗隐藏/最小化时出现；
    /// 开启后球常驻桌面，主窗显示也不隐藏（sync_with_main 读此字段联动）
    #[serde(default)]
    pub floating_ball_with_main: bool,
    /// 环形快捷菜单按钮 id 列表（view:xxx / act:xxx，去重后最多 8 个，见 floating_ball.rs）
    #[serde(default = "default_floating_ball_buttons")]
    pub floating_ball_buttons: Vec<String>,
    /// 悬浮球窗口位置（物理 px，拖拽松手后由后端记忆；与倒计时浮窗同约定）
    #[serde(default)]
    pub floating_ball_x: Option<f64>,
    #[serde(default)]
    pub floating_ball_y: Option<f64>,
    /// 悬浮球静止态保持转动（炫酷模式，默认关）：true = 陀螺环常转 + canvas 满帧
    /// （v0.6.2 及以前的行为，更耗电发热）；false = 静止时环暂停 + canvas 降 24fps
    /// （笔记本发热治理，见 FloatingBallWindow 的 rings-idle / IDLE_FPS）
    #[serde(default)]
    pub floating_ball_idle_spin: bool,
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

fn default_chat_panel_side() -> String {
    "right".to_string()
}

fn default_chat_panel_height() -> f64 {
    380.0
}

fn default_chat_window_width() -> f64 {
    460.0
}

fn default_chat_window_height() -> f64 {
    640.0
}

fn default_true() -> bool {
    true
}

fn default_wallpaper_veil() -> f64 {
    0.3
}

fn default_quote_source() -> String {
    "online".to_string()
}

fn default_runtime_strategy() -> String {
    "auto".to_string()
}

fn default_note_editor_mode() -> String {
    "wysiwyg".to_string()
}

/// x-hub 平台服务端地址（账号登录 / 平台额度 / 申请开发者 / 发布扩展 / 市场清单 / 升级清单都基于它）。
///
/// **唯一真相源，且刻意不可配置**：正式域名启用后，设置页的「服务器地址」入口已移除
/// （见约定 52）。此前可配置是为了开发期临时指向本机联调地址，代价是老用户机器上
/// 残留的临时地址会在正式域名上线后继续生效、而界面上又没有入口可以改回来。
///
/// v0.6.1 起为 `https`：域名已于 2026-09-17 上 Let's Encrypt 证书（http 会 301 到 https），
/// 账号 token、市场清单、升级包与 AI 请求不再明文过网。
pub const DEFAULT_SERVER_URL: &str = "https://x-hub.xfactor.top";

/// 市场清单接口路径（服务端代理，客户端不再知道 COS 在哪里）。
pub const MARKET_REGISTRY_PATH: &str = "/api/v1/market/registry";

/// 应用升级清单接口路径（同上）。
pub const UPDATE_MANIFEST_PATH: &str = "/api/v1/app/update";

/// 市场清单地址（平台服务端接口，`.sig` 为同级 `{url}.sig`）。
pub fn market_registry_url() -> String {
    format!("{DEFAULT_SERVER_URL}{MARKET_REGISTRY_PATH}")
}

/// 应用升级清单地址（平台服务端接口，`.sig` 为同级 `{url}.sig`）。
pub fn update_manifest_url() -> String {
    format!("{DEFAULT_SERVER_URL}{UPDATE_MANIFEST_PATH}")
}

fn default_update_interval_hours() -> u64 {
    4
}

/// 环形菜单默认 6 个按钮（ADR 0004）：工作台 / 速记 / 速达 / 全局搜索 / 剪贴板 / 设置
pub fn default_floating_ball_buttons() -> Vec<String> {
    [
        "view:dashboard",
        "view:notes",
        "view:suda",
        "act:search",
        "act:clipboard",
        "view:settings",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme_mode: "light".to_string(),
            theme_preset: "indigo".to_string(),
            accent_color: None,
            wallpaper_path: String::new(),
            wallpaper_blur: true,
            wallpaper_veil: default_wallpaper_veil(),
            wallpaper_immersive: false,
            glass_opacity: 1.0,
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
            chat_panel_side: "right".to_string(),
            chat_panel_height: 380.0,
            chat_panel_opacity: 1.0,
            chat_window_mode: false,
            chat_window_width: default_chat_window_width(),
            chat_window_height: default_chat_window_height(),
            chat_window_x: None,
            chat_window_y: None,
            chat_window_pinned: false,
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
            note_editor_mode: default_note_editor_mode(),
            runtime_strategy: "auto".to_string(),
            sidebar_extensions: Vec::new(),
            extension_open_modes: std::collections::HashMap::new(),
            // 废弃字段（不再被读取）：市场清单地址真相源是 config::market_registry_url()
            market_endpoint: String::new(),
            dev_mode_enabled: false, // 已废弃字段：仅为兼容旧 app.json 保留，不再读取
            dev_extensions: Vec::new(),
            skill_roots: Vec::new(),
            // 废弃字段（不再被读取）：地址真相源是 DEFAULT_SERVER_URL，见字段注释
            server_url: String::new(),
            run_at_startup: false,
            // 废弃字段（不再被读取）：升级清单地址真相源是 config::update_manifest_url()
            update_endpoint: String::new(),
            auto_update_enabled: true,
            update_interval_hours: default_update_interval_hours(),
            skipped_update_version: String::new(),
            floating_ball_enabled: true,
            floating_ball_auto_hide: true,
            floating_ball_with_main: false,
            floating_ball_buttons: default_floating_ball_buttons(),
            floating_ball_x: None,
            floating_ball_y: None,
            floating_ball_idle_spin: false,
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
                // 迁移结果必须落盘：只改内存的话每次启动都会重新迁移，
                // 磁盘上那份已停用的地址永远留着（用户手工看一眼还是会被误导）。
                if normalize(&mut config) {
                    if let Err(e) = save_to(&config, path) {
                        log::warn!("配置迁移结果写入失败（本次仍按迁移后的值运行）: {e}");
                    }
                }
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
///
/// 返回值 = 是否有改动（有则调用方负责落盘）。
fn normalize(config: &mut AppConfig) -> bool {
    let mut changed = false;
    if config.clock_quote == "日拱一卒，功不唐捐。" {
        config.clock_quote = String::new();
        changed = true;
    }
    if migrate_legacy_endpoints(config) {
        changed = true;
    }
    changed
}

/// `market_endpoint` / `update_endpoint` 两个字段自 v0.6.1 起**已废弃不再被读取**（清单地址
/// 由 [`market_registry_url`] / [`update_manifest_url`] 按服务端地址拼），但老 `app.json`
/// 里会残留 R2/COS 时代的地址：0.6.0 及更早的安装升级上来后，文件里的旧地址与程序实际
/// 行为不一致，人工排查时会被误导（表现为「市场源异常：…404」但界面无从自救）。
/// 这里把任何非服务端的残留值一句话归一并落盘；空值同样归一，保证文件里始终是当前真相。
fn migrate_legacy_endpoints(config: &mut AppConfig) -> bool {
    let mut changed = false;
    let registry = market_registry_url();
    if config.market_endpoint.trim() != registry {
        if !config.market_endpoint.trim().is_empty() {
            log::info!(
                "市场源字段已废弃（v0.6.1 起走服务端接口），{} → {}",
                config.market_endpoint,
                registry
            );
        }
        config.market_endpoint = registry;
        changed = true;
    }
    let manifest = update_manifest_url();
    if config.update_endpoint.trim() != manifest {
        if !config.update_endpoint.trim().is_empty() {
            log::info!(
                "更新源字段已废弃（v0.6.1 起走服务端接口），{} → {}",
                config.update_endpoint,
                manifest
            );
        }
        config.update_endpoint = manifest;
        changed = true;
    }
    changed
}

/// 后端管理的字段清单（`merge_disk_authoritative` 保留哪些字段）。
///
/// 判定标准：**只由后端命令写盘、前端不回写**（前端 `state.config` 是启动快照）。
/// 新增这类字段时必须同步两处：① `merge_disk_authoritative` 里合并它；
/// ② 这里登记名字 —— 回归测试 `merge_keeps_backend_managed_fields` 逐个按此清单断言，
/// 漏了任何一步都会红（这个坑踩过两次，两次都是用户升级后才发现的）。
#[cfg(test)]
const BACKEND_MANAGED_FIELDS: &[&str] = &[
    // AI 对话模型配置：只经 save_chat_models 变更
    "chat_models",
    // AI 对话独立窗：开关经 chat_window_save_mode、几何由拖拽/缩放记忆
    "chat_window_mode",
    "chat_window_width",
    "chat_window_height",
    "chat_window_x",
    "chat_window_y",
    "chat_window_pinned",
    // 悬浮球：开关经 save_settings、位置由 drag_end 记忆
    "floating_ball_enabled",
    "floating_ball_auto_hide",
    "floating_ball_with_main",
    "floating_ball_buttons",
    "floating_ball_x",
    "floating_ball_y",
    "floating_ball_idle_spin",
    // 「我的扩展」本机源码目录：只经 add/remove_dev_extension 变更
    "dev_extensions",
    "dev_mode_enabled",
    // Skills 自定义安装根：只经 install_skill / remove_skill_root 变更
    "skill_roots",
    // 「跳过此版本」：只经 skip_update_version 变更
    "skipped_update_version",
    // 已废弃的两个端点字段（v0.6.1）：真相源是内置常量，只由 migrate_legacy_endpoints 归一
    "market_endpoint",
    "update_endpoint",
];

/// `save_config` 的「以磁盘为准」合并（**纯函数，便于回归测试**）：把前端整份提交的配置
/// 与磁盘上的当前配置合并，后端管理的字段一律取磁盘值，其余（用户可编辑项）以前端提交为准。
///
/// 为什么必须这样：前端提交的是**启动快照**（`state.config`），既不认识也不会回写这些后端字段，
/// 让它们跟着快照落盘就等于「保存任意设置 = 把这些字段回滚到启动时刻」。最迷惑的一次是
/// 「加完源码目录顺手把卡片拖进工作台」→ `setDashboardLayout` 整份保存 → 刚加的扩展从
/// 「我的扩展」里凭空消失（扩展其实还在运行、卡片也还在）。
pub fn merge_disk_authoritative(merged: &mut AppConfig, disk: &AppConfig) {
    // 独立窗几何/开关（chat_window::preserve_disk_fields）
    crate::chat_window::preserve_disk_fields(merged, disk);
    merged.chat_models = disk.chat_models.clone();
    merged.floating_ball_enabled = disk.floating_ball_enabled;
    merged.floating_ball_auto_hide = disk.floating_ball_auto_hide;
    merged.floating_ball_with_main = disk.floating_ball_with_main;
    merged.floating_ball_buttons = disk.floating_ball_buttons.clone();
    merged.floating_ball_x = disk.floating_ball_x;
    merged.floating_ball_y = disk.floating_ball_y;
    merged.floating_ball_idle_spin = disk.floating_ball_idle_spin;
    merged.dev_extensions = disk.dev_extensions.clone();
    // 已废弃字段（登记即加载后不再读取），仍以磁盘为准以免被快照写回
    merged.dev_mode_enabled = disk.dev_mode_enabled;
    // 已废弃的两个端点字段（v0.6.1）：前端启动快照里已经没有它们，不合并就会把磁盘上
    // 归一后的服务端地址覆盖成空串（行为无影响，但文件里会来回翻烧饼）
    merged.market_endpoint = disk.market_endpoint.clone();
    merged.update_endpoint = disk.update_endpoint.clone();
    merged.skill_roots = disk.skill_roots.clone();
    merged.skipped_update_version = disk.skipped_update_version.clone();
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
        assert_eq!(c.note_editor_mode, "wysiwyg");
    }

    #[test]
    fn note_editor_mode_missing_falls_back_to_wysiwyg() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("note_editor_mode");
        let loaded: AppConfig = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.note_editor_mode, "wysiwyg");
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

    // ---------------- 已废弃端点字段的归一迁移 ----------------
    // 守的是「老 app.json 里的 R2/COS 地址没有任何迁移、升级后刷新市场永远 404」那个坑：
    // 0.6.0 已发布后仍在多台机器上复现，界面上又没有任何入口能改回来。
    // v0.6.1 起两个字段本身废弃（地址改为按服务端地址拼），归一迁移保证文件里不再留着
    // 会误导排查的旧地址。

    #[test]
    fn legacy_endpoint_values_are_normalized_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        let mut cfg = AppConfig::default();
        // R2 时代残留 + COS 时代残留（0.6.0 及更早安装写进 app.json 的默认值）
        cfg.market_endpoint = "https://r2.dckxx.com/extensions/registry.json".to_string();
        cfg.update_endpoint =
            "https://x-hub-dist-1251402600.cos.ap-guangzhou.myqcloud.com/releases/update.json"
                .to_string();
        fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.market_endpoint, market_registry_url());
        assert_eq!(loaded.update_endpoint, update_manifest_url());

        // 归一必须落盘（否则用户手看 app.json 仍是旧地址，且每次启动都要重写一遍）
        let on_disk = fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains(MARKET_REGISTRY_PATH));
        assert!(on_disk.contains(UPDATE_MANIFEST_PATH));
        assert!(!on_disk.contains("r2.dckxx.com"));
        assert!(!on_disk.contains("cos.ap-guangzhou"));
    }

    #[test]
    fn default_config_gets_canonical_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        fs::write(&path, serde_json::to_string_pretty(&AppConfig::default()).unwrap()).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded.market_endpoint, market_registry_url());
        assert_eq!(loaded.update_endpoint, update_manifest_url());
        // 两个地址都必须落在平台服务端域名下，且带 https（http 会 301，POST 语义会丢）
        for url in [market_registry_url(), update_manifest_url()] {
            assert!(url.starts_with("https://"));
            assert!(url.starts_with(DEFAULT_SERVER_URL));
        }
    }

    // ---------------- merge_disk_authoritative 回归测试 ----------------
    // 这一组守的是「后端管理的字段被前端启动快照整份覆盖」那个坑（踩过两次）。
    // 反向也守：用户可编辑项必须仍然以前端提交为准，不能被合并顺手冻住。

    /// 造一份「磁盘上的当前配置」：后端管理的字段全部设成**非默认值**，
    /// 这样「被快照覆盖」一定表现为断言失败，而不是恰好等于默认值蒙混过关。
    fn disk_with_backend_values() -> AppConfig {
        AppConfig {
            chat_models: vec![ChatModelConfig {
                id: "disk-model".to_string(),
                name: "磁盘上的模型".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                model: "disk-model-v1".to_string(),
                api_key: String::new(),
                is_default: true,
                has_api_key: false,
                provider_name: "Disk".to_string(),
            }],
            chat_window_mode: true,
            chat_window_width: 777.0,
            chat_window_height: 666.0,
            chat_window_x: Some(111.0),
            chat_window_y: Some(222.0),
            chat_window_pinned: true,
            floating_ball_enabled: false,
            floating_ball_auto_hide: false,
            floating_ball_with_main: true,
            floating_ball_buttons: vec!["view:disk".to_string()],
            floating_ball_x: Some(333.0),
            floating_ball_y: Some(444.0),
            dev_extensions: vec!["E:\\src\\my-ext".to_string()],
            dev_mode_enabled: true,
            skill_roots: vec!["E:\\skills-custom".to_string()],
            skipped_update_version: "9.9.9".to_string(),
            ..AppConfig::default()
        }
    }

    /// 模拟前端整份提交里「后端字段被冲掉」的那一半：把磁盘配置序列化后**删掉登记的键**
    /// 再反序列化回来 —— 复现「前端不认识这些字段」的路径（如 `skill_roots`）。
    /// 注意另一条路径（前端认识、但带的是启动快照旧值，如 `chat_models`/`floating_ball_*`）
    /// 的形态等价：值不是当前的磁盘值。两条都由下面的断言覆盖。
    /// 补缺用的是 `AppConfig::default()` 的**同名字段值**（容器级 `#[serde(default)]`），
    /// 不是字段类型的 `Default` —— 所以默认模型/默认悬浮球按钮会填进来，而不是空值。
    fn snapshot_without_backend_fields(disk: &AppConfig) -> AppConfig {
        let mut value = serde_json::to_value(disk).unwrap();
        let obj = value.as_object_mut().unwrap();
        for field in BACKEND_MANAGED_FIELDS {
            obj.remove(*field);
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn merge_keeps_backend_managed_fields() {
        let disk = disk_with_backend_values();
        let snapshot = snapshot_without_backend_fields(&disk);

        // 前提校验：快照确实与磁盘不同（否则这个测试什么也没守）
        assert_ne!(
            snapshot.dev_extensions, disk.dev_extensions,
            "快照里的 dev_extensions 应当已被清空，测试前提不成立"
        );
        assert_ne!(
            snapshot.skill_roots, disk.skill_roots,
            "快照里的 skill_roots 应当已被清空，测试前提不成立"
        );
        // 注意：容器级 `#[serde(default)]` 会用 `AppConfig::default()` 的**同名字段值**补缺，
        // 所以缺字段的快照拿到的是默认模型/默认悬浮球按钮，而不是空值 —— 正是旧快照的形状。
        assert_ne!(
            serde_json::to_value(&snapshot.chat_models).unwrap(),
            serde_json::to_value(&disk.chat_models).unwrap()
        );

        let mut merged = snapshot.clone();
        merge_disk_authoritative(&mut merged, &disk);

        // 比对整体 JSON：字段级断言写漏了也逃不掉（JSON 里每个键都要等于磁盘值）
        assert_eq!(
            serde_json::to_value(&merged.chat_models).unwrap(),
            serde_json::to_value(&disk.chat_models).unwrap()
        );
        assert_eq!(merged.chat_window_mode, disk.chat_window_mode);
        assert_eq!(merged.chat_window_width, disk.chat_window_width);
        assert_eq!(merged.chat_window_height, disk.chat_window_height);
        assert_eq!(merged.chat_window_x, disk.chat_window_x);
        assert_eq!(merged.chat_window_y, disk.chat_window_y);
        assert_eq!(merged.chat_window_pinned, disk.chat_window_pinned);
        assert_eq!(merged.floating_ball_enabled, disk.floating_ball_enabled);
        assert_eq!(merged.floating_ball_auto_hide, disk.floating_ball_auto_hide);
        assert_eq!(
            merged.floating_ball_with_main,
            disk.floating_ball_with_main
        );
        assert_eq!(merged.floating_ball_buttons, disk.floating_ball_buttons);
        assert_eq!(merged.floating_ball_x, disk.floating_ball_x);
        assert_eq!(merged.floating_ball_y, disk.floating_ball_y);
        assert_eq!(merged.dev_extensions, disk.dev_extensions);
        assert_eq!(merged.dev_mode_enabled, disk.dev_mode_enabled);
        assert_eq!(merged.skill_roots, disk.skill_roots);
        assert_eq!(merged.skipped_update_version, disk.skipped_update_version);
    }

    /// 清单漏登就是这条红：任何登记在案的名字都必须是 `AppConfig` 真实存在的字段，
    /// 且合并后确实取到了磁盘值（名字打错/字段改名都能被抓到）。
    #[test]
    fn backend_managed_field_list_is_real_and_effective() {
        let disk = disk_with_backend_values();
        let mut merged = snapshot_without_backend_fields(&disk);
        merge_disk_authoritative(&mut merged, &disk);

        let merged_json = serde_json::to_value(&merged).unwrap();
        let disk_json = serde_json::to_value(&disk).unwrap();
        for field in BACKEND_MANAGED_FIELDS {
            assert!(
                merged_json.get(field).is_some(),
                "BACKEND_MANAGED_FIELDS 里的 `{field}` 不是 AppConfig 的字段（名字写错或字段已改名）"
            );
            assert_eq!(
                merged_json.get(field),
                disk_json.get(field),
                "`{field}` 声明为后端管理，但合并后没有取磁盘值"
            );
        }
    }

    /// 反向：用户可编辑项必须仍以前端提交为准（合并别把手伸过头，把所有设置都冻成磁盘旧值）。
    #[test]
    fn merge_keeps_user_editable_fields_from_snapshot() {
        let disk = AppConfig::default();
        let mut snapshot = snapshot_without_backend_fields(&disk);
        snapshot.theme_mode = "dark".to_string();
        snapshot.theme_preset = "midnight".to_string();
        snapshot.accent_color = Some("#8b8bff".to_string());
        snapshot.sidebar_toggle = true;
        snapshot.window.width = 1280.0;
        snapshot.dashboard_layout = r#"[{"id":"clock"}]"#.to_string();

        let mut merged = snapshot.clone();
        merge_disk_authoritative(&mut merged, &disk);

        assert_eq!(merged.theme_mode, "dark");
        assert_eq!(merged.theme_preset, "midnight");
        assert_eq!(merged.accent_color.as_deref(), Some("#8b8bff"));
        assert!(merged.sidebar_toggle);
        assert_eq!(merged.window.width, 1280.0);
        assert_eq!(merged.dashboard_layout, snapshot.dashboard_layout);
    }
}
