use crate::models::Todo;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn list(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, done, priority, created_at, updated_at, completed_at FROM todos
         ORDER BY done ASC,
           CASE WHEN done = 1 THEN completed_at ELSE created_at END DESC,
           id DESC",
    )?;
    let rows = stmt.query_map([], row_to_todo)?;
    rows.collect()
}

pub fn create(conn: &Connection, title: &str) -> Result<Todo> {
    let ts = now();
    conn.execute(
        "INSERT INTO todos (title, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![title, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Todo> {
    conn.query_row(
        "SELECT id, title, done, priority, created_at, updated_at, completed_at FROM todos WHERE id = ?1",
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

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, done, priority, created_at, updated_at, completed_at FROM todos
         WHERE title LIKE '%' || ?1 || '%' ORDER BY done ASC, created_at DESC LIMIT 20",
    )?;
    let rows = stmt.query_map(params![keyword], row_to_todo)?;
    rows.collect()
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
        let t = create(&conn, "写周报").unwrap();
        assert_eq!(t.title, "写周报");
        assert!(!t.done);
        assert_eq!(t.priority, 0);
        assert_eq!(t.completed_at, None);
        assert!(!t.created_at.is_empty());
    }

    #[test]
    fn list_returns_all() {
        let conn = setup();
        create(&conn, "任务 A").unwrap();
        create(&conn, "任务 B").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        let titles: Vec<&str> = list.iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"任务 A"));
        assert!(titles.contains(&"任务 B"));
    }

    #[test]
    fn toggle_marks_done_then_undone() {
        let conn = setup();
        let t = create(&conn, "洗衣服").unwrap();
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
        let t = create(&conn, "旧标题").unwrap();
        let updated = update(&conn, t.id, "新标题", 2).unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.priority, 2);
    }

    #[test]
    fn delete_removes_todo() {
        let conn = setup();
        let t = create(&conn, "临时任务").unwrap();
        delete(&conn, t.id).unwrap();
        assert!(get(&conn, t.id).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn search_matches_substring_and_percent() {
        let conn = setup();
        create(&conn, "买牛奶").unwrap();
        create(&conn, "进度 50%").unwrap();
        let found = search(&conn, "牛奶").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "买牛奶");
        // LIKE 中 % 是通配符，无需转义即可命中含 % 的标题
        let pct = search(&conn, "50%").unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(pct[0].title, "进度 50%");
    }
}
