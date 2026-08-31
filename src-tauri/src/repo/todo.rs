use crate::models::Todo;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const COLS: &str =
    "id, title, done, priority, created_at, updated_at, completed_at, due_at, remind_at, remind_fired, parent_id";

pub fn list(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         ORDER BY done ASC,
           CASE WHEN done = 1 THEN completed_at ELSE created_at END DESC,
           id DESC",
    ))?;
    let rows = stmt.query_map([], row_to_todo)?;
    rows.collect()
}

/// 创建待办；parent_id 非空时创建为该父条目下的子待办（父不存在报错）；
/// created_at 非空时保留原时间戳（删除撤销恢复用，避免恢复项排到最新位置）
pub fn create(
    conn: &Connection,
    title: &str,
    parent_id: Option<i64>,
    created_at: Option<&str>,
) -> Result<Todo> {
    if let Some(pid) = parent_id {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE id = ?1",
            params![pid],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "父待办不存在: {pid}"
            )));
        }
    }
    let ts = created_at
        .map(str::to_owned)
        .unwrap_or_else(now);
    conn.execute(
        "INSERT INTO todos (title, parent_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![title, parent_id, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Todo> {
    conn.query_row(
        &format!("SELECT {COLS} FROM todos WHERE id = ?1"),
        params![id],
        row_to_todo,
    )
}

pub fn update(conn: &Connection, id: i64, title: &str, priority: i64) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET title = ?1, priority = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, priority, now(), id],
    )?;
    get(conn, id)
}

/// 设置截止/提醒时刻（毫秒时间戳；NULL 表示清除）。
/// 每次排期都重置 remind_fired，用户改提醒时间后重新武装后台触发。
pub fn schedule(
    conn: &Connection,
    id: i64,
    due_at: Option<i64>,
    remind_at: Option<i64>,
) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET due_at = ?1, remind_at = ?2, remind_fired = 0, updated_at = ?3 WHERE id = ?4",
        params![due_at, remind_at, now(), id],
    )?;
    get(conn, id)
}

pub fn toggle(conn: &Connection, id: i64) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET
           done = CASE WHEN done = 1 THEN 0 ELSE 1 END,
           completed_at = CASE WHEN done = 1 THEN NULL ELSE strftime('%Y-%m-%d %H:%M:%f','now') END,
           updated_at = ?1
         WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

/// 删除待办。子待办经外键 ON DELETE CASCADE 一并删除。
pub fn delete(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let kids = children_ids(conn, id)?;
    conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
    Ok(kids)
}

/// 直接子待办 id 列表（仅一层，无嵌套子待办）
pub fn children_ids(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM todos WHERE parent_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![id], |r| r.get(0))?;
    rows.collect()
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         WHERE title LIKE '%' || ?1 || '%' ORDER BY done ASC, created_at DESC LIMIT 20",
    ))?;
    let rows = stmt.query_map(params![keyword], row_to_todo)?;
    rows.collect()
}

/// 到期待提醒的待办（未完成、未触发过、提醒时刻已到）。
/// 提醒只针对未完成项：完成后由 done 过滤，无需清 remind_at。
pub fn list_due_reminders(conn: &Connection, now_ms: i64) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         WHERE done = 0 AND remind_fired = 0 AND remind_at IS NOT NULL AND remind_at <= ?1
         ORDER BY remind_at ASC",
    ))?;
    let rows = stmt.query_map(params![now_ms], row_to_todo)?;
    rows.collect()
}

pub fn mark_remind_fired(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE todos SET remind_fired = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn row_to_todo(row: &rusqlite::Row) -> Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        done: row.get(2)?,
        priority: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        completed_at: row.get(6)?,
        due_at: row.get(7)?,
        remind_at: row.get(8)?,
        remind_fired: row.get::<_, i64>(9)? != 0,
        parent_id: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn setup() -> Connection {
        init_in_memory().unwrap()
    }

    #[test]
    fn create_sets_defaults() {
        let conn = setup();
        let t = create(&conn, "写周报", None, None).unwrap();
        assert_eq!(t.title, "写周报");
        assert!(!t.done);
        assert_eq!(t.priority, 0);
        assert_eq!(t.completed_at, None);
        assert_eq!(t.due_at, None);
        assert_eq!(t.remind_at, None);
        assert!(!t.remind_fired);
        assert_eq!(t.parent_id, None);
        assert!(!t.created_at.is_empty());
    }

    #[test]
    fn create_preserves_provided_created_at() {
        // 删除撤销恢复：保留原 created_at，避免恢复项按新时间排到列表最新位置
        let conn = setup();
        let t = create(&conn, "恢复的待办", None, Some("2026-01-02 03:04:05.123456")).unwrap();
        assert_eq!(t.created_at, "2026-01-02 03:04:05.123456");
    }

    #[test]
    fn list_returns_all() {
        let conn = setup();
        create(&conn, "任务 A", None, None).unwrap();
        create(&conn, "任务 B", None, None).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        let titles: Vec<&str> = list.iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"任务 A"));
        assert!(titles.contains(&"任务 B"));
    }

    #[test]
    fn toggle_marks_done_then_undone() {
        let conn = setup();
        let t = create(&conn, "洗衣服", None, None).unwrap();
        let done = toggle(&conn, t.id).unwrap();
        assert!(done.done);
        assert!(done.completed_at.is_some());
        let undone = toggle(&conn, t.id).unwrap();
        assert!(!undone.done);
        assert_eq!(undone.completed_at, None);
    }

    #[test]
    fn update_changes_title_and_priority() {
        let conn = setup();
        let t = create(&conn, "旧标题", None, None).unwrap();
        let updated = update(&conn, t.id, "新标题", 2).unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.priority, 2);
    }

    #[test]
    fn delete_removes_todo() {
        let conn = setup();
        let t = create(&conn, "临时任务", None, None).unwrap();
        delete(&conn, t.id).unwrap();
        assert!(get(&conn, t.id).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn search_matches_substring_and_percent() {
        let conn = setup();
        create(&conn, "买牛奶", None, None).unwrap();
        create(&conn, "进度 50%", None, None).unwrap();
        let found = search(&conn, "牛奶").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "买牛奶");
        // LIKE 中 % 是通配符，无需转义即可命中含 % 的标题
        let pct = search(&conn, "50%").unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(pct[0].title, "进度 50%");
    }

    #[test]
    fn sub_todo_links_to_parent() {
        let conn = setup();
        let parent = create(&conn, "准备 PPT", None, None).unwrap();
        let sub = create(&conn, "完成初稿", Some(parent.id), None).unwrap();
        assert_eq!(sub.parent_id, Some(parent.id));
        // 重复父创建：父不存在时报错
        assert!(create(&conn, "孤儿", Some(99999), None).is_err());
    }

    #[test]
    fn delete_parent_cascades_children() {
        let conn = setup();
        let parent = create(&conn, "父待办", None, None).unwrap();
        let s1 = create(&conn, "子一", Some(parent.id), None).unwrap();
        let s2 = create(&conn, "子二", Some(parent.id), None).unwrap();
        let kids = delete(&conn, parent.id).unwrap();
        assert_eq!(kids, vec![s1.id, s2.id]);
        assert!(get(&conn, parent.id).is_err());
        assert!(get(&conn, s1.id).is_err());
        assert!(get(&conn, s2.id).is_err());
    }

    #[test]
    fn schedule_sets_and_clears_due_and_remind() {
        let conn = setup();
        let t = create(&conn, "交房租", None, None).unwrap();
        let due = 1_800_000_000_000;
        let remind = due - 30 * 60_000;
        let s = schedule(&conn, t.id, Some(due), Some(remind)).unwrap();
        assert_eq!(s.due_at, Some(due));
        assert_eq!(s.remind_at, Some(remind));
        assert!(!s.remind_fired);
        // 清除
        let cleared = schedule(&conn, t.id, None, None).unwrap();
        assert_eq!(cleared.due_at, None);
        assert_eq!(cleared.remind_at, None);
    }

    #[test]
    fn due_reminders_skip_fired_and_done() {
        let conn = setup();
        let now = 1_800_000_000_000;
        let a = create(&conn, "到期未触发", None, None).unwrap();
        schedule(&conn, a.id, Some(now + 1), Some(now - 60_000)).unwrap();
        let b = create(&conn, "已触发", None, None).unwrap();
        schedule(&conn, b.id, None, Some(now - 60_000)).unwrap();
        mark_remind_fired(&conn, b.id).unwrap();
        let c = create(&conn, "已完成", None, None).unwrap();
        schedule(&conn, c.id, None, Some(now - 60_000)).unwrap();
        toggle(&conn, c.id).unwrap();
        let d = create(&conn, "未到点", None, None).unwrap();
        schedule(&conn, d.id, None, Some(now + 60_000)).unwrap();

        let due = list_due_reminders(&conn, now).unwrap();
        let titles: Vec<&str> = due.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["到期未触发"]);
    }
}
