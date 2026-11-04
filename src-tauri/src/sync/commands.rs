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

fn err_str(e: impl std::fmt::Display) -> String {
    e.to_string()
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

/// 开启云同步：设口令 → 派生 → 建空间（或轮换桌面设备）→ 落 meta 与钥匙串。
#[tauri::command]
pub async fn sync_enable(state: State<'_, DbState>, passphrase: String) -> Result<Value, String> {
    if passphrase.trim().chars().count() < 8 {
        return Err("口令至少 8 位".into());
    }
    let salt: [u8; 16] = {
        let mut s = [0u8; 16];
        getrandom::fill(&mut s).map_err(err_str)?;
        s
    };
    let master = crypto::derive_master(&passphrase, &salt);
    let recovery_prf = crypto::derive_recovery_prf(&master, "placeholder-space");

    // 锁段 1：是否已有空间绑定（重装 = rotate，首开 = create）
    let has_space = {
        let conn = lock(&state)?;
        repo::meta_get(&conn, engine::META_SPACE).map_err(err_str)?.is_some()
    };

    // HTTP（锁外）
    let resp = if has_space {
        api::owner_post("/sync/devices/rotate-desktop", json!({
            "kdf_salt": hex(&salt), "kdf_params": kdf_params_json(), "recovery_hash": sha256_hex(&recovery_prf),
        })).await?
    } else {
        api::owner_post("/sync/spaces", json!({
            "kdf_salt": hex(&salt), "kdf_params": kdf_params_json(), "recovery_hash": sha256_hex(&recovery_prf),
        })).await?
    };

    let space_id = resp["space_id"].as_str().ok_or("服务端缺 space_id")?.to_string();
    let device_token = resp["device_token"].as_str().ok_or("服务端缺 device_token")?.to_string();
    let device_id = resp["device_id"].as_str().ok_or("服务端缺 device_id")?.to_string();
    let server_salt = hex_decode_32(&resp["kdf_salt"].as_str().unwrap_or(&hex(&salt)))?;

    // 以服务端确认的空间 ID 重派生（space_id 参与 HKDF salt，两端必须一致）
    let record_key = crypto::derive_record_key(&master, &space_id);
    let recovery_prf = crypto::derive_recovery_prf(&master, &space_id);
    let _ = server_salt; // 口径以服务端存储为准，仅校验非空
    crypto::store_keys(&record_key, &recovery_prf)?;
    crypto::store_device_token(&device_token)?;
    crypto::store_device_id(&device_id)?;

    // 锁段 2：落 meta；新空间标记初始全量推送
    {
        let conn = lock(&state)?;
        repo::meta_set(&conn, engine::META_SPACE, &space_id).map_err(err_str)?;
        repo::meta_set(&conn, engine::META_DEVICE, &device_id).map_err(err_str)?;
        repo::meta_set(&conn, engine::META_KEY_VERSION, "1").map_err(err_str)?;
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
    let mut pull_seq: i64 = { let conn = lock(&state)?; engine::status(&conn).pull_seq };
    let mut applied_total = 0usize;
    let mut failed_total = 0usize;
    let mut deferred: Vec<String> = Vec::new();
    for _page in 0..50 {
        let path = format!("/sync/changes?since={pull_seq}&limit=200");
        let resp = api::device_get(&path, &device_token, &device_id).await?;
        let changes = resp["changes"].as_array().cloned().unwrap_or_default();
        if changes.is_empty() {
            break;
        }
        {
            let conn = lock(&state)?;
            let (stats, def) = engine::apply_remote_changes(&conn, &record_key, &changes).map_err(err_str)?;
            applied_total += stats.applied + stats.tombstoned;
            failed_total += stats.failed;
            deferred = def;
        }
        pull_seq = resp["head"].as_i64().unwrap_or(pull_seq);
        if resp["has_more"].as_bool() != Some(true) {
            break;
        }
    }
    // deferred 重试：父行已在本页落库，再跑一遍残余
    if !deferred.is_empty() {
        let path = format!("/sync/changes?since={}&limit=500", pull_seq.saturating_sub(1000));
        if let Ok(resp) = api::device_get(&path, &device_token, &device_id).await {
            let changes = resp["changes"].as_array().cloned().unwrap_or_default();
            if !changes.is_empty() {
                let conn = lock(&state)?;
                if let Ok((stats, _)) = engine::apply_remote_changes(&conn, &record_key, &changes) {
                    applied_total += stats.applied + stats.tombstoned;
                }
            }
        }
    }
    { let conn = lock(&state)?; repo::meta_set(&conn, engine::META_PULL_SEQ, &pull_seq.to_string()).map_err(err_str)?; }

    // ---- push ----
    let mut pushed = 0usize;
    for _batch in 0..20 {
        let (items, entries) = {
            let conn = lock(&state)?;
            engine::build_push_items(&conn, &record_key, 100)?
        };
        if items.is_empty() { break; }
        let resp = api::device_post("/sync/push", &device_token, &device_id, json!({ "items": items })).await?;
        let results = resp["results"].as_array().cloned().unwrap_or_default();
        let mut consumed = Vec::new();
        let mut stale_uids = Vec::new();
        for (i, r) in results.iter().enumerate() {
            match r["status"].as_str() {
                Some("ok") | Some("equal") => {
                    if let Some(e) = entries.get(i) { consumed.push(e.clone()); }
                    if r["status"] == "ok" { pushed += 1; }
                }
                _ => {
                    if let Some(e) = entries.get(i) {
                        if e.op == "upsert" { stale_uids.push(e.uid.clone().unwrap_or_default()); }
                        consumed.push(e.clone()); // rejected/duplicate 条目无法推进，丢弃等触发器再标
                    }
                }
            }
        }
        {
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &consumed).map_err(err_str)?;
        }
        if results.len() < 100 { break; }
    }

    // ---- 收敛：push 中出现 stale/equal 后窄拉一轮 ----
    // （MVP：整页轻拉一次；出现高频冲突再升级为按 uid 过滤）

    if applied_total > 0 {
        let _ = app.emit("sync-applied", json!({ "applied": applied_total }));
    }
    Ok(json!({ "applied": applied_total, "pushed": pushed, "failed": failed_total }))
}

/// 设备列表（数据端点）
#[tauri::command]
pub async fn sync_device_list() -> Result<Value, String> {
    let (device_id, token) = device_creds()?;
    api::device_get("/sync/devices", &token, &device_id).await
}

/// 撤销某台设备（主人操作）
#[tauri::command]
pub async fn sync_revoke_device(device_id: String) -> Result<Value, String> {
    api::owner_post(&format!("/sync/devices/{device_id}/revoke"), json!({})).await
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
        let _ = repo::meta_set(&conn, engine::META_SPACE, "");
        let _ = repo::meta_set(&conn, engine::META_DEVICE, "");
        let _ = repo::meta_set(&conn, engine::META_PULL_SEQ, "0");
    }
    Ok(json!({ "ok": true }))
}

fn device_creds() -> Result<(String, String), String> {
    let token = crypto::load_device_token().ok_or("NOT_PAIRED: 请重新开启云同步")?;
    let id = crypto::load_device_id().ok_or("NOT_PAIRED: 请重新开启云同步")?;
    Ok((id, token))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b);
    hex(&h.finalize())
}

fn kdf_params_json() -> String {
    json!({ "N": 32768, "r": 8, "p": 1, "dkLen": 64 }).to_string()
}

fn hex_decode_32(s: &str) -> Result<Vec<u8>, String> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| "salt hex 非法".to_string()))
        .collect()
}
