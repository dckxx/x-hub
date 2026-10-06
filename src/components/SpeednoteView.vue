<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, ref, watch } from 'vue'
import type { Note } from '../api/tauri'
import { useStore } from '../stores/workbench'
import NoteFolderTree from './NoteFolderTree.vue'
import ConfirmDialog from './ConfirmDialog.vue'

/**
 * 速记视图（docs/speednote-plan.md §5.1；2026-10-04 用户反馈改为**两栏**）：
 * 文件夹树（左，唯一导航面：树 + 搜索 + 标签筛选 + 回收站三种形态）+ 编辑器（右）。
 * 中栏笔记列表已删——树里已经列出笔记，再放一列是重复信息。
 * - 树根 = 未归类笔记（不做「全部/未归类」虚拟节点）；选中树根时树根层笔记可见可直接点开。
 * - 新建落**当前选中文件夹**（树根选中即落根）。
 * - 删除一律软删（回收站），永久删除需确认。
 */
const NoteEditor = defineAsyncComponent(() => import('./NoteEditor.vue'))

const store = useStore()

// ---- 选中与形态（query/trashMode 由树面板经 v-model 双向绑定；标签筛选以 # 语法并入搜索词） ----
const selectedFolderId = ref<number | null>(null) // null = 树根
const activeNoteId = ref<number | null>(null)
const query = ref('')
const trashMode = ref(false)
const trashedNotes = ref<Note[]>([])

const activeNote = computed<Note | null>(() => {
  if (activeNoteId.value == null) return null
  if (trashMode.value) return trashedNotes.value.find((n) => n.id === activeNoteId.value) ?? null
  return store.state.notes.find((n) => n.id === activeNoteId.value) ?? null
})

// ---- 选择（单一选中模型：文件夹与笔记互斥高亮） ----
// 点笔记 = 进入编辑（清文件夹选中，新建落点改随打开笔记所在文件夹）；
// 点文件夹 = 浏览该文件夹（清笔记选中，编辑器回空态）。
function onSelectFolder(id: number | null) {
  selectedFolderId.value = id
  activeNoteId.value = null
  if (trashMode.value) trashMode.value = false
}

function onSelectNote(id: number) {
  activeNoteId.value = id
  selectedFolderId.value = null
}

watch(trashMode, async (on) => {
  if (on) {
    trashedNotes.value = await store.loadTrashedNotes()
    query.value = ''
  }
})

// 选中文件夹被删除/不存在时回到树根
watch(
  () => store.state.noteFolders,
  (folders) => {
    if (selectedFolderId.value != null && !folders.some((f) => f.id === selectedFolderId.value)) {
      selectedFolderId.value = null
    }
  },
  { deep: false },
)

// ---- 新建（落点：选中文件夹 > 打开中笔记所在文件夹 > 树根） ----
async function createNote() {
  if (trashMode.value) trashMode.value = false
  query.value = ''
  // 先结清上一条「刚新建」标记：watch 是异步队列，等它触发时本条 id 已写入标记，
  // 逐次新建会漏掉前一条的清理（连续按热键新建空笔记的场景）
  if (pendingEmptyNoteId != null) schedulePristineCleanup()
  const folderId = selectedFolderId.value ?? activeNote.value?.folder_id ?? null
  const n = await store.addNote('无标题笔记', folderId)
  activeNoteId.value = n.id
  selectedFolderId.value = null
  // 标记「刚新建」：切走时仍未输入任何内容 → 直接物理删除（不留空壳，见下方延迟复查）
  pendingEmptyNoteId = n.id
}

/** 树右键「新建笔记（在某文件夹）」：直接落到指定文件夹 */
async function createNoteIn(folderId: number) {
  if (trashMode.value) trashMode.value = false
  query.value = ''
  if (pendingEmptyNoteId != null) schedulePristineCleanup()
  const n = await store.addNote('无标题笔记', folderId)
  activeNoteId.value = n.id
  selectedFolderId.value = null
  pendingEmptyNoteId = n.id
}

// ---- 空笔记自清理：新建后没有任何输入就切走时直接删除 ----
// 不能在切换瞬间判定：NoteEditor 的防抖保存（600ms）可能还在途，此时正文仍读作空，
// 会误删用户刚输入的内容。统一延迟 800ms 后按**落库状态**复查——内容非空或标题已
// 派生/修改即保留；仍为「空正文 + 默认标题」才删。物理删除（软删会给回收站塞垃圾）。
let pendingEmptyNoteId: number | null = null
const PRISTINE_RECHECK_MS = 800

function isPristineEmpty(n: Note | undefined): boolean {
  if (!n) return false
  // 正文可能只含空行占位符（NBSP 旧版 / 零宽空格现版，见 NoteEditor.normalizeEmptyParagraphs），
  // 归一后判断；标题仍为默认值才算没动过
  const text = n.content.replace(/\u00A0|\u200B/g, ' ')
  return text.trim() === '' && (n.title === '' || n.title === '无标题笔记')
}

function schedulePristineCleanup() {
  const id = pendingEmptyNoteId
  pendingEmptyNoteId = null
  if (id == null) return
  setTimeout(() => {
    if (isPristineEmpty(store.state.notes.find((x) => x.id === id))) {
      void store.purgeNote(id)
    }
  }, PRISTINE_RECHECK_MS)
}

watch(activeNoteId, (_next, prev) => {
  if (typeof prev === 'number' && prev === pendingEmptyNoteId) schedulePristineCleanup()
})

onBeforeUnmount(() => {
  // 切出速记视图（卸载）同样触发自清理；NoteEditor 卸载时的 flush 保存会先于 800ms 复查落库
  schedulePristineCleanup()
})

// ---- 删除 / 回收站 ----
const confirmState = ref<{
  visible: boolean
  title: string
  message: string
  hint?: string
  confirmText: string
  tone?: 'default' | 'danger'
  onConfirm: () => void
} | null>(null)

function onTrashNote(id: number) {
  void store.trashNote(id)
  if (activeNoteId.value === id) activeNoteId.value = null
  // 回收站列表里若正显示该笔记（不可能：软删后才进回收站），无需处理
}

// ---- 一键清空回收站（树头触发，此处确认） ----
function onPurgeAll() {
  const n = trashedNotes.value.length
  if (n === 0) return
  confirmState.value = {
    visible: true,
    title: '清空回收站',
    message: `回收站里的 ${n} 条笔记将被永久删除，清空之后就无法找回了。`,
    confirmText: '清空',
    tone: 'danger',
    onConfirm: async () => {
      await store.purgeAllTrashedNotes()
      trashedNotes.value = []
      activeNoteId.value = null
    },
  }
}

function onPurgeNote(id: number) {
  const target = trashedNotes.value.find((n) => n.id === id)
  confirmState.value = {
    visible: true,
    title: '永久删除笔记',
    message: `「${target?.title ?? '无标题笔记'}」将被永久删除，此操作不可撤销。`,
    confirmText: '永久删除',
    tone: 'danger',
    onConfirm: async () => {
      await store.purgeNote(id)
      trashedNotes.value = trashedNotes.value.filter((n) => n.id !== id)
      if (activeNoteId.value === id) activeNoteId.value = null
    },
  }
}

async function onRestoreNote(id: number) {
  await store.restoreNote(id)
  trashedNotes.value = trashedNotes.value.filter((n) => n.id !== id)
  if (activeNoteId.value === id) activeNoteId.value = null
}

// ---- 文件夹操作 ----
function onCreateFolder(parentId: number | null, name: string) {
  void store.createNoteFolder(name, parentId)
}

function onRenameFolder(id: number, name: string) {
  void store.renameNoteFolder(id, name)
}

function onDeleteFolder(id: number) {
  const f = store.state.noteFolders.find((x) => x.id === id)
  if (!f) return
  confirmState.value = {
    visible: true,
    title: '删除文件夹',
    message: `「${f.name}」将被删除，其中的笔记与子文件夹会**上移一级**，不会丢失。`,
    confirmText: '删除文件夹',
    tone: 'danger',
    onConfirm: () => store.deleteNoteFolder(id),
  }
}

function onMoveNote(noteId: number, folderId: number | null) {
  void store.setNoteFolder(noteId, folderId)
}

function onReorderFolders(moves: { id: number; parent_id: number | null; sort_order: number }[]) {
  void store.reorderNoteFolders(moves)
}

function onConfirmDialogConfirm() {
  const c = confirmState.value
  confirmState.value = null
  void c?.onConfirm()
}

// ---- 编辑器 ----
function onSaveNote(id: number, title: string, content: string) {
  void store.saveNote(id, title, content)
}

/** 编辑器空态落点提示：与 createNote 实际落点一致（选中文件夹 > 根目录），不空承诺 */
const newNoteHint = computed(() => {
  const fid = selectedFolderId.value ?? activeNote.value?.folder_id ?? null
  if (fid == null) return '新笔记默认落到根目录'
  const f = store.state.noteFolders.find((x) => x.id === fid)
  return f ? `新笔记将落到「${f.name}」` : '新笔记默认落到根目录'
})

// ---- 外部入口（index.vue：全局搜索跳转 / 热键新建） ----
function openNote(id: number) {
  trashMode.value = false
  query.value = ''
  selectedFolderId.value = null
  activeNoteId.value = id
}

/** 编辑器内 [[链接]] / 双链面板跳转：直接切换选中 */
function onOpenNoteFromEditor(id: number) {
  activeNoteId.value = id
  selectedFolderId.value = null
}

defineExpose({ openNote, createNote })
</script>

<template>
  <section class="view view-notes sn-root" tabindex="-1" aria-label="速记">
    <NoteFolderTree
      v-model:query="query"
      v-model:trash-mode="trashMode"
      :folders="store.state.noteFolders"
      :notes="store.state.notes"
      :trashed-notes="trashedNotes"
      :selected-folder-id="selectedFolderId"
      :active-note-id="activeNoteId"
      @select-folder="onSelectFolder"
      @select-note="onSelectNote"
      @create-note="createNote"
      @create-note-in="createNoteIn"
      @create-folder="onCreateFolder"
      @rename-folder="onRenameFolder"
      @delete-folder="onDeleteFolder"
      @trash-note="onTrashNote"
      @restore-note="onRestoreNote"
      @purge-note="onPurgeNote"
      @purge-all="onPurgeAll"
      @move-note="onMoveNote"
      @reorder-folders="onReorderFolders"
    />

    <NoteEditor
      :note="activeNote"
      :new-note-hint="newNoteHint"
      @save="onSaveNote"
      @delete="onTrashNote"
      @open-note="onOpenNoteFromEditor"
      @create-note="createNote"
    />

    <!-- 确认弹窗：宿主层单实例（文件夹删除 / 永久删除共用） -->
    <ConfirmDialog
      :visible="confirmState?.visible ?? false"
      :title="confirmState?.title ?? ''"
      :message="confirmState?.message ?? ''"
      :hint="confirmState?.hint"
      :confirm-text="confirmState?.confirmText ?? '确认'"
      :tone="confirmState?.tone ?? 'default'"
      @confirm="onConfirmDialogConfirm"
      @cancel="confirmState = null"
    />
  </section>
</template>

<style scoped>
.sn-root {
  display: grid;
  grid-template-columns: minmax(230px, 300px) minmax(0, 1fr);
  gap: 12px;
  height: 100%;
  min-height: 0;
}

@media (max-width: 900px) {
  .sn-root {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(180px, 280px) minmax(0, 1fr);
  }
}
</style>
