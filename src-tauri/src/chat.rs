use crate::config;
use crate::models::{ChatMessage, ChatModelConfig, ChatSession};
use futures_util::TryStreamExt;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

/// 流式事件（发送到前端 Channel）
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatStreamEvent {
    /// 增量文本
    Chunk { content: String },
    /// 完整回复已落库（携带最终消息 + 更新后的会话：含自动标题与累计 token）
    Done { message: ChatMessage, session: ChatSession },
    /// 出错（携带已生成的增量，前端可保留）
    Error { message: String, partial: String },
}

/// 单轮回复的 token 用量（OpenAI 兼容 usage 字段）
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ChatUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub reasoning: i64,
}

/// API Key 仅存系统钥匙串，失败时明确报错，不降级写入明文。
const KEYRING_SERVICE: &str = "x-hub-chat";

/// 平台中转的占位 Key：真实凭据是账号会话 token（登录态），不写进配置也不写钥匙串。
/// 这样退出登录/换号时不需要清理任何模型配置，token 失效会自然表现为 401。
pub const PLATFORM_KEY_SENTINEL: &str = "__xhub_platform__";

/// 平台额度在界面上的**单一入口名**：设置里它只是一个「开启/关闭」开关，AI 对话的模型下拉里
/// 也只有这一项（不再逐个列出 `platform:<模型名>`）。会话的 `model_name` 存这个入口名，
/// 具体用哪个平台模型由后端在全部平台条目之间负载切换（`commands::pick_chat_model`）。
pub const PLATFORM_ENTRY_NAME: &str = "x-hub 平台";

/// 是否平台额度条目（`platform:<模型名>`）：这是后端识别「用账号登录态换额度」那批条目的唯一依据，
/// 界面折叠（一个开关 / 一个下拉项）都建立在这个前缀上。
pub fn is_platform_model(m: &ChatModelConfig) -> bool {
    m.id.starts_with("platform:")
}

/// 解析某模型实际要用的 Key，并回答「它是不是平台模型」：
/// 平台占位符 → 账号 token（未登录则视为未配置）
fn resolve_api_key(model_id: &str) -> (Option<String>, bool) {
    match get_api_key(model_id) {
        Some(k) if k == PLATFORM_KEY_SENTINEL => (crate::account::session_token(), true),
        other => (other, false),
    }
}

/// 平台模型的 base_url：**由内置服务端地址现算，不信任配置里存的值**。
///
/// 平台模型是「登录后用账号额度」的那批条目（`platform:<模型名>`）。用户在服务端地址
/// 还可配置的旧版本里添加过它们，条目里存的是当时的地址；地址常量换域名后那些条目会
/// 一直打旧地址，而设置里已经没有地址入口可供改回（见 AGENTS.md 约定 52）。
/// 自备 Key 的供应商不受影响——它们的 base_url 是用户自己的，永远以配置为准。
pub fn platform_base_url() -> String {
    format!("{}/v1", crate::account::server_url())
}

pub fn save_api_key(model_id: &str, key: &str) -> Result<(), String> {
    // 迁移失败不阻断写入（理由同 account::save_token）：旧 chat_keys.json 损坏或删不掉时，
    // 不该连带让用户再也存不进新的 API Key；钥匙串不可用时下面的 set_password 仍会报错。
    if let Err(e) = crate::credentials::migrate_to_keyring(&key_file_path(), KEYRING_SERVICE, None) {
        log::warn!("旧 AI 凭据迁移未完成: {e}");
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, model_id).map_err(|e| e.to_string())?;
    entry.set_password(key).map_err(|e| format!("钥匙串写入失败，未保存明文: {e}"))
}

pub fn get_api_key(model_id: &str) -> Option<String> {
    if let Err(e) = crate::credentials::migrate_to_keyring(&key_file_path(), KEYRING_SERVICE, None) {
        log::warn!("旧 AI 凭据迁移未完成: {e}");
    }
    match keyring::Entry::new(KEYRING_SERVICE, model_id) {
        Ok(e) => e.get_password().ok(),
        Err(_) => None,
    }
}

fn key_file_path() -> std::path::PathBuf {
    config::config_dir().join("chat_keys.json")
}

/// 拉取平台可用模型（「使用平台免费额度」用；需登录账号）
#[tauri::command]
pub async fn platform_models() -> Result<Vec<String>, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先在「设置 → 账号」登录")?;
    let v = crate::account::get_json(crate::api_spec::platform_models_path(), &token).await?;
    Ok(v.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default())
}

/// 从对话接口的错误响应体提取可读文案，两处（流式对话 / 拉模型列表）共用。
/// 三种形状按序尝试：自备供应商是 OpenAI 形状 `{"error":{"message":..}}`；
/// 平台中转是扁平形状 `{"error":"quota_exceeded","message":"AI 额度已用完"}`（message 优先展示）；
/// 都不在（非 JSON / HTML 错误页）时退回原文截断。
fn extract_error_detail(body: &str) -> String {
    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return truncate_body(body),
    };
    if let Some(s) = parsed["error"]["message"].as_str() {
        return s.to_string();
    }
    if let Some(s) = parsed.get("message").and_then(|m| m.as_str()) {
        return s.to_string();
    }
    if let Some(s) = parsed.get("error").and_then(|e| e.as_str()) {
        return s.to_string();
    }
    truncate_body(body)
}

fn truncate_body(body: &str) -> String {
    if body.chars().count() > 300 {
        body.chars().take(300).collect()
    } else {
        body.to_string()
    }
}

/// 发送一次 OpenAI 兼容流式对话请求，逐段回调 on_chunk，并把完整回复累积到 out
///
/// - 协议：`POST {base_url}/chat/completions`，`stream: true`，SSE 逐行解析
/// - 兼容：DeepSeek / OpenAI / 通义 / Moonshot / Ollama(vLLM, one-api 中转) 等一切
///   OpenAI 兼容实现，不绑定任何厂商 SDK
/// - out 由调用方持有并持续追加，流式期间不产生第二份全量副本（成功即完整回复，
///   出错时保留已生成部分供前端展示）
pub async fn stream_chat<F>(
    model: &ChatModelConfig,
    messages: &[ChatMessage],
    out: &mut String,
    mut on_chunk: F,
) -> Result<ChatUsage, String>
where
    F: FnMut(String) -> Result<(), String>,
{
    let (api_key, is_platform) = resolve_api_key(&model.id);
    let api_key = api_key.ok_or_else(|| {
        format!("模型「{}」未配置 API Key（平台模型需先登录账号）", model.name)
    })?;

    // 平台模型走内置服务端（国内）→ 强制直连；用户自配的第三方服务保持跟随系统代理（见 crate::net）
    let client = if is_platform {
        crate::net::direct()
    } else {
        reqwest::Client::builder()
    }
    .redirect(reqwest::redirect::Policy::none())
    .timeout(std::time::Duration::from_secs(300))
    .build()
    .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))?;

    // OpenAI 消息格式（role 仅保留 user/assistant 两种）
    let payload_messages: Vec<Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    // 平台模型的地址一律指向内置服务端（理由见 platform_base_url 注释）
    let base_url = if is_platform {
        platform_base_url()
    } else {
        model.base_url.trim().to_string()
    };
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    crate::credentials::validate_endpoint(&url)?;
    let payload = serde_json::json!({
        "model": model.model,
        "messages": payload_messages,
        "stream": true,
        // 让 OpenAI 兼容后端在最后一个 chunk 返回 usage（输入/输出/缓存/推理 token）
        "stream_options": { "include_usage": true },
    });

    let resp = client
        .post(&url)
        .bearer_auth(api_key.trim())
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            // reqwest 的 Error 不含响应体，单独透出网络层错误
            format!("请求失败: {}", e)
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // 常见错误提取：OpenAI 形状 error.message / 平台中转扁平 message / 原文截断
        return Err(format!("模型接口返回 {}: {}", status, extract_error_detail(&body)));
    }

    // 逐行解析 SSE：data: {json}，[DONE] 结束；delta.content 为增量
    let stream = resp
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));

    let mut reader = BufReader::new(tokio_util::io::StreamReader::new(stream));
    let mut usage = ChatUsage::default();
    // 流式推送攒批：首段立即推（保住首字延迟），后续增量先攒入 pending，
    // 达到 ≥64 字符或间隔 ≥40ms 才合并推送一次——避免几千个 1~4 字符的小 chunk
    // 逐个走 IPC，把主线程/渲染成本摊薄到 ~25 次/s，长回复整体明显更跟手
    let mut pending = String::new();
    let mut last_flush = std::time::Instant::now();
    let mut sent_any = false;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("读取流失败: {}", e))?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(data) = trimmed.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let parsed: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // usage 通常在最后一个 chunk（开启 include_usage 后），或每个 chunk 都可能带
            if let Some(u) = parsed.get("usage") {
                usage = usage_from_json(u);
            }
            if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                if !delta.is_empty() {
                    out.push_str(delta);
                    if !sent_any {
                        sent_any = true;
                        on_chunk(delta.to_string()).map_err(|e| e)?;
                        last_flush = std::time::Instant::now();
                    } else {
                        pending.push_str(delta);
                        if pending.len() >= 64 || last_flush.elapsed().as_millis() >= 40 {
                            let batch = std::mem::take(&mut pending);
                            on_chunk(batch).map_err(|e| e)?;
                            last_flush = std::time::Instant::now();
                        }
                    }
                }
            }
        }
    }
    // 结束时补推残留的批量增量
    if !pending.is_empty() {
        on_chunk(pending).map_err(|e| e)?;
    }

    Ok(usage)
}

/// 解析 OpenAI 兼容 usage 字段（含 DeepSeek 的 prompt_cache_hit_tokens 别名）
fn usage_from_json(v: &Value) -> ChatUsage {
    ChatUsage {
        input: v["prompt_tokens"].as_i64().unwrap_or(0),
        output: v["completion_tokens"].as_i64().unwrap_or(0),
        cache_read: v["prompt_tokens_details"]["cached_tokens"]
            .as_i64()
            .or_else(|| v["prompt_cache_hit_tokens"].as_i64())
            .unwrap_or(0),
        reasoning: v["completion_tokens_details"]["reasoning_tokens"]
            .as_i64()
            .unwrap_or(0),
    }
}

/// 连通性测试 + 拉取模型列表：`GET {base_url}/models`（OpenAI 兼容）
///
/// 返回结构：
/// - `Ok(provider)`：连通成功，携带可用的模型 id 列表（`data[].id`）
/// - `Err(msg)`：连通失败，透出可读错误信息
pub async fn fetch_provider_models(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    crate::credentials::validate_endpoint(base_url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))?;

    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .bearer_auth(api_key.trim())
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("接口返回 {}: {}", status, extract_error_detail(&body)));
    }

    let body: Value = resp.json().await.map_err(|e| format!("解析响应失败: {}", e))?;
    let ids: Vec<String> = body["data"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v["id"].as_str().map(|s| s.to_string()))
        .collect();
    if ids.is_empty() {
        return Err("连接成功，但未获取到可用模型（返回的 data 为空）".to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ChatMessage;

    #[test]
    fn extract_error_detail_covers_both_shapes() {
        // OpenAI 形状（自备供应商）
        assert_eq!(
            extract_error_detail(r#"{"error":{"message":"Incorrect API key"}}"#),
            "Incorrect API key"
        );
        // 平台中转扁平形状：可读 message 优先于错误码
        assert_eq!(
            extract_error_detail(r#"{"error":"quota_exceeded","message":"AI 额度已用完"}"#),
            "AI 额度已用完"
        );
        // 老服务端只有错误码时原样带出（前端有映射兜底）
        assert_eq!(extract_error_detail(r#"{"error":"server_busy"}"#), "server_busy");
        // 非 JSON（网关 HTML 错误页）退回原文截断
        assert_eq!(extract_error_detail("<html>502</html>"), "<html>502</html>");
    }

    #[test]
    fn build_messages_map() {
        let msgs = vec![
            ChatMessage {
                id: 1,
                session_id: 1,
                role: "user".into(),
                content: "你好".into(),
                created_at: String::new(),
            },
            ChatMessage {
                id: 2,
                session_id: 1,
                role: "assistant".into(),
                content: "你好！".into(),
                created_at: String::new(),
            },
        ];
        let v: Vec<Value> = msgs
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["role"], "user");
        assert_eq!(v[1]["content"], "你好！");
    }
}
