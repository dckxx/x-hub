use crate::models::{Resource, ResourceKind};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 文本列归一：空白串按 NULL 处理（说明 / 备注标签）
fn norm_text(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.trim().is_empty())
}

/// 备注落盘：空串归一 NULL；非空经 DPAPI 加密（secret.rs），加密失败按 SQL 参数错误
/// 上抛让保存整体失败——绝不明文落库
fn encrypt_remark(remark: Option<&str>) -> Result<Option<String>> {
    match norm_text(remark) {
        Some(s) => Ok(Some(crate::secret::protect(s).map_err(|e| {
            rusqlite::Error::InvalidParameterName(format!("备注加密失败: {e}"))
        })?)),
        None => Ok(None),
    }
}

pub fn create(
    conn: &Connection,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
    zone_id: Option<i64>,
    description: Option<&str>,
    remark: Option<&str>,
    remark_label: Option<&str>,
) -> Result<Resource> {
    let remark_enc = encrypt_remark(remark)?;
    let ts = now();
    conn.execute(
        "INSERT INTO resources (kind, name, target, category, icon, args, description, remark, remark_label, sort_order, zone_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM resources), ?10, ?11, ?11)",
        params![kind_to_str(&kind), name, target, category, icon, args, norm_text(description), remark_enc, norm_text(remark_label), zone_id, ts],
    )?;
    let id = conn.last_insert_rowid();
    // ADR 0012 决策 3：新建未指定小类 → 自动归入该大类的默认小类；
    // 该大类还没有任何小类时保持 NULL（「未归类」）。编辑时显式置 NULL 不走这里。
    if category.is_none() {
        let kind_str = kind_to_str(&kind);
        if let Some(default_cat) = super::subcategory::default_name(conn, &kind_str)? {
            conn.execute(
                "UPDATE resources SET category = ?1 WHERE id = ?2",
                params![default_cat, id],
            )?;
        }
    }
    get(conn, id)
}

pub fn get(conn: &Connection, id: i64) -> Result<Resource> {
    conn.query_row(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at, zone_id, description, remark, remark_label FROM resources WHERE id = ?1",
        params![id],
        row_to_resource,
    )
}

pub fn list_all(conn: &Connection) -> Result<Vec<Resource>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at, zone_id, description, remark, remark_label FROM resources ORDER BY sort_order ASC, id ASC",
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
    zone_id: Option<i64>,
    description: Option<&str>,
    remark: Option<&str>,
    remark_label: Option<&str>,
) -> Result<Resource> {
    let remark_enc = encrypt_remark(remark)?;
    update_write(conn, id, kind, name, target, category, icon, args, zone_id, description, remark_enc, remark_label)
}

/// 与 update 同一写入路径，但 remark **原样落库**（调用方传已落盘原值——密文或
/// 旧明文）：扩展桥「全对象写」只改其它字段时透传现值，不做解密→重加密往返
pub fn update_with_stored_remark(
    conn: &Connection,
    id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
    zone_id: Option<i64>,
    description: Option<&str>,
    remark_stored: Option<String>,
    remark_label: Option<&str>,
) -> Result<Resource> {
    update_write(conn, id, kind, name, target, category, icon, args, zone_id, description, remark_stored, remark_label)
}

fn update_write(
    conn: &Connection,
    id: i64,
    kind: ResourceKind,
    name: &str,
    target: &str,
    category: Option<&str>,
    icon: Option<&str>,
    args: Option<&str>,
    zone_id: Option<i64>,
    description: Option<&str>,
    remark_enc: Option<String>,
    remark_label: Option<&str>,
) -> Result<Resource> {
    let affected = conn.execute(
        "UPDATE resources SET kind = ?1, name = ?2, target = ?3, category = ?4, icon = ?5, args = ?6, zone_id = ?7, description = ?8, remark = ?9, remark_label = ?10, updated_at = ?11 WHERE id = ?12",
        params![kind_to_str(&kind), name, target, category, icon, args, zone_id, norm_text(description), remark_enc, norm_text(remark_label), now(), id],
    )?;
    if affected == 0 {
        // 带 NOT_FOUND 前缀：扩展桥调用方据此与服务器错误区分，不再盲目重试
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 资源 {id} 不存在"
        )));
    }
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

/// 批量改分区归属（右键「移动到分区」/ 删分区撤销恢复用），不动 sort_order
pub fn set_zone(conn: &Connection, ids: &[i64], zone_id: Option<i64>) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute(
            "UPDATE resources SET zone_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![zone_id, ts, id],
        )?;
    }
    tx.commit()
}

/// 分区模式拖拽的原子写回：entries 顺序即新的全表 sort_order（0..n-1），
/// 每项同时携带目标分区——归属与顺序绝不拆成两次写，中途崩溃不会留下半态。
/// 调用方需先校验 entries 覆盖全表且 zone_id 均存在（commands 层把关）。
pub fn reorder_zoned(conn: &Connection, entries: &[(i64, Option<i64>)]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (order, (id, zone_id)) in entries.iter().enumerate() {
        tx.execute(
            "UPDATE resources SET sort_order = ?1, zone_id = ?2, updated_at = ?3 WHERE id = ?4",
            params![order as i64, zone_id, ts, id],
        )?;
    }
    tx.commit()
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Resource>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, target, category, icon, args, sort_order, last_launched_at, created_at, updated_at, zone_id, description, remark, remark_label FROM resources WHERE name LIKE ?1 ORDER BY sort_order ASC",
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
        zone_id: row.get(11)?,
        description: row.get(12)?,
        // 备注明文不随列表/单查下发（库里是 DPAPI 密文，secret.rs）：明文只经
        // remark_plaintext 按需解密进编辑弹窗，不散进资源列表/搜索结果
        remark: None,
        remark_label: row.get(14)?,
    })
}

/// 备注列原值（DPAPI 密文或旧明文）原样读出：扩展桥「全对象写」透传写回用
pub fn remark_raw(conn: &Connection, id: i64) -> Result<Option<String>> {
    conn.query_row(
        "SELECT remark FROM resources WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )
}

/// 备注明文按需解密：仅编辑弹窗拉取一次，明文不进 store/列表/搜索结果/扩展桥。
/// 跨机器/跨用户解不开时按无备注处理（unprotect 返回 None）
pub fn remark_plaintext(conn: &Connection, id: i64) -> Result<Option<String>> {
    Ok(remark_raw(conn, id)?.and_then(|s| crate::secret::unprotect(&s)))
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
        let r = create(&conn, ResourceKind::App, "VS Code", "/usr/bin/code", None, Some("icon"), Some("--reuse-window"), None, None, None, None)
            .unwrap();
        assert_eq!(r.name, "VS Code");
        assert_eq!(r.kind, ResourceKind::App);
        assert_eq!(r.sort_order, 1);
    }

    #[test]
    fn create_and_get_file_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::File, "报告", "C:/docs/report.pdf", Some("文档"), None, None, None, None, None, None)
            .unwrap();
        assert_eq!(r.kind, ResourceKind::File);
        assert_eq!(r.category.as_deref(), Some("文档"));
        assert_eq!(r.target, "C:/docs/report.pdf");
    }

    #[test]
    fn list_all_ordered() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None, None, None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None, None, None, None, None).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_resource_fields() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Old", "/bin/old", None, None, None, None, None, None, None).unwrap();
        let updated = update(&conn, r.id, ResourceKind::Web, "New", "https://new.com", None, Some("i"), Some("a"), None, None, None, None)
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.kind, ResourceKind::Web);
        assert_eq!(updated.target, "https://new.com");
    }

    #[test]
    fn reorder_resources() {
        let conn = setup();
        let a = create(&conn, ResourceKind::Web, "A", "https://a.com", None, None, None, None, None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "B", "https://b.com", None, None, None, None, None, None, None).unwrap();
        reorder(&conn, &[b.id, a.id]).unwrap();
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id, a.id]);
    }

    #[test]
    fn delete_resource() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "Temp", "/bin/temp", None, None, None, None, None, None, None).unwrap();
        delete(&conn, r.id).unwrap();
        assert!(get(&conn, r.id).is_err());
    }

    #[test]
    fn set_zone_updates_membership_without_reordering() {
        let conn = setup();
        let a = create(&conn, ResourceKind::App, "A", "https://a.com", None, None, None, None, None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "B", "https://b.com", None, None, None, None, None, None, None).unwrap();
        set_zone(&conn, &[a.id, b.id], Some(7)).unwrap();
        assert_eq!(get(&conn, a.id).unwrap().zone_id, Some(7));
        assert_eq!(get(&conn, b.id).unwrap().zone_id, Some(7));
        // 顺序不动
        let list = list_all(&conn).unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a.id, b.id]);
        set_zone(&conn, &[a.id], None).unwrap();
        assert_eq!(get(&conn, a.id).unwrap().zone_id, None);
    }

    #[test]
    fn reorder_zoned_writes_order_and_membership_together() {
        let conn = setup();
        let a = create(&conn, ResourceKind::App, "A", "https://a.com", None, None, None, None, None, None, None).unwrap();
        let b = create(&conn, ResourceKind::Web, "B", "https://b.com", None, None, None, None, None, None, None).unwrap();
        let c = create(&conn, ResourceKind::File, "C", "C:/c", None, None, None, None, None, None, None).unwrap();
        reorder_zoned(&conn, &[(c.id, Some(2)), (a.id, Some(2)), (b.id, None)]).unwrap();
        let list = list_all(&conn).unwrap();
        // 顺序 = entries 顺序；归属 = 各自携带的 zone_id
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![c.id, a.id, b.id]);
        assert_eq!(list[0].zone_id, Some(2));
        assert_eq!(list[1].zone_id, Some(2));
        assert_eq!(list[2].zone_id, None);
    }

    #[test]
    fn search_resources_by_name() {
        let conn = setup();
        create(&conn, ResourceKind::Web, "GitHub", "https://github.com", None, None, None, None, None, None, None).unwrap();
        create(&conn, ResourceKind::Web, "Google", "https://google.com", None, None, None, None, None, None, None).unwrap();
        let found = search(&conn, "git").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "GitHub");
    }

    #[test]
    fn remark_encrypted_at_rest_and_roundtrips() {
        let conn = setup();
        let r = create(
            &conn,
            ResourceKind::Web,
            "公司邮箱",
            "https://mail.example.com",
            None,
            None,
            None,
            None,
            Some("工作邮箱登录页"),
            Some("admin@example.com p@ss"),
            Some("账号密码"),
        )
        .unwrap();
        let got = get(&conn, r.id).unwrap();
        assert_eq!(got.description.as_deref(), Some("工作邮箱登录页"));
        // get 不再下发明文；明文走按需解密通道
        assert_eq!(got.remark, None, "get 不得下发备注明文");
        assert_eq!(remark_plaintext(&conn, r.id).unwrap().as_deref(), Some("admin@example.com p@ss"));
        assert_eq!(got.remark_label.as_deref(), Some("账号密码"));
        // 落盘必须是密文：原始列值带 dp1: 前缀且不含明文
        let raw: String = conn
            .query_row("SELECT remark FROM resources WHERE id = ?1", params![r.id], |row| row.get(0))
            .unwrap();
        assert!(raw.starts_with("dp1:"), "落盘值应为 DPAPI 密文: {raw}");
        assert!(!raw.contains("p@ss"));
    }

    #[test]
    fn update_can_clear_and_rename_remark() {
        let conn = setup();
        let r = create(&conn, ResourceKind::File, "压缩包", "C:/a.7z", None, None, None, None, None, Some("old-pw"), None).unwrap();
        let renamed = update(&conn, r.id, ResourceKind::File, "压缩包", "C:/a.7z", None, None, None, None, None, Some("new-pw"), Some("解压密码")).unwrap();
        assert_eq!(remark_plaintext(&conn, r.id).unwrap().as_deref(), Some("new-pw"));
        assert_eq!(renamed.remark_label.as_deref(), Some("解压密码"));
        let cleared = update(&conn, r.id, ResourceKind::File, "压缩包", "C:/a.7z", None, None, None, None, None, None, None).unwrap();
        assert_eq!(remark_plaintext(&conn, r.id).unwrap(), None);
        assert_eq!(cleared.remark_label, None);
        let raw: Option<String> = conn
            .query_row("SELECT remark FROM resources WHERE id = ?1", params![r.id], |row| row.get(0))
            .unwrap();
        assert_eq!(raw, None, "清空备注必须落 NULL，不得残留密文");
    }

    #[test]
    fn empty_remark_and_label_normalized_to_null() {
        let conn = setup();
        let r = create(&conn, ResourceKind::App, "A", "/a.exe", None, None, None, None, Some("  "), Some(""), Some("  ")).unwrap();
        let got = get(&conn, r.id).unwrap();
        assert_eq!(got.description, None);
        assert_eq!(remark_plaintext(&conn, r.id).unwrap(), None);
        assert_eq!(got.remark_label, None);
    }
}
