//! 同步数据层：outbox / 墓碑 / meta / guard / uid 惰性回填（docs/sync-protocol.md §8）。
//!
//! 与业务 repo 的边界：本模块只动 sync_* 表和 notes/todos 的 sync_uid / sync_updated_at 两列，
//! 绝不碰业务字段——业务字段的路由在 map.rs，网络在 api.rs，编排在 engine.rs。
use rusqlite::{params, Connection};

/// outbox 一行。delete 的 uid 由触发器写入（业务行已删，uid 只存在于这里）。
#[derive(Debug, Clone)]
pub struct OutboxEntry {
    pub entity: String,
    pub rowid: i64,
    pub op: String,
    pub uid: Option<String>,
}

/// 取待推送条目；ORDER BY rowid_ref 让"父先子后"自然成立（子待办总是后创建）
pub fn outbox_list(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare(
        "SELECT entity, rowid_ref, op, uid FROM sync_outbox ORDER BY rowid_ref ASC LIMIT ?",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok(OutboxEntry {
            entity: r.get(0)?,
            rowid: r.get(1)?,
            op: r.get(2)?,
            uid: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn outbox_clear(conn: &Connection, entity: &str, rowid: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sync_outbox WHERE entity = ? AND rowid_ref = ?", params![entity, rowid])?;
    Ok(())
}

/// 给 outbox 中 uid 为空的 upsert 行回填 ULID（推送前置步骤；SQLite 触发器造不出 ULID）。
/// 返回回填数量。回填的 UPDATE 会再触发脏触发器，但 outbox 同键 upsert 幂等，无副作用。
pub fn ensure_uids(conn: &Connection) -> rusqlite::Result<usize> {
    let mut filled = 0;
    for entity in ["todo", "note"] {
        let table = if entity == "todo" { "todos" } else { "notes" };
        let pairs: Vec<(i64, i64)> = {
            let mut stmt = conn.prepare(
                &format!(
                    "SELECT o.rowid_ref, t.id FROM sync_outbox o
                      JOIN {table} t ON t.id = o.rowid_ref
                     WHERE o.entity = ? AND o.op = 'upsert' AND t.sync_uid IS NULL
                     ORDER BY o.rowid_ref"
                )
            )?;
            let rows = stmt.query_map(params![entity], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<(i64, i64)>>>()?
        };
        for (_outbox_rowid, biz_id) in pairs {
            let uid = ulid::Ulid::new().to_string();
            conn.execute(&format!("UPDATE {table} SET sync_uid = ? WHERE id = ?"), params![uid, biz_id])?;
            conn.execute(
                "UPDATE sync_outbox SET uid = ? WHERE entity = ? AND rowid_ref = ?",
                params![uid, entity, biz_id],
            )?;
            filled += 1;
        }
    }
    Ok(filled)
}

/// 按业务行读 uid（upsert 推送用）
pub fn uid_of(conn: &Connection, entity: &str, rowid: i64) -> rusqlite::Result<Option<String>> {
    let table = if entity == "todo" { "todos" } else { "notes" };
    let mut stmt = conn.prepare(&format!("SELECT sync_uid FROM {table} WHERE id = ?"))?;
    let uid = stmt.query_row(params![rowid], |r| r.get(0))?;
    Ok(uid)
}

/// 记录一条本地墓碑（本地删除已上过云的行时触发器已写；引擎应用远端墓碑也走这里）
pub fn tombstone_add(conn: &Connection, entity: &str, uid: &str, deleted_at_ms: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_tombstones (entity, uid, deleted_at_ms) VALUES (?, ?, ?)",
        params![entity, uid, deleted_at_ms],
    )?;
    Ok(())
}

pub fn tombstone_get(conn: &Connection, entity: &str, uid: &str) -> rusqlite::Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT deleted_at_ms FROM sync_tombstones WHERE entity = ? AND uid = ?")?;
    let ts = stmt.query_row(params![entity, uid], |r| r.get(0))?;
    Ok(ts)
}

// ---------------- meta（游标 / 空间绑定 / key_version 等本地同步状态） ----------------

pub fn meta_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT v FROM sync_meta WHERE k = ?")?;
    let v = stmt
        .query_row(params![key], |r| r.get(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(v)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sync_meta (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![key, value],
    )?;
    Ok(())
}

/// 删除一条 meta（**不要写空串当"已清除"**：`sync_status` 用 `is_some()` 判是否已配对，
/// 空串会被当成"有空间"，表现为关闭同步后仍显示已开启、且再也回不到设口令的入口）
pub fn meta_del(conn: &Connection, key: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sync_meta WHERE k = ?", params![key])?;
    Ok(())
}

// ---------------- guard（引擎应用远端数据期间静默触发器） ----------------

/// 显式 begin/end 对（引擎在应用远端记录的事务外套用；end 幂等）。
/// 有意不做 RAII：guard 的生命周期必须跨越"读载荷 → 写库 → 收尾"的完整事务，
/// 隐式 Drop 在错误路径上可能提前解除，静默窗口比显式配对更难审计。
pub fn apply_begin(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO sync_guard (on_) VALUES (1)", [])?;
    Ok(())
}

pub fn apply_end(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sync_guard", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn outbox_uid_backfill_and_clear() {
        let conn = db::init_in_memory().unwrap();
        conn.execute("INSERT INTO todos (title) VALUES ('a')", []).unwrap();
        let id = conn.last_insert_rowid();
        let entries = outbox_list(&conn, 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].uid.is_none(), "新行 uid 为空，等回填");

        let n = ensure_uids(&conn).unwrap();
        assert_eq!(n, 1, "回填 1 行");
        let entries = outbox_list(&conn, 10).unwrap();
        assert_eq!(entries[0].uid.as_deref().map(str::len), Some(26), "outbox 带 ULID");
        assert_eq!(uid_of(&conn, "todo", id).unwrap().as_deref().map(str::len), Some(26));

        outbox_clear(&conn, "todo", id).unwrap();
        assert_eq!(outbox_list(&conn, 10).unwrap().len(), 0);
    }

    #[test]
    fn outbox_delete_keeps_uid_after_row_gone() {
        let conn = db::init_in_memory().unwrap();
        conn.execute("INSERT INTO todos (title) VALUES ('b')", []).unwrap();
        let id = conn.last_insert_rowid();
        ensure_uids(&conn).unwrap();
        let uid = uid_of(&conn, "todo", id).unwrap().unwrap();
        conn.execute("DELETE FROM todos WHERE id = ?1", params![id]).unwrap();
        let entries = outbox_list(&conn, 10).unwrap();
        assert_eq!(entries[0].op, "delete");
        assert_eq!(entries[0].uid.as_deref(), Some(uid.as_str()), "行删了 uid 不丢");
        assert!(tombstone_get(&conn, "todo", &uid).unwrap().is_some());
    }

    #[test]
    fn guard_roundtrip_and_meta() {
        let conn = db::init_in_memory().unwrap();
        apply_begin(&conn).unwrap();
        conn.execute("INSERT INTO todos (title) VALUES ('quiet')", []).unwrap();
        apply_end(&conn).unwrap();
        assert_eq!(outbox_list(&conn, 10).unwrap().len(), 0, "guard 期间无 outbox");

        meta_set(&conn, "pull_seq", "42").unwrap();
        assert_eq!(meta_get(&conn, "pull_seq").unwrap().as_deref(), Some("42"));
        meta_del(&conn, "pull_seq").unwrap();
        assert_eq!(meta_get(&conn, "pull_seq").unwrap(), None, "关闭同步必须删键，不能写空串");
        assert_eq!(meta_get(&conn, "不存在").unwrap(), None);
    }
}
