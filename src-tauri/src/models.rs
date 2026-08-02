use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    App,
    Web,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: i64,
    pub group_id: i64,
    pub kind: ResourceKind,
    pub name: String,
    pub target: String,
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
    pub files: Vec<FileEntry>,
}

/// 文件管理条目（仅存储链接，不复制源文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub category: String,
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
