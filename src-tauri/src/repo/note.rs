use crate::models::Note;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(conn: &Connection, title: &str) -> Result<Note> {
    let ts = now();
    conn.execute(
        "INSERT INTO notes (title, created_at, updated_at) VALUES (?1, ?2, ?2)",
        params![title, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Note> {
    conn.query_row(
        "SELECT id, title, content, created_at, updated_at FROM notes WHERE id = ?1",
        params![id],
        row_to_note,
    )
}

pub fn list(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, created_at, updated_at FROM notes ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

pub fn update(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Note> {
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Note>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(
        "SELECT id, title, content, created_at, updated_at FROM notes WHERE title LIKE ?1 OR content LIKE ?1 ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_note)?;
    rows.collect()
}

pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_get_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "待办事项").unwrap();
        assert_eq!(n.title, "待办事项");
        assert_eq!(n.content, "");
    }

    #[test]
    fn list_notes_ordered_by_updated_desc() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        update(&conn, a.id, "A", "updated later").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_note_title_and_content() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        let updated = update(&conn, n.id, "新标题", "这是内容").unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "这是内容");
    }

    #[test]
    fn delete_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        delete(&conn, n.id).unwrap();
        assert!(get(&conn, n.id).is_err());
    }

    #[test]
    fn search_notes_by_title_and_content() {
        let conn = init_in_memory().unwrap();
        create(&conn, "购物清单").unwrap();
        let second = create(&conn, "会议记录").unwrap();
        update(&conn, second.id, "会议记录", "讨论了发布计划").unwrap();
        let by_title = search(&conn, "购物").unwrap();
        assert_eq!(by_title.len(), 1);
        let by_content = search(&conn, "发布计划").unwrap();
        assert_eq!(by_content.len(), 1);
        assert_eq!(by_content[0].id, second.id);
    }
}
