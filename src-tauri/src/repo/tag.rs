use crate::models::Tag;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 笔记标签（与待办标签是两套独立定义，ADR 0010）。
/// 速记改造（docs/speednote-plan.md）补上改名能力并保护内置标签——
/// 修缺陷③「标签不能改名、不能删除」：改名在 repo 层完成，归属关系（note_tags）不动；
/// 删除只解关联（note_tags 级联清掉），笔记本身不受影响。

const TAG_COLS: &str = "id, name, created_at, builtin";

pub fn list(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TAG_COLS} FROM tags ORDER BY created_at ASC, id ASC"
    ))?;
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
        &format!("SELECT {TAG_COLS} FROM tags WHERE name = ?1"),
        params![name],
        row_to_tag,
    )
}

/// 改名（note_tags 归属随 id 保持不变）。重名拒绝（TAG_EXISTS: 前缀供前端出友好提示），
/// 内置标签（「剪藏」）拒绝（FORBIDDEN_BUILTIN: 前缀）。
pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<()> {
    ensure_not_builtin(conn, id)?;
    let taken: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tags WHERE name = ?1 AND id <> ?2",
        params![name, id],
        |r| r.get(0),
    )?;
    if taken > 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "TAG_EXISTS: 标签「{name}」已存在"
        )));
    }
    let affected = conn.execute("UPDATE tags SET name = ?1 WHERE id = ?2", params![name, id])?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 标签 {id} 不存在"
        )));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    ensure_not_builtin(conn, id)?;
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

fn ensure_not_builtin(conn: &Connection, id: i64) -> Result<()> {
    let builtin: i64 = conn.query_row(
        "SELECT builtin FROM tags WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if builtin != 0 {
        return Err(rusqlite::Error::InvalidParameterName(
            "FORBIDDEN_BUILTIN: 内置标签不可改名或删除".into(),
        ));
    }
    Ok(())
}

/// 查询笔记的标签列表
pub fn tags_of_note(conn: &Connection, note_id: i64) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.name, t.created_at, t.builtin FROM tags t
         JOIN note_tags nt ON nt.tag_id = t.id
         WHERE nt.note_id = ?1 ORDER BY t.created_at ASC, t.id ASC"
    ))?;
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
        builtin: row.get::<_, i64>(3)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn rename_keeps_links_and_rejects_duplicates() {
        let conn = init_in_memory().unwrap();
        let t = create(&conn, "工作").unwrap();
        let n = crate::repo::note::create(&conn, "N").unwrap();
        set_note_tags(&conn, n.id, &[t.id]).unwrap();
        rename(&conn, t.id, "重要").unwrap();
        let tags = tags_of_note(&conn, n.id).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "重要");
        // 重名拒绝
        let other = create(&conn, "生活").unwrap();
        let err = rename(&conn, other.id, "重要").unwrap_err().to_string();
        assert!(err.contains("TAG_EXISTS"), "err: {err}");
        // 改成自己的名字（无冲突）通过
        rename(&conn, other.id, "生活").unwrap();
    }

    #[test]
    fn builtin_tag_protected() {
        let conn = init_in_memory().unwrap();
        let builtin: i64 = conn
            .query_row("SELECT id FROM tags WHERE builtin = 1", [], |r| r.get(0))
            .unwrap();
        let err = rename(&conn, builtin, "改名").unwrap_err().to_string();
        assert!(err.contains("FORBIDDEN_BUILTIN"), "err: {err}");
        assert!(delete(&conn, builtin).is_err());
    }

    #[test]
    fn delete_unlinks_but_keeps_notes() {
        let conn = init_in_memory().unwrap();
        let t = create(&conn, "工作").unwrap();
        let n = crate::repo::note::create(&conn, "N").unwrap();
        set_note_tags(&conn, n.id, &[t.id]).unwrap();
        delete(&conn, t.id).unwrap();
        assert!(tags_of_note(&conn, n.id).unwrap().is_empty());
        assert!(crate::repo::note::get(&conn, n.id).is_ok());
        let links: i64 = conn
            .query_row("SELECT COUNT(*) FROM note_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(links, 0, "删除标签只解关联");
    }
}
