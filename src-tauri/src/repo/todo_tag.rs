use crate::models::TodoTag;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const COLS: &str = "id, name, color, sort_order, created_at";

/// 待办标签定义列表（按手动排序位升序，未排序按创建时间）
pub fn list(conn: &Connection) -> Result<Vec<TodoTag>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todo_tags ORDER BY sort_order ASC, created_at ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_tag)?;
    rows.collect()
}

/// 创建待办标签（同名已存在则直接返回已有标签；与笔记标签 tags 无关，两套独立定义）
pub fn create(conn: &Connection, name: &str, color: &str) -> Result<TodoTag> {
    conn.execute(
        "INSERT OR IGNORE INTO todo_tags (name, color, created_at) VALUES (?1, ?2, ?3)",
        params![name, color, now()],
    )?;
    conn.query_row(
        &format!("SELECT {COLS} FROM todo_tags WHERE name = ?1"),
        params![name],
        row_to_tag,
    )
}

pub fn update(conn: &Connection, id: i64, name: &str, color: &str) -> Result<TodoTag> {
    conn.execute(
        "UPDATE todo_tags SET name = ?1, color = ?2 WHERE id = ?3",
        params![name, color, id],
    )?;
    conn.query_row(
        &format!("SELECT {COLS} FROM todo_tags WHERE id = ?1"),
        params![id],
        row_to_tag,
    )
}

/// 删除标签（关联行经外键 ON DELETE CASCADE 一并删除）
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM todo_tags WHERE id = ?1", params![id])?;
    Ok(())
}

/// 全量设置某条待办的标签（先清空再写入）
pub fn set_todo_tags(conn: &Connection, todo_id: i64, tag_ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM todo_tag_links WHERE todo_id = ?1", params![todo_id])?;
    for tag_id in tag_ids {
        tx.execute(
            "INSERT OR IGNORE INTO todo_tag_links (todo_id, tag_id) VALUES (?1, ?2)",
            params![todo_id, tag_id],
        )?;
    }
    tx.commit()
}

/// 待办-标签全量关联（前端构建筛选映射用）
pub fn list_links(conn: &Connection) -> Result<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare("SELECT todo_id, tag_id FROM todo_tag_links")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    rows.collect()
}

fn row_to_tag(row: &rusqlite::Row) -> Result<TodoTag> {
    Ok(TodoTag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::repo::todo;

    #[test]
    fn create_is_idempotent_by_name() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "工作", "#5b5bf5").unwrap();
        let b = create(&conn, "工作", "#000000").unwrap();
        assert_eq!(a.id, b.id);
        // 同名不覆盖既有颜色
        assert_eq!(b.color, "#5b5bf5");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn set_todo_tags_replaces_all() {
        let conn = init_in_memory().unwrap();
        let t = todo::create(&conn, "写周报", None, None).unwrap();
        let work = create(&conn, "工作", "").unwrap();
        let home = create(&conn, "生活", "").unwrap();

        set_todo_tags(&conn, t.id, &[work.id]).unwrap();
        assert_eq!(list_links(&conn).unwrap(), vec![(t.id, work.id)]);

        set_todo_tags(&conn, t.id, &[home.id]).unwrap();
        assert_eq!(list_links(&conn).unwrap(), vec![(t.id, home.id)]);

        set_todo_tags(&conn, t.id, &[]).unwrap();
        assert!(list_links(&conn).unwrap().is_empty());
    }

    #[test]
    fn delete_tag_cascades_links() {
        let conn = init_in_memory().unwrap();
        let t = todo::create(&conn, "写周报", None, None).unwrap();
        let work = create(&conn, "工作", "").unwrap();
        set_todo_tags(&conn, t.id, &[work.id]).unwrap();
        delete(&conn, work.id).unwrap();
        assert!(list_links(&conn).unwrap().is_empty());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn delete_todo_cascades_links() {
        let conn = init_in_memory().unwrap();
        let t = todo::create(&conn, "写周报", None, None).unwrap();
        let work = create(&conn, "工作", "").unwrap();
        set_todo_tags(&conn, t.id, &[work.id]).unwrap();
        todo::delete(&conn, t.id).unwrap();
        assert!(list_links(&conn).unwrap().is_empty());
        // 标签定义本身保留
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn todo_tags_are_independent_from_note_tags() {
        // 两套独立定义：笔记标签表里同名标签不影响待办标签
        let conn = init_in_memory().unwrap();
        conn.execute("INSERT INTO tags (name) VALUES ('工作')", []).unwrap();
        let t = create(&conn, "工作", "#123456").unwrap();
        assert_eq!(t.color, "#123456");
        // 只数用户标签：tags 表含内置种子（「剪藏」，速记改造引入），不参与独立断言
        let note_tags: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE builtin = 0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(note_tags, 1);
    }
}
