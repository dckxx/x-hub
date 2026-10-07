use crate::models::{RepeatRule, Todo};
use crate::repo::now;
use crate::todo_recurrence;
use rusqlite::{params, Connection, Result};

const COLS: &str =
    "id, title, done, priority, created_at, updated_at, completed_at, due_at, remind_at, remind_fired, parent_id, sort_order, version, description, pinned, repeat_mode, repeat_every, repeat_unit, repeat_weekdays, repeat_month_day, repeat_month_nth, repeat_end_mode, repeat_end_at, repeat_count, repeat_done_count, repeat_last_done_at";

pub fn list(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         ORDER BY done ASC,
           CASE WHEN done = 1 THEN completed_at ELSE created_at END DESC,
           id DESC",
    ))?;
    let rows = stmt.query_map([], row_to_todo)?;
    rows.collect()
}

/// 创建待办；parent_id 非空时创建为该父条目下的子待办（父不存在报错）；
/// created_at 非空时保留原时间戳（删除撤销恢复用，避免恢复项排到最新位置）
pub fn create(
    conn: &Connection,
    title: &str,
    parent_id: Option<i64>,
    created_at: Option<&str>,
) -> Result<Todo> {
    if let Some(pid) = parent_id {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE id = ?1",
            params![pid],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "父待办不存在: {pid}"
            )));
        }
    }
    let ts = created_at
        .map(str::to_owned)
        .unwrap_or_else(now);
    conn.execute(
        "INSERT INTO todos (title, parent_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![title, parent_id, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> Result<Todo> {
    conn.query_row(
        &format!("SELECT {COLS} FROM todos WHERE id = ?1"),
        params![id],
        row_to_todo,
    )
}

pub fn update(conn: &Connection, id: i64, title: &str, priority: i64) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET title = ?1, priority = ?2, updated_at = ?3, version = version + 1 WHERE id = ?4",
        params![title, priority, now(), id],
    )?;
    get(conn, id)
}

/// 设置截止/提醒时刻（毫秒时间戳；NULL 表示清除）。
/// 每次排期都重置 remind_fired，用户改提醒时间后重新武装后台触发。
/// 同时清空手动排序位：截止日期决定分组归属，换组后原顺序语义失效，
/// 该条目回到新组默认位置（创建时间倒序）。
pub fn schedule(
    conn: &Connection,
    id: i64,
    due_at: Option<i64>,
    remind_at: Option<i64>,
) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET due_at = ?1, remind_at = ?2, remind_fired = 0, sort_order = NULL, updated_at = ?3, version = version + 1 WHERE id = ?4",
        params![due_at, remind_at, now(), id],
    )?;
    get(conn, id)
}

/// affected == 0 时区分「记录不存在」与「版本冲突」：若把 NOT_FOUND 误报成 CONFLICT，
/// 同步端会走「刷新重试」而记录根本不存在，永远失败。label 如「待办」。
fn not_found_or_conflict(conn: &Connection, id: i64, label: &str) -> String {
    let exists: Option<i64> = conn
        .query_row("SELECT 1 FROM todos WHERE id = ?1", params![id], |r| r.get(0))
        .ok();
    match exists {
        None => format!("NOT_FOUND: 待办 {id} 不存在"),
        Some(_) => format!("CONFLICT: {label} {id} 已被他人修改，请刷新后重试"),
    }
}

/// 乐观锁更新（局域网同步写回）：仅当记录的 version == expected_version 才更新并 +1，
/// 否则返回 CONFLICT 错误（带 prefix，供桥 handler / 扩展后端识别）。
/// 不校验（expected = None）时等价于普通更新，供主 UI 本地写回使用。
pub fn update_with_version(
    conn: &Connection,
    id: i64,
    title: &str,
    priority: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET title = ?1, priority = ?2, updated_at = ?3, version = version + 1
             WHERE id = ?4 AND (?5 IS NULL OR version = ?5)",
            params![title, priority, now(), id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 乐观锁切换完成状态。expected_version 命中才翻转，否则 CONFLICT。
pub fn toggle_with_version(
    conn: &Connection,
    id: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET
               done = CASE WHEN done = 1 THEN 0 ELSE 1 END,
               completed_at = CASE WHEN done = 1 THEN NULL ELSE strftime('%Y-%m-%d %H:%M:%f','now') END,
               updated_at = ?1, version = version + 1
             WHERE id = ?2 AND (?3 IS NULL OR version = ?3)",
            params![now(), id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 乐观锁删除（子待办级联）。expected_version 命中才删除，否则 CONFLICT；
/// 返回被级联删除的子待办 id（供同步端传播级联删除），与普通 delete 语义一致。
pub fn delete_with_version(
    conn: &Connection,
    id: i64,
    expected_version: Option<i64>,
) -> Result<Vec<i64>, String> {
    // 先记下级联子 id（删除后经 FK ON DELETE CASCADE 消失，无法再查）
    let kids = children_ids(conn, id).map_err(|e| e.to_string())?;
    let affected = conn
        .execute(
            "DELETE FROM todos WHERE id = ?1 AND (?2 IS NULL OR version = ?2)",
            params![id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    Ok(kids)
}

/// 乐观锁排期。expected_version 命中才更新，否则 CONFLICT。
pub fn schedule_with_version(
    conn: &Connection,
    id: i64,
    due_at: Option<i64>,
    remind_at: Option<i64>,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET due_at = ?1, remind_at = ?2, remind_fired = 0, sort_order = NULL,
               updated_at = ?3, version = version + 1
             WHERE id = ?4 AND (?5 IS NULL OR version = ?5)",
            params![due_at, remind_at, now(), id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

pub fn toggle(conn: &Connection, id: i64) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET
           done = CASE WHEN done = 1 THEN 0 ELSE 1 END,
           completed_at = CASE WHEN done = 1 THEN NULL ELSE strftime('%Y-%m-%d %H:%M:%f','now') END,
           updated_at = ?1, version = version + 1
         WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

// ---------- 待办 v1：描述 / 置顶 / 周期 ----------

/// 单条描述最大字符数：超过即截断（防扩展桥/粘贴把几 MB 文本写进 app.db，同剪贴板 MAX_ITEM_LEN 口径）。
/// 入口层（宿主命令 / 扩展桥）会先按这个上限报错，这里是兜底——任何新调用点都不会写爆数据库。
pub const MAX_DESCRIPTION_LEN: usize = 200;

/// 截断到 MAX_DESCRIPTION_LEN 个字符（按字符而非字节，避免截断多字节字符）
fn clamp_description(s: &str) -> String {
    if s.chars().count() <= MAX_DESCRIPTION_LEN {
        return s.to_string();
    }
    s.chars().take(MAX_DESCRIPTION_LEN).collect()
}

/// 设置描述（轻量 Markdown）
pub fn set_description(conn: &Connection, id: i64, description: &str) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET description = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
        params![clamp_description(description), now(), id],
    )?;
    get(conn, id)
}

pub fn set_description_with_version(
    conn: &Connection,
    id: i64,
    description: &str,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET description = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND (?4 IS NULL OR version = ?4)",
            params![clamp_description(description), now(), id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 置顶开关（置顶条目脱离日期分组，固定排在列表最顶部「置顶」区）
pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET pinned = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
        params![pinned as i64, now(), id],
    )?;
    get(conn, id)
}

pub fn set_pinned_with_version(
    conn: &Connection,
    id: i64,
    pinned: bool,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET pinned = ?1, updated_at = ?2, version = version + 1
             WHERE id = ?3 AND (?4 IS NULL OR version = ?4)",
            params![pinned as i64, now(), id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 写入周期规则（整组 repeat_* 列一起写；`repeat_done_count` 是统计量，改规则不清零）
pub fn set_repeat(conn: &Connection, id: i64, rule: &RepeatRule) -> Result<Todo> {
    conn.execute(
        "UPDATE todos SET
           repeat_mode = ?1, repeat_every = ?2, repeat_unit = ?3, repeat_weekdays = ?4,
           repeat_month_day = ?5, repeat_month_nth = ?6, repeat_end_mode = ?7,
           repeat_end_at = ?8, repeat_count = ?9, updated_at = ?10, version = version + 1
         WHERE id = ?11",
        params![
            rule.mode,
            rule.every,
            rule.unit,
            rule.weekdays,
            rule.month_day,
            rule.month_nth,
            rule.end_mode,
            rule.end_at,
            rule.count,
            now(),
            id
        ],
    )?;
    get(conn, id)
}

pub fn set_repeat_with_version(
    conn: &Connection,
    id: i64,
    rule: &RepeatRule,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let affected = conn
        .execute(
            "UPDATE todos SET
               repeat_mode = ?1, repeat_every = ?2, repeat_unit = ?3, repeat_weekdays = ?4,
               repeat_month_day = ?5, repeat_month_nth = ?6, repeat_end_mode = ?7,
               repeat_end_at = ?8, repeat_count = ?9, updated_at = ?10, version = version + 1
             WHERE id = ?11 AND (?12 IS NULL OR version = ?12)",
            params![
                rule.mode,
                rule.every,
                rule.unit,
                rule.weekdays,
                rule.month_day,
                rule.month_nth,
                rule.end_mode,
                rule.end_at,
                rule.count,
                now(),
                id,
                expected_version
            ],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 子待办全部复位为未完成（周期滚动到下一轮时调用，§11 第 6 项）
pub fn reset_children(conn: &Connection, parent_id: i64) -> Result<usize> {
    let n = conn.execute(
        "UPDATE todos SET done = 0, completed_at = NULL, updated_at = ?1, version = version + 1
         WHERE parent_id = ?2 AND done = 1",
        params![now(), parent_id],
    )?;
    Ok(n)
}

/// 周期待办「完成本轮」：`due_at` 滚到下一个未来时刻（不补逾期历史），
/// `remind_at` 按同一偏移平移并重新武装；计数 +1；子待办复位；
/// 非置顶条目清空手动排序位（换组后原顺序失效，与 schedule() 同口径）。
/// 结束条件用尽时把 `repeat_mode` 置回 `once` 并保留该行（§5.3）。
pub fn complete_recurring(
    conn: &Connection,
    id: i64,
    now_ms: i64,
) -> Result<Todo, String> {
    complete_recurring_inner(conn, id, now_ms, None)
}

pub fn complete_recurring_with_version(
    conn: &Connection,
    id: i64,
    now_ms: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    complete_recurring_inner(conn, id, now_ms, expected_version)
}

fn complete_recurring_inner(
    conn: &Connection,
    id: i64,
    now_ms: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let t = get(conn, id).map_err(|e| e.to_string())?;
    if t.repeat_mode == "once" {
        return Err(format!("NOT_RECURRING: 待办 {id} 不是周期待办"));
    }
    let Some(due) = t.due_at else {
        return Err(format!("INVALID_STATE: 周期待办 {id} 缺少截止时刻，无法滚动"));
    };
    let rule = RepeatRule::from_todo(&t);
    // Err = 迭代预算耗尽（计算失败），必须原样上报；只有 Ok(None) 才是规则真的用尽，
    // 可以转一次性——把两者混为一谈会静默取消还在生效的周期。
    let next = todo_recurrence::next_occurrence(&rule, due, now_ms)?;
    let ended = match next {
        None => true,
        Some(n) => todo_recurrence::reached_end(&rule, n, t.repeat_done_count),
    };
    let ts = now();
    // 父行滚动与子待办复位放同一事务：否则复位失败会留下
    // 「父已滚到下一轮、子项仍标记完成」的半写状态。
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let affected = if ended {
        // 规则用尽：转普通待办并保留该行，due_at 不动（过期即按逾期处理）
        tx.execute(
            "UPDATE todos SET repeat_mode = 'once',
               repeat_done_count = repeat_done_count + 1, repeat_last_done_at = ?1,
               updated_at = ?2, version = version + 1
             WHERE id = ?3 AND (?4 IS NULL OR version = ?4)",
            params![ts, ts, id, expected_version],
        )
    } else {
        let new_due = next.unwrap();
        let shift = new_due - due;
        let new_remind = t.remind_at.map(|r| r + shift);
        tx.execute(
            "UPDATE todos SET due_at = ?1, remind_at = ?2, remind_fired = 0,
               repeat_done_count = repeat_done_count + 1, repeat_last_done_at = ?3,
               sort_order = CASE WHEN pinned = 1 THEN sort_order ELSE NULL END,
               updated_at = ?4, version = version + 1
             WHERE id = ?5 AND (?6 IS NULL OR version = ?6)",
            params![new_due, new_remind, ts, ts, id, expected_version],
        )
    }
    .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    if !ended {
        reset_children(&tx, id).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    get(conn, id).map_err(|e| e.to_string())
}

/// 撤销「完成本轮」：计数 −1，`due_at` 滚回上一个实例，提醒随之回移
pub fn undo_recurring(conn: &Connection, id: i64) -> Result<Todo, String> {
    undo_recurring_inner(conn, id, None)
}

pub fn undo_recurring_with_version(
    conn: &Connection,
    id: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    undo_recurring_inner(conn, id, expected_version)
}

fn undo_recurring_inner(
    conn: &Connection,
    id: i64,
    expected_version: Option<i64>,
) -> Result<Todo, String> {
    let t = get(conn, id).map_err(|e| e.to_string())?;
    if t.repeat_mode == "once" {
        // 已滚动到结束时 repeat_mode 已置回 once，规则信息不再存在，无法回滚
        return Err(format!("NOT_RECURRING: 待办 {id} 已结束周期，无法撤销本轮"));
    }
    // 累计完成次数为 0 = 从未「完成本轮」，再往回滚会越过初始 due_at（每日规则
    // 的 previous_occurrence 总会算出昨天）。只能按已完成次数撤销，不能无限回退。
    if t.repeat_done_count <= 0 {
        return Err(format!(
            "NOTHING_TO_UNDO: 待办 {id} 没有可撤销的完成记录（已回到初始日期）"
        ));
    }
    let Some(due) = t.due_at else {
        return Err(format!("INVALID_STATE: 周期待办 {id} 缺少截止时刻，无法回滚"));
    };
    let rule = RepeatRule::from_todo(&t);
    let Some(prev) = todo_recurrence::previous_occurrence(&rule, due) else {
        return Err(format!("INVALID_STATE: 待办 {id} 没有上一轮可回滚"));
    };
    let shift = prev - due;
    let new_remind = t.remind_at.map(|r| r + shift);
    let ts = now();
    let affected = conn
        .execute(
            "UPDATE todos SET due_at = ?1, remind_at = ?2, remind_fired = 0,
               repeat_done_count = MAX(repeat_done_count - 1, 0),
               updated_at = ?3, version = version + 1
             WHERE id = ?4 AND (?5 IS NULL OR version = ?5)",
            params![prev, new_remind, ts, id, expected_version],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err(not_found_or_conflict(conn, id, "待办"));
    }
    get(conn, id).map_err(|e| e.to_string())
}

/// 日历展开周期实例用的候选集：只取「顶级 + 未完成 + 周期 + 有截止」的行。
/// 用 `list()` 全表读 26 列再在内存里过滤，翻月/切周时每次都要付一遍这个成本。
pub fn list_recurring_candidates(conn: &Connection) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         WHERE done = 0 AND parent_id IS NULL AND repeat_mode <> 'once' AND due_at IS NOT NULL
         ORDER BY due_at ASC"
    ))?;
    let rows = stmt.query_map([], row_to_todo)?;
    rows.collect()
}

/// 删除待办。子待办经外键 ON DELETE CASCADE 一并删除。
pub fn delete(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let kids = children_ids(conn, id)?;
    conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
    Ok(kids)
}

/// 直接子待办 id 列表（仅一层，无嵌套子待办）
pub fn children_ids(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM todos WHERE parent_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![id], |r| r.get(0))?;
    rows.collect()
}

/// 按传入顺序写入手动排序位（拖拽排序；ids[i] 的 sort_order = i+1）。
/// 只更新传入的条目——前端按分组计算顺序，后端不做分组解释。
pub fn reorder(conn: &Connection, ids: &[i64]) -> Result<()> {
    let ts = now();
    let tx = conn.unchecked_transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE todos SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
            params![(i + 1) as i64, ts, id],
        )?;
    }
    tx.commit()
}

/// 把子待办改挂到另一个**顶级**父待办下（跨父拖拽），并重写目标父下的子项顺序。
/// `ordered_ids` 为目标父落点后的完整子项顺序（含被移动项）。
/// 仅支持一层：新父必须是顶级（`parent_id IS NULL`），挡住「子待办挂到子待办下」。
/// 源父剩余子项各自 `sort_order` 相对顺序不变，无需回写。
pub fn move_child(
    conn: &Connection,
    id: i64,
    new_parent_id: i64,
    ordered_ids: &[i64],
) -> Result<Todo, String> {
    let child = get(conn, id).map_err(|e| e.to_string())?;
    let Some(old_parent) = child.parent_id else {
        return Err(format!("INVALID_STATE: 待办 {id} 不是子待办，无法改挂父级"));
    };
    let parent = get(conn, new_parent_id)
        .map_err(|_| format!("NOT_FOUND: 目标父待办 {new_parent_id} 不存在"))?;
    if parent.parent_id.is_some() {
        return Err(format!(
            "INVALID_STATE: 目标 {new_parent_id} 不是顶级待办，子待办仅支持一层"
        ));
    }
    if old_parent == new_parent_id {
        // 同父：只重排，不写 parent_id（与顶层拖拽同一语义，也不额外推进版本）
        reorder(conn, ordered_ids).map_err(|e| e.to_string())?;
        return get(conn, id).map_err(|e| e.to_string());
    }
    let ts = now();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE todos SET parent_id = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
        params![new_parent_id, ts, id],
    )
    .map_err(|e| e.to_string())?;
    // 限定 `parent_id = 目标父`：ordered_ids 只应含目标父下的子项（含刚改挂进来的 id），
    // 万一前端送来过期的 id（并发下该子项已被移走）也不至于写到别人名下
    for (i, oid) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE todos SET sort_order = ?1, updated_at = ?2 WHERE id = ?3 AND parent_id = ?4",
            params![(i + 1) as i64, ts, oid, new_parent_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    get(conn, id).map_err(|e| e.to_string())
}

pub fn search(conn: &Connection, keyword: &str) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         WHERE title LIKE '%' || ?1 || '%' ORDER BY done ASC, created_at DESC LIMIT 20",
    ))?;
    let rows = stmt.query_map(params![keyword], row_to_todo)?;
    rows.collect()
}

/// 到期待提醒的待办（未完成、未触发过、提醒时刻已到）。
/// 提醒只针对未完成项：完成后由 done 过滤，无需清 remind_at。
pub fn list_due_reminders(conn: &Connection, now_ms: i64) -> Result<Vec<Todo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM todos
         WHERE done = 0 AND remind_fired = 0 AND remind_at IS NOT NULL AND remind_at <= ?1
         ORDER BY remind_at ASC",
    ))?;
    let rows = stmt.query_map(params![now_ms], row_to_todo)?;
    rows.collect()
}

pub fn mark_remind_fired(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE todos SET remind_fired = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn row_to_todo(row: &rusqlite::Row) -> Result<Todo> {
    Ok(Todo {
        id: row.get(0)?,
        title: row.get(1)?,
        done: row.get(2)?,
        priority: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        completed_at: row.get(6)?,
        due_at: row.get(7)?,
        remind_at: row.get(8)?,
        remind_fired: row.get::<_, i64>(9)? != 0,
        parent_id: row.get(10)?,
        sort_order: row.get(11)?,
        version: row.get(12)?,
        description: row.get(13)?,
        pinned: row.get::<_, i64>(14)? != 0,
        repeat_mode: row.get(15)?,
        repeat_every: row.get(16)?,
        repeat_unit: row.get(17)?,
        repeat_weekdays: row.get(18)?,
        repeat_month_day: row.get(19)?,
        repeat_month_nth: row.get(20)?,
        repeat_end_mode: row.get(21)?,
        repeat_end_at: row.get(22)?,
        repeat_count: row.get(23)?,
        repeat_done_count: row.get(24)?,
        repeat_last_done_at: row.get(25)?,
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
    fn create_sets_defaults() {
        let conn = setup();
        let t = create(&conn, "写周报", None, None).unwrap();
        assert_eq!(t.title, "写周报");
        assert!(!t.done);
        assert_eq!(t.priority, 0);
        assert_eq!(t.completed_at, None);
        assert_eq!(t.due_at, None);
        assert_eq!(t.remind_at, None);
        assert!(!t.remind_fired);
        assert_eq!(t.parent_id, None);
        assert!(!t.created_at.is_empty());
    }

    #[test]
    fn create_preserves_provided_created_at() {
        // 删除撤销恢复：保留原 created_at，避免恢复项按新时间排到列表最新位置
        let conn = setup();
        let t = create(&conn, "恢复的待办", None, Some("2026-01-02 03:04:05.123456")).unwrap();
        assert_eq!(t.created_at, "2026-01-02 03:04:05.123456");
    }

    #[test]
    fn list_returns_all() {
        let conn = setup();
        create(&conn, "任务 A", None, None).unwrap();
        create(&conn, "任务 B", None, None).unwrap();
        let list = list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        let titles: Vec<&str> = list.iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"任务 A"));
        assert!(titles.contains(&"任务 B"));
    }

    #[test]
    fn toggle_marks_done_then_undone() {
        let conn = setup();
        let t = create(&conn, "洗衣服", None, None).unwrap();
        let done = toggle(&conn, t.id).unwrap();
        assert!(done.done);
        assert!(done.completed_at.is_some());
        let undone = toggle(&conn, t.id).unwrap();
        assert!(!undone.done);
        assert_eq!(undone.completed_at, None);
    }

    #[test]
    fn update_changes_title_and_priority() {
        let conn = setup();
        let t = create(&conn, "旧标题", None, None).unwrap();
        let updated = update(&conn, t.id, "新标题", 2).unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.priority, 2);
    }

    #[test]
    fn delete_removes_todo() {
        let conn = setup();
        let t = create(&conn, "临时任务", None, None).unwrap();
        delete(&conn, t.id).unwrap();
        assert!(get(&conn, t.id).is_err());
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn search_matches_substring_and_percent() {
        let conn = setup();
        create(&conn, "买牛奶", None, None).unwrap();
        create(&conn, "进度 50%", None, None).unwrap();
        let found = search(&conn, "牛奶").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "买牛奶");
        // LIKE 中 % 是通配符，无需转义即可命中含 % 的标题
        let pct = search(&conn, "50%").unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(pct[0].title, "进度 50%");
    }

    #[test]
    fn sub_todo_links_to_parent() {
        let conn = setup();
        let parent = create(&conn, "准备 PPT", None, None).unwrap();
        let sub = create(&conn, "完成初稿", Some(parent.id), None).unwrap();
        assert_eq!(sub.parent_id, Some(parent.id));
        // 重复父创建：父不存在时报错
        assert!(create(&conn, "孤儿", Some(99999), None).is_err());
    }

    #[test]
    fn delete_parent_cascades_children() {
        let conn = setup();
        let parent = create(&conn, "父待办", None, None).unwrap();
        let s1 = create(&conn, "子一", Some(parent.id), None).unwrap();
        let s2 = create(&conn, "子二", Some(parent.id), None).unwrap();
        let kids = delete(&conn, parent.id).unwrap();
        assert_eq!(kids, vec![s1.id, s2.id]);
        assert!(get(&conn, parent.id).is_err());
        assert!(get(&conn, s1.id).is_err());
        assert!(get(&conn, s2.id).is_err());
    }

    #[test]
    fn schedule_sets_and_clears_due_and_remind() {
        let conn = setup();
        let t = create(&conn, "交房租", None, None).unwrap();
        let due = 1_800_000_000_000;
        let remind = due - 30 * 60_000;
        let s = schedule(&conn, t.id, Some(due), Some(remind)).unwrap();
        assert_eq!(s.due_at, Some(due));
        assert_eq!(s.remind_at, Some(remind));
        assert!(!s.remind_fired);
        // 清除
        let cleared = schedule(&conn, t.id, None, None).unwrap();
        assert_eq!(cleared.due_at, None);
        assert_eq!(cleared.remind_at, None);
    }

    #[test]
    fn reorder_assigns_sort_order_in_given_sequence() {
        let conn = setup();
        let a = create(&conn, "任务 A", None, None).unwrap();
        let b = create(&conn, "任务 B", None, None).unwrap();
        let c = create(&conn, "任务 C", None, None).unwrap();
        // 新建条目未手动排序
        assert!(list(&conn).unwrap().iter().all(|t| t.sort_order.is_none()));
        reorder(&conn, &[c.id, a.id, b.id]).unwrap();
        let got = |id: i64| get(&conn, id).unwrap().sort_order;
        assert_eq!(got(c.id), Some(1));
        assert_eq!(got(a.id), Some(2));
        assert_eq!(got(b.id), Some(3));
    }

    #[test]
    fn schedule_clears_manual_sort_order() {
        // 截止日期决定分组归属，换组后原手动顺序失效，应回到默认排序
        let conn = setup();
        let t = create(&conn, "换组的任务", None, None).unwrap();
        reorder(&conn, &[t.id]).unwrap();
        assert_eq!(get(&conn, t.id).unwrap().sort_order, Some(1));
        let s = schedule(&conn, t.id, Some(1_800_000_000_000), None).unwrap();
        assert_eq!(s.sort_order, None);
    }

    #[test]
    fn due_reminders_skip_fired_and_done() {
        let conn = setup();
        let now = 1_800_000_000_000;
        let a = create(&conn, "到期未触发", None, None).unwrap();
        schedule(&conn, a.id, Some(now + 1), Some(now - 60_000)).unwrap();
        let b = create(&conn, "已触发", None, None).unwrap();
        schedule(&conn, b.id, None, Some(now - 60_000)).unwrap();
        mark_remind_fired(&conn, b.id).unwrap();
        let c = create(&conn, "已完成", None, None).unwrap();
        schedule(&conn, c.id, None, Some(now - 60_000)).unwrap();
        toggle(&conn, c.id).unwrap();
        let d = create(&conn, "未到点", None, None).unwrap();
        schedule(&conn, d.id, None, Some(now + 60_000)).unwrap();

        let due = list_due_reminders(&conn, now).unwrap();
        let titles: Vec<&str> = due.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["到期未触发"]);
    }

    #[test]
    fn update_with_version_optimistic_lock() {
        let conn = setup();
        let t = create(&conn, "原标题", None, None).unwrap();
        assert_eq!(t.version, 0);

        // 版本命中：更新并自增 version
        let updated = update_with_version(&conn, t.id, "新标题", 1, Some(0)).unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(updated.version, 1);

        // 版本不匹配：返回 CONFLICT，数据不被覆盖
        let err = update_with_version(&conn, t.id, "冲突写入", 2, Some(0)).unwrap_err();
        assert!(err.starts_with("CONFLICT"));
        let t2 = get(&conn, t.id).unwrap();
        assert_eq!(t2.title, "新标题");
        assert_eq!(t2.version, 1);

        // 不校验（None）等价普通更新
        let local = update_with_version(&conn, t.id, "本地直写", 0, None).unwrap();
        assert_eq!(local.title, "本地直写");
        assert_eq!(local.version, 2);
    }

    #[test]
    fn toggle_with_version_conflicts_on_stale_version() {
        let conn = setup();
        let t = create(&conn, "开关", None, None).unwrap();

        let done = toggle_with_version(&conn, t.id, Some(0)).unwrap();
        assert!(done.done);
        assert_eq!(done.version, 1);

        // 旧版本号切换：CONFLICT
        let err = toggle_with_version(&conn, t.id, Some(0)).unwrap_err();
        assert!(err.starts_with("CONFLICT"));
        assert!(get(&conn, t.id).unwrap().done);
    }

    #[test]
    fn delete_with_version_guards_stale_version() {
        let conn = setup();
        let t = create(&conn, "待删", None, None).unwrap();
        // 先改一次让 version=1
        update_with_version(&conn, t.id, "已改", 0, Some(0)).unwrap();

        // 用旧版本删除：CONFLICT，记录仍存在
        let err = delete_with_version(&conn, t.id, Some(0)).unwrap_err();
        assert!(err.starts_with("CONFLICT"));
        assert!(get(&conn, t.id).is_ok());

        // 命中版本删除成功
        delete_with_version(&conn, t.id, Some(1)).unwrap();
        assert!(get(&conn, t.id).is_err());
    }

    #[test]
    fn schedule_with_version_optimistic_lock() {
        let conn = setup();
        let t = create(&conn, "排期", None, None).unwrap();
        let due = 1_800_000_000_000;

        let s = schedule_with_version(&conn, t.id, Some(due), None, Some(0)).unwrap();
        assert_eq!(s.due_at, Some(due));
        assert_eq!(s.version, 1);

        // 旧版本排期：CONFLICT
        let err = schedule_with_version(&conn, t.id, None, None, Some(0)).unwrap_err();
        assert!(err.starts_with("CONFLICT"));
        assert_eq!(get(&conn, t.id).unwrap().due_at, Some(due));
    }

    #[test]
    fn local_ui_writes_bump_version() {
        // 主 UI 本地写（旧函数）也必须推进版本链：否则同步端持过期 expectedVersion
        // 仍能「命中」写回，本机 UI 刚做的修改被静默覆盖
        let conn = setup();
        let t = create(&conn, "原始", None, None).unwrap();
        assert_eq!(t.version, 0);

        let u = update(&conn, t.id, "改标题", 2).unwrap();
        assert_eq!(u.version, 1);

        let g = toggle(&conn, t.id).unwrap();
        assert_eq!(g.version, 2);

        let s = schedule(&conn, t.id, Some(123_456), None).unwrap();
        assert_eq!(s.version, 3);
    }

    #[test]
    fn with_version_reports_not_found_for_missing_id() {
        // 记录不存在必须报 NOT_FOUND 而非 CONFLICT（同步端不应反复重试一条已删记录）
        let conn = setup();
        let err = update_with_version(&conn, 9999, "x", 0, Some(0)).unwrap_err();
        assert!(err.starts_with("NOT_FOUND"), "{err}");
        let err = toggle_with_version(&conn, 9999, None).unwrap_err();
        assert!(err.starts_with("NOT_FOUND"), "{err}");
        let err = schedule_with_version(&conn, 9999, None, None, None).unwrap_err();
        assert!(err.starts_with("NOT_FOUND"), "{err}");
        let err = delete_with_version(&conn, 9999, None).unwrap_err();
        assert!(err.starts_with("NOT_FOUND"), "{err}");

        // 对照：记录存在但版本不匹配仍报 CONFLICT
        let t = create(&conn, "存在", None, None).unwrap();
        let err = update_with_version(&conn, t.id, "x", 0, Some(5)).unwrap_err();
        assert!(err.starts_with("CONFLICT"), "{err}");
    }

    #[test]
    fn delete_with_version_returns_cascade_children() {
        let conn = setup();
        let parent = create(&conn, "父", None, None).unwrap();
        let kid = create(&conn, "子", Some(parent.id), None).unwrap();

        let kids = delete_with_version(&conn, parent.id, None).unwrap();
        assert_eq!(kids, vec![kid.id]);
        assert!(get(&conn, parent.id).is_err());
    }

    // ---------- 待办升级：描述 / 置顶 / 周期 ----------

    fn ts(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn daily_rule() -> RepeatRule {
        RepeatRule {
            mode: "daily".into(),
            every: None,
            unit: None,
            weekdays: None,
            month_day: None,
            month_nth: None,
            end_mode: None,
            end_at: None,
            count: None,
        }
    }

    fn recurring(conn: &Connection, title: &str, due: i64) -> Todo {
        let t = create(conn, title, None, None).unwrap();
        schedule(conn, t.id, Some(due), None).unwrap();
        set_repeat(conn, t.id, &daily_rule()).unwrap()
    }

    #[test]
    fn set_description_and_pinned_persist() {
        let conn = setup();
        let t = create(&conn, "交房租", None, None).unwrap();
        assert_eq!(t.description, "");
        assert!(!t.pinned);

        let d = set_description(&conn, t.id, "转账给房东").unwrap();
        assert_eq!(d.description, "转账给房东");
        assert_eq!(d.version, 1);

        let p = set_pinned(&conn, t.id, true).unwrap();
        assert!(p.pinned);
        assert_eq!(p.version, 2);
        // 描述不被置顶写入抹掉
        assert_eq!(p.description, "转账给房东");
    }

    #[test]
    fn set_description_truncates_over_limit() {
        let conn = setup();
        let t = create(&conn, "超长描述", None, None).unwrap();
        let long = "字".repeat(MAX_DESCRIPTION_LEN + 500);
        let d = set_description(&conn, t.id, &long).unwrap();
        assert_eq!(d.description.chars().count(), MAX_DESCRIPTION_LEN);
        // 上限内原样写入
        let ok = "x".repeat(MAX_DESCRIPTION_LEN);
        let d2 = set_description(&conn, t.id, &ok).unwrap();
        assert_eq!(d2.description.chars().count(), MAX_DESCRIPTION_LEN);
    }

    #[test]
    fn set_repeat_writes_all_rule_columns() {
        let conn = setup();
        let t = create(&conn, "每月交房租", None, None).unwrap();
        let mut rule = daily_rule();
        rule.mode = "monthly".into();
        rule.month_day = Some(1);
        rule.end_mode = Some("until".into());
        rule.end_at = Some(ts(2027, 1, 1, 0, 0));
        let got = set_repeat(&conn, t.id, &rule).unwrap();
        assert_eq!(got.repeat_mode, "monthly");
        assert_eq!(got.repeat_month_day, Some(1));
        assert_eq!(got.repeat_end_mode.as_deref(), Some("until"));
        assert_eq!(got.repeat_end_at, Some(ts(2027, 1, 1, 0, 0)));
    }

    #[test]
    fn complete_recurring_rolls_due_and_counts() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = recurring(&conn, "喝 8 杯水", due);

        let rolled = complete_recurring(&conn, t.id, due).unwrap();
        assert_eq!(rolled.due_at, Some(ts(2026, 9, 22, 9, 0)));
        assert_eq!(rolled.repeat_done_count, 1);
        assert!(rolled.repeat_last_done_at.is_some());
        // 周期待办不置 done、不进已完成列表
        assert!(!rolled.done);
        assert_eq!(rolled.completed_at, None);
        assert_eq!(rolled.repeat_mode, "daily");
    }

    #[test]
    fn complete_recurring_skips_missed_and_shifts_reminder() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let remind = ts(2026, 9, 21, 8, 30);
        let t = create(&conn, "喝 8 杯水", None, None).unwrap();
        schedule(&conn, t.id, Some(due), Some(remind)).unwrap();
        set_repeat(&conn, t.id, &daily_rule()).unwrap();

        // 逾期 3 天后点掉：直接滚到明天，不补做历史
        let now = ts(2026, 9, 24, 10, 0);
        let rolled = complete_recurring(&conn, t.id, now).unwrap();
        assert_eq!(rolled.due_at, Some(ts(2026, 9, 25, 9, 0)));
        // 提醒按同一偏移平移（提前 30 分钟），并重新武装
        assert_eq!(rolled.remind_at, Some(ts(2026, 9, 25, 8, 30)));
        assert!(!rolled.remind_fired);
        assert_eq!(rolled.repeat_done_count, 1);
    }

    #[test]
    fn complete_recurring_resets_children() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let parent = recurring(&conn, "每周复盘", due);
        let k1 = create(&conn, "子一", Some(parent.id), None).unwrap();
        let k2 = create(&conn, "子二", Some(parent.id), None).unwrap();
        toggle(&conn, k1.id).unwrap();
        toggle(&conn, k2.id).unwrap();

        complete_recurring(&conn, parent.id, due).unwrap();
        // 下一轮子待办全部复位为未完成（§11 第 6 项）
        assert!(!get(&conn, k1.id).unwrap().done);
        assert!(!get(&conn, k2.id).unwrap().done);
        assert_eq!(get(&conn, k1.id).unwrap().completed_at, None);
    }

    #[test]
    fn complete_recurring_clears_sort_order_unless_pinned() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let a = recurring(&conn, "普通周期", due);
        reorder(&conn, &[a.id]).unwrap();
        let rolled = complete_recurring(&conn, a.id, due).unwrap();
        // 换组后原手动顺序失效（与 schedule() 同口径）
        assert_eq!(rolled.sort_order, None);

        let b = recurring(&conn, "置顶周期", due);
        set_pinned(&conn, b.id, true).unwrap();
        reorder(&conn, &[b.id]).unwrap();
        let rolled_b = complete_recurring(&conn, b.id, due).unwrap();
        // 置顶条目脱离日期分组，滚动不影响置顶区内的次序
        assert_eq!(rolled_b.sort_order, Some(1));
        assert!(rolled_b.pinned);
    }

    #[test]
    fn complete_recurring_ends_on_until() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = create(&conn, "到某日为止", None, None).unwrap();
        schedule(&conn, t.id, Some(due), None).unwrap();
        let mut rule = daily_rule();
        rule.end_mode = Some("until".into());
        rule.end_at = Some(ts(2026, 9, 22, 0, 0));
        set_repeat(&conn, t.id, &rule).unwrap();

        // 本轮完成后下一轮 9/23 已超出 until → 结束：转 once、保留该行、due_at 不动
        let ended = complete_recurring(&conn, t.id, due).unwrap();
        assert_eq!(ended.repeat_mode, "once");
        assert_eq!(ended.due_at, Some(due));
        assert_eq!(ended.repeat_done_count, 1);
    }

    #[test]
    fn complete_recurring_ends_on_count() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = create(&conn, "共 2 次", None, None).unwrap();
        schedule(&conn, t.id, Some(due), None).unwrap();
        let mut rule = daily_rule();
        rule.end_mode = Some("count".into());
        rule.count = Some(2);
        set_repeat(&conn, t.id, &rule).unwrap();

        let first = complete_recurring(&conn, t.id, due).unwrap();
        assert_eq!(first.repeat_mode, "daily");
        assert_eq!(first.repeat_done_count, 1);

        let second = complete_recurring(&conn, t.id, first.due_at.unwrap()).unwrap();
        assert_eq!(second.repeat_mode, "once");
        assert_eq!(second.repeat_done_count, 2);
    }

    #[test]
    fn undo_recurring_rolls_back_due_and_count() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = recurring(&conn, "喝 8 杯水", due);
        complete_recurring(&conn, t.id, due).unwrap();

        let back = undo_recurring(&conn, t.id).unwrap();
        assert_eq!(back.due_at, Some(due));
        assert_eq!(back.repeat_done_count, 0);
    }

    #[test]
    fn undo_recurring_rejects_when_count_zero() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = recurring(&conn, "喝 8 杯水", due);
        assert_eq!(t.repeat_done_count, 0);
        // 从未完成本轮：不能凭规则硬算出「昨天」把初始 due 再往回推
        let err = undo_recurring(&conn, t.id).unwrap_err();
        assert!(err.starts_with("NOTHING_TO_UNDO"), "{err}");
        assert_eq!(get(&conn, t.id).unwrap().due_at, Some(due));

        complete_recurring(&conn, t.id, due).unwrap();
        undo_recurring(&conn, t.id).unwrap();
        // 撤回到初始后再次撤销同样拒绝
        let err2 = undo_recurring(&conn, t.id).unwrap_err();
        assert!(err2.starts_with("NOTHING_TO_UNDO"), "{err2}");
        assert_eq!(get(&conn, t.id).unwrap().due_at, Some(due));
        assert_eq!(get(&conn, t.id).unwrap().repeat_done_count, 0);
    }

    #[test]
    fn complete_recurring_rejects_plain_todo() {
        let conn = setup();
        let t = create(&conn, "普通待办", None, None).unwrap();
        let err = complete_recurring(&conn, t.id, 0).unwrap_err();
        assert!(err.starts_with("NOT_RECURRING"), "{err}");
    }

    #[test]
    fn complete_recurring_with_version_conflicts_on_stale() {
        let conn = setup();
        let due = ts(2026, 9, 21, 9, 0);
        let t = recurring(&conn, "喝 8 杯水", due);
        // schedule + set_repeat 各推进一次版本
        let cur = get(&conn, t.id).unwrap().version;
        let err = complete_recurring_with_version(&conn, t.id, due, Some(cur - 1)).unwrap_err();
        assert!(err.starts_with("CONFLICT"), "{err}");
        let ok = complete_recurring_with_version(&conn, t.id, due, Some(cur)).unwrap();
        assert_eq!(ok.due_at, Some(ts(2026, 9, 22, 9, 0)));
    }

    // ---------- 跨父拖拽：子待办改挂父级 ----------

    #[test]
    fn move_child_reparents_and_orders_target() {
        let conn = setup();
        let p1 = create(&conn, "父一", None, None).unwrap();
        let p2 = create(&conn, "父二", None, None).unwrap();
        let a = create(&conn, "子A", Some(p1.id), None).unwrap();
        let b = create(&conn, "子B", Some(p2.id), None).unwrap();

        let moved = move_child(&conn, a.id, p2.id, &[a.id, b.id]).unwrap();
        assert_eq!(moved.parent_id, Some(p2.id));
        assert_eq!(moved.sort_order, Some(1));
        assert_eq!(get(&conn, b.id).unwrap().sort_order, Some(2));
        // 源父下不再挂着被移走的子项（源父已无子项）
        assert!(children_ids(&conn, p1.id).unwrap().is_empty());
        // 改挂推进版本链
        assert_eq!(moved.version, a.version + 1);
    }

    #[test]
    fn move_child_ignores_foreign_ids_in_order() {
        let conn = setup();
        let p1 = create(&conn, "父一", None, None).unwrap();
        let p2 = create(&conn, "父二", None, None).unwrap();
        let p3 = create(&conn, "父三", None, None).unwrap();
        let a = create(&conn, "子A", Some(p1.id), None).unwrap();
        let b = create(&conn, "子B", Some(p2.id), None).unwrap();
        let c = create(&conn, "子C", Some(p3.id), None).unwrap();
        // 给 c 一个显式排序位，验证跨父落点不会波及不属于目标父的条目
        reorder(&conn, &[c.id]).unwrap();
        let c_before = get(&conn, c.id).unwrap();

        let moved = move_child(&conn, a.id, p2.id, &[a.id, b.id, c.id]).unwrap();
        assert_eq!(moved.parent_id, Some(p2.id));
        assert_eq!(moved.sort_order, Some(1));
        assert_eq!(get(&conn, b.id).unwrap().sort_order, Some(2));
        // 外来 id：父级与排序位都不动
        let c_after = get(&conn, c.id).unwrap();
        assert_eq!(c_after.parent_id, Some(p3.id));
        assert_eq!(c_after.sort_order, c_before.sort_order);
    }

    #[test]
    fn move_child_same_parent_only_reorders() {
        let conn = setup();
        let p = create(&conn, "父", None, None).unwrap();
        let a = create(&conn, "子A", Some(p.id), None).unwrap();
        let b = create(&conn, "子B", Some(p.id), None).unwrap();

        let moved = move_child(&conn, a.id, p.id, &[b.id, a.id]).unwrap();
        assert_eq!(moved.parent_id, Some(p.id));
        assert_eq!(get(&conn, b.id).unwrap().sort_order, Some(1));
        assert_eq!(moved.sort_order, Some(2));
        // 同父重排不额外推进版本
        assert_eq!(moved.version, a.version);
    }

    #[test]
    fn move_child_rejects_missing_target_parent() {
        let conn = setup();
        let p = create(&conn, "父", None, None).unwrap();
        let k = create(&conn, "子", Some(p.id), None).unwrap();
        let err = move_child(&conn, k.id, 99999, &[k.id]).unwrap_err();
        assert!(err.starts_with("NOT_FOUND"), "{err}");
        // 失败不改挂
        assert_eq!(get(&conn, k.id).unwrap().parent_id, Some(p.id));
    }

    #[test]
    fn move_child_rejects_two_level_nesting() {
        let conn = setup();
        let p = create(&conn, "父", None, None).unwrap();
        let k1 = create(&conn, "子一", Some(p.id), None).unwrap();
        let k2 = create(&conn, "子二", Some(p.id), None).unwrap();
        // 目标必须是顶级：挂到另一个子待办下应被拒
        let err = move_child(&conn, k2.id, k1.id, &[k2.id]).unwrap_err();
        assert!(err.starts_with("INVALID_STATE"), "{err}");
        assert_eq!(get(&conn, k2.id).unwrap().parent_id, Some(p.id));
    }

    #[test]
    fn move_child_rejects_top_level_source() {
        let conn = setup();
        let p = create(&conn, "父", None, None).unwrap();
        let top = create(&conn, "顶级待办", None, None).unwrap();
        let err = move_child(&conn, top.id, p.id, &[top.id]).unwrap_err();
        assert!(err.starts_with("INVALID_STATE"), "{err}");
        assert_eq!(get(&conn, top.id).unwrap().parent_id, None);
    }
}
