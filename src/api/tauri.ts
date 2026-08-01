import { invoke } from '@tauri-apps/api/core'

export interface Group {
  id: number
  name: string
  sort_order: number
  created_at: string
  updated_at: string
}

export interface Resource {
  id: number
  group_id: number
  kind: 'app' | 'web'
  name: string
  target: string
  icon: string | null
  args: string | null
  sort_order: number
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
}

export interface InitialData {
  groups: Group[]
  resources: Resource[]
  notes: Note[]
  config: AppConfig
}

export interface SearchResult {
  resources: Resource[]
  notes: Note[]
}

export interface WindowPosPayload {
  x: number | null
  y: number | null
  width: number
  height: number
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const tauriApi = {
  getInitialData: () => invoke<InitialData>('get_initial_data'),
  createGroup: (name: string) => invoke<Group>('create_group', { name }),
  updateGroup: (id: number, name: string) => invoke<Group>('update_group', { id, name }),
  deleteGroup: (id: number) => invoke<void>('delete_group', { id }),
  reorderGroups: (ids: number[]) => invoke<void>('reorder_groups', { ids }),
  createResource: (payload: {
    groupId: number
    kind: 'app' | 'web'
    name: string
    target: string
    icon?: string | null
    args?: string | null
  }) => invoke<Resource>('create_resource', {
    groupId: payload.groupId,
    kind: payload.kind,
    name: payload.name,
    target: payload.target,
    icon: payload.icon ?? null,
    args: payload.args ?? null,
  }),
  updateResource: (payload: {
    id: number
    groupId: number
    kind: 'app' | 'web'
    name: string
    target: string
    icon?: string | null
    args?: string | null
  }) => invoke<Resource>('update_resource', {
    id: payload.id,
    groupId: payload.groupId,
    kind: payload.kind,
    name: payload.name,
    target: payload.target,
    icon: payload.icon ?? null,
    args: payload.args ?? null,
  }),
  deleteResource: (id: number) => invoke<void>('delete_resource', { id }),
  reorderResources: (groupId: number, ids: number[]) =>
    invoke<void>('reorder_resources', { groupId, ids }),
  launchResource: (id: number) => invoke<void>('launch_resource', { id }),
  createNote: (title: string) => invoke<Note>('create_note', { title }),
  updateNote: (id: number, title: string, content: string) =>
    invoke<Note>('update_note', { id, title, content }),
  deleteNote: (id: number) => invoke<void>('delete_note', { id }),
  searchAll: (keyword: string) => invoke<SearchResult>('search_all', { keyword }),
  getConfig: () => invoke<AppConfig>('get_config'),
  saveConfig: (config: AppConfig) => invoke<AppConfig>('save_config', { config }),
  saveWindowState: (payload: WindowPosPayload) =>
    invoke<void>('save_window_state', { payload }),
  setWindowAlwaysOnTop: (value: boolean) =>
    invoke<void>('set_window_always_on_top', { value }),
  setAlwaysOnTopConfig: (value: boolean) =>
    invoke<void>('set_always_on_top_config', { value }),
  minimizeWindow: () => invoke<void>('minimize_window'),
  toggleMaximize: () => invoke<void>('toggle_maximize'),
  hideToTray: () => invoke<void>('hide_to_tray'),
  toggleWindowVisibility: () => invoke<void>('toggle_window_visibility'),
  quitApp: () => invoke<void>('quit_app'),
}
