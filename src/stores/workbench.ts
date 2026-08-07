import { reactive, readonly } from 'vue'
import {
  tauriApi,
  isTauri,
  type AppConfig,
  type Note,
  type Resource,
  type Tag,
} from '../api/tauri'

interface StoreState {
  resources: Resource[]
  notes: Note[]
  tags: Tag[]
  config: AppConfig
  loaded: boolean
}

const state = reactive<StoreState>({
  resources: [],
  notes: [],
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
  loaded: false,
})

export function useStore() {
  async function loadInitialData() {
    if (!isTauri()) return
    const data = await tauriApi.getInitialData()
    state.resources = data.resources
    state.notes = data.notes
    state.tags = data.tags
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
    if (!isTauri()) return { resources: [] as Resource[], notes: [] as Note[] }
    return tauriApi.searchAll(keyword)
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
    createTag,
    deleteTag,
    loadNoteTagsMap,
    setTheme,
    setAlwaysOnTop,
    setGlobalShortcut,
  }
}
