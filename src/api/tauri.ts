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

export interface AppConfig {
  theme_mode: string // 'light' | 'dark' | 'system'
  theme_preset: string // 'indigo' | 'green' | 'morandi' | 'midnight'
  accent_color: string | null // hex like '#5b5bf5'; null = follow preset recommended
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
  /** 升级后弹窗显示更新说明（默认开启） */
  whats_new_enabled: boolean
  /** 上次已记录「更新说明」的版本号（空串表示首次运行） */
  last_seen_version: string
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
  /** service 扩展运行时策略：auto / builtin / system */
  runtime_strategy: string
  /** 固定到左侧栏的扩展 id 列表（点击侧栏菜单即在主区打开对应扩展） */
  sidebar_extensions: string[]
  /** 扩展「默认打开方式」映射：extId → view / window / drawer（未设置时侧栏点击默认 view） */
  extension_open_modes: Record<string, string>
  /** 开机自启动（登录 Windows 时自动驻留托盘） */
  run_at_startup: boolean
  /** 开机自启动是否以管理员身份运行（仅 run_at_startup 启用时生效） */
  run_at_startup_admin: boolean
}

export interface AppInfo {
  version: string
  changelog: string
  latest_section: string
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
}

/** 市场下载进度事件负载（market-download-progress） */
export interface MarketDownloadProgress {
  id: string
  received: number
  total: number | null
}

export interface DataPathInfo {
  /** 当前数据根绝对路径 */
  path: string
  /** default（默认 %APPDATA% 路径）/ custom（用户自定义）/ portable（便携模式，跟随程序目录） */
  mode: 'default' | 'custom' | 'portable'
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

export type ChatStreamEvent =
  | { type: 'chunk'; content: string }
  | { type: 'done'; message: ChatMessage; session: ChatSession }
  | { type: 'error'; message: string; partial: string }

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const tauriApi = {
  getInitialData: () => invoke<InitialData>('get_initial_data'),
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
  createNote: (title: string) => invoke<Note>('create_note', { title }),
  updateNote: (id: number, title: string, content: string) =>
    invoke<Note>('update_note', { id, title, content }),
  deleteNote: (id: number) => invoke<void>('delete_note', { id }),
  listNotes: () => invoke<Note[]>('list_notes'),
  searchAll: (keyword: string) => invoke<SearchResult>('search_all', { keyword }),
  listTodos: () => invoke<Todo[]>('list_todos'),
  createTodo: (title: string) => invoke<Todo>('create_todo', { title }),
  toggleTodo: (id: number) => invoke<Todo>('toggle_todo', { id }),
  updateTodo: (id: number, title: string, priority: number) =>
    invoke<Todo>('update_todo', { id, title, priority }),
  deleteTodo: (id: number) => invoke<void>('delete_todo', { id }),
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
    invoke<{ enabled: boolean; admin: boolean }>('get_run_at_startup'),
  setRunAtStartup: (enabled: boolean, admin: boolean) =>
    invoke<void>('set_run_at_startup', { enabled, admin }),
  getStartupHidden: () => invoke<boolean>('get_startup_hidden'),
  logClientError: (payload: ClientErrorPayload) =>
    invoke<void>('log_client_error', { message: payload.message, detail: payload.detail }),
  minimizeWindow: () => invoke<void>('minimize_window'),
  toggleMaximize: () => invoke<void>('toggle_maximize'),
  hideToTray: () => invoke<void>('hide_to_tray'),
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
  saveChatModels: (models: ChatModelConfig[]) => invoke<ChatModelConfig[]>('save_chat_models', { models }),
  fetchChatProviderModels: (baseUrl: string, apiKey: string, keyId?: string) =>
    invoke<string[]>('fetch_chat_provider_models', { baseUrl, apiKey, keyId }),
  getChatApiKey: (modelId: string) => invoke<string>('get_chat_api_key', { modelId }),
  setChatPanel: (width: number, height: number, open: boolean) =>
    invoke<void>('set_chat_panel', { width, height, open }),
  getChatPanel: () => invoke<[number, number, boolean]>('get_chat_panel'),
  setChatPanelSide: (side: string) => invoke<void>('set_chat_panel_side', { side }),
  getAppInfo: () => invoke<AppInfo>('get_app_info'),
  checkWhatsNew: () => invoke<string | null>('check_whats_new'),  createCountdown: (payload: {
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
  // ---- 剪贴板历史 ----
  clipboardList: (keyword?: string, limit?: number) =>
    invoke<ClipboardItem[]>('clipboard_list', { keyword: keyword ?? null, limit: limit ?? 50 }),
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
  /** 读取扩展某形态入口（注入桥脚本后返回临时 HTML 绝对路径） */
  readExtensionEntry: (id: string, surface?: string | null) =>
    invoke<string>('read_extension_entry', { id, surface: surface ?? null }),
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
  /** 从市场下载并安装扩展（流式下载 + sha256 校验 + 解包），返回扩展 id */
  installFromMarket: (extension: MarketExtension) =>
    invoke<string>('install_from_market', { extension }),
  /** 从市场更新扩展（校验 + 版本比较 + 备份 + 保留用户点文件 + 原子替换 + 回滚），返回扩展 id */
  updateFromMarket: (extension: MarketExtension) =>
    invoke<string>('update_extension', { extension }),
  /** 打开外部链接（系统默认浏览器；仅放行 http/https） */
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  /** 桥 API 统一分发：扩展 iframe 经主窗口转发调用 */
  xhubCall: (extId: string, namespace: string, method: string, args: unknown) =>
    invoke<unknown>('xhub_call', { extId, namespace, method, args }),
}
