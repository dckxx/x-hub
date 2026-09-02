<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import TitleBar from '../components/TitleBar.vue'
import TodoCard from '../components/TodoCard.vue'
import Suda from '../components/Suda.vue'
import NoteList from '../components/NoteList.vue'
import NotesOverviewCard from '../components/NotesOverviewCard.vue'
import TodoOverviewCard from '../components/TodoOverviewCard.vue'
import ResourcesOverviewCard from '../components/ResourcesOverviewCard.vue'
import SysMonitorCard from '../components/SysMonitorCard.vue'
import PromptBoxCard from '../components/PromptBoxCard.vue'
import RecentBar from '../components/RecentBar.vue'
import ClockCard from '../components/ClockCard.vue'
import StickyCard from '../components/StickyCard.vue'
import CountdownCard from '../components/CountdownCard.vue'
import { useStore } from '../stores/workbench'
import { isTauri, tauriApi } from '../api/tauri'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { Countdown, ExtensionEntry, Note, Resource, Todo } from '../api/tauri'
import { playChime } from '../utils/chime'
import { FileText, FolderOpen, LayoutDashboard, MessageSquare, Puzzle, Settings, ChevronLeft, ChevronRight, AppWindow, PanelRight } from 'lucide-vue-next'
import type { Component } from 'vue'
import { useTheme, systemDarkMode } from '../composables/useTheme'
import { broadcastThemeToFrames } from '../composables/themeTokens'
import { iconSrc } from '../composables/useResourceIcon'
import { useDashboardLayout, type DashPlacement } from '../composables/useDashboardLayout'

// 大体量/低频视图异步分包按需加载，缩小首屏主 chunk
const NoteEditor = defineAsyncComponent(() => import('../components/NoteEditor.vue'))
const GlobalSearch = defineAsyncComponent(() => import('../components/GlobalSearch.vue'))
const SettingsView = defineAsyncComponent(() => import('../components/SettingsView.vue'))
const PromptManageDialog = defineAsyncComponent(() => import('../components/PromptManageDialog.vue'))
const ChatPanel = defineAsyncComponent(() => import('../components/ChatPanel.vue'))
const ExtensionCenter = defineAsyncComponent(() => import('../components/ExtensionCenter.vue'))
const ExtensionView = defineAsyncComponent(() => import('../components/ExtensionView.vue'))
const DashboardLayoutEditor = defineAsyncComponent(() => import('../components/DashboardLayoutEditor.vue'))

const store = useStore()

// 初始化三轴主题系统（应用 data-theme/data-preset/inline --accent，监听系统变化）
useTheme()

// ---- 应用壁纸：主窗口所有视图共用，浮窗不跟随；文件失效时静默回退渐变背景 ----
// 亮/暗主题各自一套壁纸配置：暗色未单独设置时跟随亮色（升级兼容，老用户观感不变）
const wallpaperFailed = ref(false)
const isDarkTheme = computed(() => {
  const mode = store.state.config.theme_mode
  return mode === 'dark' || (mode === 'system' && systemDarkMode().value)
})
watch(
  () => [store.state.config.wallpaper_path, store.state.config.wallpaper_path_dark] as const,
  () => {
    wallpaperFailed.value = false
  },
)
const wallpaperSrc = computed(() => {
  const { wallpaper_path, wallpaper_path_dark } = store.state.config
  const p = isDarkTheme.value ? wallpaper_path_dark || wallpaper_path : wallpaper_path
  if (!p || wallpaperFailed.value || !isTauri()) return ''
  return convertFileSrc(p)
})

// 蒙版不透明度钳制 0–0.85：85% 封顶保证壁纸仍可辨识；暗色负值 = 跟随亮色值
const wallpaperVeil = computed(() => {
  const clamp = (v: number) => Math.min(0.85, Math.max(0, v))
  if (isDarkTheme.value) {
    const dark = store.state.config.wallpaper_veil_dark
    if (dark >= 0) return clamp(dark)
  }
  return clamp(store.state.config.wallpaper_veil)
})

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
  { id: 'notes', label: '速记', icon: FileText },
  { id: 'suda', label: '速达', icon: FolderOpen },
  { id: 'chat', label: '对话', icon: MessageSquare },
] as const

// 对话入口暂时隐藏（后续恢复），功能仍可通过标题栏按钮 / Ctrl+Shift+K 唤起
const visibleNavigation = navigation.filter((item) => item.id !== 'chat')

// 设置不在顶部导航列表，作为独立入口固定在侧栏左下角，但同样是视图切换逻辑
// 自定义布局编辑器也是独立视图（从设置进入，完成后回主页面）
type ViewId = (typeof navigation)[number]['id'] | 'settings' | 'layout-editor' | 'extensions' | 'extension'
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
  extensionReloadTick.value++
  openedExtension.value = { id: ext.id, surface: null, name: ext.name }
  activeView.value = 'extension'
}

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

function openExtensionWindow() {
  if (!openedExtension.value) return
  tauriApi.openExtensionWindow(openedExtension.value.id).catch((e) => {
    showToast(`打开窗口失败：${String(e)}`)
  })
}

function openExtensionDrawer() {
  if (!openedExtension.value) return
  extensionReloadTick.value++
  drawerExtension.value = { ...openedExtension.value }
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
// 待办概览卡的「去待办」：待办卡就在工作台，直接切回工作台即可
function openTodo() {
  activeView.value = 'dashboard'
}

// ---- 工作台自定义布局（12 列单元格网格，模块库两栏编辑器） ----
const layout = useDashboardLayout()

// 模块 id → 组件 + props 映射（含原「中上可切换」的 5 个独立模块）
const dashCardComponents: Record<string, Component> = {
  clock: ClockCard,
  sysmon: SysMonitorCard,
  sticky1: StickyCard,
  sticky2: StickyCard,
  notes: NotesOverviewCard,
  todo_overview: TodoOverviewCard,
  resources: ResourcesOverviewCard,
  countdown: CountdownCard,
  prompts: PromptBoxCard,
  todo: TodoCard,
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
      onOpenSurface: (surface: string) => openExtensionSurface(extId, surface),
    }
  }
  switch (id) {
    case 'sticky1':
      return { slot: 1 }
    case 'sticky2':
      return { slot: 2 }
    case 'notes':
      return { onOpenDetail: openNotes }
    case 'todo_overview':
      return { onOpenDetail: openTodo }
    case 'resources':
      return { onOpenDetail: openSuda }
    case 'prompts':
      return { onOpenManage: openPromptManage }
    case 'todo':
      return { highlightId: highlightTodoId.value }
    case 'countdown':
      return { sizeW: p.w, sizeH: p.h }
    default:
      return {}
  }
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
  store.loadInitialData().finally(() => {
    store.startOnlineMonitor()
    const wait = Math.max(0, BOOT_MIN_MS - (performance.now() - bootStartAt))
    setTimeout(hideBootSplash, wait)
  })
  // 兜底：无论数据是否加载成功，最多 4s 后隐藏欢迎页，避免一直遮挡
  setTimeout(hideBootSplash, BOOT_MAX_MS)
  // 浮窗便签还原/删除后，主窗口实时同步便签与脱离状态
  if (isTauri()) {
    void refreshInstalledExtensions()
    unlistenStickies = await listen('stickies-changed', () => {
      store.refreshStickies()
    })
    // 倒计时到点：toast 提示 + 刷新列表（浮窗水罐同步）
    unlistenCountdownFired = await listen<Countdown>('countdown-fired', (e) => {
      const name = e.payload?.name ?? ''
      showToast(name ? `「${name}」时间到` : '倒计时时间到')
      if (store.state.config.countdown_sound) {
        playChime()
      }
      void store.refreshCountdowns()
    })
    // ticker 顺延 / 创建更新后同步
    unlistenCountdownsChanged = await listen('countdowns-changed', () => {
      void store.refreshCountdowns()
    })
    // 剪贴板浮层「存为速记 / 加入提示词库」后，主窗口实时刷新列表
    unlistenNotesChanged = await listen('notes-changed', () => {
      void store.refreshNotes()
    })
    unlistenSnippetsChanged = await listen('snippets-changed', () => {
      void store.loadSnippets()
    })
    unlistenTodosChanged = await listen('todos-changed', () => {
      void store.refreshTodos()
    })
    // 待办提醒到点：toast 提示（系统通知由后端 todo_reminder 直接发）
    unlistenTodoRemind = await listen<Todo>('todo-remind', (e) => {
      const title = e.payload?.title ?? ''
      showToast(title ? `待办提醒：「${title}」` : '待办提醒时间到')
    })
  }
  window.addEventListener('keydown', onSearchKeydown)
  window.addEventListener('keydown', onChatKeydown)
  await restoreChatPanel()
})

let unlistenStickies: (() => void) | null = null
let unlistenCountdownFired: (() => void) | null = null
let unlistenCountdownsChanged: (() => void) | null = null
let unlistenNotesChanged: (() => void) | null = null
let unlistenSnippetsChanged: (() => void) | null = null
let unlistenTodosChanged: (() => void) | null = null
let unlistenTodoRemind: (() => void) | null = null

onUnmounted(() => {
  store.stopOnlineMonitor()
  unlistenStickies?.()
  unlistenCountdownFired?.()
  unlistenCountdownsChanged?.()
  unlistenNotesChanged?.()
  unlistenSnippetsChanged?.()
  unlistenTodosChanged?.()
  unlistenTodoRemind?.()
  window.removeEventListener('keydown', onSearchKeydown)
  window.removeEventListener('keydown', onChatKeydown)
})

function hideBootSplash() {
  const el = document.getElementById('boot-splash')
  if (el && !el.classList.contains('hide')) {
    el.classList.add('hide')
    setTimeout(() => el.remove(), 450)
  }
}

// ---- 笔记选中与操作 ----
const activeNoteId = ref<number | null>(null)
const highlightTodoId = ref<number | null>(null)

const activeNote = computed(
  () => store.state.notes.find((n) => n.id === activeNoteId.value) ?? null,
)

async function onCreateNote() {
  const n = await store.addNote('无标题笔记')
  activeNoteId.value = n.id
}

function onSelectNote(id: number) {
  activeNoteId.value = id
}

async function onDeleteNote(id: number) {
  const target = store.state.notes.find((n) => n.id === id)
  if (!target) return
  await store.removeNote(id)
  if (activeNoteId.value === id) activeNoteId.value = null
  showToast('笔记已删除', {
    label: '撤销',
    onClick: async () => {
      const n = await store.addNote(target.title)
      await store.saveNote(n.id, target.title, target.content)
      activeNoteId.value = n.id
      showToast('已恢复笔记')
    },
  })
}

function onSaveNote(id: number, title: string, content: string) {
  store.saveNote(id, title, content)
}

// ---- 全局搜索 / 设置 ----
const searchVisible = ref(false)
const promptManageVisible = ref(false)
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

async function onOpenResource(r: Resource) {
  searchVisible.value = false
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法启动「${r.name}」：${String(e)}`)
  }
}

function onOpenNote(n: Note) {
  activeNoteId.value = n.id
  activeView.value = 'notes'
  searchVisible.value = false
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
            <span class="sidebar-nav-icon" aria-hidden="true">
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
            :style="{ gridTemplateRows: `repeat(${dashGridRows}, minmax(0, 1fr))` }"
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

        <!-- 速记：独立视图 -->
        <section v-else-if="activeView === 'notes'" class="view view-notes" tabindex="-1" aria-label="速记">
          <div class="notes-split">
            <NoteList
              :notes="store.state.notes"
              :active-id="activeNoteId"
              @select="onSelectNote"
              @create="onCreateNote"
              @delete="onDeleteNote"
            />
            <NoteEditor
              :note="activeNote"
              @save="onSaveNote"
              @delete="onDeleteNote"
            />
          </div>
        </section>

        <!-- 速达：独立视图 -->
        <section v-else-if="activeView === 'suda'" class="view view-suda" tabindex="-1" aria-label="速达">
          <Suda />
        </section>

        <!-- 扩展中心：独立视图 -->
        <section v-else-if="activeView === 'extensions'" class="view view-extensions" tabindex="-1" aria-label="扩展中心">
          <ExtensionCenter
            @open="onOpenExtension"
            @open-surface="(ext, surface) => openExtensionSurface(ext.id, surface)"
            @changed="onExtensionsChanged"
          />
        </section>

        <!-- 扩展运行视图：主区渲染扩展入口（iframe + window.xhub 桥 API） -->
        <section v-else-if="activeView === 'extension'" class="view view-extension" tabindex="-1" aria-label="扩展">
          <div class="ext-toolbar">
            <span class="ext-toolbar-name">{{ openedExtension?.name ?? '扩展' }}</span>
            <div class="ext-toolbar-spacer" />
            <button class="icon-btn" type="button" title="在窗口打开" aria-label="在窗口打开" @click="openExtensionWindow">
              <AppWindow :size="15" :stroke-width="2" />
            </button>
            <button class="icon-btn" type="button" title="在抽屉打开" aria-label="在抽屉打开" @click="openExtensionDrawer">
              <PanelRight :size="15" :stroke-width="2" />
            </button>
          </div>
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
          <SettingsView :initial-section="settingsSection" @open-layout-editor="openLayoutEditor" />
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
/* 扩展图标：统一「应用图标」质感——中性软底 + 细描边 + 内边距，与主导航线形图标视觉协调 */
.sidebar-ext .sidebar-nav-icon {
  background: var(--bg-card-soft);
  box-shadow: inset 0 0 0 1px var(--border-soft);
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

/* 工作台布局：12 列 fr 比例网格 + 行高 1fr 均分填满（缩放/分辨率只改每格像素值，布局结构不变） */
.dash-wrap {
  position: relative;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}
.dash-grid {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: var(--space-4);
  padding: 0 20px 20px 0;
  height: 100%;
}
.dash-cell {
  min-width: 0;
  min-height: 0;
  position: relative;
  display: flex;
  flex-direction: column;
  overflow: hidden;
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
/* 扩展运行视图：仅右下外边距 */
.view-extension {
  padding: 0 20px 20px 0;
}
.ext-toolbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0 12px;
}
.ext-toolbar-spacer {
  flex: 1;
}
.ext-toolbar-name {
  font-size: 0.8125rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-toolbar .icon-btn {
  width: 30px;
  height: 30px;
  color: var(--text-3);
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
.notes-split {
  display: flex;
  gap: 14px;
  height: 100%;
  min-height: 0;
}
.notes-split > *:first-child {
  flex: 0 0 300px;
  min-width: 0;
}
.notes-split > *:last-child {
  flex: 1;
  min-width: 0;
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
