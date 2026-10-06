use crate::models::Note;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 笔记数据访问（速记改造，docs/speednote-plan.md）：
/// - 列表口径固定「创建时间正序」（Q13–Q18），deleted_at IS NULL 的才是活笔记；
/// - delete 语义 = 软删（写 deleted_at 进回收站），硬删走 purge；
/// - folder_id / source_url 一次建行（create_in），剪藏与「首行成标题」共用。

const NOTE_COLS: &str =
    "id, title, content, created_at, updated_at, folder_id, source_url, deleted_at, icon";

pub fn create(conn: &Connection, title: &str) -> Result<Note> {
    create_in(conn, title, "", None, "")
}

/// 一次性带 folder/source 建行：剪藏（create_in + 打标签）与新建落当前文件夹共用，
/// 也修掉旧「先建行再 update 补正文」两步写法在中间态丢正文的窗口（缺陷②）。
pub fn create_in(
    conn: &Connection,
    title: &str,
    content: &str,
    folder_id: Option<i64>,
    source_url: &str,
) -> Result<Note> {
    if let Some(fid) = folder_id {
        let known: i64 = conn.query_row(
            "SELECT COUNT(*) FROM note_folders WHERE id = ?1",
            params![fid],
            |r| r.get(0),
        )?;
        if known == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 目标文件夹 {fid} 不存在"
            )));
        }
    }
    let ts = now();
    conn.execute(
        "INSERT INTO notes (title, content, folder_id, source_url, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![title, content, folder_id, source_url, ts],
    )?;
    let id = conn.last_insert_rowid();
    crate::repo::note_link::reindex(conn, id, content)?;
    get(conn, id)
}

pub fn get(conn: &Connection, id: i64) -> Result<Note> {
    conn.query_row(
        &format!("SELECT {NOTE_COLS} FROM notes WHERE id = ?1"),
        params![id],
        row_to_note,
    )
}

/// 全量活笔记（不含回收站）。列表本身不规定展示顺序，排序口径由各消费方自定
/// （速记视图创建正序 / 概览卡更新倒序）。
pub fn list(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 笔记列表（仅元信息，不拉正文）：外部浮层保存速记后主窗刷新列表用。
pub fn list_meta(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, '', created_at, updated_at, folder_id, source_url, deleted_at, icon
         FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 全量笔记（含正文，trashed=true 时为回收站）：速记视图一次性拉全量、前端切文件夹/
/// 搜索本地过滤（≤3000 条上限），回收站界面也吃这一份。列表口径：创建时间正序，
/// 旧的在上（Q17 按文档既定正序实现）。
pub fn list_full(conn: &Connection, trashed: bool) -> Result<Vec<Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes WHERE deleted_at {} ORDER BY created_at ASC, id ASC",
        if trashed { "IS NOT NULL" } else { "IS NULL" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_note)?;
    rows.collect()
}

/// 回收站全量（含正文），还原/永久删除界面用。
pub fn list_trashed(conn: &Connection) -> Result<Vec<Note>> {
    list_full(conn, true)
}

pub fn update(conn: &Connection, id: i64, title: &str, content: &str) -> Result<Note> {
    let affected = conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now(), id],
    )?;
    if affected == 0 {
        // 带 NOT_FOUND 前缀：扩展桥调用方据此与服务器错误区分，不再盲目重试
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    crate::repo::note_link::reindex(conn, id, content)?;
    get(conn, id)
}

/// 改名感知的更新：标题变化时**同一事务**内做两件事（双链的改名断链防护，方案 §7 坑①）——
/// ① 全库引用替换 `[[旧标题]] → [[新标题]]` 并同步 note_links.to_title；
/// ② 本笔记出链按新正文重建。
/// 失败整体回滚，不留「标题改了、引用没跟上」的半套状态。
pub fn update_with_link_fixup(
    conn: &Connection,
    id: i64,
    title: &str,
    content: &str,
) -> Result<Note> {
    let tx = conn.unchecked_transaction()?;
    let old: String = tx
        .query_row("SELECT title FROM notes WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|_| {
            rusqlite::Error::InvalidParameterName(format!("NOT_FOUND: 笔记 {id} 不存在"))
        })?;
    let ts = now();
    let affected = tx.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, ts, id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    let mut touched: Vec<i64> = vec![id];
    if old != title {
        // 全库正文替换（只动引用语法字面量，LIKE 预过滤避免全表盲写），
        // 与 note_links.to_title 同事务——改名断链防护（方案 §7 坑①）
        tx.execute(
            "UPDATE notes SET content = replace(content, ?1, ?2)
             WHERE content LIKE '%' || ?1 || '%' AND id <> ?3",
            params![format!("[[{old}]]"), format!("[[{title}]]"), id],
        )?;
        tx.execute(
            "UPDATE note_links SET to_title = ?1 WHERE to_title = ?2",
            params![title, old],
        )?;
        // 被替换过正文的笔记出链要按新正文重建（此时标题已落库，新标题可解析）
        let mut stmt = tx.prepare(
            "SELECT id FROM notes WHERE content LIKE '%' || ?1 || '%' AND id <> ?2",
        )?;
        let rows = stmt.query_map(params![format!("[[{title}]]"), id], |r| r.get(0))?;
        for nid in rows {
            touched.push(nid?);
        }
    }
    // 出链重建（含本篇；读的是替换后的最终正文）
    for nid in touched {
        let body: String = tx
            .query_row("SELECT content FROM notes WHERE id = ?1", params![nid], |r| r.get(0))?;
        crate::repo::note_link::reindex(&tx, nid, &body)?;
    }
    tx.commit()?;
    get(conn, id)
}

/// 移动单条笔记到文件夹（None = 树根）
pub fn set_folder(conn: &Connection, id: i64, folder_id: Option<i64>) -> Result<()> {
    if let Some(fid) = folder_id {
        let known: i64 = conn.query_row(
            "SELECT COUNT(*) FROM note_folders WHERE id = ?1",
            params![fid],
            |r| r.get(0),
        )?;
        if known == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 文件夹 {fid} 不存在"
            )));
        }
    }
    let affected = conn.execute(
        "UPDATE notes SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![folder_id, now(), id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    Ok(())
}

/// 设置/清除笔记自定义树图标（emoji；None = 恢复默认文件图标）
pub fn set_icon(conn: &Connection, id: i64, icon: Option<&str>) -> Result<()> {
    let icon = icon.map(str::trim).filter(|s| !s.is_empty());
    let affected = conn.execute(
        "UPDATE notes SET icon = ?1 WHERE id = ?2",
        params![icon, id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    Ok(())
}

/// 软删：移入回收站（写 deleted_at）。已删除的笔记重复调用静默幂等。
pub fn trash(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute(
        "UPDATE notes SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now(), id],
    )?;
    if affected == 0 && get(conn, id).is_err() {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    Ok(())
}

pub fn restore(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute(
        "UPDATE notes SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 回收站中不存在笔记 {id}"
        )));
    }
    Ok(())
}

/// 硬删（回收站永久删除 / 扩展桥 delete 语义保持「删干净」）。
/// note_tags 经外键级联清除；note_links 出链级联、入链置 NULL 保留「未链接提及」。
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    let affected = conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 笔记 {id} 不存在"
        )));
    }
    Ok(())
}

/// 按保留天数清理回收站（retention_days <= 0 = 永久保留，直接返回 0）。
/// 返回清理条数。启动时与设置变更时各跑一次。
pub fn purge_expired(conn: &Connection, retention_days: i64) -> Result<usize> {
    if retention_days <= 0 {
        return Ok(0);
    }
    let n = conn.execute(
        "DELETE FROM notes
         WHERE deleted_at IS NOT NULL
           AND julianday(deleted_at) <= julianday('now') - ?1",
        params![retention_days],
    )?;
    Ok(n)
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Note>> {
    let pattern = format!("%{}%", keyword);
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM notes
         WHERE deleted_at IS NULL AND (title LIKE ?1 OR content LIKE ?1)
         ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map(params![pattern], row_to_note)?;
    rows.collect()
}

/// 全量正文扫描（孤儿图片 GC 用）：含回收站——活引用必须把回收站内笔记也算进去，
/// 否则「先清孤儿、再还原笔记」图片就没了（方案 §10 风险 4）。
pub fn all_contents_including_trashed(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT content FROM notes")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn row_to_note(row: &rusqlite::Row) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        folder_id: row.get(5)?,
        source_url: row.get(6)?,
        deleted_at: row.get(7)?,
        icon: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_get_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "待办事项").unwrap();
        assert_eq!(n.title, "待办事项");
        assert_eq!(n.content, "");
    }

    #[test]
    fn list_notes_ordered_by_updated_desc() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        update(&conn, a.id, "A", "updated later").unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a.id, b.id]);
    }

    #[test]
    fn update_note_title_and_content() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        let updated = update(&conn, n.id, "新标题", "这是内容").unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.content, "这是内容");
    }

    #[test]
    fn trash_restore_and_list_split() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        trash(&conn, a.id).unwrap();
        // 活列表只剩 B；回收站只有 A
        assert_eq!(list(&conn).unwrap().iter().map(|n| n.id).collect::<Vec<_>>(), vec![b.id]);
        let trashed = list_trashed(&conn).unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].id, a.id);
        assert!(trashed[0].deleted_at.is_some());
        // 还原回活列表
        restore(&conn, a.id).unwrap();
        assert!(list_trashed(&conn).unwrap().is_empty());
        assert_eq!(list(&conn).unwrap().len(), 2);
        // 永久删除真正删行
        trash(&conn, b.id).unwrap();
        delete(&conn, b.id).unwrap();
        assert!(get(&conn, b.id).is_err());
    }

    #[test]
    fn purge_expired_respects_retention_days() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A").unwrap();
        let b = create(&conn, "B").unwrap();
        trash(&conn, a.id).unwrap();
        // 人为把 A 的删除时间拨到 10 天前
        conn.execute(
            "UPDATE notes SET deleted_at = datetime('now', '-10 days') WHERE id = ?1",
            params![a.id],
        )
        .unwrap();
        trash(&conn, b.id).unwrap();
        // 0 = 永久保留
        assert_eq!(purge_expired(&conn, 0).unwrap(), 0);
        // 保留 30 天：谁都不清
        assert_eq!(purge_expired(&conn, 30).unwrap(), 0);
        assert_eq!(list_trashed(&conn).unwrap().len(), 2);
        // 保留 7 天：只清掉 10 天前删的 A
        assert_eq!(purge_expired(&conn, 7).unwrap(), 1);
        assert!(get(&conn, a.id).is_err());
        assert!(get(&conn, b.id).is_ok());
    }

    #[test]
    fn create_in_sets_folder_and_rejects_missing_folder() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::note_folder::create(&conn, "剪藏外", None).unwrap();
        let n = create_in(&conn, "t", "正文", Some(f.id), "https://x").unwrap();
        assert_eq!(n.folder_id, Some(f.id));
        assert_eq!(n.source_url, "https://x");
        assert!(create_in(&conn, "t", "", Some(999), "").is_err());
        // 根部创建
        let root_note = create_in(&conn, "r", "", None, "").unwrap();
        assert_eq!(root_note.folder_id, None);
    }

    #[test]
    fn list_full_orders_created_asc_and_splits_trash() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::note_folder::create(&conn, "F", None).unwrap();
        let a = create_in(&conn, "根1", "", None, "").unwrap();
        let b = create_in(&conn, "根2", "", None, "").unwrap();
        let c = create_in(&conn, "夹内", "", Some(f.id), "").unwrap();
        // 全量（跨文件夹，前端过滤的数据源）
        let all = list_full(&conn, false).unwrap();
        assert_eq!(all.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a.id, b.id, c.id]);
        // 创建时间正序（旧的在上）
        assert_eq!(all[0].id, a.id);
        // 回收站的从活列表消失
        trash(&conn, a.id).unwrap();
        assert_eq!(list_full(&conn, false).unwrap().len(), 2);
        assert_eq!(list_full(&conn, true).unwrap().len(), 1);
    }

    #[test]
    fn set_folder_moves_note() {
        let conn = init_in_memory().unwrap();
        let f = crate::repo::note_folder::create(&conn, "F", None).unwrap();
        let n = create(&conn, "N").unwrap();
        set_folder(&conn, n.id, Some(f.id)).unwrap();
        assert_eq!(get(&conn, n.id).unwrap().folder_id, Some(f.id));
        set_folder(&conn, n.id, None).unwrap();
        assert_eq!(get(&conn, n.id).unwrap().folder_id, None);
        assert!(set_folder(&conn, n.id, Some(999)).is_err());
    }

    #[test]
    fn search_excludes_trashed() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "购物清单").unwrap();
        let second = create(&conn, "会议记录").unwrap();
        update(&conn, second.id, "会议记录", "讨论了发布计划").unwrap();
        let by_title = search(&conn, "购物").unwrap();
        assert_eq!(by_title.len(), 1);
        let by_content = search(&conn, "发布计划").unwrap();
        assert_eq!(by_content.len(), 1);
        assert_eq!(by_content[0].id, second.id);
        trash(&conn, a.id).unwrap();
        assert!(search(&conn, "购物").unwrap().is_empty());
    }

    #[test]
    fn delete_note() {
        let conn = init_in_memory().unwrap();
        let n = create(&conn, "T").unwrap();
        delete(&conn, n.id).unwrap();
        assert!(get(&conn, n.id).is_err());
    }

    #[test]
    fn hard_delete_clears_links_but_keeps_unresolved_mentions() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "甲").unwrap();
        let b = create(&conn, "乙").unwrap();
        update(&conn, b.id, "乙", "见 [[甲]] 与 [[丙]]").unwrap();
        // 删除被引用的甲：入链转「未链接提及」，不报错
        delete(&conn, a.id).unwrap();
        let (outgoing, incoming) = crate::repo::note_link::for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing.len(), 2);
        assert!(incoming.is_empty());
        // 删除引用方：出链随 CASCADE 消失
        delete(&conn, b.id).unwrap();
        let (outgoing, _) = crate::repo::note_link::for_note(&conn, b.id).unwrap_or_default();
        assert!(outgoing.is_empty());
    }

    #[test]
    fn update_with_link_fixup_replaces_references() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "甲").unwrap();
        let b = create(&conn, "乙").unwrap();
        let _ = update(&conn, b.id, "乙", "见 [[甲]]");
        // 改名甲 → 甲乙：乙的正文与链索引同事务跟上
        update_with_link_fixup(&conn, a.id, "甲乙", "正文甲乙").unwrap();
        let nb = get(&conn, b.id).unwrap();
        assert_eq!(nb.content, "见 [[甲乙]]");
        let (_, incoming) = crate::repo::note_link::for_note(&conn, a.id).unwrap();
        assert_eq!(incoming.len(), 1);
        // 不改名时不做全库替换
        let before = get(&conn, b.id).unwrap().content;
        update_with_link_fixup(&conn, a.id, "甲乙", "正文又改了").unwrap();
        assert_eq!(get(&conn, b.id).unwrap().content, before);
    }
}
