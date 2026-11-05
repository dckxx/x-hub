//! 云同步 Tauri 命令（前台触发口径，docs/plan.md「REST 增量 + 前台触发」）。
//!
//! 锁纪律：DbState 互斥锁**绝不跨 await**——每个锁段只做同步 DB 操作，
//! HTTP 调用在锁外；sync_now 的拉-合-推由多个锁段拼接完成。

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::commands::DbState;
use crate::sync::{api, crypto, engine, pairing, repo};

fn lock<'a>(state: &'a State<'_, DbState>) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>, String> {
    state.0.lock().map_err(|e| format!("数据库锁不可用: {e}"))
}

/// 主人操作（账号 token）401：账号登录态失效，清设备绑定没用，提示重新登录
fn owner_auth_error(e: String) -> String {
    if e.starts_with("AUTH_REVOKED") {
        return "NOT_LOGGED_IN: 账号登录已失效，请先在「设置 → 账号」重新登录再开启云同步".to_string();
    }
    e
}

fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 设备/会话失效（api 层把 401/403 归一成 `AUTH_REVOKED:`）→ 清本地设备绑定并回到
/// 「未配对」，让同步面板重新出现「设置口令」入口（否则本机永远 401、界面还显示已开启）。
/// 只清 device 绑定，保留 space_id 与 record_key：重新开启走 rotate 路径即可，不必重建空间。
fn auth_error(state: &State<'_, DbState>, e: String) -> String {
    if !e.starts_with("AUTH_REVOKED") {
        return e;
    }
    crypto::clear_device_binding();
    if let Ok(conn) = lock(state) {
        let _ = repo::meta_del(&conn, engine::META_DEVICE);
    }
    "NOT_PAIRED: 设备授权已失效（可能在其它设备上被移除），请重新开启云同步".to_string()
}

/// 同步总状态（SyncPanel 首屏）
#[tauri::command]
pub fn sync_status(state: State<'_, DbState>) -> Result<Value, String> {
    let conn = lock(&state)?;
    let s = engine::status(&conn);
    Ok(json!({
        "paired": s.space_id.is_some() && s.device_id.is_some(),
        "keyReady": s.key_ready,
        "spaceId": s.space_id,
        "deviceId": s.device_id,
        "pullSeq": s.pull_seq,
        "pending": s.pending,
        "keyVersion": s.key_version,
    }))
}

/// 开启云同步：设口令 → 派生 → 建空间 / 轮换桌面设备 → 绑定恢复校验值 → 落 meta 与钥匙串。
///
/// 两条硬约束：
/// 1. 派生必须用**服务端实际存储的 kdf_salt**：新建时即本端刚提交的盐，重装时是空间
///    创建时的旧盐（绝不能用新的随机盐，否则解不开既有密文）。
/// 2. `recovery_prf` 以 space_id 作 HKDF salt，而 space_id 建空间后才由服务端生成，
///    因此 recovery_hash 只能在建空间之后单独绑定（PUT /sync/spaces/:id/recovery-hash）。
#[tauri::command]
pub async fn sync_enable(state: State<'_, DbState>, passphrase: String) -> Result<Value, String> {
    if passphrase.trim().chars().count() < 8 {
        return Err("口令至少 8 位".into());
    }

    // 锁段 1：是否已有空间绑定（重装 = rotate，首开 = create）
    let (has_space, local_space_id) = {
        let conn = lock(&state)?;
        // 空串不算有空间（旧版 sync_disable 写过空串）
        let sid = repo::meta_get(&conn, engine::META_SPACE)
            .map_err(err_str)?
            .filter(|s| !s.trim().is_empty());
        (sid.is_some(), sid)
    };

    // 新建空间时盐由本端生成（服务端只存不复算）；此时还没拿到 space_id，
    // recovery_hash 先用随机占位值顶住（不参与任何派生），拿到 space_id 后立刻绑定真值。
    let local_salt: Option<[u8; 16]> = if has_space {
        None
    } else {
        Some(random_bytes(16)?.try_into().expect("16 字节盐"))
    };

    // HTTP（锁外）
    let resp = if let Some(salt) = &local_salt {
        api::owner_post("/sync/spaces", json!({
            "kdf_salt": crypto::hex_encode(salt),
            "kdf_params": kdf_params_json(),
            "recovery_hash": crypto::hex_encode(&random_bytes(32)?),
        })).await.map_err(owner_auth_error)?
    } else {
        api::owner_post("/sync/devices/rotate-desktop", json!({})).await.map_err(owner_auth_error)?
    };

    let space_id = resp["space_id"]
        .as_str()
        .map(str::to_string)
        .or(local_space_id)
        .ok_or("服务端缺 space_id")?;
    let device_token = resp["device_token"].as_str().ok_or("服务端缺 device_token")?.to_string();
    let device_id = resp["device_id"].as_str().ok_or("服务端缺 device_id")?.to_string();
    let key_version = resp["key_version"].as_i64().unwrap_or(1);

    // 派生口径 = 服务端存储的盐；新建时再校验它确实是我们提交的那个值。
    let server_salt = resp["kdf_salt"].as_str().unwrap_or_default().to_ascii_lowercase();
    let salt_bytes = match hex_to_bytes(&server_salt) {
        Ok(b) if b.len() == 16 => b,
        _ => {
            if local_salt.is_some() {
                let _ = api::owner_delete(&format!("/sync/spaces/{space_id}")).await;
            }
            return Err("服务端 kdf_salt 不合法（应为 32 位 hex）".into());
        }
    };
    if let Some(local) = &local_salt {
        if crypto::hex_encode(local) != server_salt {
            // 服务端没有按提交值存储：回滚刚建的空空间，避免留下无法恢复的空间
            let _ = api::owner_delete(&format!("/sync/spaces/{space_id}")).await;
            return Err("服务端返回的 kdf_salt 与提交值不一致，已回滚新建空间".into());
        }
    }

    let master = crypto::derive_master(&passphrase, &salt_bytes);
    let record_key = crypto::derive_record_key(&master, &space_id);
    let recovery_prf = crypto::derive_recovery_prf(&master, &space_id);

    // 绑定真实 recovery_hash：新建首绑；重装时顺带自愈旧版本写错的占位值。
    // 只改 hash 不动 salt / key_version，密文与 record_key 都不受影响。
    if let Err(e) = api::owner_put(
        &format!("/sync/spaces/{space_id}/recovery-hash"),
        json!({ "recovery_hash": sha256_hex(&recovery_prf) }),
    )
    .await
    {
        if local_salt.is_some() {
            let _ = api::owner_delete(&format!("/sync/spaces/{space_id}")).await;
            return Err(format!("初始化恢复凭据失败，已回滚新建空间: {e}"));
        }
        return Err(format!("更新恢复凭据失败: {e}"));
    }

    // 重装：先证明这枚口令能解开空间里已有的记录，再落钥匙（否则会静默用错口令同步出一堆 failed）。
    if local_salt.is_none() {
        verify_record_key(&device_token, &device_id, &record_key).await?;
    }

    if let Err(e) = crypto::store_keys(&record_key, &recovery_prf)
        .and_then(|_| crypto::store_device_token(&device_token))
        .and_then(|_| crypto::store_device_id(&device_id))
    {
        if local_salt.is_some() {
            let _ = api::owner_delete(&format!("/sync/spaces/{space_id}")).await;
        }
        return Err(format!("写入本机钥匙串失败{}{e}", if local_salt.is_some() { "，已回滚新建空间: " } else { ": " }));
    }

    // 锁段 2：落 meta；新空间标记初始全量推送
    {
        let conn = lock(&state)?;
        repo::meta_set(&conn, engine::META_SPACE, &space_id).map_err(err_str)?;
        repo::meta_set(&conn, engine::META_DEVICE, &device_id).map_err(err_str)?;
        repo::meta_set(&conn, engine::META_KEY_VERSION, &key_version.to_string()).map_err(err_str)?;
        engine::mark_all_dirty_if_first_push(&conn).map_err(err_str)?;
    }
    Ok(json!({ "spaceId": space_id, "deviceId": device_id }))
}

/// 出配对二维码（前置：已 enable）
#[tauri::command]
pub async fn sync_begin_pairing() -> Result<String, String> {
    pairing::begin().await
}

/// 轮询配对状态（配对弹窗打开期间 2s 一次）
#[tauri::command]
pub async fn sync_poll_pairing(pairing_id: String) -> Result<Value, String> {
    let r = pairing::poll(&pairing_id).await?;
    Ok(json!({ "status": r.status, "deviceName": r.device_name }))
}

#[tauri::command]
pub async fn sync_pairing_cancel(pairing_id: String) -> Result<(), String> {
    pairing::cancel(&pairing_id).await
}

/// 一次完整同步轮：pull（分页解密合并）→ push（首批 outbox）→ stale 收敛再拉。
/// 应用过远端数据则发 `sync-applied` 事件让前端重载业务界面。
#[tauri::command]
pub async fn sync_now(app: AppHandle, state: State<'_, DbState>) -> Result<Value, String> {
    // 锁段 1：读绑定与钥匙
    let (_space_id, device_id) = {
        let conn = lock(&state)?;
        let s = engine::status(&conn);
        match (s.space_id, s.device_id) {
            (Some(s), Some(d)) => (s, d),
            _ => return Err("NOT_PAIRED: 尚未开启云同步".into()),
        }
    };
    let record_key = crypto::load_record_key().ok_or("NO_KEY: 请先设置同步口令")?;
    let device_token = crypto::load_device_token().ok_or("NOT_PAIRED: 请重新开启云同步")?;

    // ---- pull ----
    // 只读游标：别为了一个数字再调 engine::status（那里面会再读一次系统钥匙串）
    let mut pull_seq: i64 = {
        let conn = lock(&state)?;
        repo::meta_get(&conn, engine::META_PULL_SEQ)
            .map_err(err_str)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    let mut applied_total = 0usize;
    let mut failed_total = 0usize;
    // 父行还没落库而暂时应用不了的记录：留在内存里，整轮拉完再补一次（不再多发一次 HTTP 猜窗口）
    let mut deferred_changes: Vec<serde_json::Value> = Vec::new();
    for _page in 0..50 {
        let path = format!("/sync/changes?since={pull_seq}&limit=200");
        let resp = api::device_get(&path, &device_token, &device_id)
            .await
            .map_err(|e| auth_error(&state, e))?;
        let changes = resp["changes"].as_array().cloned().unwrap_or_default();
        if changes.is_empty() {
            break;
        }
        {
            let conn = lock(&state)?;
            let (stats, def) = engine::apply_remote_changes(&conn, &record_key, &changes).map_err(err_str)?;
            applied_total += stats.applied + stats.tombstoned;
            failed_total += stats.failed;
            for uid in def {
                if let Some(c) = changes.iter().find(|c| c["uid"].as_str() == Some(uid.as_str())) {
                    deferred_changes.push(c.clone());
                }
            }
        }
        let next_seq = resp["head"].as_i64().unwrap_or(pull_seq);
        // 游标不前进说明服务端口径与预期不符：宁可停下（下次同步从同一位置重试），
        // 也不能拿同一个 since 死循环重放同一页
        if next_seq <= pull_seq {
            log::warn!("[云同步] 拉取游标未前进（since={pull_seq}, head={next_seq}），本轮停止分页");
            break;
        }
        pull_seq = next_seq;
        // 首次全量同步可能连续几十页，每页都是串行解密 + 落库并抢 DB 锁：
        // 主动让出一拍，别把界面（同样走 DbState 的命令）整段饿住
        tokio::task::yield_now().await;
        if resp["has_more"].as_bool() != Some(true) {
            break;
        }
    }
    // deferred 重试：父行已在前面的页里落库，用同一批记录补一次（不额外请求）
    if !deferred_changes.is_empty() {
        let conn = lock(&state)?;
        if let Ok((stats, _)) = engine::apply_remote_changes(&conn, &record_key, &deferred_changes) {
            applied_total += stats.applied + stats.tombstoned;
            failed_total += stats.failed;
        }
    }
    { let conn = lock(&state)?; repo::meta_set(&conn, engine::META_PULL_SEQ, &pull_seq.to_string()).map_err(err_str)?; }

    // ---- push ----
    let mut pushed = 0usize;
    let mut push_failed = 0usize;
    for _batch in 0..20 {
        let batch = {
            let conn = lock(&state)?;
            engine::build_push_items(&conn, &record_key, 100)?
        };
        // 无需上云的条目（从未拿到 uid 就删掉的行）直接清，别占着 outbox 让 pending 常亮
        if !batch.skipped.is_empty() {
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &batch.skipped).map_err(err_str)?;
        }
        if batch.items.is_empty() { break; }
        let resp = api::device_post("/sync/push", &device_token, &device_id, json!({ "items": batch.items }))
            .await
            .map_err(|e| auth_error(&state, e))?;
        let results = resp["results"].as_array().cloned().unwrap_or_default();
        let mut consumed = Vec::new();
        let mut retry_later = false;
        for (i, r) in results.iter().enumerate() {
            // items 与 consumed 逐条对齐（engine::build_push_items 保证），下标可直接回查
            let Some(entry) = batch.consumed.get(i) else { break };
            match r["status"].as_str() {
                Some("ok") => { consumed.push(entry.clone()); pushed += 1; }
                // equal = 服务端已是同一版本，收敛完成
                Some("equal") => consumed.push(entry.clone()),
                // stale / rejected / duplicate：**不能清 outbox**。清了就再也推不上去，
                // 本地改动等于静默丢失；保留下来等下一轮拉取把服务端版本合并进来再推
                _ => { retry_later = true; push_failed += 1; }
            }
        }
        {
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &consumed).map_err(err_str)?;
        }
        if retry_later || results.len() < batch.consumed.len() { break; }
    }

    // ---- 收敛：push 中出现 stale/equal 后窄拉一轮 ----
    // （MVP：整页轻拉一次；出现高频冲突再升级为按 uid 过滤）

    if applied_total > 0 {
        let _ = app.emit("sync-applied", json!({ "applied": applied_total }));
    }
    Ok(json!({ "applied": applied_total, "pushed": pushed, "failed": failed_total + push_failed }))
}

/// 设备列表（数据端点）
#[tauri::command]
pub async fn sync_device_list(state: State<'_, DbState>) -> Result<Value, String> {
    let (device_id, token) = device_creds()?;
    api::device_get("/sync/devices", &token, &device_id)
        .await
        .map_err(|e| auth_error(&state, e))
}

/// 撤销某台设备（主人操作）
#[tauri::command]
pub async fn sync_revoke_device(device_id: String) -> Result<Value, String> {
    api::owner_post(&format!("/sync/devices/{device_id}/revoke"), json!({}))
        .await
        .map_err(owner_auth_error)
}

/// 关闭本机同步：吊销自己的设备行 + 清本地钥匙（云端密文保留）
#[tauri::command]
pub async fn sync_disable(state: State<'_, DbState>) -> Result<Value, String> {
    if let Ok((device_id, token)) = device_creds() {
        let _ = api::device_delete("/sync/devices/self", &token, &device_id).await;
    }
    crypto::clear_local_keys();
    {
        let conn = lock(&state)?;
        // 删键而不是写空串：sync_status 用「是否存在 + 非空」判 paired，写空串会让
        // 关闭后仍显示已开启，且口令入口（只在 !paired 时渲染）再也出不来
        let _ = repo::meta_del(&conn, engine::META_SPACE);
        let _ = repo::meta_del(&conn, engine::META_DEVICE);
        let _ = repo::meta_del(&conn, engine::META_PULL_SEQ);
    }
    Ok(json!({ "ok": true }))
}

fn device_creds() -> Result<(String, String), String> {
    let token = crypto::load_device_token().ok_or("NOT_PAIRED: 请重新开启云同步")?;
    let id = crypto::load_device_id().ok_or("NOT_PAIRED: 请重新开启云同步")?;
    Ok((id, token))
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b);
    crypto::hex_encode(&h.finalize())
}

fn kdf_params_json() -> String {
    json!({ "N": 32768, "r": 8, "p": 1, "dkLen": 64 }).to_string()
}

/// 重装场景的端到端校验：拉空间里的记录逐条试解，**任意一条能解开**就说明
/// 「服务端盐 + 输入口令」确实对得上。空空间（没有任何记录）无从验证，直接放行。
///
/// ⚠️ 不能只看第一条：`since=0` 取到的是**最旧**的记录，密钥轮换/换口令之后旧密文
/// 本来就解不开，只试第一条会把正确口令误报成 PASSPHRASE_MISMATCH。
async fn verify_record_key(device_token: &str, device_id: &str, record_key: &[u8; 32]) -> Result<(), String> {
    let resp = api::device_get("/sync/changes?since=0&limit=20", device_token, device_id).await?;
    let Some(changes) = resp["changes"].as_array() else {
        return Ok(());
    };
    if changes.is_empty() {
        return Ok(());
    }
    for change in changes {
        let entity = change["entity"].as_str().unwrap_or_default();
        let uid = change["uid"].as_str().unwrap_or_default();
        let updated_at = change["updated_at"].as_i64().unwrap_or(0);
        let Ok(nonce) = decode_b64(change["nonce"].as_str().unwrap_or_default()) else { continue };
        let Ok(ct) = decode_b64(change["payload_ct"].as_str().unwrap_or_default()) else { continue };
        let Ok(nonce) = <[u8; crypto::NONCE_LEN]>::try_from(nonce.as_slice()) else { continue };
        if crypto::open_record(record_key, entity, uid, updated_at, &nonce, &ct).is_ok() {
            return Ok(());
        }
    }
    Err("PASSPHRASE_MISMATCH: 口令不正确，或该空间的密钥已轮换；请核对口令后重试".into())
}

fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| "记录编码不合法".to_string())
}

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("salt hex 长度不是偶数".into());
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| "salt hex 非法".to_string()))
        .collect()
}

fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut v = vec![0u8; n];
    getrandom::fill(&mut v).map_err(err_str)?;
    Ok(v)
}
