use rusqlite::{Connection, OpenFlags};

#[test]
fn verify_local_today() {
    let db = r"C:\Users\Administrator\AppData\Roaming\x-hub\app.db";
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let today_start = chrono::Local::now().date_naive().and_hms_opt(0,0,0).unwrap().and_local_timezone(chrono::Local).unwrap().timestamp_millis();
    let (cnt, ti, to): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(tokens_input),0), COALESCE(SUM(tokens_output),0) FROM ai_usage WHERE time_created >= ?1",
        [today_start], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    println!("TODAY: rows={} input={} output={}", cnt, ti, to);
    let (all, ai): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(tokens_input),0) FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    println!("ALL: rows={} input={}", all, ai);
}
