import { reactive, readonly } from 'vue'
import {
  tauriApi,
  isTauri,
  type AppConfig,
  type ChatModelConfig,
  type Countdown,
  type DetachedSticky,
  type GeoLocation,
  type Note,
  type Quote,
  type Resource,
  type Snippet,
  type Sticky,
  type SystemInfo,
  type Tag,
  type Todo,
  type WeatherCurrent,
} from '../api/tauri'

// 浏览器预览环境的兜底默认值；真实默认由 Rust 端 shortcut.rs 决定
const IS_MAC_PREVIEW =
  typeof navigator !== 'undefined' &&
  (/Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform))
const DEFAULT_GLOBAL_SHORTCUT = IS_MAC_PREVIEW
  ? 'CommandOrControl+Shift+Space'
  : 'Ctrl+Shift+Space'

interface StoreState {
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
  stickies: Sticky[]
  detached: DetachedSticky[]
  countdowns: Countdown[]
  snippets: Snippet[]
  tags: Tag[]
  config: AppConfig
  systemInfo: SystemInfo | null
  online: boolean
  weather: WeatherCurrent | null
  quote: Quote | null
  loaded: boolean
}

const state = reactive<StoreState>({
  resources: [],
  notes: [],
  todos: [],
  stickies: [],
  detached: [],
  countdowns: [],
  snippets: [],
  tags: [],
  config: {
    theme_mode: 'light',
    theme_preset: 'indigo',
    accent_color: null,
    sidebar_toggle: false,
    window: {
      width: 1400,
      height: 900,
      x: null,
      y: null,
      always_on_top: false,
    },
    global_shortcut: DEFAULT_GLOBAL_SHORTCUT,
    dashboard_mid_content: 'countdown',
    dashboard_layout: '',
    countdown_sound: false,
    clock_quote: '',
    online_enabled: true,
    weather_city: '',
    weather_lat: 0,
    weather_lng: 0,
    quote_source: 'online',
    chat_models: [],
    chat_panel_width: 420,
    chat_panel_open: false,
    chat_panel_side: 'right',
    chat_panel_height: 380,
    chat_panel_opacity: 1,
    whats_new_enabled: true,
    last_seen_version: '',
    clipboard_shortcut: IS_MAC_PREVIEW ? 'CommandOrControl+Alt+V' : 'Ctrl+`',
    clipboard_max_items: 500,
    clipboard_ttl_days: 7,
    clipboard_paused: false,
    clipboard_paste_method: 'auto',
    clipboard_image_enabled: true,
    clipboard_file_enabled: true,
    font_scale: 1,
    font_sticky: 1,
    font_notes: 1,
    font_prompt: 1,
    font_todo: 1,
    runtime_strategy: 'auto',
    sidebar_extensions: [],
    extension_open_modes: {},
    run_at_startup: false,
    run_at_startup_admin: false,
  },
  systemInfo: null,
  online: false,
  weather: null,
  quote: null,
  loaded: false,
})

export function useStore() {
  async function loadInitialData() {
    if (!isTauri()) return
    // get_initial_data 不含 snippets，并行单独拉取；后端命令未就绪时兜底为空列表
    const [data, snippets] = await Promise.all([
      tauriApi.getInitialData(),
      tauriApi.listSnippets().catch(() => [] as Snippet[]),
    ])
    state.resources = data.resources
    state.notes = data.notes
    state.todos = data.todos
    state.stickies = data.stickies
    state.detached = data.detached
    state.tags = data.tags
    state.config = data.config
    state.snippets = snippets
    state.countdowns = data.countdowns
    state.loaded = true
  }

  // ---- 提示词百宝箱 ----
  // 与后端 repo/snippet.rs 排序一致：置顶 → 复制次数 → 最近复制 → id 倒序
  function sortSnippets() {
    state.snippets.sort((a, b) => {
      if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1
      if (a.copy_count !== b.copy_count) return b.copy_count - a.copy_count
      if (a.last_copied_at !== b.last_copied_at) {
        return b.last_copied_at.localeCompare(a.last_copied_at)
      }
      return b.id - a.id
    })
  }

  function replaceSnippet(updated: Snippet) {
    const idx = state.snippets.findIndex((x) => x.id === updated.id)
    if (idx >= 0) state.snippets[idx] = updated
    else state.snippets.push(updated)
    sortSnippets()
  }

  function localSnippet(title: string, content: string): Snippet {
    const now = new Date().toISOString()
    return {
      id: Date.now(),
      title,
      content,
      is_pinned: false,
      copy_count: 0,
      last_copied_at: '',
      created_at: now,
      updated_at: now,
    }
  }

  async function loadSnippets() {
    if (!isTauri()) return
    state.snippets = await tauriApi.listSnippets()
  }

  async function addSnippet(title: string, content: string) {
    const s = isTauri()
      ? await tauriApi.createSnippet(title, content)
      : localSnippet(title, content)
    state.snippets.push(s)
    sortSnippets()
    return s
  }

  async function editSnippet(id: number, title: string, content: string) {
    const s = isTauri()
      ? await tauriApi.updateSnippet(id, title, content)
      : { ...(state.snippets.find((x) => x.id === id) ?? localSnippet(title, content)), title, content, updated_at: new Date().toISOString() }
    replaceSnippet(s)
    return s
  }

  async function removeSnippet(id: number) {
    if (isTauri()) await tauriApi.deleteSnippet(id)
    state.snippets = state.snippets.filter((x) => x.id !== id)
  }

  async function toggleSnippetPin(id: number) {
    if (!isTauri()) {
      const cur = state.snippets.find((x) => x.id === id)
      if (!cur) return null
      replaceSnippet({ ...cur, is_pinned: !cur.is_pinned, updated_at: new Date().toISOString() })
      return state.snippets.find((x) => x.id === id) ?? null
    }
    const updated = await tauriApi.toggleSnippetPin(id)
    replaceSnippet(updated)
    return updated
  }

  async function togglePromptFloat() {
    if (!isTauri()) return
    await tauriApi.togglePromptFloat()
  }

  async function toggleTodoFloat() {
    if (!isTauri()) return
    await tauriApi.toggleTodoFloat()
  }

  async function toggleFloatPin(label: string, value: boolean) {
    if (!isTauri()) return
    await tauriApi.toggleFloatPin(label, value)
  }

  async function recordSnippetCopy(id: number) {
    if (!isTauri()) {
      const cur = state.snippets.find((x) => x.id === id)
      if (!cur) return null
      const now = new Date().toISOString()
      replaceSnippet({ ...cur, copy_count: cur.copy_count + 1, last_copied_at: now, updated_at: now })
      return state.snippets.find((x) => x.id === id) ?? null
    }
    const updated = await tauriApi.recordSnippetCopy(id)
    replaceSnippet(updated)
    return updated
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
    const n = isTauri()
      ? await tauriApi.createNote(title)
      : { id: Date.now(), title, content: '', created_at: new Date().toISOString(), updated_at: new Date().toISOString() }
    state.notes.unshift(n)
    return n
  }

  async function saveNote(id: number, title: string, content: string) {
    const n = isTauri()
      ? await tauriApi.updateNote(id, title, content)
      : { id, title, content, created_at: new Date().toISOString(), updated_at: new Date().toISOString() }
    const idx = state.notes.findIndex((x) => x.id === id)
    if (idx >= 0) state.notes[idx] = n
    return n
  }

  async function removeNote(id: number) {
    if (isTauri()) await tauriApi.deleteNote(id)
    state.notes = state.notes.filter((x) => x.id !== id)
  }

  /** 剪贴板浮层等外部保存速记后，主窗口刷新笔记列表（仅拉元信息，轻量） */
  async function refreshNotes() {
    if (!isTauri()) return
    state.notes = await tauriApi.listNotes()
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

  /** 待办浮窗等外部修改后刷新列表 */
  async function refreshTodos() {
    if (!isTauri()) return
    state.todos = await tauriApi.listTodos()
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

  // ---- 便签脱离浮窗 ----
  /** 脱离：复制内容到浮窗并清空原卡；该卡已有浮窗则聚焦 */
  async function detachSticky(slot: number) {
    const d = await tauriApi.detachSticky(slot)
    const i = state.detached.findIndex((x) => x.slot === slot)
    if (i >= 0) state.detached[i] = d
    else state.detached.push(d)
    // 原卡已被清空，同步本地状态
    const si = state.stickies.findIndex((x) => x.slot === slot)
    if (si >= 0) state.stickies[si].content = ''
    return d
  }

  async function focusDetachedSticky(slot: number) {
    if (!isTauri()) return false
    return tauriApi.focusDetachedSticky(slot)
  }

  /** 浮窗输入保存（600ms 防抖由浮窗组件处理） */
  async function saveDetachedSticky(slot: number, content: string) {
    if (!isTauri()) return
    await tauriApi.saveDetachedSticky(slot, content)
  }

  async function toggleDetachedStickyPin(slot: number, value: boolean) {
    if (!isTauri()) return
    await tauriApi.toggleDetachedStickyPin(slot, value)
    const d = state.detached.find((x) => x.slot === slot)
    if (d) d.always_on_top = value
  }

  /** 还原到主面板：返回写入的槽位 */
  async function restoreDetachedSticky(slot: number) {
    const target = await tauriApi.restoreDetachedSticky(slot)
    state.detached = state.detached.filter((x) => x.slot !== slot)
    return target
  }

  /** 删除浮窗便签 */
  async function deleteDetachedSticky(slot: number) {
    if (isTauri()) await tauriApi.deleteDetachedSticky(slot)
    state.detached = state.detached.filter((x) => x.slot !== slot)
  }

  /** 收到后端 stickies-changed 事件时刷新（还原/删除后主窗口同步） */
  async function refreshStickies() {
    if (!isTauri()) return
    const [stickies, detached] = await Promise.all([
      tauriApi.listStickies(),
      tauriApi.getDetachedStickies().catch(() => [] as DetachedSticky[]),
    ])
    state.stickies = stickies
    state.detached = detached
  }

  // ---- 倒计时 ----
  function upsertCountdown(updated: Countdown) {
    const idx = state.countdowns.findIndex((x) => x.id === updated.id)
    if (idx >= 0) state.countdowns[idx] = updated
    else state.countdowns.push(updated)
  }

  async function addCountdown(payload: {
    name: string
    repeatMode: string
    endAt: number
    totalMs: number
    intervalMinutes?: number | null
  }) {
    const c = isTauri()
      ? await tauriApi.createCountdown(payload)
      : ({
          id: Date.now(),
          name: payload.name,
          repeat_mode: payload.repeatMode,
          end_at: payload.endAt,
          total_ms: payload.totalMs,
          interval_minutes: payload.intervalMinutes ?? null,
          paused: false,
          paused_remaining_ms: null,
          finished: false,
          floated: false,
          float_x: null,
          float_y: null,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        } as Countdown)
    upsertCountdown(c)
    return c
  }

  async function editCountdown(payload: {
    id: number
    name: string
    repeatMode: string
    endAt: number
    totalMs: number
    intervalMinutes?: number | null
  }) {
    const c = isTauri()
      ? await tauriApi.updateCountdown(payload)
      : ({
          ...(state.countdowns.find((x) => x.id === payload.id) ?? ({} as Countdown)),
          ...payload,
          repeat_mode: payload.repeatMode,
          end_at: payload.endAt,
          total_ms: payload.totalMs,
          interval_minutes: payload.intervalMinutes ?? null,
          paused: false,
          paused_remaining_ms: null,
          updated_at: new Date().toISOString(),
        } as Countdown)
    upsertCountdown(c)
    return c
  }

  async function removeCountdown(id: number) {
    if (isTauri()) await tauriApi.deleteCountdown(id)
    state.countdowns = state.countdowns.filter((x) => x.id !== id)
  }

  async function toggleCountdownPause(id: number) {
    if (!isTauri()) return null
    const cur = state.countdowns.find((x) => x.id === id)
    const c = cur?.paused
      ? await tauriApi.resumeCountdown(id)
      : await tauriApi.pauseCountdown(id)
    upsertCountdown(c)
    return c
  }

  async function floatCountdown(id: number) {
    if (!isTauri()) return null
    const c = await tauriApi.floatCountdown(id)
    upsertCountdown(c)
    return c
  }

  async function unfloatCountdown(id: number) {
    if (!isTauri()) return null
    const c = await tauriApi.unfloatCountdown(id)
    upsertCountdown(c)
    return c
  }

  async function refreshCountdowns() {
    if (!isTauri()) return
    state.countdowns = await tauriApi.listCountdowns()
  }

  // ---- 标签 ----
  async function createTag(name: string) {
    const t = isTauri()
      ? await tauriApi.createTag(name)
      : { id: Date.now(), name, created_at: new Date().toISOString() }
    if (!state.tags.some((x) => x.id === t.id)) state.tags.push(t)
    return t
  }

  async function deleteTag(id: number) {
    if (isTauri()) await tauriApi.deleteTag(id)
    state.tags = state.tags.filter((x) => x.id !== id)
  }

  // ---- 笔记-标签关联（列表筛选用） ----
  async function loadNoteTagsMap() {
    if (!isTauri()) return []
    return tauriApi.listNoteTags()
  }

  // ---- 配置 ----
  async function setThemeMode(mode: 'light' | 'dark' | 'system') {
    state.config.theme_mode = mode
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setThemePreset(preset: string) {
    state.config.theme_preset = preset
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setAccentColor(hex: string | null) {
    state.config.accent_color = hex
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

  /** 主页面「中上区块」显示内容：token/notes/todo/resources/countdown */
  async function setDashboardMidContent(value: string) {
    state.config.dashboard_mid_content = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 工作台自定义布局（placements JSON 数组字符串，经 config.json 落盘） */
  async function setDashboardLayout(value: string) {
    state.config.dashboard_layout = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 倒计时到点提示音开关 */
  async function setCountdownSound(value: boolean) {
    state.config.countdown_sound = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 侧边栏展开/收缩功能开关 */
  async function setSidebarToggle(value: boolean) {
    state.config.sidebar_toggle = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 时钟卡片语录（空串时 ClockCard 回退默认句子） */
  async function setClockQuote(value: string) {
    state.config.clock_quote = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** AI 模型配置同步进内存快照：save_chat_models 后调用，防止后续 saveConfig 用旧快照覆盖 */
  function setChatModels(models: ChatModelConfig[]) {
    state.config.chat_models = models
  }

  /** AI 对话面板透明度（0.5–1.0） */
  async function setChatPanelOpacity(value: number) {
    state.config.chat_panel_opacity = Math.min(1, Math.max(0.5, value))
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** AI 对话面板方位：left / right / top / bottom */
  async function setChatPanelSide(value: 'left' | 'right' | 'top' | 'bottom') {
    state.config.chat_panel_side = value
    if (!isTauri()) return
    await tauriApi.setChatPanelSide(value)
    await tauriApi.saveConfig(state.config)
  }

  /** 升级后弹窗显示更新说明（默认关闭） */
  async function setWhatsNewEnabled(value: boolean) {
    state.config.whats_new_enabled = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 字号缩放钳制到 0.85–1.30，保留 2 位小数 */
  function clampFontScale(value: number) {
    return Math.round(Math.min(1.3, Math.max(0.85, value)) * 100) / 100
  }

  /** 全局字体缩放（0.85–1.30） */
  async function setFontScale(value: number) {
    state.config.font_scale = clampFontScale(value)
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 单模块字体缩放：sticky / notes / prompt / todo */
  async function setModuleFontScale(
    module: 'sticky' | 'notes' | 'prompt' | 'todo',
    value: number,
  ) {
    const key = `font_${module}` as 'font_sticky' | 'font_notes' | 'font_prompt' | 'font_todo'
    state.config[key] = clampFontScale(value)
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** service 扩展运行时策略：auto（自动检测）/ builtin（始终内置）/ system（始终系统） */
  async function setRuntimeStrategy(value: 'auto' | 'builtin' | 'system') {
    state.config.runtime_strategy = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 固定/取消固定扩展到左侧栏：点击侧栏菜单即在主区打开对应扩展（view 形态） */
  function setSidebarExtension(id: string, pinned: boolean) {
    const cur = state.config.sidebar_extensions ?? []
    state.config.sidebar_extensions = pinned
      ? cur.includes(id)
        ? cur
        : [...cur, id]
      : cur.filter((x) => x !== id)
    if (!isTauri()) return
    void tauriApi.saveConfig(state.config)
  }

  /** 批量覆盖侧栏固定扩展列表（卸载后清理残留 id 用） */
  function setSidebarExtensionBulk(ids: string[]) {
    state.config.sidebar_extensions = [...ids]
    if (isTauri()) void tauriApi.saveConfig(state.config)
  }

  /** 扩展默认打开方式：view / window / drawer（侧栏点击等入口按此打开） */
  function setExtensionOpenMode(id: string, mode: string) {
    const modes = state.config.extension_open_modes ?? {}
    state.config.extension_open_modes = { ...modes, [id]: mode }
    if (!isTauri()) return
    void tauriApi.saveConfig(state.config)
  }

  // ---- 开机自启动 ----
  /** 应用开机自启动（enabled 总开关；admin 是否以管理员身份运行），失败回滚内存状态 */
  async function setRunAtStartup(enabled: boolean, admin: boolean) {
    const prevEnabled = state.config.run_at_startup
    const prevAdmin = state.config.run_at_startup_admin
    state.config.run_at_startup = enabled
    state.config.run_at_startup_admin = admin && enabled
    if (!isTauri()) return
    try {
      await tauriApi.setRunAtStartup(enabled, admin)
      await tauriApi.saveConfig(state.config)
    } catch (e) {
      state.config.run_at_startup = prevEnabled
      state.config.run_at_startup_admin = prevAdmin
      throw e
    }
  }

  // ---- 系统资源 ----
  async function refreshSystemInfo() {
    if (!isTauri()) return null
    state.systemInfo = await tauriApi.getSystemInfo()
    return state.systemInfo
  }

  // ---- 剪贴板历史 ----
  async function setClipboardShortcut(value: string) {
    state.config.clipboard_shortcut = value
    if (!isTauri()) return value
    const saved = await tauriApi.setClipboardShortcut(value)
    state.config.clipboard_shortcut = saved
    return saved
  }

  async function setClipboardPaused(value: boolean) {
    state.config.clipboard_paused = value
    if (!isTauri()) return
    await tauriApi.clipboardSetPaused(value)
    await tauriApi.saveConfig(state.config)
  }

  async function setClipboardRetention(maxItems: number, ttlDays: number) {
    // 与后端 set_clipboard_retention 的钳制范围对齐，避免 saveConfig 用未钳制值覆盖后端结果
    const clampedMax = Math.min(5000, Math.max(20, Math.round(maxItems)))
    const clampedTtl = Math.min(365, Math.max(1, Math.round(ttlDays)))
    state.config.clipboard_max_items = clampedMax
    state.config.clipboard_ttl_days = clampedTtl
    if (!isTauri()) return
    await tauriApi.setClipboardRetention(clampedMax, clampedTtl)
    await tauriApi.saveConfig(state.config)
  }

  async function setClipboardMediaEnabled(image: boolean, file: boolean) {
    state.config.clipboard_image_enabled = image
    state.config.clipboard_file_enabled = file
    if (!isTauri()) return
    await tauriApi.setClipboardMediaEnabled(image, file)
    await tauriApi.saveConfig(state.config)
  }

  // ---- 在线服务（天气 / 名言 / 连通性） ----
  let onlineTimer: ReturnType<typeof setInterval> | null = null
  let lastWeatherRefresh = 0
  let failStreak = 0

  /** 探测外网连通性并更新 state.online（滞回：连续 3 次失败才判离线，避免单次抖动） */
  async function checkOnline(): Promise<boolean> {
    if (!isTauri()) return false
    try {
      const ok = await tauriApi.checkConnectivity()
      if (ok) {
        failStreak = 0
        state.online = true
      } else {
        failStreak++
        if (failStreak >= 3) state.online = false
      }
      return state.online
    } catch {
      failStreak++
      if (failStreak >= 3) state.online = false
      return state.online
    }
  }

  /** 拉取天气：未开启联网 / 未配置城市 → 清空（天气卡隐藏）；网络失败保留旧值避免闪烁 */
  async function refreshWeather() {
    if (!isTauri()) return
    if (!state.config.online_enabled || !state.config.weather_lat || !state.config.weather_lng) {
      state.weather = null
      return
    }
    try {
      state.weather = await tauriApi.getWeather()
    } catch {
      // 网络失败：保留旧值，天气卡不因单次抖动闪烁
    }
  }

  /** 拉取名言（仅在线模式且开启联网时请求；失败静默，组件回退本地语料） */
  async function refreshQuote() {
    if (!isTauri()) return
    if (!state.config.online_enabled || state.config.quote_source !== 'online') return
    try {
      state.quote = await tauriApi.getQuote()
    } catch {
      // 静默：离线/失败时组件回退本地语料
    }
  }

  /** 联网功能总开关 */
  async function setOnlineEnabled(value: boolean) {
    state.config.online_enabled = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
    if (!value) {
      state.online = false
      state.weather = null
      stopOnlineMonitor()
    } else {
      startOnlineMonitor()
    }
  }

  /** 名言来源：online（在线 hitokoto）/ local（仅本地语料） */
  async function setQuoteSource(value: 'online' | 'local') {
    state.config.quote_source = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
    if (value === 'online') await refreshQuote()
  }

  /** 手动配城市：后端 geocoding 解析经纬度并缓存，随后刷新天气 */
  async function setWeatherCity(city: string): Promise<GeoLocation> {
    const loc = isTauri()
      ? await tauriApi.setWeatherCity(city)
      : { name: city, lat: 0, lng: 0 }
    state.config.weather_city = loc.name
    state.config.weather_lat = loc.lat
    state.config.weather_lng = loc.lng
    await refreshWeather()
    return loc
  }

  /** IP 自动定位并缓存经纬度，随后刷新天气 */
  async function locateWeatherByIp(): Promise<GeoLocation> {
    const loc = await tauriApi.locateWeatherByIp()
    state.config.weather_city = loc.name
    state.config.weather_lat = loc.lat
    state.config.weather_lng = loc.lng
    await refreshWeather()
    return loc
  }

  /** 启动在线状态监听：立即探测 + 每 60s 探测 + 天气每 30 分钟刷新 */
  function startOnlineMonitor() {
    if (onlineTimer || !isTauri()) return
    const tick = async () => {
      const prev = state.online
      const ok = await checkOnline()
      const now = Date.now()
      // 天气：首次或距上次刷新 ≥30 分钟
      if (ok && (state.weather === null || now - lastWeatherRefresh >= 30 * 60_000)) {
        lastWeatherRefresh = now
        await refreshWeather()
      }
      // 名言：首次上线 / 离线恢复在线时补拉
      if (ok && (state.quote === null || !prev)) {
        await refreshQuote()
      }
      if (!ok) {
        state.weather = null
      }
    }
    tick()
    onlineTimer = setInterval(tick, 60_000)
  }

  function stopOnlineMonitor() {
    if (onlineTimer) clearInterval(onlineTimer)
    onlineTimer = null
  }

  return {
    state: readonly(state),
    loadInitialData,
    loadSnippets,
    addSnippet,
    editSnippet,
    removeSnippet,
    toggleSnippetPin,
    recordSnippetCopy,
    togglePromptFloat,
    toggleTodoFloat,
    toggleFloatPin,
    addResource,
    editResource,
    removeResource,
    launchResource,
    addNote,
    saveNote,
    removeNote,
    refreshNotes,
    searchAll,
    createTodo,
    toggleTodo,
    updateTodo,
    deleteTodo,
    refreshTodos,
    saveSticky,
    detachSticky,
    focusDetachedSticky,
    saveDetachedSticky,
    toggleDetachedStickyPin,
    restoreDetachedSticky,
    deleteDetachedSticky,
    refreshStickies,
    addCountdown,
    editCountdown,
    removeCountdown,
    toggleCountdownPause,
    floatCountdown,
    unfloatCountdown,
    refreshCountdowns,
    createTag,
    deleteTag,
    loadNoteTagsMap,
    setThemeMode,
    setThemePreset,
    setAccentColor,
    setSidebarToggle,
    setAlwaysOnTop,
    setGlobalShortcut,
    setDashboardMidContent,
    setDashboardLayout,
    setCountdownSound,
  setClockQuote,
  setChatModels,
  setChatPanelOpacity,
  setChatPanelSide,
    setWhatsNewEnabled,
    setFontScale,
    setModuleFontScale,
    setRuntimeStrategy,
    setSidebarExtension,
    setSidebarExtensionBulk,
    setExtensionOpenMode,
    setRunAtStartup,
    setClipboardShortcut,
    setClipboardPaused,
    setClipboardRetention,
    setClipboardMediaEnabled,
    refreshSystemInfo,
    checkOnline,
    refreshWeather,
    refreshQuote,
    setOnlineEnabled,
    setQuoteSource,
    setWeatherCity,
    locateWeatherByIp,
    startOnlineMonitor,
    stopOnlineMonitor,
  }
}
