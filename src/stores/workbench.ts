import { reactive, readonly } from 'vue'
import { normalizeNoteEditorMode } from '../utils/noteEditorMode'
import { compareByOrder, groupOf } from '../utils/todoSchedule'
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
  type ResourceSubcategory,
  type SudaCustomModuleConfig,
  type Snippet,
  type Sticky,
  type SystemInfo,
  type Tag,
  type Todo,
  type RepeatRuleInput,
  type TodoOccurrence,
  type TodoTag,
  type TodoTagLink,
  type WeatherCurrent,
} from '../api/tauri'

// 浏览器预览环境的兜底默认值；真实默认由 Rust 端 shortcut.rs 决定
const IS_MAC_PREVIEW =
  typeof navigator !== 'undefined' &&
  (/Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform))
const DEFAULT_GLOBAL_SHORTCUT = IS_MAC_PREVIEW
  ? 'CommandOrControl+Shift+Space'
  : 'Ctrl+Shift+Space'
const DEFAULT_SEARCH_SHORTCUT = IS_MAC_PREVIEW ? 'CommandOrControl+K' : 'Ctrl+K'
const DEFAULT_CHAT_SHORTCUT = IS_MAC_PREVIEW ? 'CommandOrControl+Shift+K' : 'Ctrl+Shift+K'

interface StoreState {
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
  stickies: Sticky[]
  detached: DetachedSticky[]
  countdowns: Countdown[]
  snippets: Snippet[]
  tags: Tag[]
  /** 待办标签定义（与笔记标签 tags 是两套独立定义） */
  todoTags: TodoTag[]
  /** 待办-标签关联（前端构建筛选映射用） */
  todoTagLinks: TodoTagLink[]
  /** 速达小类定义（ADR 0012：各大类一套、单归属；category 为 null = 未归类） */
  resourceSubcategories: ResourceSubcategory[]
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
  todoTags: [],
  todoTagLinks: [],
  resourceSubcategories: [],
  config: {
    theme_mode: 'light',
    theme_preset: 'indigo',
    accent_color: null,
    wallpaper_path: '',
    wallpaper_blur: true,
    wallpaper_veil: 0.3,
    wallpaper_immersive: false,
    glass_opacity: 1,
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
    notice_duration_ms: 5000,
    online_enabled: true,
    webview_mem_low_on_hide: true,
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
    chat_window_mode: false,
    chat_window_width: 460,
    chat_window_height: 640,
    chat_window_x: null,
    chat_window_y: null,
    chat_window_pinned: false,
    clipboard_shortcut: IS_MAC_PREVIEW ? 'CommandOrControl+Alt+V' : 'Ctrl+`',
    search_shortcut: DEFAULT_SEARCH_SHORTCUT,
    chat_shortcut: DEFAULT_CHAT_SHORTCUT,
    global_shortcut_enabled: true,
    clipboard_shortcut_enabled: true,
    search_shortcut_enabled: true,
    chat_shortcut_enabled: true,
    clipboard_max_items: 500,
    clipboard_ttl_days: 7,
    clipboard_paused: false,
    clipboard_paste_method: 'auto',
    suda_web_open_mode: 'panel',
    suda_custom_modules: [],
    suda_panel_toolbar: false,
    clipboard_image_enabled: true,
    clipboard_file_enabled: true,
    font_scale: 1,
    font_sticky: 1,
    font_notes: 1,
    font_prompt: 1,
    font_todo: 1,
    note_editor_mode: 'wysiwyg',
    runtime_strategy: 'auto',
    service_auto_trust: false,
    sidebar_extensions: [],
    extension_open_modes: {},
    extension_link_modes: {},
    extension_row_click: 'detail',
    run_at_startup: false,
    auto_update_enabled: true,
    update_interval_hours: 4,
    skipped_update_version: '',
    update_snooze_until_ms: 0,
    floating_ball_enabled: true,
    floating_ball_auto_hide: true,
    floating_ball_with_main: false,
    floating_ball_buttons: [
      'view:dashboard',
      'view:notes',
      'view:suda',
      'act:search',
      'act:clipboard',
      'view:settings',
    ],
    floating_ball_x: null,
    floating_ball_y: null,
    floating_ball_idle_spin: false,
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
    // 待办标签与关联单独拉（不进 get_initial_data：老库/老版本兼容面更小）
    void refreshTodoTags()
    // 速达小类单独拉（同上）
    void refreshSubcategories()
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

  /** 拖拽排序：整表按传入 id 顺序写 sort_order（ids[i] → i），本地乐观更新 + 后端持久化 */
  async function reorderResources(ids: number[]) {
    const rank = new Map(ids.map((id, i) => [id, i]))
    state.resources = state.resources
      .map((r) => ({ ...r, sort_order: rank.get(r.id) ?? Number.MAX_SAFE_INTEGER }))
      .sort((a, b) => a.sort_order - b.sort_order)
      .map((r, i) => ({ ...r, sort_order: i }))
    if (isTauri()) await tauriApi.reorderResources(ids)
  }

  /**
   * 打开资源。网页条目按 `suda_web_open_mode` 分流（ADR 0011）：
   * - panel → 派发 CustomEvent 交 index.vue 切到内嵌面板视图（store 不持有视图状态；
   *   最近使用由 suda_panel_show 在后端写）
   * - window → 独立应用内浏览器窗口池（后端 suda_browser_open 写最近使用）
   * 应用/文件维持系统路径 launch_resource。
   */
  async function launchResource(id: number) {
    const r = state.resources.find((x) => x.id === id)
    if (r && r.kind === 'web' && isTauri()) {
      if (state.config.suda_web_open_mode === 'window') {
        await tauriApi.sudaBrowserOpen(id)
        r.last_launched_at = new Date().toISOString()
        return
      }
      window.dispatchEvent(
        new CustomEvent('suda-open-web-panel', { detail: { id, url: r.target, name: r.name } }),
      )
      // 与 window 分支同口径：后端 suda_panel_show 落库，这里同步本地时间戳，
      // 否则「常用」/最近使用要等下次刷新才重排
      r.last_launched_at = new Date().toISOString()
      return
    }
    await tauriApi.launchResource(id)
    if (r) r.last_launched_at = new Date().toISOString()
  }

  /** 用指定浏览器打开网页资源（browserExe 来自 listInstalledBrowsers） */
  async function openResourceInBrowser(id: number, browserExe: string) {
    await tauriApi.openUrlWithBrowser(id, browserExe)
    const r = state.resources.find((x) => x.id === id)
    if (r) r.last_launched_at = new Date().toISOString()
  }

  /** 以管理员身份启动「程序」资源（UAC 确认；网页/文件由后端拒绝） */
  async function launchResourceAsAdmin(id: number) {
    await tauriApi.launchResourceAsAdmin(id)
    const r = state.resources.find((x) => x.id === id)
    if (r) r.last_launched_at = new Date().toISOString()
  }

  // ---- 速达小类（ADR 0012）----
  async function refreshSubcategories() {
    if (!isTauri()) return
    state.resourceSubcategories = await tauriApi.listSubcategories()
  }

  function subcategoriesOf(kind: 'app' | 'web' | 'file'): ResourceSubcategory[] {
    return state.resourceSubcategories.filter((s) => s.kind === kind)
  }

  /** 大类的默认小类名：is_default 优先，否则排序最前；该大类还没有小类时 null（未归类） */
  function defaultSubcategoryName(kind: 'app' | 'web' | 'file'): string | null {
    const subs = subcategoriesOf(kind)
    if (!subs.length) return null
    return (subs.find((s) => s.is_default) ?? subs[0]).name
  }

  async function addSubcategory(kind: 'app' | 'web' | 'file', name: string) {
    const sub = await tauriApi.createSubcategory(kind, name)
    state.resourceSubcategories.push(sub)
    return sub
  }

  /** 改名级联：后端单事务同步资源条目，本地按同口径推演 */
  async function editSubcategory(id: number, name: string) {
    await tauriApi.renameSubcategory(id, name)
    const sub = state.resourceSubcategories.find((s) => s.id === id)
    if (!sub) return
    const old = sub.name
    sub.name = name
    for (const r of state.resources) {
      if (r.kind === sub.kind && r.category === old) r.category = name
    }
  }

  /** 删除小类：条目改挂默认小类（删默认时按排序最前晋升，与后端口径一致）；删空回未归类 */
  async function removeSubcategory(id: number) {
    const sub = state.resourceSubcategories.find((s) => s.id === id)
    await tauriApi.deleteSubcategory(id)
    state.resourceSubcategories = state.resourceSubcategories.filter((s) => s.id !== id)
    if (!sub) return
    const subs = subcategoriesOf(sub.kind)
    const fallback = subs.find((s) => s.is_default) ?? subs[0] ?? null
    for (const r of state.resources) {
      if (r.kind === sub.kind && r.category === sub.name) {
        r.category = fallback ? fallback.name : null
      }
    }
  }

  async function reorderSubcategories(kind: 'app' | 'web' | 'file', ids: number[]) {
    const rank = new Map(ids.map((id, i) => [id, i]))
    state.resourceSubcategories = state.resourceSubcategories
      .map((s) => (s.kind === kind ? { ...s, sort_order: rank.get(s.id) ?? s.sort_order } : s))
      .sort((a, b) => a.sort_order - b.sort_order)
    if (isTauri()) await tauriApi.reorderSubcategories(kind, ids)
  }

  async function setDefaultSubcategory(id: number) {
    await tauriApi.setDefaultSubcategory(id)
    const target = state.resourceSubcategories.find((s) => s.id === id)
    if (!target) return
    for (const s of state.resourceSubcategories) {
      if (s.kind === target.kind) s.is_default = s.id === id
    }
  }

  /** 网页默认打开方式（ADR 0011：panel=内嵌面板 / window=独立窗口） */
  async function setSudaWebOpenMode(mode: 'panel' | 'window') {
    state.config.suda_web_open_mode = mode
    if (!isTauri()) return
    // 专属命令自带校验 + 配置锁落盘，不必再 saveConfig 整体写一遍（双写已去）
    await tauriApi.setSudaWebOpenMode(mode)
  }

  /** 显式以独立应用内浏览器窗口打开网页资源（右键菜单，绕过默认打开方式） */
  async function openResourceInWindow(id: number) {
    await tauriApi.sudaBrowserOpen(id)
    const r = state.resources.find((x) => x.id === id)
    if (r) r.last_launched_at = new Date().toISOString()
  }

  /** 显式以内嵌面板打开网页资源（与 launchResource 的 panel 分流同一条事件通道） */
  function openWebPanel(id: number) {
    const r = state.resources.find((x) => x.id === id)
    if (!r || r.kind !== 'web') return
    window.dispatchEvent(
      new CustomEvent('suda-open-web-panel', { detail: { id, url: r.target, name: r.name } }),
    )
  }

  /** 工作台「自定义速达」槽位内容配置（suda1..suda4）：未配置过返回 undefined */
  function sudaCustomConfigOf(id: string): SudaCustomModuleConfig | undefined {
    return state.config.suda_custom_modules?.find((m) => m.id === id)
  }

  /** 保存「自定义速达」槽位配置（upsert；用户可编辑项，随 config 整体落盘） */
  async function saveSudaCustomModule(cfg: SudaCustomModuleConfig) {
    const list = state.config.suda_custom_modules
    const i = list.findIndex((m) => m.id === cfg.id)
    if (i >= 0) list[i] = cfg
    else list.push(cfg)
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 内嵌面板工具栏显隐（默认不显示；隐藏时整个面板区域只渲染网页） */
  async function setSudaPanelToolbar(v: boolean) {
    state.config.suda_panel_toolbar = v
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
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
  // 浏览器预览兜底 id：Date.now() 会因同毫秒创建父子待办而撞 id，追加序列保证唯一
  let previewTodoSeq = 0
  async function createTodo(
    title: string,
    parentId: number | null = null,
    createdAt?: string,
  ) {
    const t = isTauri()
      ? await tauriApi.createTodo(title, parentId, createdAt)
      : {
          id: Date.now() + ++previewTodoSeq,
          title,
          done: false,
          priority: 0,
          created_at: createdAt ?? new Date().toISOString(),
          updated_at: new Date().toISOString(),
          completed_at: null,
          due_at: null,
          remind_at: null,
          remind_fired: false,
          parent_id: parentId,
          sort_order: null,
          description: '',
          pinned: false,
          repeat_mode: 'once' as const,
          repeat_every: null,
          repeat_unit: null,
          repeat_weekdays: null,
          repeat_month_day: null,
          repeat_month_nth: null,
          repeat_end_mode: null,
          repeat_end_at: null,
          repeat_count: null,
          repeat_done_count: 0,
          repeat_last_done_at: null,
        }
    state.todos.push(t)
    // 顶级待办落入「手动排序过」的分组时（组内已有 sort_order 非空条目），
    // 以 [新条目, ...组内原序] 整组重写排序位，让新建条目维持「新的在最上」直觉；
    // 组内全部未排序则不用管，创建时间倒序天然置顶。批量创建（序号拆分）并发补值
    // 出现的次序抖动与原有「新条目置顶、组内倒序」默认行为一致。
    // 置顶条目经 groupOf 独立成「置顶」组，不会出现在日期组的 peers 里——
    // 新建（未置顶）条目的补位永远排不到置顶条目上面。
    if (isTauri() && t.parent_id == null) {
      const now = new Date()
      const group = groupOf(t, now)
      const peers = state.todos
        .filter((x) => x.parent_id == null && x.id !== t.id && groupOf(x, now) === group)
        .sort(compareByOrder)
      if (peers.some((x) => x.sort_order != null)) {
        void assignTodoOrder([t.id, ...peers.map((x) => x.id)])
      }
    }
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
    const wasPriority = state.todos[i].priority
    let updated: Todo
    if (isTauri()) {
      updated = await tauriApi.updateTodo(id, title, priority)
      state.todos[i] = updated
    } else {
      const cur = state.todos[i]
      updated = { ...cur, title, priority, updated_at: new Date().toISOString() }
      state.todos[i] = updated
    }
    // 优先级与「置顶」联动（跟手动置顶同一状态，保证观感与行为一致）：
    // 切到「紧急」(2) → 自动置顶并排到「置顶」组最前；从紧急降下来 → 取消自动置顶。
    if (priority === 2 && wasPriority !== 2) await promoteUrgent(id)
    else if (wasPriority === 2 && priority !== 2) await demoteUrgent(id)
    return updated
  }

  /**
   * 优先级切成「紧急」时自动置顶：直接复用 `pinned` 字段（与手动置顶同一状态），
   * 再把自己的排序位排到「置顶」组最前。只处理未完成的顶级待办（子项 / 已完成不参与）。
   */
  async function promoteUrgent(id: number) {
    const t = state.todos.find((x) => x.id === id)
    if (!t || t.parent_id != null || t.done) return
    if (!t.pinned) await setTodoPinned(id, true)
    const peers = state.todos
      .filter((x) => x.parent_id == null && !x.done && x.id !== id && x.pinned)
      .sort(compareByOrder)
    await assignTodoOrder([id, ...peers.map((x) => x.id)])
  }

  /** 从「紧急」降级时取消自动置顶（仅当当前处于置顶态，避免无谓写库） */
  async function demoteUrgent(id: number) {
    const t = state.todos.find((x) => x.id === id)
    if (!t || t.parent_id != null || !t.pinned) return
    await setTodoPinned(id, false)
  }

  async function deleteTodo(id: number) {
    if (isTauri()) await tauriApi.deleteTodo(id)
    // 后端级联删除子待办，这里同步移出本地状态
    state.todos = state.todos.filter((t) => t.id !== id && t.parent_id !== id)
  }

  /** 设置截止/提醒时刻（毫秒时间戳；null 即清除） */
  async function scheduleTodo(id: number, dueAt: number | null, remindAt: number | null) {
    const i = state.todos.findIndex((t) => t.id === id)
    if (i < 0) return null
    if (isTauri()) {
      const updated = await tauriApi.scheduleTodo(id, dueAt, remindAt)
      state.todos[i] = updated
      return updated
    }
    // 截止日期决定分组归属，换组后原手动顺序失效，同步清空排序位
    const updated = { ...state.todos[i], due_at: dueAt, remind_at: remindAt, remind_fired: false, sort_order: null, updated_at: new Date().toISOString() }
    state.todos[i] = updated
    return updated
  }

  /** 按传入顺序写入手动排序位：本地乐观更新 + 后端持久化（ids[i] → sort_order i+1） */
  async function assignTodoOrder(ids: number[]) {
    const rank = new Map(ids.map((id, i) => [id, i + 1]))
    state.todos = state.todos.map((t) => {
      const r = rank.get(t.id)
      return r == null ? t : { ...t, sort_order: r }
    })
    if (isTauri()) await tauriApi.reorderTodoOrders(ids)
  }

  /** 拖拽排序落库：前端按分组计算完整顺序后调用 */
  function reorderTodos(ids: number[]) {
    return assignTodoOrder(ids)
  }

  /**
   * 跨父拖拽落库：把子待办改挂到另一个顶级父待办，并按 `orderedIds` 重写目标父下的子项排序。
   * 本地乐观更新（改 parent_id + 目标父下 sort_order），后端失败则整批回滚并抛出，由调用方提示。
   */
  async function moveTodoChild(id: number, newParentId: number, orderedIds: number[]) {
    const snapshot = state.todos.map((t) => ({
      id: t.id,
      parent_id: t.parent_id,
      sort_order: t.sort_order,
    }))
    const rank = new Map(orderedIds.map((x, i) => [x, i + 1]))
    state.todos = state.todos.map((t) => {
      if (t.id === id) return { ...t, parent_id: newParentId }
      const r = rank.get(t.id)
      return r == null ? t : { ...t, sort_order: r }
    })
    if (!isTauri()) return
    try {
      await tauriApi.moveTodoChild(id, newParentId, orderedIds)
    } catch (e) {
      const byId = new Map(snapshot.map((s) => [s.id, s]))
      state.todos = state.todos.map((t) => {
        const s = byId.get(t.id)
        return s ? { ...t, parent_id: s.parent_id, sort_order: s.sort_order } : t
      })
      throw e
    }
  }

  /** 待办浮窗等外部修改后刷新列表 */
  async function refreshTodos() {
    if (!isTauri()) return
    state.todos = await tauriApi.listTodos()
  }

  // ---- 待办升级：描述 / 置顶 / 周期 / 标签 ----

  function replaceTodo(updated: Todo) {
    const i = state.todos.findIndex((t) => t.id === updated.id)
    if (i >= 0) state.todos[i] = updated
    return updated
  }

  /** 设置描述（轻量 Markdown） */
  async function setTodoDescription(id: number, description: string) {
    if (!isTauri()) {
      const cur = state.todos.find((t) => t.id === id)
      return cur ? replaceTodo({ ...cur, description }) : null
    }
    return replaceTodo(await tauriApi.setTodoDescription(id, description))
  }

  /** 置顶开关：置顶条目脱离日期分组，固定排在列表最顶部「置顶」区 */
  async function setTodoPinned(id: number, pinned: boolean) {
    if (!isTauri()) {
      const cur = state.todos.find((t) => t.id === id)
      return cur ? replaceTodo({ ...cur, pinned }) : null
    }
    return replaceTodo(await tauriApi.setTodoPinned(id, pinned))
  }

  /** 写入周期规则（规则由 Rust 侧唯一实现，这里只落库） */
  async function setTodoRepeat(id: number, rule: RepeatRuleInput) {
    if (!isTauri()) {
      const cur = state.todos.find((t) => t.id === id)
      return cur
        ? replaceTodo({
            ...cur,
            repeat_mode: rule.mode,
            repeat_every: rule.every,
            repeat_unit: rule.unit,
            repeat_weekdays: rule.weekdays,
            repeat_month_day: rule.month_day,
            repeat_month_nth: rule.month_nth,
            repeat_end_mode: rule.end_mode,
            repeat_end_at: rule.end_at,
            repeat_count: rule.count,
          })
        : null
    }
    return replaceTodo(await tauriApi.setTodoRepeat(id, rule))
  }

  /** 周期待办「完成本轮」：日期滚到下一轮（子待办由后端复位） */
  async function completeTodoRecurring(id: number) {
    if (!isTauri()) return null
    const updated = replaceTodo(await tauriApi.completeTodoRecurring(id))
    // 后端把子待办复位为未完成，本地同步（避免子项仍显示勾选）
    for (const k of state.todos.filter((t) => t.parent_id === id && t.done)) {
      k.done = false
      k.completed_at = null
    }
    return updated
  }

  /** 撤销「完成本轮」：日期滚回上一轮 */
  async function undoTodoRecurring(id: number) {
    if (!isTauri()) return null
    return replaceTodo(await tauriApi.undoTodoRecurring(id))
  }

  /** 展开区间内的周期待办虚拟实例（日历渲染用；规则在 Rust 侧） */
  async function expandTodoOccurrences(fromMs: number, toMs: number): Promise<TodoOccurrence[]> {
    if (!isTauri()) return []
    return tauriApi.expandTodoOccurrences(fromMs, toMs)
  }

  /** 待办标签定义 + 关联（外部改动后也可手动刷新） */
  async function refreshTodoTags() {
    if (!isTauri()) return
    const [tags, links] = await Promise.all([
      tauriApi.listTodoTags().catch(() => [] as TodoTag[]),
      tauriApi.listTodoTagLinks().catch(() => [] as TodoTagLink[]),
    ])
    state.todoTags = tags
    state.todoTagLinks = links
  }

  async function createTodoTag(name: string, color = '') {
    if (!isTauri()) {
      const tag: TodoTag = {
        id: Date.now(),
        name,
        color,
        sort_order: state.todoTags.length,
        created_at: new Date().toISOString(),
      }
      state.todoTags.push(tag)
      return tag
    }
    const tag = await tauriApi.createTodoTag(name, color)
    await refreshTodoTags()
    return tag
  }

  async function updateTodoTag(id: number, name: string, color = '') {
    if (!isTauri()) {
      const i = state.todoTags.findIndex((t) => t.id === id)
      if (i >= 0) state.todoTags[i] = { ...state.todoTags[i], name, color }
      return
    }
    await tauriApi.updateTodoTag(id, name, color)
    await refreshTodoTags()
  }

  async function deleteTodoTag(id: number) {
    if (!isTauri()) {
      state.todoTags = state.todoTags.filter((t) => t.id !== id)
      state.todoTagLinks = state.todoTagLinks.filter((l) => l.tag_id !== id)
      return
    }
    await tauriApi.deleteTodoTag(id)
    await refreshTodoTags()
  }

  /** 全量设置某条待办的标签 */
  async function setTodoTags(id: number, tagIds: number[]) {
    if (!isTauri()) {
      state.todoTagLinks = [
        ...state.todoTagLinks.filter((l) => l.todo_id !== id),
        ...tagIds.map((tag_id) => ({ todo_id: id, tag_id })),
      ]
      return
    }
    await tauriApi.setTodoTags(id, tagIds)
    state.todoTagLinks = [
      ...state.todoTagLinks.filter((l) => l.todo_id !== id),
      ...tagIds.map((tag_id) => ({ todo_id: id, tag_id })),
    ]
  }

  /** 某条待办的标签 id 列表 */
  function todoTagIds(id: number): number[] {
    return state.todoTagLinks.filter((l) => l.todo_id === id).map((l) => l.tag_id)
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
    const ok = await tauriApi.focusDetachedSticky(slot)
    // 聚焦失败 = 后端已无该浮窗（空内容关闭浮窗时记录即被删除）：
    // 清掉本地幻影记录，让便签卡的「脱离」按钮从「已脱离」恢复为可脱离
    if (!ok) state.detached = state.detached.filter((x) => x.slot !== slot)
    return ok
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

  async function setWallpaper(path: string) {
    state.config.wallpaper_path = path
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setWallpaperBlur(value: boolean) {
    state.config.wallpaper_blur = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setWallpaperVeil(value: number) {
    state.config.wallpaper_veil = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setWallpaperImmersive(value: boolean) {
    state.config.wallpaper_immersive = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  async function setGlassOpacity(value: number) {
    state.config.glass_opacity = value
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

  async function setSearchShortcut(value: string) {
    state.config.search_shortcut = value
    if (!isTauri()) return value
    const saved = await tauriApi.setSearchShortcut(value)
    state.config.search_shortcut = saved
    return saved
  }

  async function setChatShortcut(value: string) {
    state.config.chat_shortcut = value
    if (!isTauri()) return value
    const saved = await tauriApi.setChatShortcut(value)
    state.config.chat_shortcut = saved
    return saved
  }

  /** 启用/禁用某个全局快捷键：禁用 = 后端注销热键但保留键值（重开即恢复），失败回滚内存状态 */
  async function setShortcutEnabled(
    kind: 'main' | 'clipboard' | 'search' | 'chat',
    enabled: boolean,
  ) {
    const key = (
      {
        main: 'global_shortcut_enabled',
        clipboard: 'clipboard_shortcut_enabled',
        search: 'search_shortcut_enabled',
        chat: 'chat_shortcut_enabled',
      } as const
    )[kind]
    const prev = state.config[key]
    state.config[key] = enabled
    if (!isTauri()) return
    try {
      await tauriApi.setShortcutEnabled(kind, enabled)
    } catch (e) {
      state.config[key] = prev
      throw e
    }
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

  /** 右下角通知弹窗驻留时长（毫秒，1–60 秒；通知窗按后端下发的值倒计时） */
  async function setNoticeDuration(value: number) {
    state.config.notice_duration_ms = Math.min(60000, Math.max(1000, Math.round(value)))
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

  /** 仅刷新配置（AI 对话独立窗唤起用）：get_initial_data 会全量拉业务数据，独立窗只要 config */
  async function refreshConfig() {
    if (!isTauri()) return
    state.config = await tauriApi.getUiConfig()
  }

  /** AI 对话形态：独立窗口 / 主窗内嵌抽屉（互斥）。走专用命令——后端据此建窗/隐窗；
   *  失败时回滚本地状态再抛出，避免「界面已切换、后端没落盘」的漂移（同 togglePin 口径） */
  async function setChatWindowMode(value: boolean) {
    const prev = state.config.chat_window_mode
    state.config.chat_window_mode = value
    if (!isTauri()) return
    try {
      await tauriApi.chatWindowSaveMode(value)
    } catch (e) {
      state.config.chat_window_mode = prev
      throw e
    }
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

  /** 速记编辑器模式（实时预览 / 分屏预览 / 源码），按用户记住 */
  async function setNoteEditorMode(value: string) {
    const mode = normalizeNoteEditorMode(value)
    if (state.config.note_editor_mode === mode) return
    state.config.note_editor_mode = mode
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** service 扩展运行时策略：auto（自动检测）/ builtin（始终内置）/ system（始终系统） */
  async function setRuntimeStrategy(value: 'auto' | 'builtin' | 'system') {
    state.config.runtime_strategy = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 全局自动信任 service 扩展：新装/更新版本无需逐个「去授权」即可运行本地后端。
   *  单独关掉某扩展后端的选择仍优先于本开关（后端 permission_granted 判定） */
  async function setServiceAutoTrust(enabled: boolean) {
    state.config.service_auto_trust = enabled
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

  /** 扩展链接打开方式：inapp（应用内浏览器，默认）/ browser（系统默认浏览器） */
  function setExtensionLinkMode(id: string, mode: string) {
    const modes = state.config.extension_link_modes ?? {}
    state.config.extension_link_modes = { ...modes, [id]: mode }
    if (!isTauri()) return
    void tauriApi.saveConfig(state.config)
  }

  /** 扩展中心列表点击行为：detail（默认，点行看详情）/ open（点行直接打开、右侧 ⋯ 看详情） */
  async function setExtensionRowClick(value: 'detail' | 'open') {
    state.config.extension_row_click = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  // ---- 开机自启动 ----
  /** 应用开机自启动开关，失败回滚内存状态 */
  async function setRunAtStartup(enabled: boolean) {
    const prevEnabled = state.config.run_at_startup
    state.config.run_at_startup = enabled
    if (!isTauri()) return
    try {
      await tauriApi.setRunAtStartup(enabled)
      await tauriApi.saveConfig(state.config)
    } catch (e) {
      state.config.run_at_startup = prevEnabled
      throw e
    }
  }

  // ---- 桌面悬浮球（ADR 0004）：开关/贴边隐藏/同显/按钮统一经 floating_ball_save_settings 落盘，
  // 不走 saveConfig（后端对悬浮球字段做磁盘合并保护，见 commands::save_config） ----
  async function setFloatingBallEnabled(value: boolean) {
    state.config.floating_ball_enabled = value
    if (!isTauri()) return
    await tauriApi.floatingBallSaveSettings(
      value,
      state.config.floating_ball_auto_hide,
      state.config.floating_ball_with_main,
      state.config.floating_ball_buttons,
      state.config.floating_ball_idle_spin,
    )
  }

  async function setFloatingBallAutoHide(value: boolean) {
    state.config.floating_ball_auto_hide = value
    if (!isTauri()) return
    await tauriApi.floatingBallSaveSettings(
      state.config.floating_ball_enabled,
      value,
      state.config.floating_ball_with_main,
      state.config.floating_ball_buttons,
      state.config.floating_ball_idle_spin,
    )
  }

  async function setFloatingBallWithMain(value: boolean) {
    state.config.floating_ball_with_main = value
    if (!isTauri()) return
    await tauriApi.floatingBallSaveSettings(
      state.config.floating_ball_enabled,
      state.config.floating_ball_auto_hide,
      value,
      state.config.floating_ball_buttons,
      state.config.floating_ball_idle_spin,
    )
  }

  async function setFloatingBallButtons(value: string[]) {
    // 去重 + 上限钳制，与后端 floating_ball::MAX_BUTTONS 对齐
    state.config.floating_ball_buttons = [...new Set(value)].slice(0, 8)
    if (!isTauri()) return
    await tauriApi.floatingBallSaveSettings(
      state.config.floating_ball_enabled,
      state.config.floating_ball_auto_hide,
      state.config.floating_ball_with_main,
      state.config.floating_ball_buttons,
      state.config.floating_ball_idle_spin,
    )
  }

  async function setFloatingBallIdleSpin(value: boolean) {
    state.config.floating_ball_idle_spin = value
    if (!isTauri()) return
    await tauriApi.floatingBallSaveSettings(
      state.config.floating_ball_enabled,
      state.config.floating_ball_auto_hide,
      state.config.floating_ball_with_main,
      state.config.floating_ball_buttons,
      value,
    )
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
    if (!isTauri() || !state.config.online_enabled) return false
    try {
      const ok = await tauriApi.checkConnectivity()
      if (!state.config.online_enabled) return false
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

  /** 隐藏窗口降低内存占用（webview_mem：隐藏 Low / 显示 Normal） */
  async function setWebviewMemLowOnHide(value: boolean) {
    state.config.webview_mem_low_on_hide = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
  }

  /** 应用自动升级总开关 */
  async function setAutoUpdateEnabled(value: boolean) {
    state.config.auto_update_enabled = value
    if (!isTauri()) return
    await tauriApi.saveConfig(state.config)
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
    if (onlineTimer || !isTauri() || !state.config.online_enabled) return
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
    reorderResources,
    launchResource,
    launchResourceAsAdmin,
    openResourceInBrowser,
    refreshSubcategories,
    subcategoriesOf,
    defaultSubcategoryName,
    addSubcategory,
    editSubcategory,
    removeSubcategory,
    reorderSubcategories,
    setDefaultSubcategory,
    setSudaWebOpenMode,
    openResourceInWindow,
    openWebPanel,
    sudaCustomConfigOf,
    saveSudaCustomModule,
    setSudaPanelToolbar,
    addNote,
    saveNote,
    removeNote,
    refreshNotes,
    searchAll,
    createTodo,
    toggleTodo,
    updateTodo,
    deleteTodo,
    scheduleTodo,
    reorderTodos,
    moveTodoChild,
    refreshTodos,
    setTodoDescription,
    setTodoPinned,
    setTodoRepeat,
    completeTodoRecurring,
    undoTodoRecurring,
    expandTodoOccurrences,
    refreshTodoTags,
    createTodoTag,
    updateTodoTag,
    deleteTodoTag,
    setTodoTags,
    todoTagIds,
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
    setWallpaper,
    setWallpaperBlur,
    setWallpaperVeil,
    setWallpaperImmersive,
    setGlassOpacity,
    setSidebarToggle,
    setAlwaysOnTop,
    setGlobalShortcut,
    setSearchShortcut,
    setChatShortcut,
    setShortcutEnabled,
    setDashboardMidContent,
    setDashboardLayout,
    setCountdownSound,
    setNoticeDuration,
    setClockQuote,
  setChatModels,
  setChatPanelOpacity,
  setChatPanelSide,
  setChatWindowMode,
  refreshConfig,
    setFontScale,
    setModuleFontScale,
    setNoteEditorMode,
    setRuntimeStrategy,
    setServiceAutoTrust,
    setSidebarExtension,
    setSidebarExtensionBulk,
    setExtensionOpenMode,
    setExtensionLinkMode,
    setExtensionRowClick,
    setRunAtStartup,
    setFloatingBallEnabled,
    setFloatingBallAutoHide,
    setFloatingBallWithMain,
    setFloatingBallButtons,
    setFloatingBallIdleSpin,
    setClipboardShortcut,
    setClipboardPaused,
    setClipboardRetention,
    setClipboardMediaEnabled,
    refreshSystemInfo,
    checkOnline,
    refreshWeather,
    refreshQuote,
    setOnlineEnabled,
    setWebviewMemLowOnHide,
    setAutoUpdateEnabled,
    setQuoteSource,
    setWeatherCity,
    locateWeatherByIp,
    startOnlineMonitor,
    stopOnlineMonitor,
  }
}
