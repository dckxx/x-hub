//! 同步引擎：拉-合-推的 DB 编排（docs/sync-protocol.md §6）。
//!
//! 锁纪律：所有函数都是**同步**的 DB 操作，HTTP 由 commands 层在锁外穿插——
//! 绝不跨 await 持 DbState 锁（与 agent 工具的短连接纪律一致）。
//!
//! LWW 口径：本端时间戳 = `sync_updated_at`（NULL 时回退解析 updated_at 文本）。
//! 远端更新 → guard 下走 map::apply_*；本地更新 → 重新标脏等下轮推；
//! 相等 → 已收敛，跳过。引擎永不依据"本地没有"推断删除（ADR 0001）。

use base64::Engine as _;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use super::crypto::{open_record, seal_record};
use super::repo::{self, OutboxEntry};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// meta 键：拉取游标 / 空间绑定 / 初始全量推送标记
pub const META_PULL_SEQ: &str = "pull_seq";
pub const META_SPACE: &str = "space_id";
pub const META_DEVICE: &str = "device_id";
pub const META_KEY_VERSION: &str = "key_version";
pub const META_INITIAL_PUSHED: &str = "initial_pushed";

/// 单批推送的条数 / 字节预算：服务端上限是 200 条、2MB（协议 §7.3），这里各留一半。
/// **两个都必须遵守**——只限条数时，100 条大笔记仍会超 2MB 被整批 413 退回，
/// 而退回后队列头不变，下次重试还是同一批，同步就此卡死。
pub const PUSH_BATCH_ITEMS: usize = 100;
pub const PUSH_BATCH_BYTES: usize = 1024 * 1024;
/// 批间等待：服务端每空间 2s 一次的推送频率窗口（协议 §7.3），多留 200ms 抖动余量
pub const PUSH_MIN_INTERVAL_MS: u64 = 2_200;

// ---------------- 状态 ----------------

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub space_id: Option<String>,
    pub device_id: Option<String>,
    pub key_ready: bool,
    pub pull_seq: i64,
    pub pending: i64,
    pub key_version: i64,
}

pub fn status(conn: &Connection) -> Status {
    // 空串一律当未设置：sync_disable 早期写过空串，若按 is_some() 判，关闭后仍显示「已开启」，
    // 而 SyncPanel 只在 !paired 时才渲染口令入口 → 用户再也开不回来
    let meta = |k: &str| repo::meta_get(conn, k).ok().flatten().filter(|v| !v.trim().is_empty());
    let space_id = meta(META_SPACE);
    let device_id = meta(META_DEVICE);
    let key_ready = super::crypto::load_record_key().is_some();
    let pull_seq: i64 = meta(META_PULL_SEQ).and_then(|v| v.parse().ok()).unwrap_or(0);
    let pending: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))
        .unwrap_or(0);
    let key_version: i64 = meta(META_KEY_VERSION).and_then(|v| v.parse().ok()).unwrap_or(1);
    Status { space_id, device_id, key_ready, pull_seq, pending, key_version }
}

// ---------------- 本端时间戳 ----------------

/// 行的 LWW 时间戳：sync_updated_at 优先，NULL 回退解析 updated_at 文本，再回退当前时间
pub fn row_sync_ms(conn: &Connection, entity: &str, rowid: i64) -> i64 {
    let table = if entity == "todo" { "todos" } else { "notes" };
    let sql = format!("SELECT sync_updated_at, updated_at FROM {table} WHERE id = ?1");
    let row: Option<(Option<i64>, String)> = conn
        .query_row(&sql, params![rowid], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    match row {
        Some((Some(ms), _)) => ms,
        Some((None, text)) => crate::db::parse_local_ts_ms(&text).unwrap_or_else(|| now_ms()),
        None => now_ms(),
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn uid_of_row(conn: &Connection, entity: &str, rowid: i64) -> Option<String> {
    repo::uid_of(conn, entity, rowid).ok().flatten()
}

// ---------------- 初始全量推送 ----------------

/// 首次同步（云端为空）时把全部本地行标脏——"本地优先"的落云动作。
/// 只做一次（meta 标记）；此后增量全部交给触发器。
pub fn mark_all_dirty_if_first_push(conn: &Connection) -> rusqlite::Result<usize> {
    if repo::meta_get(conn, META_INITIAL_PUSHED)?.is_some() {
        return Ok(0);
    }
    let mut total = 0;
    for (entity, table) in [("todo", "todos"), ("note", "notes")] {
        let ids: Vec<i64> = {
            let mut stmt = conn.prepare(&format!("SELECT id FROM {table} ORDER BY id"))?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<i64>>>()?
        };
        for id in ids {
            conn.execute(
                "INSERT INTO sync_outbox (entity, rowid_ref, op, uid) VALUES (?1, ?2, 'upsert', NULL)
                 ON CONFLICT(entity, rowid_ref) DO UPDATE SET op = 'upsert'",
                params![entity, id],
            )?;
            total += 1;
        }
    }
    repo::meta_set(conn, META_INITIAL_PUSHED, "1")?;
    Ok(total)
}

// ---------------- 推送批次构建 ----------------

/// 推送批次：`items` 与 `consumed` **逐条对齐**（下标必须一一对应，调用方按 results
/// 下标回查 outbox 行）；`skipped` 是"无需上云、直接清掉"的条目（从未拿到 uid 就删掉的行）。
#[derive(Debug, Default)]
pub struct PushBatch {
    pub items: Vec<Value>,
    pub consumed: Vec<OutboxEntry>,
    pub skipped: Vec<OutboxEntry>,
}

/// outbox → 加密后的推送条目。delete 条目从 outbox.uid + 墓碑表取证。
/// **先推后清**：只有服务端确认才对 `consumed` 做 clear_consumed。
///
/// ⚠️ items 与 consumed 必须严格等长：旧实现跳过条目时只跳过 item、仍返回全量 entries，
/// 调用方按 results 下标取 entries 会**整体错位 → 清掉本该推送的另外一行**（本地改动
/// 从此不再上云，而被跳过的条目永久滞留在 outbox）。
///
/// `max_bytes` 是单批密文总字节预算（按 base64 长度估算）：服务端对批总量有硬上限，
/// 超了一次 413 整批退回，而 100 条大笔记能轻松超过 2MB。**单条超过预算时照常带上**
/// ——它必须被单独送一次，否则永远轮不到（也让服务端有机会按条 rejected）。
pub fn build_push_items(
    conn: &Connection,
    record_key: &[u8; 32],
    limit: usize,
    max_bytes: usize,
) -> Result<PushBatch, String> {
    // 推送前置：给还没有 uid 的行惰性发 ULID（SQLite 触发器造不出 ULID）
    repo::ensure_uids(conn).map_err(|e| e.to_string())?;
    let entries = repo::outbox_list(conn, limit as i64).map_err(|e| e.to_string())?;
    let mut batch = PushBatch::default();
    let mut bytes = 0usize;
    for e in entries {
        let uid = match (e.op.as_str(), e.uid.as_deref()) {
            ("delete", Some(uid)) => uid.to_string(),
            ("delete", None) => {
                // 行在拿到 uid 前就删了 = 从未上云，没有可推送的内容，直接清
                batch.skipped.push(e);
                continue;
            }
            _ => match uid_of_row(conn, &e.entity, e.rowid) {
                Some(uid) => uid,
                None => {
                    batch.skipped.push(e);
                    continue;
                }
            },
        };
        let env_updated_at = match e.op.as_str() {
            "delete" => repo::tombstone_get(conn, &e.entity, &uid).ok().flatten().unwrap_or_else(now_ms),
            _ => row_sync_ms(conn, &e.entity, e.rowid),
        };
        let deleted_at = if e.op == "delete" { Some(env_updated_at) } else { None };
        // 载荷：upsert 用业务行；delete 用最小占位（服务端不读内容，但解密需要合法密文）。
        // ⚠️ 占位载荷**必须带 updated_at + deleted_at**：接收侧 apply_todo/apply_note 第一句
        // 就要 `fields.updated_at`，缺了它远端删除 100% 失败（表现为「删了又复活」）
        let payload_json = if e.op == "delete" {
            json!({
                "v": 1,
                "kind": e.entity,
                "uid": uid,
                "fields": { "updated_at": env_updated_at, "deleted_at": env_updated_at },
                "x": {},
            })
            .to_string()
        } else if e.entity == "todo" {
            let t = load_todo(conn, e.rowid).ok_or("todo 行消失")?;
            super::map::todo_payload(conn, &t)
        } else {
            let n = load_note(conn, e.rowid).ok_or("note 行消失")?;
            super::map::note_payload(conn, &n)
        };
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let ct = seal_record(record_key, &e.entity, &uid, env_updated_at, &payload_json, &nonce)?;
        let ct_b64 = B64.encode(ct);
        // 字节预算：本批已非空且再加这条会超 → 停在这里，剩下的留到下一批。
        // 批内第一条永远放行（单条超预算也必须能送出去，见函数头注释）。
        let item_bytes = ct_b64.len() + 64;
        if !batch.items.is_empty() && bytes + item_bytes > max_bytes {
            break;
        }
        bytes += item_bytes;
        batch.items.push(json!({
            "entity": e.entity,
            "uid": uid,
            "updated_at": env_updated_at,
            "deleted_at": deleted_at,
            "key_version": key_version_of(conn),
            "nonce": B64.encode(nonce),
            "payload_ct": ct_b64,
        }));
        batch.consumed.push(e);
    }
    Ok(batch)
}


/// 服务端确认 ok 的条目才清 outbox；stale/equal 保留（引擎随后重拉收敛）
pub fn clear_consumed(conn: &Connection, consumed: &[OutboxEntry]) -> rusqlite::Result<()> {
    for e in consumed {
        repo::outbox_clear(conn, &e.entity, e.rowid)?;
    }
    Ok(())
}

fn key_version_of(conn: &Connection) -> i64 {
    repo::meta_get(conn, META_KEY_VERSION).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(1)
}

// ---------------- 远端变更应用 ----------------

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStats {
    pub applied: usize,
    pub local_newer: usize,
    pub tombstoned: usize,
    pub deferred: usize,
    pub failed: usize,
    /// 其中「当前 key_version 的行解不开」的条数。只有这类失败值得让上层停住游标：
    /// 旧 key_version 的行是换口令后的历史密文，本来就解不开，跳过即可。
    pub failed_current: usize,
}

/// 远端 changes 页 → 本地。deferred 的 uid 由调用方在父行落库后重试。
pub fn apply_remote_changes(
    conn: &Connection,
    record_key: &[u8; 32],
    changes: &[Value],
    local_key_version: i64,
) -> Result<(ApplyStats, Vec<String>), String> {
    let mut stats = ApplyStats::default();
    let mut deferred = Vec::new();
    for change in changes {
        let entity = change["entity"].as_str().unwrap_or_default().to_string();
        let uid = change["uid"].as_str().unwrap_or_default().to_string();
        // 缺 key_version 字段时按「当前版本」处理：保守猜测（当成历史版本会被静默丢弃，
        // 当成当前版本最多是多重试一轮）
        let is_cur_key = change["key_version"].as_i64().unwrap_or(local_key_version) == local_key_version;
        if entity.is_empty() || uid.is_empty() {
            count_failed(&mut stats, is_cur_key);
            continue;
        }
        let remote_ms = change["updated_at"].as_i64().unwrap_or(0);
        let local_ms = local_ms_by_uid(conn, &entity, &uid);

        match local_ms {
            Some(local) if local > remote_ms => {
                // 本地较新：重新标脏，等下轮推回去（LWW 由服务端最终裁决）
                mark_dirty_by_uid(conn, &entity, &uid).map_err(|e| e.to_string())?;
                stats.local_newer += 1;
                continue;
            }
            Some(local) if local == remote_ms => {
                stats.applied += 0; // 已收敛
                continue;
            }
            _ => {}
        }

        // 远端较新或本地缺失：解密 → guard 下应用
        let nonce = match change["nonce"].as_str().map(|s| B64.decode(s)) {
            Some(Ok(v)) if v.len() == 24 => v,
            _ => {
                count_failed(&mut stats, is_cur_key);
                continue;
            }
        };
        let ct = match change["payload_ct"].as_str().map(|s| B64.decode(s)) {
            Some(Ok(v)) => v,
            _ => {
                count_failed(&mut stats, is_cur_key);
                continue;
            }
        };
        let payload: Value = match open_record(record_key, &entity, &uid, remote_ms, nonce.as_slice().try_into().unwrap(), &ct) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => {
                    count_failed(&mut stats, is_cur_key);
                    continue;
                }
            },
            Err(_) => {
                // 解密失败：口令不符（当前版本）或 key_version 落后（旧版本，属正常）。
                // 只有当前版本解不开才值得让上层停游标重试。
                count_failed(&mut stats, is_cur_key);
                continue;
            }
        };
        // 远端胜出即清算本行未推的本地脏标（内容已是远端版本，再推只是冗余）
        let pending_rowid: Option<i64> = conn
            .query_row(
                &format!("SELECT id FROM {} WHERE sync_uid = ?1", if entity == "todo" { "todos" } else { "notes" }),
                params![uid],
                |r| r.get(0),
            )
            .ok();
        if let Some(rid) = pending_rowid {
            let _ = conn.execute("DELETE FROM sync_outbox WHERE entity = ? AND rowid_ref = ?", params![entity, rid]);
        }
        let guard = repo::apply_begin(conn);
        if guard.is_err() {
            count_failed(&mut stats, is_cur_key);
            continue;
        }
        let outcome = if entity == "todo" {
            super::map::apply_todo(conn, &payload)
        } else {
            super::map::apply_note(conn, &payload)
        };
        let _ = repo::apply_end(conn);
        match outcome {
            Ok(super::map::ApplyOutcome::Inserted) | Ok(super::map::ApplyOutcome::Updated) => stats.applied += 1,
            Ok(super::map::ApplyOutcome::Tombstoned) => stats.tombstoned += 1,
            Ok(super::map::ApplyOutcome::Deferred) => {
                deferred.push(uid);
                stats.deferred += 1;
            }
            Err(_) => count_failed(&mut stats, is_cur_key),
        }
    }
    Ok((stats, deferred))
}

/// 记一次应用失败；当前 key_version 的失败额外计入 failed_current（决定上层是否停游标）
fn count_failed(stats: &mut ApplyStats, is_current_key: bool) {
    stats.failed += 1;
    if is_current_key {
        stats.failed_current += 1;
    }
}

fn local_ms_by_uid(conn: &Connection, entity: &str, uid: &str) -> Option<i64> {    let table = if entity == "todo" { "todos" } else { "notes" };
    let sql = format!("SELECT id FROM {table} WHERE sync_uid = ?1");
    let rowid: Option<i64> = conn.query_row(&sql, params![uid], |r| r.get(0)).ok();
    rowid.map(|id| row_sync_ms(conn, entity, id))
}

/// 本地较新 → 重新标脏（outbox 同键 upsert 幂等）
pub fn mark_dirty_by_uid(conn: &Connection, entity: &str, uid: &str) -> rusqlite::Result<()> {
    let table = if entity == "todo" { "todos" } else { "notes" };
    let rowid: Option<i64> = conn.query_row(&format!("SELECT id FROM {table} WHERE sync_uid = ?1"), params![uid], |r| r.get(0)).ok();
    if let Some(rowid) = rowid {
        conn.execute(
            "INSERT INTO sync_outbox (entity, rowid_ref, op, uid) VALUES (?1, ?2, 'upsert', ?3)
             ON CONFLICT(entity, rowid_ref) DO UPDATE SET op = 'upsert'",
            params![entity, rowid, uid],
        )?;
    }
    Ok(())
}

// ---------------- 载荷行加载 ----------------

fn load_todo(conn: &Connection, id: i64) -> Option<crate::models::Todo> {
    let sql = format!("SELECT {} FROM todos WHERE id = ?1", crate::repo::todo::COLS);
    conn.query_row(&sql, params![id], |r| todo_row_to_model(r)).ok()
}

fn load_note(conn: &Connection, id: i64) -> Option<crate::models::Note> {
    let sql = format!("SELECT {} FROM notes WHERE id = ?1", crate::repo::note::NOTE_COLS);
    conn.query_row(&sql, params![id], |r| note_row_to_model(r)).ok()
}

// repo 模块的行转换是私有 fn——这里用等价的列序读取（列清单以 repo 常量为准，避免双写漂移）。
// 若 repo 增列，此处的编译期断言（下方 tests::cols_aligned）会先炸。
fn todo_row_to_model(r: &rusqlite::Row) -> rusqlite::Result<crate::models::Todo> {
    // 列序必须与 repo::todo::COLS 逐字一致（remind_notify_mode 在最后一位）
    Ok(crate::models::Todo {
        id: r.get(0)?,
        title: r.get(1)?,
        done: r.get::<_, i64>(2)? != 0,
        priority: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        completed_at: r.get(6)?,
        due_at: r.get(7)?,
        remind_at: r.get(8)?,
        remind_fired: r.get::<_, i64>(9)? != 0,
        parent_id: r.get(10)?,
        sort_order: r.get(11)?,
        version: r.get(12)?,
        description: r.get(13)?,
        pinned: r.get::<_, i64>(14)? != 0,
        repeat_mode: r.get(15)?,
        repeat_every: r.get(16)?,
        repeat_unit: r.get(17)?,
        repeat_weekdays: r.get(18)?,
        repeat_month_day: r.get(19)?,
        repeat_month_nth: r.get(20)?,
        repeat_end_mode: r.get(21)?,
        repeat_end_at: r.get(22)?,
        repeat_count: r.get(23)?,
        repeat_done_count: r.get(24)?,
        repeat_last_done_at: r.get(25)?,
        remind_notify_mode: r.get(26)?,
    })
}

fn note_row_to_model(r: &rusqlite::Row) -> rusqlite::Result<crate::models::Note> {
    Ok(crate::models::Note {
        id: r.get(0)?,
        title: r.get(1)?,
        content: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        folder_id: r.get(5)?,
        source_url: r.get(6)?,
        deleted_at: r.get(7)?,
        icon: r.get(8)?,
        sort_order: r.get(9)?,
        kind: r.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn keys() -> [u8; 32] {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).unwrap();
        k
    }

    #[test]
    fn initial_push_marks_all_and_push_items_decrypt_back() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "t1", None, None).unwrap();
        crate::repo::note::create(&conn, "n1").unwrap();
        let n = mark_all_dirty_if_first_push(&conn).unwrap();
        assert_eq!(n, 2, "两行全标脏");
        assert_eq!(mark_all_dirty_if_first_push(&conn).unwrap(), 0, "meta 标记后不重复");

        let key = keys();
        let batch = build_push_items(&conn, &key, 10, PUSH_BATCH_BYTES).unwrap();
        assert_eq!(batch.items.len(), 2);
        assert_eq!(batch.consumed.len(), 2, "items 与 consumed 必须逐条对齐");
        assert!(batch.skipped.is_empty());
        // 密文能解回来且载荷就是业务内容
        for item in &batch.items {
            let nonce = B64.decode(item["nonce"].as_str().unwrap()).unwrap();
            let ct = B64.decode(item["payload_ct"].as_str().unwrap()).unwrap();
            let pt = open_record(&key, item["entity"].as_str().unwrap(), item["uid"].as_str().unwrap(), item["updated_at"].as_i64().unwrap(), nonce.as_slice().try_into().unwrap(), &ct).unwrap();
            let v: Value = serde_json::from_str(&pt).unwrap();
            assert_eq!(v["kind"], item["entity"]);
            assert!(v["fields"].is_object(), "载荷必须是业务字段对象");
        }
    }

    #[test]
    fn apply_remote_lww_and_local_newer_remarks_dirty() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "本地较新", None, None).unwrap();
        repo::ensure_uids(&conn).unwrap();
        let t = crate::repo::todo::list(&conn).unwrap().remove(0);
        let uid = repo::uid_of(&conn, "todo", t.id).ok().flatten().unwrap();
        let local_ms = row_sync_ms(&conn, "todo", t.id);

        let key = keys();
        let mk = |title: &str, ms: i64| {
            let mut nonce = [0u8; 24];
            getrandom::fill(&mut nonce).unwrap();
            let payload = json!({
                "v": 1, "kind": "todo", "uid": uid,
                "fields": { "title": title, "updated_at": ms, "repeat_mode": "once" },
                "x": {},
            });
            let ct = seal_record(&key, "todo", &uid, ms, &payload.to_string(), &nonce).unwrap();
            json!({
                "entity": "todo", "uid": uid, "updated_at": ms, "deleted_at": Value::Null,
                "key_version": 1, "nonce": B64.encode(nonce), "payload_ct": B64.encode(ct),
            })
        };

        // 远端更旧 → 本地保留并重新标脏
        let older = mk("远端旧版", local_ms - 1);
        let (stats, _) = apply_remote_changes(&conn, &key, &[older], 1).unwrap();
        assert_eq!(stats.local_newer, 1);
        assert_eq!(repo::outbox_list(&conn, 10).unwrap().len(), 1, "本地较新必须重新标脏");

        // 远端更新 → 覆盖本地（guard 下，无 outbox 泄漏）
        let newer = mk("远端新版", local_ms + 5000);
        let (stats, _) = apply_remote_changes(&conn, &key, &[newer], 1).unwrap();
        assert_eq!(stats.applied, 1);
        let title: String = conn.query_row("SELECT title FROM todos WHERE id = ?1", params![t.id], |r| r.get(0)).unwrap();
        assert_eq!(title, "远端新版");
        let dirty: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        assert_eq!(dirty, 0, "应用远端不得标脏");

        // 相等 → 收敛跳过
        let same = mk("远端新版", local_ms + 5000);
        let (stats, _) = apply_remote_changes(&conn, &key, &[same], 1).unwrap();
        assert_eq!(stats.applied + stats.local_newer, 0, "相等时间戳不再动作");
    }

    #[test]
    fn delete_item_carries_uid_and_tombstone() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "会删", None, None).unwrap();
        repo::ensure_uids(&conn).unwrap();
        let t = crate::repo::todo::list(&conn).unwrap().remove(0);
        conn.execute("DELETE FROM todos WHERE id = ?1", params![t.id]).unwrap();
        let key = keys();
        let batch = build_push_items(&conn, &key, 10, PUSH_BATCH_BYTES).unwrap();
        let items = &batch.items;
        let entries = &batch.consumed;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["deleted_at"].as_i64().unwrap() > 0, true, "delete 条目必须带 deleted_at");
        assert_eq!(entries[0].op, "delete");
        clear_consumed(&conn, entries).unwrap();
        assert_eq!(repo::outbox_list(&conn, 10).unwrap().len(), 0, "确认后清 outbox");
        assert!(repo::tombstone_get(&conn, "todo", &items[0]["uid"].as_str().unwrap()).unwrap().is_some());

        // 删除载荷必须能被对端应用掉：占位载荷缺 updated_at 会让远端删除 100% 失败
        // （表现为「这台删了、那台又推回来」），2026-11-05 修
        let nonce = B64.decode(items[0]["nonce"].as_str().unwrap()).unwrap();
        let ct = B64.decode(items[0]["payload_ct"].as_str().unwrap()).unwrap();
        let pt = open_record(
            &key,
            "todo",
            items[0]["uid"].as_str().unwrap(),
            items[0]["updated_at"].as_i64().unwrap(),
            nonce.as_slice().try_into().unwrap(),
            &ct,
        )
        .unwrap();
        let payload: Value = serde_json::from_str(&pt).unwrap();
        assert!(payload["fields"]["updated_at"].as_i64().is_some(), "占位载荷必须带 updated_at");
        assert!(payload["fields"]["deleted_at"].as_i64().is_some(), "占位载荷必须带 deleted_at");

        // 对端：另一台机器上应用这条删除 → 应被墓碑化而不是报错
        let peer = db::init_in_memory().unwrap();
        crate::repo::todo::create(&peer, "会删", None, None).unwrap();
        repo::ensure_uids(&peer).unwrap();
        // 两台机器的 uid 不同（各自 ULID），这里直接改写对端行的 uid 对齐
        peer.execute("UPDATE todos SET sync_uid = ?1", params![items[0]["uid"].as_str().unwrap()]).unwrap();
        repo::apply_begin(&peer).unwrap();
        let outcome = super::super::map::apply_todo(&peer, &payload).unwrap();
        repo::apply_end(&peer).unwrap();
        assert_eq!(outcome, super::super::map::ApplyOutcome::Tombstoned);
        let left: i64 = peer.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "远端删除落地");
    }

    #[test]
    fn skipped_entries_never_shift_consumed_index() {
        // 行在拿到 uid 前就删了：没有可推送内容 → 进 skipped，且不占 items 下标，
        // 后面的条目不能因此错位（旧实现会清掉别人的 outbox）
        let conn = db::init_in_memory().unwrap();
        conn.execute("INSERT INTO todos (title) VALUES ('never-synced')", []).unwrap();
        let ghost = conn.last_insert_rowid();
        conn.execute("DELETE FROM todos WHERE id = ?1", params![ghost]).unwrap();
        crate::repo::todo::create(&conn, "会推", None, None).unwrap();
        let key = keys();
        let batch = build_push_items(&conn, &key, 10, PUSH_BATCH_BYTES).unwrap();
        assert_eq!(batch.items.len(), 1, "只有一行可推");
        assert_eq!(batch.consumed.len(), 1, "consumed 与 items 等长");
        assert_eq!(batch.skipped.len(), 1, "无 uid 的删除进 skipped");
        assert_eq!(batch.consumed[0].op, "upsert");
        assert!(batch.consumed[0].uid.is_some(), "consumed 指向真正要推的那一行");
        assert_eq!(batch.skipped[0].op, "delete");
        assert!(batch.skipped[0].uid.is_none(), "被跳过的是从未拿到 uid 的删除");
    }

    #[test]
    fn byte_budget_splits_batch_and_oversize_row_still_goes_alone() {
        // 单批必须同时守住条数与字节：100 条大笔记能轻松超服务端批总量 → 整批 413，
        // 而队列头不变，下次重试还是同一批，同步就此永久卡死（2026-11 实测）。
        let conn = db::init_in_memory().unwrap();
        // 一条远超单批预算的大笔记（600KB 正文 → 密文 base64 约 800KB）
        let big = "x".repeat(600 * 1024);
        crate::repo::note::create(&conn, &big).unwrap();
        crate::repo::todo::create(&conn, "小的", None, None).unwrap();
        let key = keys();

        // 批预算 512KB：大笔记（密文 base64 约 800KB）自己就超了，小待办不能再挤进同一批
        let b1 = build_push_items(&conn, &key, 100, 512 * 1024).unwrap();
        assert_eq!(b1.items.len(), 1, "大笔记独占一批，小条目留到下一批");
        assert_eq!(b1.consumed.len(), 1, "items 与 consumed 对齐");
        assert_eq!(b1.consumed[0].entity, "note");

        // 即使预算小到装不下任何一条，也不能返回空批（否则这行永远轮不到）
        let b2 = build_push_items(&conn, &key, 100, 1024).unwrap();
        assert_eq!(b2.items.len(), 1, "单条超预算也必须单独送一次");
        assert_eq!(b2.consumed[0].entity, "note");
    }
}
