use rusqlite::{params, Connection, Result};

/// 双链轻量版（docs/speednote-plan.md §7）：语法 `[[标题]]`，引用键 = 标题、不落块 id。
/// 索引是**派生数据**：写入笔记后解析正文重建该笔记的出链（单笔记粒度，随 save 全量重建）；
/// 引用不存在的标题保留为「未链接提及」（to_note_id NULL），不算错。
/// 块引用 `((block-id))` 明确不做（ADR 0013：要动 schema、污染 md 产物）。

use crate::models::{NoteLinkIn, NoteLinkOut};

/// 解析正文里的 `[[...]]` 引用标题（按出现顺序，同标题去重）。
/// 手写字符扫描（本仓不引 regex）：`[[` 与其后第一个 `]]` 之间、不含换行与方括号的文本即引用。
pub fn parse_link_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut titles = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = content[i + 2..].find("]]") {
                let raw = &content[i + 2..i + 2 + end];
                let title = raw.trim();
                if !title.is_empty()
                    && !title.contains('[')
                    && !title.contains(']')
                    && !title.contains('\n')
                {
                    if !titles.iter().any(|t| t == title) {
                        titles.push(title.to_string());
                    }
                    i = i + 2 + end + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    titles
}

/// 重建某条笔记的出链索引（全量替换式）。保存路径每次全量重建，单笔记粒度成本可忽略
/// （≤3000 条上限、单条 ≤200KB）。
pub fn reindex(conn: &Connection, note_id: i64, content: &str) -> Result<()> {
    let titles = parse_link_titles(content);
    conn.execute(
        "DELETE FROM note_links WHERE from_note_id = ?1",
        params![note_id],
    )?;
    for title in titles {
        // 同名笔记取 id 最小的那篇（引用键是标题，重名是罕见但允许的状态）
        let target: Option<i64> = conn
            .query_row(
                "SELECT MIN(id) FROM notes WHERE title = ?1 AND deleted_at IS NULL",
                params![title],
                |r| r.get(0),
            )
            .unwrap_or(None);
        conn.execute(
            "INSERT OR IGNORE INTO note_links (from_note_id, to_note_id, to_title) VALUES (?1, ?2, ?3)",
            params![note_id, target, title],
        )?;
    }
    Ok(())
}

/// 反链面板数据：outgoing = 本笔记的出链（含未解析提及），incoming = 引用本笔记的活笔记。
pub fn for_note(
    conn: &Connection,
    note_id: i64,
) -> Result<(Vec<NoteLinkOut>, Vec<NoteLinkIn>)> {
    let mut outgoing = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT to_title, to_note_id FROM note_links
             WHERE from_note_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt.query_map(params![note_id], |row| {
            Ok(NoteLinkOut {
                to_title: row.get(0)?,
                to_note_id: row.get(1)?,
            })
        })?;
        for row in rows {
            outgoing.push(row?);
        }
    }
    let mut incoming = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT l.from_note_id, n.title FROM note_links l
             JOIN notes n ON n.id = l.from_note_id
             WHERE l.to_note_id = ?1 AND n.deleted_at IS NULL
             ORDER BY n.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![note_id], |row| {
            Ok(NoteLinkIn {
                from_note_id: row.get(0)?,
                from_title: row.get(1)?,
            })
        })?;
        for row in rows {
            incoming.push(row?);
        }
    }
    Ok((outgoing, incoming))
}

/// 提供反链查询能力前先把存量老笔记建一遍索引（升级后首次启动/首次打开速记视图时跑）。
pub fn reindex_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM note_links", [])?;
    let mut stmt = conn.prepare("SELECT id, content FROM notes WHERE deleted_at IS NULL")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>>>()?;
    for (id, content) in rows {
        reindex(conn, id, &content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn parse_extracts_titles_in_order_and_dedupes() {
        let t = parse_link_titles("见 [[甲]] 与 [[乙]]、再提 [[甲]]；非引用 [[a\nb]] 与 [[]]");
        assert_eq!(t, vec!["甲".to_string(), "乙".to_string()]);
        // 嵌套方括号 / 跨行 / 空白标题都不算
        assert!(parse_link_titles("[[嵌[套]]").is_empty());
        assert!(parse_link_titles("[[跨\n行]]").is_empty());
        // 未闭合不成链
        assert!(parse_link_titles("开头 [[未闭合").is_empty());
    }

    #[test]
    fn reindex_resolves_existing_titles_and_keeps_mentions() {
        let conn = init_in_memory().unwrap();
        let a = crate::repo::note::create(&conn, "甲").unwrap();
        let b = crate::repo::note::create(&conn, "乙").unwrap();
        crate::repo::note::update(&conn, b.id, "乙", "见 [[甲]] 与 [[丙]]").unwrap();
        let (outgoing, _incoming) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing.len(), 2);
        assert_eq!(
            outgoing.iter().find(|l| l.to_title == "甲").unwrap().to_note_id,
            Some(a.id)
        );
        assert_eq!(
            outgoing.iter().find(|l| l.to_title == "丙").unwrap().to_note_id,
            None,
            "未解析提及保留"
        );
        let (_, incoming_of_a) = for_note(&conn, a.id).unwrap();
        assert_eq!(incoming_of_a.len(), 1);
        assert_eq!(incoming_of_a[0].from_note_id, b.id);
        // 重建幂等：清掉引用后索引归零
        crate::repo::note::update(&conn, b.id, "乙", "没有引用了").unwrap();
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert!(outgoing.is_empty());
        let (_, incoming_of_a) = for_note(&conn, a.id).unwrap();
        assert!(incoming_of_a.is_empty());
    }

    #[test]
    fn trashed_notes_are_not_link_targets() {
        let conn = init_in_memory().unwrap();
        let a = crate::repo::note::create(&conn, "甲").unwrap();
        let b = crate::repo::note::create(&conn, "乙").unwrap();
        crate::repo::note::update(&conn, b.id, "乙", "见 [[甲]]").unwrap();
        crate::repo::note::trash(&conn, a.id).unwrap();
        // 引用方保存时重建索引：回收站里的标题不再解析
        crate::repo::note::update(&conn, b.id, "乙", "见 [[甲]] 再次").unwrap();
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing[0].to_note_id, None);
        // 还原后重建即恢复解析
        crate::repo::note::restore(&conn, a.id).unwrap();
        crate::repo::note::update(&conn, b.id, "乙", "见 [[甲]] 第三次").unwrap();
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing[0].to_note_id, Some(a.id));
    }
}
