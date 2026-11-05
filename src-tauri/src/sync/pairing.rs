//! 扫码配对（docs/sync-protocol.md §3.2）：桌面建会话 → 出码 → 轮询。
//! HTTP 全部在锁外（commands 层编排），本模块只组请求与解析。

use serde_json::json;

use super::api;

/// 建配对会话，返回可直接展示的二维码内容 JSON 字符串。
/// secret 只在本返回值出现一次，服务端只存 sha256；meta 存 pid 供轮询/取消。
pub async fn begin() -> Result<String, String> {
    let resp = api::owner_post("/sync/pairing", json!({ "device_name": api::device_name() })).await?;
    let pid = resp["pairing_id"].as_str().ok_or("服务端缺 pairing_id")?.to_string();
    let secret = resp["pairing_secret"].as_str().ok_or("服务端缺 pairing_secret")?.to_string();
    let qr = api::qr_payload(&pid, &secret, &api::device_name());
    Ok(qr.to_string())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollResult {
    /// pending / completed / expired / cancelled
    pub status: String,
    pub device_name: Option<String>,
}

pub async fn poll(pairing_id: &str) -> Result<PollResult, String> {
    let path = format!("/sync/pairing/{pairing_id}");
    let resp = api::owner_get(&path).await?;
    Ok(PollResult {
        status: resp["status"].as_str().unwrap_or("pending").to_string(),
        device_name: resp["device_name"].as_str().map(str::to_string),
    })
}

pub async fn cancel(pairing_id: &str) -> Result<(), String> {
    let path = format!("/sync/pairing/{pairing_id}");
    api::owner_delete(&path).await.map(|_| ())
}
