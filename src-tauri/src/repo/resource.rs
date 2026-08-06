use crate::models::{Resource, ResourceKind};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

pub fn create(
    conn: &Connection,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    let ts = now();
    conn.execute(
        "INSERT INTO resources (kind, name, target, category, icon, args, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM resources), ?7, ?7)",
        params![kind_to_str(&kind), name, target, category, icon, args, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Resource> {
    conn.query_row(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at FROM resources WHERE id = ?1",
        params![id],
        row_to_resource,
    )
}

pub fn list_all(conn: &Connection) -> Result<Vec<Resource>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at FROM resources ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_resource)?;
    rows.collect()
}

pub fn update(
    conn: &Connection,
    id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
) -> Result<Resource> {
    conn.execute(
        "UPDATE resources SET kind = ?1, name = ?2, target = ?3, category = ?4, icon = ?5, args = ?6, updated_at = ?7 WHERE id = ?8",
        params![kind_to_str(&kind), name, target, category, icon, args, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM resources WHERE id = ?1", params![id])?;
    Ok(())
}

/// 记录资源最近启动时间（最近使用排序用）
pub fn touch(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE resources SET last_launched_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// 按新顺序重新排列所有资源
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (order, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE resources SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![order as i64, ts, id],
        )?;
    }
    tx.commit()
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Resource>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at FROM resources WHERE name LIKE ?1 ORDER BY sort_order ASC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_resource)?;
    rows.collect()
}

pub fn kind_to_str(kind: &ResourceKind) -> &'static str {
    match kind {
        ResourceKind::App => "app",
        ResourceKind::Web => "web",
        ResourceKind::File => "file",
    }
}

pub fn row_to_resource(row: &rusqlite::Row) -> Result<Resource> {
    let kind: String = row.get(1)?;
    Ok(Resource {
        id: row.get(0)?,
        kind: match kind.as_str() {
            "app" => ResourceKind::App,
            "file" => ResourceKind::File,
            _ => ResourceKind::Web,
        },
        name: row.get(2)?,
        target: row.get(3)?,
        category: row.get(4)?,
        icon: row.get(5)?,
        args: row.get(6)?,
        sort_order: row.get(7)?,
        last_launched_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
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
    fn create_and_get_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "VS Code", "/usr/bin/code", None, Some("icon"), Some("--reuse-window"))
            .unwrap();
        assert_eq!(r.name, "VS Code");
        assert_eq!(r.kind, ResourceKind::App);
        assert_eq!(r.sort_order, 1);
    }

    #[test]
    fn create_and_get_file_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::File, "报告", "C:/docs/report.pdf", Some("文档"), None, None)
            .unwrap();
        assert_eq!(r.kind, ResourceKind::File);
        assert_eq!(r.category.as_deref(), Some("文档"));
        assert_eq!(r.target, "C:/docs/report.pdf");
    }

    #[test]
    fn list_all_ordered() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_resource_fields() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Old", "/bin/old", None, None, None).unwrap();
        let updated = update(&conn, r.id, ResourceKind::Web, "New", "https://new.com", None, Some("i"), Some("a"))
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.kind, ResourceKind::Web);
        assert_eq!(updated.target, "https://new.com");
    }

    #[test]
    fn reorder_resources() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "A", "https://a.com", None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "B", "https://b.com", None, None, None).unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn delete_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Temp", "/bin/temp", None, None, None).unwrap();
        delete(&conn, r.id).unwrap();
        assert!(get(&conn, r.id).is_err());
    }

    #[test]
    fn search_resources_by_name() {
        let conn = setup();
        create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None).unwrap();
        create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None).unwrap();
        let found = search(&conn, "git").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "GitHub");
    }
}
