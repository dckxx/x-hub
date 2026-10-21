//! 速达小类（ADR 0012）：大类（resources.kind）下单归属的小类库，每条资源恰好归属
//! 一个小类（单归属），不是多选标签。`resources.category` 语义升级为「所属大类小类库中
//! 的一个小类名」——文件大类沿用原 7 个内置值作为初始小类（建表种子见 db.rs），
//! 应用/网页初始为空、按需新建；各大类一套、允许同名不同义（UNIQUE(kind, name)）。
//! 存量条目不回填：category 为 NULL 即「未归类」，与默认小类区分。

use crate::models::ResourceSubcategory;
use rusqlite::{params, Connection};

use super::now;

pub const VALID_KINDS: [&str; 3] = ["app", "web", "file"];

fn row_to_sub(row: &rusqlite::Row) -> rusqlite::Result<ResourceSubcategory> {
    Ok(ResourceSubcategory {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        sort_order: row.get(3)?,
        is_default: row.get::<_, i64>(4)? != 0,
    })
}

const COLS: &str = "id, kind, name, sort_order, is_default";

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<ResourceSubcategory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM resource_subcategories ORDER BY kind ASC, sort_order ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_sub)?;
    rows.collect()
}

/// 大类的小类名是否可用（同大类内唯一；rename 时排除自身）
pub fn is_name_free(
    conn: &Connection,
    kind: &str,
    name: &str,
    exclude_id: Option<i64>,
) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_subcategories WHERE kind = ?1 AND name = ?2 AND id != ?3",
        params![kind, name, exclude_id.unwrap_or(-1)],
        |r| r.get(0),
    )?;
    Ok(n == 0)
}

pub fn create(conn: &Connection, kind: &str, name: &str) -> rusqlite::Result<ResourceSubcategory> {
    // 大类的第一个小类自动成为默认小类（用户随后可在设置里改）
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM resource_subcategories WHERE kind = ?1",
        params![kind],
        |r| r.get(0),
    )?;
    let is_default: i64 = if count == 0 { 1 } else { 0 };
    let max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) FROM resource_subcategories WHERE kind = ?1",
        params![kind],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO resource_subcategories (kind, name, sort_order, is_default) VALUES (?1, ?2, ?3, ?4)",
        params![kind, name, max + 1, is_default],
    )?;
    Ok(ResourceSubcategory {
        id: conn.last_insert_rowid(),
        kind: kind.to_string(),
        name: name.to_string(),
        sort_order: max + 1,
        is_default: is_default != 0,
    })
}

/// 改名：同大类下挂旧名的资源条目一并改挂新名（单事务，资源层无感知）
pub fn rename(conn: &mut Connection, id: i64, name: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (kind, old): (String, String) = tx
        .query_row(
            "SELECT kind, name FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;
    let free = is_name_free(&tx, &kind, name, Some(id)).map_err(|e| e.to_string())?;
    if !free {
        return Err(format!("DUP: 大类下已有名为「{name}」的小类"));
    }
    tx.execute(
        "UPDATE resource_subcategories SET name = ?1 WHERE id = ?2",
        params![name, id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE resources SET category = ?1, updated_at = ?2 WHERE kind = ?3 AND category = ?4",
        params![name, now(), kind, old],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// 删除小类：其条目批量改挂默认小类（大类一个小类不剩时回「未归类」NULL）；
/// 若删的是默认小类，把排序最前的剩余小类提为默认。单事务完成。
pub fn delete(conn: &mut Connection, id: i64) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (kind, name, was_default): (String, String, i64) = tx
        .query_row(
            "SELECT kind, name, is_default FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;
    tx.execute(
        "DELETE FROM resource_subcategories WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    if was_default != 0 {
        tx.execute(
            "UPDATE resource_subcategories SET is_default = 1 WHERE id = (
               SELECT id FROM resource_subcategories WHERE kind = ?1
               ORDER BY sort_order ASC, id ASC LIMIT 1)",
            params![kind],
        )
        .map_err(|e| e.to_string())?;
    }
    // 条目改挂默认小类；没有默认（该大类已被删空）则回「未归类」
    let new_default: Option<String> = tx
        .query_row(
            "SELECT name FROM resource_subcategories WHERE kind = ?1 AND is_default = 1 LIMIT 1",
            params![kind],
            |r| r.get(0),
        )
        .ok();
    match new_default {
        Some(dn) => tx
            .execute(
                "UPDATE resources SET category = ?1, updated_at = ?2 WHERE kind = ?3 AND category = ?4",
                params![dn, now(), kind, name],
            )
            .map_err(|e| e.to_string())?,
        None => tx
            .execute(
                "UPDATE resources SET category = NULL, updated_at = ?1 WHERE kind = ?2 AND category = ?3",
                params![now(), kind, name],
            )
            .map_err(|e| e.to_string())?,
    };
    tx.commit().map_err(|e| e.to_string())
}

/// 组内拖拽排序：按传入 id 顺序写 sort_order
pub fn reorder(conn: &Connection, kind: &str, ids: &[i64]) -> rusqlite::Result<()> {
    for (idx, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE resource_subcategories SET sort_order = ?1 WHERE id = ?2 AND kind = ?3",
            params![idx as i64, id, kind],
        )?;
    }
    Ok(())
}

pub fn set_default(conn: &Connection, id: i64) -> Result<(), String> {
    let kind: String = conn
        .query_row(
            "SELECT kind FROM resource_subcategories WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(|_| format!("NOT_FOUND: 小类 {id} 不存在"))?;
    conn.execute(
        "UPDATE resource_subcategories SET is_default = 0 WHERE kind = ?1",
        params![kind],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE resource_subcategories SET is_default = 1 WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 大类的默认小类名：is_default=1 的行优先，否则取排序最前的行；该大类还没有小类时 None。
/// 新建资源未指定小类时经此自动归入默认（ADR 0012 决策 3）。
pub fn default_name(conn: &Connection, kind: &str) -> rusqlite::Result<Option<String>> {
    match conn.query_row(
        "SELECT name FROM resource_subcategories WHERE kind = ?1
         ORDER BY is_default DESC, sort_order ASC, id ASC LIMIT 1",
        params![kind],
        |r| r.get::<_, String>(0),
    ) {
        Ok(n) => Ok(Some(n)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;
    use crate::models::ResourceKind;
    use crate::repo::resource;

    fn kinds_eq(a: &ResourceSubcategory, kind: &str, name: &str) -> bool {
        a.kind == kind && a.name == name
    }

    #[test]
    fn seed_file_categories_and_default_other() {
        let conn = init_in_memory().unwrap();
        let subs = list(&conn).unwrap();
        assert_eq!(subs.len(), 7);
        assert!(subs.iter().all(|s| s.kind == "file"));
        let other = subs.iter().find(|s| s.name == "其他").unwrap();
        assert!(other.is_default);
        assert_eq!(default_name(&conn, "file").unwrap().as_deref(), Some("其他"));
        assert_eq!(default_name(&conn, "app").unwrap(), None);
    }

    #[test]
    fn create_auto_defaults_first_of_kind() {
        let conn = init_in_memory().unwrap();
        let s = create(&conn, "app", "开发工具").unwrap();
        assert!(s.is_default);
        let s2 = create(&conn, "app", "常用").unwrap();
        assert!(!s2.is_default);
        assert_eq!(default_name(&conn, "app").unwrap().as_deref(), Some("开发工具"));
    }

    #[test]
    fn create_resource_auto_assigns_default_subcategory() {
        let conn = init_in_memory().unwrap();
        // 应用大类还没有小类 → 保持「未归类」
        let r = resource::create(&conn, ResourceKind::App, "a", "t", None, None, None, None, None, None, None).unwrap();
        assert_eq!(r.category, None);
        // 建了小类之后 → 自动归默认
        create(&conn, "app", "开发工具").unwrap();
        let r2 = resource::create(&conn, ResourceKind::App, "b", "t", None, None, None, None, None, None, None).unwrap();
        assert_eq!(r2.category.as_deref(), Some("开发工具"));
        // 显式指定不受影响
        let r3 = resource::create(&conn, ResourceKind::App, "c", "t", Some("常用"), None, None, None, None, None, None).unwrap();
        assert_eq!(r3.category.as_deref(), Some("常用"));
    }

    #[test]
    fn rename_cascades_to_resources() {
        let conn = init_in_memory().unwrap();
        // 文件种子小类已存在，这里新建一个专属小类验证改名级联
        let sub = create(&conn, "file", "临时分类").unwrap();
        let r = resource::create(&conn, ResourceKind::File, "d", "t", Some("临时分类"), None, None, None, None, None, None).unwrap();
        let mut conn = conn;
        rename(&mut conn, sub.id, "资料").unwrap();
        let after = resource::get(&conn, r.id).unwrap();
        assert_eq!(after.category.as_deref(), Some("资料"));
        let names: Vec<String> = list(&conn).unwrap().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"资料".to_string()));
        assert!(!names.contains(&"临时分类".to_string()));
    }

    #[test]
    fn delete_reassigns_entries_to_default_and_promotes_successor() {
        let mut conn = init_in_memory().unwrap();
        let doc = create(&conn, "app", "文档类").unwrap(); // 第一个 → 默认
        let dev = create(&conn, "app", "开发工具").unwrap();
        let r_doc = resource::create(&conn, ResourceKind::App, "x", "t", Some("文档类"), None, None, None, None, None, None).unwrap();
        let r_dev = resource::create(&conn, ResourceKind::App, "y", "t", Some("开发工具"), None, None, None, None, None, None).unwrap();
        // 删默认小类 → 其条目改挂新默认（晋升的开发工具）
        delete(&mut conn, doc.id).unwrap();
        assert_eq!(
            resource::get(&conn, r_doc.id).unwrap().category.as_deref(),
            Some("开发工具")
        );
        assert_eq!(
            resource::get(&conn, r_dev.id).unwrap().category.as_deref(),
            Some("开发工具")
        );
        let subs = list(&conn).unwrap();
        let dev_sub = subs.iter().find(|s| kinds_eq(s, "app", "开发工具")).unwrap();
        assert!(dev_sub.is_default);
        // 删到只剩默认再删 → 条目回「未归类」
        delete(&mut conn, dev.id).unwrap();
        assert_eq!(resource::get(&conn, r_doc.id).unwrap().category, None);
        assert!(list(&conn).unwrap().iter().all(|s| s.kind != "app"));
    }

    #[test]
    fn set_default_and_reorder() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "web", "收藏").unwrap();
        let b = create(&conn, "web", "工作").unwrap();
        set_default(&conn, b.id).unwrap();
        assert_eq!(default_name(&conn, "web").unwrap().as_deref(), Some("工作"));
        reorder(&conn, "web", &[b.id, a.id]).unwrap();
        let subs = list(&conn).unwrap();
        let web: Vec<&ResourceSubcategory> = subs.iter().filter(|s| s.kind == "web").collect();
        assert_eq!(web[0].name, "工作");
        assert_eq!(web[1].name, "收藏");
    }
}
