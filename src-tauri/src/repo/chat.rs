use crate::models::{ChatMessage, ChatSession};
use crate::repo::now;
use rusqlite::{params, Connection, Result};

const SESSION_COLS: &str =
    "id, title, model_name, created_at, updated_at, tokens_input, tokens_output, tokens_cache_read, tokens_reasoning, elapsed_ms";

pub fn list_sessions(conn: &Connection) -> Result<Vec<ChatSession>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLS} FROM chat_sessions ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_session)?;
    rows.collect()
}

pub fn get_session(conn: &Connection, id: i64) -> Result<ChatSession> {
    conn.query_row(
        &format!("SELECT {SESSION_COLS} FROM chat_sessions WHERE id = ?1"),
        params![id],
        row_to_session,
    )
}

pub fn create_session(conn: &Connection, title: &str, model_name: &str) -> Result<ChatSession> {
    let ts = now();
    conn.execute(
        "INSERT INTO chat_sessions (title, model_name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![title, model_name, ts],
    )?;
    get_session(conn, conn.last_insert_rowid())
}

pub fn touch_session(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE chat_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn rename_session(conn: &Connection, id: i64, title: &str) -> Result<ChatSession> {
    conn.execute(
        "UPDATE chat_sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now(), id],
    )?;
    get_session(conn, id)
}

pub fn set_session_model(conn: &Connection, id: i64, model_name: &str) -> Result<ChatSession> {
    conn.execute(
        "UPDATE chat_sessions SET model_name = ?1, updated_at = ?2 WHERE id = ?3",
        params![model_name, now(), id],
    )?;
    get_session(conn, id)
}

pub fn delete_session(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM chat_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_messages(conn: &Connection, session_id: i64) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, created_at FROM chat_messages
         WHERE session_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_message)?;
    rows.collect()
}

/// 最近 N 条消息（按 id 正序返回，用于发送请求时的上下文窗口）。
/// 长对话只取最近一段作为模型上下文，避免全量历史带来的内存尖峰与请求体膨胀。
pub fn list_recent_messages(conn: &Connection, session_id: i64, limit: i64) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, created_at FROM chat_messages
         WHERE session_id = ?1
         ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![session_id, limit], row_to_message)?;
    let mut msgs: Vec<ChatMessage> = rows.collect::<Result<_, _>>()?;
    msgs.reverse();
    Ok(msgs)
}

pub fn add_message(
    conn: &Connection,
    session_id: i64,
    role: &str,
    content: &str,
) -> Result<ChatMessage> {
    let ts = now();
    conn.execute(
        "INSERT INTO chat_messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![session_id, role, content, ts],
    )?;
    let id = conn.last_insert_rowid();
    get_message(conn, id)
}

pub fn get_message(conn: &Connection, id: i64) -> Result<ChatMessage> {
    conn.query_row(
        "SELECT id, session_id, role, content, created_at FROM chat_messages WHERE id = ?1",
        params![id],
        row_to_message,
    )
}

/// 累加一轮回复的 token 用量与生成耗时至会话（input 为本次请求输入，output 为本次生成输出）
pub fn add_session_usage(
    conn: &Connection,
    id: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    reasoning: i64,
    elapsed_ms: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE chat_sessions
         SET tokens_input = tokens_input + ?2,
             tokens_output = tokens_output + ?3,
             tokens_cache_read = tokens_cache_read + ?4,
             tokens_reasoning = tokens_reasoning + ?5,
             elapsed_ms = elapsed_ms + ?6
         WHERE id = ?1",
        params![id, input, output, cache_read, reasoning, elapsed_ms],
    )?;
    Ok(())
}

/// 删除会话内从某条消息起的全部消息（用于中断后清理半截回复）
pub fn delete_messages_from(conn: &Connection, session_id: i64, after_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM chat_messages WHERE session_id = ?1 AND id > ?2",
        params![session_id, after_id],
    )?;
    Ok(())
}

fn row_to_session(row: &rusqlite::Row) -> Result<ChatSession> {
    Ok(ChatSession {
        id: row.get(0)?,
        title: row.get(1)?,
        model_name: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        tokens_input: row.get(5)?,
        tokens_output: row.get(6)?,
        tokens_cache_read: row.get(7)?,
        tokens_reasoning: row.get(8)?,
        elapsed_ms: row.get(9)?,
    })
}

fn row_to_message(row: &rusqlite::Row) -> Result<ChatMessage> {
    Ok(ChatMessage {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
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
    fn session_crud() {
        let conn = setup();
        let s = create_session(&conn, "新对话", "DeepSeek").unwrap();
        assert_eq!(s.title, "新对话");
        assert_eq!(s.model_name, "DeepSeek");

        let renamed = rename_session(&conn, s.id, "面板布局").unwrap();
        assert_eq!(renamed.title, "面板布局");

        let sessions = list_sessions(&conn).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "面板布局");

        delete_session(&conn, s.id).unwrap();
        assert!(list_sessions(&conn).unwrap().is_empty());
    }

    #[test]
    fn message_flow_with_cascade() {
        let conn = setup();
        let s = create_session(&conn, "测试", "DeepSeek").unwrap();
        let u = add_message(&conn, s.id, "user", "你好").unwrap();
        let a = add_message(&conn, s.id, "assistant", "你好！").unwrap();
        let msgs = list_messages(&conn, s.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].content, "你好！");

        // 删除会话应级联删除消息
        delete_session(&conn, s.id).unwrap();
        assert!(list_messages(&conn, s.id).unwrap().is_empty());

        // 独立使用 a/u 的 id 无冲突
        assert_eq!(u.id, a.id - 1);
    }

    #[test]
    fn delete_messages_from_truncates() {
        let conn = setup();
        let s = create_session(&conn, "测试", "DeepSeek").unwrap();
        add_message(&conn, s.id, "user", "a").unwrap();
        let b = add_message(&conn, s.id, "assistant", "半截回复").unwrap();
        delete_messages_from(&conn, s.id, b.id - 1).unwrap();
        let msgs = list_messages(&conn, s.id).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }
}
