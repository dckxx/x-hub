<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import TitleBar from '../components/TitleBar.vue'
import SudaWebPanel from '../components/SudaWebPanel.vue'
import TodoCard from '../components/TodoCard.vue'
import TodoCalendarCard from '../components/TodoCalendarCard.vue'
import Suda from '../components/Suda.vue'
import NotesOverviewCard from '../components/NotesOverviewCard.vue'
import TodoOverviewCard from '../components/TodoOverviewCard.vue'
import ResourcesOverviewCard from '../components/ResourcesOverviewCard.vue'
import SudaCustomCard from '../components/SudaCustomCard.vue'
import SysMonitorCard from '../components/SysMonitorCard.vue'
import PromptBoxCard from '../components/PromptBoxCard.vue'
import RecentBar from '../components/RecentBar.vue'
import ClockCard from '../components/ClockCard.vue'
import WeatherCard from '../components/WeatherCard.vue'
import StickyCard from '../components/StickyCard.vue'
import CountdownCard from '../components/CountdownCard.vue'
import { useStore } from '../stores/workbench'
import { isTauri, tauriApi } from '../api/tauri'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { Countdown, ExtensionEntry, Note, Resource, Todo } from '../api/tauri'
import { playChime } from '../utils/chime'
import { FileText, FolderOpen, LayoutDashboard, ListTodo, MessageSquare, Puzzle, Settings, ChevronLeft, ChevronRight } from 'lucide-vue-next'
import type { Component } from 'vue'
import { useTheme } from '../composables/useTheme'
import { broadcastThemeToFrames } from '../composables/themeTokens'
import { iconSrc } from '../composables/useResourceIcon'
import {
  dashPlacementHideTitle,
  dashPlacementTitle,
  dashVariantDef,
  useDashboardLayout,
  type DashPlacement,
} from '../composables/useDashboardLayout'
import SettingsSkeleton from '../components/SettingsSkeleton.vue'

// 大体量/低频视图异步分包按需加载，缩小首屏主 chunk
// 速记视图：两栏容器（NoteFolderTree 树 + NoteEditor，编辑器在其内部异步分包）
const SpeednoteView = defineAsyncComponent(() => import('../components/SpeednoteView.vue'))
const GlobalSearch = defineAsyncComponent(() => import('../components/GlobalSearch.vue'))
// 待办视图：自带编辑弹层 / 确认弹窗 / 日期时间字段，体量大且非首屏，同样按需分包
const TodoView = defineAsyncComponent(() => import('../components/TodoView.vue'))
// 设置页：加载期间用同骨架占位（**delay 0**：以前设 80ms 是为了避免骨架一闪而过，
// 结果那 80ms 是纯空白 —— 用户感知到的「点设置先空白」有一半来自这里）
// 另外启动后空闲时预热这个 chunk，点「设置」时直接命中模块缓存，几乎零等待。
const SettingsView = defineAsyncComponent({
  loader: () => import('../components/SettingsView.vue'),
  loadingComponent: SettingsSkeleton,
  delay: 0,
})
const PromptManageDialog = defineAsyncComponent(() => import('../components/PromptManageDialog.vue'))
const SudaCustomEditDialog = defineAsyncComponent(() => import('../components/SudaCustomEditDialog.vue'))
const ChatPanel = defineAsyncComponent(() => import('../components/ChatPanel.vue'))
const ExtensionCenter = defineAsyncComponent(() => import('../components/ExtensionCenter.vue'))
const ExtensionView = defineAsyncComponent(() => import('../components/ExtensionView.vue'))
const DashboardLayoutEditor = defineAsyncComponent(() => import('../components/DashboardLayoutEditor.vue'))

const store = useStore()

// 初始化三轴主题系统（应用 data-theme/data-preset/inline --accent，监听系统变化）
useTheme()

// ---- 应用壁纸：主窗口所有视图共用，浮窗不跟随；文件失效时静默回退渐变背景 ----
// 壁纸单一套，所有主题模式共用同一张
const wallpaperFailed = ref(false)
watch(
  () => store.state.config.wallpaper_path,
  () => {
    wallpaperFailed.value = false
  },
)
const wallpaperSrc = computed(() => {
  const p = store.state.config.wallpaper_path
  if (!p || wallpaperFailed.value || !isTauri()) return ''
  return convertFileSrc(p)
})

// 蒙版不透明度钳制 0–0.85：85% 封顶保证壁纸仍可辨识
const wallpaperVeil = computed(() =>
  Math.min(0.85, Math.max(0, store.state.config.wallpaper_veil)),
)

// 壁纸在场标记（html data-wallpaper）：壁纸态可读性增强的作用域开关。
// data-wallpaper-clear：卡片真实透底（低玻璃透明度或沉浸模式）时才为真，控制卡片文字光晕的启停
watch(
  [
    wallpaperSrc,
    () => store.state.config.glass_opacity,
    () => store.state.config.wallpaper_immersive,
  ] as const,
  ([src, glass, immersive]) => {
    const el = document.documentElement
    if (src) {
      el.dataset.wallpaper = '1'
    } else {
      delete el.dataset.wallpaper
    }
    if (src && (immersive || glass < 0.9)) {
      el.dataset.wallpaperClear = '1'
    } else {
      delete el.dataset.wallpaperClear
    }
    // 壁纸态切换会翻转扩展令牌（压墨/白墨），重新广播给扩展 iframe
    requestAnimationFrame(() => broadcastThemeToFrames())
  },
  { immediate: true },
)

function onWallpaperError() {
  wallpaperFailed.value = true
}

// ---- 视图切换（统一导航范式：每个侧栏项 = 一个独立视图） ----
const navigation = [
  { id: 'dashboard', label: '工作台', icon: LayoutDashboard },
  { id: 'todos', label: '待办', icon: ListTodo },
  { id: 'notes', label: '速记', icon: FileText },
  { id: 'suda', label: '速达', icon: FolderOpen },
  { id: 'chat', label: '对话', icon: MessageSquare },
] as const

// 对话入口暂时隐藏（后续恢复），功能仍可通过标题栏按钮 / Ctrl+Shift+K 唤起
const visibleNavigation = navigation.filter((item) => item.id !== 'chat')

// 设置不在顶部导航列表，作为独立入口固定在侧栏左下角，但同样是视图切换逻辑
// 自定义布局编辑器也是独立视图（从设置进入，完成后回主页面）
type ViewId =
  | (typeof navigation)[number]['id']
  | 'settings'
  | 'layout-editor'
  | 'extensions'
  | 'extension'
  | 'suda-web'
const activeView = ref<ViewId>('dashboard')

// 对话入口：点击侧栏「对话」即唤起右侧面板（面板是主形态，视图仅占位说明）
function onNavClick(id: ViewId) {
  activeView.value = id
  if (id === 'chat' && !chatOpen.value) {
    toggleChat()
  }
}

// ---- 扩展打开：扩展中心点开某个扩展 → 主区渲染扩展入口（view 形态） ----
const openedExtension = ref<{ id: string; surface: string | null; name: string } | null>(null)
const drawerExtension = ref<{ id: string; surface: string | null; name: string } | null>(null)
// 强制重载计数：WebView2 在窗口后台久置后可能丢弃扩展 iframe 内容（恢复前台后空白），
// 而点击「同一个」已打开的扩展时 extId/surface 均不变、useExtensionFrame 的 watch 不触发，
// 表现为点了没反应——每次打开都递增，强制 iframe 重新导航
const extensionReloadTick = ref(0)
const installedExtensions = ref<ExtensionEntry[]>([])

function onOpenExtension(ext: ExtensionEntry) {
  // 扩展中心点开扩展 = 按该扩展的「打开方式」打开（设置弹窗里配置，默认 view=软件内）。
  // view 保持旧的 surface:null（入口按 manifest 默认形态解析），window/drawer 走分流
  const mode = store.state.config.extension_open_modes?.[ext.id] ?? 'view'
  if (mode === 'view') {
    extensionReloadTick.value++
    openedExtension.value = { id: ext.id, surface: null, name: ext.name }
    activeView.value = 'extension'
    return
  }
  openExtensionSurface(ext.id, mode)
}

// ---- 速达「应用内打开网页」：主窗内嵌面板（ADR 0011） ----
// store.launchResource 在网页条目 + panel 模式时派发 CustomEvent（store 不持有视图状态），
// 这里切到 suda-web 视图渲染 SudaWebPanel（挂载时按内容区矩形调 suda_panel_show）
const sudaWebPanel = ref<{ id: number; url: string; name: string } | null>(null)
let viewBeforeSudaWeb: ViewId = 'suda'

function openSudaWebPanel(detail: { id: number; url: string; name: string }) {
  if (activeView.value !== 'suda-web') viewBeforeSudaWeb = activeView.value
  sudaWebPanel.value = detail
  activeView.value = 'suda-web'
}

function onSudaWebPanelEvent(e: Event) {
  const detail = (e as CustomEvent<{ id: number; url: string; name: string }>).detail
  if (detail && typeof detail.id === 'number') openSudaWebPanel(detail)
}

function closeSudaWebPanel() {
  void tauriApi.sudaPanelHide().catch(() => {})
  sudaWebPanel.value = null
  activeView.value = viewBeforeSudaWeb
}

// 经侧栏等途径离开面板视图时也要收起子 webview（面板没有侧栏入口，回不去即应隐藏）
watch(activeView, (now, prev) => {
  if (prev === 'suda-web' && now !== 'suda-web') {
    void tauriApi.sudaPanelHide().catch(() => {})
    sudaWebPanel.value = null
  }
})

async function refreshInstalledExtensions() {
  if (!isTauri()) return
  try {
    installedExtensions.value = await tauriApi.listExtensions()
    pruneStalePinnedExtensions()
  } catch {
    // 忽略：扩展列表加载失败时侧栏仅保留已缓存的条目
  }
}

// 卸载后同步清理配置里残留的固定 id（否则下次重启侧栏会出现失效条目，点击报“不存在”）
function pruneStalePinnedExtensions() {
  const pinned = store.state.config.sidebar_extensions ?? []
  const validIds = new Set(installedExtensions.value.map((e) => e.id))
  const stale = pinned.filter((id) => !validIds.has(id))
  if (stale.length > 0) {
    store.setSidebarExtensionBulk(pinned.filter((id) => validIds.has(id)))
  }
}

function onExtensionsChanged() {
  void refreshInstalledExtensions()
}

// 固定到左侧栏的扩展：点击侧栏菜单即在主区打开（view 形态）
const pinnedExtensionIds = computed(() => store.state.config.sidebar_extensions ?? [])

const sidebarExtensions = computed(() =>
  installedExtensions.value.filter(
    (e) =>
      pinnedExtensionIds.value.includes(e.id) &&
      !e.invalid &&
      !e.disabled &&
      (e.missing_dependencies ?? []).length === 0,
  ),
)

function openSidebarExtension(ext: ExtensionEntry) {
  // 按扩展设置的「默认打开方式」打开：view（主区）/ window（独立窗口）/ drawer（抽屉）
  const mode = store.state.config.extension_open_modes?.[ext.id] ?? 'view'
  openExtensionSurface(ext.id, mode)
}

// 进出扩展中心时刷新侧栏扩展列表（新装/卸载/固定后即时反映）
watch(activeView, (v) => {
  if (v === 'extensions') void refreshInstalledExtensions()
})

// 扩展 module 内通过 runtime.open(surface) 请求打开自身某个形态（通用能力）
const VALID_EXT_SURFACES = new Set(['view', 'window', 'drawer'])

function openExtensionSurface(extId: string, surface: string) {
  const s = VALID_EXT_SURFACES.has(surface) ? surface : 'view'
  const ext = installedExtensions.value.find((e) => e.id === extId)
  if (s === 'window') {
    tauriApi.openExtensionWindow(extId).catch((e) => {
      showToast(`打开窗口失败：${String(e)}`)
    })
    return
  }
  if (s === 'drawer') {
    extensionReloadTick.value++
    drawerExtension.value = { id: extId, surface: 'drawer', name: ext?.name ?? extId }
    return
  }
  extensionReloadTick.value++
  openedExtension.value = { id: extId, surface: s, name: ext?.name ?? extId }
  activeView.value = 'extension'
}

function closeExtension() {
  openedExtension.value = null
  activeView.value = 'extensions'
}

function closeExtensionDrawer() {
  drawerExtension.value = null
}

// ---- 侧边栏收起（展开功能默认关闭，侧栏默认收起；开启后显示展开/收起按钮） ----
const sidebarCollapsed = ref(true)

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
}

function openPromptManage() {
  promptManageVisible.value = true
}

function openNotes() {
  activeView.value = 'notes'
}
function openSuda() {
  activeView.value = 'suda'
}
// 待办概览卡 / 待办卡标题栏的「打开待办视图」：进独立的待办视图（标签筛选、月/周日历、周期待办）
function openTodo() {
  activeView.value = 'todos'
}

// ---- 工作台自定义布局（12 列单元格网格，模块库两栏编辑器） ----
const layout = useDashboardLayout()

// 模块 id → 组件 + props 映射（含原「中上可切换」的 5 个独立模块）
const dashCardComponents: Record<string, Component> = {
  clock: ClockCard,
  weather: WeatherCard,
  sysmon: SysMonitorCard,
  sticky1: StickyCard,
  sticky2: StickyCard,
  notes: NotesOverviewCard,
  todo_overview: TodoOverviewCard,
  resources: ResourcesOverviewCard,
  suda1: SudaCustomCard,
  suda2: SudaCustomCard,
  suda3: SudaCustomCard,
  suda4: SudaCustomCard,
  countdown: CountdownCard,
  prompts: PromptBoxCard,
  todo: TodoCard,
  calendar: TodoCalendarCard,
  recent: RecentBar,
}

function dashCardComponent(id: string): Component {
  // 扩展 module：id 形如 ext:<扩展id>，用 iframe 渲染其 module 入口（复用桥 API）
  if (id.startsWith('ext:')) return ExtensionView
  return dashCardComponents[id] ?? ClockCard
}

function dashCardProps(p: DashPlacement): Record<string, unknown> {
  const id = p.id
  if (id.startsWith('ext:')) {
    const extId = id.slice('ext:'.length)
    return {
      extId,
      surface: 'module',
      variant: p.variant ?? dashVariantDef(id)?.id ?? null,
      onOpenSurface: (surface: string) => openExtensionSurface(extId, surface),
      // 扩展 module 卡片也有宿主表头（标题 = 自定义标题 ?? manifest.name），与内置模块一致
      ...titleProps(p),
      // 扩展卡没有内置文案兜底，标题给宿主侧解析后的值
      title: dashPlacementTitle(p),
    }
  }
  switch (id) {
    case 'clock':
      return { variant: p.variant }
    case 'weather':
      return { variant: p.variant }
    case 'sticky1':
      return { slot: 1, ...titleProps(p) }
    case 'sticky2':
      return { slot: 2, ...titleProps(p) }
    case 'notes':
      return { onOpenDetail: openNotes, ...titleProps(p) }
    case 'todo_overview':
      return { onOpenDetail: openTodo, ...titleProps(p) }
    case 'resources':
      return { onOpenDetail: openSuda, ...titleProps(p) }
    case 'suda1':
    case 'suda2':
    case 'suda3':
    case 'suda4':
      return {
        slotId: id,
        ...titleProps(p),
        onConfigure: () => {
          sudaCustomEditId.value = id
        },
      }
    case 'prompts':
      return { onOpenManage: openPromptManage, ...titleProps(p) }
    case 'todo':
      return { highlightId: highlightTodoId.value, onOpenDetail: openTodo, ...titleProps(p) }
    case 'calendar':
      return { onOpenDetail: openTodo, ...titleProps(p) }
    case 'countdown':
      return { sizeW: p.w, sizeH: p.h, ...titleProps(p) }
    case 'sysmon':
    case 'recent':
      return { ...titleProps(p) }
    default:
      return {}
  }
}

/** 卡片标题相关 props：title = 自定义标题（缺省由卡片回退内置文案），hideTitle = 关闭标题行 */
function titleProps(p: DashPlacement): { title?: string; hideTitle: boolean } {
  return { title: p.title, hideTitle: dashPlacementHideTitle(p) }
}

// 主界面行高按「总行数均分可用高度」自适应（1fr），窗口缩放/分辨率变化只改每格像素值、
// 卡片占格比例不变 → 不滚动、不留白、不因缩放而错位
const dashGridRows = computed(() => {
  let m = 1
  for (const p of layout.placements.value) {
    m = Math.max(m, p.y + p.h)
  }
  return m
})

function dashCellStyle(p: DashPlacement) {
  return {
    gridColumn: `${p.x + 1} / span ${p.w}`,
    gridRow: `${p.y + 1} / span ${p.h}`,
  }
}

function openLayoutEditor() {
  activeView.value = 'layout-editor'
}

// ---- 启动加载：数据就绪后隐藏欢迎页（窗口已改为启动即显示） ----
// 欢迎页至少展示 2.2s：本地数据库很小，数据可能几十毫秒就加载完，
// 若不保底，淡出会发生在首帧绘制之前，用户根本看不到欢迎页
const bootStartAt = performance.now()
const BOOT_MIN_MS = 2200
const BOOT_MAX_MS = 4000

onMounted(async () => {
  // 设置页与待办视图 chunk 空闲预热：点进去时直接命中模块缓存，不再出现加载空白。
  // （设置页内部也按大类分包 + 占位 + 悬停预取，见 SettingsView.vue 头部说明）
  const warmSettings = () => {
    void import('../components/SettingsView.vue')
    void import('../components/TodoView.vue')
  }
  const idle = (
    window as unknown as {
      requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => void
    }
  ).requestIdleCallback
  if (idle) idle(warmSettings, { timeout: 3000 })
  else window.setTimeout(warmSettings, 1500)

  store.loadInitialData().finally(() => {
    store.startOnlineMonitor()
    const wait = Math.max(0, BOOT_MIN_MS - (performance.now() - bootStartAt))
    setTimeout(hideBootSplash, wait)
  })
  // 兜底：无论数据是否加载成功，最多 4s 后隐藏欢迎页，避免一直遮挡
  setTimeout(hideBootSplash, BOOT_MAX_MS)
  // 浮窗便签还原/删除后，主窗口实时同步便签与脱离状态
  if (isTauri()) {
    // 速达网页内嵌面板打开请求（store.launchResource 派发，DOM 事件免视图状态跨层传递）
    window.addEventListener('suda-open-web-panel', onSudaWebPanelEvent)
    /** 单条监听注册失败（桥未就绪等）不能中断启动流程：
     *  裸 await 的任一 reject 会让其后所有监听与抽屉还原都不再执行。 */
    const on = async <T,>(ev: string, cb: (e: { payload: T }) => void) => {
      try {
        return await listen<T>(ev, cb)
      } catch {
        return null
      }
    }
    void refreshInstalledExtensions()
    unlistenStickies = await on('stickies-changed', () => {
      store.refreshStickies()
    })
    // 倒计时到点：toast 提示 + 刷新列表（浮窗水罐同步）
    unlistenCountdownFired = await on<Countdown>('countdown-fired', (e) => {
      const name = e.payload?.name ?? ''
      showToast(name ? `「${name}」时间到` : '倒计时时间到')
      if (store.state.config.countdown_sound) {
        playChime()
      }
      void store.refreshCountdowns()
    })
    // ticker 顺延 / 创建更新后同步
    unlistenCountdownsChanged = await on('countdowns-changed', () => {
      void store.refreshCountdowns()
    })
    // 剪贴板浮层「存为速记 / 加入提示词库」后，主窗口实时刷新列表
    unlistenNotesChanged = await on('notes-changed', () => {
      void store.refreshNotes()
    })
    unlistenSnippetsChanged = await on('snippets-changed', () => {
      void store.loadSnippets()
    })
    unlistenTodosChanged = await on('todos-changed', () => {
      void store.refreshTodos()
    })
    // 待办标签定义/关联被外部（扩展桥）改动后刷新
    unlistenTodoTagsChanged = await on('todo-tags-changed', () => {
      void store.refreshTodoTags()
    })
    // 待办提醒到点：toast 提示（系统通知由后端 todo_reminder 直接发）
    unlistenTodoRemind = await on<Todo>('todo-remind', (e) => {
      const title = e.payload?.title ?? ''
      showToast(title ? `待办提醒：「${title}」` : '待办提醒时间到')
    })
    // 桌面悬浮球动作（ADR 0004）：切视图 / 全局搜索 / 新建速记
    // （Rust trigger 已先显示主窗口；剪贴板 act:clipboard 在 Rust 直呼浮层，不经这里）
    unlistenBallAction = await on<string>('floating-ball-action', (e) => {
      const id = e.payload ?? ''
      if (id.startsWith('view:')) {
        const v = id.slice(5)
        if (v === 'chat') {
          activeView.value = 'chat'
          if (!chatOpen.value) toggleChat()
        } else {
          activeView.value = v as ViewId
        }
      } else if (id === 'act:search') {
        searchVisible.value = true
      } else if (id === 'act:note') {
        void onCreateNote()
      }
    })
    // 独立对话窗「模型设置」入口：唤出主窗并定位到设置 → AI 助手
    unlistenOpenChatSettings = await on('open-chat-settings', () => {
      onOpenChatSettings()
    })
    // 搜索快捷键（全局注册，Rust 分发）：主窗前端负责打开/关闭搜索弹窗
    unlistenSearchShortcut = await on('search-shortcut', () => {
      searchVisible.value = !searchVisible.value
    })
    // AI 对话快捷键（全局注册，Rust 分发）：与抽屉/独立窗口形态分流共用 toggleChat
    unlistenChatShortcut = await on('chat-shortcut', () => {
      toggleChat()
    })
    // 速记快捷键（全局注册，Rust 分发）：唤起主窗（Rust 侧 show_window）→ 切速记视图 → 聚焦新建
    unlistenNotesShortcut = await on('notes-shortcut', () => {
      void onCreateNote()
    })
    // 扩展页「去授权」跳转（桥 API xhub.openPermissions）：切到扩展中心并打开该扩展的
    // 设置弹窗（权限管理所在处）。payload = 扩展 id
    unlistenOpenExtSettings = await on<string>('open-extension-settings', (e) => {
      if (e.payload) openExtensionSettings(e.payload)
    })
    // 形态切换（设置里的开关 / 独立窗侧改动）：切到独立窗口时收起内嵌抽屉，二者互斥
    unlistenChatMode = await on<boolean>('chat-window-mode', (e) => {
      if (e.payload && chatOpen.value) {
        chatOpen.value = false
        persistChatPanelSize()
      }
    })
  }
  // 浏览器预览没有全局快捷键：保留应用内 Ctrl+K / Ctrl+Shift+K 兜底。
  // 桌面端一律走全局注册 + 事件分发（在这里注册应用内监听会与全局事件双触发、
  // 两次 toggle 相互抵消，表现为「按了没反应」）
  if (!isTauri()) {
    window.addEventListener('keydown', onSearchKeydown)
    window.addEventListener('keydown', onChatKeydown)
  }
  await restoreChatPanel()
  // 点击抽屉外部收起（捕获阶段，覆盖 main-area / 标题栏 / 侧边栏等一切抽屉外区域）
  document.addEventListener('pointerdown', onDocPointerDown, true)
})

let unlistenStickies: (() => void) | null = null
let unlistenCountdownFired: (() => void) | null = null
let unlistenCountdownsChanged: (() => void) | null = null
let unlistenNotesChanged: (() => void) | null = null
let unlistenSnippetsChanged: (() => void) | null = null
let unlistenTodosChanged: (() => void) | null = null
let unlistenTodoTagsChanged: (() => void) | null = null
let unlistenTodoRemind: (() => void) | null = null
let unlistenBallAction: (() => void) | null = null
let unlistenOpenChatSettings: (() => void) | null = null
let unlistenSearchShortcut: (() => void) | null = null
let unlistenChatShortcut: (() => void) | null = null
let unlistenNotesShortcut: (() => void) | null = null
let unlistenOpenExtSettings: (() => void) | null = null
let unlistenChatMode: (() => void) | null = null

onUnmounted(() => {
  store.stopOnlineMonitor()
  unlistenStickies?.()
  unlistenCountdownFired?.()
  unlistenCountdownsChanged?.()
  unlistenNotesChanged?.()
  unlistenSnippetsChanged?.()
  unlistenTodosChanged?.()
  unlistenTodoTagsChanged?.()
  unlistenTodoRemind?.()
  unlistenBallAction?.()
  unlistenOpenChatSettings?.()
  unlistenSearchShortcut?.()
  unlistenChatShortcut?.()
  unlistenNotesShortcut?.()
  unlistenOpenExtSettings?.()
  unlistenChatMode?.()
  window.removeEventListener('suda-open-web-panel', onSudaWebPanelEvent)
  window.removeEventListener('keydown', onSearchKeydown)
  window.removeEventListener('keydown', onChatKeydown)
  document.removeEventListener('pointerdown', onDocPointerDown, true)
})

function hideBootSplash() {
  const el = document.getElementById('boot-splash')
  if (el && !el.classList.contains('hide')) {
    el.classList.add('hide')
    setTimeout(() => el.remove(), 450)
  }
}

// ---- 笔记选中与操作（三栏视图挂 SpeednoteView，选择/删除/回收站在视图内部管理；
// 这里只保留外部入口：悬浮球新建、全局搜索跳转、热键直达） ----
const highlightTodoId = ref<number | null>(null)

const speednoteRef = ref<{ openNote: (id: number) => void; createNote: () => void } | null>(null)

/** 等速记视图就绪（异步 chunk + 挂载），最多 ~1.5s；未就绪返回 null（调用方静默放弃） */
async function waitSpeednoteReady() {
  activeView.value = 'notes'
  await nextTick()
  for (let i = 0; i < 30; i++) {
    if (speednoteRef.value) return speednoteRef.value
    await new Promise((r) => setTimeout(r, 50))
  }
  return null
}

async function onCreateNote() {
  // 悬浮球等入口触发时可能停在其它视图：新建后必须切到速记页，否则只见新建不见页面
  const view = await waitSpeednoteReady()
  view?.createNote()
}

// ---- 全局搜索 / 设置 ----
const searchVisible = ref(false)
const promptManageVisible = ref(false)
// 正在配置内容的工作台「自定义速达」槽位 id（suda1..suda4；null = 弹窗关闭）
const sudaCustomEditId = ref<string | null>(null)
const settingsSection = ref('')

function onOpenTodo(t: Todo) {
  searchVisible.value = false
  highlightTodoId.value = t.id
  activeView.value = 'dashboard'
}

function onSearchKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    searchVisible.value = !searchVisible.value
  }
}

// ---- AI 对话抽屉（支持上下左右四个方位）----
const chatOpen = ref(false)
const chatWidth = ref(420)
const chatHeight = ref(380)
// 方位取自配置（设置页可切换），default 回退右侧
const chatSide = computed(() => (store.state.config.chat_panel_side || 'right') as 'left' | 'right' | 'top' | 'bottom')
// 左右方位用宽度、上下方位用高度（传给 dock 布局）
const chatDockStyle = computed(() =>
  chatSide.value === 'top' || chatSide.value === 'bottom'
    ? { height: chatHeight.value + 'px', width: '100%' }
    : { width: chatWidth.value + 'px', height: '100%' },
)

function toggleChat() {
  // 独立窗口形态（设置「AI 助手 → 以独立窗口打开」决定，与内嵌抽屉互斥）：
  // 唤起/收起后端常驻的对话小窗，抽屉状态不参与
  if (isTauri() && store.state.config.chat_window_mode) {
    void tauriApi.chatWindowToggle().catch(() => {})
    return
  }
  chatOpen.value = !chatOpen.value
  // 窗口开关状态不持久化：重启后始终默认收起，仅保存尺寸
  persistChatPanelSize()
}

function persistChatPanelSize() {
  if (!isTauri()) return
  void tauriApi.setChatPanel(chatWidth.value, chatHeight.value, chatOpen.value)
}

function onChatToggle() {
  toggleChat()
}

// 面板「去配置大模型」：跳转设置页并定位到 AI 助手分类
function onOpenChatSettings() {
  settingsSection.value = 'ai'
  if (chatOpen.value) toggleChat()
  activeView.value = 'settings'
}

/** 设置 → 扩展中心（本机源码目录的增删都在那里） */
function onOpenExtensionsView() {
  activeView.value = 'extensions'
}

/** 扩展中心「我的扩展」→「点击跳转」：跳设置页并定位到 Skills 分区（装扩展开发 Skill 的入口） */
function onOpenSkillsSettings() {
  settingsSection.value = 'skills'
  activeView.value = 'settings'
}

// ---- 扩展设置弹窗跳转（扩展页 xhub.openPermissions 的「去授权」落点） ----
// ExtensionCenter 不在 DOM 时（正看某个扩展的 view）无法直接弹它的设置弹窗：
// 这里切到扩展中心，把「要打开哪个扩展的设置」经 prop 递进去，列表加载完即弹。
const extensionSettingsJump = ref<{ id: string; nonce: number } | null>(null)
let extensionSettingsNonce = 0

function openExtensionSettings(extId: string) {
  extensionSettingsJump.value = { id: extId, nonce: ++extensionSettingsNonce }
  if (activeView.value !== 'extensions') activeView.value = 'extensions'
}

async function restoreChatPanel() {
  if (!isTauri()) return
  try {
    const [w, h] = await tauriApi.getChatPanel()
    chatWidth.value = w
    chatHeight.value = h
    // 启动时始终默认收起（开关状态不持久化）
    chatOpen.value = false
  } catch {
    // 忽略：命令未就绪时保持默认收起
  }
}

// 拖拽改尺寸后由 ChatPanel 回调同步本地状态（宽度/高度根据当前方位取对应值）
function onChatPanelResized(w: number, h: number) {
  chatWidth.value = w
  chatHeight.value = h
}

function onChatKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    toggleChat()
  }
}

// ---- 点击抽屉外部收起（仅内嵌抽屉形态；独立小窗有自己的关闭逻辑）----
function closeChatDrawer() {
  if (!chatOpen.value) return
  chatOpen.value = false
  persistChatPanelSize()
}

// 用 pointerdown 的捕获阶段拿「按下」事件，比 click 更早，避免和抽屉内部按钮的
// click 抢时序。判定：
//   1) 抽屉内部（输入框/会话菜单/拖拽改尺寸手柄等）→ 不算外部；
//   2) 标记了 data-chat-opener 的对话入口（标题栏对话按钮）→ 不算外部，
//      交给它自己的 click 去切换，否则「按下先收起、click 再打开」会互相抵消。
function onDocPointerDown(e: PointerEvent) {
  if (!chatOpen.value) return
  const t = e.target as HTMLElement | null
  if (!t || typeof t.closest !== 'function') return
  // 抽屉内部 + 对话入口按钮都不算外部；
  // 另外两个是 Teleport 到 body 的浮层（会话菜单 / 模型下拉），它们不在 .chat-dock 里，
  // 但属于对话面板的操作——不豁免的话 pointerdown 会先把抽屉收起，导致「新建/删除对话」点了没反应。
  if (
    t.closest('.chat-dock') ||
    t.closest('[data-chat-opener]') ||
    t.closest('.cp-menu') ||
    t.closest('.app-select-menu')
  ) return
  closeChatDrawer()
}

async function onOpenResource(r: Resource) {
  searchVisible.value = false
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法启动「${r.name}」：${String(e)}`)
  }
}

async function onOpenNote(n: Note) {
  searchVisible.value = false
  const view = await waitSpeednoteReady()
  view?.openNote(n.id)
}

// ---- 轻提示 ----
interface ToastAction {
  label: string
  onClick: () => void
}

const toastMsg = ref('')
const toastAction = ref<ToastAction | null>(null)
let toastTimer: ReturnType<typeof setTimeout> | null = null

function showToast(msg: string, action?: ToastAction) {
  toastMsg.value = msg
  toastAction.value = action ?? null
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toastMsg.value = ''
    toastAction.value = null
  }, action ? 5000 : 2200)
}

provide('showToast', showToast)

</script>

<template>
  <div class="app-shell">
    <!-- 应用壁纸层：仅主窗口渲染，垫在全部内容之下、body 渐变之上（模糊作用于本层整体，见 ADR 0002） -->
    <div v-if="wallpaperSrc" class="wallpaper-layer" aria-hidden="true">
      <img
        class="wallpaper-img"
        :class="{ blur: store.state.config.wallpaper_blur && !store.state.config.wallpaper_immersive }"
        :src="wallpaperSrc"
        alt=""
        @error="onWallpaperError"
      />
      <!-- 壁纸蒙版：主题底色罩层，在壁纸鲜亮度与文字/图标对比度之间取平衡 -->
      <div class="wallpaper-veil" :style="{ opacity: wallpaperVeil }"></div>
      <div class="wallpaper-glow"></div>
    </div>
    <TitleBar
      @search="searchVisible = true"
      @chat="toggleChat"
    />

    <div class="app-body" :class="{ collapsed: sidebarCollapsed }">
      <aside
        class="sidebar"
        :class="{ collapsed: sidebarCollapsed }"
        aria-label="应用侧栏"
      >
        <nav class="sidebar-nav" aria-label="主要导航">
          <button
            v-for="item in visibleNavigation"
            :key="item.id"
            class="sidebar-nav-item"
            :class="{ active: activeView === item.id }"
            :aria-current="activeView === item.id ? 'page' : undefined"
            :data-tip="item.label"
            type="button"
            @click="onNavClick(item.id)"
          >
            <span class="sidebar-nav-icon" aria-hidden="true">
              <component :is="item.icon" :size="16" :stroke-width="2" />
            </span>
            <span>{{ item.label }}</span>
          </button>
        </nav>

        <!-- 固定到侧栏的扩展：点击即在主区打开（view 形态） -->
        <div v-if="sidebarExtensions.length" class="sidebar-ext">
          <p class="sidebar-ext-label">扩展</p>
          <button
            v-for="ext in sidebarExtensions"
            :key="ext.id"
            class="sidebar-nav-item"
            :class="{ active: activeView === 'extension' && openedExtension?.id === ext.id }"
            :aria-current="activeView === 'extension' && openedExtension?.id === ext.id ? 'page' : undefined"
            :data-tip="ext.name"
            type="button"
            @click="openSidebarExtension(ext)"
          >
            <span class="sidebar-nav-icon" :class="{ 'has-img': !!ext.icon }" aria-hidden="true">
              <img
                v-if="ext.icon"
                class="sidebar-ext-img"
                :src="iconSrc(ext.icon)"
                :alt="ext.name"
                draggable="false"
              />
              <Puzzle v-else :size="15" :stroke-width="2" />
            </span>
            <span>{{ ext.name }}</span>
          </button>
        </div>

        <div class="sidebar-foot">
        <button
          class="sidebar-status"
          :class="{ active: activeView === 'extensions' }"
          type="button"
          aria-label="打开扩展中心"
          data-tip="扩展中心"
          @click="activeView = 'extensions'"
        >
          <Puzzle :size="15" :stroke-width="2" aria-hidden="true" />
          <span>扩展中心</span>
        </button>

        <button
          class="sidebar-status"
          :class="{ active: activeView === 'settings' }"
          type="button"
          aria-label="打开设置"
          data-tip="设置"
          @click="activeView = 'settings'"
        >
          <Settings :size="15" :stroke-width="2" aria-hidden="true" />
          <span>设置</span>
        </button>
        <button
          v-if="store.state.config.sidebar_toggle"
          class="sidebar-status sidebar-collapse"
          type="button"
          :aria-label="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
          :data-tip="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
          @click="toggleSidebar"
        >
          <component
            :is="sidebarCollapsed ? ChevronRight : ChevronLeft"
            :size="15"
            :stroke-width="2"
            aria-hidden="true"
          />
          <span v-if="!sidebarCollapsed">收起</span>
        </button>
        </div>
      </aside>

      <div class="main-area">
        <main class="workspace" aria-label="主工作区">
        <!-- 工作台：可自定义布局（12 列单元格网格，模块库编辑器） -->
        <div v-if="activeView === 'dashboard'" class="dash-wrap">
          <div
            v-if="layout.placements.value.length"
            class="dash-grid"
            :style="{
              gridTemplateRows: `repeat(${dashGridRows}, minmax(var(--dash-row-min), 1fr))`,
              '--dash-rows': dashGridRows,
            }"
          >
            <div
              v-for="p in layout.placements.value"
              :key="p.id"
              class="dash-cell"
              :style="dashCellStyle(p)"
            >
              <component
                :is="dashCardComponent(p.id)"
                v-bind="dashCardProps(p)"
                @go-suda="activeView = 'suda'"
              />
            </div>
          </div>
          <div v-else class="dash-empty">
            <p>工作台还没有模块，去设置里自定义布局吧</p>
            <button class="pill-btn" type="button" @click="openLayoutEditor">自定义布局</button>
          </div>
        </div>

        <!-- 待办视图：标签筛选 / 月·周日历 / 周期待办（宽窗左右同屏，窄窗单栏） -->
        <TodoView v-else-if="activeView === 'todos'" />

        <!-- 速记：三栏视图（文件夹树 / 列表 / 编辑器，docs/speednote-plan.md） -->
        <SpeednoteView v-else-if="activeView === 'notes'" ref="speednoteRef" class="view view-notes" />

        <!-- 速达：独立视图 -->
        <section v-else-if="activeView === 'suda'" class="view view-suda" tabindex="-1" aria-label="速达">
          <Suda />
        </section>

        <!-- 速达网页内嵌面板（ADR 0011）：工具栏是 DOM，内容区是预创建子 webview 的空白位 -->
        <section
          v-else-if="activeView === 'suda-web'"
          class="view view-suda-web"
          tabindex="-1"
          aria-label="应用内浏览器"
        >
          <SudaWebPanel
            v-if="sudaWebPanel"
            :resource-id="sudaWebPanel.id"
            :url="sudaWebPanel.url"
            :name="sudaWebPanel.name"
            @close="closeSudaWebPanel"
          />
        </section>

        <!-- 扩展中心：独立视图 -->
        <section v-else-if="activeView === 'extensions'" class="view view-extensions" tabindex="-1" aria-label="扩展中心">
          <ExtensionCenter
            :jump-settings="extensionSettingsJump"
            @open="onOpenExtension"
            @open-surface="(ext, surface) => openExtensionSurface(ext.id, surface)"
            @changed="onExtensionsChanged"
            @open-skills="onOpenSkillsSettings"
          />
        </section>

        <!-- 扩展运行视图：主区渲染扩展入口（iframe + window.xhub 桥 API）。
             不带宿主工具栏：扩展名与「窗口/抽屉」多形态打开入口已由扩展中心承载 -->
        <section v-else-if="activeView === 'extension'" class="view view-extension" tabindex="-1" aria-label="扩展">
          <ExtensionView
            v-if="openedExtension"
            :ext-id="openedExtension.id"
            :surface="openedExtension.surface"
            :reload-key="extensionReloadTick"
            @close="closeExtension"
          />
        </section>

        <!-- 对话：独立视图（完整视图，与右侧面板共用会话数据） -->
        <section v-else-if="activeView === 'chat'" class="view view-chat" tabindex="-1" aria-label="对话">
          <div class="view-chat-hint">
            <MessageSquare :size="20" :stroke-width="1.8" />
            <p>抽屉面板已是最佳对话形态，可点击标题栏对话按钮或按 Ctrl+Shift+K 唤起（方位可在设置 → AI 助手调整）。</p>
          </div>
        </section>

        <!-- 设置：独立视图 -->
        <section v-else-if="activeView === 'settings'" class="view view-settings" tabindex="-1" aria-label="设置">
          <SettingsView
            :initial-section="settingsSection"
            @open-layout-editor="openLayoutEditor"
            @open-extensions="onOpenExtensionsView"
          />
        </section>

        <!-- 自定义布局编辑器：独立视图（从设置进入，完成后回主页面） -->
        <section v-else class="view view-layout-editor" tabindex="-1" aria-label="自定义布局">
          <DashboardLayoutEditor @done="activeView = 'dashboard'" />
        </section>
        </main>

        <!-- AI 对话抽屉（覆盖式，悬浮在内容上方，可从上下左右滑入，尺寸可拖拽） -->
        <Transition :name="`chat-drawer-${chatSide}`">
          <div
            v-if="chatOpen"
            class="chat-dock"
            :class="`dock-${chatSide}`"
            :style="{
              opacity: store.state.config.chat_panel_opacity ?? 1,
              ...chatDockStyle,
            }"
          >
            <ChatPanel
              :side="chatSide"
              @toggle="onChatToggle"
              @open-model-settings="onOpenChatSettings"
              @resized="onChatPanelResized"
            />
          </div>
        </Transition>

        <!-- 扩展抽屉（覆盖式，右侧滑出） -->
        <Transition name="chat-drawer">
          <div v-if="drawerExtension" class="ext-drawer">
            <div class="ext-drawer-header">
              <span class="ext-drawer-title">{{ drawerExtension.name }}</span>
              <button
                class="ext-drawer-close"
                type="button"
                aria-label="关闭抽屉"
                @click="closeExtensionDrawer"
              >
                <ChevronRight :size="16" :stroke-width="2" aria-hidden="true" />
              </button>
            </div>
            <div class="ext-drawer-body">
              <ExtensionView
                :ext-id="drawerExtension.id"
                :surface="drawerExtension.surface"
                :reload-key="extensionReloadTick"
              />
            </div>
          </div>
        </Transition>
      </div>
    </div>

    <GlobalSearch
      :visible="searchVisible"
      @close="searchVisible = false"
      @open-resource="onOpenResource"
      @open-note="onOpenNote"
      @open-todo="onOpenTodo"
    />
    <PromptManageDialog
      :visible="promptManageVisible"
      @close="promptManageVisible = false"
    />
    <SudaCustomEditDialog
      v-if="sudaCustomEditId"
      :slot-id="sudaCustomEditId"
      :visible="!!sudaCustomEditId"
      @close="sudaCustomEditId = null"
    />

    <Transition name="toast">
      <div v-if="toastMsg" class="toast">
        <span class="toast-msg" :title="toastMsg">{{ toastMsg }}</span>
        <button
          v-if="toastAction"
          class="toast-action"
          type="button"
          @click="toastAction.onClick()"
        >
          {{ toastAction.label }}
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
/* 应用壁纸层：z-index -1 加入根层叠上下文负相位，盖过 body 渐变、垫在全部内容之下 */
.wallpaper-layer {
  position: fixed;
  inset: 0;
  z-index: -1;
  overflow: hidden;
  pointer-events: none;
}
.wallpaper-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  object-position: center;
}
.wallpaper-img.blur {
  filter: blur(8px);
  /* 收进模糊产生的四周透明羽化 */
  transform: scale(1.06);
}
/* 壁纸蒙版：取主题中性底色（亮色近白/暗色近黑，随模式联动），文字对比度兜底 */
.wallpaper-veil {
  position: absolute;
  inset: 0;
  background: linear-gradient(150deg, var(--bg-base-a) 0%, var(--bg-base-b) 100%);
}
/* 主题光晕叠加：壁纸替换渐变背景但保留主题氛围（--glow-* 随模式/预设联动） */
.wallpaper-glow {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(1200px 800px at 12% -8%, var(--glow-a), transparent 55%),
    radial-gradient(1000px 700px at 100% 4%, var(--glow-b), transparent 55%),
    radial-gradient(1200px 900px at 55% 118%, var(--glow-c), transparent 55%);
}

.app-shell {
  min-height: 100dvh;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: transparent;
  overflow: hidden;
  position: relative;
}

.app-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  transition: grid-template-columns 0.18s ease-out;
}
.app-body.collapsed {
  grid-template-columns: 56px minmax(0, 1fr);
}
.sidebar {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  /* 顶部贴齐标题栏下沿，菜单与内容卡片头部对齐 */
  padding: 0 var(--space-3) var(--space-3);
  background: transparent;
  overflow: hidden;
  transition: padding 0.18s ease-out;
}
.sidebar.collapsed {
  padding: 0 8px var(--space-3);
}
/* 铬件（侧栏）始终全透明：与背景（渐变/壁纸）构成同一个连续平面，表面只属于卡片（ADR 0003） */
/* 侧栏 hover 气泡是瞬态表面（实底 + 自带 blur），不吃壁纸态的文字光晕 */
html[data-wallpaper='1'] .sidebar [data-tip]::after,
html[data-wallpaper='1'] .title-bar [data-tip]::after {
  text-shadow: none;
}
.sidebar-nav {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.sidebar-nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 36px;
  padding: 0 var(--space-2);
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.sidebar-nav-item:hover,
.sidebar-nav-item.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.sidebar-nav-item.active {
  font-weight: 700;
}
.sidebar-nav-icon {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  transition: background 150ms ease-out, color 150ms ease-out, box-shadow 150ms ease-out;
}
.sidebar-status {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 34px;
  padding: 0 var(--space-2);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  text-align: left;
  border: 0;
  cursor: pointer;
}
.sidebar-nav + .sidebar-status { margin-top: auto; }
.sidebar-foot {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
/* 固定到侧栏的扩展组：独立区块，与主导航视觉一致，可滚动 */
.sidebar-ext {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-height: 0;
  overflow-y: auto;
}
.sidebar-ext-label {
  margin: 0;
  padding: 0 var(--space-2);
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-3);
}
/* 扩展图标：无图回退（Puzzle）才用中性软底 + 细描边；有图时不加任何托底，按图片原样显示 */
.sidebar-ext .sidebar-nav-icon {
  background: var(--bg-card-soft);
  box-shadow: inset 0 0 0 1px var(--border-soft);
}
.sidebar-ext .sidebar-nav-icon.has-img {
  background: transparent;
  box-shadow: none;
}
.sidebar-ext-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  padding: 4px;
}
.sidebar.collapsed .sidebar-ext {
  align-items: center;
  gap: 6px;
}
.sidebar.collapsed .sidebar-ext-label {
  display: none;
}
.sidebar.collapsed .sidebar-ext .sidebar-nav-icon {
  overflow: hidden;
}
.sidebar-status:hover { color: var(--text-1); }
.sidebar-status.active {
  background: var(--brand-50);
  color: var(--brand-500);
}

/* 收起态：只保留图标 */
.sidebar.collapsed .sidebar-nav-item,
.sidebar.collapsed .sidebar-status {
  justify-content: center;
  padding: 0;
}
.sidebar.collapsed .sidebar-nav-item {
  width: 40px;
  min-height: 40px;
  border-radius: 50%;
}
.sidebar.collapsed .sidebar-nav-item:hover {
  background: color-mix(in srgb, var(--brand-500) 8%, transparent);
}
.sidebar.collapsed .sidebar-nav-item.active,
.sidebar.collapsed .sidebar-nav-item.active:hover {
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
}
.sidebar.collapsed .sidebar-nav-item.active .sidebar-nav-icon {
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--brand-500) 10%, transparent);
}
.sidebar.collapsed .sidebar-nav-item > span:not(.sidebar-nav-icon),
.sidebar.collapsed .sidebar-status span {
  display: none;
}
.sidebar.collapsed .sidebar-status {
  min-height: 34px;
}
.sidebar.collapsed .sidebar-nav {
  align-items: center;
  gap: 6px;
}

/* 收起态：hover 图标时在右侧显示名称气泡 */
.sidebar.collapsed {
  overflow: visible;
}
.sidebar.collapsed .sidebar-status {
  position: relative;
}
.sidebar.collapsed [data-tip]::after {
  content: attr(data-tip);
  position: absolute;
  left: calc(100% + 12px);
  top: 50%;
  transform: translateY(-50%);
  padding: 4px 10px;
  font-size: 0.75rem;
  font-weight: 500;
  white-space: nowrap;
  color: var(--text-1);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  box-shadow: var(--shadow-card);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.12s ease-out;
  z-index: 60;
}
.sidebar.collapsed [data-tip]:hover::after {
  opacity: 1;
  transition-delay: 0.3s;
}

/* 主工作区：抽屉悬浮在内容上方，不再挤压左侧内容 */
.main-area {
  position: relative;
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: stretch;
  overflow: hidden;
}
.main-area .workspace {
  flex: 1;
  min-width: 0;
}

/* 覆盖式抽屉：absolute 悬浮于工作区之上，支持上下左右四个方位滑入 */
.main-area .chat-dock {
  position: absolute;
  z-index: 40;
  pointer-events: none;
}
.main-area .chat-dock :deep(.chat-panel) {
  height: 100%;
  width: 100%;
  pointer-events: auto;
}
.main-area .chat-dock.dock-right {
  top: 0;
  right: 0;
  bottom: 0;
}
.main-area .chat-dock.dock-left {
  top: 0;
  left: 0;
  bottom: 0;
}
.main-area .chat-dock.dock-top {
  top: 0;
  left: 0;
  right: 0;
}
.main-area .chat-dock.dock-bottom {
  bottom: 0;
  left: 0;
  right: 0;
}

/* 四个方位的滑入/滑出过渡（配合 ChatPanel 内部尺寸拖拽，动画只做 transform） */
.chat-drawer-right-enter-active,
.chat-drawer-right-leave-active,
.chat-drawer-left-enter-active,
.chat-drawer-left-leave-active,
.chat-drawer-top-enter-active,
.chat-drawer-top-leave-active,
.chat-drawer-bottom-enter-active,
.chat-drawer-bottom-leave-active {
  transition: transform 0.24s ease-out, opacity 0.18s ease-out;
}
.chat-drawer-right-enter-from,
.chat-drawer-right-leave-to {
  transform: translateX(100%);
}
.chat-drawer-left-enter-from,
.chat-drawer-left-leave-to {
  transform: translateX(-100%);
}
.chat-drawer-top-enter-from,
.chat-drawer-top-leave-to {
  transform: translateY(-100%);
}
.chat-drawer-bottom-enter-from,
.chat-drawer-bottom-leave-to {
  transform: translateY(100%);
}

/* 工作台布局：12 列 fr 比例网格 + 行高 1fr 均分填满（缩放/分辨率只改每格像素值，布局结构不变）。
   行高带下限（--dash-row-min）：窗口够高时仍是 1fr 撑满、不滚动、不留白；窗口过矮时不再把每格
   压到几十像素后裁掉卡片内容，而是让 .dash-wrap 出现纵向滚动条兜底。 */
.dash-wrap {
  position: relative;
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
}
.dash-grid {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: var(--space-4);
  padding: 0 20px 20px 0;
  height: 100%;
  --dash-row-min: 36px;
  min-height: calc(var(--dash-row-min) * var(--dash-rows) + var(--space-4) * (var(--dash-rows) - 1) + 20px);
}
.dash-cell {
  min-width: 0;
  min-height: 0;
  position: relative;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  /* 容器查询容器：卡片内 cq 单位随格子缩放（形态化卡片的核心前提） */
  container-type: size;
}
.dash-cell > * {
  flex: 1;
  min-height: 0;
}
.dash-empty {
  height: 100%;
  min-height: 240px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.8125rem;
}

/* 独立视图 */
.view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 速记视图：两栏布局（仅右下外边距，左上贴边与原版一致） */
.view-notes {
  padding: 0 20px 20px 0;
}
.view-suda {
  padding: 0 20px 20px 0;
}
.view-settings {
  padding: 0 20px 20px 0;
}
/* 扩展中心：覆盖组件内四边 padding，仅保留右下外边距（左上贴边与原版一致） */
.view-extensions :deep(.extension-center) {
  padding: 0 20px 20px 0;
}
/* 扩展运行视图（整页形态既定标准）：宿主零边距，留白由扩展页自行控制——
   默认左上 0 / 右下 20（skill 模板内置），用户选「完全无边距」的扩展四面贴边 */
.view-extension {
  padding: 0;
}

/* 扩展抽屉：absolute 悬浮于工作区之上，右侧滑入 */
.ext-drawer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 45;
  width: 480px;
  max-width: 82vw;
  display: flex;
  flex-direction: column;
  background: var(--bg-card-solid);
  border-left: 1px solid var(--border-strong);
  box-shadow: var(--shadow-dock);
  pointer-events: auto;
}
/* 壁纸态：抽屉是瞬态表面，改用真实取景模糊 + 玻璃底 ——
   否则那块 0.88 的不透明白会把壁纸切成一块死白（它不在 main 子树里，
   拿不到透底态对 --bg-card-solid 的覆盖，取的是 root 的 0.88 白）。 */
html[data-wallpaper='1'] .ext-drawer {
  background: var(--frost-surface);
  backdrop-filter: blur(18px) saturate(1.15);
  border-left-color: var(--border-soft);
}
/* 真实透底（玻璃透明度 < 0.9 或沉浸模式）：去掉白描边只留中性落影，文字加一档光晕（口径同 .card） */
html[data-wallpaper-clear='1'] .ext-drawer {
  border-left-color: transparent;
  box-shadow: -8px 0 30px rgba(0, 0, 0, 0.2);
  text-shadow: 0 1px 2px rgba(255, 255, 255, 0.5), 0 0 4px rgba(255, 255, 255, 0.3);
}
html[data-theme='dark'][data-wallpaper-clear='1'] .ext-drawer {
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5), 0 0 4px rgba(0, 0, 0, 0.3);
}
.ext-drawer-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-soft);
}
.ext-drawer-title {
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-drawer-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.ext-drawer-close:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ext-drawer-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.ext-drawer-body :deep(.extension-view) {
  flex: 1;
  min-width: 0;
  min-height: 0;
}
.view-layout-editor {
  padding: 0 20px 20px 0;
}
.view-chat-hint {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-3);
  text-align: center;
  padding: 0 var(--space-6);
}
.view-chat-hint svg {
  color: var(--text-3);
  opacity: 0.6;
}
.view-chat-hint p {
  font-size: 0.8125rem;
  line-height: 1.6;
  max-width: 360px;
}

@media (max-width: 1100px) {
  .workspace { padding: 0 10px 10px 0; }
  /* 窄窗口下视图外边距交给 workspace，避免叠加 */
  .dash-grid { padding: 0; }
  .view-notes { padding: 0; }
  .view-suda { padding: 0; }
  .view-settings { padding: 0; }
}

/* 窄窗口：保持 6 列（拖拽坐标与压缩算法固定按 6 列计算），仅收窄外边距 */
@media (max-width: 720px) {
  .app-body { grid-template-columns: 1fr; }
  .app-body.collapsed { grid-template-columns: 1fr; }
  .sidebar { position: relative; max-height: 280px; }
  .sidebar.collapsed { padding: 0 var(--space-3) var(--space-3); }
  .sidebar.collapsed .sidebar-nav-item,
  .sidebar.collapsed .sidebar-status {
    justify-content: flex-start;
    padding: 0 var(--space-3);
  }
  .sidebar.collapsed .sidebar-nav-item span,
  .sidebar.collapsed .sidebar-status span {
    display: initial;
  }
  .workspace { padding: 0 var(--space-2) var(--space-2) 0; overflow-y: auto; }
}

/* 轻提示 */
.toast {
  position: fixed;
  top: 56px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 500;
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--text-1);
  color: var(--text-on-accent);
  font-size: 0.8125rem;
  font-weight: 500;
  padding: 9px 18px;
  border-radius: var(--radius-pill);
  box-shadow: var(--shadow-dock);
  max-width: 70vw;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: auto;
}
.toast-msg {
  overflow: hidden;
  text-overflow: ellipsis;
}
.toast-action {
  flex-shrink: 0;
  border: none;
  background: color-mix(in srgb, var(--text-on-accent) 10%, transparent);
  color: inherit;
  font-size: 0.75rem;
  font-weight: 700;
  padding: 3px 10px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition: background 0.15s;
}
.toast-action:hover {
  background: color-mix(in srgb, var(--text-on-accent) 14%, transparent);
}
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.2s ease-out, transform 0.2s ease-out;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-8px);
}
</style>
