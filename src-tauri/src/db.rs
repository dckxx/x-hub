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
          description TEXT,
          remark TEXT,
          remark_label TEXT,
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
          -- 手动拖拽排序位（分组内升序；NULL = 未手动排序，组内按创建时间倒序）
          sort_order INTEGER,
          -- 乐观锁版本号（局域网同步冲突检测：每次写回 +1）
          version INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
          completed_at TEXT,
          -- 轻量 Markdown 正文（勾选一律用子待办，正文不放 checklist）
          description TEXT NOT NULL DEFAULT '',
          -- 置顶：脱离日期分组，固定排在列表最顶部「置顶」区
          pinned INTEGER NOT NULL DEFAULT 0,
          -- 周期规则总开关：once 一次性 / daily / weekly / monthly / yearly / weekdays / custom
          repeat_mode TEXT NOT NULL DEFAULT 'once',
          -- custom：每 N 个 repeat_unit
          repeat_every INTEGER,
          repeat_unit TEXT,
          -- 位掩码 bit0=周一 … bit6=周日（weekly 多选、custom+week 用）
          repeat_weekdays INTEGER,
          -- monthly：1..31，-1 = 月末
          repeat_month_day INTEGER,
          -- monthly：第几个（1..5，-1 = 最后一个），非空时与 repeat_weekdays 组合表达「第几个星期几」
          repeat_month_nth INTEGER,
          -- 结束条件：never / until / count
          repeat_end_mode TEXT,
          repeat_end_at INTEGER,
          repeat_count INTEGER,
          -- 累计完成次数与上次完成时间（统计用，不逐次留历史）
          repeat_done_count INTEGER NOT NULL DEFAULT 0,
          repeat_last_done_at TEXT
        );

        -- 待办标签：与笔记标签（tags / note_tags）是两套独立定义，互不同步
        CREATE TABLE IF NOT EXISTS todo_tags (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE,
          -- '' = 用默认色；否则 #rrggbb
          color TEXT NOT NULL DEFAULT '',
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        CREATE TABLE IF NOT EXISTS todo_tag_links (
          todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
          tag_id INTEGER NOT NULL REFERENCES todo_tags(id) ON DELETE CASCADE,
          PRIMARY KEY (todo_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS stickies (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          slot INTEGER NOT NULL UNIQUE CHECK (slot IN (1, 2)),
          content TEXT NOT NULL DEFAULT '',
          -- 乐观锁版本号
          version INTEGER NOT NULL DEFAULT 0,
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
          -- 乐观锁版本号
          version INTEGER NOT NULL DEFAULT 0,
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

    // 速达小类（ADR 0012）：大类（resources.kind）下单归属的小类库。
    // 文件大类的 7 个内置 category 直接作为初始小类并入（名称与前端 utils/categories.ts
    // 逐字一致），「其他」暂承接默认小类（细则待用户确认）；应用/网页初始为空、按需新建。
    // INSERT OR IGNORE + UNIQUE(kind,name) 保证幂等，老库升级与新装库都只补缺、不覆盖。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS resource_subcategories (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          kind TEXT NOT NULL CHECK (kind IN ('app', 'web', 'file')),
          name TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          is_default INTEGER NOT NULL DEFAULT 0,
          UNIQUE (kind, name)
        );

        INSERT OR IGNORE INTO resource_subcategories (kind, name, sort_order, is_default) VALUES
          ('file', '文件夹', 0, 0),
          ('file', '文档', 1, 0),
          ('file', '图片', 2, 0),
          ('file', '视频', 3, 0),
          ('file', '音频', 4, 0),
          ('file', '压缩包', 5, 0),
          ('file', '其他', 6, 1);
        ",
    )?;

    // 速达分区：「全部」tab 的自定义成组陈列，独立于小类（无 kind 维度、无层级、跨大类混居）。
    // 一个分区都没有时「全部」保持平铺，行为与分区功能引入前完全一致；成员经
    // resources.zone_id 关联（NULL = 未分区），不设外键约束，级联在 repo::zone 单事务手工做。
    // cols/rows = 分区框尺寸（卡片格数，新建默认 3×2）：是**下限**——内容超出时前端按行自动
    // 膨胀，尺寸只决定空框的最小占位与拖拽缩放的吸附单元。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS resource_zones (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL UNIQUE,
          sort_order INTEGER NOT NULL DEFAULT 0,
          cols INTEGER NOT NULL DEFAULT 3,
          rows INTEGER NOT NULL DEFAULT 2
        );
        ",
    )?;
    // 老库（分区功能首版建的表）补尺寸列，默认 3×2
    let zone_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resource_zones)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !zone_cols.iter().any(|c| c == "cols") {
        conn.execute(
            "ALTER TABLE resource_zones ADD COLUMN cols INTEGER NOT NULL DEFAULT 3",
            [],
        )?;
    }
    if !zone_cols.iter().any(|c| c == "rows") {
        conn.execute(
            "ALTER TABLE resource_zones ADD COLUMN rows INTEGER NOT NULL DEFAULT 2",
            [],
        )?;
    }
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(resources)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !cols.iter().any(|c| c == "zone_id") {
        conn.execute("ALTER TABLE resources ADD COLUMN zone_id INTEGER", [])?;
    }
    // 速达「说明 / 备注 / 备注标签」（备注为 DPAPI 密文，见 secret.rs）：幂等补列
    for (col, def) in [
        ("description", "TEXT"),
        ("remark", "TEXT"),
        ("remark_label", "TEXT"),
    ] {
        if !cols.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE resources ADD COLUMN {col} {def}"), [])?;
        }
    }

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
    // 待办手动排序位（v0.4.3，拖拽排序；NULL = 未手动排序）
    if !todo_cols.iter().any(|c| c == "sort_order") {
        conn.execute("ALTER TABLE todos ADD COLUMN sort_order INTEGER", [])?;
    }
    // 乐观锁版本号（局域网同步冲突检测：写回时 WHERE version 比对，每次 +1）
    if !todo_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 待办 v1（标签 / 置顶 / 周期）：描述 / 置顶 / 周期规则列。全部可空或带 DEFAULT，
    // 老库启动即补齐，零数据改造（与 v0.3.4 补列的既有模式一致）。
    if !todo_cols.iter().any(|c| c == "description") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "pinned") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !todo_cols.iter().any(|c| c == "repeat_mode") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN repeat_mode TEXT NOT NULL DEFAULT 'once'",
            [],
        )?;
    }
    for col in [
        "repeat_every",
        "repeat_weekdays",
        "repeat_month_day",
        "repeat_month_nth",
        "repeat_end_at",
        "repeat_count",
    ] {
        if !todo_cols.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE todos ADD COLUMN {col} INTEGER"), [])?;
        }
    }
    for col in ["repeat_unit", "repeat_end_mode", "repeat_last_done_at"] {
        if !todo_cols.iter().any(|c| c == col) {
            conn.execute(&format!("ALTER TABLE todos ADD COLUMN {col} TEXT"), [])?;
        }
    }
    if !todo_cols.iter().any(|c| c == "repeat_done_count") {
        conn.execute(
            "ALTER TABLE todos ADD COLUMN repeat_done_count INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 便签/浮窗便签表补乐观锁版本号
    let sticky_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(stickies)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !sticky_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE stickies ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let detached_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(detached_stickies)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !detached_cols.iter().any(|c| c == "version") {
        conn.execute(
            "ALTER TABLE detached_stickies ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
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
    // 待办升级新增索引：同样必须在补列之后创建（老库此时才具备这些列）
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_todo_tag_links_tag ON todo_tag_links(tag_id)",
        [],
    )?;
    // 曾建的 idx_todos_repeat 已移除：全工程没有按 repeat_mode 等值过滤的查询
    // （周期展开用 `repeat_mode <> 'once'`，SQLite 不走索引），它只让每次写 todos
    // 多维护一棵 B 树。老库在这里顺手删掉。
    conn.execute("DROP INDEX IF EXISTS idx_todos_repeat", [])?;

    // ---- 速记改造（docs/speednote-plan.md）：文件夹树 / 回收站 / 剪藏来源 / 双链 ----
    // 文件夹树（ADR 0015：单归属、可嵌套不限深度、删除不级联——笔记与子文件夹在
    // repo 层事务内上移一级，这里的 ON DELETE SET NULL 只是兜底）；builtin=1 不可改名/删除。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS note_folders (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          parent_id INTEGER REFERENCES note_folders(id) ON DELETE SET NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          builtin INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
        );

        -- 双链轻量版：引用键 = 标题（不落块 id）。to_note_id 解析到已存在标题时补上；
        -- 引用了不存在的标题保留为「未链接提及」（to_note_id NULL），不算错。
        CREATE TABLE IF NOT EXISTS note_links (
          from_note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
          to_note_id INTEGER REFERENCES notes(id) ON DELETE SET NULL,
          to_title TEXT NOT NULL,
          PRIMARY KEY (from_note_id, to_title)
        );
        ",
    )?;
    // notes 补列（老库幂等）：folder_id=NULL=树根；source_url 剪藏来源兼去重键；deleted_at 有值=在回收站
    let note_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(notes)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !note_cols.iter().any(|c| c == "folder_id") {
        conn.execute(
            "ALTER TABLE notes ADD COLUMN folder_id INTEGER REFERENCES note_folders(id) ON DELETE SET NULL",
            [],
        )?;
    }
    if !note_cols.iter().any(|c| c == "source_url") {
        conn.execute(
            "ALTER TABLE notes ADD COLUMN source_url TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !note_cols.iter().any(|c| c == "deleted_at") {
        conn.execute("ALTER TABLE notes ADD COLUMN deleted_at TEXT", [])?;
    }
    // 笔记自定义树图标（emoji，用户在标题旁设置；NULL = 默认文件图标）
    if !note_cols.iter().any(|c| c == "icon") {
        conn.execute("ALTER TABLE notes ADD COLUMN icon TEXT", [])?;
    }
    // 标签内置位（「剪藏」标签不可改删；老库无此列）
    let tag_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(tags)")?
        .query_map([], |row| row.get(1))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    if !tag_cols.iter().any(|c| c == "builtin") {
        conn.execute(
            "ALTER TABLE tags ADD COLUMN builtin INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    // 内置项种子（幂等）：「剪藏」文件夹（树根）+「剪藏」标签。
    // 标签种子必须先认领同名再插入：tags.name UNIQUE，老用户手动建过「剪藏」时
    // 只查 builtin 会撞 UNIQUE → 迁移失败 → 启动死循环。
    conn.execute_batch(
        "
        INSERT INTO note_folders (name, parent_id, sort_order, builtin)
        SELECT '剪藏', NULL, 0, 1
        WHERE NOT EXISTS (SELECT 1 FROM note_folders WHERE builtin = 1);
        UPDATE tags SET builtin = 1 WHERE name = '剪藏' AND builtin = 0;
        INSERT INTO tags (name, builtin)
        SELECT '剪藏', 1
        WHERE NOT EXISTS (SELECT 1 FROM tags WHERE name = '剪藏');
        ",
    )?;
    // 索引必须在补列之后创建（老库此时才具备这些列）
    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_note_links_to ON note_links(to_note_id);
        CREATE INDEX IF NOT EXISTS idx_notes_folder ON notes(folder_id);
        CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(deleted_at);
        CREATE INDEX IF NOT EXISTS idx_notes_source_url ON notes(source_url);
        CREATE INDEX IF NOT EXISTS idx_notes_title ON notes(title);
        ",
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
        for col in ["due_at", "remind_at", "remind_fired", "parent_id", "sort_order"] {
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
        for col in ["due_at", "remind_at", "remind_fired", "parent_id", "sort_order"] {
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
    fn legacy_notes_schema_migrates_and_seeds_builtin() {
        // 速记改造迁移：老库（notes/tags 无新列、无 note_folders/note_links）补列 + 内置种子 +
        // 索引，连跑两次幂等；业务层 SELECT 引用新列能通过（迁移不完整会在此暴露）。
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "
            CREATE TABLE notes (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              title TEXT NOT NULL DEFAULT '',
              content TEXT NOT NULL DEFAULT '',
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
              updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE tags (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL UNIQUE,
              created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
            );
            CREATE TABLE note_tags (
              note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
              tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
              PRIMARY KEY (note_id, tag_id)
            );
            INSERT INTO notes (title, content) VALUES ('老笔记', '正文');
            INSERT INTO tags (name) VALUES ('工作');
            ",
        )
        .unwrap();

        migrate(&conn).unwrap();
        migrate(&conn).unwrap();

        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(notes)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap();
        for col in ["folder_id", "source_url", "deleted_at"] {
            assert!(cols.iter().any(|c| c == col), "notes 缺列 {col}");
        }
        assert!(table_exists(&conn, "note_folders"));
        assert!(table_exists(&conn, "note_links"));

        // 内置种子只一份
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM note_folders WHERE builtin = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "内置文件夹应只有一条");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM tags WHERE builtin = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "内置标签应只有一条");

        // 老数据无损 + 新列默认值
        let notes = crate::repo::note::list(&conn).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].folder_id, None);
        assert_eq!(notes[0].source_url, "");
        assert_eq!(notes[0].deleted_at, None);

        // 新能力可用：建文件夹 + 移笔记进去（外键生效）
        let f = crate::repo::note_folder::create(&conn, "项目", None).unwrap();
        crate::repo::note::set_folder(&conn, notes[0].id, Some(f.id)).unwrap();
        let got = crate::repo::note::get(&conn, notes[0].id).unwrap();
        assert_eq!(got.folder_id, Some(f.id));
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
