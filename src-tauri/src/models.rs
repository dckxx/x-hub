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

/// 笔记标签
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub created_at: String,
}

/// AI 用量单条记录（从 opencode 同步）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tokens_input: i64,
    pub tokens_cache_read: i64,
    pub tokens_output: i64,
    pub tokens_reasoning: i64,
    pub tokens_cache_write: i64,
    pub cost: f64,
    pub time_created: i64,
    pub source: String,
}

/// AI 用量汇总（今日 / 7 日 / 本月 / 累计，三大指标分开）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub today_input: i64,
    pub today_cache_input: i64,
    pub today_output: i64,
    pub today_cost: f64,
    pub seven_day_input: i64,
    pub seven_day_cache_input: i64,
    pub seven_day_output: i64,
    pub seven_day_cost: f64,
    pub month_input: i64,
    pub month_cache_input: i64,
    pub month_output: i64,
    pub month_cost: f64,
    pub total_input: i64,
    pub total_cache_input: i64,
    pub total_output: i64,
    pub total_cost: f64,
    pub record_count: i64,
    pub last_sync_at: Option<i64>,
}

/// 按天趋势
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDaily {
    pub date: String,
    pub input: i64,
    pub cache_input: i64,
    pub output: i64,
    pub cost: f64,
}

/// 按 provider 排行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageProvider {
    pub provider: String,
    pub count: i64,
    pub input: i64,
    pub cache_input: i64,
    pub output: i64,
    pub cost: f64,
}

/// 用量详情页全量数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDetail {
    pub daily: Vec<UsageDaily>,
    pub providers: Vec<UsageProvider>,
    pub records: Vec<UsageRecord>,
    pub total: i64,
}

/// 同步结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub inserted: i64,
    pub cursor: i64,
    pub listening: bool,
    pub path: Option<String>,
}
