use crate::models::ClipboardItem;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 单条文本最大存储长度（超过截断，防数据库膨胀）
pub const MAX_ITEM_LEN: usize = 20000;

/// 记录文本（可能来自剪贴板）：
/// - 超过 MAX_ITEM_LEN 截断（html 同步丢弃，避免与截断文本不一致）
/// - 相同内容（非置顶）已存在 → 刷新 updated_at/source_app 挪到最前，不新增重复条目
/// - 写入后执行保留策略清理
pub fn insert(conn: &Connection, content: &str, html: Option<&str>, source: Option<&str>) -> Result<()> {
    let truncated = content.chars().take(MAX_ITEM_LEN).collect::<String>();
    let html = if content.len() == truncated.len() {
        html.map(|h| h.to_string())
    } else {
        None
    };
    let ts = now();

    // 已有相同内容（置顶项不动）：仅刷新时间与来源，实现「内容一样就挪到最前、不新增」
    let recent_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM clipboard_history
             WHERE content = ?1 AND is_pinned = 0
             ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![truncated],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = recent_id {
        conn.execute(
            "UPDATE clipboard_history SET updated_at = ?1, source_app = COALESCE(?2, source_app) WHERE id = ?3",
            params![ts, source, id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO clipboard_history (content, html, source_app, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![truncated, html, source, ts],
        )?;
    }
    cleanup(conn)?;
    Ok(())
}

/// 使用记录：刷新 updated_at 把该条目挪到列表最前（粘贴/复制历史项时调用）。
pub fn touch(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE clipboard_history SET updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// 保留策略（Q5/Q12 决策）：
/// - 非置顶项超过 ttl_days 自动删除
/// - 总条数（含置顶）超过 max_items 时删最旧
pub fn cleanup(conn: &Connection) -> Result<()> {
    let cfg = crate::config::load();
    cleanup_with(conn, cfg.clipboard_max_items, cfg.clipboard_ttl_days)
}

/// 保留策略实际执行（参数化便于测试，避免污染真实配置）
pub fn cleanup_with(conn: &Connection, max_items: i64, ttl_days: i64) -> Result<()> {
    let max_items = max_items.max(1);

    let cutoff = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(ttl_days.max(0)))
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();
    conn.execute(
        "DELETE FROM clipboard_history WHERE is_pinned = 0 AND updated_at < ?1",
        params![cutoff],
    )?;

    conn.execute(
        "DELETE FROM clipboard_history WHERE id IN (
           SELECT id FROM clipboard_history
           ORDER BY updated_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         )",
        params![max_items],
    )?;
    Ok(())
}

/// 置顶优先，其次最近复制；可选关键字搜索（内容/来源应用）
pub fn list(conn: &Connection, keyword: Option<&str>, limit: i64) -> Result<Vec<ClipboardItem>> {
    let kw = keyword.map(|k| k.trim()).filter(|k| !k.is_empty());
    let mut sql = String::from(
        "SELECT id, content, html, source_app, is_pinned, created_at, updated_at
         FROM clipboard_history ",
    );
    if let Some(k) = kw {
        let pattern = format!("%{}%", k);
        sql.push_str(
            "WHERE content LIKE ?1 OR html LIKE ?1 OR COALESCE(source_app,'') LIKE ?1 ",
        );
        sql.push_str("ORDER BY is_pinned DESC, updated_at DESC, id DESC LIMIT ?2");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pattern, limit], row_to_item)?;
        rows.collect()
    } else {
        sql.push_str("ORDER BY is_pinned DESC, updated_at DESC, id DESC LIMIT ?1");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![limit], row_to_item)?;
        rows.collect()
    }
}

pub fn get(conn: &Connection, id: i64) -> Result<ClipboardItem> {
    conn.query_row(
        "SELECT id, content, html, source_app, is_pinned, created_at, updated_at
         FROM clipboard_history WHERE id = ?1",
        params![id],
        row_to_item,
    )
}

pub fn toggle_pin(conn: &Connection, id: i64) -> Result<ClipboardItem> {
    conn.execute(
        "UPDATE clipboard_history SET is_pinned = CASE WHEN is_pinned = 1 THEN 0 ELSE 1 END, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM clipboard_history", [])?;
    Ok(())
}

pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
}

fn row_to_item(row: &rusqlite::Row) -> Result<ClipboardItem> {
    Ok(ClipboardItem {
        id: row.get(0)?,
        content: row.get(1)?,
        html: row.get(2)?,
        source_app: row.get(3)?,
        is_pinned: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
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
    fn insert_then_list_newest_first() {
        let conn = setup();
        insert(&conn, "first", None, None).unwrap();
        insert(&conn, "second", Some("<b>html</b>"), Some("浏览器")).unwrap();
        let list = list(&conn, None, 50).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].content, "second");
        assert_eq!(list[0].html.as_deref(), Some("<b>html</b>"));
        assert_eq!(list[0].source_app.as_deref(), Some("浏览器"));
        assert_eq!(list[1].content, "first");
    }

    #[test]
    fn dedup_within_window_refreshes_not_inserts() {
        let conn = setup();
        insert(&conn, "same", None, Some("A")).unwrap();
        let first: i64 = conn
            .query_row("SELECT id FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        insert(&conn, "same", None, Some("B")).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert!(first > 0);
    }

    #[test]
    fn long_content_is_truncated_and_drops_html() {
        let conn = setup();
        let long = "x".repeat(MAX_ITEM_LEN + 100);
        insert(&conn, &long, Some("<b>html</b>"), None).unwrap();
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all[0].content.chars().count(), MAX_ITEM_LEN);
        assert!(all[0].html.is_none());
    }

    #[test]
    fn dedup_anytime_refreshes_existing_row() {
        let conn = setup();
        insert(&conn, "same", None, Some("A")).unwrap();
        // 模拟很久以前的旧条目（远超原 2s 去重窗口），再复制相同内容
        conn.execute(
            "UPDATE clipboard_history SET updated_at = '2000-01-01 00:00:00.000000'",
            [],
        )
        .unwrap();
        insert(&conn, "same", None, Some("B")).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "相同内容不新增条目");
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert!(all[0].updated_at.as_str() > "2000-01-01");
    }

    #[test]
    fn touch_moves_item_to_front() {
        let conn = setup();
        insert(&conn, "a", None, None).unwrap();
        insert(&conn, "b", None, None).unwrap();
        let a_id = list(&conn, None, 50).unwrap()[1].id;
        std::thread::sleep(std::time::Duration::from_millis(5));
        touch(&conn, a_id).unwrap();
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all[0].id, a_id, "touch 后条目挪到最前");
    }

    #[test]
    fn pin_exempts_from_ttl_cleanup() {
        let conn = setup();
        let old = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(30))
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S%.6f")
            .to_string();
        conn.execute(
            "INSERT INTO clipboard_history (content, is_pinned, created_at, updated_at) VALUES ('pinned', 1, ?1, ?1)",
            params![old],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (content, is_pinned, created_at, updated_at) VALUES ('plain', 0, ?1, ?1)",
            params![old],
        )
        .unwrap();
        cleanup(&conn).unwrap();
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].content, "pinned");
    }

    #[test]
    fn max_items_caps_total_including_pinned() {
        let conn = setup();
        // 直接参数化调用，避免污染真实用户配置
        insert(&conn, "a", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "b", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "c", None, None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        insert(&conn, "d", None, None).unwrap();

        cleanup_with(&conn, 3, 7).unwrap();
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all.len(), 3);
        assert!(!all.iter().any(|i| i.content == "a"));
    }

    #[test]
    fn search_matches_content() {
        let conn = setup();
        insert(&conn, "https://github.com/tauri", None, None).unwrap();
        insert(&conn, "别的文本", None, None).unwrap();
        let found = list(&conn, Some("github"), 50).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].content.contains("github"));
    }

    #[test]
    fn toggle_pin_and_delete() {
        let conn = setup();
        insert(&conn, "x", None, None).unwrap();
        let item = list(&conn, None, 50).unwrap().remove(0);
        let pinned = toggle_pin(&conn, item.id).unwrap();
        assert!(pinned.is_pinned);
        delete(&conn, item.id).unwrap();
        assert!(get(&conn, item.id).is_err());
        assert_eq!(count(&conn).unwrap(), 0);
    }
}
