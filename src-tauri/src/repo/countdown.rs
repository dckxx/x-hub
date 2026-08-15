use crate::models::Countdown;
use crate::repo::now;
use rusqlite::{params, Connection, Result};

/// 当前毫秒时间戳（ticker 与进度计算统一基准）
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 倒计时数量上限（工作台中上区块两列三行，最多展示 6 个）
pub const MAX_COUNTDOWNS: i64 = 6;

/// 总数（含已结束，用于上限校验）
pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM countdowns", [], |r| r.get(0))
}

/// 列出全部倒计时：未结束在前（once 未到点 / daily / interval），按到点时间升序；已结束（once 灰态）在后
pub fn list(conn: &Connection) -> Result<Vec<Countdown>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repeat_mode, end_at, total_ms, interval_minutes,
                paused, paused_remaining_ms, finished, floated, float_x, float_y,
                created_at, updated_at
         FROM countdowns
         ORDER BY finished ASC,
           CASE WHEN repeat_mode = 'once' AND finished = 0 THEN end_at ELSE end_at END ASC,
           id DESC",
    )?;
    let rows = stmt.query_map([], row_to_countdown)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: i64) -> Result<Countdown> {
    conn.query_row(
        "SELECT id, name, repeat_mode, end_at, total_ms, interval_minutes,
                paused, paused_remaining_ms, finished, floated, float_x, float_y,
                created_at, updated_at
         FROM countdowns WHERE id = ?1",
        params![id],
        row_to_countdown,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    name: &str,
    repeat_mode: &str,
    end_at: i64,
    total_ms: i64,
    interval_minutes: Option<i64>,
) -> Result<Countdown> {
    let ts = now();
    conn.execute(
        "INSERT INTO countdowns
           (name, repeat_mode, end_at, total_ms, interval_minutes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![name, repeat_mode, end_at, total_ms, interval_minutes, ts],
    )?;
    get(conn, conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    conn: &Connection,
    id: i64,
    name: &str,
    repeat_mode: &str,
    end_at: i64,
    total_ms: i64,
    interval_minutes: Option<i64>,
) -> Result<Countdown> {
    conn.execute(
        "UPDATE countdowns SET
           name = ?1, repeat_mode = ?2, end_at = ?3, total_ms = ?4,
           interval_minutes = ?5, paused = 0, paused_remaining_ms = NULL,
           updated_at = ?6
         WHERE id = ?7",
        params![name, repeat_mode, end_at, total_ms, interval_minutes, now(), id],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM countdowns WHERE id = ?1", params![id])?;
    Ok(())
}

/// 到点置 finished（once 专用，触发后灰态待删）
pub fn mark_finished(conn: &Connection, id: i64) -> Result<Countdown> {
    conn.execute(
        "UPDATE countdowns SET finished = 1, paused_remaining_ms = NULL, updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    get(conn, id)
}

/// 到点后顺延：把 end_at 推进到未来的下一次（daily 按 24h，interval 按间隔），跳过错过的轮次
pub fn advance(conn: &Connection, id: i64, new_end_at: i64) -> Result<Countdown> {
    conn.execute(
        "UPDATE countdowns SET end_at = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_end_at, now(), id],
    )?;
    get(conn, id)
}

/// 暂停：
/// - once：冻结剩余毫秒（paused_remaining_ms），到点不再触发
/// - daily / interval：仅标记 paused，恢复时重算下一次
pub fn pause(conn: &Connection, id: i64) -> Result<Countdown> {
    conn.execute(
        "UPDATE countdowns SET
           paused = 1,
           paused_remaining_ms = CASE WHEN repeat_mode = 'once' THEN MAX(end_at - ?1, 0) ELSE NULL END,
           updated_at = ?2
         WHERE id = ?3",
        params![now_ms(), now(), id],
    )?;
    get(conn, id)
}

/// 恢复：
/// - once：end_at = now + 暂停时冻结的剩余
/// - daily：把 end_at 顺延到未来最近的同一时刻
/// - interval：从恢复时刻重新起算一个间隔
pub fn resume(conn: &Connection, id: i64) -> Result<Countdown> {
    let c = get(conn, id)?;
    let now = now_ms();
    let new_end_at = match c.repeat_mode.as_str() {
        "once" => now + c.paused_remaining_ms.unwrap_or(0).max(0),
        "daily" => {
            let mut t = c.end_at;
            while t <= now {
                t += 24 * 60 * 60 * 1000;
            }
            t
        }
        _ => {
            let interval = c.interval_minutes.unwrap_or(1).max(1) * 60 * 1000;
            let mut t = c.end_at;
            while t <= now {
                t += interval;
            }
            t
        }
    };
    conn.execute(
        "UPDATE countdowns SET
           paused = 0, paused_remaining_ms = NULL, end_at = ?1, updated_at = ?2
         WHERE id = ?3",
        params![new_end_at, crate::repo::now(), id],
    )?;
    get(conn, id)
}

/// 查询已到点且需要处理的项（未暂停、未结束、end_at <= now）
pub fn list_due(conn: &Connection, now: i64) -> Result<Vec<Countdown>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repeat_mode, end_at, total_ms, interval_minutes,
                paused, paused_remaining_ms, finished, floated, float_x, float_y,
                created_at, updated_at
         FROM countdowns
         WHERE paused = 0 AND finished = 0 AND end_at <= ?1
         ORDER BY end_at ASC",
    )?;
    let rows = stmt.query_map(params![now], row_to_countdown)?;
    rows.collect()
}

/// 启动时恢复浮窗：浮起状态 + 位置持久化在表里
pub fn list_floated(conn: &Connection) -> Result<Vec<Countdown>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repeat_mode, end_at, total_ms, interval_minutes,
                paused, paused_remaining_ms, finished, floated, float_x, float_y,
                created_at, updated_at
         FROM countdowns
         WHERE floated = 1
         ORDER BY end_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_countdown)?;
    rows.collect()
}

/// 浮窗浮起/收起 + 位置持久化
pub fn set_floated(
    conn: &Connection,
    id: i64,
    floated: bool,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Countdown> {
    conn.execute(
        "UPDATE countdowns SET floated = ?1, float_x = ?2, float_y = ?3, updated_at = ?4 WHERE id = ?5",
        params![floated as i64, x, y, now(), id],
    )?;
    get(conn, id)
}

/// 浮窗拖动时更新位置
pub fn update_position(conn: &Connection, id: i64, x: f64, y: f64) -> Result<()> {
    conn.execute(
        "UPDATE countdowns SET float_x = ?1, float_y = ?2 WHERE id = ?3",
        params![x, y, id],
    )?;
    Ok(())
}

pub fn row_to_countdown(row: &rusqlite::Row) -> Result<Countdown> {
    Ok(Countdown {
        id: row.get(0)?,
        name: row.get(1)?,
        repeat_mode: row.get(2)?,
        end_at: row.get(3)?,
        total_ms: row.get(4)?,
        interval_minutes: row.get(5)?,
        paused: row.get::<_, i64>(6)? != 0,
        paused_remaining_ms: row.get(7)?,
        finished: row.get::<_, i64>(8)? != 0,
        floated: row.get::<_, i64>(9)? != 0,
        float_x: row.get(10)?,
        float_y: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
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
    fn create_once_and_list() {
        let conn = setup();
        let now = now_ms();
        let c = create(&conn, "喝水", "interval", now + 60_000, 60_000, Some(1)).unwrap();
        assert_eq!(c.name, "喝水");
        assert_eq!(c.repeat_mode, "interval");
        assert!(!c.paused);
        assert!(!c.finished);
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn mark_finished_sets_gray_state() {
        let conn = setup();
        let now = now_ms();
        let c = create(&conn, "番茄钟", "once", now + 25 * 60_000, 25 * 60_000, None).unwrap();
        let done = mark_finished(&conn, c.id).unwrap();
        assert!(done.finished);
    }

    #[test]
    fn pause_once_freezes_remaining_and_resume_restores() {
        let conn = setup();
        let now = now_ms();
        let c = create(&conn, "定时", "once", now + 120_000, 120_000, None).unwrap();
        // 模拟走了 30s
        let c = pause(&conn, c.id).unwrap();
        assert!(c.paused);
        let remaining = c.paused_remaining_ms.unwrap();
        assert!(remaining > 60_000 && remaining <= 120_000);
        let r = resume(&conn, c.id).unwrap();
        assert!(!r.paused);
        assert!(r.end_at >= now_ms());
    }

    #[test]
    fn resume_interval_starts_from_now() {
        let conn = setup();
        let now = now_ms();
        let c = create(&conn, "喝水", "interval", now + 60_000, 60_000, Some(1)).unwrap();
        let c = pause(&conn, c.id).unwrap();
        assert!(c.paused);
        let r = resume(&conn, c.id).unwrap();
        assert!(!r.paused);
        assert!(r.end_at - now_ms() <= 60_000);
    }

    #[test]
    fn resume_daily_advances_to_future_occurrence() {
        let conn = setup();
        let now = now_ms();
        // 今天 15:00 附近：取当天 15:00，若已过则自动落到明天
        let mut t = now - (now % (24 * 60 * 60 * 1000)) + 15 * 60 * 60 * 1000;
        if t <= now {
            t += 24 * 60 * 60 * 1000;
        }
        let c = create(&conn, "下班提醒", "daily", t, 24 * 60 * 60 * 1000, None).unwrap();
        let c = pause(&conn, c.id).unwrap();
        assert!(c.paused);
        let r = resume(&conn, c.id).unwrap();
        assert!(!r.paused);
        assert!(r.end_at > now_ms());
    }

    #[test]
    fn list_due_returns_only_ready_items() {
        let conn = setup();
        let now = now_ms();
        create(&conn, "已到期", "once", now - 1000, 60_000, None).unwrap();
        create(&conn, "未到期", "once", now + 60_000, 60_000, None).unwrap();
        let p = create(&conn, "暂停的", "interval", now - 1000, 60_000, Some(1)).unwrap();
        pause(&conn, p.id).unwrap();
        let due = list_due(&conn, now).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "已到期");
    }

    #[test]
    fn float_fields_persist() {
        let conn = setup();
        let now = now_ms();
        let c = create(&conn, "喝水", "interval", now + 60_000, 60_000, Some(1)).unwrap();
        let f = set_floated(&conn, c.id, true, Some(100.0), Some(200.0)).unwrap();
        assert!(f.floated);
        assert_eq!(f.float_x, Some(100.0));
        update_position(&conn, c.id, 150.0, 260.0).unwrap();
        let g = get(&conn, c.id).unwrap();
        assert_eq!(g.float_x, Some(150.0));
        assert_eq!(list_floated(&conn).unwrap().len(), 1);
    }

    #[test]
    fn count_returns_total_including_finished() {
        let conn = setup();
        let now = now_ms();
        assert_eq!(count(&conn).unwrap(), 0);
        create(&conn, "a", "once", now + 60_000, 60_000, None).unwrap();
        let b = create(&conn, "b", "daily", now + 60_000, 60_000, None).unwrap();
        assert_eq!(count(&conn).unwrap(), 2);
        mark_finished(&conn, b.id).unwrap();
        assert_eq!(count(&conn).unwrap(), 2);
        assert_eq!(MAX_COUNTDOWNS, 6);
    }
}
