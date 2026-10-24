use crate::config;
use crate::models::{ChatMessage, ChatModelConfig, ChatSession};
use futures_util::TryStreamExt;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

/// 流式事件（发送到前端 Channel）
/// `rename_all` 只重命名变体名（ToolCall → toolCall），字段必须再补
/// `rename_all_fields`（turn_id → turnId）——只写前者时字段按 snake_case 序列化，
/// 前端读 e.turnId 全是 undefined（0.9.0 审查修复）
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatStreamEvent {
    /// 增量文本
    Chunk { content: String },
    /// 完整回复已落库（携带最终消息 + 更新后的会话：含自动标题与累计 token）
    Done { message: ChatMessage, session: ChatSession },
    /// 出错（携带已生成的增量，前端可保留）
    Error { message: String, partial: String },
    // ---- Agent 循环事件（计划 §4 事件协议，#t44；UI 渲染在 #t46）----
    /// 模型发起工具调用，已获准执行（渲染工具卡片；args 为模型产出的 JSON 原文）
    ToolCall {
        turn_id: String,
        call_id: String,
        name: String,
        args: String,
        /// read / write / dangerous
        level: String,
    },
    /// 工具执行完毕（更新卡片状态；detail 是回给模型的完整结果文本，UI 可折叠展示）
    ToolResult {
        call_id: String,
        ok: bool,
        summary: String,
        detail: String,
    },
    /// Ask 档需用户确认（前端渲染内联确认卡片，经 respond_agent_approval 回答）
    ApprovalRequest {
        turn_id: String,
        call_id: String,
        name: String,
        args: String,
        level: String,
    },
    /// 一轮对话（turn）收尾：最终回复已落库，携带累计 usage 与工具调用数
    TurnDone {
        turn_id: String,
        message: ChatMessage,
        session: ChatSession,
        usage: ChatUsage,
        tool_count: usize,
    },
    /// 回合已开始（#t47+：turn_id 后端生成，开工即推给前端，
    /// 否则纯文本回复要等首个工具事件才拿得到 id，停止键一直出不来）
    TurnStart { turn_id: String },
}

/// 平台供应商预设（后台可配的「添加供应商」快速填充项，#t47）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub name: String,
    pub base_url: String,
    /// 申请 API Key 的指引页（空 = 前端不显示「获取 Key」入口）
    pub key_guide_url: String,
    /// 备注说明（如免费额度、计费方式），空 = 无
    pub note: String,
}

/// 单轮回复的 token 用量（OpenAI 兼容 usage 字段）
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ChatUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub reasoning: i64,
}

impl ChatUsage {
    /// agent 多轮循环的用量累加（一轮对话 = N 次模型调用之和，TurnDone 汇总用）
    pub fn accumulate(&mut self, other: ChatUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.reasoning += other.reasoning;
    }
}

/// 上游请求的一次工具调用（SSE 里 tool_calls 分片累积后的最终形态，#t43）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    /// 模型产出的参数原文（JSON 字符串）；解析与校验由 agent 循环做
    pub arguments: String,
}

/// 一次流式请求的完整结果：usage + 累积出的 tool_calls（模型没发起调用时为空）
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ChatStreamResult {
    pub usage: ChatUsage,
    pub tool_calls: Vec<ToolCallRequest>,
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

/// 拉取平台当前可用模型名清单（`/api/v1/ai/models`，账号登录态鉴权）。
/// `platform_models` 命令与清单对账同步（`commands::sync_platform_models`）共用。
pub async fn fetch_platform_model_names() -> Result<Vec<String>, String> {
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

/// 拉取平台可用模型（「使用平台免费额度」用；需登录账号）
#[tauri::command]
pub async fn platform_models() -> Result<Vec<String>, String> {
    fetch_platform_model_names().await
}

/// 供应商预设（「添加供应商」快速填充；后台可配，需登录账号）。
/// 拉取失败时前端回落内置兜底清单，本命令的 Err 不阻塞用户配置。
#[tauri::command]
pub async fn platform_provider_presets() -> Result<Vec<ProviderPreset>, String> {
    let token = crate::account::session_token().ok_or("UNAUTHORIZED: 请先在「设置 → 账号」登录")?;
    let v = crate::account::get_json(crate::api_spec::provider_presets_path(), &token).await?;
    Ok(v.get("presets")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| {
                    let name = x.get("name")?.as_str()?.to_string();
                    Some(ProviderPreset {
                        name,
                        base_url: x.get("base_url").and_then(|b| b.as_str()).unwrap_or("").to_string(),
                        key_guide_url: x
                            .get("key_guide_url")
                            .and_then(|k| k.as_str())
                            .unwrap_or("")
                            .to_string(),
                        note: x.get("note").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    })
                })
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
    extract_error_detail_from(&parsed)
}

/// 已解析版本：SSE 流内错误帧（已 parse 过）不必再解一次 JSON
fn extract_error_detail_from(parsed: &Value) -> String {
    if let Some(s) = parsed["error"]["message"].as_str() {
        return s.to_string();
    }
    if let Some(s) = parsed.get("message").and_then(|m| m.as_str()) {
        return s.to_string();
    }
    if let Some(s) = parsed.get("error").and_then(|e| e.as_str()) {
        return s.to_string();
    }
    truncate_body(&parsed.to_string())
}

fn truncate_body(body: &str) -> String {
    if body.chars().count() > 300 {
        body.chars().take(300).collect()
    } else {
        body.to_string()
    }
}

/// 等待中断旗标置位（50ms 轮询：实现最简单，等待侧开销可忽略）。
/// 无旗标（普通对话/笔记整理）时永不完成——select! 里就是纯透传。
async fn wait_cancelled(cancel: &Option<std::sync::Arc<std::sync::atomic::AtomicBool>>) {
    match cancel {
        Some(flag) => loop {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        },
        None => std::future::pending::<()>().await,
    }
}

/// 判断一个已解析的 SSE 帧是不是**流内错误**，是则返回可展示的文案。
///
/// 这是 0.9.2「发消息不回复、只留一个空气泡」的根因所在：上游用 HTTP 200 开场，
/// 错误写在 SSE 正文里（平台中转实测原样返回
/// `data: {"error":{"message":"当前模型暂无可用供应方（协议 OPENAI）…","type":"api_error"}}`
/// 紧跟 `data: [DONE]`）。旧解析只认 `choices[].delta.content`，错误帧被当成
/// 「没有正文的普通帧」跳过，整轮于是以「正常结束 + 0 字符」收场。
///
/// `error` 为 null / 缺失都算正常帧——部分网关每帧都带 `"error": null`。
fn sse_error_detail(parsed: &Value) -> Option<String> {
    let err = parsed.get("error").filter(|e| !e.is_null())?;
    // 形状与 HTTP 非 2xx 的错误体一致，复用同一提取器（对象取 message、扁平码取原文）
    let detail = extract_error_detail_from(parsed);
    if detail.trim().is_empty() {
        Some(err.to_string())
    } else {
        Some(detail)
    }
}

/// 平台 429 响应体是否为积分不足（#t39：`{"error":"insufficient_credits","remaining":0}`）。
/// 只认扁平 error 码，避免把「正文里恰好含这个词」的其它错误误判成积分不足。
fn is_insufficient_credits(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s == "insufficient_credits"))
        .unwrap_or(false)
}

/// 平台 400 响应体是否为「模型不可用」（`{"error":"model_not_allowed","model":"..."}`）：
/// 请求带的模型名不在服务端当前可用清单（被下架/改名/未对用户开放）。
/// 同为扁平 error 码精确匹配，理由同 `is_insufficient_credits`。
fn is_model_not_allowed(body: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s == "model_not_allowed"))
        .unwrap_or(false)
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
    on_chunk: F,
) -> Result<ChatUsage, String>
where
    F: FnMut(String) -> Result<(), String>,
{
    stream_chat_ex(model, messages, None, None, out, on_chunk)
        .await
        .map(|r| r.usage)
}

/// 扩展版（agent 循环用，#t43）：
/// - `tools`：OpenAI tools 数组（权限档过滤后），非空时随请求下发
/// - `turn_id`：本 turn 标识；平台请求以 `X-XHub-Turn-Id` 头透传，仅作服务端对账、不影响计费
/// - `cancel`：中断旗标（agent 循环的用户取消）。不只是回调前检查——SSE 读取本身
///   也挂在 select 上，**模型还没吐出首个分片时点停止也能立刻掐断等待**（否则要
///   干等到上游超时，前端"停止"按钮形同虚设）
/// - 返回 `ChatStreamResult`：usage + 累积完整的 tool_calls（模型发起工具调用时非空）
pub async fn stream_chat_ex<F>(
    model: &ChatModelConfig,
    messages: &[ChatMessage],
    tools: Option<&[Value]>,
    turn_id: Option<&str>,
    out: &mut String,
    on_chunk: F,
) -> Result<ChatStreamResult, String>
where
    F: FnMut(String) -> Result<(), String>,
{
    stream_chat_ex_with_cancel(model, messages, tools, turn_id, None, out, on_chunk).await
}

/// 带中断旗标的完整版：`cancel` 为 Some 时流式读取可被外部随时掐断。
pub async fn stream_chat_ex_with_cancel<F>(
    model: &ChatModelConfig,
    messages: &[ChatMessage],
    tools: Option<&[Value]>,
    turn_id: Option<&str>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    out: &mut String,
    mut on_chunk: F,
) -> Result<ChatStreamResult, String>
where
    F: FnMut(String) -> Result<(), String>,
{
    // OpenAI 消息格式：tool 消息 / 带 tool_calls 的 assistant 按协议展开（见 payload_message）
    let payload_messages: Vec<Value> = messages.iter().map(payload_message).collect();

    // 平台 400 自愈（约定 33）：服务端后台加减模型后，本地快照里的旧模型名会撞
    // 400 model_not_allowed。此时同步一次平台清单并换用新条目重试（仅一次）——
    // 服务端改名/换模型对用户无感，不必手动重开「平台额度」开关。
    let mut model = model.clone();
    let mut retried_platform_sync = false;
    let resp = loop {
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
        // 不能设总超时：SSE 的整个生成期都算「读正文」，总帽会把超长回复中途掐死
        // （0.9.2 反馈「AI 回复被截断」的客户端侧成因）。改为连接 15s + 单次读 120s
        // 空闲超时——只要上游还在吐字就不限时，真卡死才报错
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {}", e))?;

        // 平台模型的地址一律指向内置服务端（理由见 platform_base_url 注释）
        let base_url = if is_platform {
            platform_base_url()
        } else {
            model.base_url.trim().to_string()
        };
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        crate::credentials::validate_endpoint(&url)?;
        let mut payload = serde_json::json!({
            "model": model.model,
            "messages": &payload_messages,
            "stream": true,
            // 让 OpenAI 兼容后端在最后一个 chunk 返回 usage（输入/输出/缓存/推理 token）
            "stream_options": { "include_usage": true },
        });
        // 工具清单：仅 agent 循环传（普通对话 / 笔记整理不带，省 token 也避免模型乱调）
        if let Some(tools) = tools.filter(|t| !t.is_empty()) {
            payload["tools"] = Value::Array(tools.to_vec());
        }

        let mut req = client
            .post(&url)
            .bearer_auth(api_key.trim())
            .header("Content-Type", "application/json");
        // turn 标识仅平台链路需要（服务端 usage_logs 对账用，§8）；BYOK 直连不发
        if is_platform {
            if let Some(t) = turn_id.map(str::trim).filter(|t| !t.is_empty()) {
                req = req.header("X-XHub-Turn-Id", t);
            }
        }
        let resp = req
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
            // 平台积分不足（429 insufficient_credits，#t39）单独成句：文案可读 + 带稳定 token，
            // 前端据此渲染「积分不足」分流卡片（去兑换 / 配置自备 Key），而不是裸报错
            if is_platform && status == reqwest::StatusCode::TOO_MANY_REQUESTS && is_insufficient_credits(&body) {
                return Err(
                    "积分余额不足（insufficient_credits）：可到「设置 → 账号」兑换积分，或在「设置 → AI 助手」配置自备 API Key"
                        .into(),
                );
            }
            if is_platform
                && status == reqwest::StatusCode::BAD_REQUEST
                && is_model_not_allowed(&body)
                && !retried_platform_sync
            {
                retried_platform_sync = true;
                match crate::commands::sync_platform_models().await {
                    Ok(Some(diff)) => {
                        log::info!("平台模型清单自愈同步: +{:?} -{:?}", diff.added, diff.removed);
                    }
                    Ok(None) => {}
                    Err(e) => log::warn!("平台模型清单自愈同步失败: {e}"),
                }
                // 换到别的平台模型才值得重试：清单没变（还是同一个名字）就原样报错
                if let Some(fresh) = crate::commands::pick_fresh_platform_model() {
                    if fresh.model != model.model {
                        log::info!(
                            "平台模型 400 自愈: 「{}」→「{}」，换模型重试一次",
                            model.model,
                            fresh.model
                        );
                        model = fresh;
                        continue;
                    }
                }
            }
            // 常见错误提取：OpenAI 形状 error.message / 平台中转扁平 message / 原文截断
            return Err(format!("模型接口返回 {}: {}", status, extract_error_detail(&body)));
        }
        break resp;
    };

    // 逐行解析 SSE：data: {json}，[DONE] 结束；delta.content 为文本增量，
    // delta.tool_calls 为工具调用分片（按 index 对位累积，见 accumulate_tool_call）
    let stream = resp
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));

    let mut reader = BufReader::new(tokio_util::io::StreamReader::new(stream));
    let mut usage = ChatUsage::default();
    let mut tool_acc: Vec<ToolCallAccum> = Vec::new();
    // 流式推送攒批：首段立即推（保住首字延迟），后续增量先攒入 pending，
    // 达到 ≥64 字符或间隔 ≥40ms 才合并推送一次——避免几千个 1~4 字符的小 chunk
    // 逐个走 IPC，把主线程/渲染成本摊薄到 ~25 次/s，长回复整体明显更跟手
    let mut pending = String::new();
    let mut last_flush = std::time::Instant::now();
    let mut sent_any = false;
    // 终止判定：[DONE] 或任一帧带 finish_reason 都算完整。都没有就 EOF = 上游中途断流
    //（服务端超时/网关掐线），此时静默当成正常结束就是「回复被截断」的来源——
    // 已有正文时补提示标记，一个字都没有时直接报错
    let mut terminated = false;
    loop {
        let mut line = String::new();
        // 读取与中断旗标竞速：取消时立刻掐断（哪怕上游一个字都还没吐）
        let n = tokio::select! {
            r = reader.read_line(&mut line) => r.map_err(|e| format!("读取流失败: {}", e))?,
            _ = wait_cancelled(&cancel) => return Err("已中断".into()),
        };
        if n == 0 {
            if !terminated {
                if out.is_empty() && tool_acc.is_empty() {
                    return Err("上游连接中断（未返回任何内容）".into());
                }
                if !out.is_empty() {
                    log::warn!("SSE 上游未终止就断流（已收 {} 字符），标记回复可能不完整", out.chars().count());
                    out.push_str("\n\n> ⚠️ 连接中断，这条回复可能不完整。");
                }
            }
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(data) = trimmed.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                // 直接 break 即可：走到这里就已离开循环，不会再去判 EOF 断流
                break;
            }
            let parsed: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // 流内错误帧（HTTP 本来 200，错误在 SSE 正文里）：平台中转发
            // `{"error":{"message":"当前模型暂无可用供应方…","type":"api_error"}}` 后跟 [DONE]，
            // 上游 OpenAI 兼容网关也可能发 `{"error":{...}}`。不识别的话这轮会以
            // 「正常结束 + 0 字符」收场——界面上就是一个没有文字、没有报错的空气泡
            // （0.9.2 反馈「发消息不回复」的成因）。已吐过正文时按断流处理，保留 partial。
            if let Some(detail) = sse_error_detail(&parsed) {
                // 没有可见正文就整轮作废：哪怕是「工具调用攒了一半才报错」也不能放行——
                // 半截的 arguments 拿去执行等于拿残缺 JSON 动用户数据，宁可报错重试
                if out.is_empty() {
                    return Err(format!("模型接口返回错误：{}", detail));
                }
                log::warn!("SSE 中途报错（已收 {} 字符），保留部分回复", out.chars().count());
                out.push_str(&format!("\n\n> ⚠️ 回复中断：{}", detail));
                break;
            }
            // 部分兼容网关不发 [DONE]，靠末帧 finish_reason 收尾——同样算完整
            if parsed["choices"][0]
                .get("finish_reason")
                .is_some_and(|v| !v.is_null())
            {
                terminated = true;
            }
            // usage 通常在最后一个 chunk（开启 include_usage 后），或每个 chunk 都可能带
            if let Some(u) = parsed.get("usage") {
                usage = usage_from_json(u);
            }
            // 工具调用分片：先于文本（可并存——模型可以边说边发起调用）
            if let Some(tcs) = parsed["choices"][0]["delta"]
                .get("tool_calls")
                .and_then(|v| v.as_array())
            {
                for tc in tcs {
                    accumulate_tool_call(&mut tool_acc, tc);
                }
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

    Ok(ChatStreamResult {
        usage,
        tool_calls: finish_tool_calls(tool_acc),
    })
}

/// 单条消息 → 上游请求体格式：tool 消息（tool_call_id）与带 tool_calls 的 assistant
/// 按 OpenAI 协议展开；普通消息与旧版行为一致（role + content）。
fn payload_message(m: &ChatMessage) -> Value {
    let mut obj = serde_json::json!({ "role": m.role, "content": m.content });
    if let Some(tc) = m.tool_calls.as_deref().filter(|s| !s.trim().is_empty()) {
        // 落库时存的是 JSON 数组文本，原样还原给上游（坏 JSON 静默跳过：宁缺勿错）
        if let Ok(v) = serde_json::from_str::<Value>(tc) {
            obj["tool_calls"] = v;
        }
        // 发起调用的 assistant 消息常无正文，OpenAI 协议要求 content 为 null 而不是 ""
        if m.content.is_empty() {
            obj["content"] = Value::Null;
        }
    }
    if let Some(id) = m.tool_call_id.as_deref().filter(|s| !s.is_empty()) {
        obj["tool_call_id"] = Value::String(id.to_string());
    }
    obj
}

/// 条数窗口截断的边界可能落在 agent 回合中间：头部出现没有前置 assistant(tool_calls)
/// 的孤儿 tool 消息，上游会以 400（"tool message must follow tool_calls"）硬拒——
/// 一个 20 次工具调用的回合约 43 行，单独就超过 30 条窗口，此后每次发消息都稳定失败。
/// 从头部丢弃连续的孤儿 tool 行（截断只发生在头部一侧，尾部始终是刚落的 user 消息）。
pub fn trim_leading_orphan_tools(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .skip_while(|m| m.role == "tool")
        .collect()
}

/// tool_calls 分片累积器：SSE 每帧只带一角（index + 可选 id + function.name/arguments 增量），
/// 按 index 对位合并；id 只认首帧（同一次调用的 id 不变），name/arguments 拼接。
#[derive(Default, Clone)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

fn accumulate_tool_call(acc: &mut Vec<ToolCallAccum>, tc: &Value) {
    let idx = tc
        .get("index")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0) as usize;
    if acc.len() <= idx {
        acc.resize(idx + 1, ToolCallAccum::default());
    }
    let slot = &mut acc[idx];
    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
        if !id.is_empty() && slot.id.is_empty() {
            slot.id = id.to_string();
        }
    }
    if let Some(f) = tc.get("function") {
        if let Some(n) = f.get("name").and_then(|v| v.as_str()) {
            // name 在协议里从不分片（分片的只有 arguments）；部分兼容网关每帧都带
            // 完整 name，逐帧拼接会得到「list_todoslist_todos」——与 id 同款只认首帧
            if slot.name.is_empty() {
                slot.name = n.to_string();
            }
        }
        if let Some(a) = f.get("arguments").and_then(|v| v.as_str()) {
            slot.arguments.push_str(a);
        }
    }
}

/// 累积器 → 最终调用清单：丢掉全空的槽位（某些实现会发空 index 占位帧），
/// 缺 id 的调用补 `call_<序号>`（个别网关不回传 id，agent 循环仍需要成对键）。
fn finish_tool_calls(acc: Vec<ToolCallAccum>) -> Vec<ToolCallRequest> {
    acc.into_iter()
        .enumerate()
        .filter(|(_, a)| !(a.id.is_empty() && a.name.is_empty() && a.arguments.is_empty()))
        .map(|(i, a)| ToolCallRequest {
            id: if a.id.is_empty() { format!("call_{i}") } else { a.id },
            name: a.name,
            arguments: a.arguments,
        })
        .collect()
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

    /// 0.9.0 审查回归锁：事件字段必须是 camelCase（rename_all 只管变体名，
    /// 字段漏 rename_all_fields 时前端读 e.turnId 恒为 undefined，类型检查测不出来）
    #[test]
    fn stream_event_fields_serialize_camel_case() {
        let v = serde_json::to_value(ChatStreamEvent::TurnStart { turn_id: "t1".into() })
            .unwrap();
        assert_eq!(v["type"], "turnStart");
        assert_eq!(v["turnId"], "t1");
        assert!(v.get("turn_id").is_none());

        let v = serde_json::to_value(ChatStreamEvent::ToolCall {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            name: "list_todos".into(),
            args: "{}".into(),
            level: "read".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "toolCall");
        assert_eq!(v["turnId"], "t1");
        assert_eq!(v["callId"], "c1");
    }

    /// 条数窗口截在 agent 回合中间：头部孤儿 tool 行被剪掉，成对的消息原样保留
    #[test]
    fn trim_leading_orphan_tools_drops_head_orphans_only() {
        let msg = |id: i64, role: &str| ChatMessage {
            id,
            session_id: 1,
            turn_id: Some("t1".into()),
            role: role.into(),
            content: format!("m{id}"),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            created_at: String::new(),
        };
        // 截断边界落在 tool 结果处：[tool, tool, user] → [user]
        let out = trim_leading_orphan_tools(vec![msg(1, "tool"), msg(2, "tool"), msg(3, "user")]);
        let ids: Vec<i64> = out.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![3]);
        // 无孤儿：原样保留
        let out = trim_leading_orphan_tools(vec![msg(1, "user"), msg(2, "assistant")]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn payload_message_maps_tool_fields() {
        // 普通消息：与旧版一致（role + content）
        let user = ChatMessage {
            id: 1,
            session_id: 1,
            turn_id: Some("turn_1".into()),
            role: "user".into(),
            content: "你好".into(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            created_at: String::new(),
        };
        let v = payload_message(&user);
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"], "你好");
        assert!(v.get("tool_calls").is_none() && v.get("tool_call_id").is_none());

        // 发起调用的 assistant：tool_calls 还原成 JSON 数组，空正文转 null
        let assistant = ChatMessage {
            tool_calls: Some(r#"[{"id":"call_1","type":"function","function":{"name":"list_todos","arguments":"{}"}}]"#.into()),
            content: "".into(),
            role: "assistant".into(),
            ..user.clone()
        };
        let v = payload_message(&assistant);
        assert!(v["tool_calls"].is_array());
        assert!(v["content"].is_null());

        // tool 结果消息：tool_call_id 必须带上
        let tool = ChatMessage {
            role: "tool".into(),
            content: "[]".into(),
            tool_call_id: Some("call_1".into()),
            ..user.clone()
        };
        let v = payload_message(&tool);
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "call_1");
    }

    #[test]
    fn tool_call_fragments_accumulate() {
        // 三类真实分片形态：同 index 多帧续参、多调用并行（index 1）、空占位帧
        let mut acc: Vec<ToolCallAccum> = Vec::new();
        let frames = vec![
            serde_json::json!({"index":0,"id":"call_1","type":"function","function":{"name":"list_todos","arguments":""}}),
            serde_json::json!({"index":0,"function":{"arguments":"{\"filter\""}}),
            serde_json::json!({"index":0,"function":{"arguments":":\"today\"}"}}),
            serde_json::json!({"index":1,"id":"call_2","type":"function","function":{"name":"create_note"}}),
            serde_json::json!({"index":1,"function":{"arguments":"{\"title\":\"x\"}"}}),
        ];
        for f in &frames {
            accumulate_tool_call(&mut acc, f);
        }
        let done = finish_tool_calls(acc);
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].id, "call_1");
        assert_eq!(done[0].name, "list_todos");
        assert_eq!(done[0].arguments, r#"{"filter":"today"}"#);
        assert_eq!(done[1].id, "call_2");
        assert_eq!(done[1].name, "create_note");

        // 缺 id 的调用（个别网关）补 call_<序号>；全空槽位丢弃
        let mut acc: Vec<ToolCallAccum> = Vec::new();
        accumulate_tool_call(&mut acc, &serde_json::json!({"index":0,"function":{"name":"a","arguments":"{}"}}));
        accumulate_tool_call(&mut acc, &serde_json::json!({"index":1}));
        let done = finish_tool_calls(acc);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].id, "call_0");
    }

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
    fn insufficient_credits_only_matches_the_flat_code() {
        // 服务端 #t39 的固定形状
        assert!(is_insufficient_credits(r#"{"error":"insufficient_credits","remaining":0}"#));
        // 其它错误码 / OpenAI 形状 / 非 JSON 一律不算（不能靠正文关键词误判）
        assert!(!is_insufficient_credits(r#"{"error":"rate_limited"}"#));
        assert!(!is_insufficient_credits(r#"{"error":{"code":"insufficient_credits"}}"#));
        assert!(!is_insufficient_credits("plain text"));
    }

    #[test]
    fn model_not_allowed_only_matches_the_flat_code() {
        // 服务端对「清单外的模型名」的固定回包（400 自愈重试的触发条件）
        assert!(is_model_not_allowed(r#"{"error":"model_not_allowed","model":"LongCat-2.0"}"#));
        // 其它 400 / OpenAI 形状 / 非 JSON 一律不算（BYOK 的上游报错不走自愈）
        assert!(!is_model_not_allowed(r#"{"error":"invalid_body"}"#));
        assert!(!is_model_not_allowed(r#"{"error":{"message":"model not found"}}"#));
        assert!(!is_model_not_allowed("plain text"));
    }

    /// 0.9.2 回归：上游把错误写在 SSE 正文里（HTTP 200 + 错误帧 + [DONE]）时，
    /// 客户端必须报错，不能以「正常结束 + 0 字符」收场——那正是「发消息不回复、
    /// 只留一个没有文字也没有报错的空气泡」的成因。
    #[test]
    fn sse_error_frame_is_detected() {
        // 平台中转的实测原样回包（本次故障的现场）
        let v: Value = serde_json::from_str(
            r#"{"error":{"message":"当前模型暂无可用供应方（协议 OPENAI），请稍后重试或联系管理员","type":"api_error"}}"#,
        )
        .unwrap();
        let d = sse_error_detail(&v).expect("错误帧必须被识别");
        assert!(d.contains("暂无可用供应方"), "错误文案要透传给用户：{d}");

        // 扁平 code + 独立 message 形状
        let v: Value =
            serde_json::from_str(r#"{"error":"quota_exceeded","message":"AI 额度已用完"}"#).unwrap();
        assert_eq!(sse_error_detail(&v).as_deref(), Some("AI 额度已用完"));

        // 只有扁平 code 时直接把码报出来
        let v: Value = serde_json::from_str(r#"{"error":"overloaded"}"#).unwrap();
        assert_eq!(sse_error_detail(&v).as_deref(), Some("overloaded"));

        // 正常帧：没有 error 字段，或显式 null（部分网关每帧都带）——都不能误判
        let v: Value = serde_json::from_str(
            r#"{"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}"#,
        )
        .unwrap();
        assert!(sse_error_detail(&v).is_none());
        let v: Value = serde_json::from_str(r#"{"choices":[],"error":null}"#).unwrap();
        assert!(sse_error_detail(&v).is_none(), "error:null 是正常帧，不得误报");
    }

    #[test]
    fn build_messages_map() {
        let msgs = vec![
            ChatMessage {
                id: 1,
                session_id: 1,
                turn_id: None,
                role: "user".into(),
                content: "你好".into(),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
                created_at: String::new(),
            },
            ChatMessage {
                id: 2,
                session_id: 1,
                turn_id: None,
                role: "assistant".into(),
                content: "你好！".into(),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
                tool_status: None,
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
