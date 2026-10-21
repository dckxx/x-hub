//! 扩展反馈板：提交 / 公开列表 / 开发者收件箱。
//!
//! 反馈托管在 x-hub-server，不依赖开发者自留的外部渠道：匿名可提交（登录则带用户名），
//! 公开可见；开发者在自己客户端的「我的扩展 → 用户反馈」里看完整信息（联系方式 / 诊断 /
//! 机器环境）并公开回复。
//!
//! 请求规格（路径与键名）的唯一来源是 `crate::api_spec`；本模块只做参数校验、取 token/mid
//! 与响应解析。鉴权走 `account::post_json` / `get_json` 的既有语义（401 自动清理本地会话）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// 提交/收件箱的超时：都是用户显式动作，15s 足够；未读轮询失败由前端静默处理。
const REQUEST_TIMEOUT_SECS: u64 = 15;

/// 一条反馈。公开接口只带前半段（`contact` / `diagnostics` / `app_version` / `hidden` 缺省），
/// 开发者接口返回全部 —— 同一个结构体靠 `Option` 兼容两种响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionFeedback {
    pub id: i64,
    /// bug / idea / question
    pub kind: String,
    /// 提交时的展示名；None = 匿名
    #[serde(default)]
    pub author_name: Option<String>,
    pub body: String,
    /// open / replied / resolved
    pub status: String,
    #[serde(default)]
    pub reply: Option<String>,
    #[serde(default)]
    pub replied_at: Option<i64>,
    #[serde(default)]
    pub ext_version: Option<String>,
    pub created_at: i64,
    // ---- 仅开发者接口返回 ----
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub diagnostics: Option<String>,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

/// 公开反馈列表的一页：`total` 是全部条数（不受 limit 截断），详情弹窗用它显示「共 N 条」
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionFeedbackPage {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub items: Vec<ExtensionFeedback>,
}

/// 收件箱里一个扩展的未读数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackUnreadExtension {
    pub ext_id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub unread: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackUnread {
    pub total: i64,
    #[serde(default)]
    pub extensions: Vec<FeedbackUnreadExtension>,
}

fn require_token() -> Result<String, String> {
    crate::account::session_token()
        .ok_or_else(|| "UNAUTHORIZED: 请先在「设置 → 账号」登录".to_string())
}

/// 匿名 GET（公开反馈列表）：**不带 Authorization** —— 本地持有一个过期 token 时，
/// 公开读取不该因此 401（公开列表本来就允许匿名读）。
async fn get_public(path: &str) -> Result<Value, String> {
    let url = format!("{}{path}", crate::account::server_url());
    let client = crate::net::direct()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("NETWORK_ERROR: HTTP 客户端初始化失败 {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("NETWORK_ERROR: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::account::api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

fn parse_items(v: &Value) -> Result<Vec<ExtensionFeedback>, String> {
    let items = v
        .get("items")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    serde_json::from_value(items).map_err(|e| format!("INTERNAL: 反馈列表解析失败 {e}"))
}

/// 提交反馈（匿名可；登录时服务端按账号记展示名）。
///
/// 参数校验在本地先做一遍（快速失败、文案比服务端 `CODE:` 更友好），服务端仍会独立校验。
#[tauri::command]
pub async fn submit_extension_feedback(
    ext_id: String,
    kind: String,
    body: String,
    contact: Option<String>,
    diagnostics: Option<String>,
    app_version: Option<String>,
    ext_version: Option<String>,
) -> Result<i64, String> {
    let ext_id = ext_id.trim().to_string();
    if ext_id.is_empty() {
        return Err("INVALID: 扩展 id 为空".to_string());
    }
    if !matches!(kind.as_str(), "bug" | "idea" | "question") {
        return Err("INVALID: 反馈类型不合法".to_string());
    }
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err("INVALID: 请填写详细描述".to_string());
    }
    if body.chars().count() > 500 {
        return Err("INVALID: 描述最多 500 字".to_string());
    }
    let contact: String = contact
        .unwrap_or_default()
        .trim()
        .chars()
        .take(100)
        .collect();
    let diagnostics = diagnostics.unwrap_or_default();
    if diagnostics.chars().count() > 2000 {
        return Err("INVALID: 诊断信息过长".to_string());
    }
    let app_version: String = app_version.unwrap_or_default().chars().take(40).collect();
    let ext_version: String = ext_version.unwrap_or_default().chars().take(40).collect();

    // mid 取不到时留空：服务端按 IP 兜底限频，不因数据根不可写而挡住提交
    let mid = crate::market::install_mid().unwrap_or_default();
    let token = crate::account::session_token();

    let payload = crate::api_spec::FeedbackSubmit {
        ext_id: &ext_id,
        mid: &mid,
        kind: &kind,
        body: &body,
        contact: &contact,
        diagnostics: &diagnostics,
        app_version: &app_version,
        ext_version: &ext_version,
    };
    let (url, json_body) =
        crate::api_spec::extension_feedback_submit(&crate::account::server_url(), &payload);
    let v = crate::account::post_json(&url, token.as_deref(), json_body).await?;
    v.get("id")
        .and_then(|x| x.as_i64())
        .ok_or_else(|| "INTERNAL: 响应缺少反馈 id".to_string())
}

/// 某扩展的公开反馈列表（匿名可读；不含联系方式与诊断）。
#[tauri::command]
pub async fn list_extension_feedback(ext_id: String) -> Result<ExtensionFeedbackPage, String> {
    let path = crate::api_spec::extension_feedback_list_path(ext_id.trim());
    let v = get_public(&path).await?;
    serde_json::from_value(v).map_err(|e| format!("INTERNAL: 反馈列表解析失败 {e}"))
}

/// 我发布的扩展与未读数（收件箱红点）。未登录返回空结构而不是 Err ——
/// 红点是辅助提示，未登录时静默无红点即可，不该在界面上报一句登录错误。
#[tauri::command]
pub async fn feedback_inbox_unread() -> Result<FeedbackUnread, String> {
    let Some(token) = crate::account::session_token() else {
        return Ok(FeedbackUnread {
            total: 0,
            extensions: Vec::new(),
        });
    };
    let v = crate::account::get_json(crate::api_spec::feedback_unread_path(), &token).await?;
    serde_json::from_value(v).map_err(|e| format!("INTERNAL: 未读数据解析失败 {e}"))
}

/// 开发者收件箱完整列表（含已隐藏、联系方式与诊断；需登录且为扩展所有者）。
#[tauri::command]
pub async fn feedback_inbox_list(ext_id: String) -> Result<Vec<ExtensionFeedback>, String> {
    let token = require_token()?;
    let path = crate::api_spec::feedback_inbox_list_path(ext_id.trim());
    let v = crate::account::get_json(&path, &token).await?;
    parse_items(&v)
}

/// 把这批反馈标记为已读（服务端水位推到该扩展当前最大 id）。
#[tauri::command]
pub async fn feedback_mark_read(ext_id: String) -> Result<(), String> {
    let token = require_token()?;
    let (url, body) =
        crate::api_spec::feedback_mark_read(&crate::account::server_url(), ext_id.trim());
    crate::account::post_json(&url, Some(&token), body)
        .await
        .map(|_| ())
}

/// 公开回复（可同时标记为已解决）。
#[tauri::command]
pub async fn feedback_reply(id: i64, reply: String, resolved: bool) -> Result<(), String> {
    let token = require_token()?;
    let reply = reply.trim().to_string();
    if reply.is_empty() {
        return Err("INVALID: 回复不能为空".to_string());
    }
    if reply.chars().count() > 500 {
        return Err("INVALID: 回复最多 500 字".to_string());
    }
    let status = if resolved { "resolved" } else { "replied" };
    let (url, body) =
        crate::api_spec::feedback_reply(&crate::account::server_url(), id, &reply, status);
    crate::account::post_json(&url, Some(&token), body)
        .await
        .map(|_| ())
}

/// 隐藏 / 恢复一条反馈（软删：公开面消失，管理员仍可见）。
#[tauri::command]
pub async fn feedback_hide(id: i64, hidden: bool) -> Result<(), String> {
    let token = require_token()?;
    let (url, body) = crate::api_spec::feedback_hide(&crate::account::server_url(), id, hidden);
    crate::account::post_json(&url, Some(&token), body)
        .await
        .map(|_| ())
}
