//! x-hub-server /sync 的 HTTP 客户端（docs/sync-protocol.md §7.2）。
//!
//! 两种身份：主人操作走平台账号 token（account.rs 同一枚），数据端点走设备 token（钥匙串）。
//! 客户端策略与 account.rs 同口径：强制直连（不被本机代理带偏）、20s 超时、X-Device-* 头。

use serde_json::{json, Value};

const TIMEOUT_SECS: u64 = 20;

pub fn base_url() -> String {
    crate::account::server_url()
}

fn client() -> Result<reqwest::Client, String> {
    crate::net::direct()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))
}

fn device_headers(id: Option<&str>) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(id) = id {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(id) {
            headers.insert(reqwest::header::HeaderName::from_static("x-device-id"), v);
        }
    }
    if let Ok(v) = reqwest::header::HeaderValue::from_str(crate::account::device_label().as_str()) {
        headers.insert(reqwest::header::HeaderName::from_static("x-device-label"), v);
    }
    headers
}

async fn send(method: reqwest::Method, path: &str, token: &str, device_id: Option<&str>, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{}{}", base_url(), path);
    let c = client()?;
    let mut req = c
        .request(method, &url)
        .bearer_auth(token)
        .headers(device_headers(device_id));
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| format!("NETWORK_ERROR: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // 401 = 会话/设备失效：交给调用方清本地绑定态（不在这里清，避免误清账号 token）
        let parsed: Option<Value> = serde_json::from_str(&text).ok();
        let code = parsed
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("HTTP_ERROR");
        let msg = parsed
            .as_ref()
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        return Err(format!("{}: {}", code.to_uppercase(), if msg.is_empty() { format!("服务端返回 {}", status.as_u16()) } else { msg.to_string() }));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

/// 主人操作（建空间 / 配对 / 撤销设备 / 换口令）：平台账号 token
pub async fn owner_post(path: &str, body: Value) -> Result<Value, String> {
    let token = crate::account::load_token().ok_or("NOT_LOGGED_IN: 请先在设置中登录 x-hub 账号")?;
    send(reqwest::Method::POST, path, &token, None, Some(body)).await
}

pub async fn owner_delete(path: &str) -> Result<Value, String> {
    let token = crate::account::load_token().ok_or("NOT_LOGGED_IN: 请先在设置中登录 x-hub 账号")?;
    send(reqwest::Method::DELETE, path, &token, None, None).await
}

/// 数据端点（state / changes / push）：设备 token + X-Device-Id
pub async fn device_get(path: &str, token: &str, device_id: &str) -> Result<Value, String> {
    send(reqwest::Method::GET, path, token, Some(device_id), None).await
}

pub async fn device_post(path: &str, token: &str, device_id: &str, body: Value) -> Result<Value, String> {
    send(reqwest::Method::POST, path, token, Some(device_id), Some(body)).await
}

pub async fn device_delete(path: &str, token: &str, device_id: &str) -> Result<Value, String> {
    send(reqwest::Method::DELETE, path, token, Some(device_id), None).await
}

/// 设备名（X-Device-Label 复用 account 的口径：COMPUTERNAME 优先）
pub fn device_name() -> String {
    crate::account::device_label()
}

pub fn qr_payload(pairing_id: &str, secret: &str, desktop_name: &str) -> Value {
    json!({
        "v": 1,
        "srv": base_url(),
        "pid": pairing_id,
        "secret": secret,
        "name": desktop_name,
    })
}

pub async fn owner_get(path: &str) -> Result<Value, String> {
    let token = crate::account::load_token().ok_or("NOT_LOGGED_IN: 请先在设置中登录 x-hub 账号")?;
    send(reqwest::Method::GET, path, &token, None, None).await
}
