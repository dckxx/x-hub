use crate::models::{Resource, ResourceKind};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(
    conn: &Connection,
    group_id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    let ts = now();
    conn.execute(
        "INSERT INTO resources (group_id, kind, name, target, icon, args, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM resources WHERE group_id = ?1), ?7, ?7)",
        params![
            group_id,
            kind_to_str(&kind),
            name,
            target,
            icon,
            args,
            ts
        ],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Resource> {
    conn.query_row(
        "SELECT id, group_id, kind, name, target, icon, args, sort_order, created_at, updated_at FROM resources WHERE id = ?1",
        params![id],
        row_to_resource,
    )
}

pub fn list_by_group(conn: &Connection, group_id: i64) -> Result<Vec<Resource>> {
    let mut stmt = conn.prepare(
        "SELECT id, group_id, kind, name, target, icon, args, sort_order, created_at, updated_at FROM resources WHERE group_id = ?1 ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![group_id], row_to_resource)?;
    rows.collect()
}

pub fn list_all(conn: &Connection) -> Result<Vec<Resource>> {
    let mut stmt = conn.prepare(
        "SELECT id, group_id, kind, name, target, icon, args, sort_order, created_at, updated_at FROM resources ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_resource)?;
    rows.collect()
}

pub fn update(
    conn: &Connection,
    id: i64,
    group_id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    conn.execute(
        "UPDATE resources SET group_id = ?1, kind = ?2, name = ?3, target = ?4, icon = ?5, args = ?6, updated_at = ?7 WHERE id = ?8",
        params![group_id, kind_to_str(&kind), name, target, icon, args, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM resources WHERE id = ?1", params![id])?;
    Ok(())
}

/// 重新排列指定分组内的资源顺序。
/// ids 为目标分组内按新顺序排列的资源 id 列表。
pub fn reorder(conn: &Connection, group_id: i64, ids: &[i64]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (order, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE resources SET sort_order = ?1, group_id = ?2, updated_at = ?3 WHERE id = ?4",
            params![order as i64, group_id, ts, id],
        )?;
    }
    tx.commit()
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Resource>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(
        "SELECT id, group_id, kind, name, target, icon, args, sort_order, created_at, updated_at FROM resources WHERE name LIKE ?1 ORDER BY sort_order ASC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_resource)?;
    rows.collect()
}

pub fn kind_to_str(kind: &ResourceKind) -> &'static str {
    match kind {
        ResourceKind::App => "app",
        ResourceKind::Web => "web",
    }
}

pub fn row_to_resource(row: &rusqlite::Row) -> Result<Resource> {
    let kind: String = row.get(2)?;
    Ok(Resource {
        id: row.get(0)?,
        group_id: row.get(1)?,
        kind: match kind.as_str() {
            "app" => ResourceKind::App,
            _ => ResourceKind::Web,
        },
        name: row.get(3)?,
        target: row.get(4)?,
        icon: row.get(5)?,
        args: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::repo::group;

    fn setup() -> (Connection, i64) {
        let conn = init_in_memory().unwrap();
        let g = group::create(&conn, "开发").unwrap();
        (conn, g.id)
    }

    #[test]
    fn create_and_get_resource() {
        let (conn, gid) = setup();
        let r = create(&conn, gid, ResourceKind::App, "VS Code", "/usr/bin/code", Some("icon"), Some("--reuse-window"))
            .unwrap();
        assert_eq!(r.name, "VS Code");
        assert_eq!(r.kind, ResourceKind::App);
        assert_eq!(r.sort_order, 1);
    }

    #[test]
    fn list_by_group_ordered() {
        let (conn, gid) = setup();
        let a = create(&conn, gid, ResourceKind::Web, "GitHub", "https://github.com", None, None).unwrap();
        let b = create(&conn, gid, ResourceKind::Web, "Google", "https://google.com", None, None).unwrap();
        let list = list_by_group(&conn, gid).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_resource_fields() {
        let (conn, gid) = setup();
        let r = create(&conn, gid, ResourceKind::App, "Old", "/bin/old", None, None).unwrap();
        let updated = update(&conn, r.id, gid, ResourceKind::Web, "New", "https://new.com", Some("i"), Some("a"))
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.kind, ResourceKind::Web);
        assert_eq!(updated.target, "https://new.com");
    }

    #[test]
    fn reorder_within_group() {
        let (conn, gid) = setup();
        let a = create(&conn, gid, ResourceKind::Web, "A", "https://a.com", None, None).unwrap();
        let b = create(&conn, gid, ResourceKind::Web, "B", "https://b.com", None, None).unwrap();
        reorder(&conn, gid, &[b.id, a.id]).unwrap();
        let list = list_by_group(&conn, gid).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn reorder_moves_resource_to_another_group() {
        let conn = init_in_memory().unwrap();
        let g1 = group::create(&conn, "G1").unwrap();
        let g2 = group::create(&conn, "G2").unwrap();
        let a = create(&conn, g1.id, ResourceKind::Web, "A", "https://a.com", None, None).unwrap();
        let b = create(&conn, g1.id, ResourceKind::Web, "B", "https://b.com", None, None).unwrap();
        // 将 a 移动到 g2
        reorder(&conn, g2.id, &[a.id]).unwrap();
        let g1_list = list_by_group(&conn, g1.id).unwrap();
        let g2_list = list_by_group(&conn, g2.id).unwrap();
        assert_eq!(g1_list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id]);
        assert_eq!(g2_list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id]);
    }

    #[test]
    fn delete_resource() {
        let (conn, gid) = setup();
        let r = create(&conn, gid, ResourceKind::App, "Temp", "/bin/temp", None, None).unwrap();
        delete(&conn, r.id).unwrap();
        assert!(get(&conn, r.id).is_err());
    }

    #[test]
    fn search_resources_by_name() {
        let (conn, gid) = setup();
        create(&conn, gid, ResourceKind::Web, "GitHub", "https://github.com", None, None).unwrap();
        create(&conn, gid, ResourceKind::Web, "Google", "https://google.com", None, None).unwrap();
        let found = search(&conn, "git").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "GitHub");
    }

    #[test]
    fn delete_group_cascades_resources() {
        let (conn, gid) = setup();
        create(&conn, gid, ResourceKind::Web, "A", "https://a.com", None, None).unwrap();
        create(&conn, gid, ResourceKind::Web, "B", "https://b.com", None, None).unwrap();
        group::delete(&conn, gid).unwrap();
        assert_eq!(list_by_group(&conn, gid).unwrap().len(), 0);
    }
}
