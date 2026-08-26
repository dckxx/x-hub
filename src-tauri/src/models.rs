use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    App,
    Web,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: i64,
    pub kind: ResourceKind,
    pub name: String,
    pub target: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub args: Option<String>,
    pub sort_order: i64,
    pub last_launched_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub todos: Vec<Todo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
    pub priority: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

/// 便签（工作台左上，slot 1/2 两张卡，每卡一条多行文本）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sticky {
    pub id: i64,
    pub slot: i64,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 脱离为系统级浮窗的便签（slot 对应来源卡片，一卡最多一个浮窗）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedSticky {
    pub id: i64,
    pub slot: i64,
    pub content: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub always_on_top: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 提示词百宝箱单条（可置顶、统计复制次数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub is_pinned: bool,
    pub copy_count: i64,
    pub last_copied_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 剪贴板历史单条（文本 / 图片 / 文件三类型；html 为可选富文本片段，粘贴时优先还原格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    /// 富文本 HTML 片段（如浏览器复制时携带）；无则空
    pub html: Option<String>,
    /// 来源应用（记录时取前台窗口所属进程名）
    pub source_app: Option<String>,
    pub is_pinned: bool,
    /// 条目类型：text / image / file
    pub kind: String,
    /// 图片快照文件路径（kind=image 时非空，位于 app_data_dir/clipboard/images/）
    pub image_path: Option<String>,
    /// 文件路径列表（kind=file 时非空）
    pub file_paths: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 笔记标签
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub created_at: String,
}

/// 倒计时（三种形态统一建模）：
/// - once    一次性：end_at 为绝对时刻，到点置 finished，卡片灰态待删
/// - daily   每天固定时刻：end_at 为当天/次日 HH:MM 时刻，到点顺延 24h
/// - interval 每隔 N 分钟：end_at 为当前轮结束时刻，到点按 interval_minutes 顺延
/// total_ms 为周期总长（once 创建时长 / daily 24h / interval N 分钟），用于水位进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Countdown {
    pub id: i64,
    pub name: String,
    pub repeat_mode: String,
    pub end_at: i64,
    pub total_ms: i64,
    pub interval_minutes: Option<i64>,
    pub paused: bool,
    pub paused_remaining_ms: Option<i64>,
    pub finished: bool,
    pub floated: bool,
    pub float_x: Option<f64>,
    pub float_y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

/// AI 对话会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: i64,
    pub title: String,
    pub model_name: String,
    pub created_at: String,
    pub updated_at: String,
    /// 会话级累计 token（输入 / 输出 / 缓存读取 / 推理）
    #[serde(default)]
    pub tokens_input: i64,
    #[serde(default)]
    pub tokens_output: i64,
    #[serde(default)]
    pub tokens_cache_read: i64,
    #[serde(default)]
    pub tokens_reasoning: i64,
    /// 会话级累计生成耗时（毫秒），用于计算 TPS
    #[serde(default)]
    pub elapsed_ms: i64,
}

/// AI 对话消息（user / assistant）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// 自定义模型配置（不绑定厂商，统一 OpenAI 兼容协议）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatModelConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// 仅保存时携带；读取/落盘时一律清空（真实 Key 存系统钥匙串）
    pub api_key: String,
    pub is_default: bool,
    /// 是否已配置 API Key（返回给前端做状态展示，保存时忽略）
    #[serde(default)]
    pub has_api_key: bool,
    /// 供应商名称（如 DeepSeek / OpenAI），同一 base_url 下的模型归为一组
    #[serde(default)]
    pub provider_name: String,
}
