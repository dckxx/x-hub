<script setup lang="ts">
import { computed, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import TitleBar from '../components/TitleBar.vue'
import TodoCard from '../components/TodoCard.vue'
import Suda from '../components/Suda.vue'
import NoteList from '../components/NoteList.vue'
import NoteEditor from '../components/NoteEditor.vue'
import GlobalSearch from '../components/GlobalSearch.vue'
import SettingsDialog from '../components/SettingsDialog.vue'
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
import { useStore } from '../stores/workbench'
import { isTauri } from '../api/tauri'
import type { Note, Resource, Todo } from '../api/tauri'
import { FileText, FolderOpen, Gauge, LayoutDashboard, Moon, Settings2, Sun, ChevronLeft, ChevronRight } from 'lucide-vue-next'

const store = useStore()

// ---- 视图切换（统一导航范式：每个侧栏项 = 一个独立视图） ----
const navigation = [
  { id: 'dashboard', label: '工作台', icon: LayoutDashboard },
  { id: 'notes', label: '速记', icon: FileText },
  { id: 'suda', label: '速达', icon: FolderOpen },
  { id: 'usage', label: '用量', icon: Gauge },
] as const

type ViewId = (typeof navigation)[number]['id']
const activeView = ref<ViewId>('dashboard')

// ---- 侧边栏收起（每次打开软件默认收起，会话内可手动展开） ----
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

// ---- 主页面「中上区块」内容（Token 统计 / 速记统计 / 待办概览 / 速达数量，设置中切换） ----
const dashMidContent = computed(() => store.state.config.dashboard_mid_content)
const dashMidCard = computed(() => {
  switch (dashMidContent.value) {
    case 'notes':
      return NotesOverviewCard
    case 'todo':
      return TodoOverviewCard
    case 'resources':
      return ResourcesOverviewCard
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
      return { onOpenDetail: openUsageDetail }
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

// ---- 主题（跟随配置，持久化） ----
const theme = computed(() => store.state.config.theme)
const isDark = computed(() => theme.value === 'dark')
watch(
  theme,
  (t) => {
    document.documentElement.dataset.theme = t === 'dark' ? 'dark' : ''
  },
  { immediate: true },
)

function toggleTheme() {
  store.setTheme(isDark.value ? 'light' : 'dark')
}

// ---- 启动加载：数据就绪后显示窗口，避免长时间空白 ----
onMounted(async () => {
  store.loadInitialData().then(() => revealWindow())
  // 兜底：无论数据是否加载成功，最多 1.5s 后显示窗口，避免一直不可见
  setTimeout(revealWindow, 1500)
  // 浮窗便签还原/删除后，主窗口实时同步便签与脱离状态
  if (isTauri()) {
    unlistenStickies = await listen('stickies-changed', () => {
      store.refreshStickies()
    })
  }
  window.addEventListener('keydown', onSearchKeydown)
})

let unlistenStickies: (() => void) | null = null

onUnmounted(() => {
  unlistenStickies?.()
  window.removeEventListener('keydown', onSearchKeydown)
})

function revealWindow() {
  if (isTauri()) getCurrentWindow().show()
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
const settingsVisible = ref(false)
const promptManageVisible = ref(false)

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
      @settings="settingsVisible = true"
    />

    <div class="app-body" :class="{ collapsed: sidebarCollapsed }">
      <aside
        class="sidebar"
        :class="{ collapsed: sidebarCollapsed }"
        aria-label="应用侧栏"
      >
        <nav class="sidebar-nav" aria-label="主要导航">
          <button
            v-for="item in navigation"
            :key="item.id"
            class="sidebar-nav-item"
            :class="{ active: activeView === item.id }"
            :aria-current="activeView === item.id ? 'page' : undefined"
            :data-tip="item.label"
            type="button"
            @click="activeView = item.id"
          >
            <span class="sidebar-nav-icon" aria-hidden="true">
              <component :is="item.icon" :size="16" :stroke-width="2" />
            </span>
            <span>{{ item.label }}</span>
          </button>
        </nav>

        <button class="sidebar-status" type="button" aria-label="打开设置" data-tip="设置" @click="settingsVisible = true">
          <Settings2 :size="15" :stroke-width="2" aria-hidden="true" />
          <span>本地工作台</span>
          <span class="status-dot" aria-hidden="true"></span>
        </button>
        <button
          class="sidebar-status sidebar-theme"
          type="button"
          :aria-label="isDark ? '切换到亮色模式' : '切换到暗色模式'"
          :data-tip="isDark ? '亮色模式' : '暗色模式'"
          @click="toggleTheme"
        >
          <component :is="isDark ? Sun : Moon" :size="15" :stroke-width="2" aria-hidden="true" />
          <span>{{ isDark ? '亮色模式' : '暗色模式' }}</span>
        </button>
        <button
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
          <NoteList
            :notes="store.state.notes"
            :active-id="activeNoteId"
            @select="onSelectNote"
            @create="onCreateNote"
            @delete="onDeleteNote"
          />
        </section>

        <!-- 速达：独立视图 -->
        <section v-else-if="activeView === 'suda'" class="view view-suda" tabindex="-1" aria-label="速达">
          <Suda />
        </section>

        <!-- 用量：独立视图 -->
        <section v-else class="view view-usage" tabindex="-1" aria-label="用量">
          <UsageView />
        </section>
      </main>
    </div>

    <NoteEditor
      :note="activeNote"
      @save="onSaveNote"
      @delete="onDeleteNote"
      @close="activeNoteId = null"
    />

    <GlobalSearch
      :visible="searchVisible"
      @close="searchVisible = false"
      @open-resource="onOpenResource"
      @open-note="onOpenNote"
      @open-todo="onOpenTodo"
    />
    <SettingsDialog
      :visible="settingsVisible"
      @close="settingsVisible = false"
    />
    <PromptManageDialog
      :visible="promptManageVisible"
      @close="promptManageVisible = false"
    />

    <Transition name="toast">
      <div v-if="toastMsg" class="toast">
        <span class="toast-msg">{{ toastMsg }}</span>
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
  font-size: 13px;
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
  font-size: 12px;
  text-align: left;
  border: 0;
  cursor: pointer;
}
.sidebar-status.sidebar-theme { margin-top: var(--space-1); }
.sidebar-nav + .sidebar-status { margin-top: auto; }
.sidebar-status:hover { color: var(--text-1); }
.status-dot {
  width: 6px;
  height: 6px;
  margin-left: auto;
  border-radius: 50%;
  background: var(--c-green-ink);
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
.sidebar.collapsed .sidebar-status span,
.sidebar.collapsed .status-dot {
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
  font-size: 12px;
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

/* 主工作区 */
.workspace {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  padding: 0 var(--space-3) var(--space-3) 0;
  background: transparent;
}

/* 工作台布局：三列（时钟/系统/便签 | Token/提示词 | 待办）+ 底部最近使用通栏 */
.dash-grid {
  height: 100%;
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) minmax(0, 1.8fr) minmax(0, 1fr);
  grid-template-rows: auto minmax(0, 1fr) auto;
  gap: var(--space-4);
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
  max-height: 440px;
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
/* 速记/速达视图：保留 .card 的边框/圆角/阴影，与其他卡片一致 */
.view-usage :deep(.usage-view) {
  padding: 0;
}

@media (max-width: 1100px) {
  .workspace { padding: 0 10px 10px 0; }
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
  .sidebar.collapsed .sidebar-status span,
  .sidebar.collapsed .status-dot {
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
  font-size: 13px;
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
  font-size: 12px;
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
