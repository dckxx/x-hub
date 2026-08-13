use crate::models::DetachedSticky;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn list(conn: &Connection) -> Result<Vec<DetachedSticky>> {
    let mut stmt = conn.prepare(
        "SELECT id, slot, content, x, y, always_on_top, created_at, updated_at
         FROM detached_stickies ORDER BY slot ASC",
    )?;
    let rows = stmt.query_map([], row_to_detached)?;
    rows.collect()
}

pub fn get_by_slot(conn: &Connection, slot: i64) -> Result<Option<DetachedSticky>> {
    let mut stmt = conn.prepare(
        "SELECT id, slot, content, x, y, always_on_top, created_at, updated_at
         FROM detached_stickies WHERE slot = ?1",
    )?;
    let mut rows = stmt.query_map(params![slot], row_to_detached)?;
    rows.next().transpose()
}

pub fn upsert(
    conn: &Connection,
    slot: i64,
    content: &str,
    x: Option<f64>,
    y: Option<f64>,
    always_on_top: bool,
) -> Result<DetachedSticky> {
    conn.execute(
        "INSERT INTO detached_stickies (slot, content, x, y, always_on_top, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(slot) DO UPDATE SET
           content = excluded.content,
           x = excluded.x,
           y = excluded.y,
           always_on_top = excluded.always_on_top,
           updated_at = excluded.updated_at",
        params![slot, content, x, y, always_on_top, now()],
    )?;
    get_by_slot(conn, slot)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn update_content(conn: &Connection, slot: i64, content: &str) -> Result<()> {
    conn.execute(
        "UPDATE detached_stickies SET content = ?1, updated_at = ?2 WHERE slot = ?3",
        params![content, now(), slot],
    )?;
    Ok(())
}

pub fn update_position(conn: &Connection, slot: i64, x: f64, y: f64) -> Result<()> {
    conn.execute(
        "UPDATE detached_stickies SET x = ?1, y = ?2, updated_at = ?3 WHERE slot = ?4",
        params![x, y, now(), slot],
    )?;
    Ok(())
}

pub fn update_pin(conn: &Connection, slot: i64, always_on_top: bool) -> Result<()> {
    conn.execute(
        "UPDATE detached_stickies SET always_on_top = ?1, updated_at = ?2 WHERE slot = ?3",
        params![always_on_top, now(), slot],
    )?;
    Ok(())
}

pub fn delete_by_slot(conn: &Connection, slot: i64) -> Result<()> {
    conn.execute("DELETE FROM detached_stickies WHERE slot = ?1", params![slot])?;
    Ok(())
}

pub fn row_to_detached(row: &rusqlite::Row) -> Result<DetachedSticky> {
    Ok(DetachedSticky {
        id: row.get(0)?,
        slot: row.get(1)?,
        content: row.get(2)?,
        x: row.get(3)?,
        y: row.get(4)?,
        always_on_top: row.get(5)?,
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
    fn upsert_creates_then_updates_same_slot() {
        let conn = setup();
        let created = upsert(&conn, 1, "浮窗内容", Some(10.0), Some(20.0), true).unwrap();
        assert_eq!(created.slot, 1);
        assert_eq!(created.content, "浮窗内容");
        assert_eq!(created.x, Some(10.0));
        assert!(created.always_on_top);

        let updated = upsert(&conn, 1, "改过了", Some(30.0), Some(40.0), false).unwrap();
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.content, "改过了");
        assert_eq!(updated.x, Some(30.0));
        assert!(!updated.always_on_top);
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn slots_are_independent_and_unique() {
        let conn = setup();
        upsert(&conn, 1, "卡一浮窗", None, None, true).unwrap();
        upsert(&conn, 2, "卡二浮窗", None, None, true).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 2);
        assert!(get_by_slot(&conn, 1).unwrap().is_some());
        assert!(get_by_slot(&conn, 2).unwrap().is_some());
    }

    #[test]
    fn update_and_delete_work() {
        let conn = setup();
        upsert(&conn, 2, "原始", None, None, true).unwrap();
        update_content(&conn, 2, "新内容").unwrap();
        update_position(&conn, 2, 100.0, 200.0).unwrap();
        update_pin(&conn, 2, false).unwrap();
        let s = get_by_slot(&conn, 2).unwrap().unwrap();
        assert_eq!(s.content, "新内容");
        assert_eq!(s.x, Some(100.0));
        assert!(!s.always_on_top);

        delete_by_slot(&conn, 2).unwrap();
        assert!(get_by_slot(&conn, 2).unwrap().is_none());
    }
}
