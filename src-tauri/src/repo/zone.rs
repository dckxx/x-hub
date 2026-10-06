//! 速达分区：「全部」tab 的自定义成组陈列（独立于小类的分组维度，见 ADR 0012 的对照）。
//! 跨大类混居、无 kind 维度、无层级；成员经 `resources.zone_id` 关联（NULL = 未分区）。
//! 分区与小类是两个正交维度：小类（category）服务于应用/网页/文件 tab 的筛选 chips，
//! 分区只管「全部」tab 的陈列分组；一个分区都没有时「全部」保持平铺。

use crate::models::ResourceZone;
use rusqlite::{params, Connection};

use super::now;

fn row_to_zone(row: &rusqlite::Row) -> rusqlite::Result<ResourceZone> {
    Ok(ResourceZone {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_order: row.get(2)?,
        cols: row.get(3)?,
        rows: row.get(4)?,
    })
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<ResourceZone>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, sort_order, cols, rows FROM resource_zones ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_zone)?;
    rows.collect()
}

/// 分区名是否可用（全局唯一；rename 时排除自身）
pub fn is_name_free(
    conn: &Connection,
    name: &str,
    exclude_id: Option<i64>,
) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_zones WHERE name = ?1 AND id != ?2",
        params![name, exclude_id.unwrap_or(-1)],
        |r| r.get(0),
    )?;
    Ok(n == 0)
}

/// 新建分区：落到分区序列尾部（sort_order = max + 1），「未分区」恒在成员陈列的最后一块；
/// 尺寸默认 3×2（宽度 3 卡格 × 高度 2 卡行，均为下限）
pub fn create(conn: &Connection, name: &str) -> rusqlite::Result<ResourceZone> {
    let max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) FROM resource_zones",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO resource_zones (name, sort_order, cols, rows) VALUES (?1, ?2, 3, 2)",
        params![name, max + 1],
    )?;
    Ok(ResourceZone {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
        sort_order: max + 1,
        cols: 3,
        rows: 2,
    })
}

/// 改名：资源按 id 关联，无需级联（与小类按名字符串关联不同）
pub fn rename(conn: &mut Connection, id: i64, name: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let exists: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM resource_zones WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err(format!("NOT_FOUND: 分区 {id} 不存在"));
    }
    let free = is_name_free(&tx, name, Some(id)).map_err(|e| e.to_string())?;
    if !free {
        return Err(format!("DUP: 已有名为「{name}」的分区"));
    }
    tx.execute(
        "UPDATE resource_zones SET name = ?1 WHERE id = ?2",
        params![name, id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// 删除分区：成员批量落「未分区」（zone_id 置 NULL，资源本身不动），单事务完成
pub fn delete(conn: &mut Connection, id: i64) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let exists: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM resource_zones WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err(format!("NOT_FOUND: 分区 {id} 不存在"));
    }
    tx.execute(
        "UPDATE resources SET zone_id = NULL, updated_at = ?1 WHERE zone_id = ?2",
        params![now(), id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM resource_zones WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// 分区排序：按传入 id 顺序写 sort_order（整组重写，单事务——中途失败不留半态）
pub fn reorder(conn: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (idx, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE resource_zones SET sort_order = ?1 WHERE id = ?2",
            params![idx as i64, id],
        )?;
    }
    tx.commit()
}

/// 调整分区框尺寸（卡片格数；前端拖拽缩放已按格吸附，这里只做存在性与范围兜底）
pub fn resize(conn: &Connection, id: i64, cols: i64, rows: i64) -> Result<(), String> {
    if !exists(conn, id).map_err(|e| e.to_string())? {
        return Err(format!("NOT_FOUND: 分区 {id} 不存在"));
    }
    conn.execute(
        "UPDATE resource_zones SET cols = ?1, rows = ?2 WHERE id = ?3",
        params![cols, rows, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn exists(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_zones WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::models::ResourceKind;
    use crate::repo::resource;

    #[test]
    fn create_appends_to_tail_and_lists_ordered() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "办公区").unwrap();
        let b = create(&conn, "影音区").unwrap();
        assert_eq!(a.sort_order, 0);
        assert_eq!(b.sort_order, 1);
        let zones = list(&conn).unwrap();
        assert_eq!(
            zones.iter().map(|z| z.name.clone()).collect::<Vec<_>>(),
            vec!["办公区", "影音区"]
        );
    }

    #[test]
    fn rename_keeps_membership_and_rejects_duplicate() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "办公区").unwrap();
        create(&conn, "影音区").unwrap();
        let r = resource::create(&conn, ResourceKind::App, "x", "t", None, None, None, None, None, None, None).unwrap();
        resource::set_zone(&conn, &[r.id], Some(a.id)).unwrap();
        let mut conn = conn;
        rename(&mut conn, a.id, "工作区").unwrap();
        assert_eq!(resource::get(&conn, r.id).unwrap().zone_id, Some(a.id));
        // 改成与另一个分区重名 → DUP
        assert!(rename(&mut conn, a.id, "影音区").is_err());
    }

    #[test]
    fn delete_drops_members_to_unzoned_in_one_tx() {
        let conn = init_in_memory().unwrap();
        let z = create(&conn, "办公区").unwrap();
        let r1 = resource::create(&conn, ResourceKind::App, "a", "t", None, None, None, Some(z.id), None, None, None).unwrap();
        let r2 = resource::create(&conn, ResourceKind::Web, "b", "u", None, None, None, Some(z.id), None, None, None).unwrap();
        let mut conn = conn;
        delete(&mut conn, z.id).unwrap();
        assert_eq!(resource::get(&conn, r1.id).unwrap().zone_id, None);
        assert_eq!(resource::get(&conn, r2.id).unwrap().zone_id, None);
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn reorder_zones() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        let zones = list(&conn).unwrap();
        assert_eq!(zones[0].name, "B");
        assert_eq!(zones[1].name, "A");
    }

    #[test]
    fn create_defaults_3x2_and_resize_persists() {
        let conn = init_in_memory().unwrap();
        let z = create(&conn, "办公区").unwrap();
        assert_eq!((z.cols, z.rows), (3, 2));
        resize(&conn, z.id, 5, 1).unwrap();
        let after = list(&conn).unwrap().into_iter().find(|x| x.id == z.id).unwrap();
        assert_eq!((after.cols, after.rows), (5, 1));
        assert!(resize(&conn, 9999, 3, 3).is_err());
    }

    #[test]
    fn name_free_excludes_self() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "办公区").unwrap();
        assert!(!is_name_free(&conn, "办公区", None).unwrap());
        assert!(is_name_free(&conn, "办公区", Some(a.id)).unwrap());
        assert!(is_name_free(&conn, "别的", None).unwrap());
    }
}
