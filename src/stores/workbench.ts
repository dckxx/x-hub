import { reactive, readonly } from 'vue'
import {
  tauriApi,
  isTauri,
  type AppConfig,
  type Note,
  type Resource,
  type Sticky,
  type SyncResult,
  type SystemInfo,
  type Tag,
  type Todo,
  type UsageDetail,
  type UsageSummary,
} from '../api/tauri'

interface StoreState {
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
  stickies: Sticky[]
  tags: Tag[]
  config: AppConfig
  usageSummary: UsageSummary | null
  usageDetail: UsageDetail | null
  usageListening: boolean
  systemInfo: SystemInfo | null
  loaded: boolean
}

const state = reactive<StoreState>({
  resources: [],
  notes: [],
  todos: [],
  stickies: [],
  tags: [],
  config: {
    theme: 'light',
    window: {
      width: 1400,
      height: 900,
      x: null,
      y: null,
      always_on_top: false,
    },
    global_shortcut: 'CommandOrControl+Shift+Space',
  },
  usageSummary: null,
  usageDetail: null,
  usageListening: false,
  systemInfo: null,
  loaded: false,
})

export function useStore() {
  async function loadInitialData() {
    if (!isTauri()) return
    const data = await tauriApi.getInitialData()
    state.resources = data.resources
    state.notes = data.notes
    state.todos = data.todos
    state.stickies = data.stickies
    state.tags = data.tags
    state.usageSummary = data.usage_summary
    state.config = data.config
    state.loaded = true
  }

  // ---- 速达资源 ----
  async function addResource(payload: {
    kind: 'app' | 'web' | 'file'
    name: string
    target: string
    category?: string | null
    icon?: string | null
    args?: string | null
  }) {
    const r = await tauriApi.createResource(payload)
    state.resources.push(r)
    return r
  }

  async function editResource(payload: {
    id: number
    kind: 'app' | 'web' | 'file'
    name: string
    target: string
    category?: string | null
    icon?: string | null
    args?: string | null
  }) {
    const r = await tauriApi.updateResource(payload)
    const idx = state.resources.findIndex((x) => x.id === r.id)
    if (idx >= 0) state.resources[idx] = r
    return r
  }

  async function removeResource(id: number) {
    await tauriApi.deleteResource(id)
    state.resources = state.resources.filter((x) => x.id !== id)
  }

  async function launchResource(id: number) {
    await tauriApi.launchResource(id)
    const r = state.resources.find((x) => x.id === id)
    if (r) r.last_launched_at = new Date().toISOString()
  }

  // ---- 笔记 ----
  async function addNote(title: string) {
    const n = await tauriApi.createNote(title)
    state.notes.unshift(n)
    return n
  }

  async function saveNote(id: number, title: string, content: string) {
    const n = await tauriApi.updateNote(id, title, content)
    const idx = state.notes.findIndex((x) => x.id === id)
    if (idx >= 0) state.notes[idx] = n
    return n
  }

  async function removeNote(id: number) {
    await tauriApi.deleteNote(id)
    state.notes = state.notes.filter((x) => x.id !== id)
  }

  async function searchAll(keyword: string) {
    if (!isTauri()) return { resources: [] as Resource[], notes: [] as Note[], todos: [] as Todo[] }
    return tauriApi.searchAll(keyword)
  }

  // ---- 待办 ----
  async function createTodo(title: string) {
    const t = isTauri()
      ? await tauriApi.createTodo(title)
      : { id: Date.now(), title, done: false, priority: 0, created_at: new Date().toISOString(), updated_at: new Date().toISOString(), completed_at: null }
    state.todos.push(t)
    return t
  }

  async function toggleTodo(id: number) {
    const i = state.todos.findIndex((t) => t.id === id)
    if (i < 0) return null
    if (isTauri()) {
      const updated = await tauriApi.toggleTodo(id)
      state.todos[i] = updated
      return updated
    }
    const cur = state.todos[i]
    const flipped = {
      ...cur,
      done: !cur.done,
      completed_at: cur.done ? null : new Date().toISOString(),
      updated_at: new Date().toISOString(),
    }
    state.todos[i] = flipped
    return flipped
  }

  async function updateTodo(id: number, title: string, priority: number) {
    const i = state.todos.findIndex((t) => t.id === id)
    if (i < 0) return null
    if (isTauri()) {
      const updated = await tauriApi.updateTodo(id, title, priority)
      state.todos[i] = updated
      return updated
    }
    const cur = state.todos[i]
    const updated = { ...cur, title, priority, updated_at: new Date().toISOString() }
    state.todos[i] = updated
    return updated
  }

  async function deleteTodo(id: number) {
    if (isTauri()) await tauriApi.deleteTodo(id)
    state.todos = state.todos.filter((t) => t.id !== id)
  }

  // ---- 便签 ----
  async function saveSticky(slot: number, content: string) {
    if (!isTauri()) {
      const existing = state.stickies.find((s) => s.slot === slot)
      if (existing) {
        existing.content = content
        existing.updated_at = new Date().toISOString()
        return existing
      }
      const created: Sticky = {
        id: Date.now(),
        slot,
        content,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      }
      state.stickies.push(created)
      return created
    }
    const s = await tauriApi.saveSticky(slot, content)
    const i = state.stickies.findIndex((x) => x.slot === s.slot)
    if (i >= 0) state.stickies[i] = s
    else state.stickies.push(s)
    return s
  }

  // ---- 标签 ----
  async function createTag(name: string) {
    const t = await tauriApi.createTag(name)
    if (!state.tags.some((x) => x.id === t.id)) state.tags.push(t)
    return t
  }

  async function deleteTag(id: number) {
    await tauriApi.deleteTag(id)
    state.tags = state.tags.filter((x) => x.id !== id)
  }

  // ---- 笔记-标签关联（列表筛选用） ----
  async function loadNoteTagsMap() {
    if (!isTauri()) return []
    return tauriApi.listNoteTags()
  }

  // ---- 配置 ----
  async function setTheme(theme: 'light' | 'dark') {
    state.config.theme = theme
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setAlwaysOnTop(value: boolean) {
    state.config.window.always_on_top = value
    if (!isTauri()) return
    await tauriApi.setAlwaysOnTopConfig(value)
    await tauriApi.setWindowAlwaysOnTop(value)
  }

  async function setGlobalShortcut(value: string) {
    state.config.global_shortcut = value
    if (!isTauri()) return value
    const saved = await tauriApi.setGlobalShortcut(value)
    state.config.global_shortcut = saved
    return saved
  }

  // ---- AI 用量 ----
  async function refreshUsage(path?: string): Promise<SyncResult> {
    if (!isTauri()) {
      return { inserted: 0, cursor: 0, listening: false, path: null }
    }
    const result = await tauriApi.syncAiUsage(path)
    state.usageListening = result.listening
    if (result.listening) {
      state.usageSummary = await tauriApi.getUsageSummary()
    }
    return result
  }

  async function loadUsageSummary() {
    if (!isTauri()) return
    state.usageSummary = await tauriApi.getUsageSummary()
  }

  async function loadUsageDetail(days = 7, limit = 50, offset = 0) {
    if (!isTauri()) return null
    state.usageDetail = await tauriApi.getUsageDetail(days, limit, offset)
    return state.usageDetail
  }

  // ---- 系统资源 ----
  async function refreshSystemInfo() {
    if (!isTauri()) return null
    state.systemInfo = await tauriApi.getSystemInfo()
    return state.systemInfo
  }

  return {
    state: readonly(state),
    loadInitialData,
    addResource,
    editResource,
    removeResource,
    launchResource,
    addNote,
    saveNote,
    removeNote,
    searchAll,
    createTodo,
    toggleTodo,
    updateTodo,
    deleteTodo,
    saveSticky,
    createTag,
    deleteTag,
    loadNoteTagsMap,
    setTheme,
    setAlwaysOnTop,
    setGlobalShortcut,
    refreshUsage,
    loadUsageSummary,
    loadUsageDetail,
    refreshSystemInfo,
  }
}
