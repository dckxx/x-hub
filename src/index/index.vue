<script setup lang="ts">
import { computed, onMounted, onUnmounted, provide, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import TitleBar from '../components/TitleBar.vue'
import TodoCard from '../components/TodoCard.vue'
import Suda from '../components/Suda.vue'
import NoteList from '../components/NoteList.vue'
import NoteEditor from '../components/NoteEditor.vue'
import GlobalSearch from '../components/GlobalSearch.vue'
import SettingsView from '../components/SettingsView.vue'
import TokenStatsCard from '../components/TokenStatsCard.vue'
import NotesOverviewCard from '../components/NotesOverviewCard.vue'
import TodoOverviewCard from '../components/TodoOverviewCard.vue'
import ResourcesOverviewCard from '../components/ResourcesOverviewCard.vue'
import SysMonitorCard from '../components/SysMonitorCard.vue'
import PromptBoxCard from '../components/PromptBoxCard.vue'
import PromptManageDialog from '../components/PromptManageDialog.vue'
import UsageView from '../components/UsageView.vue'
import RecentBar from '../components/RecentBar.vue'
import ClockCard from '../components/ClockCard.vue'
import StickyCard from '../components/StickyCard.vue'
import CountdownCard from '../components/CountdownCard.vue'
import ChatPanel from '../components/ChatPanel.vue'
import WhatsNewDialog from '../components/WhatsNewDialog.vue'
import { useStore } from '../stores/workbench'
import { isTauri, tauriApi } from '../api/tauri'
import type { Countdown, Note, Resource, Todo } from '../api/tauri'
import { playChime } from '../utils/chime'
import { FileText, FolderOpen, Gauge, LayoutDashboard, MessageSquare, Settings, ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { useTheme } from '../composables/useTheme'

const store = useStore()

// 初始化三轴主题系统（应用 data-theme/data-preset/inline --accent，监听系统变化）
useTheme()

// ---- 视图切换（统一导航范式：每个侧栏项 = 一个独立视图） ----
const navigation = [
  { id: 'dashboard', label: '工作台', icon: LayoutDashboard },
  { id: 'notes', label: '速记', icon: FileText },
  { id: 'suda', label: '速达', icon: FolderOpen },
  { id: 'usage', label: '用量', icon: Gauge },
  { id: 'chat', label: '对话', icon: MessageSquare },
] as const

// 对话入口暂时隐藏（后续恢复），功能仍可通过标题栏按钮 / Ctrl+Shift+K 唤起
const visibleNavigation = navigation.filter((item) => item.id !== 'chat')

// 设置不在顶部导航列表，作为独立入口固定在侧栏左下角，但同样是视图切换逻辑
type ViewId = (typeof navigation)[number]['id'] | 'settings'
const activeView = ref<ViewId>('dashboard')

// 对话入口：点击侧栏「对话」即唤起右侧面板（面板是主形态，视图仅占位说明）
function onNavClick(id: ViewId) {
  activeView.value = id
  if (id === 'chat' && !chatOpen.value) {
    toggleChat()
  }
}

// ---- 侧边栏收起（展开功能默认关闭，侧栏默认收起；开启后显示展开/收起按钮） ----
const sidebarCollapsed = ref(true)

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
}

function openUsageDetail() {
  activeView.value = 'usage'
}

function openPromptManage() {
  promptManageVisible.value = true
}

// ---- 主页面「中上区块」内容（Token 统计 / 速记统计 / 待办概览 / 速达数量 / 倒计时，设置中切换） ----
const dashMidContent = computed(() => store.state.config.dashboard_mid_content)
const dashMidCard = computed(() => {
  switch (dashMidContent.value) {
    case 'notes':
      return NotesOverviewCard
    case 'todo':
      return TodoOverviewCard
    case 'resources':
      return ResourcesOverviewCard
    case 'countdown':
      return CountdownCard
    default:
      return TokenStatsCard
  }
})
const dashMidProps = computed(() => {
  switch (dashMidContent.value) {
    case 'token':
      return { onOpenDetail: openUsageDetail }
    case 'notes':
      return { onOpenDetail: openNotes }
    case 'todo':
      return { onOpenDetail: openTodo }
    case 'resources':
      return { onOpenDetail: openSuda }
    default:
      return {}
  }
})

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
  }
  window.addEventListener('keydown', onSearchKeydown)
  window.addEventListener('keydown', onChatKeydown)
  await restoreChatPanel()
  // 升级检测：仅在版本变化且用户开启「升级后显示更新说明」时弹一次 What's New
  if (isTauri()) {
    try {
      const latest = await tauriApi.checkWhatsNew()
      if (latest) whatsNewContent.value = latest
    } catch {
      // 忽略：命令未就绪或检测失败时不打扰
    }
  }
})

let unlistenStickies: (() => void) | null = null
let unlistenCountdownFired: (() => void) | null = null
let unlistenCountdownsChanged: (() => void) | null = null
let unlistenNotesChanged: (() => void) | null = null
let unlistenSnippetsChanged: (() => void) | null = null

onUnmounted(() => {
  store.stopOnlineMonitor()
  unlistenStickies?.()
  unlistenCountdownFired?.()
  unlistenCountdownsChanged?.()
  unlistenNotesChanged?.()
  unlistenSnippetsChanged?.()
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
const whatsNewContent = ref<string | null>(null)

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

// ---- AI 对话右侧面板 ----
const chatOpen = ref(false)
const chatWidth = ref(420)

function toggleChat() {
  chatOpen.value = !chatOpen.value
  // 窗口开关状态不持久化：重启后始终默认收起，仅保存宽度
  if (isTauri()) void tauriApi.setChatPanel(chatWidth.value, false)
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
    const [w] = await tauriApi.getChatPanel()
    chatWidth.value = w
    // 启动时始终默认收起（开关状态不持久化）
    chatOpen.value = false
  } catch {
    // 忽略：命令未就绪时保持默认收起
  }
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
      </aside>

      <div class="main-area">
        <main class="workspace" aria-label="主工作区">
        <!-- 工作台：时钟/系统/便签 + Token 统计/提示词 + 待办 + 最近使用 -->
        <div v-if="activeView === 'dashboard'" class="dash-grid">
          <div class="dash-panel dash-left">
            <ClockCard class="dash-clock" />
            <SysMonitorCard class="dash-sysmon" />
            <div class="dash-stickies">
              <StickyCard :slot="1" />
              <StickyCard :slot="2" />
            </div>
          </div>
          <component
            :is="dashMidCard"
            class="dash-panel dash-usage"
            :on-open-detail="dashMidProps.onOpenDetail"
          />
          <PromptBoxCard class="dash-panel dash-prompts" :on-open-manage="openPromptManage" />
          <TodoCard class="dash-panel dash-todo" :highlight-id="highlightTodoId" />
          <RecentBar class="dash-panel dash-recent" @go-suda="activeView = 'suda'" />
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

        <!-- 用量：独立视图 -->
        <section v-else-if="activeView === 'usage'" class="view view-usage" tabindex="-1" aria-label="用量">
          <UsageView />
        </section>

        <!-- 对话：独立视图（完整视图，与右侧面板共用会话数据） -->
        <section v-else-if="activeView === 'chat'" class="view view-chat" tabindex="-1" aria-label="对话">
          <div class="view-chat-hint">
            <MessageSquare :size="20" :stroke-width="1.8" />
            <p>右侧面板已是最佳对话形态，可点击标题栏对话按钮或按 Ctrl+Shift+K 唤起。</p>
          </div>
        </section>

        <!-- 设置：独立视图 -->
        <section v-else class="view view-settings" tabindex="-1" aria-label="设置">
          <SettingsView :initial-section="settingsSection" />
        </section>
        </main>

        <!-- AI 对话抽屉（覆盖式，悬浮在内容上方，可拖拽调宽） -->
        <Transition name="chat-drawer">
          <div
            v-if="chatOpen"
            class="chat-dock"
            :style="{ opacity: store.state.config.chat_panel_opacity ?? 1 }"
          >
            <ChatPanel
              @toggle="onChatToggle"
              @open-model-settings="onOpenChatSettings"
            />
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
    <WhatsNewDialog
      :content="whatsNewContent"
      @close="whatsNewContent = null"
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

/* 覆盖式抽屉：absolute 悬浮于工作区之上，右侧滑入 */
.main-area .chat-dock {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  z-index: 40;
  pointer-events: none;
}
.main-area .chat-dock :deep(.chat-panel) {
  height: 100%;
  pointer-events: auto;
}

.chat-drawer-enter-active,
.chat-drawer-leave-active {
  transition: transform 0.24s ease-out;
}
.chat-drawer-enter-from,
.chat-drawer-leave-to {
  transform: translateX(100%);
}

/* 工作台布局：三列（时钟/系统/便签 | Token/提示词 | 待办）+ 底部最近使用通栏 */
.dash-grid {
  height: 100%;
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) minmax(0, 1.8fr) minmax(0, 1fr);
  grid-template-rows: auto minmax(0, 1fr) auto;
  gap: var(--space-4);
  /* 仅右下保留外边距（左上与 0.1.15 原版一致贴边），与各视图统一 */
  padding: 0 20px 20px 0;
}
.dash-panel {
  min-width: 0;
  min-height: 0;
}
.dash-left {
  grid-column: 1;
  grid-row: 1 / 3;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  min-height: 0;
  overflow-y: auto;
}
.dash-left .dash-clock {
  flex-shrink: 0;
}
.dash-left .dash-sysmon {
  flex-shrink: 0;
}
.dash-stickies {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}
.dash-usage { grid-column: 2; grid-row: 1; }
.dash-prompts { grid-column: 2; grid-row: 2; }
.dash-todo { grid-column: 3; grid-row: 1 / 3; }
.dash-recent { grid-column: 1 / -1; grid-row: 3; }

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
/* 用量页：覆盖组件内四边 20px padding，仅保留右下外边距（左上贴边与原版一致） */
.view-usage :deep(.usage-view) {
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

/* 960px 以下：三列改两列（左：时钟+系统+便签；右：Token+提示词/待办） */
@media (max-width: 960px) {
  .dash-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-template-rows: auto minmax(0, 1fr) auto auto;
  }
  .dash-left { grid-column: 1; grid-row: 1 / 3; }
  .dash-usage { grid-column: 2; grid-row: 1; }
  .dash-prompts { grid-column: 2; grid-row: 2; }
  .dash-todo { grid-column: 1; grid-row: 3; }
  .dash-recent { grid-column: 1 / -1; grid-row: 4; }
}

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
  .dash-grid { display: flex; flex-direction: column; gap: var(--space-4); }
  .dash-panel { flex: none; }
  .dash-clock { min-height: 120px; }
  .dash-sysmon { min-height: 110px; }
  .dash-stickies { min-height: 220px; }
  .dash-todo { min-height: 320px; }
  .dash-usage { min-height: 260px; }
  .dash-prompts { min-height: 280px; }
  .dash-recent { flex: none; }
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
