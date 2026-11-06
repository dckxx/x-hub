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

/// 自动同步节流的 meta 键 / 窗口：聚焦抖动、页面来回切都会触发，别每次都打服务端
const META_AUTO_SYNC_AT: &str = "auto_sync_at";
const AUTO_SYNC_MIN_INTERVAL_MS: i64 = 20_000;

/// 是否已开启云同步（有空间绑定 + 钥匙 + 设备 token）。自动路径的先决条件。
fn sync_ready(state: &State<'_, DbState>) -> bool {
    let Ok(conn) = lock(state) else { return false };
    let s = engine::status(&conn);
    s.space_id.is_some() && s.device_id.is_some() && s.key_ready
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
///
/// completed 时顺手触发一轮同步：手机此刻已经能解密了，桌面必须先把数据推上去，
/// 否则用户在手机上点「立即同步」只会拉到空空间（本期实测的「同步不过来」）。
#[tauri::command]
pub async fn sync_poll_pairing(
    app: AppHandle,
    state: State<'_, DbState>,
    pairing_id: String,
) -> Result<Value, String> {
    let r = pairing::poll(&pairing_id).await?;
    if r.status == "completed" {
        // 绕过 20s 节流：这一轮是「手机刚加入」的关键动作，等不得
        let has_pending = {
            let conn = lock(&state)?;
            crate::sync::repo::outbox_list(&conn, 1)
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        };
        if has_pending {
            match sync_now(app.clone(), state).await {
                Ok(v) => log::info!("[云同步] 配对完成，首轮同步: {v}"),
                Err(e) => log::warn!("[云同步] 配对完成后的首轮同步失败: {e}"),
            }
        }
    }
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
    // 当前版本的行解不开 = 钥匙不对（口令错 / 被篡改）。此时游标必须停在这一页之前，
    // 否则这批行一批过去就再也不重试，界面上却只显示「已是最新」。
    // 当前钥匙版本：判断「解不开」是口令不对（要停下来重试）还是历史密文（跳过即可）
    let key_version = {
        let conn = lock(&state)?;
        engine::status(&conn).key_version
    };
    for _page in 0..50 {
        let path = format!("/sync/changes?since={pull_seq}&limit=200");
        let resp = api::device_get(&path, &device_token, &device_id)
            .await
            .map_err(|e| auth_error(&state, e))?;
        let changes = resp["changes"].as_array().cloned().unwrap_or_default();
        if changes.is_empty() {
            break;
        }
        let failed_current;
        {
            let conn = lock(&state)?;
            let (stats, def) = engine::apply_remote_changes(&conn, &record_key, &changes, key_version).map_err(err_str)?;
            applied_total += stats.applied + stats.tombstoned;
            failed_total += stats.failed;
            failed_current = stats.failed_current;
            for uid in def {
                if let Some(c) = changes.iter().find(|c| c["uid"].as_str() == Some(uid.as_str())) {
                    deferred_changes.push(c.clone());
                }
            }
        }
        // 游标用**本页最后一条的 seq**：head 是查完本页之后取的，并发写入时可能更大，
        // 跳过去等于永久丢行；next_since 在 has_more=false 时回的是请求里的 since，游标不前进。
        let page_last = changes
            .iter()
            .filter_map(|c| c["seq"].as_i64())
            .max()
            .unwrap_or(pull_seq);
        if failed_current > 0 {
            // 有当前版本的行解不开：停在本页之前，下轮重试同一页
            log::warn!(
                "[云同步] 本页有 {failed_current} 条当前版本记录解不开（since={pull_seq}），游标停在此页前重试"
            );
            break;
        }
        if page_last <= pull_seq {
            log::warn!("[云同步] 拉取游标未前进（since={pull_seq}, last={page_last}），本轮停止分页");
            break;
        }
        pull_seq = page_last;
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
        if let Ok((stats, _)) = engine::apply_remote_changes(&conn, &record_key, &deferred_changes, key_version) {
            applied_total += stats.applied + stats.tombstoned;
            failed_total += stats.failed;
        }
    }
    { let conn = lock(&state)?; repo::meta_set(&conn, engine::META_PULL_SEQ, &pull_seq.to_string()).map_err(err_str)?; }

    // ---- push ----
    // 分批：条数与字节双预算（见 engine::PUSH_BATCH_*）。批间必须留出服务端的
    // 每空间 2s 频率窗口，否则第二批立刻 429、整轮同步失败——首次全量推送
    // 必然 >100 条，这条路径天天走。
    let mut pushed = 0usize;
    let mut push_failed = 0usize;
    let mut rejected_total = 0usize;
    for _batch_round in 0..50 {
        let batch = {
            let conn = lock(&state)?;
            engine::build_push_items(&conn, &record_key, engine::PUSH_BATCH_ITEMS, engine::PUSH_BATCH_BYTES)?
        };
        // 无需上云的条目（从未拿到 uid 就删掉的行）直接清，别占着 outbox 让 pending 常亮
        if !batch.skipped.is_empty() {
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &batch.skipped).map_err(err_str)?;
        }
        // 本地预检出的超限条目：清 outbox + 计入 rejected（SyncPanel 的
        // 「N 条被服务端拒收」提示随之带到界面），明细日志在 engine 里已打
        if !batch.oversize.is_empty() {
            rejected_total += batch.oversize.len();
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &batch.oversize).map_err(err_str)?;
        }
        if batch.items.is_empty() {
            break;
        }
        // 429 是节奏问题不是数据问题：退避后重试同一批（不重建批次，避免重新加密）
        let mut resp = None;
        let mut too_large = false;
        for attempt in 0..3 {
            match api::device_post("/sync/push", &device_token, &device_id, json!({ "items": batch.items })).await {
                Ok(v) => {
                    resp = Some(v);
                    break;
                }
                Err(e) if e.contains("RATE_LIMITED") => {
                    tokio::time::sleep(std::time::Duration::from_millis(1_500 * (attempt + 1) as u64)).await;
                }
                Err(e) if e.contains("TOO_LARGE") => {
                    too_large = true;
                    break;
                }
                Err(e) => return Err(auth_error(&state, e)),
            }
        }
        if too_large {
            // 整批因「含超限条目」被服务端退回（部署的服务端可能比本地预检默认值旧/小）：
            // 把预检阈值收紧到「本批最大单条密文以下」——重建批次时超限条目直接走本地
            // 预检（清 outbox + 明细日志），其余条目照常成批推，不再反复撞 413。
            // 阈值是运行时自适应的：每轮 413 都会继续收紧，几轮内收敛。
            let max_ct = batch
                .items
                .iter()
                .map(|it| it["payload_ct"].as_str().map(|s| s.len() * 3 / 4).unwrap_or(0))
                .max()
                .unwrap_or(0);
            engine::shrink_record_limit(max_ct.saturating_sub(1));
            log::warn!(
                "[云同步] 批内含超限条目（批 {} 条），单条预检阈值收紧到 {} B",
                batch.consumed.len(),
                engine::record_limit()
            );
            continue;
        }
        let Some(resp) = resp else {
            // 一直是 429：本轮放弃（outbox 原样保留，下轮接着推）
            push_failed += batch.items.len();
            break;
        };
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
                // rejected = 服务端判定这条数据本身不可接受（非法 uid / 单条超上限等）。
                // 重试永远不会成功，**必须丢弃**：留着它当队列头会让后面所有条目
                // 永久推不上去（2026-11 实测：一条 476KB 笔记堵死整台设备）。
                // 只有 stale 才值得重试——那是本端旧了，重拉后自然收敛。
                Some("rejected") => {
                    rejected_total += 1;
                    consumed.push(entry.clone());
                    log::warn!(
                        "[云同步] 服务端拒收，已跳过: entity={} uid={} reason={}",
                        entry.entity,
                        entry.uid.as_deref().unwrap_or(""),
                        r["error"].as_str().unwrap_or("unknown")
                    );
                }
                _ => { retry_later = true; push_failed += 1; }
            }
        }
        {
            let conn = lock(&state)?;
            engine::clear_consumed(&conn, &consumed).map_err(err_str)?;
        }
        if retry_later || results.len() < batch.consumed.len() { break; }
        // 还有下一批：等过服务端 2s 的推送频率窗口（首次全量推送可能连续几十批）
        tokio::time::sleep(std::time::Duration::from_millis(engine::PUSH_MIN_INTERVAL_MS)).await;
    }

    // ---- 收敛：push 中出现 stale/equal 后窄拉一轮 ----
    // （MVP：整页轻拉一次；出现高频冲突再升级为按 uid 过滤）

    if applied_total > 0 {
        let _ = app.emit("sync-applied", json!({ "applied": applied_total }));
    }
    Ok(json!({
        "applied": applied_total,
        "pushed": pushed,
        "failed": failed_total + push_failed,
        // 被服务端按条拒收（数据本身不可接受，已跳过）：界面要明确告知，
        // 否则用户只会看到「同步完成」而某些条目悄悄没上云
        "rejected": rejected_total,
    }))
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

/// 自动同步（窗口聚焦 / 定时器 / 配对完成 / 本地改动后调用）。
///
/// 手动「立即同步」是同步**唯一**的触发入口曾是本期最严重的可用性缺陷：配对完成后
/// 桌面一次都没推过，手机上点「立即同步」自然只能拿到空空间（用户看起来就是
/// 「配对成功但同步不过来」）。这个命令把协议 §6.4 的档 0 触发点补齐：
/// 静默、节流、失败不打扰用户（outbox 与游标都持久化，下次触发接着来）。
///
/// 返回本轮结果；未开启同步 / 节流窗口内返回 `{skipped: true}`。
#[tauri::command]
pub async fn sync_auto(app: AppHandle, state: State<'_, DbState>) -> Result<Value, String> {
    if !sync_ready(&state) {
        return Ok(json!({ "skipped": true, "reason": "not_ready" }));
    }
    // 自动同步开关（设置 → 数据与关于 → 云同步）：关 = 所有触发点（outbox 轮询 /
    // 定时对账 / 聚焦返回）全部空转，只有用户点「立即同步」（sync_now）才同步。
    // 放在节流之前：开关关闭时不该消耗节流时间戳，开了开关能立刻跑第一轮。
    if !crate::config::load().sync_auto_enabled {
        return Ok(json!({ "skipped": true, "reason": "disabled" }));
    }
    // 节流：同机 20s 内只跑一轮（聚焦/切页可能连续触发好几次）
    {
        let conn = lock(&state)?;
        let last = repo::meta_get(&conn, META_AUTO_SYNC_AT)
            .map_err(err_str)?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if engine::now_ms() - last < AUTO_SYNC_MIN_INTERVAL_MS {
            return Ok(json!({ "skipped": true, "reason": "throttled" }));
        }
        repo::meta_set(&conn, META_AUTO_SYNC_AT, &engine::now_ms().to_string()).map_err(err_str)?;
    }
    match sync_now(app, state).await {
        Ok(v) => Ok(v),
        Err(e) => {
            log::warn!("[云同步] 自动同步失败: {e}");
            Err(e)
        }
    }
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
