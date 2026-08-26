# 本地文件搜索（索引工作区）方案

> 状态：方案定稿，待实现。生成日期：2026-08-20。

## 1. 背景与目标

现有全局搜索（`Ctrl+K` → `GlobalSearch.vue` → `search_all` 命令）只检索 SQLite 中的三张表：`resources`（速达）、`notes`（速记）、`todos`（待办）。其中的「文件」仅覆盖用户在速达里**手动登记过的单文件快捷方式**，并非真实的文件系统搜索。

本方案目标：在全局搜索中支持搜索**本地磁盘上真实存在的文件与文件夹**，命中后可直接打开或定位。

## 2. 方案定稿：索引工作区模型

不是隐式扫描用户目录，而是由用户**显式注册一批文件夹作为「索引工作区」**。每个工作区 = 一个用户主动引入的本地目录（如 `E:\workspace\x-hub`、`D:\docs`），后台对其递归建索引，搜索只在这批注册目录内进行。范围可控、噪声小、边界清晰。

### 已拍板决策

| 决策点 | 结论 |
|---|---|
| 工作区命名 | 可自定义命名，默认取文件夹名 |
| 建索引时机 | 注册后立即后台建；启动时对 enabled 工作区全量同步重建 |
| 排除目录 | 全局黑名单，可配置 |
| 搜索范围 | 同时匹配文件与文件夹 |
| 定位 | 支持「定位到所在文件夹」（资源管理器选中） |
| 脏数据清理 | 全量同步重建（seen 标记）+ 打开前 `exists` 校验双保险 |
| 与速达联动 | 命中结果可一键「加入速达」 |
| 拼音/模糊 | 第二阶段（索引预留列，MVP 暂不启用） |

## 3. 数据模型（新增三张表）

新增表追加到 `db.rs::migrate()` 的建表块（`CREATE TABLE IF NOT EXISTS`），旧库启动即自动创建，无需数据迁移。

```sql
CREATE TABLE IF NOT EXISTS search_roots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 1,
  file_count INTEGER NOT NULL DEFAULT 0,
  last_indexed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);

CREATE TABLE IF NOT EXISTS file_index (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  root_id INTEGER NOT NULL REFERENCES search_roots(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  is_dir INTEGER NOT NULL DEFAULT 0,
  ext TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL DEFAULT 0,
  modified_at INTEGER NOT NULL DEFAULT 0,
  name_pinyin TEXT NOT NULL DEFAULT '',
  name_pinyin_initial TEXT NOT NULL DEFAULT '',
  seen INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_file_index_root ON file_index(root_id);
CREATE INDEX IF NOT EXISTS idx_file_index_name ON file_index(name);
CREATE INDEX IF NOT EXISTS idx_file_index_path ON file_index(path);

CREATE TABLE IF NOT EXISTS search_exclusions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  pattern TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now'))
);
```

- `file_index.path` 唯一约束，用于重建时 `INSERT ... ON CONFLICT(path) DO UPDATE` 的 upsert。
- `file_index.seen`：重建标记位，用于「全量同步去脏」。
- `name_pinyin` / `name_pinyin_initial`：第二阶段拼音匹配预留列，MVP 阶段留空。
- `search_exclusions`：全局排除黑名单，默认写入 `node_modules`、`.git`、`target`、`$RECYCLE.BIN`、`System Volume Information`、`.idea`、`.vscode`（首次无记录时插入）。

## 4. 后端改动清单

| 文件 | 改动 |
|---|---|
| `Cargo.toml` | 新增 `walkdir = "2"` |
| `models.rs` | 新增 `SearchRoot { id, name, path, enabled, file_count, last_indexed_at }`、`FileHit { name, path, is_dir, ext, size, modified_at }`；`SearchResult` 增加 `files: Vec<FileHit>` |
| `db.rs` | 建表块追加上述三张表 + 索引 |
| `repo/file_index.rs`（新） | 工作区/索引/排除的 CRUD 与查询、遍历建索引、search 匹配逻辑 |
| `commands.rs` | 新增命令（见下表） |
| `lib.rs` | 注册命令；启动时后台线程对 enabled 工作区做全量同步重建 |

### 新增命令（9 个）

| 命令 | 说明 |
|---|---|
| `add_search_root(name, path)` → `SearchRoot` | 注册工作区，成功后 `spawn_blocking` 后台建索引 |
| `remove_search_root(id)` | 删除工作区（`file_index` 级联清空） |
| `list_search_roots()` → `Vec<SearchRoot>` | 列出工作区（含文件数、最近建索引时间） |
| `search_files(keyword)` → `Vec<FileHit>` | 在 `file_index` 匹配文件名 |
| `rebuild_file_index(root_id?)` | 手动重建（root_id 空则全部） |
| `open_path(path)` | 复用 `process::open_path`：文件走系统默认程序、文件夹走 explorer |
| `reveal_path(path)` | Windows `explorer /select,path` 选中；非 Win 打开父目录 |
| `get_search_exclusions()` → `Vec<String>` | 读排除黑名单 |
| `save_search_exclusions(Vec<String>)` | 全量保存排除黑名单（设置页一次编辑） |

`open_path` / `reveal_path` 内部先 `exists` 校验，失效则删除 `file_index` 对应行并返回「文件已不存在」，前端 toast 提示。

## 5. 前端改动清单

| 文件 | 改动 |
|---|---|
| `api/tauri.ts` | 新增 `SearchRoot`、`FileHit` 类型；`SearchResult` 加 `files`；新增 8 个 API（`addSearchRoot` / `removeSearchRoot` / `listSearchRoots` / `searchFiles` / `rebuildFileIndex` / `openPath` / `revealPath` / `getExclusions` / `saveExclusions`） |
| `stores/workbench.ts` | 对应 store 方法（含 `searchFiles` 供 GlobalSearch 用，浏览器预览环境 `isTauri()` 守卫返回空） |
| `components/GlobalSearch.vue` | 增加「文件」分组（紫色 file badge，文件夹用独立 badge；子行显示路径）；文件结果行复用 `ContextMenu.vue` 右键菜单：打开 / 定位 / 加入速达；空态文案更新 |
| `index.vue` | 处理 `openFile` / `revealFile` / `addFileToSuda` 事件 |
| `SettingsView.vue`（数据区） | 新增「索引目录」管理块：工作区增删（可命名）、显示文件数与最近建索引时间、一键重建；「排除目录」黑名单编辑 |

## 6. 匹配与排序

- 关键词 `trim` 后非空才检索。
- **前缀命中**（文件名 `LIKE 'kw%'`）优先于**子串命中**（`LIKE '%kw%'`）。
- 组内按 `modified_at` 倒序；大小写不敏感（SQLite `LIKE` 对 ASCII 默认不敏感）。
- 文件与文件夹不额外区分权重，统一按「前缀 → 子串 → 修改时间」排序。
- 结果上限（如 50 条），避免弹窗列表过长。

## 7. 索引与去脏策略

- **建索引**：`tauri::async_runtime::spawn_blocking` 内 `walkdir` 遍历（`filter_entry` 跳过排除目录，目录名忽略大小写匹配黑名单），事务批量 upsert（`ON CONFLICT(path)`），每约 500 条提交一次，结束更新 `search_roots.file_count` 与 `last_indexed_at`。
- **去脏（双保险）**：
  1. 重建采用「seen 标记」：先 `UPDATE file_index SET seen=0 WHERE root_id=?`，遍历时把命中项 `seen=1`，结束 `DELETE FROM file_index WHERE root_id=? AND seen=0`（天然清理被删/移动的文件）。
  2. 打开/定位前 `exists` 校验，失效即删索引行并提示。
- **启动**：对每个 enabled 工作区后台做一次全量同步重建（默认），保证日常脏数据不积累。

## 8. 与速达联动

命中结果右键「加入速达」→ 复用现有 `store.addResource({ kind: 'file', name, target: path, category })`，category 按扩展名归类（`utils/categories`），图标走名称 hash 首字母（`useResourceIcon`）。零后端改动。

## 9. 阶段拆分

**MVP（第一阶段）**：上表全部内容，**除拼音/模糊外**全部落地。

**第二阶段（可选增强）**：
- 拼音匹配：引入拼音库生成 `name_pinyin` + `name_pinyin_initial` 两列，关键词为字母时同时匹配。
- 子序列 fuzzy 加分。
- `notify` 文件监听实时增量。
- 按工作区单独配置排除列表。

## 10. 验收要点

1. 注册工作区后，能搜到其下文件与文件夹，前缀命中排在子串前。
2. 点击文件/文件夹能正确打开（文件走默认程序、文件夹走资源管理器）。
3. 「定位」能在资源管理器中打开并选中该文件。
4. 排除目录中的文件不出现在结果里。
5. 磁盘删除文件后，重建或打开时能被清理，不再返回脏结果。
6. 「加入速达」后该文件出现在速达列表且可正常启动。
7. 浏览器预览环境（非 Tauri）不崩溃（`isTauri()` 守卫）。
