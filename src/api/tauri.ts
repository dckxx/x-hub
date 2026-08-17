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
  countdown_sound: boolean
  clock_quote: string // 时钟卡片语录（可配置，空串回退默认）
  chat_models: ChatModelConfig[]
  chat_panel_width: number
  chat_panel_open: boolean
  /** AI 对话面板透明度（0.5–1.0，设置中可调） */
  chat_panel_opacity: number
  /** 升级后弹窗显示更新说明（默认关闭） */
  whats_new_enabled: boolean
  /** 上次已记录「更新说明」的版本号（空串表示首次运行） */
  last_seen_version: string
}

export interface AppInfo {
  version: string
  changelog: string
  latest_section: string
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
  usage_summary: UsageSummary
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

export interface UsageRecord {
  session_id: string
  provider: string | null
  model: string | null
  tokens_input: number
  tokens_cache_read: number
  tokens_output: number
  tokens_reasoning: number
  tokens_cache_write: number
  cost: number
  time_created: number
  source: string
}

export interface UsageSummary {
  today_input: number
  today_cache_input: number
  today_output: number
  today_cost: number
  /** 今日调用（消息）条数 */
  today_count: number
  seven_day_input: number
  seven_day_cache_input: number
  seven_day_output: number
  seven_day_cost: number
  month_input: number
  month_cache_input: number
  month_output: number
  month_cost: number
  total_input: number
  total_cache_input: number
  total_output: number
  total_cost: number
  record_count: number
  last_sync_at: number | null
}

export interface UsageDaily {
  date: string
  input: number
  cache_input: number
  output: number
  cost: number
}

export interface UsageProvider {
  provider: string
  count: number
  input: number
  cache_input: number
  output: number
  cost: number
}

export interface UsageDetail {
  daily: UsageDaily[]
  providers: UsageProvider[]
  records: UsageRecord[]
  total: number
}

export interface SyncResult {
  inserted: number
  cursor: number
  listening: boolean
  path: string | null
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
  | { type: 'done'; message: ChatMessage }
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
  backupData: (targetDir: string) => invoke<void>('backup_data', { targetDir }),
  restoreData: (sourceDir: string) => invoke<void>('restore_data', { sourceDir }),
  saveConfig: (config: AppConfig) => invoke<AppConfig>('save_config', { config }),
  setWindowAlwaysOnTop: (value: boolean) =>
    invoke<void>('set_window_always_on_top', { value }),
  setAlwaysOnTopConfig: (value: boolean) =>
    invoke<void>('set_always_on_top_config', { value }),
  getGlobalShortcut: () => invoke<string>('get_global_shortcut'),
  setGlobalShortcut: (value: string) => invoke<string>('set_global_shortcut', { value }),
  logClientError: (payload: ClientErrorPayload) =>
    invoke<void>('log_client_error', { message: payload.message, detail: payload.detail }),
  minimizeWindow: () => invoke<void>('minimize_window'),
  toggleMaximize: () => invoke<void>('toggle_maximize'),
  hideToTray: () => invoke<void>('hide_to_tray'),
  syncAiUsage: (path?: string) => invoke<SyncResult>('sync_ai_usage', { path: path ?? null }),
  getUsageSummary: () => invoke<UsageSummary>('get_usage_summary'),
  getUsageDetail: (days: number, limit: number, offset: number) =>
    invoke<UsageDetail>('get_usage_detail', { days, limit, offset }),
  getSystemInfo: () => invoke<SystemInfo>('get_system_info'),
  listSnippets: () => invoke<Snippet[]>('list_snippets'),
  createSnippet: (title: string, content: string) =>
    invoke<Snippet>('create_snippet', { title, content }),
  updateSnippet: (id: number, title: string, content: string) =>
    invoke<Snippet>('update_snippet', { id, title, content }),
  deleteSnippet: (id: number) => invoke<void>('delete_snippet', { id }),
  toggleSnippetPin: (id: number) => invoke<Snippet>('toggle_snippet_pin', { id }),
  recordSnippetCopy: (id: number) => invoke<Snippet>('record_snippet_copy', { id }),
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
  setChatPanel: (width: number, open: boolean) =>
    invoke<void>('set_chat_panel', { width, open }),
  getChatPanel: () => invoke<[number, boolean]>('get_chat_panel'),
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
}
