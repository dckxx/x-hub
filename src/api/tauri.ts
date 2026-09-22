import { Channel, invoke } from '@tauri-apps/api/core'

export interface Resource {
  id: number
  kind: 'app' | 'web' | 'file'
  name: string
  target: string
  category: string | null
  icon: string | null
  args: string | null
  sort_order: number
  last_launched_at: string | null
  created_at: string
  updated_at: string
}

/** 本机已安装浏览器（list_installed_browsers，注册表 StartMenuInternet 枚举） */
export interface InstalledBrowser {
  /** 显示名（如 Google Chrome / Microsoft Edge） */
  name: string
  /** 浏览器 exe 绝对路径 */
  exe: string
}

export interface Note {
  id: number
  title: string
  content: string
  created_at: string
  updated_at: string
}

export interface Todo {
  id: number
  title: string
  done: boolean
  priority: number
  created_at: string
  updated_at: string
  completed_at: string | null
  /** 截止时刻（毫秒时间戳），无截止为 null */
  due_at: number | null
  /** 提醒时刻（毫秒时间戳），可独立于截止时间设置 */
  remind_at: number | null
  /** 提醒是否已触发（后台到点发过通知即置真） */
  remind_fired: boolean
  /** 父待办 id（子待办），顶级为 null */
  parent_id: number | null
  /** 手动拖拽排序位（分组内升序）；null = 未手动排序，按创建时间倒序 */
  sort_order: number | null
  /** 轻量 Markdown 正文（勾选一律用子待办） */
  description: string
  /** 置顶：脱离日期分组，固定排在列表最顶部「置顶」区 */
  pinned: boolean
  /** 周期规则总开关：once / daily / weekly / monthly / yearly / weekdays / custom */
  repeat_mode: RepeatMode
  repeat_every: number | null
  /** custom：day / week / month / year */
  repeat_unit: RepeatUnit | null
  /** 位掩码 bit0=周一 … bit6=周日 */
  repeat_weekdays: number | null
  /** monthly：1..31，-1 = 月末 */
  repeat_month_day: number | null
  /** monthly：第几个（1..5，-1 = 最后一个） */
  repeat_month_nth: number | null
  /** never / until / count */
  repeat_end_mode: RepeatEndMode | null
  /** until：截止日期（毫秒时间戳） */
  repeat_end_at: number | null
  /** count：共 N 次 */
  repeat_count: number | null
  /** 累计完成次数（统计用，不逐次留历史） */
  repeat_done_count: number
  repeat_last_done_at: string | null
}

export type RepeatMode = 'once' | 'daily' | 'weekly' | 'monthly' | 'yearly' | 'weekdays' | 'custom'
export type RepeatUnit = 'day' | 'week' | 'month' | 'year'
export type RepeatEndMode = 'never' | 'until' | 'count'

/** 周期规则（写入用；与后端 RepeatRule 对齐） */
export interface RepeatRuleInput {
  mode: RepeatMode
  every: number | null
  unit: RepeatUnit | null
  weekdays: number | null
  month_day: number | null
  month_nth: number | null
  end_mode: RepeatEndMode | null
  end_at: number | null
  count: number | null
}

/** 待办标签（与笔记标签是两套独立定义） */
export interface TodoTag {
  id: number
  name: string
  /** '' = 用默认色；否则 #rrggbb */
  color: string
  sort_order: number
  created_at: string
}

/** 待办-标签关联对 */
export interface TodoTagLink {
  todo_id: number
  tag_id: number
}

/** 周期待办的虚拟实例（不落库，仅日历渲染） */
export interface TodoOccurrence {
  todo_id: number
  at_ms: number
}

export interface Sticky {
  id: number
  slot: number
  content: string
  created_at: string
  updated_at: string
}

export interface DetachedSticky {
  id: number
  slot: number
  content: string
  x: number | null
  y: number | null
  always_on_top: boolean
  created_at: string
  updated_at: string
}

export interface Snippet {
  id: number
  title: string
  content: string
  is_pinned: boolean
  copy_count: number
  last_copied_at: string
  created_at: string
  updated_at: string
}

export interface ClipboardItem {
  id: number
  content: string
  /** 富文本 HTML 片段（粘贴时优先还原格式） */
  html: string | null
  /** 来源应用 */
  source_app: string | null
  is_pinned: boolean
  /** 条目类型：text / image / file */
  kind: 'text' | 'image' | 'file'
  /** 图片快照文件路径（kind=image 时非空） */
  image_path: string | null
  /** 文件路径列表（kind=file 时非空） */
  file_paths: string[]
  created_at: string
  updated_at: string
}

export interface ClipboardInfo {
  paused: boolean
  max_items: number
  ttl_days: number
  total: number
  shortcut: string
}

export interface WindowState {
  width: number
  height: number
  x: number | null
  y: number | null
  always_on_top: boolean
}

/**
 * 应用配置（与 Rust `AppConfig` 对应的前端类型；**少数字段故意缺席**）。
 *
 * ⚠️ 判据：**这个类型里有没有某个字段，都不代表前端说了算**。凡是在后端
 * `config.rs::BACKEND_MANAGED_FIELDS` 里登记的字段，一律以磁盘为准（合并实现见
 * `config.rs::merge_disk_authoritative`，三条回归测试守着）。两种情形**都会**覆盖磁盘：
 *   - 前端认识它（如 `chat_models`、`chat_window_*`、`floating_ball_*`、`skipped_update_version`）
 *     → 提交时带的是**启动快照**里的旧值；
 *   - 前端不认识它（如 `dev_extensions`、`dev_mode_enabled`、`skill_roots`）→ 序列化后整份提交，
 *     反序列化时按 `AppConfig::default()` 的**同名字段值**补缺（容器级 `#[serde(default)]`），
 *     照样把磁盘值冲掉（`skill_roots` 那次事故就是这么发生的）。
 * 所以「不放进这个类型」**不是**保护手段——唯一的保护是「合并 + 登记 + 测试」这三件套。
 *
 * 新增「只由后端命令写盘」的字段时：连同 `merge_disk_authoritative`、`BACKEND_MANAGED_FIELDS`
 * 一起改（清单见 `AGENTS.md` 的配置约定）；确实需要前端读写某个字段，也请连同合并逻辑一起改。
 */
export interface AppConfig {
  theme_mode: string // 'light' | 'dark' | 'system'
  theme_preset: string // 'indigo' | 'green' | 'morandi' | 'midnight'
  accent_color: string | null // hex like '#5b5bf5'; null = follow preset recommended
  /** 应用壁纸绝对路径（空 = 未设置，回退主题渐变背景） */
  wallpaper_path: string
  /** 壁纸整屏静态模糊（默认开启，见 ADR 0002） */
  wallpaper_blur: boolean
  /** 壁纸蒙版：主题底色罩层不透明度（0–0.85，默认 0.3） */
  wallpaper_veil: number
  /** 沉浸模式：卡片真毛玻璃局部取景模糊（ADR 0003 受控例外，默认关） */
  wallpaper_immersive: boolean
  /** 卡片玻璃透明度（0.4–1.0，默认 1.0 = 不透明） */
  glass_opacity: number
  sidebar_toggle: boolean // 侧边栏展开/收缩功能开关（默认关闭）
  window: WindowState
  global_shortcut: string
  dashboard_mid_content: string
  /** 工作台自定义布局（placements JSON 数组字符串；空串 = 未自定义，回退推荐布局） */
  dashboard_layout: string
  countdown_sound: boolean
  clock_quote: string // 时钟卡片语录（可配置，空串回退默认）
  online_enabled: boolean // 联网功能总开关（默认开）
  weather_city: string // 天气城市展示名（空串 = 未配置）
  weather_lat: number // 天气纬度缓存
  weather_lng: number // 天气经度缓存
  quote_source: string // 名言来源：online / local
  chat_models: ChatModelConfig[]
  chat_panel_width: number
  chat_panel_open: boolean
  /** AI 对话面板透明度（0.5–1.0，设置中可调） */
  chat_panel_opacity: number
  /** AI 对话面板方位：left / right / top / bottom */
  chat_panel_side: string
  /** AI 对话面板在顶部/底部方位时的高度（px） */
  chat_panel_height: number
  /** AI 对话是否以独立窗口打开（false = 主窗内嵌抽屉） */
  chat_window_mode: boolean
  /** AI 对话独立窗口尺寸（逻辑 px，由后端拖拽/缩放记忆） */
  chat_window_width: number
  chat_window_height: number
  /** AI 对话独立窗口位置（物理 px，由后端记忆） */
  chat_window_x: number | null
  chat_window_y: number | null
  /** AI 对话独立窗口是否置顶 */
  chat_window_pinned: boolean
  /** 剪贴板历史全局呼出快捷键 */
  clipboard_shortcut: string
  /** 剪贴板历史最大条数（含置顶） */
  clipboard_max_items: number
  /** 非置顶记录保留天数 */
  clipboard_ttl_days: number
  /** 暂停剪贴板记录 */
  clipboard_paused: boolean
  /** 粘贴快捷键方式：auto / ctrl_v / ctrl_shift_v / shift_insert */
  clipboard_paste_method: string
  /** 记录剪贴板图片（默认开启） */
  clipboard_image_enabled: boolean
  /** 记录剪贴板文件（默认开启） */
  clipboard_file_enabled: boolean
  /** 全局字体缩放系数（0.85–1.30，默认 1.0） */
  font_scale: number
  /** 便签模块字体缩放系数（相对全局的额外缩放，默认 1.0） */
  font_sticky: number
  /** 速记模块字体缩放系数 */
  font_notes: number
  /** 提示词模块字体缩放系数 */
  font_prompt: number
  /** 待办模块字体缩放系数 */
  font_todo: number
  /** 速记编辑器模式：wysiwyg（实时预览）/ split（分屏预览）/ source（源码） */
  note_editor_mode: string
  /** service 扩展运行时策略：auto / builtin / system */
  runtime_strategy: string
  /** 固定到左侧栏的扩展 id 列表（点击侧栏菜单即在主区打开对应扩展） */
  sidebar_extensions: string[]
  /** 扩展「默认打开方式」映射：extId → view / window / drawer（未设置时侧栏点击默认 view） */
  extension_open_modes: Record<string, string>
  /** 开机自启动（登录 Windows 时自动驻留托盘） */
  run_at_startup: boolean
  /** 自动升级总开关（默认开启） */
  auto_update_enabled: boolean
  /** 静默检查更新频率（小时，默认 4） */
  update_interval_hours: number
  /** 用户「跳过此版本」记录的版本号（空 = 未跳过） */
  skipped_update_version: string
  /** 桌面悬浮球总开关（ADR 0004，默认开启）：主窗口隐藏时在桌面显示悬浮球 */
  floating_ball_enabled: boolean
  /** 悬浮球贴边自动隐藏（拖到屏幕边缘附近松手 → 半隐只露一半，悬停完整露出） */
  floating_ball_auto_hide: boolean
  /** 与主窗口同时显示：主窗可见时球保持常驻（默认 false = 仅主窗隐藏/最小化时出现） */
  floating_ball_with_main: boolean
  /** 环形快捷菜单按钮 id 列表（view:xxx / act:xxx，去重后最多 8 个） */
  floating_ball_buttons: string[]
  /** 悬浮球窗口位置（物理 px，拖拽后由后端记忆） */
  floating_ball_x: number | null
  floating_ball_y: number | null
  /** 悬浮球静止态保持转动（炫酷模式，默认关）：开启 = 陀螺环常转 + canvas 满帧（旧版行为，更耗电） */
  floating_ball_idle_spin: boolean
}

export interface AppInfo {
  version: string
  changelog: string
  latest_section: string
}

/** AI 对话独立窗口状态（Rust chat_window::chat_window_get_state） */
export interface ChatWindowState {
  /** 是否采用「独立窗口」形态（与主窗内嵌抽屉互斥） */
  mode: boolean
  /** 是否置顶 */
  pinned: boolean
  /** 当前是否可见 */
  visible: boolean
}

/** 悬浮球停靠边（球心落在该侧屏幕/工作区边缘，球体一半藏屏外） */
export interface BallDock {
  left: boolean
  right: boolean
  top: boolean
  bottom: boolean
}

/** 悬浮球状态（Rust floating_ball::get_state） */
export interface FloatingBallState {
  enabled: boolean
  /** 贴边自动隐藏开关（取代旧「贴边吸附」） */
  auto_hide: boolean
  /** 与主窗口同时显示（主窗可见时球保持常驻） */
  with_main: boolean
  buttons: string[]
  /** 记忆的球心位置（物理 px，拖拽后由后端记忆） */
  x: number | null
  y: number | null
  /** 静止态保持转动（炫酷模式）：前端据此跳过 rings-idle 暂停与 24fps 降帧 */
  idle_spin: boolean
  /** 当前停靠边：前端据此做「悬停滑出露全」的 CSS 平移 */
  dock: BallDock
  /** 球态窗口逻辑边长（前端 resize 失配自检的期望值之一） */
  ball_size: number
  menu_size: number
}

/** 主题配置（悬浮球等独立窗口自取：主窗 useTheme 推送之外的初始值来源） */
export interface ThemeConfig {
  mode: string
  preset: string
  accent: string | null
}

/** 已安装扩展的注册表项（后端 extension.rs 扫描返回） */
export interface ExtensionEntry {
  id: string
  name: string
  version: string
  /** web | service */
  runtime: 'web' | 'service'
  /** module | view | window | drawer */
  kind: string
  surfaces: string[]
  open_in: string[]
  permissions: string[]
  description: string
  /** 图标文件绝对路径（存在时才非空） */
  icon: string | null
  /** 扩展目录绝对路径 */
  dir: string
  /** 来源：installed（已装，位于扩展根）| dev（「我的扩展」直挂的本机源码目录） */
  source: 'installed' | 'dev'
  /** manifest 缺失 / 解析失败时为 true */
  invalid: boolean
  error: string | null
  /** 条件禁用求值结果（manifest.disabled 命中） */
  disabled: boolean
  /** 缺失的宿主能力（manifest.requires 中宿主未实现的） */
  missing_capabilities: string[]
  /** 缺失的依赖扩展 id（manifest.dependsOn 中未安装的） */
  missing_dependencies: string[]
  /** 扩展声明的依赖扩展 id（manifest.dependsOn） */
  depends_on: string[]
  /** 暴露给其它扩展调用的方法名（manifest.expose） */
  expose: string[]
  /** 快捷动作（manifest.actions，能力注入） */
  actions: { id: string; title: string; surface: string }[]
  /** 工作台模块形态声明（manifest.moduleVariants；module 形态多形态注册，空 = 单个默认形态） */
  module_variants: ExtensionModuleVariant[]
  /** 工作台模块选项（manifest.moduleOptions；module 卡片表头默认显隐） */
  module_options: ExtensionModuleOptions
}

/** 工作台模块选项（后端 extension.rs::ModuleOptions） */
export interface ExtensionModuleOptions {
  /**
   * 作者对 module 卡片宿主表头的声明：
   * `null`/缺省 = 未声明（默认不显示表头）；`false` = 默认显示；`true` = 默认不显示。
   * 用户仍可在布局编辑器里按卡片覆盖。
   */
  default_hide_title: boolean | null
}

/** 「我的扩展」状态（后端 extension.rs::DevModeStatus） */
export interface DevModeStatus {
  /** ⚠️ 兼容字段，恒为 true：v0.6.x 起没有开发者模式开关，登记即加载 */
  enabled: boolean
  extensions: DevExtensionInfo[]
}

/** 单个本机源码目录的解析结果 */
export interface DevExtensionInfo {
  /** 注册的源码目录绝对路径 */
  path: string
  id: string
  name: string
  version: string
  /** manifest 是否可解析 */
  valid: boolean
  /** valid=false 时的原因 */
  error: string | null
  /** 与已装扩展同 id（此时不会被加载，已装优先） */
  conflict: boolean
  /** 目录当前是否存在 */
  exists: boolean
}

/** 内置技能包元信息（后端 skills.rs::SkillInfo） */
export interface SkillInfo {
  id: string
  name: string
  description: string
  /** 安装到目标 skills 根下的子目录名 */
  dir_name: string
  /** 文件数 */
  file_count: number
  /** 原始字节数 */
  size: number
  /** 内容哈希（sha256 前 16 位；展示与「可更新」判定） */
  hash: string
  /** 随客户端版本 */
  app_version: string
}

/** 一个 skills 安装目标（= 某助手的 skills 根目录） */
export interface SkillTarget {
  /** 展示名（Claude Code / DSH / Codex / 自定义目录） */
  label: string
  /** skills 根目录绝对路径 */
  path: string
  /** auto = 自动探测的已知助手目录；custom = 用户添加的自定义目录 */
  kind: 'auto' | 'custom'
  /** 目标下已存在本技能目录 */
  installed: boolean
  /** 已安装且内容与内置一致（无需更新） */
  up_to_date: boolean
  /** 目录存在但不是本客户端装的（无标记文件）：覆盖前需二次确认 */
  foreign: boolean
  /** 标记里记录的安装版本（无标记时为 null） */
  installed_version: string | null
}

/** 技能总览（get_skill_overview 返回） */
export interface SkillOverview {
  skill: SkillInfo
  targets: SkillTarget[]
}

/** 发布前本地预检结果（level: ok/warn/error；clean = 无 error） */
export interface PrecheckResult {
  clean: boolean
  items: { level: 'ok' | 'warn' | 'error'; label: string; detail?: string }[]
}

/** 一台在线设备（多设备登录；每台一条 token，可单独撤销） */
export interface AccountDevice {
  id: number
  label: string
  created_at: number
  last_seen_at: number
  /** 是不是本机（当前正在用的这枚 token）：本机不能「撤销」，否则等于把自己踢下线 */
  current?: boolean
}

/** 发布提交结果（关卡逐项结论；客户端只展示服务端结论，不内置任何审核规则） */
export interface SubmitResult {
  id: number
  /** pending_review / gate_failed */
  status: string
  gatePassed: boolean
  gateItems: { id: string; label: string; ok: boolean; detail?: string | null }[]
  extId: string
  version: string
  /** 剩余配额（服务端只回剩余次数，不回上限） */
  quota: { drafts_remaining?: number; published_remaining?: number; daily_submits_remaining?: number } | null
}

/** 我的一条提交记录 */
export interface DevSubmissionRow {
  id: number
  ext_id: string
  version: string
  runtime: string
  status: string
  review_note: string
  size: number
  created_at: number
  reviewed_at: number | null
  has_ai_report: number
}

/** 提交详情（含关卡逐项结论；**不含** AI 预审报告——那是给审核者看的） */
export interface DevSubmissionDetail extends DevSubmissionRow {
  market: { changelog?: string; minAppVersion?: string; homepage?: string }
  permissions: string[]
  gate_report: { id: string; label: string; ok: boolean; detail?: string | null }[]
}

export interface DevSubmissionList {
  submissions: DevSubmissionRow[]
  total: number
  quota: { drafts_remaining?: number; published_remaining?: number; daily_submits_remaining?: number }
}

/** 平台账号状态（后端 account.rs::AccountStatus） */
export interface AccountStatus {
  /** 是否已登录（本地存在 token） */
  loggedIn: boolean
  /** 服务端地址（空 = 未配置） */
  serverUrl: string
  username: string
  role: string
  quotaTotal: number
  quotaRemaining: number
  /** 已兑换邀请码（= 有权益：额度 + 可申请开发者） */
  inviteRedeemed: boolean
  /** none / pending / approved / rejected */
  developerStatus: 'none' | 'pending' | 'approved' | 'rejected'
  canApplyDeveloper: boolean
  /** 已登录但拉取失败时的原因（网络不可用等） */
  error: string | null
}

/** GitHub 设备码登录：发起结果 */
export interface GithubDeviceStart {
  pollId: string
  userCode: string
  verificationUri: string
  interval: number
  expiresIn: number
}

/** GitHub 设备码登录：轮询结果 */
export interface GithubPollResult {
  /**
   * pending = 还没授权；slow_down = GitHub 要求降低轮询频率（必须把间隔 +5s 再继续，
   * 否则它会一直回 slow_down —— 表现为「浏览器已授权、客户端永远等待」）；ok = 登录成功
   */
  status: 'pending' | 'slow_down' | 'ok' | 'error'
  message: string | null
}

/** 邮箱验证码发送结果（服务端未配置发信时 ok=false + 说明） */
export interface EmailSendResult {
  ok: boolean
  message: string | null
}

/** 开发者申请状态 */
export interface DevApplyStatus {
  /** none / pending / approved / rejected */
  status: string
  reviewNote: string
  isDeveloper: boolean
  inviteRedeemed: boolean
}

/** manifest.moduleVariants 里的单个形态声明（与 Rust ModuleVariant 对齐） */
export interface ExtensionModuleVariant {  id: string
  name: string
  minW: number
  minH: number
  idealW: number
  idealH: number
}

/** 市场清单里的一条扩展（v2：R2 远端清单格式） */
export interface MarketExtension {
  id: string
  name: string
  version: string
  description: string
  runtime: string
  author: string
  /** 下载地址（zip 包，R2 公开 URL） */
  downloadUrl: string
  /** zip 包 sha256（hex 小写），下载后校验 */
  sha256: string
  /** zip 包字节大小（0 = 未知） */
  size: number
  /** 市场卡片图标 URL（https，直接 <img> 加载） */
  icon: string
  /** 宿主最低版本门槛（如 "0.3.0"） */
  minAppVersion: string
  /** 本版本更新说明 */
  changelog: string
  /** 项目主页 */
  homepage: string
  /** 官方内置扩展标记 */
  required: boolean
  /** 截图（展示物料，完整 URL；老清单没有此字段 = 空数组 → 详情页显示「作者未提供截图」） */
  screenshots: string[]
  /** 该扩展申请的权限（发布时由服务端从 manifest 写入；老清单缺此字段 = 未提供） */
  permissions?: string[]
}

/** 市场状态（get_market_registry / refresh_market_registry 返回） */
export interface MarketStatus {
  extensions: MarketExtension[]
  /** 清单更新时间（远端 updatedAt 透传） */
  last_updated: string
  /** remote（刷新成功）/ cache（离线或验签失败回退） */
  source: 'remote' | 'cache'
  /** 拉取/验签失败原因（source=cache 时非空） */
  error: string | null
  /** 撤销列表（`id@version`）：已装扩展命中则警示并停止自动更新（不静默卸载/禁用） */
  revoked: string[]
}

/** 扩展打包结果（pack_extension_archive 返回） */
export interface PackedArchive {
  /** 产物绝对路径（.xhpack，zip 格式） */
  path: string
  id: string
  version: string
  size: number
  /** 产物 sha256（hex 小写） */
  sha256: string
}

/** 市场下载进度事件负载（market-download-progress） */
export interface MarketDownloadProgress {
  id: string
  received: number
  total: number | null
}

/** 应用更新信息（check_for_update / download_update / get_update_status / update-available 负载） */
export interface UpdateInfo {
  /** 是否有可用更新（已命中版本且未下载） */
  available: boolean
  /** 目标版本号（空 = 无目标版本） */
  version: string
  /** 更新说明摘要 */
  notes: string
  /** 本次更新 zip 大小（0 = 未知） */
  size: number
  /** 该更新是否为便携版专属 */
  portable: boolean
  /** 是否已就绪待重启应用（下载完成并写好标记） */
  ready: boolean
  /** 当前应用的版本号 */
  current: string
}

export interface DataPathInfo {
  /** 当前数据根绝对路径 */
  path: string
  /** default（默认 %APPDATA% 路径）/ custom（用户自定义）/ portable（便携模式，跟随程序目录） */
  mode: 'default' | 'custom' | 'portable'
}

/** 开机自启动真实状态：区分「用户意图」与「系统里是否真的会生效」 */
export interface AutostartStatus {
  /** 是否真正会开机自启 = configured && registered && !os_disabled */
  enabled: boolean
  /** 用户开关意图（config.run_at_startup） */
  configured: boolean
  /** Run 键存在且指向当前 exe */
  registered: boolean
  /** 被任务管理器/安全软件在启动项里禁用 */
  os_disabled: boolean
}

export interface WeatherCurrent {
  temperature: number
  apparent_temperature: number
  relative_humidity: number
  wind_speed: number
  weather_code: number
  city: string
}

export interface Quote {
  content: string
  from: string
}

export interface GeoLocation {
  name: string
  lat: number
  lng: number
}

export interface ClientErrorPayload {
  message: string
  detail: string | null
}

export interface Tag {
  id: number
  name: string
  created_at: string
}

export interface InitialData {
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
  stickies: Sticky[]
  detached: DetachedSticky[]
  countdowns: Countdown[]
  tags: Tag[]
  config: AppConfig
}

export interface SearchResult {
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
}

export interface NoteTagRow {
  note_id: number
  tag_id: number
}

export interface Countdown {
  id: number
  name: string
  /** once / daily / interval */
  repeat_mode: string
  /** 下一次到点时刻（毫秒时间戳） */
  end_at: number
  /** 周期总长（毫秒），用于水位进度 */
  total_ms: number
  interval_minutes: number | null
  paused: boolean
  paused_remaining_ms: number | null
  finished: boolean
  floated: boolean
  float_x: number | null
  float_y: number | null
  created_at: string
  updated_at: string
}

export interface DroppedAppInfo {
  name: string
  target: string
  icon: string | null
}

export interface InstalledAppInfo {
  name: string
  target: string
  icon: string | null
}

export interface SystemInfo {
  cpuUsage: number
  memUsedMb: number
  memTotalMb: number
  memPercent: number
}

export interface ChatSession {
  id: number
  title: string
  model_name: string
  created_at: string
  updated_at: string
  /** 会话级累计 token（输入 / 输出 / 缓存读取 / 推理） */
  tokens_input: number
  tokens_output: number
  tokens_cache_read: number
  tokens_reasoning: number
  /** 会话级累计生成耗时（毫秒），用于计算 TPS */
  elapsed_ms: number
}

export interface ChatMessage {
  id: number
  session_id: number
  role: 'user' | 'assistant'
  content: string
  created_at: string
}

export interface ChatModelConfig {
  id: string
  name: string
  base_url: string
  model: string
  api_key: string
  is_default: boolean
  has_api_key: boolean
  provider_name?: string
}

/** 平台免费额度在界面上的单一入口名（与后端 `chat.rs::PLATFORM_ENTRY_NAME` 是同一个值，改动需两端同步） */
export const PLATFORM_ENTRY_NAME = 'x-hub 平台'

/** 是否平台额度条目（写入时定死 `platform:` 前缀；旧条目可能只有 provider_name 标识，一并认） */
export function isPlatformModel(m: ChatModelConfig): boolean {
  return m.id.startsWith('platform:') || (m.provider_name ?? '').trim() === PLATFORM_ENTRY_NAME
}

export type ChatStreamEvent =
  | { type: 'chunk'; content: string }
  | { type: 'done'; message: ChatMessage; session: ChatSession }
  | { type: 'error'; message: string; partial: string }

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const tauriApi = {
  getInitialData: () => invoke<InitialData>('get_initial_data'),
  /** 轻量配置读取（AI 对话独立窗唤起用）：只拉 config，不拉九类业务数据 */
  getUiConfig: () => invoke<AppConfig>('get_ui_config'),
  createResource: (payload: {
    kind: 'app' | 'web' | 'file'
    name: string
    target: string
    category?: string | null
    icon?: string | null
    args?: string | null
  }) => invoke<Resource>('create_resource', {
    kind: payload.kind,
    name: payload.name,
    target: payload.target,
    category: payload.category ?? null,
    icon: payload.icon ?? null,
    args: payload.args ?? null,
  }),
  updateResource: (payload: {
    id: number
    kind: 'app' | 'web' | 'file'
    name: string
    target: string
    category?: string | null
    icon?: string | null
    args?: string | null
  }) => invoke<Resource>('update_resource', {
    id: payload.id,
    kind: payload.kind,
    name: payload.name,
    target: payload.target,
    category: payload.category ?? null,
    icon: payload.icon ?? null,
    args: payload.args ?? null,
  }),
  deleteResource: (id: number) => invoke<void>('delete_resource', { id }),
  reorderResources: (ids: number[]) => invoke<void>('reorder_resources', { ids }),
  launchResource: (id: number) => invoke<void>('launch_resource', { id }),
  listInstalledBrowsers: () => invoke<InstalledBrowser[]>('list_installed_browsers'),
  openUrlWithBrowser: (id: number, browserExe: string) =>
    invoke<void>('open_url_with_browser', { id, browserExe }),
  createNote: (title: string) => invoke<Note>('create_note', { title }),
  updateNote: (id: number, title: string, content: string) =>
    invoke<Note>('update_note', { id, title, content }),
  deleteNote: (id: number) => invoke<void>('delete_note', { id }),
  listNotes: () => invoke<Note[]>('list_notes'),
  searchAll: (keyword: string) => invoke<SearchResult>('search_all', { keyword }),
  listTodos: () => invoke<Todo[]>('list_todos'),
  createTodo: (title: string, parentId?: number | null, createdAt?: string) =>
    invoke<Todo>('create_todo', {
      title,
      parentId: parentId ?? null,
      // 撤销恢复时保留原创建时间，避免恢复项排到最新位置
      createdAt: createdAt ?? null,
    }),
  toggleTodo: (id: number) => invoke<Todo>('toggle_todo', { id }),
  updateTodo: (id: number, title: string, priority: number) =>
    invoke<Todo>('update_todo', { id, title, priority }),
  deleteTodo: (id: number) => invoke<void>('delete_todo', { id }),
  scheduleTodo: (id: number, dueAt: number | null, remindAt: number | null) =>
    invoke<Todo>('schedule_todo', { id, dueAt, remindAt }),
  /** 待办拖拽排序：按传入顺序写入手动排序位（前端按分组计算完整顺序） */
  reorderTodoOrders: (ids: number[]) => invoke<void>('reorder_todo_orders', { ids }),
  /** 设置待办描述（轻量 Markdown） */
  setTodoDescription: (id: number, description: string) =>
    invoke<Todo>('set_todo_description', { id, description }),
  /** 置顶开关：置顶条目脱离日期分组，固定排在列表最顶部「置顶」区 */
  setTodoPinned: (id: number, pinned: boolean) =>
    invoke<Todo>('set_todo_pinned', { id, pinned }),
  /** 写入周期规则（整组 repeat_* 列一起写） */
  setTodoRepeat: (id: number, rule: RepeatRuleInput) =>
    invoke<Todo>('set_todo_repeat', {
      id,
      repeatMode: rule.mode,
      repeatEvery: rule.every,
      repeatUnit: rule.unit,
      repeatWeekdays: rule.weekdays,
      repeatMonthDay: rule.month_day,
      repeatMonthNth: rule.month_nth,
      repeatEndMode: rule.end_mode,
      repeatEndAt: rule.end_at,
      repeatCount: rule.count,
    }),
  /** 周期待办「完成本轮」：due_at 滚到下一个未来时刻、计数 +1、子待办复位 */
  completeTodoRecurring: (id: number) => invoke<Todo>('complete_todo_recurring', { id }),
  /** 撤销「完成本轮」：计数 −1，due_at 滚回上一个实例 */
  undoTodoRecurring: (id: number) => invoke<Todo>('undo_todo_recurring', { id }),
  /** 展开区间内的周期待办虚拟实例（规则只实现于 Rust 侧） */
  expandTodoOccurrences: (fromMs: number, toMs: number) =>
    invoke<TodoOccurrence[]>('expand_todo_occurrences', { fromMs, toMs }),
  listTodoTags: () => invoke<TodoTag[]>('list_todo_tags'),
  createTodoTag: (name: string, color?: string) =>
    invoke<TodoTag>('create_todo_tag', { name, color: color ?? null }),
  updateTodoTag: (id: number, name: string, color?: string) =>
    invoke<TodoTag>('update_todo_tag', { id, name, color: color ?? null }),
  deleteTodoTag: (id: number) => invoke<void>('delete_todo_tag', { id }),
  /** 全量设置某条待办的标签 */
  setTodoTags: (id: number, tagIds: number[]) =>
    invoke<void>('set_todo_tags', { id, tagIds }),
  listTodoTagLinks: () => invoke<TodoTagLink[]>('list_todo_tag_links'),
  listStickies: () => invoke<Sticky[]>('list_stickies'),
  getDetachedStickies: () => invoke<DetachedSticky[]>('get_detached_stickies'),
  saveSticky: (slot: number, content: string) =>
    invoke<Sticky>('save_sticky', { slot, content }),
  detachSticky: (slot: number) => invoke<DetachedSticky>('detach_sticky', { slot }),
  focusDetachedSticky: (slot: number) =>
    invoke<boolean>('focus_detached_sticky', { slot }),
  saveDetachedSticky: (slot: number, content: string) =>
    invoke<void>('save_detached_sticky', { slot, content }),
  toggleDetachedStickyPin: (slot: number, alwaysOnTop: boolean) =>
    invoke<void>('toggle_detached_sticky_pin', { slot, alwaysOnTop }),
  restoreDetachedSticky: (slot: number) =>
    invoke<number>('restore_detached_sticky', { slot }),
  deleteDetachedSticky: (slot: number) =>
    invoke<void>('delete_detached_sticky', { slot }),
  parseDroppedPath: (path: string) => invoke<DroppedAppInfo>('parse_dropped_path', { path }),
  scanInstalledApps: () => invoke<InstalledAppInfo[]>('scan_installed_apps'),
  getRunningProcesses: () => invoke<string[]>('get_running_processes'),
  importIconFile: (source: string) =>
    invoke<string | null>('import_icon_file', { source }),
  importWallpaper: (source: string) =>
    invoke<string>('import_wallpaper', { source }),
  /** 清理壁纸目录中当前配置（亮/暗）均未引用的文件；前端先改配置再调用 */
  cleanupWallpapers: () => invoke<void>('cleanup_wallpapers'),
  /** 保存笔记图片（base64，不含 data: 前缀）：落盘 notes/images，返回 xhub-note 协议 URL */
  importNoteImage: (dataB64: string, ext: string) =>
    invoke<string>('import_note_image', { dataB64, ext }),
  inspectPath: (path: string) =>
    invoke<{ name: string; is_dir: boolean }>('inspect_path', { path }),
  listTags: () => invoke<Tag[]>('list_tags'),
  createTag: (name: string) => invoke<Tag>('create_tag', { name }),
  deleteTag: (id: number) => invoke<void>('delete_tag', { id }),
  getNoteTags: (noteId: number) => invoke<Tag[]>('get_note_tags', { noteId }),
  setNoteTags: (noteId: number, tagIds: number[]) =>
    invoke<void>('set_note_tags', { noteId, tagIds }),
  listNoteTags: () => invoke<NoteTagRow[]>('list_note_tags'),
  backupData: (targetDir: string) => invoke<string>('backup_data', { targetDir }),
  restoreData: (source: string) => invoke<void>('restore_data', { source }),
  getDataPath: () => invoke<DataPathInfo>('get_data_path'),
  changeDataDir: (newDir: string) => invoke<void>('change_data_dir', { newDir }),
  restartApp: () => invoke<void>('restart_app'),
  saveConfig: (config: AppConfig) => invoke<AppConfig>('save_config', { config }),
  setWindowAlwaysOnTop: (value: boolean) =>
    invoke<void>('set_window_always_on_top', { value }),
  setAlwaysOnTopConfig: (value: boolean) =>
    invoke<void>('set_always_on_top_config', { value }),
  getGlobalShortcut: () => invoke<string>('get_global_shortcut'),
  setGlobalShortcut: (value: string) => invoke<string>('set_global_shortcut', { value }),
  getRunAtStartup: () =>
    invoke<AutostartStatus>('get_run_at_startup'),
  setRunAtStartup: (enabled: boolean) => invoke<void>('set_run_at_startup', { enabled }),
  getStartupHidden: () => invoke<boolean>('get_startup_hidden'),
  /** 右下角通知窗：前端上报内容高度→后端锚定工作区右下角并显示 */
  noticeReady: () => invoke<void>('notice_ready'),
  noticeLayout: (height: number) => invoke<void>('notice_layout', { height }),
  /** 通知队列清空后收起通知窗 */
  noticeDismiss: () => invoke<void>('notice_dismiss_window'),
  logClientError: (payload: ClientErrorPayload) =>
    invoke<void>('log_client_error', { message: payload.message, detail: payload.detail }),
  minimizeWindow: () => invoke<void>('minimize_window'),
  toggleMaximize: () => invoke<void>('toggle_maximize'),
  hideToTray: () => invoke<void>('hide_to_tray'),
  // ---- 桌面悬浮球（ADR 0004，窗口几何恒定 + 椭圆命中区域/吸附/位置记忆均在 Rust 侧） ----
  floatingBallGetState: () => invoke<FloatingBallState>('floating_ball_get_state'),
  floatingBallSaveSettings: (
    enabled: boolean,
    autoHide: boolean,
    withMain: boolean,
    buttons: string[],
    idleSpin: boolean,
  ) =>
    invoke<void>('floating_ball_save_settings', {
      enabled,
      autoHide,
      withMain,
      buttons,
      idleSpin,
    }),
  floatingBallDragBegin: () => invoke<void>('floating_ball_drag_begin'),
  floatingBallDragCancel: () => invoke<void>('floating_ball_drag_cancel'),
  floatingBallExpand: (expanded: boolean) => invoke<void>('floating_ball_expand', { expanded }),
  floatingBallTrigger: (id: string) => invoke<void>('floating_ball_trigger', { id }),
  floatingBallContextMenu: () => invoke<void>('floating_ball_context_menu'),
  /** 前端失配自检兜底：携带视口 CSS 宽度，让 Rust 按「物理宽÷视口宽」实测缩放重算
   * 窗口几何（窗口 DPI 上下文过期时 GetDpiForWindow 也是旧值，只有视口实测可信） */
  floatingBallReapply: (viewportW: number) =>
    invoke<void>('floating_ball_reapply', { viewportW }),
  getThemeConfig: () => invoke<ThemeConfig>('get_theme_config'),
  getSystemInfo: () => invoke<SystemInfo>('get_system_info'),
  listSnippets: () => invoke<Snippet[]>('list_snippets'),
  createSnippet: (title: string, content: string) =>
    invoke<Snippet>('create_snippet', { title, content }),
  updateSnippet: (id: number, title: string, content: string) =>
    invoke<Snippet>('update_snippet', { id, title, content }),
  deleteSnippet: (id: number) => invoke<void>('delete_snippet', { id }),
  toggleSnippetPin: (id: number) => invoke<Snippet>('toggle_snippet_pin', { id }),
  recordSnippetCopy: (id: number) => invoke<Snippet>('record_snippet_copy', { id }),
  togglePromptFloat: () => invoke<void>('toggle_prompt_float'),
  toggleTodoFloat: () => invoke<void>('toggle_todo_float'),
  toggleFloatPin: (label: string, alwaysOnTop: boolean) =>
    invoke<void>('toggle_float_pin', { label, alwaysOnTop }),
  listCountdowns: () => invoke<Countdown[]>('list_countdowns'),
  // ---- AI 对话 ----
  listChatSessions: () => invoke<ChatSession[]>('list_chat_sessions'),
  createChatSession: (payload?: { title?: string; modelName?: string }) =>
    invoke<ChatSession>('create_chat_session', {
      title: payload?.title ?? null,
      modelName: payload?.modelName ?? null,
    }),
  deleteChatSession: (id: number) => invoke<void>('delete_chat_session', { id }),
  renameChatSession: (id: number, title: string) =>
    invoke<ChatSession>('rename_chat_session', { id, title }),
  setChatSessionModel: (id: number, modelName: string) =>
    invoke<ChatSession>('set_chat_session_model', { id, modelName }),
  listChatMessages: (sessionId: number) =>
    invoke<ChatMessage[]>('list_chat_messages', { sessionId }),
  sendChatMessage: (
    sessionId: number,
    content: string,
    onEvent: (e: ChatStreamEvent) => void,
  ) => {
    const channel = new Channel<ChatStreamEvent>()
    channel.onmessage = onEvent
    return invoke<void>('send_chat_message', { sessionId, content, onEvent: channel })
  },
  getChatModels: () => invoke<ChatModelConfig[]>('get_chat_models'),
  /** 平台可用模型（「使用平台免费额度」；需登录账号） */
  platformModels: () => invoke<string[]>('platform_models'),
  saveChatModels: (models: ChatModelConfig[]) => invoke<ChatModelConfig[]>('save_chat_models', { models }),
  fetchChatProviderModels: (baseUrl: string, apiKey: string, keyId?: string) =>
    invoke<string[]>('fetch_chat_provider_models', { baseUrl, apiKey, keyId }),
  getChatApiKey: (modelId: string) => invoke<string>('get_chat_api_key', { modelId }),
  setChatPanel: (width: number, height: number, open: boolean) =>
    invoke<void>('set_chat_panel', { width, height, open }),
  getChatPanel: () => invoke<[number, number, boolean]>('get_chat_panel'),
  setChatPanelSide: (side: string) => invoke<void>('set_chat_panel_side', { side }),
  // ---- AI 对话独立窗口（与主窗内嵌抽屉互斥，窗口常驻隐藏、只做 show/hide） ----
  chatWindowGetState: () => invoke<ChatWindowState>('chat_window_get_state'),
  chatWindowToggle: () => invoke<void>('chat_window_toggle'),
  chatWindowClose: () => invoke<void>('chat_window_close'),
  chatWindowSetPinned: (pinned: boolean) =>
    invoke<void>('chat_window_set_pinned', { pinned }),
  chatWindowSaveMode: (enabled: boolean) =>
    invoke<void>('chat_window_save_mode', { enabled }),
  chatWindowOpenSettings: () => invoke<void>('chat_window_open_settings'),
  getAppInfo: () => invoke<AppInfo>('get_app_info'),
  createCountdown: (payload: {
    name: string
    repeatMode: string
    endAt: number
    totalMs: number
    intervalMinutes?: number | null
  }) =>
    invoke<Countdown>('create_countdown', {
      name: payload.name,
      repeatMode: payload.repeatMode,
      endAt: payload.endAt,
      totalMs: payload.totalMs,
      intervalMinutes: payload.intervalMinutes ?? null,
    }),
  updateCountdown: (payload: {
    id: number
    name: string
    repeatMode: string
    endAt: number
    totalMs: number
    intervalMinutes?: number | null
  }) =>
    invoke<Countdown>('update_countdown', {
      id: payload.id,
      name: payload.name,
      repeatMode: payload.repeatMode,
      endAt: payload.endAt,
      totalMs: payload.totalMs,
      intervalMinutes: payload.intervalMinutes ?? null,
    }),
  deleteCountdown: (id: number) => invoke<void>('delete_countdown', { id }),
  pauseCountdown: (id: number) => invoke<Countdown>('pause_countdown', { id }),
  resumeCountdown: (id: number) => invoke<Countdown>('resume_countdown', { id }),
  floatCountdown: (id: number) => invoke<Countdown>('float_countdown', { id }),
  unfloatCountdown: (id: number) => invoke<Countdown>('unfloat_countdown', { id }),
  // 同步工作台倒计时卡片可见性：卡片不在已提交布局时后端冻结全部非浮窗倒计时（不计时、不提醒）
  setCountdownCardVisible: (visible: boolean) =>
    invoke<void>('set_countdown_card_visible', { visible }),
  // ---- 剪贴板历史 ----
  clipboardList: (keyword?: string, limit?: number, offset?: number) =>
    invoke<ClipboardItem[]>('clipboard_list', {
      keyword: keyword ?? null,
      limit: limit ?? 50,
      offset: offset ?? 0,
    }),
  clipboardCopy: (id: number) => invoke<void>('clipboard_copy', { id }),
  clipboardPaste: (id: number) => invoke<void>('clipboard_paste', { id }),
  clipboardTogglePin: (id: number) => invoke<ClipboardItem>('clipboard_toggle_pin', { id }),
  clipboardDelete: (id: number) => invoke<void>('clipboard_delete', { id }),
  clipboardClear: () => invoke<void>('clipboard_clear'),
  clipboardSetPaused: (paused: boolean) => invoke<void>('clipboard_set_paused', { paused }),
  setClipboardMediaEnabled: (image: boolean, file: boolean) =>
    invoke<void>('set_clipboard_media_enabled', { image, file }),
  clipboardExportImage: (id: number, dest: string) =>
    invoke<void>('clipboard_export_image', { id, dest }),
  clipboardActivate: () => invoke<void>('clipboard_activate'),
  clipboardHide: () => invoke<void>('clipboard_hide'),
  setClipboardPasteMethod: (method: string) => invoke<string>('set_clipboard_paste_method', { method }),
  clipboardGetInfo: () => invoke<ClipboardInfo>('clipboard_get_info'),
  setClipboardShortcut: (value: string) => invoke<string>('set_clipboard_shortcut', { value }),
  setClipboardRetention: (maxItems: number, ttlDays: number) =>
    invoke<void>('set_clipboard_retention', { maxItems, ttlDays }),
  // ---- 在线服务 ----
  checkConnectivity: () => invoke<boolean>('check_connectivity'),
  getWeather: () => invoke<WeatherCurrent | null>('get_weather'),
  getQuote: () => invoke<Quote>('get_quote'),
  setWeatherCity: (city: string) => invoke<GeoLocation>('set_weather_city', { city }),
  locateWeatherByIp: () => invoke<GeoLocation>('locate_weather_by_ip'),
  // ---- 扩展系统 ----
  listExtensions: () => invoke<ExtensionEntry[]>('list_extensions'),
  extensionsStamp: () => invoke<number>('extensions_stamp'),
  /** 读取扩展某形态入口 URL（xhub-ext 协议，直接作为 iframe src；入口 HTML 由后端注入桥脚本） */
  readExtensionEntry: (id: string, surface?: string | null) =>
    invoke<string>('read_extension_entry', { id, surface: surface ?? null }),
  /** 在系统文件管理器中打开扩展所在目录（开发调试用；返回实际打开的绝对路径） */
  openExtensionDir: (id: string) => invoke<string>('open_extension_dir', { id }),
  // ---- 「我的扩展」（本机源码目录直挂，登记即加载；见 docs/adr/0005） ----
  getDevModeStatus: () => invoke<DevModeStatus>('get_dev_mode_status'),
  addDevExtension: (path: string) => invoke<DevModeStatus>('add_dev_extension', { path }),
  removeDevExtension: (path: string) => invoke<DevModeStatus>('remove_dev_extension', { path }),
  /** 本机源码目录内容戳（全目录 FNV+mtime；变化即热重载对应 iframe） */
  devExtensionsStamp: () => invoke<number>('dev_extensions_stamp'),
  // ---- 扩展开发技能包（Skills：内置 x-hub-extension 一键装到本机 AI 助手 skills 目录） ----
  getSkillOverview: () => invoke<SkillOverview>('get_skill_overview'),
  /** 安装/更新到指定 skills 根；目标已存在且非本客户端安装时需 `force=true` 覆盖 */
  installSkill: (path: string, force = false) =>
    invoke<SkillOverview>('install_skill', { path, force }),
  uninstallSkill: (path: string) => invoke<SkillOverview>('uninstall_skill', { path }),
  /** 从列表移除自定义目录（只解除登记，不动磁盘文件） */
  removeSkillRoot: (path: string) => invoke<SkillOverview>('remove_skill_root', { path }),
  // ---- 平台账号（登录 / 额度 / 开发者申请；服务端地址是内置常量，不可配置） ----
  accountStatus: () => invoke<AccountStatus>('account_status'),
  accountLoginGithubStart: () => invoke<GithubDeviceStart>('account_login_github_start'),
  accountLoginGithubPoll: (pollId: string) =>
    invoke<GithubPollResult>('account_login_github_poll', { pollId }),
  accountLoginEmailSend: (email: string) =>
    invoke<EmailSendResult>('account_login_email_send', { email }),
  accountLoginEmailVerify: (email: string, code: string) =>
    invoke<AccountStatus>('account_login_email_verify', { email, code }),
  accountLogout: () => invoke<AccountStatus>('account_logout'),
  /** 兑换邀请码：账号与权益解耦，兑换后才发额度、才可申请开发者 */
  accountRedeem: (code: string) => invoke<AccountStatus>('account_redeem', { code }),
  devApply: (reason: string) => invoke<DevApplyStatus>('dev_apply', { reason }),
  devApplyStatus: () => invoke<DevApplyStatus>('dev_apply_status'),
  /** 我的在线设备（不含 token 明文） */
  accountListDevices: () =>
    invoke<{ devices: AccountDevice[]; max: number }>('account_list_devices'),
  /** 撤销某台设备（换机/设备丢失时用） */
  accountRevokeDevice: (id: number) => invoke<unknown>('account_revoke_device', { id }),
  // ---- 扩展发布（打包上传 / 我的提交 / 撤回） ----
  devSubmit: (id: string, changelog?: string, minAppVersion?: string, homepage?: string, screenshots?: string[]) =>
    invoke<SubmitResult>('dev_submit', {
      id,
      changelog: changelog ?? null,
      minAppVersion: minAppVersion ?? null,
      homepage: homepage ?? null,
      screenshots: screenshots && screenshots.length ? screenshots : null,
    }),
  /** 读本地图片为 data URL（发布弹窗的截图缩略图预览用；作者选的图不在资产白名单目录里） */
  readImageDataUrl: (path: string) => invoke<string>('read_image_data_url', { path }),
  devListSubmissions: (page?: number, pageSize?: number) =>
    invoke<DevSubmissionList>('dev_list_submissions', {
      page: page ?? null,
      pageSize: pageSize ?? null,
    }),
  devGetSubmission: (id: number) =>
    invoke<{ submission: DevSubmissionDetail }>('dev_get_submission', { id }),
  devWithdrawSubmission: (id: number) => invoke<unknown>('dev_withdraw_submission', { id }),
  /** 发布前本地预检（作者侧 lint：manifest / 权限申报 / 桥 API 可用性） */
  precheckExtension: (id: string) => invoke<PrecheckResult>('precheck_extension', { id }),
  /** 打开扩展的独立窗口（window 形态） */
  openExtensionWindow: (id: string) => invoke<void>('open_extension_window', { id }),
  /** 卸载扩展（停止 service 后端进程并删除目录） */
  uninstallExtension: (id: string) => invoke<void>('uninstall_extension', { id }),
  /** 从本地压缩包（.xhpack，zip 格式）安装扩展，返回扩展 id */
  installLocalArchive: (path: string) => invoke<string>('install_local_archive', { path }),
  /** 查询扩展权限状态（manifest 声明 → 是否授予） */
  getExtensionPermissions: (id: string) =>
    invoke<Record<string, boolean>>('get_extension_permissions', { id }),
  /** 设置扩展某权限开关 */
  setExtensionPermission: (id: string, permission: string, granted: boolean) =>
    invoke<void>('set_extension_permission', { id, permission, granted }),
  // ---- 扩展市场 ----
  getMarketRegistry: () => invoke<MarketStatus>('get_market_registry'),
  /** 拉取远端市场清单（fetch 原始字节 + Ed25519 验签 + 原子落缓存），失败回退本地缓存 */
  refreshMarketRegistry: () => invoke<MarketStatus>('refresh_market_registry'),
  /** 把扩展目录打成 .xhpack（manifest 在包根，排除 node_modules 与隐藏项） */
  packExtensionArchive: (id: string, outPath?: string | null) =>
    invoke<PackedArchive>('pack_extension_archive', { id, outPath: outPath ?? null }),
  /** 从市场下载并安装扩展（流式下载 + sha256 校验 + 解包），返回扩展 id */
  installFromMarket: (extension: MarketExtension) =>
    invoke<string>('install_from_market', { extension }),
  /** 从市场更新扩展（校验 + 版本比较 + 备份 + 保留用户点文件 + 原子替换 + 回滚），返回扩展 id */
  updateFromMarket: (extension: MarketExtension) =>
    invoke<string>('update_extension', { extension }),
  // ---- 应用更新 ----
  /** 检查应用更新（拉取 update.json + Ed25519 验签 + 版本比较），失败静默返回无更新。
   * `manual`：About 页手动触发时传 true——忽略「跳过此版本」记录，让用户能再次主动获取该版本。 */
  checkForUpdate: (manual = false) => invoke<UpdateInfo>('check_for_update', { manual }),
  /** 下载可用更新（重新验签清单 + 流式下载 + sha256 校验 + 写待应用标记后广播 update-ready） */
  downloadUpdate: (version: string) => invoke<UpdateInfo>('download_update', { version }),
  /** 读取当前更新状态（本地标记，不发网络请求） */
  getUpdateStatus: () => invoke<UpdateInfo>('get_update_status'),
  /** 记录「跳过此版本」：该版本后续不再提示更新 */
  skipUpdateVersion: (version: string) => invoke<void>('skip_update_version', { version }),
  /** 打开外部链接（系统默认浏览器；仅放行 http/https） */
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  /** 桥 API 统一分发：扩展 iframe 经主窗口转发调用 */
  xhubCall: (extId: string, namespace: string, method: string, args: unknown) =>
    invoke<unknown>('xhub_call', { extId, namespace, method, args }),
}
