use crate::models::Snippet;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 置顶优先，其次复制次数，再次最近复制时间，最后按 id 倒序保证唯一性
pub fn list(conn: &Connection) -> Result<Vec<Snippet>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, is_pinned, copy_count, last_copied_at, created_at, updated_at
         FROM snippets
         ORDER BY is_pinned DESC, copy_count DESC, last_copied_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_snippet)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: i64) -> Result<Snippet> {
    conn.query_row(
        "SELECT id, title, content, is_pinned, copy_count, last_copied_at, created_at, updated_at
         FROM snippets WHERE id = ?1",
        params![id],
        row_to_snippet,
    )
}

pub fn create(conn: &Connection, title: &str, content: &str) -> Result<Snippet> {
    let ts = now();
    conn.execute(
        "INSERT INTO snippets (title, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![title, content, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Snippet> {
    conn.execute(
        "UPDATE snippets SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn toggle_pin(conn: &Connection, id: i64) -> Result<Snippet> {
    conn.execute(
        "UPDATE snippets SET is_pinned = CASE WHEN is_pinned = 1 THEN 0 ELSE 1 END, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

pub fn record_copy(conn: &Connection, id: i64) -> Result<Snippet> {
    conn.execute(
        "UPDATE snippets SET copy_count = copy_count + 1, last_copied_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

pub fn row_to_snippet(row: &rusqlite::Row) -> Result<Snippet> {
    Ok(Snippet {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        is_pinned: row.get(3)?,
        copy_count: row.get(4)?,
        last_copied_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
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
        let s = create(&conn, "翻译助手", "把下面内容翻译成英文").unwrap();
        assert_eq!(s.title, "翻译助手");
        assert_eq!(s.content, "把下面内容翻译成英文");
        assert!(!s.is_pinned);
        assert_eq!(s.copy_count, 0);
        assert_eq!(s.last_copied_at, "");
        assert!(!s.created_at.is_empty());
        assert!(!s.updated_at.is_empty());
    }

    #[test]
    fn list_default_sorts_by_id_desc() {
        let conn = setup();
        let a = create(&conn, "A", "内容 A").unwrap();
        let b = create(&conn, "B", "内容 B").unwrap();
        let list = list(&conn).unwrap();
        // 未置顶且无复制记录时，按 id DESC
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn pinned_ranks_above_frequency() {
        let conn = setup();
        let frequent = create(&conn, "高频", "x").unwrap();
        record_copy(&conn, frequent.id).unwrap();
        record_copy(&conn, frequent.id).unwrap();
        let pinned = create(&conn, "置顶", "x").unwrap();
        toggle_pin(&conn, pinned.id).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list[0].id, pinned.id);
        assert!(list[0].is_pinned);
        assert_eq!(list[1].id, frequent.id);
    }

    #[test]
    fn list_orders_by_copy_count_desc() {
        let conn = setup();
        let a = create(&conn, "A", "x").unwrap();
        let b = create(&conn, "B", "x").unwrap();
        record_copy(&conn, a.id).unwrap();
        record_copy(&conn, a.id).unwrap();
        record_copy(&conn, b.id).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list[0].id, a.id);
        assert_eq!(list[0].copy_count, 2);
        assert_eq!(list[1].id, b.id);
        assert_eq!(list[1].copy_count, 1);
    }

    #[test]
    fn toggle_pin_flips_and_persists() {
        let conn = setup();
        let s = create(&conn, "T", "C").unwrap();
        assert!(!s.is_pinned);
        let pinned = toggle_pin(&conn, s.id).unwrap();
        assert!(pinned.is_pinned);
        let unpinned = toggle_pin(&conn, s.id).unwrap();
        assert!(!unpinned.is_pinned);
    }

    #[test]
    fn record_copy_updates_count_and_timestamp() {
        let conn = setup();
        let s = create(&conn, "T", "C").unwrap();
        let copied = record_copy(&conn, s.id).unwrap();
        assert_eq!(copied.copy_count, 1);
        assert!(!copied.last_copied_at.is_empty());
        let again = record_copy(&conn, s.id).unwrap();
        assert_eq!(again.copy_count, 2);
        // 时间戳单调不减（同秒内可能相等）
        assert!(again.last_copied_at >= copied.last_copied_at);
    }

    #[test]
    fn update_changes_title_and_content() {
        let conn = setup();
        let s = create(&conn, "旧标题", "旧内容").unwrap();
        let updated = update(&conn, s.id, "新标题", "新内容").unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "新内容");
    }

    #[test]
    fn delete_removes_snippet() {
        let conn = setup();
        let s = create(&conn, "临时", "x").unwrap();
        delete(&conn, s.id).unwrap();
        assert!(get(&conn, s.id).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }
}
