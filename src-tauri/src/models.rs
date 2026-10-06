use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    App,
    Web,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: i64,
    pub kind: ResourceKind,
    pub name: String,
    pub target: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub args: Option<String>,
    pub sort_order: i64,
    pub last_launched_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 所属速达分区 id（「全部」tab 的自定义分组，跨大类）；NULL = 未分区。
    /// 与 category（小类）是两个独立维度：小类归各大类的筛选 chips 用，分区只管「全部」的陈列。
    #[serde(default)]
    pub zone_id: Option<i64>,
    /// 用途说明（非敏感：这个网页/程序/文件是什么），编辑弹窗与卡片悬浮提示展示
    #[serde(default)]
    pub description: Option<String>,
    /// 备注（敏感：可存网页账号密码、文件解压密码等）。DPAPI 加密落盘（secret.rs，
    /// `dp1:` 前缀密文），内存与 IPC 中为明文；扩展桥载荷一律剥离本字段
    #[serde(default)]
    pub remark: Option<String>,
    /// 备注自定义标签（如「账号密码」「解压密码」）；NULL = 默认「备注」
    #[serde(default)]
    pub remark_label: Option<String>,
}

/// 速达小类（ADR 0012）：大类（resources.kind）下单归属的小类，单归属、非多选标签。
/// 各大类一套小类库，允许同名不同义；存量资源 category 为 NULL =「未归类」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSubcategory {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub sort_order: i64,
    pub is_default: bool,
}

/// 速达分区：「全部」tab 的自定义成组陈列（如「办公区」「影音区」），跨大类混居、
/// 无 kind 维度、无层级；成员经 resources.zone_id 关联。一个分区都没有时「全部」保持平铺。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceZone {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    /// 分区框宽度（卡片格数，吸附单元），新建默认 3
    #[serde(default = "default_zone_cols")]
    pub cols: i64,
    /// 分区框高度（卡片行数，下限——内容超出按行自动膨胀），新建默认 2
    #[serde(default = "default_zone_rows")]
    pub rows: i64,
}

fn default_zone_cols() -> i64 {
    3
}

fn default_zone_rows() -> i64 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
    /// 所在文件夹（NULL = 树根）；速记改造（docs/speednote-plan.md）新增
    #[serde(default)]
    pub folder_id: Option<i64>,
    /// 剪藏来源 URL（普通笔记为空串）；同 URL 落库即去重键
    #[serde(default)]
    pub source_url: String,
    /// 回收站标记（NULL = 正常；有值 = 已移入回收站）
    #[serde(default)]
    pub deleted_at: Option<String>,
    /// 自定义树图标（emoji，用户设置；None = 默认文件图标）
    #[serde(default)]
    pub icon: Option<String>,
}

/// 笔记文件夹（单归属、可嵌套；builtin=1 的「剪藏」不可改名/删除，ADR 0015）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteFolder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
    pub builtin: bool,
    pub created_at: String,
}

/// 回收站保留策略下的清理报告（purge_expired_notes）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeReport {
    pub purged: usize,
}

/// 双链出链项（to_note_id = None 表示「未链接提及」，引用了不存在的标题）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteLinkOut {
    pub to_title: String,
    pub to_note_id: Option<i64>,
}

/// 双链入链项（谁引用了我）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteLinkIn {
    pub from_note_id: i64,
    pub from_title: String,
}

/// 反链面板数据（get_note_links）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteLinks {
    pub outgoing: Vec<NoteLinkOut>,
    pub incoming: Vec<NoteLinkIn>,
}

/// 孤儿图片清理报告（先 dry_run 出报告，用户确认后再真删）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteImageGcReport {
    pub dry_run: bool,
    /// notes/images 下图片总数
    pub total_files: usize,
    /// 被活笔记（含回收站）引用的图片数
    pub referenced: usize,
    /// 孤儿图片文件名列表（hash.ext）
    pub orphan_files: Vec<String>,
    /// 真删模式下的实际删除数（dry_run = 0）
    pub removed: usize,
    /// 删除失败的文件数（被占用等）
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub resources: Vec<Resource>,
    pub notes: Vec<Note>,
    pub todos: Vec<Todo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
    pub priority: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    /// 截止时刻（毫秒时间戳）；无截止为 NULL
    #[serde(default)]
    pub due_at: Option<i64>,
    /// 提醒时刻（毫秒时间戳）；可独立于截止时间设置
    #[serde(default)]
    pub remind_at: Option<i64>,
    /// 提醒是否已触发（到点发过通知即置 1，防后台线程每秒重复提醒）
    #[serde(default)]
    pub remind_fired: bool,
    /// 父待办 id（子待办缩进挂在父条目下；顶级为 NULL）
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// 手动拖拽排序位（分组内按此升序）；NULL = 未手动排序，按创建时间倒序
    #[serde(default)]
    pub sort_order: Option<i64>,
    /// 乐观锁版本号（局域网同步冲突检测；写回时需携带期望 version，命中后 +1）
    #[serde(default)]
    pub version: i64,
    /// 轻量 Markdown 正文（勾选一律用子待办）
    #[serde(default)]
    pub description: String,
    /// 置顶：脱离日期分组，固定排在列表最顶部「置顶」区
    #[serde(default)]
    pub pinned: bool,
    /// 周期规则总开关：once / daily / weekly / monthly / yearly / weekdays / custom
    #[serde(default = "default_repeat_mode")]
    pub repeat_mode: String,
    /// custom：每 N 个 repeat_unit
    #[serde(default)]
    pub repeat_every: Option<i64>,
    /// custom：day / week / month / year
    #[serde(default)]
    pub repeat_unit: Option<String>,
    /// 位掩码 bit0=周一 … bit6=周日
    #[serde(default)]
    pub repeat_weekdays: Option<i64>,
    /// monthly：1..31，-1 = 月末
    #[serde(default)]
    pub repeat_month_day: Option<i64>,
    /// monthly：第几个（1..5，-1 = 最后一个）
    #[serde(default)]
    pub repeat_month_nth: Option<i64>,
    /// 结束条件：never / until / count
    #[serde(default)]
    pub repeat_end_mode: Option<String>,
    /// until：截止日期（毫秒时间戳）
    #[serde(default)]
    pub repeat_end_at: Option<i64>,
    /// count：共 N 次
    #[serde(default)]
    pub repeat_count: Option<i64>,
    /// 累计完成次数（统计用，不逐次留历史）
    #[serde(default)]
    pub repeat_done_count: i64,
    /// 上次完成时间
    #[serde(default)]
    pub repeat_last_done_at: Option<String>,
}

fn default_repeat_mode() -> String {
    "once".to_string()
}

/// 待办标签（定义表；与笔记标签 tags 无关，两套独立定义）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoTag {
    pub id: i64,
    pub name: String,
    /// '' = 用默认色；否则 #rrggbb
    pub color: String,
    pub sort_order: i64,
    pub created_at: String,
}

/// 周期待办在给定区间内的虚拟实例（日历渲染用；不落库）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoOccurrence {
    pub todo_id: i64,
    /// 实例时刻（毫秒时间戳）
    pub at_ms: i64,
}

/// 待办-标签关联对（前端构建筛选映射用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoTagLink {
    pub todo_id: i64,
    pub tag_id: i64,
}

/// 周期规则（`repeat_mode = once` 之外才有效）。
/// 与倒计时 countdowns 的做法一致：用有约束的列表达，不用自由文本（RRULE 只作导入导出格式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatRule {
    /// once / daily / weekly / monthly / yearly / weekdays / custom
    pub mode: String,
    pub every: Option<i64>,
    /// day / week / month / year
    pub unit: Option<String>,
    pub weekdays: Option<i64>,
    pub month_day: Option<i64>,
    pub month_nth: Option<i64>,
    /// never / until / count
    pub end_mode: Option<String>,
    pub end_at: Option<i64>,
    pub count: Option<i64>,
}

impl RepeatRule {
    pub fn from_todo(t: &Todo) -> Self {
        Self {
            mode: t.repeat_mode.clone(),
            every: t.repeat_every,
            unit: t.repeat_unit.clone(),
            weekdays: t.repeat_weekdays,
            month_day: t.repeat_month_day,
            month_nth: t.repeat_month_nth,
            end_mode: t.repeat_end_mode.clone(),
            end_at: t.repeat_end_at,
            count: t.repeat_count,
        }
    }

    pub fn is_once(&self) -> bool {
        self.mode == "once"
    }

    /// 只保留当前 mode 用得上的列（与 countdowns.interval_minutes 仅 interval 模式有值同理），
    /// 避免改模式后残留旧列被后续读取误判。
    pub fn normalize(&mut self) {
        let keep_every = self.mode == "custom";
        let keep_unit = self.mode == "custom";
        let keep_weekdays = self.mode == "weekly"
            || (self.mode == "custom" && self.unit.as_deref() == Some("week"))
            || (self.mode == "monthly" && self.month_nth.is_some());
        let keep_month_day = self.mode == "monthly" && self.month_nth.is_none();
        let keep_month_nth = self.mode == "monthly" && self.month_nth.is_some();
        if !keep_every {
            self.every = None;
        }
        if !keep_unit {
            self.unit = None;
        }
        if !keep_weekdays {
            self.weekdays = None;
        }
        if !keep_month_day {
            self.month_day = None;
        }
        if !keep_month_nth {
            self.month_nth = None;
        }
        if self.mode == "once" {
            self.end_mode = None;
            self.end_at = None;
            self.count = None;
        }
    }

    /// 校验（宿主命令与扩展桥共用）：非法取值 fail-fast，避免静默降级成别的周期。
    pub fn validate(&self) -> Result<(), String> {
        const MODES: [&str; 7] = ["once", "daily", "weekly", "monthly", "yearly", "weekdays", "custom"];
        if !MODES.contains(&self.mode.as_str()) {
            return Err(format!("INVALID_ARGUMENT: repeat.mode 取值非法: {}", self.mode));
        }
        if let Some(unit) = self.unit.as_deref() {
            if !["day", "week", "month", "year"].contains(&unit) {
                return Err(format!("INVALID_ARGUMENT: repeat.unit 取值非法: {unit}"));
            }
        }
        if let Some(end_mode) = self.end_mode.as_deref() {
            if !["never", "until", "count"].contains(&end_mode) {
                return Err(format!("INVALID_ARGUMENT: repeat.endMode 取值非法: {end_mode}"));
            }
            if end_mode == "until" && self.end_at.is_none() {
                return Err("INVALID_ARGUMENT: endMode=until 需要 endAt".to_string());
            }
            if end_mode == "count" && self.count.unwrap_or(0) <= 0 {
                return Err("INVALID_ARGUMENT: endMode=count 需要 count > 0".to_string());
            }
        }
        // 数值列范围校验。扩展是不可信输入源，而非法值在引擎里会静默改变语义：
        // monthNth=0 算不出锚点 → 规则被当成「已用尽」→ 周期静默转一次性；
        // weekdays 越界（如 1<<40）会被 weekday_from_index 兜底成周日。
        // every 上限同时防 chrono 的 Duration::days 溢出 panic（极大间隔的 custom）。
        if let Some(day) = self.month_day {
            if day != -1 && !(1..=31).contains(&day) {
                return Err(format!(
                    "INVALID_ARGUMENT: repeat.monthDay 取值非法: {day}（1..31 或 -1）"
                ));
            }
        }
        if let Some(nth) = self.month_nth {
            if nth != -1 && !(1..=5).contains(&nth) {
                return Err(format!(
                    "INVALID_ARGUMENT: repeat.monthNth 取值非法: {nth}（1..5 或 -1）"
                ));
            }
        }
        if let Some(mask) = self.weekdays {
            if !(1..=0b111_1111).contains(&mask) {
                return Err(format!(
                    "INVALID_ARGUMENT: repeat.weekdays 位掩码非法: {mask}（bit0=周一 … bit6=周日）"
                ));
            }
        }
        if let Some(every) = self.every {
            if !(1..=999).contains(&every) {
                return Err(format!(
                    "INVALID_ARGUMENT: repeat.every 取值非法: {every}（1..999）"
                ));
            }
        }
        if self.mode == "custom" {
            if self.every.unwrap_or(0) <= 0 {
                return Err("INVALID_ARGUMENT: custom 需要 every > 0".to_string());
            }
            let unit = self.unit.as_deref().unwrap_or("day");
            if unit == "week" && self.weekdays.unwrap_or(0) == 0 {
                return Err("INVALID_ARGUMENT: custom+week 需要选择星期几".to_string());
            }
        }
        if self.mode == "weekly" && self.weekdays.unwrap_or(0) == 0 {
            return Err("INVALID_ARGUMENT: weekly 需要选择星期几".to_string());
        }
        if self.mode == "monthly" {
            if self.month_day.is_none() && self.month_nth.is_none() {
                return Err(
                    "INVALID_ARGUMENT: monthly 需要指定每月第几天或第几个星期几".to_string()
                );
            }
            // 「第几个星期几」必须带掩码，否则 month_anchor 取不到星期几 → 规则静默用尽
            if self.month_nth.is_some() && self.weekdays.unwrap_or(0) == 0 {
                return Err(
                    "INVALID_ARGUMENT: monthly 的「第几个星期几」需要 weekdays".to_string()
                );
            }
        }
        Ok(())
    }
}

/// 便签（工作台左上，slot 1/2 两张卡，每卡一条多行文本）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sticky {
    pub id: i64,
    pub slot: i64,
    pub content: String,
    /// 乐观锁版本号
    #[serde(default)]
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 脱离为系统级浮窗的便签（slot 对应来源卡片，一卡最多一个浮窗）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedSticky {
    pub id: i64,
    pub slot: i64,
    pub content: String,
    /// 乐观锁版本号
    #[serde(default)]
    pub version: i64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub always_on_top: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 提示词百宝箱单条（可置顶、统计复制次数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub is_pinned: bool,
    pub copy_count: i64,
    pub last_copied_at: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 剪贴板历史单条（文本 / 图片 / 文件三类型；html 为可选富文本片段，粘贴时优先还原格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    /// 富文本 HTML 片段（如浏览器复制时携带）；无则空
    pub html: Option<String>,
    /// 来源应用（记录时取前台窗口所属进程名）
    pub source_app: Option<String>,
    pub is_pinned: bool,
    /// 条目类型：text / image / file
    pub kind: String,
    /// 图片快照文件路径（kind=image 时非空，位于 app_data_dir/clipboard/images/）
    pub image_path: Option<String>,
    /// 文件路径列表（kind=file 时非空）
    pub file_paths: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 笔记标签（builtin=1 的「剪藏」标签不可改名/删除）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    #[serde(default)]
    pub builtin: bool,
}

/// 倒计时（三种形态统一建模）：
/// - once    一次性：end_at 为绝对时刻，到点置 finished，卡片灰态待删
/// - daily   每天固定时刻：end_at 为当天/次日 HH:MM 时刻，到点顺延 24h
/// - interval 每隔 N 分钟：end_at 为当前轮结束时刻，到点按 interval_minutes 顺延
/// total_ms 为周期总长（once 创建时长 / daily 24h / interval N 分钟），用于水位进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Countdown {
    pub id: i64,
    pub name: String,
    pub repeat_mode: String,
    pub end_at: i64,
    pub total_ms: i64,
    pub interval_minutes: Option<i64>,
    pub paused: bool,
    pub paused_remaining_ms: Option<i64>,
    pub finished: bool,
    pub floated: bool,
    pub float_x: Option<f64>,
    pub float_y: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

/// AI 对话会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: i64,
    pub title: String,
    pub model_name: String,
    pub created_at: String,
    pub updated_at: String,
    /// 会话级累计 token（输入 / 输出 / 缓存读取 / 推理）
    #[serde(default)]
    pub tokens_input: i64,
    #[serde(default)]
    pub tokens_output: i64,
    #[serde(default)]
    pub tokens_cache_read: i64,
    #[serde(default)]
    pub tokens_reasoning: i64,
    /// 会话级累计生成耗时（毫秒），用于计算 TPS
    #[serde(default)]
    pub elapsed_ms: i64,
}

/// AI 对话消息（user / assistant）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// 自定义模型配置（不绑定厂商，统一 OpenAI 兼容协议）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatModelConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// 仅保存时携带；读取/落盘时一律清空（真实 Key 存系统钥匙串）
    pub api_key: String,
    pub is_default: bool,
    /// 是否已配置 API Key（返回给前端做状态展示，保存时忽略）
    #[serde(default)]
    pub has_api_key: bool,
    /// 供应商名称（如 DeepSeek / OpenAI），同一 base_url 下的模型归为一组
    #[serde(default)]
    pub provider_name: String,
}
