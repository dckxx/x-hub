<script setup lang="ts">
import { computed, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import TitleBar from '../components/TitleBar.vue'
import CalendarCard from '../components/CalendarCard.vue'
import Suda from '../components/Suda.vue'
import NoteList from '../components/NoteList.vue'
import NoteEditor from '../components/NoteEditor.vue'
import GlobalSearch from '../components/GlobalSearch.vue'
import SettingsDialog from '../components/SettingsDialog.vue'
import { useStore } from '../stores/workbench'
import { isTauri } from '../api/tauri'
import type { Note, Resource } from '../api/tauri'
import { FileText, FolderOpen, LayoutDashboard, Moon, Settings2, Sun, ChevronLeft, ChevronRight } from 'lucide-vue-next'

const store = useStore()
const todayRef = ref<HTMLElement | null>(null)
const notesRef = ref<HTMLElement | null>(null)
const sudaRef = ref<HTMLElement | null>(null)

const navigation = [
  { id: 'dashboard', label: '工作台', icon: LayoutDashboard, target: 'today' },
  { id: 'notes', label: '速记', icon: FileText, target: 'notes' },
  { id: 'suda', label: '速达', icon: FolderOpen, target: 'suda' },
] as const

const activeNavigation = ref('dashboard')

// ---- 侧边栏收起（跨会话记忆） ----
const sidebarCollapsed = ref(localStorage.getItem('sidebar-collapsed') === '1')

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
  localStorage.setItem('sidebar-collapsed', sidebarCollapsed.value ? '1' : '0')
}

function focusPanel(target: (typeof navigation)[number]['target'], id: string) {
  activeNavigation.value = id
  const panel = {
    today: todayRef,
    notes: notesRef,
    suda: sudaRef,
  }[target]
  panel.value?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  panel.value?.focus({ preventScroll: true })
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
})

function revealWindow() {
  if (isTauri()) getCurrentWindow().show()
}

// ---- 笔记选中与操作 ----
const activeNoteId = ref<number | null>(null)

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

onMounted(() => window.addEventListener('keydown', onSearchKeydown))
onUnmounted(() => window.removeEventListener('keydown', onSearchKeydown))
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
            :class="{ active: activeNavigation === item.id }"
            :aria-current="activeNavigation === item.id ? 'page' : undefined"
            type="button"
            @click="focusPanel(item.target, item.id)"
          >
            <span class="sidebar-nav-icon" aria-hidden="true">
              <component :is="item.icon" :size="16" :stroke-width="2" />
            </span>
            <span>{{ item.label }}</span>
          </button>
        </nav>

        <button class="sidebar-status" type="button" aria-label="打开设置" @click="settingsVisible = true">
          <Settings2 :size="15" :stroke-width="2" aria-hidden="true" />
          <span>本地工作台</span>
          <span class="status-dot" aria-hidden="true"></span>
        </button>
        <button
          class="sidebar-status sidebar-theme"
          type="button"
          :aria-label="isDark ? '切换到亮色模式' : '切换到暗色模式'"
          @click="toggleTheme"
        >
          <component :is="isDark ? Sun : Moon" :size="15" :stroke-width="2" aria-hidden="true" />
          <span>{{ isDark ? '亮色模式' : '暗色模式' }}</span>
        </button>
        <button
          class="sidebar-status sidebar-collapse"
          type="button"
          :aria-label="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
          :title="sidebarCollapsed ? '展开侧边栏' : '收起侧边栏'"
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
        <div class="workspace-grid">
          <section ref="todayRef" class="workspace-panel today-panel" tabindex="-1" aria-label="日历">
            <CalendarCard />
          </section>

          <section ref="notesRef" class="workspace-panel notes-panel" tabindex="-1" aria-label="速记">
            <NoteList
              :notes="store.state.notes"
              :active-id="activeNoteId"
              @select="onSelectNote"
              @create="onCreateNote"
              @delete="onDeleteNote"
            />
          </section>

          <section ref="sudaRef" class="workspace-panel suda-panel" tabindex="-1" aria-label="速达">
            <Suda />
          </section>
        </div>
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
    />
    <SettingsDialog
      :visible="settingsVisible"
      @close="settingsVisible = false"
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
  padding: var(--space-3);
  background: var(--bg-sidebar);
  border-right: 1px solid var(--border-soft);
  overflow: hidden;
  transition: padding 0.18s ease-out;
}
.sidebar.collapsed {
  padding: var(--space-3) 8px;
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
  padding: 0 var(--space-3) 0 32px;
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
.sidebar.collapsed .sidebar-nav-item span,
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
.workspace {
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-6);
  background: var(--bg-page);
}
.workspace-grid {
  display: grid;
  grid-template-columns: minmax(300px, 0.8fr) minmax(420px, 1.2fr);
  grid-template-rows: minmax(320px, 360px) minmax(240px, 1fr);
  gap: var(--space-4);
  min-height: 100%;
}
.workspace-panel {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
}
.workspace-panel:focus-visible { box-shadow: var(--shadow-focus); }
.today-panel { padding: var(--space-5); }
.today-panel :deep(.calendar) {
  padding: 0 0 var(--space-3);
  border: 0;
  border-radius: 0;
  box-shadow: none;
  background: transparent;
}
.notes-panel :deep(.note-list),
.suda-panel :deep(.suda) {
  border: 0;
  border-radius: 0;
  box-shadow: none;
}
.suda-panel { grid-column: 1 / -1; }

@media (max-width: 1100px) {
  .workspace { padding: var(--space-5); }
  .workspace-grid { grid-template-columns: minmax(280px, 0.78fr) minmax(380px, 1.22fr); }
}

@media (max-width: 720px) {
  .app-body { grid-template-columns: 1fr; }
  .app-body.collapsed { grid-template-columns: 1fr; }
  .sidebar { position: relative; max-height: 280px; border-right: 0; border-bottom: 1px solid var(--border-soft); }
  .sidebar.collapsed { padding: var(--space-3); }
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
  .workspace { padding: var(--space-4); }
  .workspace-grid { display: flex; flex-direction: column; }
  .workspace-panel { min-height: 320px; }
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
  color: var(--bg-card);
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
  background: color-mix(in srgb, var(--bg-card) 22%, transparent);
  color: inherit;
  font-size: 12px;
  font-weight: 700;
  padding: 3px 10px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition: background 0.15s;
}
.toast-action:hover {
  background: color-mix(in srgb, var(--bg-card) 38%, transparent);
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
