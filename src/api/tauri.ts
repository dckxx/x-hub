import { invoke } from '@tauri-apps/api/core'

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

export interface WindowState {
  width: number
  height: number
  x: number | null
  y: number | null
  always_on_top: boolean
}

export interface AppConfig {
  theme: string
  window: WindowState
  global_shortcut: string
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

export interface DroppedAppInfo {
  name: string
  target: string
  icon: string | null
}

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
  parseDroppedPath: (path: string) => invoke<DroppedAppInfo>('parse_dropped_path', { path }),
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
}
