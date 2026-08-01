import { reactive, readonly } from 'vue'
import {
  tauriApi,
  isTauri,
  type AppConfig,
  type Group,
  type Note,
  type Resource,
} from '../api/tauri'

interface StoreState {
  groups: Group[]
  resources: Resource[]
  notes: Note[]
  config: AppConfig
  loaded: boolean
}

const state = reactive<StoreState>({
  groups: [],
  resources: [],
  notes: [],
  config: {
    theme: 'light',
    window: {
      width: 1100,
      height: 760,
      x: null,
      y: null,
      always_on_top: false,
    },
  },
  loaded: false,
})

export function useStore() {
  async function loadInitialData() {
    if (!isTauri()) return
    const data = await tauriApi.getInitialData()
    state.groups = data.groups
    state.resources = data.resources
    state.notes = data.notes
    state.config = data.config
    state.loaded = true
  }

  // ---- 分组 ----
  async function addGroup(name: string) {
    const g = await tauriApi.createGroup(name)
    state.groups.push(g)
    return g
  }

  async function renameGroup(id: number, name: string) {
    const g = await tauriApi.updateGroup(id, name)
    const idx = state.groups.findIndex((x) => x.id === id)
    if (idx >= 0) state.groups[idx] = g
    return g
  }

  async function removeGroup(id: number) {
    await tauriApi.deleteGroup(id)
    state.groups = state.groups.filter((x) => x.id !== id)
    state.resources = state.resources.filter((x) => x.group_id !== id)
  }

  async function moveGroups(ids: number[]) {
    await tauriApi.reorderGroups(ids)
    state.groups = state.groups
      .slice()
      .sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id))
  }

  // ---- 资源 ----
  async function addResource(payload: {
    groupId: number
    kind: 'app' | 'web'
    name: string
    target: string
    icon?: string | null
    args?: string | null
  }) {
    const r = await tauriApi.createResource(payload)
    state.resources.push(r)
    return r
  }

  async function editResource(payload: {
    id: number
    groupId: number
    kind: 'app' | 'web'
    name: string
    target: string
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

  async function moveResources(groupId: number, ids: number[]) {
    await tauriApi.reorderResources(groupId, ids)
    const idSet = new Set(ids)
    state.resources.forEach((r) => {
      if (idSet.has(r.id)) r.group_id = groupId
    })
    const groupResources = state.resources
      .filter((r) => r.group_id === groupId)
      .sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id))
    const otherResources = state.resources.filter((r) => r.group_id !== groupId)
    state.resources = groupResources.concat(otherResources)
  }

  async function launchResource(id: number) {
    await tauriApi.launchResource(id)
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

  return {
    state: readonly(state),
    loadInitialData,
    addGroup,
    renameGroup,
    removeGroup,
    moveGroups,
    addResource,
    editResource,
    removeResource,
    moveResources,
    launchResource,
    addNote,
    saveNote,
    removeNote,
    searchAll,
    setTheme,
    setAlwaysOnTop,
  }
}
