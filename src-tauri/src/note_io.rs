use crate::commands::DbState;
use crate::models::{Note, NoteFolder, Tag};
use crate::repo::{note, note_folder, tag};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, State};

/// 速记导出/导入（docs/speednote-plan.md §8）：
/// - 导出：按文件夹树建目录，每条笔记一个 .md + front-matter（title/created_at/source_url/tags/folder），
///   xhub-note 图片 URL 改相对路径 `assets/<hash>.<ext>` 并把用到的图复制进 assets/。
/// - 导入（限定 (a) 档）：只导**我们自己导出的产物**（front-matter 格式已知、闭环）；
///   Obsidian/思源迁移明确不做（无真实样本，做了也白做）。
/// - 手写 `key: value` 行解析，不引 YAML 依赖（serde_yaml 已停维护）。
/// - 逐条事务，失败不回滚整体；导入可取消（IMPORT_CANCELLED 原子标志，逐条检查）。

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub exported: usize,
    /// 复制进 assets/ 的图片数
    pub images: usize,
    pub failed: Vec<String>,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    /// source_url 命中已有笔记而跳过（去重）
    pub skipped: usize,
    /// 失败条目（文件名 + 原因）
    pub failed: Vec<String>,
    pub cancelled: bool,
}

static IMPORT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// 取消正在进行的导入（幂等；无导入在进行时无副作用）
#[tauri::command]
pub fn import_notes_cancel() -> Result<(), String> {
    IMPORT_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn export_notes(state: State<'_, DbState>, dir: String) -> Result<ExportReport, String> {
    // 数据快照只在锁内读一下，重文件 IO 全程不持锁：导入/导出是分钟级操作，
    // 长持 std Mutex 会把主线程上执行的同步 DB 命令全部卡住（= 整窗冻结）
    let snapshot = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        read_export_snapshot(&conn)?
    };
    tokio::task::block_in_place(|| export_notes_sync(snapshot, Path::new(&dir)))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ImportOptions {
    /// source_url 命中已有笔记时是否覆盖（默认跳过）
    pub overwrite: bool,
}

#[tauri::command]
pub async fn import_notes(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    dir: String,
    opts: Option<ImportOptions>,
) -> Result<ImportReport, String> {
    IMPORT_CANCELLED.store(false, Ordering::SeqCst);
    // 逐条短暂持锁（见 export_notes 注释）：文件 IO/解析在锁外，DB 段每条一锁
    tokio::task::block_in_place(|| {
        import_notes_sync(&app, &state.0, Path::new(&dir), opts.unwrap_or_default())
    })
}

// ---------- 导出 ----------

/// 导出快照：四张表一次读齐，之后导出全程不再碰数据库
type ExportSnapshot = (Vec<Note>, Vec<NoteFolder>, Vec<Tag>, Vec<(i64, i64)>);

fn read_export_snapshot(conn: &Connection) -> Result<ExportSnapshot, String> {
    let notes = note::list_full(conn, false).map_err(|e| e.to_string())?;
    let folders = note_folder::list(conn).map_err(|e| e.to_string())?;
    let tags = tag::list(conn).map_err(|e| e.to_string())?;
    let note_tags = tag::list_note_tags(conn).map_err(|e| e.to_string())?;
    Ok((notes, folders, tags, note_tags))
}

fn export_notes_sync(
    (notes, folders, tags, note_tags): ExportSnapshot,
    root: &Path,
) -> Result<ExportReport, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let mut tag_names: HashMap<i64, String> = HashMap::new();
    for t in &tags {
        tag_names.insert(t.id, t.name.clone());
    }
    let mut tags_of: HashMap<i64, Vec<String>> = HashMap::new();
    for (nid, tid) in &note_tags {
        if let Some(name) = tag_names.get(tid) {
            tags_of.entry(*nid).or_default().push(name.clone());
        }
    }

    // 文件夹 id → 相对路径（sanitized，已建目录）
    let mut dir_cache: HashMap<i64, PathBuf> = HashMap::new();
    let mut used_files: HashSet<String> = HashSet::new();
    let mut copied_images: HashSet<String> = HashSet::new();
    let assets_dir = root.join("assets");
    let mut report = ExportReport {
        exported: 0,
        images: 0,
        failed: Vec::new(),
        dir: root.display().to_string(),
    };
    let images_root = crate::paths::data_root().join("notes").join("images");

    for n in &notes {
        let folder_dir = match n.folder_id {
            Some(fid) => match ensure_folder_dir(&folders, fid, root, &mut dir_cache) {
                Ok(p) => p,
                Err(e) => {
                    report.failed.push(format!("{}: {e}", n.title));
                    continue;
                }
            },
            None => root.to_path_buf(),
        };
        // 文件名 = 清洗后的标题 + 去重
        let stem = unique_filename(&n.title, &folder_dir, &mut used_files);
        let mut body = rewrite_image_urls_to_relative(&n.content);
        // 收集引用的图片并复制
        for name in extract_asset_names(&body) {
            if copied_images.insert(name.clone()) {
                let src = images_root.join(&name);
                if src.is_file() {
                    std::fs::create_dir_all(&assets_dir).ok();
                    if let Err(e) = std::fs::copy(&src, assets_dir.join(&name)) {
                        report.failed.push(format!("{} 图片 {name}: {e}", n.title));
                    } else {
                        report.images += 1;
                    }
                }
            }
        }
        let mut md = String::new();
        md.push_str("---\n");
        md.push_str(&format!("title: {}\n", n.title));
        md.push_str(&format!("created_at: {}\n", n.created_at));
        md.push_str(&format!("source_url: {}\n", n.source_url));
        md.push_str(&format!("folder: {}\n", folder_rel_path(&folders, n.folder_id)));
        if let Some(icon) = n.icon.as_deref().filter(|s| !s.is_empty()) {
            md.push_str(&format!("icon: {}\n", icon));
        }
        md.push_str(&format!("tags: {}\n", tags_of.get(&n.id).cloned().unwrap_or_default().join(", ")));
        md.push_str("---\n\n");
        body = body.trim_start().to_string();
        md.push_str(&body);
        if !body.is_empty() && !body.ends_with('\n') {
            md.push('\n');
        }
        let path = folder_dir.join(format!("{stem}.md"));
        if let Err(e) = std::fs::write(&path, md) {
            report.failed.push(format!("{}: {e}", n.title));
            continue;
        }
        report.exported += 1;
    }
    if !report.failed.is_empty() {
        log::warn!("速记导出部分失败: {} 条", report.failed.len());
    }
    log::info!(
        "速记导出完成: {} 条笔记，{} 张图片 → {}",
        report.exported,
        report.images,
        root.display()
    );
    Ok(report)
}

/// 文件夹 id 的导出目录（递归父级，全部创建）；dir_cache 记忆已建目录
fn ensure_folder_dir(
    folders: &[crate::models::NoteFolder],
    id: i64,
    root: &Path,
    cache: &mut HashMap<i64, PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(p) = cache.get(&id) {
        return Ok(p.clone());
    }
    let f = folders
        .iter()
        .find(|f| f.id == id)
        .ok_or_else(|| format!("文件夹 {id} 不存在"))?;
    let parent_dir = match f.parent_id {
        Some(pid) => ensure_folder_dir(folders, pid, root, cache)?,
        None => root.to_path_buf(),
    };
    let dir = parent_dir.join(sanitize_filename(&f.name));
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    cache.insert(id, dir.clone());
    Ok(dir)
}

/// front-matter `folder:` 字段用的相对路径（逻辑名，未清洗；展示用）
fn folder_rel_path(folders: &[crate::models::NoteFolder], folder_id: Option<i64>) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut cursor = folder_id;
    let mut hops = 0;
    while let Some(cid) = cursor {
        hops += 1;
        if hops > 100 {
            break;
        }
        match folders.iter().find(|f| f.id == cid) {
            Some(f) => {
                names.push(f.name.clone());
                cursor = f.parent_id;
            }
            None => break,
        }
    }
    names.reverse();
    names.join("/")
}

/// Windows 文件名清洗（方案 §8.1）：非法字符替换、结尾点/空格去掉、保留名加前缀、超长截断
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    let mut s = cleaned.trim().to_string();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    if s.is_empty() {
        return "无标题笔记".to_string();
    }
    // 保留名按「点号前的主干」判断（CON.md 也算保留名）
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        s = format!("_{s}");
    }
    // 200 字符上限（字符数而非字节，避免截断出半个中文）
    if s.chars().count() > 200 {
        s = s.chars().take(200).collect();
        while s.ends_with('.') || s.ends_with(' ') {
            s.pop();
        }
    }
    s
}

/// 同目录唯一文件名：重名加 ` (2)`、` (3)`…
fn unique_filename(title: &str, dir: &Path, used: &mut HashSet<String>) -> String {
    let base = sanitize_filename(title);
    let mut candidate = base.clone();
    let mut n = 2;
    loop {
        let key = format!("{}\u{0}{}", dir.display(), candidate);
        let taken = used.contains(&key)
            || dir.join(format!("{candidate}.md")).exists()
            || dir.join(&candidate).exists();
        if !taken {
            used.insert(key);
            return candidate;
        }
        candidate = format!("{base} ({n})");
        n += 1;
    }
}

/// 正文里的 xhub-note 图片 URL 改相对路径 assets/<name>（导出：只动 xhub-note URL，无误伤）
fn rewrite_image_urls_to_relative(content: &str) -> String {
    content.replace(NOTE_URL_PREFIX, "assets/")
}

/// 导入侧的反向改写：只认 `assets/<16位哈希>.<ext>` 形态（导出产物的图片约定），
/// 用户正文里自写的 assets/ 链接（非哈希名）不会被误改
fn rewrite_asset_urls(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(pos) = rest.find("assets/") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + "assets/".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let stem = name.split('.').next().unwrap_or("");
        let is_image =
            stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && name.contains('.');
        if is_image {
            out.push_str(NOTE_URL_PREFIX);
            out.push_str(&name);
            rest = tail;
        } else {
            out.push_str("assets/");
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

const NOTE_URL_PREFIX: &str = "http://xhub-note.localhost/";

/// 从改写后的正文里抽 assets/<name> 引用名
fn extract_asset_names(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find("assets/") {
        let tail = &rest[pos + "assets/".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let stem = name.split('.').next().unwrap_or("");
        if stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && name.contains('.') {
            names.push(name.clone());
        }
        rest = tail;
    }
    names
}

// ---------- 导入 ----------

/// 解析 front-matter：首行 `---`，到下一个 `---` 行为止的 `key: value` 行。
/// 复杂结构（嵌套/列表）直接忽略该字段——我们自己的导出产物只有平铺键值。
/// 逐行按 `\n` 定位原始行（`\r` 剥在行尾）：偏移天然兼容 LF/CRLF——`str::lines()`
/// 会把 `\r\n` 整体剥掉，若按 `len()+1` 累计偏移，CRLF 文件正文起点每行前移
/// 1 字节，多行 front-matter 时切片错位甚至落进多字节字符中间直接 panic。
pub fn parse_front_matter(md: &str) -> (HashMap<String, String>, String) {
    let text = md.strip_prefix('\u{feff}').unwrap_or(md);
    let mut keys = HashMap::new();
    let mut rest = text;
    let mut first = true;
    while let Some(nl) = rest.find('\n') {
        let mut line = &rest[..nl];
        rest = &rest[nl + 1..];
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
        if first {
            first = false;
            if line.trim() != "---" {
                return (HashMap::new(), md.to_string());
            }
            continue;
        }
        if line.trim() == "---" {
            return (keys, rest.to_string());
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if !k.is_empty() && !k.contains(' ') {
                keys.insert(k, v);
            }
        }
    }
    // 没等到闭合 `---`：按无 front-matter 处理，正文原样保留（宁漏勿改）
    (HashMap::new(), md.to_string())
}

fn import_notes_sync(
    app: &tauri::AppHandle,
    conn: &Mutex<Connection>,
    root: &Path,
    opts: ImportOptions,
) -> Result<ImportReport, String> {
    if !root.is_dir() {
        return Err(format!("目录不存在: {}", root.display()));
    }
    let mut report = ImportReport {
        imported: 0,
        skipped: 0,
        failed: Vec::new(),
        cancelled: false,
    };
    let mut md_files: Vec<PathBuf> = Vec::new();
    collect_md_files(root, &mut md_files, 0);
    if md_files.is_empty() {
        return Err("所选目录下没有 .md 文件".into());
    }
    let images_root = crate::paths::data_root().join("notes").join("images");
    std::fs::create_dir_all(&images_root).map_err(|e| e.to_string())?;
    // 文件夹路径 → id 缓存（"" = 树根）
    let mut folder_cache: HashMap<String, Option<i64>> = HashMap::new();

    for path in md_files {
        if IMPORT_CANCELLED.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                report.failed.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let (meta, body) = parse_front_matter(&content);
        if meta.is_empty() {
            // 没有 front-matter 的 .md 不是我们的导出产物：跳过并说明
            report.failed.push(format!("{rel}: 缺少 front-matter（仅支持导入 x-hub 导出的产物）"));
            continue;
        }
        let title = meta
            .get("title")
            .cloned()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("无标题笔记")
                    .to_string()
            });
        // 正文图片：assets/<name> 复制进 notes/images（沿用内容哈希名）；xhub-note URL 保持
        let asset_names = extract_asset_names(&body);
        let mut body_final = body;
        for name in &asset_names {
            let src = path.parent().map(|p| p.join("assets").join(&name));
            // 相对路径以该 .md 所在目录为基准；导出时 assets 固定在导出根，
            // 所以再试一次以导入根为基准
            let candidates = match src {
                Some(s) => vec![s, root.join("assets").join(&name)],
                None => vec![root.join("assets").join(&name)],
            };
            for c in candidates {
                if c.is_file() {
                    let dst = images_root.join(&name);
                    if !dst.exists() {
                        if let Err(e) = std::fs::copy(&c, &dst) {
                            report.failed.push(format!("{rel} 图片 {name}: {e}"));
                        }
                    }
                    break;
                }
            }
        }
        body_final = rewrite_asset_urls(&body_final);
        // 逐条短暂持锁落库（DB 段不含文件 IO）：把锁粒度从「整个导入」缩到「单条」，
        // 逐条事务语义不变。长持锁会把主线程上执行的同步 DB 命令全部卡住（= 整窗冻结）。
        let outcome = {
            let Ok(conn) = conn.lock() else {
                report.failed.push(format!("{rel}: 数据库不可用，导入中止"));
                break;
            };
            import_one(&conn, &title, &body_final, &meta, opts.overwrite, &mut folder_cache)
        };
        match outcome {
            Ok(ImportOne::Imported) => report.imported += 1,
            Ok(ImportOne::Skipped) => {
                report.skipped += 1;
                continue;
            }
            Err(e) => report.failed.push(format!("{rel}: {e}")),
        }
        if report.imported % 50 == 0 {
            let _ = app.emit("notes-import-progress", report.imported);
        }
    }
    log::info!(
        "速记导入结束: 成功 {} / 跳过 {} / 失败 {}（取消: {}）",
        report.imported,
        report.skipped,
        report.failed.len(),
        report.cancelled
    );
    Ok(report)
}

/// 单条导入结果：Imported = 已写入；Skipped = source_url 命中且未开覆盖
enum ImportOne {
    Imported,
    Skipped,
}

/// 单条导入落库（调用方负责短暂持锁，锁内不做文件 IO）：source_url 去重
/// （默认跳过 / overwrite 就地覆盖）→ 还原文件夹 → 逐条事务写入 + 重建双链
/// + 标签 find-or-create。
fn import_one(
    conn: &Connection,
    title: &str,
    body_final: &str,
    meta: &HashMap<String, String>,
    overwrite: bool,
    folder_cache: &mut HashMap<String, Option<i64>>,
) -> Result<ImportOne, String> {
    let source_url = meta.get("source_url").cloned().unwrap_or_default();
    // source_url 去重（方案 §8.2）：默认跳过；overwrite = 就地覆盖原笔记
    let mut overwrite_id: Option<i64> = None;
    if !source_url.is_empty() {
        let dup: Option<i64> = conn
            .query_row(
                "SELECT id FROM notes WHERE source_url = ?1 LIMIT 1",
                [source_url.as_str()],
                |r| r.get(0),
            )
            .ok();
        if let Some(id) = dup {
            if overwrite {
                overwrite_id = Some(id);
            } else {
                return Ok(ImportOne::Skipped);
            }
        }
    }
    // 还原文件夹（按 folder 字段的 `/` 分层，find-or-create）
    let folder_rel = meta.get("folder").cloned().unwrap_or_default();
    let folder_id = resolve_folder(conn, &folder_rel, folder_cache)?;
    // 落库（逐条事务，失败不回滚整体）；overwrite 命中时覆盖原笔记而非新插
    (|conn: &Connection| -> Result<(), String> {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let id = if let Some(oid) = overwrite_id {
            tx.execute(
                "UPDATE notes SET title = ?1, content = ?2, folder_id = ?3, icon = ?4 WHERE id = ?5",
                rusqlite::params![title, body_final, folder_id, meta.get("icon"), oid],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM note_tags WHERE note_id = ?1",
                rusqlite::params![oid],
            )
            .map_err(|e| e.to_string())?;
            oid
        } else {
            let ts = crate::repo::now();
            tx.execute(
                "INSERT INTO notes (title, content, folder_id, source_url, icon, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                rusqlite::params![title, body_final, folder_id, source_url, meta.get("icon"), ts],
            )
            .map_err(|e| e.to_string())?;
            tx.last_insert_rowid()
        };
        if overwrite_id.is_none() {
            if let Some(created) = meta.get("created_at").filter(|c| !c.is_empty()) {
                tx.execute(
                    "UPDATE notes SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![created, id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        crate::repo::note_link::reindex(&tx, id, body_final).map_err(|e| e.to_string())?;
        // 标签（按名 find-or-create；「剪藏」等内置同名标签自然复用）
        if let Some(tags) = meta.get("tags").filter(|t| !t.is_empty()) {
            for name in tags.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                tx.execute(
                    "INSERT OR IGNORE INTO tags (name, created_at) VALUES (?1, ?2)",
                    rusqlite::params![name, crate::repo::now()],
                )
                .map_err(|e| e.to_string())?;
                let tid: i64 = tx
                    .query_row("SELECT id FROM tags WHERE name = ?1", rusqlite::params![name], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                tx.execute(
                    "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
                    rusqlite::params![id, tid],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })(conn)?;
    Ok(ImportOne::Imported)
}

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 20 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_md_files(&p, out, depth + 1);
        } else if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false) {
            out.push(p);
        }
    }
}

/// folder 字段（a/b/c 形式）逐级 find-or-create，返回最终文件夹 id（None = 树根）
fn resolve_folder(
    conn: &Connection,
    rel: &str,
    cache: &mut HashMap<String, Option<i64>>,
) -> Result<Option<i64>, String> {
    let rel = rel.trim().trim_matches('/').to_string();
    if rel.is_empty() {
        return Ok(None);
    }
    if let Some(id) = cache.get(&rel) {
        return Ok(*id);
    }
    let mut parent: Option<i64> = None;
    let mut built = String::new();
    for segment in rel.split('/') {
        let name = segment.trim();
        if name.is_empty() {
            continue;
        }
        if !built.is_empty() {
            built.push('/');
        }
        built.push_str(name);
        if let Some(id) = cache.get(&built) {
            parent = *id;
            continue;
        }
        // find：同名同级复用（内置「剪藏」也走这里）
        let found: Option<i64> = conn
            .query_row(
                "SELECT id FROM note_folders WHERE name = ?1 AND parent_id IS ?2 LIMIT 1",
                rusqlite::params![name, parent],
                |r| r.get(0),
            )
            .ok();
        let id = match found {
            Some(id) => id,
            None => note_folder::create(conn, name, parent)
                .map_err(|e| e.to_string())?
                .id,
        };
        cache.insert(built.clone(), Some(id));
        parent = Some(id);
    }
    cache.insert(rel, parent);
    Ok(parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_windows_filenames() {
        assert_eq!(sanitize_filename("a<b>:c/d\\e|f?g*h\"i"), "a b  c d e f g h i");
        assert_eq!(sanitize_filename("  结尾点... "), "结尾点");
        assert_eq!(sanitize_filename("..."), "无标题笔记");
        assert_eq!(sanitize_filename(""), "无标题笔记");
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con.md"), "_con.md");
        assert_eq!(sanitize_filename("COM1"), "_COM1");
        assert_eq!(sanitize_filename("正常 标题.md"), "正常 标题.md");
        let long = "长".repeat(300);
        assert_eq!(sanitize_filename(&long).chars().count(), 200);
    }

    #[test]
    fn unique_filename_dedupes() {
        let dir = std::env::temp_dir().join(format!("xhub-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut used = HashSet::new();
        let a = unique_filename("笔记", &dir, &mut used);
        let b = unique_filename("笔记", &dir, &mut used);
        assert_eq!(a, "笔记");
        assert_eq!(b, "笔记 (2)");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn front_matter_parse_roundtrip() {
        let md = "---\ntitle: 甲\ncreated_at: 2026-01-02 03:04:05.000\nsource_url: https://x\ntags: 工作, 生活\nfolder: 剪藏/深度\n---\n\n正文第一行\n";
        let (meta, body) = parse_front_matter(md);
        assert_eq!(meta.get("title").map(String::as_str), Some("甲"));
        assert_eq!(meta.get("tags").map(String::as_str), Some("工作, 生活"));
        assert_eq!(meta.get("folder").map(String::as_str), Some("剪藏/深度"));
        assert!(body.trim_start().starts_with("正文第一行"));
        // 无 front-matter → 空 meta、全文为 body
        let (meta, body) = parse_front_matter("# 纯 markdown\n");
        assert!(meta.is_empty());
        assert!(body.contains("# 纯 markdown"));
    }

    #[test]
    fn image_url_rewrite_roundtrip() {
        let url = format!("{NOTE_URL_PREFIX}0123456789abcdef.png");
        let rel = rewrite_image_urls_to_relative(&format!("![]({url})"));
        assert_eq!(rel, "![](assets/0123456789abcdef.png)");
        assert_eq!(extract_asset_names(&rel), vec!["0123456789abcdef.png".to_string()]);
        // 非 16 位哈希的 assets 引用不算图
        assert!(extract_asset_names("assets/notahash.png").is_empty());
    }

    #[test]
    fn resolve_folder_creates_and_reuses() {
        let conn = crate::db::init_in_memory().unwrap();
        let mut cache = HashMap::new();
        let a = resolve_folder(&conn, "剪藏/深度", &mut cache).unwrap();
        assert!(a.is_some());
        // 同路径复用（含内置「剪藏」同名文件夹）
        let b = resolve_folder(&conn, "剪藏/深度", &mut cache).unwrap();
        assert_eq!(a, b);
        // 树根
        assert_eq!(resolve_folder(&conn, "", &mut cache).unwrap(), None);
        assert_eq!(resolve_folder(&conn, "/", &mut cache).unwrap(), None);
    }
}
