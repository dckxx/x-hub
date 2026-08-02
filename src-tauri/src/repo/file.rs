use crate::models::FileEntry;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const COLS: &str = "id, name, path, category, created_at, updated_at";

fn row_to_file(row: &rusqlite::Row) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        category: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<FileEntry>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM files ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], row_to_file)?;
    rows.collect()
}

pub fn create(conn: &Connection, name: &str, path: &str, category: &str) -> Result<FileEntry> {
    let ts = now();
    conn.execute(
        "INSERT INTO files (name, path, category, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![name, path, category, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, name: &str, category: &str) -> Result<FileEntry> {
    let ts = now();
    conn.execute(
        "UPDATE files SET name = ?1, category = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, category, ts, id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM files WHERE id = ?1", params![id])?;
    Ok(())
}

fn get(conn: &Connection, id: i64) -> Result<FileEntry> {
    conn.query_row(
        &format!("SELECT {COLS} FROM files WHERE id = ?1"),
        params![id],
        row_to_file,
    )
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<FileEntry>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM files WHERE name LIKE ?1 ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map(params![pattern], row_to_file)?;
    rows.collect()
}
