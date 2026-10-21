use crate::models::NoteFolder;
use rusqlite::{params, Connection, Result};

/// 笔记文件夹树（ADR 0015：单归属、可嵌套不限深度、删除不级联——子文件夹与笔记
/// 在同一事务内上移一级，绝不满门抄斩；builtin=1 的「剪藏」不可改名/删除）。
/// 排序口径（docs/speednote-plan.md §3）：同层文件夹按 sort_order（同值回退 created_at）。
/// 树由前端按 parent_id 组装，后端只给平铺列表。

fn row_to_folder(row: &rusqlite::Row) -> Result<NoteFolder> {
    Ok(NoteFolder {
        id: row.get(0)?,
        name: row.get(1)?,
        parent_id: row.get(2)?,
        sort_order: row.get(3)?,
        builtin: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
    })
}

const FOLDER_COLS: &str = "id, name, parent_id, sort_order, builtin, created_at";

pub fn list(conn: &Connection) -> Result<Vec<NoteFolder>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FOLDER_COLS} FROM note_folders
         ORDER BY parent_id IS NOT NULL, sort_order ASC, created_at ASC, id ASC"
    ))?;
    let rows = stmt.query_map([], row_to_folder)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: i64) -> Result<NoteFolder> {
    conn.query_row(
        &format!("SELECT {FOLDER_COLS} FROM note_folders WHERE id = ?1"),
        params![id],
        row_to_folder,
    )
}

pub fn exists(conn: &Connection, id: i64) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM note_folders WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn create(conn: &Connection, name: &str, parent_id: Option<i64>) -> Result<NoteFolder> {
    if let Some(pid) = parent_id {
        if !exists(conn, pid)? {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 目标文件夹 {pid} 不存在"
            )));
        }
    }
    // 同层追加到末尾（同名允许，见方案 Q13–Q18）
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM note_folders
         WHERE parent_id IS ?1",
        params![parent_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO note_folders (name, parent_id, sort_order) VALUES (?1, ?2, ?3)",
        params![name, parent_id, next],
    )?;
    get(conn, conn.last_insert_rowid())
}

/// 重命名。builtin（「剪藏」）拒绝，报错带 FORBIDDEN_BUILTIN: 前缀供前端识别。
pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<()> {
    ensure_not_builtin(conn, id)?;
    let affected = conn.execute(
        "UPDATE note_folders SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    if affected == 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "NOT_FOUND: 文件夹 {id} 不存在"
        )));
    }
    Ok(())
}

fn ensure_not_builtin(conn: &Connection, id: i64) -> Result<()> {
    let builtin: i64 = conn.query_row(
        "SELECT builtin FROM note_folders WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if builtin != 0 {
        return Err(rusqlite::Error::InvalidParameterName(
            "FORBIDDEN_BUILTIN: 内置文件夹不可改名或删除".into(),
        ));
    }
    Ok(())
}

/// 删除文件夹：子文件夹与笔记**上移一级**（被删文件夹的 parent；树根文件夹的成员回树根），
/// 不级联删（ADR 0015）。单事务完成。
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    ensure_not_builtin(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    let parent: Option<i64> = tx
        .query_row(
            "SELECT parent_id FROM note_folders WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
    // 成员上移一级
    tx.execute(
        "UPDATE note_folders SET parent_id = ?1 WHERE parent_id = ?2",
        params![parent, id],
    )?;
    tx.execute(
        "UPDATE notes SET folder_id = ?1 WHERE folder_id = ?2",
        params![parent, id],
    )?;
    tx.execute("DELETE FROM note_folders WHERE id = ?1", params![id])?;
    tx.commit()
}

/// 拖拽移动 + 排序的原子写回载荷
#[derive(Debug, Clone)]
pub struct FolderMove {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
}

/// 批量写回文件夹位置（拖拽落点）：必须做**环检测**——文件夹不能被拖进自己或自己的后代，
/// 否则树成环、递归渲染死循环。任一项非法整批拒绝（宁可前端重拉，不留半套新位置）。
pub fn reorder(conn: &Connection, moves: &[FolderMove]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let all: Vec<NoteFolder> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT {FOLDER_COLS} FROM note_folders ORDER BY id"
        ))?;
        let rows = stmt.query_map([], row_to_folder)?;
        rows.collect::<Result<Vec<_>>>()?
    };
    // 以本次写回后的视角逐项校验：parent 链一路向上，不得回到自己
    for mv in moves {
        // 未知 id 不许静默 no-op：前端传错 ID 时当场报错指路
        if !all.iter().any(|f| f.id == mv.id) {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "NOT_FOUND: 文件夹 {} 不存在",
                mv.id
            )));
        }
        if mv.parent_id == Some(mv.id) {
            return Err(rusqlite::Error::InvalidParameterName(
                "CYCLE: 不能把文件夹拖进它自己".into(),
            ));
        }
        if let Some(pid) = mv.parent_id {
            if !all.iter().any(|f| f.id == pid) {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "NOT_FOUND: 目标文件夹 {pid} 不存在"
                )));
            }
        }
        // 从目标 parent 沿 parent_id 向上爬：途中碰到 mv.id 即成环。
        // 链上节点的 parent 取「写回表中该节点的目标值（若在本次批次里），否则取现值」。
        let mut cursor = mv.parent_id;
        let mut hops = 0;
        while let Some(pid) = cursor {
            if pid == mv.id {
                return Err(rusqlite::Error::InvalidParameterName(
                    "CYCLE: 不能把文件夹拖进它自己的子文件夹".into(),
                ));
            }
            hops += 1;
            if hops > all.len() + 1 {
                // 现有数据已含环（理论不可达）：拒绝本批，避免死循环
                return Err(rusqlite::Error::InvalidParameterName(
                    "CYCLE: 文件夹层级数据异常".into(),
                ));
            }
            cursor = moves
                .iter()
                .find(|m| m.id == pid)
                .and_then(|m| m.parent_id)
                .or_else(|| all.iter().find(|f| f.id == pid).and_then(|f| f.parent_id));
        }
    }
    for mv in moves {
        tx.execute(
            "UPDATE note_folders SET parent_id = ?1, sort_order = ?2 WHERE id = ?3",
            params![mv.parent_id, mv.sort_order, mv.id],
        )?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    #[test]
    fn create_and_list_folders() {
        let conn = init_in_memory().unwrap();
        let root = create(&conn, "项目", None).unwrap();
        let child = create(&conn, "子目录", Some(root.id)).unwrap();
        assert_eq!(child.parent_id, Some(root.id));
        // 内置「剪藏」在种子中出现
        let list = list(&conn).unwrap();
        assert!(list.iter().any(|f| f.builtin && f.name == "剪藏"));
        // 同层排序位递增
        let second = create(&conn, "项目2", None).unwrap();
        assert!(second.sort_order > root.sort_order);
    }

    #[test]
    fn create_under_missing_parent_rejected() {
        let conn = init_in_memory().unwrap();
        assert!(create(&conn, "孤儿", Some(999)).is_err());
    }

    #[test]
    fn builtin_folder_protected() {
        let conn = init_in_memory().unwrap();
        let builtin: i64 = conn
            .query_row(
                "SELECT id FROM note_folders WHERE builtin = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let err = rename(&conn, builtin, "改名").unwrap_err().to_string();
        assert!(err.contains("FORBIDDEN_BUILTIN"), "rename err: {err}");
        assert!(delete(&conn, builtin).is_err());
    }

    #[test]
    fn delete_moves_children_up_one_level() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A", None).unwrap();
        let b = create(&conn, "B", Some(a.id)).unwrap();
        let c = create(&conn, "C", Some(b.id)).unwrap();
        let note = crate::repo::note::create_in(&conn, "n", "", Some(b.id), "").unwrap();
        delete(&conn, b.id).unwrap();
        // 子文件夹与笔记都上移到 A（不是树根）
        assert_eq!(get(&conn, c.id).unwrap().parent_id, Some(a.id));
        assert_eq!(
            crate::repo::note::get(&conn, note.id).unwrap().folder_id,
            Some(a.id)
        );
        assert!(get(&conn, b.id).is_err());
    }

    #[test]
    fn delete_root_folder_moves_children_to_root() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A", None).unwrap();
        let child = create(&conn, "B", Some(a.id)).unwrap();
        delete(&conn, a.id).unwrap();
        assert_eq!(get(&conn, child.id).unwrap().parent_id, None);
    }

    #[test]
    fn reorder_rejects_cycles() {
        let conn = init_in_memory().unwrap();
        let a = create(&conn, "A", None).unwrap();
        let b = create(&conn, "B", Some(a.id)).unwrap();
        // 拖进自己
        let err = reorder(
            &conn,
            &[FolderMove { id: a.id, parent_id: Some(a.id), sort_order: 0 }],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("CYCLE"), "err: {err}");
        // 拖进自己的后代（A → B 的下面，B 是 A 的孩子）
        let err = reorder(
            &conn,
            &[FolderMove { id: a.id, parent_id: Some(b.id), sort_order: 0 }],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("CYCLE"), "err: {err}");
        // 同批内形成环也拒绝：A → B 下，同时 B → A 下
        let err = reorder(
            &conn,
            &[
                FolderMove { id: a.id, parent_id: Some(b.id), sort_order: 0 },
                FolderMove { id: b.id, parent_id: Some(a.id), sort_order: 0 },
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("CYCLE"), "err: {err}");
        // 合法移动通过：C 挪到 A 下
        let c = create(&conn, "C", None).unwrap();
        reorder(&conn, &[FolderMove { id: c.id, parent_id: Some(a.id), sort_order: 0 }]).unwrap();
        assert_eq!(get(&conn, c.id).unwrap().parent_id, Some(a.id));
    }
}
