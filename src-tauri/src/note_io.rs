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
///
/// 速记视图内的随手导出（2026-10-22，#t69）：
/// - 单条（树右键）三种格式：Markdown 纯正文（**不带** front-matter，分享/外带场景）；
///   HTML 自包含单文件（CSS 内联 + 图片 base64）；PDF（export_render 静默矢量导出）。
/// - 整夹（文件夹右键）：落点 <所选目录>/<文件夹名>/，**每条一个文件**（三种格式同结构）；
///   Markdown 带 front-matter（与全库导出同格式，可再导回）；HTML/PDF 纯内容。
/// - HTML 的 md→html 渲染在**前端**做（marked+DOMPurify 已有，Rust 侧零渲染依赖），
///   命令收 `bodies`（id → 渲染好的正文 HTML）；图片转 base64 在 Rust 侧统一处理。
/// - 批量导出可取消（EXPORT_CANCELLED），PDF 逐条 emit `note-export-progress`。

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub exported: usize,
    /// 复制进 assets/ 的图片数
    pub images: usize,
    pub failed: Vec<String>,
    pub dir: String,
    /// 批量导出被用户取消（仅 export_notes_of_folder 会置位）
    #[serde(default)]
    pub cancelled: bool,
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
    let tags_of = build_tags_of(&tags, &note_tags);
    let mut report = ExportReport {
        exported: 0,
        images: 0,
        failed: Vec::new(),
        dir: root.display().to_string(),
        cancelled: false,
    };
    let mut ctx = MdWriteCtx {
        folders: &folders,
        tags_of: &tags_of,
        dir_cache: HashMap::new(),
        used_files: HashSet::new(),
        copied_images: HashSet::new(),
        assets_dir: root.join("assets"),
        root,
    };

    for n in &notes {
        write_note_markdown(n, &mut ctx, true, &mut report, None);
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

// ---------- 随手导出（单条 / 整夹，Markdown + HTML + PDF，2026-10-22 #t69） ----------

static EXPORT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// 取消正在进行的文件夹批量导出（幂等）
#[tauri::command]
pub fn export_notes_cancel() -> Result<(), String> {
    EXPORT_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

fn build_tags_of(tags: &[Tag], note_tags: &[(i64, i64)]) -> HashMap<i64, Vec<String>> {
    let mut tag_names: HashMap<i64, String> = HashMap::new();
    for t in tags {
        tag_names.insert(t.id, t.name.clone());
    }
    let mut tags_of: HashMap<i64, Vec<String>> = HashMap::new();
    for (nid, tid) in note_tags {
        if let Some(name) = tag_names.get(tid) {
            tags_of.entry(*nid).or_default().push(name.clone());
        }
    }
    tags_of
}

/// Markdown 逐条写出时的共享状态：全库导出与随手导出共用一套文件名/图片/目录口径
struct MdWriteCtx<'a> {
    folders: &'a [NoteFolder],
    tags_of: &'a HashMap<i64, Vec<String>>,
    /// 文件夹 id → 相对路径（sanitized，已建目录）
    dir_cache: HashMap<i64, PathBuf>,
    used_files: HashSet<String>,
    copied_images: HashSet<String>,
    /// 图片落点目录（导出根/assets；随导出根不同而不同）
    assets_dir: PathBuf,
    root: &'a Path,
}

/// 单条 Markdown 写出：建目录 → 唯一文件名 → 图片改相对路径并复制 → 拼正文（可选
/// front-matter）→ 落盘。失败记入 report 并跳过，不中断整体。
/// `filename_override`：单条导出经系统「另存为」由用户指定文件名（可带 .md 后缀）；
/// None = 按清洗标题自动去重命名（全库/整夹导出）。
fn write_note_markdown(
    n: &Note,
    ctx: &mut MdWriteCtx,
    with_front_matter: bool,
    report: &mut ExportReport,
    filename_override: Option<String>,
) {
    let folder_dir = match n.folder_id {
        Some(fid) => match ensure_folder_dir(ctx.folders, fid, ctx.root, &mut ctx.dir_cache) {
            Ok(p) => p,
            Err(e) => {
                report.failed.push(format!("{}: {e}", n.title));
                return;
            }
        },
        None => ctx.root.to_path_buf(),
    };
    let images_root = crate::paths::data_root().join("notes").join("images");
    let mut body = rewrite_image_urls_to_relative(&n.content);
    // 收集引用的图片并复制（同内容哈希名跨笔记共享，覆盖无害）
    for name in extract_asset_names(&body) {
        if ctx.copied_images.insert(name.clone()) {
            let src = images_root.join(&name);
            if src.is_file() {
                std::fs::create_dir_all(&ctx.assets_dir).ok();
                if let Err(e) = std::fs::copy(&src, ctx.assets_dir.join(&name)) {
                    report.failed.push(format!("{} 图片 {name}: {e}", n.title));
                } else {
                    report.images += 1;
                }
            }
        }
    }
    let mut md = String::new();
    if with_front_matter {
        md.push_str("---\n");
        md.push_str(&format!("title: {}\n", n.title));
        md.push_str(&format!("created_at: {}\n", n.created_at));
        md.push_str(&format!("source_url: {}\n", n.source_url));
        md.push_str(&format!("folder: {}\n", folder_rel_path(ctx.folders, n.folder_id)));
        if let Some(icon) = n.icon.as_deref().filter(|s| !s.is_empty()) {
            md.push_str(&format!("icon: {}\n", icon));
        }
        md.push_str(&format!(
            "tags: {}\n",
            ctx.tags_of.get(&n.id).cloned().unwrap_or_default().join(", ")
        ));
        md.push_str("---\n\n");
    }
    body = body.trim_start().to_string();
    md.push_str(&body);
    if !body.is_empty() && !body.ends_with('\n') {
        md.push('\n');
    }
    let path = match filename_override {
        Some(name) => folder_dir.join(name),
        None => {
            let stem = unique_filename(&n.title, &folder_dir, &mut ctx.used_files);
            folder_dir.join(format!("{stem}.md"))
        }
    };
    if let Err(e) = std::fs::write(&path, md) {
        report.failed.push(format!("{}: {e}", n.title));
        return;
    }
    report.exported += 1;
}

/// 文件夹 id 的子树（含自身）；parent 链成环时靠 visited 去重，深度超限剪枝
fn subtree_folder_ids(folders: &[NoteFolder], root_id: i64) -> Vec<i64> {
    let mut out = vec![root_id];
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        for f in folders {
            if f.parent_id == Some(cur) && !out.contains(&f.id) {
                out.push(f.id);
            }
        }
        i += 1;
        if out.len() > 10_000 {
            break;
        }
    }
    out
}

/// 单条笔记随手导出。format = markdown | html | pdf。
/// markdown：纯正文（无 front-matter），图片落 `assets/`（与所选文件同目录）；
/// html/pdf：`body_html` 是前端 renderNoteMarkdown 的渲染结果，图片转 base64 内嵌。
#[tauri::command]
pub async fn export_note_file(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    path: String,
    format: String,
    body_html: Option<String>,
) -> Result<ExportReport, String> {
    // 锁内只读 DB（单条 + 标签名），文件 IO / PDF 全在锁外
    let (n, tags_of) = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let n = note::get(&conn, id).map_err(|e| e.to_string())?;
        let tags = tag::list(&conn).map_err(|e| e.to_string())?;
        let note_tags = tag::list_note_tags(&conn).map_err(|e| e.to_string())?;
        (n, build_tags_of(&tags, &note_tags))
    };
    let target = PathBuf::from(&path);
    let parent = target
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut report = ExportReport {
        exported: 0,
        images: 0,
        failed: Vec::new(),
        dir: target.display().to_string(),
        cancelled: false,
    };
    tokio::task::block_in_place(|| -> Result<(), String> {
        match format.as_str() {
            "markdown" => {
                // 用户在另存为里给的文件名照用（缺 .md 补上）；图片目录固定 assets/。
                // 单条落点由用户指定：清掉 folder_id，避免误走「按归属建目录」的整夹口径
                let mut n = n;
                n.folder_id = None;
                let name = target
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| {
                        if s.to_ascii_lowercase().ends_with(".md") {
                            s.to_string()
                        } else {
                            format!("{s}.md")
                        }
                    })
                    .ok_or_else(|| "目标路径不合法".to_string())?;
                let mut ctx = MdWriteCtx {
                    folders: &[],
                    tags_of: &tags_of,
                    dir_cache: HashMap::new(),
                    used_files: HashSet::new(),
                    copied_images: HashSet::new(),
                    assets_dir: parent.join("assets"),
                    root: &parent,
                };
                write_note_markdown(&n, &mut ctx, false, &mut report, Some(name));
            }
            "html" => {
                let body = body_html.ok_or_else(|| "缺少渲染结果".to_string())?;
                let doc = inline_note_images(&export_document_html(&n.title, &body));
                let path = ensure_ext(target, "html");
                std::fs::write(&path, doc).map_err(|e| format!("写入失败: {e}"))?;
                report.exported += 1;
            }
            "pdf" => {
                let body = body_html.ok_or_else(|| "缺少渲染结果".to_string())?;
                let doc = inline_note_images(&export_document_html(&n.title, &body));
                let path = ensure_ext(target, "pdf");
                crate::export_render::render_pdf_with_fallback(&app, &doc, &path)?;
                report.exported += 1;
            }
            other => return Err(format!("不支持的导出格式: {other}")),
        }
        Ok(())
    })?;
    Ok(report)
}

#[derive(Deserialize)]
pub struct NoteExportBody {
    pub id: i64,
    /// 前端 renderNoteMarkdown 渲染好的正文 HTML
    pub html: String,
}

/// 整夹导出（文件夹右键）：落点 `<dir>/<文件夹名>/`，每条笔记一个文件（三种格式同结构）。
/// markdown 带 front-matter（与全库导出同格式，可回导）；html/pdf 纯内容单文件。
/// 批量可取消，PDF 逐条 emit `note-export-progress`。
#[tauri::command]
pub async fn export_notes_of_folder(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    folder_id: i64,
    dir: String,
    format: String,
    bodies: Vec<NoteExportBody>,
) -> Result<ExportReport, String> {
    EXPORT_CANCELLED.store(false, Ordering::SeqCst);
    let snapshot = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        read_export_snapshot(&conn)?
    };
    let (notes, folders, tags, note_tags) = snapshot;
    let ids = subtree_folder_ids(&folders, folder_id);
    let in_subtree: Vec<Note> = notes.into_iter().filter(|n| n.folder_id.is_some_and(|f| ids.contains(&f))).collect();
    let fname = folders
        .iter()
        .find(|f| f.id == folder_id)
        .map(|f| sanitize_filename(&f.name))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "导出笔记".to_string());
    let root = PathBuf::from(&dir).join(fname);
    let total = in_subtree.len();
    let mut report = ExportReport {
        exported: 0,
        images: 0,
        failed: Vec::new(),
        dir: root.display().to_string(),
        cancelled: false,
    };
    if total == 0 {
        return Ok(report);
    }
    let tags_of = build_tags_of(&tags, &note_tags);
    let body_map: HashMap<i64, String> = bodies.into_iter().map(|b| (b.id, b.html)).collect();
    tokio::task::block_in_place(|| -> Result<(), String> {
        std::fs::create_dir_all(&root).map_err(|e| format!("创建导出目录失败: {e}"))?;
        match format.as_str() {
            "markdown" => {
                let mut ctx = MdWriteCtx {
                    folders: &folders,
                    tags_of: &tags_of,
                    dir_cache: HashMap::new(),
                    used_files: HashSet::new(),
                    copied_images: HashSet::new(),
                    assets_dir: root.join("assets"),
                    root: &root,
                };
                for n in &in_subtree {
                    if EXPORT_CANCELLED.load(Ordering::SeqCst) {
                        report.cancelled = true;
                        break;
                    }
                    write_note_markdown(n, &mut ctx, true, &mut report, None);
                }
            }
            "html" => {
                let mut used: HashSet<String> = HashSet::new();
                for n in &in_subtree {
                    if EXPORT_CANCELLED.load(Ordering::SeqCst) {
                        report.cancelled = true;
                        break;
                    }
                    let Some(body) = body_map.get(&n.id) else {
                        report.failed.push(format!("{}: 缺少渲染结果", n.title));
                        continue;
                    };
                    let doc = inline_note_images(&export_document_html(&n.title, body));
                    let stem = unique_filename_ext(&n.title, &root, "html", &mut used);
                    if let Err(e) = std::fs::write(root.join(format!("{stem}.html")), doc) {
                        report.failed.push(format!("{}: {e}", n.title));
                        continue;
                    }
                    report.exported += 1;
                }
            }
            "pdf" => {
                let mut used: HashSet<String> = HashSet::new();
                for (i, n) in in_subtree.iter().enumerate() {
                    if EXPORT_CANCELLED.load(Ordering::SeqCst) {
                        report.cancelled = true;
                        break;
                    }
                    crate::export_render::emit_progress(&app, i, total, &n.title);
                    let Some(body) = body_map.get(&n.id) else {
                        report.failed.push(format!("{}: 缺少渲染结果", n.title));
                        continue;
                    };
                    let doc = inline_note_images(&export_document_html(&n.title, body));
                    let stem = unique_filename_ext(&n.title, &root, "pdf", &mut used);
                    let out = root.join(format!("{stem}.pdf"));
                    if let Err(e) = crate::export_render::render_pdf_with_fallback(&app, &doc, &out)
                    {
                        report.failed.push(format!("{}: {e}", n.title));
                        continue;
                    }
                    report.exported += 1;
                }
                crate::export_render::emit_progress(&app, total, total, "");
            }
            other => return Err(format!("不支持的导出格式: {other}")),
        }
        Ok(())
    })?;
    Ok(report)
}

/// 确保 path 有指定扩展名（用户「另存为」可能手滑不带后缀）
fn ensure_ext(path: PathBuf, ext: &str) -> PathBuf {
    let has = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false);
    if has {
        path
    } else {
        let mut os = path.into_os_string();
        os.push(format!(".{ext}"));
        PathBuf::from(os)
    }
}

// ---------- HTML / PDF 文档拼装（Rust 侧零 markdown 依赖：正文由前端渲染） ----------

/// 导出文档的打印/阅读样式：白底、系统字体栈（含中文）、代码块/引用/表格/图片齐全。
/// HTML 导出与 PDF 渲染共用（PDF 由 WebView2 按 @page 与 PrintSettings 分页）。
pub const EXPORT_CSS: &str = r#"
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; background: #fff; }
.xh-note {
  max-width: 860px; margin: 0 auto; padding: 40px 48px;
  font-family: "Segoe UI", "Microsoft YaHei", "PingFang SC", system-ui, sans-serif;
  font-size: 15px; line-height: 1.75; color: #1f2328; word-break: break-word;
}
.xh-note h1, .xh-note h2, .xh-note h3, .xh-note h4, .xh-note h5, .xh-note h6 {
  line-height: 1.35; margin: 1.4em 0 0.6em; font-weight: 650;
}
.xh-note h1 { font-size: 1.7em; } .xh-note h2 { font-size: 1.4em; }
.xh-note h3 { font-size: 1.2em; } .xh-note h4 { font-size: 1.05em; }
.xh-note p { margin: 0.6em 0; }
.xh-note a { color: #0969da; }
.xh-note img { max-width: 100%; height: auto; border-radius: 4px; }
.xh-note code {
  font-family: Consolas, "Cascadia Mono", monospace; font-size: 0.9em;
  background: rgba(120, 120, 120, 0.14); padding: 0.15em 0.4em; border-radius: 4px;
}
.xh-note pre {
  background: #f6f8fa; border: 1px solid #e4e7eb; border-radius: 8px;
  padding: 12px 14px; overflow-x: auto;
}
.xh-note pre code { background: none; padding: 0; }
.xh-note blockquote {
  margin: 0.8em 0; padding: 2px 16px; color: #57606a;
  border-left: 3px solid #d0d7de;
}
.xh-note table { border-collapse: collapse; margin: 0.8em 0; }
.xh-note th, .xh-note td { border: 1px solid #d0d7de; padding: 6px 12px; }
.xh-note th { background: #f6f8fa; }
.xh-note hr { border: none; border-top: 1px solid #d0d7de; margin: 1.6em 0; }
.xh-note ul, .xh-note ol { padding-left: 1.6em; }
.xh-note li { margin: 0.25em 0; }
@media print { .xh-note { max-width: none; padding: 0; } }
"#;

/// 拼装独立 HTML 文档（标题经 HTML 转义；正文来自前端 DOMPurify 管线）
pub fn export_document_html(title: &str, body_html: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"UTF-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n\
         <title>{}</title>\n<style>{EXPORT_CSS}</style>\n</head>\n<body>\n\
         <article class=\"xh-note\">{body_html}</article>\n</body>\n</html>\n",
        html_escape(title)
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 正文里的 xhub-note 图片 URL 就地转 data: base64（自包含单文件 / PDF 渲染共用）。
/// 文件缺失或非图片扩展名时保留原 URL（不失败整个导出）。
pub fn inline_note_images(html: &str) -> String {
    inline_note_images_with(&crate::paths::data_root().join("notes").join("images"), html)
}

/// 可注入图片根目录的版本（单测种真实文件用）；行为与 inline_note_images 一致。
fn inline_note_images_with(images_root: &Path, html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 4096);
    let mut rest = html;
    while let Some(pos) = rest.find(NOTE_URL_PREFIX) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + NOTE_URL_PREFIX.len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let stem = name.split('.').next().unwrap_or("");
        let is_image =
            stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && name.contains('.');
        let replaced = is_image.then(|| {
            let file = images_root.join(&name);
            std::fs::read(&file).ok().and_then(|bytes| {
                let mime = mime_of_ext(&name)?;
                use base64::Engine as _;
                Some(format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ))
            })
        });
        match replaced.flatten() {
            Some(data_url) => {
                out.push_str(&data_url);
                // tail 以 name 开头，必须一并消费——否则文件名粘在 base64 尾部，
                // data URI 非法（base64 字母表外字符），浏览器直接判图裂（0.9.2 实测）
                rest = &tail[name.len()..];
            }
            None => {
                // 保留原 URL：整段「前缀 + 名字」原样落出并推进（name 空也要越过前缀，防死循环）
                out.push_str(NOTE_URL_PREFIX);
                out.push_str(&name);
                rest = &rest[pos + NOTE_URL_PREFIX.len() + name.len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn mime_of_ext(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => return None,
    })
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
    unique_filename_ext(title, dir, "md", used)
}

/// 带扩展名变体（HTML/PDF 导出共用去重口径；ext 不带点）
fn unique_filename_ext(
    title: &str,
    dir: &Path,
    ext: &str,
    used: &mut HashSet<String>,
) -> String {
    let base = sanitize_filename(title);
    let mut candidate = base.clone();
    let mut n = 2;
    loop {
        let key = format!("{}\u{0}{}", dir.display(), candidate);
        let taken = used.contains(&key) || dir.join(format!("{candidate}.{ext}")).exists();
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
    fn unique_filename_ext_dedupes_and_scopes_by_ext() {
        let dir = std::env::temp_dir().join(format!("xhub-io-test-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("笔记.html"), "x").unwrap();
        let mut used = HashSet::new();
        // 磁盘上已有 笔记.html → 让位 (2)；md 侧不受 .html 占位影响
        assert_eq!(unique_filename_ext("笔记", &dir, "html", &mut used), "笔记 (2)");
        assert_eq!(unique_filename_ext("笔记", &dir, "md", &mut used), "笔记");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn subtree_walks_children_and_prunes_cycles() {
        let f = |id: i64, parent: Option<i64>| NoteFolder {
            id,
            name: format!("f{id}"),
            parent_id: parent,
            sort_order: 0,
            builtin: false,
            created_at: String::new(),
        };
        let folders = vec![f(1, None), f(2, Some(1)), f(3, Some(2)), f(9, None)];
        let mut ids = subtree_folder_ids(&folders, 1);
        ids.sort();
        assert_eq!(ids, vec![1, 2, 3]);
        // 环路：1↔2 互为父子，不发散
        let cyclic = vec![f(1, Some(2)), f(2, Some(1))];
        assert_eq!(subtree_folder_ids(&cyclic, 1).len(), 2);
    }

    #[test]
    fn document_html_escapes_title_and_keeps_body() {
        let doc = export_document_html("<标题> & \"引号\"", "<p>正文</p>");
        assert!(doc.contains("<title>&lt;标题&gt; &amp; &quot;引号&quot;</title>"));
        assert!(doc.contains("<article class=\"xh-note\"><p>正文</p></article>"));
        assert!(doc.contains(EXPORT_CSS));
    }

    #[test]
    fn inline_images_keep_url_when_file_missing_or_not_image() {
        // 16 位哈希名但文件不存在 → 原样保留（不失败整个导出）
        let url = "http://xhub-note.localhost/0123456789abcdef.png";
        assert_eq!(inline_note_images(&format!("<img src=\"{url}\">")), format!("<img src=\"{url}\">"));
        // 非 16 位哈希名（用户自写的同名前缀）→ 不动
        let plain = "http://xhub-note.localhost/pic.png";
        assert_eq!(inline_note_images(&format!("<img src=\"{plain}\">")), format!("<img src=\"{plain}\">"));
        // 混排：前缀外文本原样保留
        let mixed = format!("前文{url}后文");
        assert_eq!(inline_note_images(&mixed), mixed);
    }

    #[test]
    fn ensure_ext_appends_only_when_missing() {
        let p = ensure_ext(PathBuf::from("C:/out/笔记"), "pdf");
        assert_eq!(p, PathBuf::from("C:/out/笔记.pdf"));
        let p = ensure_ext(PathBuf::from("C:/out/笔记.PDF"), "pdf");
        assert_eq!(p, PathBuf::from("C:/out/笔记.PDF"));
        let p = ensure_ext(PathBuf::from("C:/out/笔记.md"), "pdf");
        assert_eq!(p, PathBuf::from("C:/out/笔记.md.pdf"));
    }

    #[test]
    fn markdown_write_front_matter_toggle_and_plain_body() {
        let dir = std::env::temp_dir().join(format!("xhub-io-test-md-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tags_of = HashMap::new();
        let mut report = ExportReport {
            exported: 0,
            images: 0,
            failed: Vec::new(),
            dir: String::new(),
            cancelled: false,
        };
        let n = Note {
            id: 1,
            title: "测试".into(),
            content: "# 标题行\n\n正文\n".into(),
            created_at: "2026-01-02 03:04:05.000".into(),
            updated_at: "2026-01-02 03:04:05.000".into(),
            folder_id: None,
            source_url: String::new(),
            deleted_at: None,
            icon: None,
            sort_order: None,
        };
        let mut ctx = MdWriteCtx {
            folders: &[],
            tags_of: &tags_of,
            dir_cache: HashMap::new(),
            used_files: HashSet::new(),
            copied_images: HashSet::new(),
            assets_dir: dir.join("assets"),
            root: &dir,
        };
        write_note_markdown(&n, &mut ctx, false, &mut report, Some("测试.md".into()));
        let plain = std::fs::read_to_string(dir.join("测试.md")).unwrap();
        assert!(!plain.contains("---"));
        assert!(plain.starts_with("# 标题行"));
        assert_eq!(report.exported, 1);
        assert!(report.failed.is_empty());

        // 带 front-matter：title/created_at/source_url 全在（tags 空也写字段，回导兼容）
        write_note_markdown(&n, &mut ctx, true, &mut report, Some("测试2.md".into()));
        let with_fm = std::fs::read_to_string(dir.join("测试2.md")).unwrap();
        assert!(with_fm.starts_with("---\ntitle: 测试\n"));
        assert!(with_fm.contains("created_at: 2026-01-02 03:04:05.000"));
        assert!(with_fm.contains("tags: \n"));
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
    fn inline_images_success_consumes_whole_url() {
        // 种真实图片文件（图片根目录可注入），断言「整段 URL 被替换」——
        // 0.9.2 实测坑：只断言前缀时，文件名粘在 base64 尾部（data URI 非法 → 图裂）漏网
        let dir = std::env::temp_dir().join(format!("xhub-inline-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("0123456789abcdef.png");
        if png.exists() {
            std::fs::remove_file(&png).unwrap();
        }
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]).unwrap();
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD
            .encode([0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]);
        let html = format!("<p>前文</p><img src=\"{NOTE_URL_PREFIX}0123456789abcdef.png\" alt=\"图\"><p>后文</p>");
        let out = inline_note_images_with(&dir, &html);
        assert_eq!(
            out,
            format!("<p>前文</p><img src=\"data:image/png;base64,{b64}\" alt=\"图\"><p>后文</p>")
        );
        std::fs::remove_dir_all(&dir).ok();
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
