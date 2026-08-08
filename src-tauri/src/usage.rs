use crate::models::{SyncResult, UsageDaily, UsageDetail, UsageProvider, UsageRecord, UsageSummary};
use chrono::{Datelike, Duration, Local, TimeZone};
use rusqlite::{params, Connection, OpenFlags, Result};

/// 自动探测 opencode 数据库路径（按顺序尝试）
pub fn probe_opencode_path() -> Option<String> {
    if let Ok(p) = std::env::var("OPENCODE_DATA_DIR") {
        if !p.trim().is_empty() {
            let cand = std::path::Path::new(&p).join("opencode.db");
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local").join("share").join("opencode").join("opencode.db"));
        candidates.push(home.join(".opencode").join("opencode.db"));
    }
    candidates.into_iter().find(|p| p.exists()).map(|p| p.to_string_lossy().into_owned())
}

/// 只读打开外部 opencode 数据库
fn open_readonly(path: &str) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

/// 从 opencode.db 增量同步到本库。
/// `cursor` 为上次同步到的 `time_updated`（毫秒时间戳，epoch ms）。
/// 返回 (新增条数, 新游标, 最新 time_updated 或 0)
pub fn sync_from_opencode(
    own: &Connection,
    opencode_path: &str,
    cursor: i64,
) -> Result<SyncResult> {
    let src = open_readonly(opencode_path)?;
    // 只同步有 token 数据的会话；游标按 time_updated 增量推进
    let mut stmt = src.prepare(
        "SELECT id, tokens_input, tokens_output, tokens_reasoning,
                tokens_cache_read, tokens_cache_write, cost, time_created, time_updated, model
         FROM session
         WHERE tokens_input IS NOT NULL AND time_updated > ?1
         ORDER BY time_updated ASC",
    )?;
    let rows: Vec<(String, i64, i64, i64, i64, i64, f64, i64, i64, Option<String>)> = stmt
        .query_map(params![cursor], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<String>>(9)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut inserted: i64 = 0;
    let mut last_updated: i64 = cursor;
    for (session_id, input, output, reasoning, cache_read, cache_write, cost, created, updated, model) in rows {
        let (provider, model_name) = parse_model(&model);
        own.execute(
            "INSERT OR REPLACE INTO ai_usage
               (session_id, provider, model, tokens_input, tokens_output, tokens_reasoning,
                tokens_cache_read, tokens_cache_write, cost, time_created, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'remote')",
            params![session_id, provider, model_name, input, output, reasoning,
                    cache_read, cache_write, cost, created],
        )?;
        inserted += 1;
        if updated > last_updated {
            last_updated = updated;
        }
    }
    Ok(SyncResult {
        inserted,
        cursor: last_updated,
        listening: true,
        path: Some(opencode_path.to_string()),
    })
}

/// 解析 opencode 的 model JSON 字段 → (provider, model)
fn parse_model(raw: &Option<String>) -> (Option<String>, Option<String>) {
    let Some(raw) = raw else {
        return (None, None);
    };
    let v: serde_json::Value = serde_json::from_str(raw).unwrap_or(serde_json::Value::Null);
    let provider = v
        .get("providerID")
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
        .or_else(|| v.get("provider").and_then(|p| p.as_str()).map(|s| s.to_string()));
    let model = v
        .get("id")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .or_else(|| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()));
    (provider, model)
}

fn local_day_start_ms(days_ago: i64) -> i64 {
    let now = Local::now();
    let day = now.date_naive() - Duration::days(days_ago);
    let dt = day.and_hms_opt(0, 0, 0).unwrap();
    let local = Local.from_local_datetime(&dt).unwrap();
    local.timestamp_millis()
}

/// 查询汇总（今日 / 7 日 / 本月 / 累计）
pub fn query_summary(conn: &Connection) -> Result<UsageSummary> {
    let today = local_day_start_ms(0);
    let seven = local_day_start_ms(6);
    let now = Local::now();
    let month_start = now.with_day(1).unwrap().date_naive().and_hms_opt(0, 0, 0).unwrap();
    let month_ms = Local.from_local_datetime(&month_start).unwrap().timestamp_millis();

    let summarize = |conn: &Connection, since_ms: i64| -> Result<(i64, i64, i64, f64)> {
        conn.query_row(
            "SELECT COALESCE(SUM(tokens_input),0), COALESCE(SUM(tokens_cache_read),0),
                    COALESCE(SUM(tokens_output),0), COALESCE(SUM(cost),0)
             FROM ai_usage WHERE time_created >= ?1",
            params![since_ms],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            },
        )
    };

    let (ti, tc, to, tcost) = summarize(conn, today)?;
    let (si, sc, so, scost) = summarize(conn, seven)?;
    let (mi, mc, mo, mcost) = summarize(conn, month_ms)?;
    let (ai, ac, ao, acost) = summarize(conn, 0)?;

    let record_count = conn.query_row(
        "SELECT COUNT(*) FROM ai_usage",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    let last_sync_at = conn
        .query_row(
            "SELECT MAX(time_created) FROM ai_usage",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .unwrap_or(None);

    Ok(UsageSummary {
        today_input: ti,
        today_cache_input: tc,
        today_output: to,
        today_cost: tcost,
        seven_day_input: si,
        seven_day_cache_input: sc,
        seven_day_output: so,
        seven_day_cost: scost,
        month_input: mi,
        month_cache_input: mc,
        month_output: mo,
        month_cost: mcost,
        total_input: ai,
        total_cache_input: ac,
        total_output: ao,
        total_cost: acost,
        record_count,
        last_sync_at,
    })
}

/// 按天趋势（近 days 天，含无数据的天补 0）
pub fn query_daily(conn: &Connection, days: i64) -> Result<Vec<UsageDaily>> {
    let since = local_day_start_ms(days - 1);
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', time_created/1000, 'unixepoch', 'localtime') as day,
                COALESCE(SUM(tokens_input),0), COALESCE(SUM(tokens_cache_read),0),
                COALESCE(SUM(tokens_output),0), COALESCE(SUM(cost),0)
         FROM ai_usage WHERE time_created >= ?1
         GROUP BY day ORDER BY day ASC",
    )?;
    let map: std::collections::HashMap<String, (i64, i64, i64, f64)> = stmt
        .query_map(params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, f64>(4)?),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out = Vec::new();
    for d in 0..days {
        let day = Local::now().date_naive() - Duration::days(days - 1 - d);
        let key = day.format("%Y-%m-%d").to_string();
        let (i, c, o, cost) = map.get(&key).copied().unwrap_or((0, 0, 0, 0.0));
        out.push(UsageDaily {
            date: key,
            input: i,
            cache_input: c,
            output: o,
            cost,
        });
    }
    Ok(out)
}

/// 按 provider 排行
pub fn query_providers(conn: &Connection) -> Result<Vec<UsageProvider>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(provider, '未知'), COUNT(*), COALESCE(SUM(tokens_input),0),
                COALESCE(SUM(tokens_cache_read),0), COALESCE(SUM(tokens_output),0), COALESCE(SUM(cost),0)
         FROM ai_usage GROUP BY COALESCE(provider, '未知')
         ORDER BY COALESCE(SUM(cost),0) DESC, SUM(tokens_input) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(UsageProvider {
            provider: row.get(0)?,
            count: row.get(1)?,
            input: row.get(2)?,
            cache_input: row.get(3)?,
            output: row.get(4)?,
            cost: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// 明细（分页）
pub fn query_records(conn: &Connection, limit: i64, offset: i64) -> Result<Vec<UsageRecord>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, provider, model, tokens_input, tokens_cache_read, tokens_output,
                tokens_reasoning, tokens_cache_write, cost, time_created, source
         FROM ai_usage ORDER BY time_created DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(UsageRecord {
            session_id: row.get(0)?,
            provider: row.get(1)?,
            model: row.get(2)?,
            tokens_input: row.get(3)?,
            tokens_cache_read: row.get(4)?,
            tokens_output: row.get(5)?,
            tokens_reasoning: row.get(6)?,
            tokens_cache_write: row.get(7)?,
            cost: row.get(8)?,
            time_created: row.get(9)?,
            source: row.get(10)?,
        })
    })?;
    rows.collect()
}

/// 用量详情页全量
pub fn query_detail(conn: &Connection, days: i64, limit: i64, offset: i64) -> Result<UsageDetail> {
    let daily = query_daily(conn, days)?;
    let providers = query_providers(conn)?;
    let records = query_records(conn, limit, offset)?;
    let total = conn.query_row("SELECT COUNT(*) FROM ai_usage", [], |r| r.get::<_, i64>(0))?;
    Ok(UsageDetail {
        daily,
        providers,
        records,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_in_memory;

    fn insert_session(
        conn: &Connection,
        id: &str,
        input: i64,
        output: i64,
        updated: i64,
        created: i64,
        model: &str,
    ) {
        conn.execute(
            "INSERT INTO session (id, tokens_input, tokens_output, tokens_reasoning,
                 tokens_cache_read, tokens_cache_write, cost, time_created, time_updated, model)
             VALUES (?1, ?2, ?3, 0, ?4, 0, 0.1, ?5, ?6, ?7)",
            params![id, input, output, input / 2, created, updated, model],
        )
        .unwrap();
    }

    #[test]
    fn sync_is_incremental_and_idempotent() {
        // src 是内存库，需要落盘才能被只读打开；用临时文件
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("opencode.db");
        let src_file = Connection::open(&src_path).unwrap();
        src_file
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER,
                    tokens_cache_read INTEGER, tokens_cache_write INTEGER, cost REAL,
                    time_created INTEGER, time_updated INTEGER, model TEXT
                );",
            )
            .unwrap();
        insert_session(&src_file, "s1", 1000, 100, 1000, 900, r#"{"id":"m1","providerID":"deepseek"}"#);
        insert_session(&src_file, "s2", 2000, 200, 2000, 1800, r#"{"id":"m2","providerID":"longcat"}"#);

        let own = init_in_memory().unwrap();
        let r1 = sync_from_opencode(&own, src_path.to_str().unwrap(), 0).unwrap();
        assert_eq!(r1.inserted, 2);
        assert_eq!(r1.cursor, 2000);
        assert!(r1.listening);

        // 幂等：同样游标再同步 → 0 新增
        let r2 = sync_from_opencode(&own, src_path.to_str().unwrap(), r1.cursor).unwrap();
        assert_eq!(r2.inserted, 0);

        // 增量：新增一条 → 只同步新的
        insert_session(&src_file, "s3", 500, 50, 3000, 2900, r#"{"id":"m3","providerID":"deepseek"}"#);
        let r3 = sync_from_opencode(&own, src_path.to_str().unwrap(), r1.cursor).unwrap();
        assert_eq!(r3.inserted, 1);
        assert_eq!(r3.cursor, 3000);

        let cnt: i64 = own.query_row("SELECT COUNT(*) FROM ai_usage", [], |r| r.get(0)).unwrap();
        assert_eq!(cnt, 3);
    }

    #[test]
    fn summary_and_detail_aggregate_correctly() {
        let conn = init_in_memory().unwrap();
        let now = Local::now().timestamp_millis();
        let yesterday = (Local::now() - Duration::days(1)).timestamp_millis();
        for (i, (input, created)) in [(1000, now), (2000, yesterday)].iter().enumerate() {
            conn.execute(
                "INSERT INTO ai_usage (session_id, provider, model, tokens_input, tokens_output,
                     tokens_reasoning, tokens_cache_read, tokens_cache_write, cost, time_created, source)
                 VALUES (?1, 'deepseek', 'm1', ?2, 100, 0, ?3, 0, 0.5, ?4, 'remote')",
                params![format!("s{}", i), input, input / 2, created],
            )
            .unwrap();
        }

        let s = query_summary(&conn).unwrap();
        // 今日 1 条：input=1000, cache=500, output=100
        assert_eq!(s.today_input, 1000);
        assert_eq!(s.today_cache_input, 500);
        assert_eq!(s.today_output, 100);
        // 7 日含昨天
        assert_eq!(s.seven_day_input, 3000);
        assert_eq!(s.record_count, 2);

        let daily = query_daily(&conn, 7).unwrap();
        assert_eq!(daily.len(), 7);
        assert!(daily.iter().any(|d| d.input == 1000));

        let prov = query_providers(&conn).unwrap();
        assert_eq!(prov.len(), 1);
        assert_eq!(prov[0].provider, "deepseek");
        assert_eq!(prov[0].input, 3000);

        let detail = query_detail(&conn, 7, 10, 0).unwrap();
        assert_eq!(detail.total, 2);
        assert_eq!(detail.records.len(), 2);
    }

    #[test]
    fn parse_model_extracts_provider_and_id() {
        assert_eq!(
            parse_model(&Some(r#"{"id":"deepseek-v4-flash","providerID":"deepseek","variant":"default"}"#.to_string())),
            (Some("deepseek".to_string()), Some("deepseek-v4-flash".to_string()))
        );
        assert_eq!(parse_model(&None), (None, None));
        assert_eq!(
            parse_model(&Some(r#"not json"#.to_string())),
            (None, None)
        );
    }

    /// 真实 opencode.db 端到端验证（本机路径，默认忽略）
    #[test]
    #[ignore]
    fn sync_real_opencode_db() {
        let path = r"C:\Users\Administrator\.local\share\opencode\opencode.db";
        let own = init_in_memory().unwrap();
        let r = sync_from_opencode(&own, path, 0).unwrap();
        assert!(r.inserted > 0, "真实库应同步到记录");
        let s = query_summary(&own).unwrap();
        assert!(s.total_input > 0);
        let d = query_detail(&own, 7, 20, 0).unwrap();
        assert!(d.total == r.inserted);
        eprintln!(
            "REAL_OK inserted={} total_input={} cache_read={} providers={}",
            r.inserted,
            s.total_input,
            s.total_cache_input,
            d.providers.len()
        );
    }
}
