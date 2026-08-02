use crate::models::Tag;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn list(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at FROM tags ORDER BY created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_tag)?;
    rows.collect()
}

/// 创建标签（同名已存在则直接返回已有标签）
pub fn create(conn: &Connection, name: &str) -> Result<Tag> {
    conn.execute(
        "INSERT OR IGNORE INTO tags (name, created_at) VALUES (?1, ?2)",
        params![name, now()],
    )?;
    conn.query_row(
        "SELECT id, name, created_at FROM tags WHERE name = ?1",
        params![name],
        row_to_tag,
    )
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

/// 查询笔记的标签列表
pub fn tags_of_note(conn: &Connection, note_id: i64) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.created_at FROM tags t
         JOIN note_tags nt ON nt.tag_id = t.id
         WHERE nt.note_id = ?1 ORDER BY t.created_at ASC, t.id ASC",
    )?;
    let rows = stmt.query_map(params![note_id], row_to_tag)?;
    rows.collect()
}

/// 全量设置笔记标签（先清空再写入）
pub fn set_note_tags(conn: &Connection, note_id: i64, tag_ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM note_tags WHERE note_id = ?1", params![note_id])?;
    for tag_id in tag_ids {
        tx.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note_id, tag_id],
        )?;
    }
    tx.commit()
}

/// 笔记-标签全量关联（前端构建筛选映射用）
pub fn list_note_tags(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare("SELECT note_id, tag_id FROM note_tags")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    rows.collect()
}

fn row_to_tag(row: &rusqlite::Row) -> Result<Tag> {
    Ok(Tag {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
    })
}
