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

/// API Key 存系统钥匙串（keyring），失败时回退到本地受限权限文件，保证可用性
const KEYRING_SERVICE: &str = "x-hub-chat";

pub fn save_api_key(model_id: &str, key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, model_id).map_err(|e| e.to_string());
    match entry {
        Ok(e) => e.set_password(key).map_err(|e| format!("钥匙串写入失败: {}", e)),
        Err(_) => save_key_file(model_id, key),
    }
}

pub fn get_api_key(model_id: &str) -> Option<String> {
    match keyring::Entry::new(KEYRING_SERVICE, model_id) {
        Ok(e) => e.get_password().ok(),
        Err(_) => load_key_file(model_id),
    }
}

fn key_file_path() -> std::path::PathBuf {
    config::config_dir().join("chat_keys.json")
}

fn save_key_file(model_id: &str, key: &str) -> Result<(), String> {
    let path = key_file_path();
    let mut keys: serde_json::Map<String, Value> = load_key_file_map();
    keys.insert(model_id.to_string(), Value::String(key.to_string()));
    std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))
        .map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&keys).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_key_file_map() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(key_file_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn load_key_file(model_id: &str) -> Option<String> {
    load_key_file_map()
        .get(model_id)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
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
    let api_key = get_api_key(&model.id).ok_or_else(|| {
        format!("模型「{}」未配置 API Key，请在对话设置中填写", model.name)
    })?;

    let client = reqwest::Client::builder()
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

    let url = format!("{}/chat/completions", model.base_url.trim_end_matches('/'));
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
        // 常见错误提取（OpenAI 兼容实现通常返回 {"error":{"message":...}}）
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                if body.len() > 300 {
                    body[..300].to_string()
                } else {
                    body.clone()
                }
            });
        return Err(format!("模型接口返回 {}: {}", status, detail));
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
    let client = reqwest::Client::builder()
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
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                if body.len() > 300 {
                    body[..300].to_string()
                } else {
                    body.clone()
                }
            });
        return Err(format!("接口返回 {}: {}", status, detail));
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
