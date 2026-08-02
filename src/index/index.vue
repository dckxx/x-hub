<script setup lang="ts">
import { computed, onMounted, onUnmounted, provide, ref, watch } from 'vue'
import TitleBar from '../components/TitleBar.vue'
import CalendarCard from '../components/CalendarCard.vue'
import QuickLaunch from '../components/QuickLaunch.vue'
import NoteList from '../components/NoteList.vue'
import NoteEditor from '../components/NoteEditor.vue'
import FileManager from '../components/FileManager.vue'
import AppDock from '../components/AppDock.vue'
import GlobalSearch from '../components/GlobalSearch.vue'
import SettingsDialog from '../components/SettingsDialog.vue'
import { useStore } from '../stores/workbench'
import type { FileEntry, Note, Resource } from '../api/tauri'

const store = useStore()

// ---- 主题（跟随配置，持久化） ----
const theme = computed(() => store.state.config.theme)
watch(
  theme,
  (t) => {
    document.documentElement.dataset.theme = t === 'dark' ? 'dark' : ''
  },
  { immediate: true },
)

// ---- 启动加载 ----
onMounted(() => {
  store.loadInitialData()
})

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
  await store.removeNote(id)
  if (activeNoteId.value === id) activeNoteId.value = null
  showToast('笔记已删除')
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

async function onOpenFile(f: FileEntry) {
  searchVisible.value = false
  try {
    await store.openFile(f.path)
  } catch (e) {
    showToast(`无法打开「${f.name}」：${String(e)}`)
  }
}

// ---- 轻提示 ----
const toastMsg = ref('')
let toastTimer: ReturnType<typeof setTimeout> | null = null

function showToast(msg: string) {
  toastMsg.value = msg
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toastMsg.value = ''
  }, 2200)
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

    <main class="main-area">
      <div class="left-col">
        <CalendarCard />
        <QuickLaunch />
      </div>
      <NoteList
        :notes="store.state.notes"
        :active-id="activeNoteId"
        @select="onSelectNote"
        @create="onCreateNote"
        @delete="onDeleteNote"
      />
      <FileManager />
    </main>

    <AppDock />

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
      @open-file="onOpenFile"
    />
    <SettingsDialog
      :visible="settingsVisible"
      @close="settingsVisible = false"
    />

    <Transition name="toast">
      <div v-if="toastMsg" class="toast">{{ toastMsg }}</div>
    </Transition>
  </div>
</template>

<style scoped>
.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-page);
  border-radius: 8px;
  overflow: hidden;
  position: relative;
}

.main-area {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 360px 240px minmax(0, 1fr);
  gap: 16px;
  padding: 16px 20px;
}
.main-area > * {
  min-height: 0;
  min-width: 0;
}
.left-col {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-height: 0;
  min-width: 0;
}

/* 轻提示 */
.toast {
  position: fixed;
  top: 56px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 500;
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
  pointer-events: none;
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
