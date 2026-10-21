use rusqlite::{params, Connection, Result};

/// 双链（docs/speednote-plan.md §7 / ADR 0013）：引用语法已从 `[[标题]]` 演进为
/// 标准 Markdown 链接 `[标题](note/<id>)`（引用选择器插入的正式形态），旧式 `[[标题]]`
/// 仍兼容解析。索引是**派生数据**：写入笔记后解析正文重建该笔记的出链（单笔记粒度，
/// 随 save 全量重建）；解析不到目标保留为「未链接提及」（to_note_id NULL），不算错。
/// 块引用 `((block-id))` 明确不做（要动 schema、污染 md 产物）。

use crate::models::{NoteLinkIn, NoteLinkOut};

/// 一条正文引用：`title` = 文中显示的标题；`note_id` = 已知目标 id（新式链接携带），
/// None 表示按标题解析（旧式 `[[标题]]`）。
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRef {
    pub title: String,
    pub note_id: Option<i64>,
}

/// 解析正文引用（按出现顺序、去重）。手写字符扫描，本仓不引 regex。
/// 支持两种写法：
/// ① 新式 Markdown 链接 `[标题](note/<id>)`：引用选择器插入的正式形态，**携带目标 id**，
///    目标改名后仍指向同一篇（无需重写正文即可保留链）；
/// ② 旧式 `[[标题]]`：历史内容，按标题解析（目标不存在则为未链接提及）。
pub fn parse_refs(content: &str) -> Vec<NoteRef> {
    let mut refs: Vec<NoteRef> = Vec::new();

    // ① [标题](note/<id>)
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = content[i..].find("](note/") {
        let close = i + rel;
        let id_start = close + "](note/".len();
        let mut j = id_start;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > id_start && j < bytes.len() && bytes[j] == b')' {
            if let Ok(id) = content[id_start..j].parse::<i64>() {
                if let Some(open) = content[..close].rfind('[') {
                    let title = content[open + 1..close].trim();
                    if valid_title(title) && !refs.iter().any(|r| r.note_id == Some(id)) {
                        refs.push(NoteRef {
                            title: title.to_string(),
                            note_id: Some(id),
                        });
                    }
                }
            }
            i = j + 1;
            continue;
        }
        i = close + 1;
    }

    // ② [[标题]]（历史内容）：复用旧式扫描，转成 NoteRef
    for title in parse_link_titles(content) {
        if !refs.iter().any(|r| r.note_id.is_none() && r.title == title) {
            refs.push(NoteRef {
                title,
                note_id: None,
            });
        }
    }

    refs
}

fn valid_title(title: &str) -> bool {
    !title.is_empty() && !title.contains('[') && !title.contains(']') && !title.contains('\n')
}

/// 旧式 `[[...]]` 标题解析（保留给单测与历史语义）：`[[` 与其后第一个 `]]` 之间、
/// 不含换行与方括号的文本即引用（按出现顺序，同标题去重）。
pub fn parse_link_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut titles = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = content[i + 2..].find("]]") {
                let raw = &content[i + 2..i + 2 + end];
                let title = raw.trim();
                if valid_title(title) {
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
    let refs = parse_refs(content);
    conn.execute(
        "DELETE FROM note_links WHERE from_note_id = ?1",
        params![note_id],
    )?;
    for r in refs {
        let target: Option<i64> = match r.note_id {
            // 新式链接自带 id：目标活着就用它（改名不改链），否则降级为未链接提及
            Some(id) => {
                let alive: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM notes WHERE id = ?1 AND deleted_at IS NULL",
                    params![id],
                    |row| row.get(0),
                )?;
                if alive > 0 {
                    Some(id)
                } else {
                    None
                }
            }
            // 旧式按标题解析：同名笔记取 id 最小的那篇（重名是罕见但允许的状态）
            None => conn
                .query_row(
                    "SELECT MIN(id) FROM notes WHERE title = ?1 AND deleted_at IS NULL",
                    params![r.title],
                    |row| row.get(0),
                )
                .unwrap_or(None),
        };
        // 目标标题以库中最新为准（id 链接改名后无需重写引用正文即显示新标题）
        let to_title: String = match target {
            Some(id) => conn
                .query_row("SELECT title FROM notes WHERE id = ?1", params![id], |row| {
                    row.get(0)
                })
                .unwrap_or_else(|_| r.title.clone()),
            None => r.title.clone(),
        };
        conn.execute(
            "INSERT OR IGNORE INTO note_links (from_note_id, to_note_id, to_title) VALUES (?1, ?2, ?3)",
            params![note_id, target, to_title],
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
            "SELECT COALESCE(n.title, l.to_title), l.to_note_id FROM note_links l
             LEFT JOIN notes n ON n.id = l.to_note_id
             WHERE l.from_note_id = ?1 ORDER BY l.rowid",
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

    #[test]
    fn parse_refs_reads_markdown_links_with_id_and_legacy() {
        let refs = parse_refs("见 [甲](note/1) 与 [乙 标题](note/2)，再来 [甲](note/1)");
        assert_eq!(
            refs,
            vec![
                NoteRef {
                    title: "甲".to_string(),
                    note_id: Some(1)
                },
                NoteRef {
                    title: "乙 标题".to_string(),
                    note_id: Some(2)
                },
            ]
        );
        let refs = parse_refs("[[旧]] 和 [新](note/9)");
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|r| r.note_id == Some(9) && r.title == "新"));
        assert!(refs.iter().any(|r| r.note_id.is_none() && r.title == "旧"));
    }

    #[test]
    fn reindex_markdown_link_survives_rename_and_trash() {
        let conn = init_in_memory().unwrap();
        let a = crate::repo::note::create(&conn, "甲").unwrap();
        let b = crate::repo::note::create(&conn, "乙").unwrap();
        crate::repo::note::update(&conn, b.id, "乙", &format!("见 [甲](note/{})", a.id)).unwrap();
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing[0].to_note_id, Some(a.id));
        assert_eq!(outgoing[0].to_title, "甲");
        // 目标改名：正文里链接显示文本被同步，仍指向同一篇
        crate::repo::note::update_with_link_fixup(&conn, a.id, "甲改", "正文").unwrap();
        let body: String = conn
            .query_row("SELECT content FROM notes WHERE id = ?1", params![b.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(body, format!("见 [甲改](note/{})", a.id));
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing[0].to_note_id, Some(a.id));
        assert_eq!(outgoing[0].to_title, "甲改");
        // 目标进回收站 → 降级为未链接提及
        crate::repo::note::trash(&conn, a.id).unwrap();
        crate::repo::note::update(&conn, b.id, "乙", &format!("见 [甲改](note/{}) 再次", a.id))
            .unwrap();
        let (outgoing, _) = for_note(&conn, b.id).unwrap();
        assert_eq!(outgoing[0].to_note_id, None);
    }
}
