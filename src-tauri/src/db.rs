use rusqlite::{Connection, Result};
use std::path::Path;

pub fn init(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn init_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![name],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

fn migrate(conn: &Connection) -> Result<()> {
    // 全新安装：直接建合一的 resources 表（kind 含 app/web/file，不再有分组）
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS resources (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
          name TEXT NOT NULL,
          target TEXT NOT NULL,
          category TEXT,
          icon TEXT,
          args TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          last_launched_at TEXT,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS notes (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL DEFAULT '',
          content TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS tags (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS note_tags (
          note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
          tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
          PRIMARY KEY (note_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS todos (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL,
          done INTEGER NOT NULL DEFAULT 0,
          priority INTEGER NOT NULL DEFAULT 0,
          -- 截止 / 提醒时刻（毫秒时间戳，均可空）；remind_fired 防重复提醒
          due_at INTEGER,
          remind_at INTEGER,
          remind_fired INTEGER NOT NULL DEFAULT 0,
          -- 子待办父级（删除父条目时经外键级联删除子条目）
          parent_id INTEGER REFERENCES todos(id) ON DELETE CASCADE,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          completed_at TEXT
        );

        CREATE TABLE IF NOT EXISTS stickies (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          slot INTEGER NOT NULL UNIQUE CHECK (slot IN (1, 2)),
          content TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS detached_stickies (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          slot INTEGER NOT NULL UNIQUE CHECK (slot IN (1, 2)),
          content TEXT NOT NULL DEFAULT '',
          x REAL,
          y REAL,
          always_on_top INTEGER NOT NULL DEFAULT 1,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS snippets (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL,
          content TEXT NOT NULL,
          is_pinned INTEGER NOT NULL DEFAULT 0,
          copy_count INTEGER NOT NULL DEFAULT 0,
          last_copied_at TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS countdowns (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          -- 重复模式：once 一次性 / daily 每天固定时刻 / interval 每隔 N 分钟
          repeat_mode TEXT NOT NULL DEFAULT 'once' CHECK (repeat_mode IN ('once', 'daily', 'interval')),
          -- 下一次到点时刻（毫秒时间戳）；once 为绝对时刻，daily 为当天 HH:MM，interval 为当前轮结束时刻
          end_at INTEGER NOT NULL,
          -- 周期总长（毫秒）：once 为创建时长，daily 为 24h，interval 为 interval_minutes*60000；用于水位进度计算
          total_ms INTEGER NOT NULL DEFAULT 0,
          -- interval 专用：间隔分钟数
          interval_minutes INTEGER,
          -- 暂停：once 冻结剩余时长，daily/interval 到点不提醒
          paused INTEGER NOT NULL DEFAULT 0,
          -- once 暂停时冻结的剩余毫秒（恢复时 end_at = now + paused_remaining_ms）
          paused_remaining_ms INTEGER,
          -- 工作台卡片不可见导致的自动冻结（区别于手动暂停；卡片恢复显示或浮窗浮起时自动恢复）
          auto_paused INTEGER NOT NULL DEFAULT 0,
          -- once 到点后置 1（卡片灰态，等手动删除）；daily/interval 永不置 1
          finished INTEGER NOT NULL DEFAULT 0,
          -- 浮窗状态与位置（浮起时创建独立透明圆窗，位置随拖动持久化）
          floated INTEGER NOT NULL DEFAULT 0,
          float_x REAL,
          float_y REAL,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS chat_sessions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL DEFAULT '新对话',
          model_name TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          -- 会话级累计 token（每轮回复完成后累加，用于面板顶部实时统计）
          tokens_input INTEGER NOT NULL DEFAULT 0,
          tokens_output INTEGER NOT NULL DEFAULT 0,
          tokens_cache_read INTEGER NOT NULL DEFAULT 0,
          tokens_reasoning INTEGER NOT NULL DEFAULT 0,
          -- 会话级累计生成耗时（毫秒），用于计算 TPS（输出 token / 秒）
          elapsed_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS chat_messages (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          session_id INTEGER NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
          role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
          content TEXT NOT NULL DEFAULT '',
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS clipboard_history (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          content TEXT NOT NULL,
          html TEXT,
          source_app TEXT,
          is_pinned INTEGER NOT NULL DEFAULT 0,
          kind TEXT NOT NULL DEFAULT 'text',
          image_path TEXT,
          file_paths TEXT,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_todos_created ON todos(created_at DESC);
        -- 注意：idx_todos_parent / idx_todos_remind 不能放进本批次——老库的 todos 表
        -- 尚无 parent_id/remind_at 列，CREATE INDEX 会在补列迁移前失败；
        -- 两者统一在 migrate() 尾部（ALTER 补列之后）创建。
        CREATE INDEX IF NOT EXISTS idx_countdowns_end ON countdowns(end_at);
        CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_clipboard_updated ON clipboard_history(updated_at DESC);
        ",
    )?;

    // ---- 旧版本迁移（分组模型 -> 合一模型） ----

    // 旧 resources 表含 group_id 列：重建为合一表，去掉分组外键
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resources)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if cols.iter().any(|c| c == "group_id") {
        // 旧表可能缺 last_launched_at（极早期版本），重建前先补上，保证 SELECT 不失败
        if !cols.iter().any(|c| c == "last_launched_at") {
            conn.execute("ALTER TABLE resources ADD COLUMN last_launched_at TEXT", [])?;
        }
        conn.execute_batch(
            "
            ALTER TABLE resources RENAME TO resources_old;
            CREATE TABLE resources (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
              name TEXT NOT NULL,
              target TEXT NOT NULL,
              category TEXT,
              icon TEXT,
              args TEXT,
              sort_order INTEGER NOT NULL DEFAULT 0,
              last_launched_at TEXT,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO resources (id, kind, name, target, icon, args, sort_order, last_launched_at, created_at, updated_at)
              SELECT id, kind, name, target, icon, args, sort_order, last_launched_at, created_at, updated_at FROM resources_old;
            DROP TABLE resources_old;
            ",
        )?;
    }

    // 旧 files 表：全部并入 resources（kind='file'，target=path，category 保留），然后删除
    if table_exists(conn, "files") {
        conn.execute(
            "INSERT INTO resources (kind, name, target, category, created_at, updated_at)
             SELECT 'file', name, path, category, created_at, updated_at FROM files",
            [],
        )?;
        conn.execute("DROP TABLE files", [])?;
    }

    // 旧 groups 表及其索引：数据层已合一，不再使用
    if table_exists(conn, "groups") {
        conn.execute("DROP TABLE groups", [])?;
    }
    conn.execute("DROP INDEX IF EXISTS idx_resources_group", [])?;
    conn.execute("DROP INDEX IF EXISTS idx_files_category", [])?;

    // 旧 ai_usage 表：AI 用量统计已拆分为扩展（com.x-hub.token-stats），宿主不再使用，删表清残留
    if table_exists(conn, "ai_usage") {
        conn.execute("DROP TABLE ai_usage", [])?;
    }
    conn.execute("DROP INDEX IF EXISTS idx_ai_usage_time", [])?;

    // 兜底：resources 表缺 last_launched_at 列时补充
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resources)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !cols.iter().any(|c| c == "last_launched_at") {
        conn.execute("ALTER TABLE resources ADD COLUMN last_launched_at TEXT", [])?;
    }

    // 索引建立在迁移完成之后（旧表重建前没有 category 列，不能提前建）
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_resources_category ON resources(category)",
        [],
    )?;

    // 旧 chat_sessions 表缺 token 累计列：逐列补齐（ALTER TABLE ADD COLUMN 幂等）
    let chat_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(chat_sessions)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    for (col, def) in [
        ("tokens_input", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_output", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_cache_read", "INTEGER NOT NULL DEFAULT 0"),
        ("tokens_reasoning", "INTEGER NOT NULL DEFAULT 0"),
        ("elapsed_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !chat_cols.iter().any(|c| c == col) {
            conn.execute(
                &format!("ALTER TABLE chat_sessions ADD COLUMN {col} {def}"),
                [],
            )?;
        }
    }

    // 剪贴板历史从「纯文本」升级为「文本/图片/文件」三类型：逐列补齐（ALTER TABLE ADD COLUMN 幂等）
    let clip_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(clipboard_history)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    for (col, def) in [
        ("kind", "TEXT NOT NULL DEFAULT 'text'"),
        ("image_path", "TEXT"),
        ("file_paths", "TEXT"),
    ] {
        if !clip_cols.iter().any(|c| c == col) {
            conn.execute(
                &format!("ALTER TABLE clipboard_history ADD COLUMN {col} {def}"),
                [],
            )?;
        }
    }

    // 倒计时表补 auto_paused 列（工作台卡片不可见时的自动冻结标记，ALTER TABLE ADD COLUMN 幂等）
    let cd_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(countdowns)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !cd_cols.iter().any(|c| c == "auto_paused") {
        conn.execute(
            "ALTER TABLE countdowns ADD COLUMN auto_paused INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // 待办表补截止/提醒/子待办列（v0.3.4，ALTER TABLE ADD COLUMN 幂等；
    // parent_id 带 REFERENCES 子句时 SQLite 要求默认值为 NULL，恰好就是所需默认）
    let todo_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(todos)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !todo_cols.iter().any(|c| c == "due_at") {
        conn.execute("ALTER TABLE todos ADD COLUMN due_at INTEGER", [])?;
    }
    if !todo_cols.iter().any(|c| c == "remind_at") {
        conn.execute("ALTER TABLE todos ADD COLUMN remind_at INTEGER", [])?;
    }
    if !todo_cols.iter().any(|c| c == "remind_fired") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN remind_fired INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "parent_id") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN parent_id INTEGER REFERENCES todos(id) ON DELETE CASCADE",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todos_parent ON todos(parent_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todos_remind ON todos(remind_at)",
        [],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn fresh_db_has_unified_resources() {
        let conn = init_in_memory().unwrap();
        conn.execute(
            "INSERT INTO resources (kind, name, target, category) VALUES ('app', 'VS Code', '/x/code.exe', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO resources (kind, name, target, category) VALUES ('file', '报告', 'C:/docs/report.pdf', '文档')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        assert!(!table_exists(&conn, "groups"));
        assert!(!table_exists(&conn, "files"));
    }

    #[test]
    fn legacy_group_files_schema_migrates() {
        // 构造旧版库：groups + 带 group_id 的 resources + files
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE groups (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE resources (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
              kind TEXT NOT NULL CHECK (kind IN ('app', 'web')),
              name TEXT NOT NULL,
              target TEXT NOT NULL,
              icon TEXT,
              args TEXT,
              sort_order INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE files (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              path TEXT NOT NULL,
              category TEXT NOT NULL DEFAULT '其他',
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO groups (id, name) VALUES (1, '开发');
            INSERT INTO resources (id, group_id, kind, name, target) VALUES (1, 1, 'app', 'VS Code', '/x/code.exe');
            INSERT INTO resources (id, group_id, kind, name, target) VALUES (2, 1, 'web', 'GitHub', 'https://github.com');
            INSERT INTO files (id, name, path, category) VALUES (1, '报告', 'C:/docs/report.pdf', '文档');
            INSERT INTO files (id, name, path, category) VALUES (2, '照片', 'C:/pics/a.png', '图片');
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 4);
        assert!(!table_exists(&conn, "groups"));
        assert!(!table_exists(&conn, "files"));

        // 文件已并入，kind='file' 且分类保留
        let files: Vec<(String, String, Option<String>)> = conn
            .prepare("SELECT name, kind, category FROM resources WHERE kind = 'file' ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].1, "file");
        assert_eq!(files[0].2.as_deref(), Some("文档"));
        assert_eq!(files[1].2.as_deref(), Some("图片"));

        // 原 app/web 资源保留
        let apps: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM resources WHERE kind IN ('app', 'web')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(apps, 2);
    }

    #[test]
    fn legacy_todos_schema_migrates() {
        // 构造旧版库：todos 表无 due_at/remind_at/remind_fired/parent_id 列（v0.3.3 及更早），
        // 且已有数据。迁移必须先补列、后建引用新列的索引——若 CREATE INDEX 混在
        // migrate() 开头的建表批次里，老库会因「no such column: parent_id」启动崩溃。
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE todos (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL,
              done INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              completed_at TEXT
            );
            INSERT INTO todos (title, done) VALUES ('迁移前的老待办', 0);
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        // 新列已补齐
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(todos)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        for col in ["due_at", "remind_at", "remind_fired", "parent_id"] {
            assert!(cols.iter().any(|c| c == col), "缺列 {col}");
        }

        // 引用新列的索引已创建
        for idx in ["idx_todos_parent", "idx_todos_remind"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    params![idx],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "缺索引 {idx}");
        }

        // 老数据完好，且新列取默认值
        let (title, due, parent): (String, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT title, due_at, parent_id FROM todos WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "迁移前的老待办");
        assert_eq!(due, None);
        assert_eq!(parent, None);
    }

    #[test]
    fn legacy_v033_full_schema_migrates_twice_and_works() {
        // v0.3.3 → v0.4.1 直接升级路径：0.3.3 与 0.4.1 的建表差异只有两处——
        // todos 缺 due_at/remind_at/remind_fired/parent_id，countdowns 缺 auto_paused；
        // 其余表结构相同（由 IF NOT EXISTS 兜底）。本测试构造这两张差异表 + 存量数据，
        // 验证：migrate 连跑两次都成功（幂等）、老数据无损、业务层 repo 正常读写、级联删除生效。
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "
            CREATE TABLE todos (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL,
              done INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              completed_at TEXT
            );
            CREATE TABLE countdowns (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              repeat_mode TEXT NOT NULL DEFAULT 'once' CHECK (repeat_mode IN ('once', 'daily', 'interval')),
              end_at INTEGER NOT NULL,
              total_ms INTEGER NOT NULL DEFAULT 0,
              interval_minutes INTEGER,
              paused INTEGER NOT NULL DEFAULT 0,
              paused_remaining_ms INTEGER,
              finished INTEGER NOT NULL DEFAULT 0,
              floated INTEGER NOT NULL DEFAULT 0,
              float_x REAL,
              float_y REAL,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            INSERT INTO todos (title, done) VALUES ('升级前待办A', 0);
            INSERT INTO todos (title, done, completed_at) VALUES ('升级前待办B', 1, '2026-08-01 10:00:00.000');
            INSERT INTO countdowns (name, end_at, total_ms) VALUES ('升级前倒计时', 9999999999999, 60000);
            ",
        )
        .unwrap();

        // 连跑两次：第二次验证已迁移库上的幂等性（不重复加列/建索引报错）
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        // 差异列全部补齐
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(todos)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        for col in ["due_at", "remind_at", "remind_fired", "parent_id"] {
            assert!(cols.iter().any(|c| c == col), "todos 缺列 {col}");
        }
        let cd_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(countdowns)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        assert!(cd_cols.iter().any(|c| c == "auto_paused"), "countdowns 缺 auto_paused");

        // 存量数据无损
        let todo_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(todo_n, 2);
        let cd_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM countdowns", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cd_n, 1);

        // 业务层读取正常（SELECT 引用全部新列，迁移不完整会在此暴露）
        let todos = crate::repo::todo::list(&conn).unwrap();
        assert_eq!(todos.len(), 2);
        assert!(todos.iter().all(|t| t.due_at.is_none() && t.parent_id.is_none()));

        // 新能力可用：父子级联删除（外键 ON DELETE CASCADE 经 ALTER 补列后生效）
        let parent = crate::repo::todo::create(&conn, "父待办", None, None).unwrap();
        let child = crate::repo::todo::create(&conn, "子待办", Some(parent.id), None).unwrap();
        let removed = crate::repo::todo::delete(&conn, parent.id).unwrap();
        assert_eq!(removed, vec![child.id]);
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM todos WHERE id IN (?1, ?2)", params![parent.id, child.id], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn ai_usage_table_is_dropped_on_migrate() {
        // 构造旧版库：含 ai_usage 表（旧版 token 统计专用）及其索引，模拟老用户升级
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE ai_usage (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              session_id TEXT NOT NULL DEFAULT '',
              provider TEXT,
              model TEXT,
              tokens_input INTEGER NOT NULL DEFAULT 0,
              tokens_cache_read INTEGER NOT NULL DEFAULT 0,
              tokens_cache_write INTEGER NOT NULL DEFAULT 0,
              tokens_output INTEGER NOT NULL DEFAULT 0,
              tokens_reasoning INTEGER NOT NULL DEFAULT 0,
              cost REAL NOT NULL DEFAULT 0,
              time_created INTEGER NOT NULL,
              source TEXT NOT NULL DEFAULT 'remote'
            );
            CREATE INDEX idx_ai_usage_time ON ai_usage(time_created);
            INSERT INTO ai_usage (session_id, tokens_input, tokens_output, time_created)
              VALUES ('s1', 100, 200, 1);
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();

        // ai_usage 表与索引被清除，核心表正常建立（其它数据不受影响）
        assert!(!table_exists(&conn, "ai_usage"));
        assert!(table_exists(&conn, "resources"));
        assert!(table_exists(&conn, "notes"));
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_ai_usage_time'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 0);
    }
}
