//! 客户端 → x-hub-server 的**请求规格**（路径与请求体字段名的唯一来源）。
//!
//! 为什么单独抽出来：这些路径与字段名必须与服务端路由 / `parseBody` 读的键**完全一致**，
//! 而它们属于「只有联调才会暴露」的一层 —— 两端各自的测试都是绿的，一对接就 404 或 400。
//! 集中在一处 + 单测断言，改动时不容易只改一边。
//!
//! 注意字段名是**蛇形**（`poll_id` / `min_app_version`）：服务端 Hono 读的是原始键名，
//! 不做驼峰转换。

use serde::Serialize;
use serde_json::{json, Value};

/// 请求规格：`(完整 URL, JSON 请求体)`
pub type Spec = (String, Value);

fn base_of(base: &str) -> &str {
    base.trim_end_matches('/')
}

// ---------------- 登录（GitHub 设备码流程） ----------------

pub fn github_device_start(base: &str) -> Spec {
    (format!("{}/api/v1/auth/github/device/start", base_of(base)), json!({}))
}

pub fn github_device_poll(base: &str, poll_id: &str) -> Spec {
    (
        format!("{}/api/v1/auth/github/device/poll", base_of(base)),
        json!({ "poll_id": poll_id }),
    )
}

// ---------------- 登录（邮箱验证码） ----------------

pub fn email_send(base: &str, email: &str) -> Spec {
    (
        format!("{}/api/v1/auth/email/send", base_of(base)),
        json!({ "email": email }),
    )
}

pub fn email_verify(base: &str, email: &str, code: &str) -> Spec {
    (
        format!("{}/api/v1/auth/email/verify", base_of(base)),
        json!({ "email": email, "code": code }),
    )
}

// ---------------- 账号与权益 ----------------

/// 兑换邀请码（账号与权益解耦：兑换后才发额度、才可申请开发者）
pub fn redeem_invite(base: &str, code: &str) -> Spec {
    (format!("{}/api/v1/me/redeem", base_of(base)), json!({ "code": code }))
}

/// 开发者申请（理由会展示给审核者）
pub fn dev_apply(base: &str, reason: &str) -> Spec {
    (
        format!("{}/api/v1/dev/apply", base_of(base)),
        json!({ "reason": reason }),
    )
}

/// 开发者申请状态（GET）
pub fn dev_apply_status_path() -> &'static str {
    "/api/v1/dev/apply"
}

/// 我的在线设备（GET）
pub fn device_tokens_path() -> &'static str {
    "/api/v1/me/device-tokens"
}

/// 撤销某台设备
pub fn device_revoke(base: &str, id: i64) -> Spec {
    (
        format!("{}/api/v1/me/device-tokens/{id}/revoke", base_of(base)),
        json!({}),
    )
}

// ---------------- 平台 AI 额度 ----------------

/// 平台可用模型（GET；登录后「使用平台免费额度」的入口）
pub fn platform_models_path() -> &'static str {
    "/api/v1/ai/models"
}

/// 供应商预设清单（GET；「添加供应商」快速填充用，后台可配）
pub fn provider_presets_path() -> &'static str {
    "/api/v1/ai/provider-presets"
}

// ---------------- 扩展发布 ----------------

pub fn submit_extension_path() -> &'static str {
    "/api/v1/dev/submissions"
}

pub fn my_submissions_path() -> &'static str {
    "/api/v1/dev/submissions"
}

pub fn submission_detail_path(id: i64) -> String {
    format!("/api/v1/dev/submissions/{id}")
}

pub fn withdraw_submission(base: &str, id: i64) -> Spec {
    (
        format!("{}/api/v1/dev/submissions/{id}/withdraw", base_of(base)),
        json!({}),
    )
}

// ---------------- 扩展反馈（平台托管 · 公开可见） ----------------

/// 提交反馈的请求体：字段名 = 服务端读的原始键名（蛇形）。
///
/// 单独建结构体而不是一串位置参数 —— `app_version` / `ext_version` 两个近似字段
/// 用位置参数极易传反，且错了只有联调才暴露。
#[derive(Serialize)]
pub struct FeedbackSubmit<'a> {
    pub ext_id: &'a str,
    pub mid: &'a str,
    pub kind: &'a str,
    pub body: &'a str,
    pub contact: &'a str,
    pub diagnostics: &'a str,
    pub app_version: &'a str,
    pub ext_version: &'a str,
}

pub fn extension_feedback_submit(base: &str, body: &FeedbackSubmit<'_>) -> Spec {
    (
        format!("{}/api/v1/market/feedback", base_of(base)),
        serde_json::to_value(body).unwrap_or_else(|_| json!({})),
    )
}

/// 公开反馈列表（GET，匿名可读；不含联系方式与诊断）
pub fn extension_feedback_list_path(ext_id: &str) -> String {
    format!("/api/v1/market/feedback?extId={}", encode_component(ext_id))
}

/// 我的已发布扩展与未读数（GET，需登录）
pub fn feedback_unread_path() -> &'static str {
    "/api/v1/dev/feedback/unread"
}

/// 开发者收件箱完整列表（GET，需登录；含联系方式与诊断）
pub fn feedback_inbox_list_path(ext_id: &str) -> String {
    format!("/api/v1/dev/feedback?extId={}", encode_component(ext_id))
}

pub fn feedback_mark_read(base: &str, ext_id: &str) -> Spec {
    (
        format!("{}/api/v1/dev/feedback/read", base_of(base)),
        json!({ "ext_id": ext_id }),
    )
}

pub fn feedback_reply(base: &str, id: i64, reply: &str, status: &str) -> Spec {
    (
        format!("{}/api/v1/dev/feedback/{id}/reply", base_of(base)),
        json!({ "reply": reply, "status": status }),
    )
}

pub fn feedback_hide(base: &str, id: i64, hidden: bool) -> Spec {
    (
        format!("{}/api/v1/dev/feedback/{id}/hide", base_of(base)),
        json!({ "hidden": hidden }),
    )
}

/// 查询串百分号编码：只保留 RFC 3986 unreserved 字符。
/// 扩展 id 正常只含 `[A-Za-z0-9._-]`，这里只是不给畸形 id 留注入查询串的口子。
fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_the_server_routes() {
        let b = "https://x.example";
        assert_eq!(github_device_start(b).0, "https://x.example/api/v1/auth/github/device/start");
        assert_eq!(github_device_poll(b, "p").0, "https://x.example/api/v1/auth/github/device/poll");
        assert_eq!(email_send(b, "a@b.c").0, "https://x.example/api/v1/auth/email/send");
        assert_eq!(email_verify(b, "a@b.c", "123456").0, "https://x.example/api/v1/auth/email/verify");
        assert_eq!(redeem_invite(b, "CODE").0, "https://x.example/api/v1/me/redeem");
        assert_eq!(dev_apply(b, "reason").0, "https://x.example/api/v1/dev/apply");
        assert_eq!(dev_apply_status_path(), "/api/v1/dev/apply");
        assert_eq!(device_tokens_path(), "/api/v1/me/device-tokens");
        assert_eq!(device_revoke(b, 7).0, "https://x.example/api/v1/me/device-tokens/7/revoke");
        assert_eq!(platform_models_path(), "/api/v1/ai/models");
        assert_eq!(provider_presets_path(), "/api/v1/ai/provider-presets");
        assert_eq!(submit_extension_path(), "/api/v1/dev/submissions");
        assert_eq!(submission_detail_path(9), "/api/v1/dev/submissions/9");
        assert_eq!(withdraw_submission(b, 9).0, "https://x.example/api/v1/dev/submissions/9/withdraw");
        // 扩展反馈：公开列表匿名可读、收件箱需登录，两套路径别串
        assert_eq!(
            extension_feedback_submit(
                b,
                &FeedbackSubmit {
                    ext_id: "com.a_b.c",
                    mid: "m",
                    kind: "bug",
                    body: "b",
                    contact: "",
                    diagnostics: "d",
                    app_version: "0.9.0",
                    ext_version: "1.0.0",
                }
            )
            .0,
            "https://x.example/api/v1/market/feedback"
        );
        assert_eq!(
            extension_feedback_list_path("com.a_b.c"),
            "/api/v1/market/feedback?extId=com.a_b.c"
        );
        assert_eq!(feedback_unread_path(), "/api/v1/dev/feedback/unread");
        assert_eq!(feedback_inbox_list_path("com.a_b.c"), "/api/v1/dev/feedback?extId=com.a_b.c");
        assert_eq!(feedback_mark_read(b, "com.a").0, "https://x.example/api/v1/dev/feedback/read");
        assert_eq!(feedback_reply(b, 5, "hi", "resolved").0, "https://x.example/api/v1/dev/feedback/5/reply");
        assert_eq!(feedback_hide(b, 5, true).0, "https://x.example/api/v1/dev/feedback/5/hide");
        // 畸形 id 不能原样拼进查询串
        assert_eq!(
            extension_feedback_list_path("a b&c=1"),
            "/api/v1/market/feedback?extId=a%20b%26c%3D1"
        );
    }

    #[test]
    fn bodies_use_the_snake_case_keys_the_server_reads() {
        // 这些键名一旦拼错，服务端读不到参数 → 400/401，且只有联调才会暴露
        assert_eq!(github_device_poll("b", "dev-code").1, json!({ "poll_id": "dev-code" }));
        assert_eq!(email_send("b", "a@b.c").1, json!({ "email": "a@b.c" }));
        assert_eq!(email_verify("b", "a@b.c", "000000").1, json!({ "email": "a@b.c", "code": "000000" }));
        assert_eq!(redeem_invite("b", "X").1, json!({ "code": "X" }));
        assert_eq!(dev_apply("b", "想做一个扩展").1, json!({ "reason": "想做一个扩展" }));
        // 无参数端点必须是空对象（服务端会 await c.req.json().catch(() => null)，空对象最稳）
        assert_eq!(github_device_start("b").1, json!({}));
        assert_eq!(device_revoke("b", 1).1, json!({}));
        assert_eq!(withdraw_submission("b", 1).1, json!({}));
        // 反馈接口：服务端读的是原始蛇形键，驼峰会静默丢字段（400 / 匿名提交）
        assert_eq!(
            extension_feedback_submit(
                "b",
                &FeedbackSubmit {
                    ext_id: "com.a",
                    mid: "m",
                    kind: "bug",
                    body: "b",
                    contact: "",
                    diagnostics: "d",
                    app_version: "0.9.0",
                    ext_version: "1.0.0",
                }
            )
            .1,
            json!({
                "ext_id": "com.a",
                "mid": "m",
                "kind": "bug",
                "body": "b",
                "contact": "",
                "diagnostics": "d",
                "app_version": "0.9.0",
                "ext_version": "1.0.0",
            })
        );
        assert_eq!(feedback_mark_read("b", "com.a").1, json!({ "ext_id": "com.a" }));
        assert_eq!(
            feedback_reply("b", 5, "hi", "resolved").1,
            json!({ "reply": "hi", "status": "resolved" })
        );
        assert_eq!(feedback_hide("b", 5, true).1, json!({ "hidden": true }));
    }

    #[test]
    fn base_trailing_slash_is_normalized() {
        // 用户在设置里填了带斜杠的地址也要能用
        assert_eq!(
            github_device_start("https://x.example/").0,
            "https://x.example/api/v1/auth/github/device/start"
        );
    }
}
