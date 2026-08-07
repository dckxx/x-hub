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
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          completed_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_todos_created ON todos(created_at DESC);
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
