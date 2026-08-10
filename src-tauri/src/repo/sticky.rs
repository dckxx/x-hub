use crate::models::Sticky;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn list(conn: &Connection) -> Result<Vec<Sticky>> {
    let mut stmt = conn.prepare(
        "SELECT id, slot, content, created_at, updated_at FROM stickies ORDER BY slot ASC",
    )?;
    let rows = stmt.query_map([], row_to_sticky)?;
    rows.collect()
}

pub fn get_by_slot(conn: &Connection, slot: i64) -> Result<Option<Sticky>> {
    let mut stmt = conn.prepare(
        "SELECT id, slot, content, created_at, updated_at FROM stickies WHERE slot = ?1",
    )?;
    let mut rows = stmt.query_map(params![slot], row_to_sticky)?;
    rows.next().transpose()
}

pub fn upsert(conn: &Connection, slot: i64, content: &str) -> Result<Sticky> {
    conn.execute(
        "INSERT INTO stickies (slot, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(slot) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
        params![slot, content, now()],
    )?;
    get_by_slot(conn, slot)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn row_to_sticky(row: &rusqlite::Row) -> Result<Sticky> {
    Ok(Sticky {
        id: row.get(0)?,
        slot: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
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
    fn upsert_inserts_then_updates_same_slot() {
        let conn = setup();
        let first = upsert(&conn, 1, "第一条便签").unwrap();
        assert_eq!(first.slot, 1);
        assert_eq!(first.content, "第一条便签");
        let updated = upsert(&conn, 1, "改过了").unwrap();
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.content, "改过了");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn slots_are_independent() {
        let conn = setup();
        upsert(&conn, 1, "卡一").unwrap();
        upsert(&conn, 2, "卡二").unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(get_by_slot(&conn, 1).unwrap().unwrap().content, "卡一");
        assert_eq!(get_by_slot(&conn, 2).unwrap().unwrap().content, "卡二");
    }

    #[test]
    fn empty_list_on_fresh_db() {
        let conn = setup();
        assert!(list(&conn).unwrap().is_empty());
        assert!(get_by_slot(&conn, 1).unwrap().is_none());
    }
}
