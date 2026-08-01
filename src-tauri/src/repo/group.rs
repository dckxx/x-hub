use crate::models::Group;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(conn: &Connection, name: &str) -> Result<Group> {
    let ts = now();
    conn.execute(
        "INSERT INTO groups (name, sort_order, created_at, updated_at) VALUES (?1, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM groups), ?2, ?2)",
        params![name, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Group> {
    conn.query_row(
        "SELECT id, name, sort_order, created_at, updated_at FROM groups WHERE id = ?1",
        params![id],
        row_to_group,
    )
}

pub fn list(conn: &Connection) -> Result<Vec<Group>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, sort_order, created_at, updated_at FROM groups ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_group)?;
    rows.collect()
}

pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<Group> {
    conn.execute(
        "UPDATE groups SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (order, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE groups SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![order as i64, ts, id],
        )?;
    }
    tx.commit()
}

pub fn row_to_group(row: &rusqlite::Row) -> Result<Group> {
    Ok(Group {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_get_group() {
        let conn = init_in_memory().unwrap();
        let g = create(&conn, "开发工具").unwrap();
        assert_eq!(g.name, "开发工具");
        assert_eq!(g.sort_order, 1);
        let fetched = get(&conn, g.id).unwrap();
        assert_eq!(fetched.id, g.id);
    }

    #[test]
    fn list_groups_ordered_by_sort_order() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        let c = create(&conn, "C").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|g| g.id).collect::<Vec<_>>(), vec![a.id, b.id, c.id]);
    }

    #[test]
    fn reorder_groups() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|g| g.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn rename_group() {
        let conn = init_in_memory().unwrap();
        let g = create(&conn, "Old").unwrap();
        let renamed = rename(&conn, g.id, "New").unwrap();
        assert_eq!(renamed.name, "New");
    }

    #[test]
    fn delete_group() {
        let conn = init_in_memory().unwrap();
        let g = create(&conn, "Temp").unwrap();
        delete(&conn, g.id).unwrap();
        assert!(get(&conn, g.id).is_err());
    }
}
