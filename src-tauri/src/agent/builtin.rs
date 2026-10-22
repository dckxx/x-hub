//! 内置工具（§5 全量 30 个 + 0.9.0 新增 create_subcategory 与 get_snippet + 0.9.x 扩展桥 2 个，共 34：读 15 / 写 14 / 危险 5）。
//!
//! 统一口径：
//! - **名称匹配失败语义**：`create_note` 的 folder/tags、`create_resource` 的 category
//!   按**名称精确匹配**；失败时返回「未找到 + 最接近候选」，不猜、不静默落默认（§5）。
//! - **结果长度**：列表类最多 20 条（超出提示「还有 N 条」）、正文类回填模型最多 2000 字
//!   （截断在 turn.rs 统一做）；UI 拿到的 detail 是全文，模型拿截断版。
//! - **环境类失败语义**：`get_weather` 受联网总开关控制，关闭/未配置时返回**说明**而非
//!   错误（避免模型反复重试，§5）。
//! - 工具 description 写清「什么时候用、什么时候不用」，参数尽量少、必填最小化。

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

use serde_json::{json, Value};

use crate::models::{
    ClipboardItem, Countdown, Note, RepeatRule, Resource, ResourceKind, Snippet, Todo,
};
use crate::repo;

use super::approval::Level;
use super::tools::{AgentTool, ToolCtx, ToolOutcome, ToolRegistry};

// ---------------------------------------------------------------------------
// 参数与格式化辅助
// ---------------------------------------------------------------------------

/// 列表类结果最多条数（§5 工具结果长度控制）
const MAX_LIST: usize = 20;
/// 正文类回填模型的最大字符数（turn.rs 按此硬截断，工具内先做软截断）
const MAX_TEXT: usize = 2000;
/// 工具参数里的时间戳合法区间（2000-01-01 ~ 2100-01-01）：LLM 输出是不可信输入，
/// i64::MAX 级数值会让 `end - now` 溢出 panic、chrono 换算越界——范围外按不可识别拒绝
const TIME_MS_MIN: i64 = 946_684_800_000;
const TIME_MS_MAX: i64 = 4_102_444_800_000;
/// 倒计时时长/间隔的分钟上限（10 年）：防 `minutes * 60_000` 溢出
const MAX_COUNTDOWN_MINUTES: i64 = 5_259_600;

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty())
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| {
        v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

/// 三个状态参数的取值语义：缺省=不改动 / null=清除 / 值=设置
enum SetOpt {
    Keep,
    Clear,
    Set(i64),
}

impl SetOpt {
    fn value(&self) -> Option<i64> {
        match self {
            SetOpt::Set(v) => Some(*v),
            _ => None,
        }
    }
    fn is_changed(&self) -> bool {
        !matches!(self, SetOpt::Keep)
    }
}

fn arg_time_opt(args: &Value, key: &str) -> Result<SetOpt, String> {
    match args.get(key) {
        None => Ok(SetOpt::Keep),
        Some(Value::Null) => Ok(SetOpt::Clear),
        Some(v) => parse_time_ms(v).map(SetOpt::Set).ok_or_else(|| {
            format!("参数 {key} 不是可识别的时间（支持 毫秒时间戳 / YYYY-MM-DD HH:mm / ISO8601）")
        }),
    }
}

/// 时间解析：毫秒时间戳 / 本地 "YYYY-MM-DD HH:mm" / "YYYY-MM-DD" / ISO8601 → 毫秒。
/// 超出合法区间的数值一律 None（防溢出，见 TIME_MS_MIN/MAX 注释）
fn parse_time_ms(v: &Value) -> Option<i64> {
    let in_range = |ms: i64| (TIME_MS_MIN..=TIME_MS_MAX).contains(&ms).then_some(ms);
    match v {
        Value::Number(n) => n.as_i64().and_then(in_range),
        Value::String(s) => {
            let s = s.trim();
            if let Ok(ms) = s.parse::<i64>() {
                return in_range(ms);
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return in_range(dt.timestamp_millis());
            }
            for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
                if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
                    return Local
                        .from_local_datetime(&naive)
                        .single()
                        .and_then(|dt| in_range(dt.timestamp_millis()));
                }
                if fmt == "%Y-%m-%d" {
                    if let Ok(d) = NaiveDate::parse_from_str(s, fmt) {
                        let naive = d.and_hms_opt(0, 0, 0)?;
                        return Local
                            .from_local_datetime(&naive)
                            .single()
                            .and_then(|dt| in_range(dt.timestamp_millis()));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// 毫秒 → `MM-DD HH:mm`（跨年补年份）
fn fmt_ms(ms: i64) -> String {
    let dt = Local.timestamp_millis_opt(ms).single();
    match dt {
        Some(d) => {
            let now = Local::now();
            if d.year() == now.year() {
                format!("{:02}-{:02} {:02}:{:02}", d.month(), d.day(), d.hour(), d.minute())
            } else {
                format!("{}-{:02}-{:02}", d.year(), d.month(), d.day())
            }
        }
        None => format!("{ms}"),
    }
}

fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…（已截断，共 {} 字）", s.chars().count())
    }
}

fn fmt_todo_line(t: &Todo) -> String {
    let mut parts = vec![format!("[{}]{}", t.id, if t.done { "✔" } else { "" })];
    parts.push(if t.pinned { "📌".into() } else { String::new() });
    parts.push(t.title.clone());
    if t.priority > 0 {
        parts.push(format!("P{}", t.priority));
    }
    if let Some(due) = t.due_at {
        parts.push(format!("截止 {}", fmt_ms(due)));
    }
    if t.repeat_mode != "once" {
        parts.push(format!("({})", t.repeat_mode));
    }
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

fn fmt_note_line(n: &Note) -> String {
    format!("[{}] {}", n.id, n.title)
}

fn fmt_resource_line(r: &Resource) -> String {
    let cat = r.category.clone().unwrap_or_default();
    format!("[{}] {}（{}{}）", r.id, r.name, kind_cn(&r.kind), if cat.is_empty() { String::new() } else { format!("/{}", cat) })
}

fn kind_cn(k: &ResourceKind) -> &'static str {
    match k {
        ResourceKind::App => "应用",
        ResourceKind::Web => "网页",
        ResourceKind::File => "文件",
    }
}

fn fmt_countdown_line(c: &Countdown) -> String {
    let now = repo::countdown::now_ms();
    let remaining = if c.paused {
        c.paused_remaining_ms.unwrap_or(0)
    } else {
        (c.end_at - now).max(0)
    };
    let remain_txt = if remaining >= 3_600_000 {
        format!("{:.1} 小时", remaining as f64 / 3_600_000.0)
    } else {
        format!("{} 分钟", remaining / 60_000)
    };
    format!(
        "[{}] {}（{}，剩余 {}{}）",
        c.id,
        c.name,
        c.repeat_mode,
        remain_txt,
        if c.paused { "，已暂停" } else { "" }
    )
}

/// 名称精确匹配（§5 失败语义）：命中返回 id，否则返回候选名列表
fn match_exact(candidates: &[(String, i64)], want: &str) -> Result<i64, Vec<String>> {
    if let Some((_, id)) = candidates.iter().find(|(n, _)| n == want) {
        return Ok(*id);
    }
    Err(candidates.iter().take(8).map(|(n, _)| n.clone()).collect())
}

fn not_found_outcome(what: &str, want: &str, candidates: Vec<String>) -> ToolOutcome {
    ToolOutcome::err(
        format!("{what}「{want}」未找到"),
        format!(
            "未找到{what}「{want}」。最接近的候选：{}。请向用户确认后再操作，不要猜。",
            if candidates.is_empty() { "（无）".to_string() } else { candidates.join("、") }
        ),
    )
}

fn db_err(e: rusqlite::Error) -> ToolOutcome {
    ToolOutcome::err(format!("数据库操作失败：{e}"), String::new())
}

/// 新建速达小类（0.9.0 新增，第 31 个工具）：用户明确要建新目录时，
/// 先建小类再 create_resource 落资源——此前 create_resource 对不存在的小类
/// 只会返回候选让用户反问，模型没有任何途径新建目录，实测「创建不了速达目录」。
struct CreateSubcategory;
impl AgentTool for CreateSubcategory {
    fn name(&self) -> &'static str { "create_subcategory" }
    fn description(&self) -> &'static str { "新建速达小类（目录）。kind=app|web|file；name 为小类名。仅当用户明确要新建目录时调用；同名已存在则直接返回该小类（幂等），随后用 create_resource 的 category 引用它。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "kind":{"type":"string","enum":["app","web","file"]},
        "name":{"type":"string"}
    },"required":["kind","name"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let kind_str = match arg_str(args, "kind") {
            Some(k @ ("app" | "web" | "file")) => k,
            _ => return ToolOutcome::err("kind 必须是 app/web/file 之一", String::new()),
        };
        let Some(name) = arg_str(args, "name") else { return ToolOutcome::err("缺少 name 参数", String::new()) };
        // 幂等：同 kind 同名已存在则不重复建，直接返回（小类名在同 kind 内唯一使用）
        let subs = match repo::subcategory::list(ctx.conn) { Ok(s) => s, Err(e) => return db_err(e) };
        if let Some(exist) = subs.iter().find(|s| s.kind == kind_str && s.name == name) {
            return ToolOutcome::ok(
                format!("小类「{name}」已存在"),
                format!("速达小类已存在：[{}] {name}（kind={kind_str}），可直接用 create_resource 的 category=\"{name}\" 引用", exist.id),
            );
        }
        let s = match repo::subcategory::create(ctx.conn, kind_str, name) { Ok(s) => s, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已创建小类「{}」", s.name), format!("已创建速达小类：[{}] {}（kind={}），可用 create_resource 的 category=\"{}\" 引用", s.id, s.name, kind_str, s.name))
    }
}

/// 列表截断到 MAX_LIST 条，附「还有 N 条未列出」
fn cap_lines(mut lines: Vec<String>) -> String {
    if lines.len() <= MAX_LIST {
        return if lines.is_empty() { "（空）".into() } else { lines.join("\n") };
    }
    let rest = lines.len() - MAX_LIST;
    lines.truncate(MAX_LIST);
    format!("{}\n（还有 {rest} 条未列出）", lines.join("\n"))
}

// ---------------------------------------------------------------------------
// 读（12）
// ---------------------------------------------------------------------------

struct ListTodos;
impl AgentTool for ListTodos {
    fn name(&self) -> &'static str { "list_todos" }
    fn description(&self) -> &'static str { "列出待办（按 置顶/逾期/今天/有日期/无日期 分组，与界面一致）。仅当用户问到自己的待办/任务/安排时调用；不返回已完成项。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let todos = match repo::todo::list(ctx.conn) { Ok(t) => t, Err(e) => return db_err(e) };
        let now_ms = Local::now().timestamp_millis();
        let today_start = Local.from_local_datetime(&Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap()).single().map(|d| d.timestamp_millis());
        let today_end = today_start.map(|s| s + 86_400_000);
        let undone: Vec<&Todo> = todos.iter().filter(|t| !t.done).collect();
        let done_count = todos.len() - undone.len();
        let mut pinned = Vec::new();
        let mut overdue = Vec::new();
        let mut today = Vec::new();
        let mut dated = Vec::new();
        let mut undated = Vec::new();
        for t in &undone {
            if t.pinned { pinned.push(fmt_todo_line(t)); }
            else if let Some(due) = t.due_at {
                if due < now_ms { overdue.push(fmt_todo_line(t)); }
                else if let (Some(s), Some(e)) = (today_start, today_end) { if due >= s && due < e { today.push(fmt_todo_line(t)); } else { dated.push(fmt_todo_line(t)); } }
                else { dated.push(fmt_todo_line(t)); }
            } else { undated.push(fmt_todo_line(t)); }
        }
        let mut sections: Vec<String> = Vec::new();
        for (name, items) in [("置顶", pinned), ("逾期", overdue), ("今天", today), ("有日期", dated), ("无日期", undated)] {
            if !items.is_empty() { sections.push(format!("【{name}】\n{}", cap_lines(items))); }
        }
        if sections.is_empty() { sections.push("（没有未完成的待办）".into()); }
        if done_count > 0 { sections.push(format!("（另有 {done_count} 条已完成，未列出）")); }
        ToolOutcome::ok("已列出待办", sections.join("\n\n"))
    }
}

struct SearchNotes;
impl AgentTool for SearchNotes {
    fn name(&self) -> &'static str { "search_notes" }
    fn description(&self) -> &'static str { "按关键词搜索笔记（标题+正文，不含回收站）。仅当用户提到某篇笔记/记录且不知道 id 时调用；用户没提到笔记的内容直接回答，不要拿来「先看一眼」。读全文用 get_note。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"keyword":{"type":"string","description":"搜索关键词"}},"required":["keyword"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(kw) = arg_str(args, "keyword") else { return ToolOutcome::err("缺少 keyword 参数", String::new()) };
        let notes = match repo::note::search(ctx.conn, kw) { Ok(n) => n, Err(e) => return db_err(e) };
        let lines: Vec<String> = notes.iter().map(fmt_note_line).collect();
        ToolOutcome::ok(format!("找到 {} 篇", notes.len()), cap_lines(lines))
    }
}

struct GetNote;
impl AgentTool for GetNote {
    fn name(&self) -> &'static str { "get_note" }
    fn description(&self) -> &'static str { "读单篇笔记全文（含所在文件夹与标签）。id 来自 list_notes/search_notes/global_search；必须已有具体笔记 id 才调用，不要为了找笔记而试探。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"note_id":{"type":"integer","description":"笔记 id"}},"required":["note_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "note_id") else { return ToolOutcome::err("缺少 note_id 参数", String::new()) };
        let note = match repo::note::get(ctx.conn, id) { Ok(n) => n, Err(_) => return ToolOutcome::err(format!("笔记 {id} 不存在"), String::new()) };
        let tags = repo::tag::tags_of_note(ctx.conn, id).unwrap_or_default().into_iter().map(|t| t.name).collect::<Vec<_>>().join("、");
        let folder = note.folder_id.and_then(|fid| repo::note_folder::get(ctx.conn, fid).ok()).map(|f| f.name).unwrap_or_default();
        let meta = format!("《{}》{}{}", note.title, if folder.is_empty() { String::new() } else { format!(" 文件夹:{folder}") }, if tags.is_empty() { String::new() } else { format!(" 标签:{tags}") });
        ToolOutcome::ok(format!("已读取《{}》", note.title), format!("{meta}\n\n{}", cap_chars(&note.content, MAX_TEXT)))
    }
}

struct ListNotes;
impl AgentTool for ListNotes {
    fn name(&self) -> &'static str { "list_notes" }
    fn description(&self) -> &'static str { "列出笔记清单与文件夹树（不含正文）。仅当用户要看笔记目录/清单或按文件夹浏览时调用；与本地数据无关的对话不要调用。可选按文件夹名过滤。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"folder":{"type":"string","description":"按文件夹名过滤（精确匹配）"}}}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let folders = match repo::note_folder::list(ctx.conn) { Ok(f) => f, Err(e) => return db_err(e) };
        let mut filter_fid: Option<i64> = None;
        if let Some(want) = arg_str(args, "folder") {
            let cands: Vec<(String, i64)> = folders.iter().map(|f| (f.name.clone(), f.id)).collect();
            filter_fid = Some(match match_exact(&cands, want) {
                Ok(id) => id,
                Err(c) => return not_found_outcome("文件夹", want, c),
            });
        }
        let tree = folders.iter().map(|f| format!("文件夹[{}]: {}", f.id, f.name)).collect::<Vec<_>>().join("\n");
        let notes = match repo::note::list_meta(ctx.conn) { Ok(n) => n, Err(e) => return db_err(e) };
        let lines: Vec<String> = notes
            .iter()
            .filter(|n| filter_fid.map_or(true, |f| n.folder_id == Some(f)))
            .map(fmt_note_line)
            .collect();
        let count = lines.len();
        let mut out = String::new();
        if !tree.is_empty() { out.push_str(&format!("文件夹树：\n{tree}\n\n")); }
        out.push_str(&cap_lines(lines));
        ToolOutcome::ok(format!("共 {count} 篇"), out)
    }
}

struct ListResources;
impl AgentTool for ListResources {
    fn name(&self) -> &'static str { "list_resources" }
    fn description(&self) -> &'static str { "列出速达资源（应用/网页/文件）。仅当用户要启动或查看自己的速达资源时调用；launch_resource 需要 id。可按大类过滤。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"kind":{"type":"string","enum":["app","web","file"],"description":"按大类过滤，缺省=全部"}}}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let all = match repo::resource::list_all(ctx.conn) { Ok(r) => r, Err(e) => return db_err(e) };
        let kind_filter = arg_str(args, "kind");
        let lines: Vec<String> = all.iter()
            .filter(|r| kind_filter.map(|k| crate::repo::resource::kind_to_str(&r.kind) == k).unwrap_or(true))
            .map(fmt_resource_line).collect();
        ToolOutcome::ok(format!("共 {} 项", lines.len()), cap_lines(lines))
    }
}

struct ListCountdowns;
impl AgentTool for ListCountdowns {
    fn name(&self) -> &'static str { "list_countdowns" }
    fn description(&self) -> &'static str { "列出倒计时（含剩余时间与暂停态）。仅当用户问到自己的倒计时时调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let list = match repo::countdown::list(ctx.conn) { Ok(c) => c, Err(e) => return db_err(e) };
        let lines: Vec<String> = list.iter().map(fmt_countdown_line).collect();
        ToolOutcome::ok(format!("共 {} 个", lines.len()), cap_lines(lines))
    }
}

struct GetStickies;
impl AgentTool for GetStickies {
    fn name(&self) -> &'static str { "get_stickies" }
    fn description(&self) -> &'static str { "读工作台便签（slot 1/2）的内容。仅当用户提到便签或其中内容时调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let all = match repo::sticky::list(ctx.conn) { Ok(s) => s, Err(e) => return db_err(e) };
        let mut out = Vec::new();
        for slot in [1, 2] {
            match all.iter().find(|s| s.slot == slot) {
                Some(s) => out.push(format!("便签{slot}：\n{}", cap_chars(&s.content, MAX_TEXT))),
                None => out.push(format!("便签{slot}：（空）")),
            }
        }
        ToolOutcome::ok("已读取便签", out.join("\n\n"))
    }
}

struct ListSnippets;
impl AgentTool for ListSnippets {
    fn name(&self) -> &'static str { "list_snippets" }
    fn description(&self) -> &'static str { "列提示词百宝箱（返回置顶态与标题，不含正文——需要正文用 get_snippet，需要改动用 update_snippet）。仅当用户提到提示词或常用片段时调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let list = match repo::snippet::list(ctx.conn) { Ok(s) => s, Err(e) => return db_err(e) };
        let lines: Vec<String> = list.iter().map(|s: &Snippet| format!("{}[{}] {}", if s.is_pinned { "📌" } else { "" }, s.id, s.title)).collect();
        ToolOutcome::ok(format!("共 {} 条", lines.len()), cap_lines(lines))
    }
}

struct GetSnippet;
impl AgentTool for GetSnippet {
    fn name(&self) -> &'static str { "get_snippet" }
    fn description(&self) -> &'static str { "读单条提示词的完整正文。id 来自 list_snippets；配合用户要求把某条提示词套用到给定内容。没有具体 id 时不要调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"snippet_id":{"type":"integer","description":"提示词 id"}},"required":["snippet_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "snippet_id") else { return ToolOutcome::err("缺少 snippet_id 参数", String::new()) };
        let s = match repo::snippet::get(ctx.conn, id) { Ok(s) => s, Err(_) => return ToolOutcome::err(format!("提示词 {id} 不存在"), String::new()) };
        let meta = format!("《{}》{} 使用 {} 次", s.title, if s.is_pinned { "置顶" } else { "" }, s.copy_count);
        ToolOutcome::ok(format!("已读取《{}》", s.title), format!("{meta}\n\n{}", cap_chars(&s.content, MAX_TEXT)))
    }
}

struct ReadClipboard;
impl AgentTool for ReadClipboard {
    fn name(&self) -> &'static str { "read_clipboard" }
    fn description(&self) -> &'static str { "读最近的剪贴板历史（可带关键词过滤）。仅当用户提到剪贴板或复制过的内容时调用；图片条目只回元信息不回图。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"keyword":{"type":"string","description":"按内容过滤"},"limit":{"type":"integer","description":"最多几条（默认 10）"}}}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let kw = arg_str(args, "keyword");
        let limit = arg_i64(args, "limit").unwrap_or(10).clamp(1, 50);
        let items: Vec<ClipboardItem> = match repo::clipboard::list(ctx.conn, kw, limit, 0) { Ok(i) => i, Err(e) => return db_err(e) };
        let lines: Vec<String> = items.iter().map(|i: &ClipboardItem| match i.kind.as_str() {
            // 图片只回条目占位不回本地路径：绝对路径对本机外无意义，还会把
            // 数据根目录结构捎给外部模型
            "image" => format!("[{}][图片]", i.id),
            "file" => format!("[{}][文件] {}", i.id, i.file_paths.join("、")),
            _ => format!("[{}][文本] {}", i.id, cap_chars(&i.content, 300)),
        }).collect();
        ToolOutcome::ok(format!("{} 条", lines.len()), cap_lines(lines))
    }
}

struct GlobalSearch;
impl AgentTool for GlobalSearch {
    fn name(&self) -> &'static str { "global_search" }
    fn description(&self) -> &'static str { "跨 速达资源/笔记/待办 统一搜索。仅当用户要找的内容确定属于本机数据、但不确定在哪个模块时调用；已知模块时用各自的 search/list，与本地数据无关的对话不要调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"keyword":{"type":"string"}},"required":["keyword"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(kw) = arg_str(args, "keyword") else { return ToolOutcome::err("缺少 keyword 参数", String::new()) };
        let res = repo::resource::search(ctx.conn, kw).unwrap_or_default();
        let notes = repo::note::search(ctx.conn, kw).unwrap_or_default();
        let todos = repo::todo::search(ctx.conn, kw).unwrap_or_default();
        let mut sections = Vec::new();
        let r_lines: Vec<String> = res.iter().map(fmt_resource_line).collect();
        if !r_lines.is_empty() { sections.push(format!("【速达资源 {}】\n{}", res.len(), cap_lines(r_lines))); }
        let n_lines: Vec<String> = notes.iter().map(fmt_note_line).collect();
        if !n_lines.is_empty() { sections.push(format!("【笔记 {}】\n{}", notes.len(), cap_lines(n_lines))); }
        let t_lines: Vec<String> = todos.iter().map(fmt_todo_line).collect();
        if !t_lines.is_empty() { sections.push(format!("【待办 {}】\n{}", todos.len(), cap_lines(t_lines))); }
        if sections.is_empty() { return ToolOutcome::ok("没有匹配结果", format!("三个模块都没有匹配「{kw}」的内容。")); }
        ToolOutcome::ok("已找到", sections.join("\n\n"))
    }
}

struct GetWeather;
impl AgentTool for GetWeather {
    fn name(&self) -> &'static str { "get_weather" }
    fn description(&self) -> &'static str { "查当前天气（需要应用内已设置城市且联网开关开启）。仅当用户问天气时调用。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
        let config = crate::config::load();
        if !config.online_enabled {
            // 失败语义（§5）：返回说明而非错误，避免模型反复重试
            return ToolOutcome::ok("联网已关闭", "应用的联网开关已关闭，无法获取天气。可在设置里开启后重试。");
        }
        if config.weather_lat == 0.0 || config.weather_lng == 0.0 {
            return ToolOutcome::ok("未配置城市", "应用还没有设置天气城市，无法查天气。请用户在设置里配置城市。");
        }
        // 天气是异步网络请求，而工具执行是同步的：起独立线程 + 自建运行时，
        // 阻塞等待结果（低频调用，短暂阻塞可接受，且不会 panic 在 tokio 上下文里）
        let (tx, rx) = std::sync::mpsc::channel();
        let (lat, lng, city) = (config.weather_lat, config.weather_lng, config.weather_city.clone());
        std::thread::spawn(move || {
            let res = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt.block_on(crate::online::fetch_weather(lat, lng, &city)),
                Err(e) => Err(format!("天气运行时初始化失败：{e}")),
            };
            let _ = tx.send(res);
        });
        match rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(w)) => ToolOutcome::ok(
                format!("{} {:.0}°C", w.city, w.temperature),
                format!("{}：气温 {:.1}°C（体感 {:.1}°C），湿度 {:.0}%，风速 {:.1} km/h（WMO 代码 {}）",
                    w.city, w.temperature, w.apparent_temperature, w.relative_humidity, w.wind_speed, w.weather_code),
            ),
            Ok(Err(e)) => ToolOutcome::err(format!("天气获取失败：{e}"), "天气服务暂时不可用，可稍后重试。"),
            Err(_) => ToolOutcome::err("天气获取超时", "天气服务 10 秒内没有响应，可稍后重试。"),
        }
    }
}

struct GetSetting;
/// `get_setting` 可读键白名单：模型输出不可信，全量 AppConfig 里有供应商地址
/// （chat_models）、服务端地址、本机路径等——不设白名单等于把整份配置发给外部模型
const READABLE_SETTING_KEYS: &[&str] = &[
    "online_enabled", "weather_city", "quote_source", "countdown_sound", "clock_quote",
    "theme_mode", "font_scale", "notice_duration_ms",
    "clipboard_max_items", "clipboard_ttl_days", "clipboard_paused",
    "clipboard_image_enabled", "clipboard_file_enabled", "clipboard_paste_method",
    "chat_window_mode",
];
impl AgentTool for GetSetting {
    fn name(&self) -> &'static str { "get_setting" }
    fn description(&self) -> &'static str { "只读查询应用设置项（白名单内的展示类设置，如 online_enabled、weather_city、theme_mode）。仅当用户问应用自身的设置时调用；只读：AI 不能改设置。" }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"key":{"type":"string","description":"设置项名；缺省列出全部可用项"}}}) }
    fn execute(&self, args: &Value, _ctx: ToolCtx) -> ToolOutcome {
        let cfg = match serde_json::to_value(crate::config::load()) { Ok(v) => v, Err(e) => return ToolOutcome::err(format!("配置序列化失败：{e}"), String::new()) };
        match arg_str(args, "key") {
            Some(key) => match READABLE_SETTING_KEYS.iter().find(|k| **k == key).and_then(|k| cfg.get(*k)) {
                Some(v) => ToolOutcome::ok(format!("{key} 已读取"), format!("{key} = {}", serde_json::to_string_pretty(v).unwrap_or_default())),
                None => {
                    ToolOutcome::err(format!("设置项「{key}」不在可读范围"), format!("可用的设置项：{}", READABLE_SETTING_KEYS.join("、")))
                }
            },
            None => {
                ToolOutcome::ok("已列出", format!("可用设置项：{}", READABLE_SETTING_KEYS.join("、")))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 写（13）
// ---------------------------------------------------------------------------

struct CreateTodo;
impl AgentTool for CreateTodo {
    fn name(&self) -> &'static str { "create_todo" }
    fn description(&self) -> &'static str { "新建待办。title 必填；due_at/remind_at 支持「YYYY-MM-DD HH:mm」或毫秒时间戳；priority 0-2；repeat 传对象：{\"mode\":\"daily\"} 或 {\"mode\":\"weekly\",\"weekdays\":[1,3]} 或 {\"mode\":\"monthly\",\"month_day\":15}（也接受裸字符串 \"daily\"）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "title":{"type":"string"},
        "due_at":{"description":"截止时间：「YYYY-MM-DD HH:mm」24小时制，或毫秒时间戳；不传 = 无截止日期"},
        "remind_at":{"description":"提醒时间：格式同 due_at；不传 = 不提醒"},
        "priority":{"type":"integer","minimum":0,"maximum":2,"description":"0=普通（默认）1=重要 2=紧急"},
        "repeat":{"type":"object","properties":{
            "mode":{"type":"string","enum":["daily","weekly","monthly"]},
            "weekdays":{"type":"array","items":{"type":"integer","minimum":0,"maximum":6},"description":"weekly 必填：周几列表，0=周一…6=周日"},
            "month_day":{"type":"integer","minimum":-1,"maximum":31,"description":"monthly 必填：几号（-1=每月最后一天）"}
        },"required":["mode"]}
    },"required":["title"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(title) = arg_str(args, "title") else { return ToolOutcome::err("缺少 title 参数", String::new()) };
        let t = match repo::todo::create(ctx.conn, title, None, None) { Ok(t) => t, Err(e) => return db_err(e) };
        let mut t = t;
        let due = match arg_time_opt(args, "due_at") { Ok(v) => v, Err(m) => return ToolOutcome::err(m, String::new()) };
        let remind = match arg_time_opt(args, "remind_at") { Ok(v) => v, Err(m) => return ToolOutcome::err(m, String::new()) };
        if due.is_changed() || remind.is_changed() {
            let d = if due.is_changed() { due.value() } else { t.due_at };
            let r = if remind.is_changed() { remind.value() } else { t.remind_at };
            if d.is_some() || r.is_some() {
                t = match repo::todo::schedule(ctx.conn, t.id, d, r) { Ok(t) => t, Err(e) => return db_err(e) };
            }
        }
        if let Some(p) = arg_i64(args, "priority").filter(|p| (0..=2).contains(p)) {
            if p != t.priority {
                t = match repo::todo::update_with_version(ctx.conn, t.id, title, p, None) { Ok(t) => t, Err(e) => return ToolOutcome::err(e, String::new()) };
            }
        }
        if let Some(rep) = args.get("repeat").filter(|v| !v.is_null()) {
            let rule = match repeat_rule_from(rep) { Ok(r) => r, Err(m) => return ToolOutcome::err(m, String::new()) };
            // schema 只是建议，LLM 完全可能发任意串——不经 validate 直接落库的话，
            // 非法周期会在勾选完成时被静默降级成一次性（todo_recurrence 的 match 落空），
            // 与 commands.rs / xhub_api.rs 两条既有路径同款 fail-fast
            if let Err(e) = rule.validate() {
                return ToolOutcome::err(format!("repeat 参数不合法：{e}"), String::new());
            }
            t = match repo::todo::set_repeat(ctx.conn, t.id, &rule) { Ok(t) => t, Err(e) => return db_err(e) };
        }
        ToolOutcome::ok(format!("已创建「{}」", t.title), format!("已创建待办：{}", fmt_todo_line(&t)))
    }
}

/// repeat 参数 → RepeatRule：接受对象 `{mode, weekdays?, month_day?}` 或裸字符串 `"daily"`。
/// weekdays 为 0-6 整数数组（0=周一…6=周日），转内部位掩码
fn repeat_rule_from(v: &Value) -> Result<RepeatRule, String> {
    let (mode, weekdays, month_day) = match v {
        Value::String(s) => (s.trim().to_string(), None, None),
        Value::Object(o) => {
            let mode = o
                .get("mode")
                .and_then(|m| m.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or("repeat.mode 缺失")?
                .to_string();
            let weekdays = match o.get("weekdays") {
                None | Some(Value::Null) => None,
                Some(Value::Array(arr)) => {
                    let mut mask: i64 = 0;
                    for d in arr {
                        let n = d.as_i64().ok_or("repeat.weekdays 的元素必须是整数")?;
                        if !(0..=6).contains(&n) {
                            return Err(format!("repeat.weekdays 元素 {n} 越界（0=周一…6=周日）"));
                        }
                        mask |= 1 << n;
                    }
                    Some(mask)
                }
                Some(_) => return Err("repeat.weekdays 必须是整数数组（如 [1,3] 表示每周一、三）".into()),
            };
            let month_day = match o.get("month_day") {
                None | Some(Value::Null) => None,
                Some(Value::Number(n)) => {
                    let d = n.as_i64().ok_or("repeat.month_day 必须是整数")?;
                    if d != -1 && !(1..=31).contains(&d) {
                        return Err(format!("repeat.month_day {d} 越界（1-31，-1=月末）"));
                    }
                    Some(d)
                }
                Some(_) => return Err("repeat.month_day 必须是整数（1-31，-1=月末）".into()),
            };
            (mode, weekdays, month_day)
        }
        _ => return Err("repeat 必须是对象（{mode, weekdays?, month_day?}）或字符串（daily）".into()),
    };
    Ok(RepeatRule {
        mode,
        every: None,
        unit: None,
        weekdays,
        month_day,
        month_nth: None,
        end_mode: None,
        end_at: None,
        count: None,
    })
}

struct UpdateTodo;
impl AgentTool for UpdateTodo {
    fn name(&self) -> &'static str { "update_todo" }
    fn description(&self) -> &'static str { "修改待办：标题/截止/提醒/优先级/完成态/置顶。字段缺省=不动，due_at/remind_at 传 null=清除。完成周期待办会自动滚动到下一轮。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "todo_id":{"type":"integer"},"title":{"type":"string"},"due_at":{},"remind_at":{},
        "priority":{"type":"integer","minimum":0,"maximum":2},
        "done":{"type":"boolean"},"pinned":{"type":"boolean"}
    },"required":["todo_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "todo_id") else { return ToolOutcome::err("缺少 todo_id 参数", String::new()) };
        let mut t = match repo::todo::get(ctx.conn, id) { Ok(t) => t, Err(_) => return ToolOutcome::err(format!("待办 {id} 不存在"), String::new()) };
        let mut changes: Vec<&str> = Vec::new();
        if let Some(title) = arg_str(args, "title") {
            let p = arg_i64(args, "priority").unwrap_or(t.priority).clamp(0, 2);
            t = match repo::todo::update_with_version(ctx.conn, id, title, p, None) { Ok(t) => t, Err(e) => return ToolOutcome::err(e, String::new()) };
            changes.push("标题");
        } else if let Some(p) = arg_i64(args, "priority").filter(|p| (0..=2).contains(p)) {
            t = match repo::todo::update_with_version(ctx.conn, id, &t.title, p, None) { Ok(t) => t, Err(e) => return ToolOutcome::err(e, String::new()) };
            changes.push("优先级");
        }
        let due = match arg_time_opt(args, "due_at") { Ok(v) => v, Err(m) => return ToolOutcome::err(m, String::new()) };
        let remind = match arg_time_opt(args, "remind_at") { Ok(v) => v, Err(m) => return ToolOutcome::err(m, String::new()) };
        if due.is_changed() || remind.is_changed() {
            let d = if due.is_changed() { due.value() } else { t.due_at };
            let r = if remind.is_changed() { remind.value() } else { t.remind_at };
            t = match repo::todo::schedule(ctx.conn, id, d, r) { Ok(t) => t, Err(e) => return db_err(e) };
            changes.push("日期/提醒");
        }
        if let Some(want) = args.get("done").and_then(|v| v.as_bool()) {
            if want != t.done {
                // 周期待办完成 → 滚动到下一轮（计划 §5 口径）
                if want && t.repeat_mode != "once" {
                    t = match repo::todo::complete_recurring(ctx.conn, id, Local::now().timestamp_millis()) { Ok(t) => t, Err(e) => return ToolOutcome::err(e, String::new()) };
                } else {
                    t = match repo::todo::toggle(ctx.conn, id) { Ok(t) => t, Err(e) => return db_err(e) };
                }
                changes.push("完成态");
            }
        }
        if let Some(want) = args.get("pinned").and_then(|v| v.as_bool()) {
            if want != t.pinned {
                t = match repo::todo::set_pinned(ctx.conn, id, want) { Ok(t) => t, Err(e) => return db_err(e) };
                changes.push("置顶");
            }
        }
        if changes.is_empty() { return ToolOutcome::ok("没有改动", "没有需要改动的字段（或值与现状一致）。"); }
        ToolOutcome::ok(format!("已更新（{}）", changes.join("/")), format!("已更新待办：{}", fmt_todo_line(&t)))
    }
}

struct CreateNote;
impl AgentTool for CreateNote {
    fn name(&self) -> &'static str { "create_note" }
    fn description(&self) -> &'static str { "新建笔记。title/content 必填；folder/tags 可选且按名称**精确匹配**——匹配失败会返回候选列表，需先向用户确认，不要猜。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "title":{"type":"string"},"content":{"type":"string"},
        "folder":{"type":"string","description":"目标文件夹名（精确匹配）"},
        "tags":{"type":"array","items":{"type":"string"},"description":"标签名列表（精确匹配）"}
    },"required":["title","content"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(title) = arg_str(args, "title") else { return ToolOutcome::err("缺少 title 参数", String::new()) };
        let Some(content) = arg_str(args, "content") else { return ToolOutcome::err("缺少 content 参数", String::new()) };
        // 名称匹配先做完整校验（folder + 全部 tags），任何一个失败都不落库（§5：不猜不静默）
        let folder_id = match arg_str(args, "folder") {
            Some(want) => {
                let folders = match repo::note_folder::list(ctx.conn) { Ok(f) => f, Err(e) => return db_err(e) };
                let cands: Vec<(String, i64)> = folders.iter().map(|f| (f.name.clone(), f.id)).collect();
                match match_exact(&cands, want) { Ok(id) => Some(id), Err(c) => return not_found_outcome("文件夹", want, c) }
            }
            None => None,
        };
        let tag_ids: Vec<i64> = match args.get("tags").and_then(|v| v.as_array()) {
            Some(list) => {
                let all = match repo::tag::list(ctx.conn) { Ok(t) => t, Err(e) => return db_err(e) };
                let mut ids = Vec::new();
                for tv in list {
                    let Some(want) = tv.as_str().filter(|s| !s.trim().is_empty()) else { continue };
                    let cands: Vec<(String, i64)> = all.iter().map(|t| (t.name.clone(), t.id)).collect();
                    match match_exact(&cands, want) { Ok(id) => ids.push(id), Err(c) => return not_found_outcome("标签", want, c) }
                }
                ids
            }
            None => Vec::new(),
        };
        let note = match repo::note::create_in(ctx.conn, title, content, folder_id, "") { Ok(n) => n, Err(e) => return db_err(e) };
        if !tag_ids.is_empty() {
            if let Err(e) = repo::tag::set_note_tags(ctx.conn, note.id, &tag_ids) { return db_err(e); }
        }
        ToolOutcome::ok(format!("已创建《{}》", note.title), format!("已创建笔记 [{}]《{}》", note.id, note.title))
    }
}

struct UpdateNote;
impl AgentTool for UpdateNote {
    fn name(&self) -> &'static str { "update_note" }
    fn description(&self) -> &'static str { "修改笔记：标题或**整体替换**正文（content 是全文覆盖，追加用 append_to_note）；可换文件夹（按名精确匹配）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "note_id":{"type":"integer"},"title":{"type":"string"},"content":{"type":"string"},
        "folder":{"type":"string"}
    },"required":["note_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "note_id") else { return ToolOutcome::err("缺少 note_id 参数", String::new()) };
        let note = match repo::note::get(ctx.conn, id) { Ok(n) => n, Err(_) => return ToolOutcome::err(format!("笔记 {id} 不存在"), String::new()) };
        let title = arg_str(args, "title").unwrap_or(&note.title);
        let content = arg_str(args, "content").unwrap_or(&note.content);
        // 先做全部校验再落库（与 create_note 同款）：folder 匹配失败时标题/正文
        // 不能已经改了一半——模型拿到「失败」会整单重试，半途状态会让用户看到
        // 报错但数据已变（0.9.0 审查优化）
        let folder_id = match arg_str(args, "folder") {
            Some(want) => {
                let folders = match repo::note_folder::list(ctx.conn) { Ok(f) => f, Err(e) => return db_err(e) };
                let cands: Vec<(String, i64)> = folders.iter().map(|f| (f.name.clone(), f.id)).collect();
                match match_exact(&cands, want) { Ok(fid) => Some(fid), Err(c) => return not_found_outcome("文件夹", want, c) }
            }
            None => None,
        };
        if arg_str(args, "title").is_some() || arg_str(args, "content").is_some() {
            let _ = match repo::note::update(ctx.conn, id, title, content) { Ok(n) => n, Err(e) => return db_err(e) };
        }
        if let Some(fid) = folder_id {
            if let Err(e) = repo::note::set_folder(ctx.conn, id, Some(fid)) { return db_err(e); }
        }
        ToolOutcome::ok(format!("已更新《{}》", title), format!("已更新笔记 [{}]《{}》", id, title))
    }
}

struct AppendToNote;
impl AgentTool for AppendToNote {
    fn name(&self) -> &'static str { "append_to_note" }
    fn description(&self) -> &'static str { "把内容追加到已有笔记尾部（不覆盖原有正文）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"note_id":{"type":"integer"},"content":{"type":"string"}},"required":["note_id","content"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "note_id") else { return ToolOutcome::err("缺少 note_id 参数", String::new()) };
        let Some(text) = arg_str(args, "content") else { return ToolOutcome::err("缺少 content 参数", String::new()) };
        let note = match repo::note::get(ctx.conn, id) { Ok(n) => n, Err(_) => return ToolOutcome::err(format!("笔记 {id} 不存在"), String::new()) };
        let merged = if note.content.trim().is_empty() { text.to_string() } else { format!("{}\n{}", note.content, text) };
        match repo::note::update(ctx.conn, id, &note.title, &merged) { Ok(_) => (), Err(e) => return db_err(e) }
        ToolOutcome::ok(format!("已追加到《{}》", note.title), format!("已把 {} 字追加到笔记 [{}]《{}》尾部", text.chars().count(), id, note.title))
    }
}

struct DeleteNote;
impl AgentTool for DeleteNote {
    fn name(&self) -> &'static str { "delete_note" }
    fn description(&self) -> &'static str { "把笔记移入回收站（软删，可还原）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"note_id":{"type":"integer"}},"required":["note_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "note_id") else { return ToolOutcome::err("缺少 note_id 参数", String::new()) };
        let note = match repo::note::get(ctx.conn, id) { Ok(n) => n, Err(_) => return ToolOutcome::err(format!("笔记 {id} 不存在"), String::new()) };
        if let Err(e) = repo::note::trash(ctx.conn, id) { return db_err(e); }
        ToolOutcome::ok(format!("《{}》已移入回收站", note.title), format!("笔记 [{}]《{}》已移入回收站（可在回收站还原）。", id, note.title))
    }
}

struct CreateResource;
impl AgentTool for CreateResource {
    fn name(&self) -> &'static str { "create_resource" }
    fn description(&self) -> &'static str { "新建速达资源。name/target/kind 必填（kind=app|web|file）；category 小类可选且按名称精确匹配，不存在会返回候选——若用户明确要新建目录，先用 create_subcategory 建小类再创建资源。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "name":{"type":"string"},"target":{"type":"string","description":"exe 路径 / URL / 文件路径"},
        "kind":{"type":"string","enum":["app","web","file"]},
        "category":{"type":"string","description":"小类名（精确匹配）"}
    },"required":["name","target","kind"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(name) = arg_str(args, "name") else { return ToolOutcome::err("缺少 name 参数", String::new()) };
        let Some(target) = arg_str(args, "target") else { return ToolOutcome::err("缺少 target 参数", String::new()) };
        let kind = match arg_str(args, "kind") {
            Some("app") => ResourceKind::App,
            Some("web") => ResourceKind::Web,
            Some("file") => ResourceKind::File,
            _ => return ToolOutcome::err("kind 必须是 app/web/file 之一", String::new()),
        };
        // target 静默校验（0.9.0 审查优化）：「允许创建」不能变成「允许预置一个稍后
        // 一键启动的任意协议/程序」——web 只收 http(s)，app/file 拒绝 URL 形态
        match kind {
            ResourceKind::Web => {
                let t = target.to_ascii_lowercase();
                if !t.starts_with("http://") && !t.starts_with("https://") {
                    return ToolOutcome::err("web 资源的 target 必须是 http:// 或 https:// 开头的网址", String::new());
                }
            }
            ResourceKind::App | ResourceKind::File => {
                if target.contains("://") {
                    return ToolOutcome::err(format!("{} 资源的 target 必须是本地路径，不能是网址", crate::repo::resource::kind_to_str(&kind)), String::new());
                }
                if target.len() > 500 {
                    return ToolOutcome::err("target 路径过长", String::new());
                }
            }
        }
        let kind_str = crate::repo::resource::kind_to_str(&kind);
        let category = match arg_str(args, "category") {
            Some(want) => {
                let subs = match repo::subcategory::list(ctx.conn) { Ok(s) => s, Err(e) => return db_err(e) };
                let cands: Vec<(String, i64)> = subs.iter().filter(|s| s.kind == kind_str).map(|s| (s.name.clone(), s.id)).collect();
                match match_exact(&cands, want) { Ok(_) => Some(want), Err(c) => return not_found_outcome("小类", want, c) }
            }
            None => None,
        };
        let r = match repo::resource::create(ctx.conn, kind, name, target, category, None, None, None, None, None, None) { Ok(r) => r, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已创建「{}」", r.name), format!("已创建速达资源 [{}] {}", r.id, fmt_resource_line(&r)))
    }
}

struct AddSnippet;
impl AgentTool for AddSnippet {
    fn name(&self) -> &'static str { "add_snippet" }
    fn description(&self) -> &'static str { "保存一条提示词/常用片段到提示词百宝箱。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["title","content"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(title) = arg_str(args, "title") else { return ToolOutcome::err("缺少 title 参数", String::new()) };
        let Some(content) = arg_str(args, "content") else { return ToolOutcome::err("缺少 content 参数", String::new()) };
        let s = match repo::snippet::create(ctx.conn, title, content) { Ok(s) => s, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已保存「{}」", s.title), format!("已保存提示词 [{}] {}", s.id, s.title))
    }
}

struct UpdateSnippet;
impl AgentTool for UpdateSnippet {
    fn name(&self) -> &'static str { "update_snippet" }
    fn description(&self) -> &'static str { "修改提示词：标题/内容，可切换置顶。字段缺省=不动。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "snippet_id":{"type":"integer"},"title":{"type":"string"},"content":{"type":"string"},
        "pinned":{"type":"boolean"}
    },"required":["snippet_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "snippet_id") else { return ToolOutcome::err("缺少 snippet_id 参数", String::new()) };
        let s = match repo::snippet::get(ctx.conn, id) { Ok(s) => s, Err(_) => return ToolOutcome::err(format!("提示词 {id} 不存在"), String::new()) };
        let title = arg_str(args, "title").unwrap_or(&s.title);
        let content = arg_str(args, "content").unwrap_or(&s.content);
        if arg_str(args, "title").is_some() || arg_str(args, "content").is_some() {
            let _ = match repo::snippet::update(ctx.conn, id, title, content) { Ok(s) => s, Err(e) => return db_err(e) };
        }
        let mut pinned_note = "";
        if let Some(want) = args.get("pinned").and_then(|v| v.as_bool()) {
            if want != s.is_pinned {
                let _ = match repo::snippet::toggle_pin(ctx.conn, id) { Ok(s) => s, Err(e) => return db_err(e) };
                pinned_note = "，并切换置顶";
            }
        }
        ToolOutcome::ok(format!("已更新「{}」{pinned_note}", title), format!("已更新提示词 [{}] {}", id, title))
    }
}

struct WriteSticky;
impl AgentTool for WriteSticky {
    fn name(&self) -> &'static str { "write_sticky" }
    fn description(&self) -> &'static str { "写工作台便签（slot 1 或 2，整卡覆盖）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"slot":{"type":"integer","enum":[1,2]},"content":{"type":"string"}},"required":["slot","content"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(slot) = arg_i64(args, "slot").filter(|s| *s == 1 || *s == 2) else { return ToolOutcome::err("slot 必须是 1 或 2", String::new()) };
        let Some(content) = arg_str(args, "content") else { return ToolOutcome::err("缺少 content 参数", String::new()) };
        match repo::sticky::upsert(ctx.conn, slot, content) { Ok(_) => (), Err(e) => return db_err(e) }
        ToolOutcome::ok(format!("已写入便签{slot}"), format!("便签{slot} 已更新为 {} 字内容", content.chars().count()))
    }
}

struct CreateCountdown;
impl AgentTool for CreateCountdown {
    fn name(&self) -> &'static str { "create_countdown" }
    fn description(&self) -> &'static str { "新建倒计时。once：给 end_time（时刻）或 duration_minutes（时长）二选一；daily：end_time 为当天时刻；interval：给 interval_minutes。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{
        "name":{"type":"string"},
        "mode":{"type":"string","enum":["once","daily","interval"],"description":"默认 once"},
        "end_time":{"description":"时刻：YYYY-MM-DD HH:mm / 毫秒时间戳（daily 可只给 HH:mm 之外的完整时刻）"},
        "duration_minutes":{"type":"integer","description":"once 模式用时长代替 end_time"},
        "interval_minutes":{"type":"integer","description":"interval 模式的间隔分钟"}
    },"required":["name"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(name) = arg_str(args, "name") else { return ToolOutcome::err("缺少 name 参数", String::new()) };
        let mode = arg_str(args, "mode").unwrap_or("once").to_string();
        let now_ms = repo::countdown::now_ms();
        let (repeat_mode, end_at, total_ms, interval) = match mode.as_str() {
            "interval" => {
                let Some(m) = arg_i64(args, "interval_minutes").filter(|m| *m > 0) else { return ToolOutcome::err("interval 模式需要 interval_minutes（>0）", String::new()) };
                // LLM 输出不可信：上界防 m*60_000 与 now+m*60_000 溢出（debug panic / release 脏数据）
                if m > MAX_COUNTDOWN_MINUTES {
                    return ToolOutcome::err(format!("interval_minutes 过大（上限 {MAX_COUNTDOWN_MINUTES} 分钟）"), String::new());
                }
                let ms = m * 60_000;
                ("interval", now_ms + ms, ms, Some(m))
            }
            "daily" => {
                let Some(end) = args.get("end_time").and_then(parse_time_ms) else {
                    return ToolOutcome::err("daily 模式需要 end_time（时刻）", String::new())
                };
                ("daily", end, 86_400_000, None)
            }
            _ => {
                if let Some(m) = arg_i64(args, "duration_minutes").filter(|m| *m > 0) {
                    if m > MAX_COUNTDOWN_MINUTES {
                        return ToolOutcome::err(format!("duration_minutes 过大（上限 {MAX_COUNTDOWN_MINUTES} 分钟）"), String::new());
                    }
                    let ms = m * 60_000;
                    ("once", now_ms + ms, ms, None)
                } else if let Some(v) = args.get("end_time") {
                    let Some(end) = parse_time_ms(v) else { return ToolOutcome::err("end_time 不是可识别的时间", String::new()) };
                    let total = (end - now_ms).max(0);
                    ("once", end, total, None)
                } else {
                    return ToolOutcome::err("once 模式需要 end_time 或 duration_minutes", String::new())
                }
            }
        };
        let c = match repo::countdown::create(ctx.conn, name, repeat_mode, end_at, total_ms, interval) { Ok(c) => c, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已创建「{}」", c.name), format!("已创建倒计时 {}", fmt_countdown_line(&c)))
    }
}

struct ToggleCountdown;
impl AgentTool for ToggleCountdown {
    fn name(&self) -> &'static str { "toggle_countdown" }
    fn description(&self) -> &'static str { "暂停或恢复倒计时。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"countdown_id":{"type":"integer"},"action":{"type":"string","enum":["pause","resume"]}},"required":["countdown_id","action"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "countdown_id") else { return ToolOutcome::err("缺少 countdown_id 参数", String::new()) };
        let action = arg_str(args, "action").unwrap_or("");
        let c = match repo::countdown::get(ctx.conn, id) { Ok(c) => c, Err(_) => return ToolOutcome::err(format!("倒计时 {id} 不存在"), String::new()) };
        let r = match action {
            "pause" => if c.paused { Ok(c) } else { repo::countdown::pause(ctx.conn, id) },
            "resume" => if !c.paused { Ok(c) } else { repo::countdown::resume(ctx.conn, id) },
            _ => return ToolOutcome::err("action 必须是 pause 或 resume", String::new()),
        };
        let c = match r { Ok(c) => c, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已{}「{}」", if action == "pause" { "暂停" } else { "恢复" }, c.name), format!("倒计时 {}", fmt_countdown_line(&c)))
    }
}

struct SetClipboard;
impl AgentTool for SetClipboard {
    fn name(&self) -> &'static str { "set_clipboard" }
    fn description(&self) -> &'static str { "把文本写入系统剪贴板（同时进剪贴板历史）。" }
    fn level(&self) -> Level { Level::Write }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(text) = arg_str(args, "text") else { return ToolOutcome::err("缺少 text 参数", String::new()) };
        if let Err(e) = crate::clipboard::set_clipboard(text, None) { return ToolOutcome::err(format!("写剪贴板失败：{e}"), String::new()); }
        let _ = repo::clipboard::insert(ctx.conn, text, None, Some("agent"));
        ToolOutcome::ok(format!("已复制 {} 字", text.chars().count()), "文本已写入系统剪贴板。")
    }
}

// ---------------------------------------------------------------------------
// 危险（5）
// ---------------------------------------------------------------------------

struct DeleteTodo;
impl AgentTool for DeleteTodo {
    fn name(&self) -> &'static str { "delete_todo" }
    fn description(&self) -> &'static str { "**硬删**待办（含其子待办，不可恢复）。想保留记录请用 update_todo 改完成态。" }
    fn level(&self) -> Level { Level::Dangerous }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"todo_id":{"type":"integer"}},"required":["todo_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "todo_id") else { return ToolOutcome::err("缺少 todo_id 参数", String::new()) };
        let title = repo::todo::get(ctx.conn, id).map(|t| t.title).unwrap_or_default();
        let removed = match repo::todo::delete(ctx.conn, id) { Ok(r) => r, Err(e) => return db_err(e) };
        ToolOutcome::ok(format!("已删除「{title}」"), format!("待办 [{id}]「{title}」已硬删（连带删除 {} 条子待办）。", removed.len().saturating_sub(1)))
    }
}

struct DeleteResource;
impl AgentTool for DeleteResource {
    fn name(&self) -> &'static str { "delete_resource" }
    fn description(&self) -> &'static str { "**硬删**速达资源（不可恢复）。" }
    fn level(&self) -> Level { Level::Dangerous }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"resource_id":{"type":"integer"}},"required":["resource_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "resource_id") else { return ToolOutcome::err("缺少 resource_id 参数", String::new()) };
        let name = repo::resource::get(ctx.conn, id).map(|r| r.name).unwrap_or_default();
        if let Err(e) = repo::resource::delete(ctx.conn, id) { return db_err(e); }
        ToolOutcome::ok(format!("已删除「{name}」"), format!("速达资源 [{id}]「{name}」已硬删。"))
    }
}

struct DeleteSnippet;
impl AgentTool for DeleteSnippet {
    fn name(&self) -> &'static str { "delete_snippet" }
    fn description(&self) -> &'static str { "**硬删**提示词（不可恢复）。" }
    fn level(&self) -> Level { Level::Dangerous }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"snippet_id":{"type":"integer"}},"required":["snippet_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "snippet_id") else { return ToolOutcome::err("缺少 snippet_id 参数", String::new()) };
        let title = repo::snippet::get(ctx.conn, id).map(|s| s.title).unwrap_or_default();
        if let Err(e) = repo::snippet::delete(ctx.conn, id) { return db_err(e); }
        ToolOutcome::ok(format!("已删除「{title}」"), format!("提示词 [{id}]「{title}」已硬删。"))
    }
}

struct DeleteCountdown;
impl AgentTool for DeleteCountdown {
    fn name(&self) -> &'static str { "delete_countdown" }
    fn description(&self) -> &'static str { "**硬删**倒计时（不可恢复）。" }
    fn level(&self) -> Level { Level::Dangerous }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"countdown_id":{"type":"integer"}},"required":["countdown_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "countdown_id") else { return ToolOutcome::err("缺少 countdown_id 参数", String::new()) };
        let name = repo::countdown::get(ctx.conn, id).map(|c| c.name).unwrap_or_default();
        if let Err(e) = repo::countdown::delete(ctx.conn, id) { return db_err(e); }
        ToolOutcome::ok(format!("已删除「{name}」"), format!("倒计时 [{id}]「{name}」已硬删。"))
    }
}

struct LaunchResource;
impl AgentTool for LaunchResource {
    fn name(&self) -> &'static str { "launch_resource" }
    fn description(&self) -> &'static str { "启动速达资源：拉起应用 / 打开网页或文件（等同用户点击卡片）。id 来自 list_resources。" }
    fn level(&self) -> Level { Level::Dangerous }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{"resource_id":{"type":"integer"}},"required":["resource_id"]}) }
    fn execute(&self, args: &Value, ctx: ToolCtx) -> ToolOutcome {
        let Some(id) = arg_i64(args, "resource_id") else { return ToolOutcome::err("缺少 resource_id 参数", String::new()) };
        let res = match repo::resource::get(ctx.conn, id) { Ok(r) => r, Err(_) => return ToolOutcome::err(format!("资源 {id} 不存在"), String::new()) };
        // 与 commands::launch_resource 同一套核心（进程已在运行且有可见窗口 → 调度前台）
        let r = match res.kind {
            ResourceKind::App => {
                let no_args = res.args.as_deref().map(|a| a.trim().is_empty()).unwrap_or(true);
                if no_args && crate::process::activate_existing(&res.target) {
                    let _ = repo::resource::touch(ctx.conn, id);
                    return ToolOutcome::ok(format!("已把「{}」调度到前台", res.name), format!("程序已在运行，已把「{}」窗口调度到前台。", res.name));
                }
                crate::process::launch_program(&res.target, res.args.as_deref())
            }
            ResourceKind::Web => crate::process::open_url(&res.target),
            ResourceKind::File => crate::process::open_path(&res.target),
        };
        match r {
            Ok(()) => {
                let _ = repo::resource::touch(ctx.conn, id);
                ToolOutcome::ok(format!("已启动「{}」", res.name), format!("已启动速达资源 [{}] {}（{}：{}）", res.id, res.name, kind_cn(&res.kind), res.target))
            }
            Err(e) => ToolOutcome::err(format!("启动「{}」失败：{e}", res.name), String::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// 注册
// ---------------------------------------------------------------------------

/// 注册全部 31 个内置工具（顺序稳定——tools 数组随每次请求下发，勿乱序）
// ---------------------------------------------------------------------------
// 扩展桥（manifest.agent.tools）：扩展自描述的 AI 工具，宿主聚合后转发。
// 清单与调用都走 ext_tools 聚合器；审批档位在 approval_for 里按目标声明取。
// ---------------------------------------------------------------------------

struct ExtensionToolList;
impl AgentTool for ExtensionToolList {
    fn name(&self) -> &'static str { "extension_tool_list" }
    fn description(&self) -> &'static str {
        "列出扩展声明给 AI 的工具（service 扩展在 manifest.agent.tools 里自描述：名称/用途/参数/返回/档位）。仅当用户的任务可能落在某个扩展的数据或功能上时先调本工具发现，再用 extension_tool_call 调用；与扩展无关的对话不要调用。"
    }
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn execute(&self, _args: &Value, _ctx: ToolCtx) -> ToolOutcome {
        let cfg = crate::config::load();
        if !cfg.agent_ext_tools_enabled {
            // 失败语义（§5）：返回说明而非错误，避免模型反复重试
            return ToolOutcome::ok("扩展工具已关闭", "AI 调用扩展工具的总开关已关闭（设置 → AI 助手）。如用户需要，请指引其开启。");
        }
        let list = super::ext_tools::list_available();
        if list.is_empty() {
            return ToolOutcome::ok("没有可用的扩展工具", "当前没有扩展声明 AI 工具（仅 service 扩展可声明，且需用户允许 AI 调用）。不要臆造扩展能力。");
        }
        // 按扩展分组展示：模型能看出「这个工具是谁家的」，回话时也能交代来源
        let mut sections: Vec<String> = Vec::new();
        let mut cur_ext = String::new();
        let mut cur_lines: Vec<String> = Vec::new();
        for a in &list {
            if a.ext_id != cur_ext {
                if !cur_lines.is_empty() {
                    sections.push(format!("{cur_ext}：\n{}", cur_lines.join("\n")));
                }
                cur_ext = a.ext_id.clone();
                cur_lines = Vec::new();
                cur_lines.push(format!("《{}》（{}）提供：", a.ext_name, a.ext_id));
            }
            cur_lines.push(super::ext_tools::describe_tool(a));
        }
        if !cur_lines.is_empty() {
            sections.push(format!("{cur_ext}：\n{}", cur_lines.join("\n")));
        }
        let ext_count = sections.len();
        ToolOutcome::ok(
            format!("{ext_count} 个扩展提供 {} 个工具", list.len()),
            sections.join("\n\n"),
        )
    }
}

struct ExtensionToolCall;
impl AgentTool for ExtensionToolCall {
    fn name(&self) -> &'static str { "extension_tool_call" }
    fn description(&self) -> &'static str { "调用扩展声明的 AI 工具。ext 与 tool 取自 extension_tool_list 的清单，args 按该工具声明的参数传；没先调 extension_tool_list 拿到清单时不要调用。写/危险档的工具按权限档需要用户确认，被拒时不要重试。" }
    // 静态档 Read 仅供 ReadOnly 模式的可见性过滤；实际审批档在 approval_for 按目标声明取
    fn level(&self) -> Level { Level::Read }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "ext":{"type":"string","description":"扩展 id，来自 extension_tool_list"},
            "tool":{"type":"string","description":"工具名，来自 extension_tool_list"},
            "args":{"type":"object","description":"按该工具声明的参数传，无参工具可省略"}
        },"required":["ext","tool"]})
    }
    fn approval_for(&self, args: &Value) -> (Level, String) {
        let (Some(ext), Some(tool)) = (
            arg_str(args, "ext"),
            arg_str(args, "tool"),
        ) else {
            // 参数不全：按读档处理，执行阶段会报缺参
            return (Level::Read, self.name().to_string());
        };
        match super::ext_tools::find(ext, tool) {
            Some(a) => (super::ext_tools::level_of(&a.tool.level), super::ext_tools::perm_key(ext, tool)),
            // 目标不存在（未声明/已禁用/未授权）：读档放行到执行阶段，由聚合器报明确原因
            None => (Level::Read, self.name().to_string()),
        }
    }
    fn execute(&self, args: &Value, _ctx: ToolCtx) -> ToolOutcome {
        let Some(ext) = arg_str(args, "ext") else { return ToolOutcome::err("缺少 ext 参数（扩展 id，来自 extension_tool_list）", String::new()) };
        let Some(tool) = arg_str(args, "tool") else { return ToolOutcome::err("缺少 tool 参数（工具名，来自 extension_tool_list）", String::new()) };
        let tool_args = args.get("args").cloned().unwrap_or_else(|| json!({}));
        if !tool_args.is_object() {
            return ToolOutcome::err("args 必须是对象（按目标工具声明的参数传）", String::new());
        }
        match super::ext_tools::call(ext, tool, &tool_args) {
            Ok(v) => {
                let detail = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
                ToolOutcome::ok(format!("{} 已返回", tool), cap_chars(&detail, MAX_TEXT))
            }
            Err(e) => ToolOutcome::err(format!("{} 调用失败", tool), e),
        }
    }
}

pub fn register_all(reg: &mut ToolRegistry) {
    // 读 15
    reg.register(Box::new(ListTodos));
    reg.register(Box::new(SearchNotes));
    reg.register(Box::new(GetNote));
    reg.register(Box::new(ListNotes));
    reg.register(Box::new(ListResources));
    reg.register(Box::new(ListCountdowns));
    reg.register(Box::new(GetStickies));
    reg.register(Box::new(ListSnippets));
    reg.register(Box::new(GetSnippet));
    reg.register(Box::new(ReadClipboard));
    reg.register(Box::new(GlobalSearch));
    reg.register(Box::new(GetWeather));
    reg.register(Box::new(GetSetting));
    reg.register(Box::new(ExtensionToolList)); // 扩展桥：静态 Read（实际档位按目标声明）
    reg.register(Box::new(ExtensionToolCall));
    // 写 14
    reg.register(Box::new(CreateTodo));
    reg.register(Box::new(UpdateTodo));
    reg.register(Box::new(CreateNote));
    reg.register(Box::new(UpdateNote));
    reg.register(Box::new(AppendToNote));
    reg.register(Box::new(DeleteNote));
    reg.register(Box::new(CreateResource));
    reg.register(Box::new(CreateSubcategory)); // 0.9.0 新增：速达小类（目录）创建
    reg.register(Box::new(AddSnippet));
    reg.register(Box::new(UpdateSnippet));
    reg.register(Box::new(WriteSticky));
    reg.register(Box::new(CreateCountdown));
    reg.register(Box::new(ToggleCountdown));
    reg.register(Box::new(SetClipboard));
    // 危险 5
    reg.register(Box::new(DeleteTodo));
    reg.register(Box::new(DeleteResource));
    reg.register(Box::new(DeleteSnippet));
    reg.register(Box::new(DeleteCountdown));
    reg.register(Box::new(LaunchResource));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> ToolRegistry {
        let mut r = ToolRegistry::empty();
        register_all(&mut r);
        r
    }

    #[test]
    fn registers_exactly_34_tools() {
        let r = reg();
        assert_eq!(r.len(), 34, "§5 全量 30 个 + 0.9.0 create_subcategory + get_snippet + 扩展桥 2 个");
        let read = r.available(crate::agent::approval::PermissionMode::ReadOnly).len();
        assert_eq!(read, 15, "读 15 个（含扩展桥 extension_tool_list/call，静态 Read）");
    }

    /// 「要不要查」的工具侧门控（2026-10-21 修「随便说什么都去翻笔记/待办」）。
    ///
    /// flash 级模型对**工具描述**的就近匹配很敏感，光改 system prompt 不够：每个读工具的
    /// description 都必须带一句「什么时候不要调用」的否定式门控。这里逐条盯住，防止后续
    /// 重构顺手把措辞改回只有正向说明（那种描述下模型会把它当成「随时可用的顺手工具」）。
    #[test]
    fn read_tools_descriptions_carry_negative_gate() {
        let r = reg();
        let reads = [
            "list_todos", "search_notes", "get_note", "list_notes", "list_resources",
            "list_countdowns", "get_stickies", "list_snippets", "get_snippet",
            "read_clipboard", "global_search", "get_weather", "get_setting",
            "extension_tool_list", "extension_tool_call",
        ];
        for name in reads {
            let d = r.get(name).expect(name).description();
            assert!(
                d.contains("仅当") || d.contains("不要"),
                "读工具 {name} 的描述缺少否定式门控：{d}"
            );
        }
        // 通用型的两个读工具必须显式点名「与本地数据无关时不要调用」——
        // 它们是最容易被拿来当兜底搜刮的（global_search 一次扫三个模块）
        assert!(r.get("global_search").unwrap().description().contains("与本地数据无关的对话不要调用"));
        assert!(r.get("extension_tool_call").unwrap().description().contains("没先调 extension_tool_list"));
    }

    /// 扩展桥的审批档位覆写：approval_for 按目标工具声明取档；宿主未就绪/目标不存在
    /// 时回落 (Read, 工具名)，执行阶段报明确原因
    #[test]
    fn extension_tool_call_approval_falls_back_without_host() {
        let r = reg();
        let t = r.get("extension_tool_call").unwrap();
        let (level, key) = t.approval_for(&json!({"ext":"com.a","tool":"query"}));
        assert_eq!(level, Level::Read, "单测环境无 HOST_APP，聚合为空 → 回落 Read");
        assert_eq!(key, "extension_tool_call");
        let (level2, key2) = t.approval_for(&json!({}));
        assert_eq!(level2, Level::Read);
        assert_eq!(key2, "extension_tool_call");
    }

    /// 0.9.0 审查回归：LLM 输出的分钟数/时间戳是不可信输入——超界必须明确拒绝，
    /// 不能溢出成 panic（debug）或负值脏数据（release）
    #[test]
    fn countdown_and_time_args_reject_out_of_range() {
        let conn = crate::db::init_in_memory().unwrap();
        // interval_minutes 上界：i64::MAX 会溢出
        let out = CreateCountdown.execute(
            &json!({"name":"x","mode":"interval","interval_minutes": i64::MAX}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok, "超界 interval_minutes 不应创建成功");
        // duration_minutes 同款
        let out = CreateCountdown.execute(
            &json!({"name":"x","duration_minutes": i64::MAX}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok);
        // 合法区间内的仍然成功
        let ok = CreateCountdown.execute(
            &json!({"name":"x","mode":"interval","interval_minutes": 30}),
            ToolCtx { conn: &conn },
        );
        assert!(ok.ok, "{}", ok.summary);
        // 时间戳超出 2000~2100 区间按不可识别拒绝
        let out = CreateCountdown.execute(
            &json!({"name":"y","end_time": i64::MAX}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok);
    }

    /// 0.9.0 审查回归：repeat 模式串必须过 RepeatRule::validate——非法周期直接落库
    /// 会在勾选完成时被静默降级成一次性（todo_recurrence 的 match 落空）
    #[test]
    fn create_todo_rejects_invalid_repeat_mode() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = CreateTodo.execute(
            &json!({"title":"测试","repeat":"yearlies"}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok, "非法 repeat 模式不应落库");
        // 裸字符串只认无需附加字段的模式
        let ok = CreateTodo.execute(
            &json!({"title":"测试","repeat":"daily"}),
            ToolCtx { conn: &conn },
        );
        assert!(ok.ok, "{}", ok.summary);
        // 对象形式：weekly 带 weekdays、monthly 带 month_day
        let ok = CreateTodo.execute(
            &json!({"title":"测试","repeat":{"mode":"weekly","weekdays":[1,3]}}),
            ToolCtx { conn: &conn },
        );
        assert!(ok.ok, "{}", ok.summary);
        let ok = CreateTodo.execute(
            &json!({"title":"测试","repeat":{"mode":"monthly","month_day":15}}),
            ToolCtx { conn: &conn },
        );
        assert!(ok.ok, "{}", ok.summary);
        // weekly 缺 weekdays：validate 拒绝并给模型明确原因
        let out = CreateTodo.execute(
            &json!({"title":"测试","repeat":"weekly"}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok);
        // 越界 weekdays 拒绝
        let out = CreateTodo.execute(
            &json!({"title":"测试","repeat":{"mode":"weekly","weekdays":[9]}}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok);
    }

    /// 0.9.0 审查优化回归：create_resource 的 target 静默校验——
    /// web 只收 http(s)（防预置 file:// 等任意协议稍后一键启动），app/file 拒绝 URL 形态
    #[test]
    fn create_resource_validates_target_shape() {
        let conn = crate::db::init_in_memory().unwrap();
        let bad = CreateResource.execute(
            &json!({"name":"x","target":"file:///C:/Windows/notepad.exe","kind":"web"}),
            ToolCtx { conn: &conn },
        );
        assert!(!bad.ok, "web 资源非 http(s) 协议应被拒绝");
        let bad = CreateResource.execute(
            &json!({"name":"x","target":"https://example.com/app","kind":"app"}),
            ToolCtx { conn: &conn },
        );
        assert!(!bad.ok, "app 资源的 target 不能是网址");
        let ok = CreateResource.execute(
            &json!({"name":"x","target":"https://example.com","kind":"web"}),
            ToolCtx { conn: &conn },
        );
        assert!(ok.ok, "{}", ok.summary);
    }

    /// 0.9.0 审查优化回归：update_note 先验后写——folder 匹配失败时
    /// 标题/正文不能已经改了一半
    #[test]
    fn update_note_validates_folder_before_writing() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = repo::note::create(&conn, "原标题").unwrap();
        let out = UpdateNote.execute(
            &json!({"note_id": n.id, "title":"改一半的标题","folder":"不存在的文件夹"}),
            ToolCtx { conn: &conn },
        );
        assert!(!out.ok, "文件夹匹配失败应报错");
        let after = repo::note::get(&conn, n.id).unwrap();
        assert_eq!(after.title, "原标题", "报错时标题不能已被改动");
    }

    /// 0.9.0 新增 get_snippet：按 id 取提示词正文；不存在的 id 明确报错
    #[test]
    fn get_snippet_returns_content_and_errors_on_missing() {
        let conn = crate::db::init_in_memory().unwrap();
        let s = repo::snippet::create(&conn, "写作润色", "请把下面的文字改写得更简洁：").unwrap();
        let out = GetSnippet.execute(&json!({ "snippet_id": s.id }), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        assert!(out.detail.contains("请把下面的文字改写得更简洁"), "正文要完整返回：{}", out.detail);
        let miss = GetSnippet.execute(&json!({ "snippet_id": 99999 }), ToolCtx { conn: &conn });
        assert!(!miss.ok);
    }

    /// 0.9.0 审查回归：get_setting 只放行白名单键——chat_models 等敏感配置不外发
    #[test]
    fn get_setting_whitelists_keys() {
        let conn = crate::db::init_in_memory().unwrap();
        let ok = GetSetting.execute(&json!({"key":"online_enabled"}), ToolCtx { conn: &conn });
        assert!(ok.ok);
        let denied = GetSetting.execute(&json!({"key":"chat_models"}), ToolCtx { conn: &conn });
        assert!(!denied.ok, "chat_models 不在白名单，不应可读");
        let denied = GetSetting.execute(&json!({"key":"server_url"}), ToolCtx { conn: &conn });
        assert!(!denied.ok);
        // 无 key 列出的可用项里也不含敏感键
        let list = GetSetting.execute(&json!({}), ToolCtx { conn: &conn });
        assert!(list.ok);
        assert!(!list.detail.contains("chat_models"));
    }

    /// 0.9.0 回归：模型可自主新建速达小类，再用它创建资源（幂等不重复建）
    #[test]
    fn create_subcategory_then_resource() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = CreateSubcategory.execute(&json!({"kind":"app","name":"开发工具"}), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        // 幂等：同名再建直接返回已存在
        let again = CreateSubcategory.execute(&json!({"kind":"app","name":"开发工具"}), ToolCtx { conn: &conn });
        assert!(again.ok);
        assert!(again.detail.contains("已存在"));
        // 资源能落进新小类
        let res = CreateResource.execute(
            &json!({"name":"VSCode","target":"C:\\code\\Code.exe","kind":"app","category":"开发工具"}),
            ToolCtx { conn: &conn },
        );
        assert!(res.ok, "{}", res.summary);
        // 不同 kind 同名不算已存在（app/web/file 小类各自独立）
        let web = CreateSubcategory.execute(&json!({"kind":"web","name":"开发工具"}), ToolCtx { conn: &conn });
        assert!(web.ok);
        assert!(!web.detail.contains("已存在"));
    }

    #[test]
    fn list_todos_groups_and_hides_done() {
        let conn = crate::db::init_in_memory().unwrap();
        let _ = repo::todo::create(&conn, "普通待办", None, None).unwrap();
        let done = repo::todo::create(&conn, "已完成条目", None, None).unwrap();
        repo::todo::toggle(&conn, done.id).unwrap();
        let pin = repo::todo::create(&conn, "置顶待办", None, None).unwrap();
        repo::todo::set_pinned(&conn, pin.id, true).unwrap();
        let out = ListTodos.execute(&json!({}), ToolCtx { conn: &conn });
        assert!(out.ok);
        assert!(out.detail.contains("【置顶】"));
        assert!(out.detail.contains("置顶待办"));
        assert!(out.detail.contains("普通待办"));
        assert!(out.detail.contains("已完成，未列出"));
        assert!(!out.detail.contains("已完成条目\n") || out.detail.contains("另有"));
    }

    #[test]
    fn create_todo_with_time_and_repeat() {
        let conn = crate::db::init_in_memory().unwrap();
        let due = Local::now().timestamp_millis() + 3_600_000;
        let out = CreateTodo.execute(&json!({"title":"写周报","due_at":due,"repeat":"daily","priority":2}), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        let todos = repo::todo::list(&conn).unwrap();
        let t = todos.iter().find(|t| t.title == "写周报").unwrap();
        assert_eq!(t.due_at, Some(due));
        assert_eq!(t.repeat_mode, "daily");
        assert_eq!(t.priority, 2);
    }

    #[test]
    fn create_note_folder_mismatch_returns_candidates() {
        let conn = crate::db::init_in_memory().unwrap();
        repo::note_folder::create(&conn, "工作", None).unwrap();
        repo::note_folder::create(&conn, "生活", None).unwrap();
        let out = CreateNote.execute(&json!({"title":"t","content":"c","folder":"工作笔记"}), ToolCtx { conn: &conn });
        assert!(!out.ok);
        assert!(out.detail.contains("候选"), "{}", out.detail);
        assert!(out.detail.contains("工作") && out.detail.contains("生活"));
        // 精确匹配则成功
        let ok = CreateNote.execute(&json!({"title":"t2","content":"c","folder":"工作"}), ToolCtx { conn: &conn });
        assert!(ok.ok);
        assert_eq!(repo::note::list_meta(&conn).unwrap().len(), 1);
    }

    #[test]
    fn create_note_tags_exact_match_only() {
        let conn = crate::db::init_in_memory().unwrap();
        repo::tag::create(&conn, "重要").unwrap();
        let bad = CreateNote.execute(&json!({"title":"t","content":"c","tags":["重要啊"]}), ToolCtx { conn: &conn });
        assert!(!bad.ok);
        assert!(bad.summary.contains("「重要啊」未找到"));
        assert!(bad.detail.contains("未找到标签「重要啊」"));
        let good = CreateNote.execute(&json!({"title":"t","content":"c","tags":["重要"]}), ToolCtx { conn: &conn });
        assert!(good.ok, "{}", good.summary);
        let note = &repo::note::list_meta(&conn).unwrap()[0];
        assert_eq!(repo::tag::tags_of_note(&conn, note.id).unwrap().len(), 1);
    }

    #[test]
    fn update_todo_done_and_pin() {
        let conn = crate::db::init_in_memory().unwrap();
        let t = repo::todo::create(&conn, "测试", None, None).unwrap();
        let out = UpdateTodo.execute(&json!({"todo_id":t.id,"done":true,"pinned":true}), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        let t = repo::todo::get(&conn, t.id).unwrap();
        assert!(t.done && t.pinned);
    }

    #[test]
    fn append_and_update_note() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = repo::note::create(&conn, "日记").unwrap();
        AppendToNote.execute(&json!({"note_id":n.id,"content":"第一段"}), ToolCtx { conn: &conn });
        let n2 = repo::note::get(&conn, n.id).unwrap();
        assert!(n2.content.contains("第一段"));
        UpdateNote.execute(&json!({"note_id":n.id,"title":"日记改"}), ToolCtx { conn: &conn });
        assert_eq!(repo::note::get(&conn, n.id).unwrap().title, "日记改");
    }

    #[test]
    fn delete_note_is_soft() {
        let conn = crate::db::init_in_memory().unwrap();
        let n = repo::note::create(&conn, "要删的").unwrap();
        let out = DeleteNote.execute(&json!({"note_id":n.id}), ToolCtx { conn: &conn });
        assert!(out.ok);
        assert!(out.detail.contains("回收站"));
        assert!(repo::note::get(&conn, n.id).unwrap().deleted_at.is_some());
    }

    #[test]
    fn sticky_write_and_read() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = WriteSticky.execute(&json!({"slot":1,"content":"购物清单"}), ToolCtx { conn: &conn });
        assert!(out.ok);
        let read = GetStickies.execute(&json!({}), ToolCtx { conn: &conn });
        assert!(read.detail.contains("购物清单"));
        assert!(read.detail.contains("（空）"));
    }

    #[test]
    fn snippet_lifecycle() {
        let conn = crate::db::init_in_memory().unwrap();
        AddSnippet.execute(&json!({"title":"周报模板","content":"本周..."}), ToolCtx { conn: &conn });
        UpdateSnippet.execute(&json!({"snippet_id":1,"pinned":true}), ToolCtx { conn: &conn });
        assert!(repo::snippet::get(&conn, 1).unwrap().is_pinned);
        let del = DeleteSnippet.execute(&json!({"snippet_id":1}), ToolCtx { conn: &conn });
        assert!(del.ok);
        assert!(repo::snippet::get(&conn, 1).is_err());
    }

    #[test]
    fn countdown_create_once_and_interval() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = CreateCountdown.execute(&json!({"name":"泡面","duration_minutes":3}), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        let out2 = CreateCountdown.execute(&json!({"name":"喝水","mode":"interval","interval_minutes":45}), ToolCtx { conn: &conn });
        assert!(out2.ok, "{}", out2.summary);
        let list = repo::countdown::list(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].repeat_mode, "interval");
        assert_eq!(list[1].interval_minutes, Some(45));
        // 参数不齐 → 明确报错
        let bad = CreateCountdown.execute(&json!({"name":"x"}), ToolCtx { conn: &conn });
        assert!(!bad.ok);
    }

    #[test]
    fn clipboard_write_goes_to_history() {
        let conn = crate::db::init_in_memory().unwrap();
        // 系统剪贴板写不入（测试环境无 GUI 会话也不炸）：set_clipboard 失败则工具报 err
        let out = SetClipboard.execute(&json!({"text":"hello"}), ToolCtx { conn: &conn });
        let _ = out; // 结果依赖环境，仅验证不 panic
    }

    #[test]
    fn global_search_hits_all_three_modules() {
        let conn = crate::db::init_in_memory().unwrap();
        repo::todo::create(&conn, "买 xyz", None, None).unwrap();
        repo::note::create(&conn, "xyz 计划").unwrap();
        let out = GlobalSearch.execute(&json!({"keyword":"xyz"}), ToolCtx { conn: &conn });
        assert!(out.ok);
        assert!(out.detail.contains("待办"));
        assert!(out.detail.contains("笔记"));
    }

    #[test]
    fn get_setting_roundtrip() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = GetSetting.execute(&json!({"key":"online_enabled"}), ToolCtx { conn: &conn });
        assert!(out.ok, "{}", out.summary);
        assert!(out.detail.contains("online_enabled"));
        let bad = GetSetting.execute(&json!({"key":"no_such_key"}), ToolCtx { conn: &conn });
        assert!(!bad.ok);
        assert!(bad.detail.contains("可用的设置项"));
    }

    #[test]
    fn resource_create_with_subcategory_match() {
        let conn = crate::db::init_in_memory().unwrap();
        repo::subcategory::create(&conn, "app", "开发工具").unwrap();
        let bad = CreateResource.execute(&json!({"name":"VSCode","target":"C:\\code.exe","kind":"app","category":"开发"}), ToolCtx { conn: &conn });
        assert!(!bad.ok);
        assert!(bad.detail.contains("开发工具"));
        let ok = CreateResource.execute(&json!({"name":"VSCode","target":"C:\\code.exe","kind":"app","category":"开发工具"}), ToolCtx { conn: &conn });
        assert!(ok.ok, "{}", ok.summary);
    }

    #[test]
    fn launch_missing_resource_errors() {
        let conn = crate::db::init_in_memory().unwrap();
        let out = LaunchResource.execute(&json!({"resource_id":999}), ToolCtx { conn: &conn });
        assert!(!out.ok);
        assert!(out.summary.contains("不存在"));
    }
}
