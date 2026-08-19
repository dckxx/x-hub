use crate::models::ClipboardItem;
use crate::repo::now;
use rusqlite::{params, Connection, Result};
use std::sync::atomic::{AtomicU64, Ordering};

/// 单条文本最大存储长度（超过截断，防数据库膨胀）
pub const MAX_ITEM_LEN: usize = 20000;
/// 单条 html 片段最大长度（复制整页/大表格时 html 可能达数百 KB，超限丢弃 html 只留纯文本）
pub const MAX_HTML_LEN: usize = 64 * 1024;
/// 保留策略清理最小间隔：连续复制时每次入库都跑两个 DELETE（大表可能耗时数十 ms），
/// 节流到至少间隔该时长才执行一次，避免高频复制时 UI/监听被锁拖慢。
const CLEANUP_INTERVAL_MS: u64 = 60_000;
/// 上次清理时间戳（毫秒，进程内单调），初始为 0 表示首条就清理一次
static LAST_CLEANUP_MS: AtomicU64 = AtomicU64::new(0);

/// 节流后的保留策略清理：距离上次执行不足 CLEANUP_INTERVAL_MS 时直接跳过。
/// 保留策略允许延迟执行（临时多存几条无碍），换取热路径不频繁锁表。
pub fn cleanup_throttled(conn: &Connection) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_CLEANUP_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < CLEANUP_INTERVAL_MS {
        return;
    }
    if LAST_CLEANUP_MS
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        let _ = cleanup(conn);
    }
}

/// 记录文本（可能来自剪贴板）：
/// - 超过 MAX_ITEM_LEN 截断（html 同步丢弃，避免与截断文本不一致）
/// - html 超过 MAX_HTML_LEN 丢弃（只留纯文本，防止 app.db 暴涨）
/// - 相同内容（非置顶）已存在 → 刷新 updated_at/source_app 挪到最前，不新增重复条目
/// - 写入后执行保留策略清理
pub fn insert(conn: &Connection, content: &str, html: Option<&str>, source: Option<&str>) -> Result<()> {
    let truncated = content.chars().take(MAX_ITEM_LEN).collect::<String>();
    let html = if content.len() == truncated.len() {
        html.and_then(|h| {
            if h.len() > MAX_HTML_LEN {
                None
            } else {
                Some(h.to_string())
            }
        })
    } else {
        None
    };
    insert_typed(conn, "text", &truncated, html.as_deref(), None, None, source).map(|_| ())
}

/// 记录图片：`dedup_key` 为图片字节哈希（用于「相同图片只留一条」去重），
/// `image_path` 为快照落盘路径。content 列复用为去重键，前端展示走缩略图。
/// 返回 true 表示新插入，false 表示命中去重（调用方可据此删除本次落盘的冗余快照）。
pub fn insert_image(
    conn: &Connection,
    dedup_key: &str,
    image_path: &str,
    source: Option<&str>,
) -> Result<bool> {
    insert_typed(conn, "image", dedup_key, None, Some(image_path), None, source)
}

/// 记录文件列表：`file_paths` 为原始路径（不拷贝文件内容），content 列存路径连接便于搜索与去重。
pub fn insert_files(conn: &Connection, file_paths: &[String], source: Option<&str>) -> Result<()> {
    let content = file_paths.join("\n");
    let json = serde_json::to_string(file_paths).unwrap_or_default();
    insert_typed(conn, "file", &content, None, None, Some(&json), source).map(|_| ())
}

/// 按类型入库的统一实现：content 作为去重键，kind 隔离各类型命名空间，
/// 相同 content（非置顶）已存在 → 刷新时间与来源挪到最前，不新增重复条目。
/// 返回 true 表示新插入，false 表示去重更新。
fn insert_typed(
    conn: &Connection,
    kind: &str,
    content: &str,
    html: Option<&str>,
    image_path: Option<&str>,
    file_paths: Option<&str>,
    source: Option<&str>,
) -> Result<bool> {
    let ts = now();

    let recent_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM clipboard_history
             WHERE content = ?1 AND kind = ?2 AND is_pinned = 0
             ORDER BY updated_at DESC, id DESC LIMIT 1",
            params![content, kind],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = recent_id {
        conn.execute(
            "UPDATE clipboard_history SET updated_at = ?1, source_app = COALESCE(?2, source_app) WHERE id = ?3",
            params![ts, source, id],
        )?;
        cleanup_throttled(conn);
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO clipboard_history (content, html, source_app, kind, image_path, file_paths, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![content, html, source, kind, image_path, file_paths, ts],
        )?;
        cleanup_throttled(conn);
        Ok(true)
    }
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

    // 删除前先收集将被清掉的图片快照路径，联动删除磁盘文件（防孤儿快照泄漏）
    let mut doomed: Vec<String> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history
         WHERE is_pinned = 0 AND updated_at < ?1 AND kind = 'image' AND image_path IS NOT NULL",
    )?;
    for p in stmt.query_map(params![cutoff], |r| r.get::<_, String>(0))? {
        doomed.push(p?);
    }
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history WHERE id IN (
           SELECT id FROM clipboard_history
           ORDER BY updated_at DESC, id DESC
           LIMIT -1 OFFSET ?1
         ) AND kind = 'image' AND image_path IS NOT NULL",
    )?;
    for p in stmt.query_map(params![max_items], |r| r.get::<_, String>(0))? {
        doomed.push(p?);
    }

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

    for p in doomed {
        let _ = std::fs::remove_file(&p);
    }
    Ok(())
}

/// 置顶优先，其次最近复制；可选关键字搜索（内容/来源应用）
pub fn list(conn: &Connection, keyword: Option<&str>, limit: i64) -> Result<Vec<ClipboardItem>> {
    let kw = keyword.map(|k| k.trim()).filter(|k| !k.is_empty());
    let mut sql = String::from(
        "SELECT id, content, html, source_app, is_pinned, kind, image_path, file_paths, created_at, updated_at
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
        "SELECT id, content, html, source_app, is_pinned, kind, image_path, file_paths, created_at, updated_at
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

/// 所有图片快照路径（清空历史时联动删除磁盘文件，避免孤儿快照泄漏）
pub fn image_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT image_path FROM clipboard_history WHERE kind = 'image' AND image_path IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |r| r.get(0))
}

fn row_to_item(row: &rusqlite::Row) -> Result<ClipboardItem> {
    let file_paths: Vec<String> = row
        .get::<_, Option<String>>(7)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(ClipboardItem {
        id: row.get(0)?,
        content: row.get(1)?,
        html: row.get(2)?,
        source_app: row.get(3)?,
        is_pinned: row.get(4)?,
        kind: row.get(5)?,
        image_path: row.get(6)?,
        file_paths,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
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

    #[test]
    fn insert_image_and_files_roundtrip() {
        let conn = setup();
        insert_image(&conn, "aabbccdd11223344", "C:/x-hub/clipboard/images/1.png", Some("画图")).unwrap();
        insert_files(
            &conn,
            &["C:/a.txt".to_string(), "C:/b.pdf".to_string()],
            Some("资源管理器"),
        )
        .unwrap();
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all.len(), 2);
        // 后插入的文件在最前
        assert_eq!(all[0].kind, "file");
        assert_eq!(
            all[0].file_paths,
            vec!["C:/a.txt".to_string(), "C:/b.pdf".to_string()]
        );
        assert_eq!(all[0].content, "C:/a.txt\nC:/b.pdf");
        assert_eq!(all[1].kind, "image");
        assert_eq!(all[1].image_path.as_deref(), Some("C:/x-hub/clipboard/images/1.png"));
        assert_eq!(all[1].content, "aabbccdd11223344");
    }

    #[test]
    fn image_dedup_refreshes_not_inserts() {
        let conn = setup();
        insert_image(&conn, "samehash", "C:/img/a.png", Some("A")).unwrap();
        insert_image(&conn, "samehash", "C:/img/b.png", Some("B")).unwrap();
        assert_eq!(count(&conn).unwrap(), 1, "相同图片哈希不新增条目");
        let all = list(&conn, None, 50).unwrap();
        assert_eq!(all[0].source_app.as_deref(), Some("B"));
        assert_eq!(all[0].image_path.as_deref(), Some("C:/img/a.png"));
    }
}
