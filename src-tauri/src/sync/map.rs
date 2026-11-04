//! 业务行 ⇄ 明文载荷（docs/sync-protocol.md §5）。
//!
//! 规则：
//! - 字段名与桌面 `models.rs` 逐字一致（snake_case）；时间在载荷里一律 ms。
//! - 本地专有字段（`id` / `version` / `remind_fired` / `remind_notify_mode`）**不进载荷**——
//!   它们是单机状态，同步会把一台设备的通知状态强加给另一台。
//! - `x` 对象存手机专有键的透传缓存（sync_x 表）：构建载荷时并入，应用载荷时回存。
//! - 应用远端载荷必须发生在引擎的 guard 事务内（本模块不再自己开关 guard）。
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::db::{format_local_ts_ms, parse_local_ts_ms};
use crate::models::{Note, Todo};
use crate::sync::repo;

/// 应用远端载荷的结果（引擎据此决定是否延迟重试子待办）
#[derive(Debug, PartialEq)]
pub enum ApplyOutcome {
    Inserted,
    Updated,
    /// 远端是墓碑：本地行已删/未建，只留墓碑
    Tombstoned,
    /// parent_uid 指向的父行本地还没有（引擎排完父再试一次）
    Deferred,
}

// ---------------- x 透传 ----------------

fn x_get(conn: &Connection, entity: &str, uid: &str) -> Value {
    let raw: Option<String> = conn
        .query_row("SELECT json FROM sync_x WHERE entity = ? AND uid = ?", params![entity, uid], |r| r.get(0))
        .ok();
    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}))
}

fn x_put(conn: &Connection, entity: &str, uid: &str, x: &Value) {
    let json = x.to_string();
    conn.execute(
        "INSERT INTO sync_x (entity, uid, json) VALUES (?, ?, ?)
         ON CONFLICT(entity, uid) DO UPDATE SET json = excluded.json",
        params![entity, uid, json],
    )
    .ok(); // x 是尽力而为的透传缓存，写失败不应中断同步
}

// ---------------- 时间换算 ----------------

fn ts_text(ms: Value) -> Option<String> {
    ms.as_i64().map(format_local_ts_ms)
}

fn ts_ms(text: &str) -> Value {
    parse_local_ts_ms(text).map(|v| json!(v)).unwrap_or(Value::Null)
}

// ---------------- Todo ----------------

/// 待办标签快照：桌面 todo_tags/todo_tag_links ↔ [{name,color}]
fn todo_tags_read(conn: &Connection, todo_id: i64) -> Value {
    let mut stmt = conn
        .prepare(
            "SELECT t.name, t.color FROM todo_tags t
              JOIN todo_tag_links l ON l.tag_id = t.id
             WHERE l.todo_id = ? ORDER BY t.name",
        )
        .expect("todo_tags read");
    let tags: Vec<Value> = stmt
        .query_map(params![todo_id], |r| Ok(json!({ "name": r.get::<_, String>(0)?, "color": r.get::<_, String>(1)? })))
        .expect("todo_tags map")
        .collect::<rusqlite::Result<_>>()
        .unwrap_or_default();
    Value::Array(tags)
}

fn todo_tags_write(conn: &Connection, todo_id: i64, tags: &Value) {
    conn.execute("DELETE FROM todo_tag_links WHERE todo_id = ?1", params![todo_id]).ok();
    for t in tags.as_array().unwrap_or(&vec![]) {
        let (name, color) = match (t.get("name").and_then(Value::as_str), t.get("color").and_then(Value::as_str)) {
            (Some(n), c) => (n, c.unwrap_or("")),
            _ => continue,
        };
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM todo_tags WHERE name = ?1", params![name], |r| r.get(0))
            .ok();
        let tag_id = match existing {
            Some(id) => id,
            None => {
                if conn
                    .execute("INSERT INTO todo_tags (name, color) VALUES (?1, ?2)", params![name, color])
                    .is_err()
                {
                    continue;
                }
                conn.last_insert_rowid()
            }
        };
        conn.execute(
            "INSERT OR IGNORE INTO todo_tag_links (todo_id, tag_id) VALUES (?1, ?2)",
            params![todo_id, tag_id],
        )
        .ok();
    }
}

/// 本地待办 → 明文载荷
pub fn todo_payload(conn: &Connection, t: &Todo) -> String {
    let uid: Option<String> = conn
        .query_row("SELECT sync_uid FROM todos WHERE id = ?1", params![t.id], |r| r.get(0))
        .ok();
    let uid = uid.unwrap_or_default();
    let parent_uid: Option<String> = t
        .parent_id
        .and_then(|pid| conn.query_row("SELECT sync_uid FROM todos WHERE id = ?1", params![pid], |r| r.get(0)).ok());
    let fields = json!({
        "title": t.title,
        "done": t.done,
        "priority": t.priority,
        "created_at": ts_ms(&t.created_at),
        "updated_at": ts_ms(&t.updated_at),
        "completed_at": t.completed_at.as_deref().map(ts_ms).unwrap_or(Value::Null),
        "due_at": t.due_at.map(|v| json!(v)).unwrap_or(Value::Null),
        "remind_at": t.remind_at.map(|v| json!(v)).unwrap_or(Value::Null),
        "parent_uid": parent_uid,
        "sort_order": t.sort_order,
        "description": t.description,
        "pinned": t.pinned,
        "repeat_mode": t.repeat_mode,
        "repeat_every": t.repeat_every,
        "repeat_unit": t.repeat_unit,
        "repeat_weekdays": t.repeat_weekdays,
        "repeat_month_day": t.repeat_month_day,
        "repeat_month_nth": t.repeat_month_nth,
        "repeat_end_mode": t.repeat_end_mode,
        "repeat_end_at": t.repeat_end_at,
        "repeat_count": t.repeat_count,
        "repeat_done_count": t.repeat_done_count,
        "repeat_last_done_at": t.repeat_last_done_at.as_deref().map(ts_ms).unwrap_or(Value::Null),
        "tags": todo_tags_read(conn, t.id),
    });
    let payload = json!({
        "v": 1,
        "kind": "todo",
        "uid": uid,
        "fields": fields,
        "x": x_get(conn, "todo", &uid),
    });
    payload.to_string()
}

/// 远端待办载荷 → 本地行（调用方已保证 LWW 胜出且处于 guard 事务）
pub fn apply_todo(conn: &Connection, payload: &Value) -> Result<ApplyOutcome, String> {
    let uid = payload["uid"].as_str().ok_or("载荷缺 uid")?;
    let fields = &payload["fields"];
    let remote_ms = payload["fields"]["updated_at"]
        .as_i64()
        .or_else(|| payload.get("fields").and_then(|f| f.get("updated_at")).and_then(Value::as_i64))
        .ok_or("载荷缺 updated_at")?;
    let deleted_ms = fields.get("deleted_at").and_then(Value::as_i64);
    let updated_text = format_local_ts_ms(remote_ms);

    let parent_id: Option<i64> = match fields.get("parent_uid").and_then(Value::as_str) {
        Some(puid) if !puid.is_empty() => {
            match conn.query_row("SELECT id FROM todos WHERE sync_uid = ?1", params![puid], |r| r.get(0)) {
                Ok(id) => Some(id),
                Err(_) => return Ok(ApplyOutcome::Deferred),
            }
        }
        _ => None,
    };

    if deleted_ms.is_some() {
        // 远端墓碑：删本地行（触发器被 guard 静默），墓碑显式记录
        conn.execute("DELETE FROM todos WHERE sync_uid = ?1", params![uid]).map_err(|e| e.to_string())?;
        repo::tombstone_add(conn, "todo", uid, deleted_ms.unwrap_or(remote_ms)).map_err(|e| e.to_string())?;
        return Ok(ApplyOutcome::Tombstoned);
    }

    let existing: Option<i64> = conn
        .query_row("SELECT id FROM todos WHERE sync_uid = ?1", params![uid], |r| r.get(0))
        .ok();
    let val = |k: &str| fields.get(k).cloned().unwrap_or(Value::Null);
    let opt_i64 = |k: &str| val(k).as_i64();
    let opt_text = |k: &str| val(k).as_str().map(str::to_owned);
    let created_text = ts_text(val("created_at")).unwrap_or_else(|| updated_text.clone());
    let repeat_last = ts_text(val("repeat_last_done_at"));

    if let Some(id) = existing {
        conn.execute(
            "UPDATE todos SET title=?1, done=?2, priority=?3, created_at=?4, updated_at=?5, completed_at=?6,
               due_at=?7, remind_at=?8, parent_id=?9, sort_order=?10, description=?11, pinned=?12,
               repeat_mode=?13, repeat_every=?14, repeat_unit=?15, repeat_weekdays=?16, repeat_month_day=?17,
               repeat_month_nth=?18, repeat_end_mode=?19, repeat_end_at=?20, repeat_count=?21,
               repeat_done_count=?22, repeat_last_done_at=?23, sync_updated_at=?24
             WHERE id=?25",
            params![
                val("title").as_str().unwrap_or(""),
                val("done").as_bool().unwrap_or(false) as i64,
                val("priority").as_i64().unwrap_or(0),
                created_text,
                updated_text,
                ts_text(val("completed_at")),
                opt_i64("due_at"),
                opt_i64("remind_at"),
                parent_id,
                val("sort_order").as_i64(),
                val("description").as_str().unwrap_or(""),
                val("pinned").as_bool().unwrap_or(false) as i64,
                val("repeat_mode").as_str().unwrap_or("once"),
                opt_i64("repeat_every"),
                opt_text("repeat_unit"),
                opt_i64("repeat_weekdays"),
                opt_i64("repeat_month_day"),
                opt_i64("repeat_month_nth"),
                opt_text("repeat_end_mode"),
                opt_i64("repeat_end_at"),
                opt_i64("repeat_count"),
                val("repeat_done_count").as_i64().unwrap_or(0),
                repeat_last,
                remote_ms,
                id,
            ],
        )
        .map_err(|e| e.to_string())?;
        todo_tags_write(conn, id, &val("tags"));
        if let Some(x) = payload.get("x") { x_put(conn, "todo", uid, x); }
        return Ok(ApplyOutcome::Updated);
    }

    conn.execute(
        "INSERT INTO todos (title, done, priority, created_at, updated_at, completed_at, due_at, remind_at,
           parent_id, sort_order, description, pinned, repeat_mode, repeat_every, repeat_unit, repeat_weekdays,
           repeat_month_day, repeat_month_nth, repeat_end_mode, repeat_end_at, repeat_count, repeat_done_count,
           repeat_last_done_at, sync_uid, sync_updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
        params![
            val("title").as_str().unwrap_or(""),
            val("done").as_bool().unwrap_or(false) as i64,
            val("priority").as_i64().unwrap_or(0),
            created_text,
            updated_text,
            ts_text(val("completed_at")),
            opt_i64("due_at"),
            opt_i64("remind_at"),
            parent_id,
            val("sort_order").as_i64(),
            val("description").as_str().unwrap_or(""),
            val("pinned").as_bool().unwrap_or(false) as i64,
            val("repeat_mode").as_str().unwrap_or("once"),
            opt_i64("repeat_every"),
            opt_text("repeat_unit"),
            opt_i64("repeat_weekdays"),
            opt_i64("repeat_month_day"),
            opt_i64("repeat_month_nth"),
            opt_text("repeat_end_mode"),
            opt_i64("repeat_end_at"),
            opt_i64("repeat_count"),
            val("repeat_done_count").as_i64().unwrap_or(0),
            repeat_last,
            uid,
            remote_ms,
        ],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    todo_tags_write(conn, id, &val("tags"));
    if let Some(x) = payload.get("x") { x_put(conn, "todo", uid, x); }
    Ok(ApplyOutcome::Inserted)
}

// ---------------- Note ----------------

/// 文件夹路径 ↔ id（`工作/项目` 形式；逐级 find-or-create，删除语义不在此处理）
fn folder_path_of(conn: &Connection, folder_id: Option<i64>) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    let mut cur = folder_id;
    while let Some(fid) = cur {
        let row: Option<(String, Option<i64>)> = conn
            .query_row("SELECT name, parent_id FROM note_folders WHERE id = ?1", params![fid], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok();
        match row {
            Some((name, parent)) => {
                names.push(name);
                cur = parent;
            }
            None => break,
        }
    }
    if names.is_empty() { None } else { names.reverse(); Some(names.join("/")) }
}

fn folder_id_of_path(conn: &Connection, path: &str) -> Option<i64> {
    let mut parent: Option<i64> = None;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let found: Option<i64> = match parent {
            Some(pid) => conn
                .query_row("SELECT id FROM note_folders WHERE name = ?1 AND parent_id = ?2", params![seg, pid], |r| r.get(0))
                .ok(),
            None => conn
                .query_row("SELECT id FROM note_folders WHERE name = ?1 AND parent_id IS NULL", params![seg], |r| r.get(0))
                .ok(),
        };
        parent = match found {
            Some(id) => Some(id),
            None => {
                conn.execute("INSERT INTO note_folders (name, parent_id) VALUES (?1, ?2)", params![seg, parent]).ok()?;
                Some(conn.last_insert_rowid())
            }
        };
    }
    parent
}

/// 本地笔记 → 明文载荷
pub fn note_payload(conn: &Connection, n: &Note) -> String {
    let uid: Option<String> = conn
        .query_row("SELECT sync_uid FROM notes WHERE id = ?1", params![n.id], |r| r.get(0))
        .ok();
    let uid = uid.unwrap_or_default();
    let deleted_ms = n.deleted_at.as_deref().and_then(parse_local_ts_ms);
    let tags: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT t.name FROM tags t JOIN note_tags l ON l.tag_id = t.id WHERE l.note_id = ?1 ORDER BY t.name")
            .expect("note tags read");
        stmt.query_map(params![n.id], |r| r.get(0))
            .expect("note tags map")
            .collect::<rusqlite::Result<_>>()
            .unwrap_or_default()
    };
    let payload = json!({
        "v": 1,
        "kind": "note",
        "uid": uid,
        "fields": {
            "title": n.title,
            "content": n.content,
            "created_at": ts_ms(&n.created_at),
            "updated_at": ts_ms(&n.updated_at),
            "deleted_at": deleted_ms,
            "folder": folder_path_of(conn, n.folder_id),
            "tags": tags,
            "source_url": n.source_url,
            "icon": n.icon,
            "sort_order": n.sort_order,
        },
        "x": x_get(conn, "note", &uid),
    });
    payload.to_string()
}

/// 远端笔记载荷 → 本地行
pub fn apply_note(conn: &Connection, payload: &Value) -> Result<ApplyOutcome, String> {
    let uid = payload["uid"].as_str().ok_or("载荷缺 uid")?;
    let fields = &payload["fields"];
    let remote_ms = fields.get("updated_at").and_then(Value::as_i64).ok_or("载荷缺 updated_at")?;
    let deleted_ms = fields.get("deleted_at").and_then(Value::as_i64);
    let updated_text = format_local_ts_ms(remote_ms);
    let val = |k: &str| fields.get(k).cloned().unwrap_or(Value::Null);

    if deleted_ms.is_some() {
        conn.execute("DELETE FROM notes WHERE sync_uid = ?1", params![uid]).map_err(|e| e.to_string())?;
        repo::tombstone_add(conn, "note", uid, deleted_ms.unwrap_or(remote_ms)).map_err(|e| e.to_string())?;
        return Ok(ApplyOutcome::Tombstoned);
    }

    let existing: Option<i64> = conn
        .query_row("SELECT id FROM notes WHERE sync_uid = ?1", params![uid], |r| r.get(0))
        .ok();
    let folder_id = val("folder").as_str().and_then(|p| folder_id_of_path(conn, p));
    let opt_text = |k: &str| val(k).as_str().map(str::to_owned);
    let created_text = ts_text(val("created_at")).unwrap_or_else(|| updated_text.clone());

    if let Some(id) = existing {
        conn.execute(
            "UPDATE notes SET title=?1, content=?2, created_at=?3, updated_at=?4, folder_id=?5,
               source_url=?6, deleted_at=NULL, icon=?7, sort_order=?8, sync_updated_at=?9 WHERE id=?10",
            params![
                val("title").as_str().unwrap_or(""),
                val("content").as_str().unwrap_or(""),
                created_text,
                updated_text,
                folder_id,
                opt_text("source_url").unwrap_or_default(),
                opt_text("icon"),
                val("sort_order").as_i64(),
                remote_ms,
                id,
            ],
        )
        .map_err(|e| e.to_string())?;
        if let Some(x) = payload.get("x") { x_put(conn, "note", uid, x); }
        return Ok(ApplyOutcome::Updated);
    }

    conn.execute(
        "INSERT INTO notes (title, content, created_at, updated_at, folder_id, source_url, deleted_at,
           icon, sort_order, kind, sync_uid, sync_updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,NULL,?7,?8,'text',?9,?10)",
        params![
            val("title").as_str().unwrap_or(""),
            val("content").as_str().unwrap_or(""),
            created_text,
            updated_text,
            folder_id,
            opt_text("source_url").unwrap_or_default(),
            opt_text("icon"),
            val("sort_order").as_i64(),
            uid,
            remote_ms,
        ],
    )
    .map_err(|e| e.to_string())?;
    if let Some(x) = payload.get("x") { x_put(conn, "note", uid, x); }
    Ok(ApplyOutcome::Inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn todo_payload_apply_roundtrip() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "给花浇水 🌱", None, None).unwrap();
        repo::ensure_uids(&conn).unwrap();
        let todo = crate::repo::todo::list(&conn).unwrap().remove(0);
        let payload = todo_payload(&conn, &todo);
        let v: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["kind"], "todo");
        assert_eq!(v["fields"]["title"], "给花浇水 🌱");
        assert_eq!(v["fields"]["repeat_mode"], "once");

        // 全新库上应用（模拟对端）：插入行并保持字段一致
        let fresh = db::init_in_memory().unwrap();
        repo::apply_begin(&fresh).unwrap();
        let outcome = apply_todo(&fresh, &v).unwrap();
        repo::apply_end(&fresh).unwrap();
        assert_eq!(outcome, ApplyOutcome::Inserted);

        let round = crate::repo::todo::list(&fresh).unwrap().remove(0);
        assert_eq!(round.title, "给花浇水 🌱");
        assert_eq!(round.repeat_mode, "once");
        assert_eq!(round.priority, todo.priority);
        assert_eq!(crate::db::parse_local_ts_ms(&round.updated_at), crate::db::parse_local_ts_ms(&todo.updated_at));
        // 触发器被 guard 静默：应用远端不产生新的本地 outbox
        assert_eq!(repo::outbox_list(&fresh, 10).unwrap().len(), 0);
    }

    #[test]
    fn apply_tombstone_removes_row_and_records() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "会删的", None, None).unwrap();
        repo::ensure_uids(&conn).unwrap();
        let todo = crate::repo::todo::list(&conn).unwrap().remove(0);
        let payload: Value = serde_json::from_str(&todo_payload(&conn, &todo)).unwrap();
        let mut tomb = payload.clone();
        tomb["fields"]["deleted_at"] = json!(1_730_000_999_999i64);

        let outcome = apply_todo(&conn, &tomb).unwrap();
        assert_eq!(outcome, ApplyOutcome::Tombstoned);
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "远端墓碑必须删除本地行");
        let uid = payload["uid"].as_str().unwrap();
        assert_eq!(repo::tombstone_get(&conn, "todo", uid).unwrap(), Some(1_730_000_999_999));
    }

    #[test]
    fn child_defers_until_parent_exists() {
        let conn = db::init_in_memory().unwrap();
        crate::repo::todo::create(&conn, "父", None, None).unwrap();
        let parent = crate::repo::todo::list(&conn).unwrap().remove(0);
        crate::repo::todo::create(&conn, "子", Some(parent.id), None).unwrap();
        repo::ensure_uids(&conn).unwrap();

        let child = crate::repo::todo::list(&conn).unwrap().into_iter().find(|t| t.parent_id.is_some()).unwrap();
        let child_payload: Value = serde_json::from_str(&todo_payload(&conn, &child)).unwrap();

        // 空库先来子：父 uid 不存在 → Deferred（绝不落成顶级待办）
        let fresh = db::init_in_memory().unwrap();
        let outcome = apply_todo(&fresh, &child_payload).unwrap();
        assert_eq!(outcome, ApplyOutcome::Deferred);
        let n: i64 = fresh.query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "Deferred 不得写入");

        // 父来了之后再应用子：正常插入并接上 parent_id
        let parent_payload: Value = serde_json::from_str(&todo_payload(&conn, &parent)).unwrap();
        repo::apply_begin(&fresh).unwrap();
        assert_eq!(apply_todo(&fresh, &parent_payload).unwrap(), ApplyOutcome::Inserted);
        assert_eq!(apply_todo(&fresh, &child_payload).unwrap(), ApplyOutcome::Inserted);
        repo::apply_end(&fresh).unwrap();
        let linked: Option<i64> = fresh
            .query_row("SELECT t.parent_id FROM todos t JOIN todos p ON p.id = t.parent_id WHERE t.title = '子'", [], |r| r.get(0))
            .ok();
        assert!(linked.is_some(), "子待办必须挂到远端父行上");
    }

    #[test]
    fn note_payload_apply_roundtrip_with_folder() {
        let conn = db::init_in_memory().unwrap();
        let folder = crate::repo::note_folder::create(&conn, "工作/项目", None).map(|f| f.id).ok();
        let note = crate::repo::note::create_in(&conn, "标题", "正文", folder, "").unwrap();
        repo::ensure_uids(&conn).unwrap();
        let payload: Value = serde_json::from_str(&note_payload(&conn, &note)).unwrap();
        assert_eq!(payload["fields"]["folder"], "工作/项目");

        let fresh = db::init_in_memory().unwrap();
        repo::apply_begin(&fresh).unwrap();
        assert_eq!(apply_note(&fresh, &payload).unwrap(), ApplyOutcome::Inserted);
        repo::apply_end(&fresh).unwrap();
        let got = crate::repo::note::list(&fresh).unwrap().remove(0);
        assert_eq!(got.content, "正文");
        assert!(got.folder_id.is_some(), "远端文件夹路径必须在本端落成真实文件夹");
        let fname: String = fresh
            .query_row("SELECT name FROM note_folders WHERE id = ?1", params![got.folder_id.unwrap()], |r| r.get(0))
            .unwrap();
        assert_eq!(fname, "项目");
    }
}
