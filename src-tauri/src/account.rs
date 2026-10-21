//! 平台账号（x-hub-server）：登录、额度、开发者身份、提交。
//!
//! 设计要点：
//! - **凭据存储与 `chat.rs` 同口径**：只写系统钥匙串；旧明文文件成功迁移后清理。
//! - **登录方式**（P1a）：GitHub Device Flow 与邮箱验证码，**都由服务端代理**——
//!   客户端只与本机配置的 `server_url` 通信，不直连 GitHub（弱网/受限网络更稳，
//!   且 device_code 只留在服务端）。
//! - **登录只服务「发布与申请」**：消费者装扩展永远不需要账号；自带 API Key 的 AI 对话也不受影响。
//! - **服务端地址是内置常量**（`config::DEFAULT_SERVER_URL`，见约定 52）：正式域名启用后
//!   设置页不再提供地址入口，配置里可能残留的 `server_url`（开发期临时联调地址）一律忽略，
//!   否则老用户升级后会继续打旧地址、又没有任何界面可以改回来。

use serde::Serialize;
use serde_json::Value;

const KEYRING_SERVICE: &str = "x-hub-account";
const TOKEN_KEY: &str = "session-token";
const TIMEOUT_SECS: u64 = 20;

/// 账号状态（前端设置页展示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    /// 是否已登录（本地存在 token）
    pub logged_in: bool,
    /// 服务端地址（空 = 未配置）
    pub server_url: String,
    pub username: String,
    pub role: String,
    pub quota_total: u64,
    pub quota_remaining: u64,
    /// 积分账本（#t39 权威口径；/me credits 双口径之一）。legacy 次数由它派生，
    /// 界面展示与 429 分流都以积分为准
    pub credit_total: u64,
    pub credit_remaining: u64,
    /// 已兑换邀请码（= 有权益：额度 + 可申请开发者）
    pub invite_redeemed: bool,
    /// none / pending / approved / rejected
    pub developer_status: String,
    pub can_apply_developer: bool,
    /// 已登录但拉取失败时的原因（网络不可用等），供界面提示
    pub error: Option<String>,
}

impl AccountStatus {
    fn signed_out(server_url: String) -> Self {
        Self {
            logged_in: false,
            server_url,
            username: String::new(),
            role: String::new(),
            quota_total: 0,
            quota_remaining: 0,
            credit_total: 0,
            credit_remaining: 0,
            invite_redeemed: false,
            developer_status: "none".into(),
            can_apply_developer: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceStart {
    pub poll_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPollResult {
    /// pending | ok | error
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSendResult {
    pub ok: bool,
    /// 服务端未配置发信时给出说明
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevApplyStatus {
    /// none / pending / approved / rejected
    pub status: String,
    pub review_note: String,
    pub is_developer: bool,
    pub invite_redeemed: bool,
}

// ---------------- token 存取（只用系统钥匙串） ----------------

fn token_file() -> std::path::PathBuf {
    crate::config::config_dir().join("account_token.json")
}

fn save_token(token: &str) -> Result<(), String> {
    // 旧明文文件的迁移是「尽力而为」：文件损坏 / 删不掉不该阻断新凭据写入，
    // 否则升级后一个坏掉的 account_token.json 会让登录永远失败（读取侧 load_token 同样只 warn）。
    // 钥匙串本身不可用时下面的 set_password 仍会报错，不会静默丢 token。
    if let Err(e) = crate::credentials::migrate_to_keyring(&token_file(), KEYRING_SERVICE, Some(TOKEN_KEY)) {
        log::warn!("旧账号凭据迁移未完成: {e}");
    }
    match keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY) {
        Ok(entry) => entry
            .set_password(token)
            .map_err(|e| format!("钥匙串写入失败: {e}")),
        Err(_) => Err("系统钥匙串不可用，未保存明文登录凭据".into()),
    }
}

fn load_token() -> Option<String> {
    if let Err(e) = crate::credentials::migrate_to_keyring(&token_file(), KEYRING_SERVICE, Some(TOKEN_KEY)) {
        log::warn!("旧账号凭据迁移未完成: {e}");
    }
    match keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY) {
        Ok(entry) => entry.get_password().ok(),
        Err(_) => None,
    }
}

fn clear_token() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, TOKEN_KEY) {
        let _ = entry.delete_credential();
    }
    let _ = std::fs::remove_file(token_file());
}

// ---------------- 服务端通信 ----------------

/// 平台服务端地址（内置常量，不再读配置——理由见文件头注释）
pub fn server_url() -> String {
    crate::config::DEFAULT_SERVER_URL.to_string()
}

/// 设备名：给服务端「我的设备」列表显示用（不然只能看到 `reqwest/0.12` 这种无意义的 UA）
fn device_label() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| std::env::consts::OS.to_string())
}

fn client() -> Result<reqwest::Client, String> {
    // 每个请求都带上设备名（服务端读 X-Device-Label，缺失才回落 User-Agent）
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&device_label()) {
        headers.insert(
            reqwest::header::HeaderName::from_static("x-device-label"),
            v,
        );
    }
    // 平台服务端在国内：强制直连，别被用户本地代理（Clash/v2rayN）带沟里（见 crate::net）
    crate::net::direct()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))
}

/// 把服务端的错误响应转成 `CODE: message`（前端按既有约定解析）
pub(crate) fn api_error(status: reqwest::StatusCode, body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|v| v.as_str())
        .unwrap_or("HTTP_ERROR");
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if message.is_empty() {
        format!("{}: 服务端返回 {}", code.to_uppercase(), status.as_u16())
    } else {
        format!("{}: {message}", code.to_uppercase())
    }
}

pub(crate) async fn post_json(url: &str, token: Option<&str>, body: Value) -> Result<Value, String> {
    let c = client()?;
    let mut req = c.post(url).json(&body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| format!("NETWORK_ERROR: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // 带 token 的请求收到 401 = 本地会话已失效（典型场景：在「我的设备」里撤销了本机，
        // 或服务端重置了 token）：**立刻清本地 token**，否则界面会继续显示「已登录」，
        // 而用户下一次操作才撞 401，提示与动作完全对不上。
        if token.is_some() && status == reqwest::StatusCode::UNAUTHORIZED {
            clear_token();
            log::warn!("账号会话已失效，已清理本地 token");
        }
        return Err(api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

pub(crate) async fn get_json(path: &str, token: &str) -> Result<Value, String> {
    let base = server_url();
    let c = client()?;
    let resp = c
        .get(format!("{base}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("NETWORK_ERROR: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // 同 post_json：会话失效就地清 token，别留一个「看起来还登录着」的假象
        if status == reqwest::StatusCode::UNAUTHORIZED {
            clear_token();
            log::warn!("账号会话已失效，已清理本地 token");
        }
        return Err(api_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL: 响应解析失败 {e}"))
}

async fn get_me(token: &str) -> Result<Value, String> {
    get_json("/me", token).await
}

fn status_from_me(server: String, me: &Value) -> AccountStatus {
    let quota = me.get("quota").cloned().unwrap_or(Value::Null);
    let credits = me.get("credits").cloned().unwrap_or(Value::Null);
    AccountStatus {
        logged_in: true,
        server_url: server,
        username: me.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        role: me.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string(),
        quota_total: quota.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
        quota_remaining: quota.get("remaining").and_then(|v| v.as_u64()).unwrap_or(0),
        credit_total: credits.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
        credit_remaining: credits.get("remaining").and_then(|v| v.as_u64()).unwrap_or(0),
        invite_redeemed: me.get("invite_redeemed").and_then(|v| v.as_bool()).unwrap_or(false),
        developer_status: me
            .get("developer_status")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_string(),
        can_apply_developer: me.get("can_apply_developer").and_then(|v| v.as_bool()).unwrap_or(false),
        error: None,
    }
}

// ---------------- 命令 ----------------

/// 读取登录状态：未登录只回本地信息；已登录则拉一次 `/me`（网络失败不清 token，只带 error）
#[tauri::command]
pub async fn account_status(app: tauri::AppHandle) -> Result<AccountStatus, String> {
    let _ = app;
    let server = server_url();
    let Some(token) = load_token() else {
        return Ok(AccountStatus::signed_out(server));
    };
    match get_me(&token).await {
        Ok(me) => Ok(status_from_me(server, &me)),
        Err(e) => {
            if e.starts_with("UNAUTHORIZED") {
                // 会话已失效：清掉本地 token，回到未登录态
                clear_token();
                return Ok(AccountStatus::signed_out(server));
            }
            let mut st = AccountStatus::signed_out(server);
            st.logged_in = true; // token 还在，只是暂时拉不到
            st.error = Some(e);
            Ok(st)
        }
    }
}

/// 发起 GitHub 登录（设备码流程）：拿用户码 + 验证地址 + 轮询 id
#[tauri::command]
pub async fn account_login_github_start() -> Result<GithubDeviceStart, String> {
    let base = server_url();
    let (url, body) = crate::api_spec::github_device_start(&base);
    let v = post_json(&url, None, body).await?;
    Ok(GithubDeviceStart {
        poll_id: v.get("poll_id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        user_code: v.get("user_code").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        verification_uri: v.get("verification_uri").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        interval: v.get("interval").and_then(|x| x.as_u64()).unwrap_or(5),
        expires_in: v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900),
    })
}

/// 轮询 GitHub 授权结果；成功即落 token
#[tauri::command]
pub async fn account_login_github_poll(poll_id: String) -> Result<GithubPollResult, String> {
    let base = server_url();
    let (url, body) = crate::api_spec::github_device_poll(&base, &poll_id);
    let v = post_json(&url, None, body).await?;
    let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("pending").to_string();
    if status == "ok" {
        if let Some(token) = v.get("token").and_then(|x| x.as_str()) {
            save_token(token)?;
            let user = v.get("user").and_then(|u| u.get("username")).and_then(|x| x.as_str()).unwrap_or("");
            log::info!("GitHub 登录成功: {user}");
        }
    }
    Ok(GithubPollResult {
        status,
        message: v.get("error").and_then(|x| x.as_str()).map(|s| s.to_string()),
    })
}

/// 发送邮箱验证码（服务端未配置发信时给出明确说明）
#[tauri::command]
pub async fn account_login_email_send(email: String) -> Result<EmailSendResult, String> {
    let base = server_url();
    let (url, body) = crate::api_spec::email_send(&base, &email);
    match post_json(&url, None, body).await {
        Ok(_) => Ok(EmailSendResult { ok: true, message: None }),
        Err(e) if e.starts_with("EMAIL_NOT_CONFIGURED") => Ok(EmailSendResult {
            ok: false,
            message: Some("服务器尚未配置发信，暂时无法用邮箱登录".into()),
        }),
        Err(e) => Err(e),
    }
}

/// 校验邮箱验证码并落 token
#[tauri::command]
pub async fn account_login_email_verify(email: String, code: String) -> Result<AccountStatus, String> {
    let base = server_url();
    let (url, body) = crate::api_spec::email_verify(&base, &email, &code);
    let v = post_json(&url, None, body).await?;
    let token = v.get("token").and_then(|x| x.as_str()).ok_or("INTERNAL: 服务端未返回 token")?;
    save_token(token)?;
    let me = get_me(token).await?;
    Ok(status_from_me(base, &me))
}

/// 退出登录（清本地 token；服务端设备记录由「设置 → 账号 → 我的设备」管理）
#[tauri::command]
pub fn account_logout() -> Result<AccountStatus, String> {
    clear_token();
    log::info!("已退出账号");
    Ok(AccountStatus::signed_out(server_url()))
}

/// 兑换邀请码：账号与权益解耦 —— 兑换后才发额度、才可申请开发者
#[tauri::command]
pub async fn account_redeem(code: String) -> Result<AccountStatus, String> {
    let base = server_url();
    let token = load_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let (url, body) = crate::api_spec::redeem_invite(&base, &code);
    post_json(&url, Some(&token), body).await?;
    let me = get_me(&token).await?;
    Ok(status_from_me(base, &me))
}

/// 提交开发者申请
#[tauri::command]
pub async fn dev_apply(reason: String) -> Result<DevApplyStatus, String> {
    let base = server_url();
    let token = load_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let (url, body) = crate::api_spec::dev_apply(&base, &reason);
    post_json(&url, Some(&token), body).await?;
    dev_apply_status().await
}

/// 查询自己的开发者申请状态
#[tauri::command]
pub async fn dev_apply_status() -> Result<DevApplyStatus, String> {
    let token = load_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let v = get_json(crate::api_spec::dev_apply_status_path(), &token).await?;
    let app = v.get("application").cloned().unwrap_or(Value::Null);
    Ok(DevApplyStatus {
        status: app
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("none")
            .to_string(),
        review_note: app
            .get("review_note")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        is_developer: v.get("is_developer").and_then(|x| x.as_bool()).unwrap_or(false),
        invite_redeemed: v.get("invite_redeemed").and_then(|x| x.as_bool()).unwrap_or(false),
    })
}

/// 我的在线设备（每台设备一条 token；不含 token 明文）
#[tauri::command]
pub async fn account_list_devices() -> Result<Value, String> {
    let token = load_token().ok_or("UNAUTHORIZED: 请先登录")?;
    crate::account::get_json(crate::api_spec::device_tokens_path(), &token).await
}

/// 撤销某一台设备（换机 / 设备丢失时用；上限由服务端控制，超出会自动淘汰最久未用的）
#[tauri::command]
pub async fn account_revoke_device(id: i64) -> Result<Value, String> {
    let token = load_token().ok_or("UNAUTHORIZED: 请先登录")?;
    let base = server_url();
    let (url, body) = crate::api_spec::device_revoke(&base, id);
    crate::account::post_json(&url, Some(&token), body).await
}

/// 供发布流程（下一批命令）复用的 token 读取
#[allow(dead_code)]
pub fn session_token() -> Option<String> {
    load_token()
}

/// 供发布流程复用的「服务端地址」（内置常量，不会失败）
pub fn base_url() -> String {
    server_url()
}

/// 应用启动时清理：如果 token 对应的会话已失效（401），静默清掉，避免界面一直显示"已登录"
pub async fn verify_session_on_startup(app: &tauri::AppHandle) {
    let _ = app;
    let Some(token) = load_token() else {
        return;
    };
    if let Err(e) = get_me(&token).await {
        if e.starts_with("UNAUTHORIZED") {
            clear_token();
            log::info!("启动检查：账号会话已失效，已清理本地 token");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：服务端地址是**内置常量**，配置里残留的旧值（开发期临时联调地址）不得影响它。
    ///
    /// 这条语义的代价面很大：地址入口已从设置里移除，一旦有人把读取逻辑改回
    /// `config::load().server_url`，老用户机器上那份残留地址就会重新生效，
    /// 表现为账号/额度/发布全部连不上、而界面上无从改回（见 AGENTS.md 约定 52）。
    #[test]
    fn server_url_is_builtin_and_ignores_config() {
        assert_eq!(server_url(), crate::config::DEFAULT_SERVER_URL);
        assert_eq!(base_url(), crate::config::DEFAULT_SERVER_URL);
        // 形态约束：带 scheme、不带结尾斜杠（拼接 `{base}/api/v1/...` 才能得到单斜杠）
        assert!(server_url().starts_with("http"));
        assert!(!server_url().ends_with('/'));
    }
}
