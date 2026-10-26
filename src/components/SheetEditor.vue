<script setup lang="ts">
import { inject, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { LoaderCircle, Sheet, Trash2, TriangleAlert } from 'lucide-vue-next'
import type { Note } from '../api/tauri'
import { isTauri, tauriApi } from '../api/tauri'
import { sheetStoredNameFromContent } from '../utils/noteSheetRef'

/**
 * 速记「表格笔记」编辑器（速记视图右栏，与 NoteEditor 同位分流，#t70）：
 * 打开 = fetch xhub-sheet 协议字节 → exceljs 解析 → Univer createWorkbook；
 * 保存 = univerAPI 快照 → exceljs 生成 xlsx/csv → save_note_sheet 就地写回同名文件。
 * 自动保存：任何编辑命令（CommandExecuted）置脏，1.2s 防抖落盘；切笔记/卸载前
 * 同步取快照再异步写盘（快照是纯数据，不依赖 Univer 实例存活）。
 * Univer / exceljs / 转换层全部动态 import：数 MB 级依赖只有真正打开表格时才加载。
 */

const props = defineProps<{
  note: Readonly<Note>
}>()

const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
}>()

const showToast = inject<((message: string) => void) | undefined>('showToast', undefined)

const containerEl = ref<HTMLDivElement | null>(null)
const loading = ref(false)
const error = ref('')
/** dirty = 表格有未写盘的修改；saving = 写盘进行中（标题防抖保存不算） */
const dirty = ref(false)
const saving = ref(false)

const localTitle = ref(props.note.title)
let titleTimer: ReturnType<typeof setTimeout> | null = null

type UniverBundle = {
  univer: { dispose: () => void }
  univerAPI: {
    createWorkbook: (data: unknown) => { getId: () => string } | null
    getActiveWorkbook: () => { getId: () => string; save: () => unknown } | null
    disposeUnit: (id: string) => boolean
    addEvent: (event: string, callback: () => void) => unknown
    Event: { CommandExecuted: string }
  }
}
let bundle: UniverBundle | null = null
/** 同一时刻只允许一次初始化/销毁（快速切换笔记防重入） */
let lifecycleSeq = 0
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null

function storedName(): string | null {
  return sheetStoredNameFromContent(props.note.content)
}

async function initEditor() {
  const stored = storedName()
  if (!containerEl.value || !stored) return
  const seq = ++lifecycleSeq
  loading.value = true
  error.value = ''
  dirty.value = false
  try {
    const { fetchSheetBytes, sheetBytesToUniver } = await import('../utils/noteSheet')
    // fetchSheetBytes 按协议 URL 末段判扩展名（xlsx/csv 分流解析），必须传完整 URL
    const { bytes, ext } = await fetchSheetBytes(props.note.content.trim())
    if (seq !== lifecycleSeq) return
    const workbook = await sheetBytesToUniver(bytes, props.note.title, ext)
    if (seq !== lifecycleSeq || !containerEl.value) return
    const mod = await import('@univerjs/presets')
    const presetMod = await import('@univerjs/preset-sheets-core')
    const localeZh = (await import('@univerjs/preset-sheets-core/locales/zh-CN')).default
    if (seq !== lifecycleSeq || !containerEl.value) return
    const created = mod.createUniver({
      locale: mod.LocaleType.ZH_CN,
      locales: { [mod.LocaleType.ZH_CN]: mod.mergeLocales(localeZh) },
      presets: [
        presetMod.UniverSheetsCorePreset({ container: containerEl.value }),
      ],
    })
    created.univerAPI.createWorkbook(workbook)
    // 脏标记：任何命令执行（输入/格式/结构变更）即视为有未落盘修改；事件句柄随 univer.dispose() 一起释放
    created.univerAPI.addEvent(created.univerAPI.Event.CommandExecuted, () => {
      if (seq !== lifecycleSeq) return
      dirty.value = true
      scheduleAutoSave()
    })
    bundle = created as unknown as UniverBundle
  } catch (e) {
    if (seq !== lifecycleSeq) return
    console.error('表格编辑器初始化失败', e)
    error.value = typeof e === 'string' ? e : (e instanceof Error ? e.message : String(e))
  } finally {
    if (seq === lifecycleSeq) loading.value = false
  }
}

/** 销毁前先把快照同步取出来（fwb.save() 是纯数据导出），销毁后异步写盘不受影响 */
function teardownEditor() {
  lifecycleSeq++
  if (autoSaveTimer !== null) {
    clearTimeout(autoSaveTimer)
    autoSaveTimer = null
  }
  if (!bundle) return
  let snapshot: unknown = null
  try {
    const fwb = bundle.univerAPI.getActiveWorkbook()
    if (fwb && dirty.value) snapshot = fwb.save()
  } catch {
    /* 实例可能已半初始化 */
  }
  try {
    const fwb = bundle.univerAPI.getActiveWorkbook()
    if (fwb) bundle.univerAPI.disposeUnit(fwb.getId())
  } catch {
    /* 同上 */
  }
  try {
    bundle.univer.dispose()
  } catch {
    /* 同上 */
  }
  bundle = null
  if (snapshot != null) void persistSnapshot(snapshot)
}

function scheduleAutoSave() {
  if (autoSaveTimer !== null) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    autoSaveTimer = null
    void saveSheet()
  }, 1200)
}

async function saveSheet() {
  if (!bundle || !isTauri()) return
  const fwb = bundle.univerAPI.getActiveWorkbook()
  if (!fwb) return
  await persistSnapshot(fwb.save())
}

/** 快照 → exceljs 生成 xlsx/csv → 就地写回 notes/files 原文件（与打开时同一文件名） */
async function persistSnapshot(snapshot: unknown) {
  const stored = storedName()
  if (!stored) return
  saving.value = true
  try {
    const { univerToSheetBytes } = await import('../utils/noteSheet')
    const ext = stored.split('.').pop() === 'csv' ? 'csv' : 'xlsx'
    const bytes = await univerToSheetBytes(snapshot as Parameters<typeof univerToSheetBytes>[0], ext)
    const u8 = new Uint8Array(bytes)
    let binary = ''
    const chunk = 0x8000
    for (let i = 0; i < u8.length; i += chunk) {
      binary += String.fromCharCode(...u8.subarray(i, i + chunk))
    }
    await tauriApi.saveNoteSheet(stored, btoa(binary))
    dirty.value = false
  } catch (e) {
    console.error('表格保存失败', e)
    showToast?.(`表格保存失败：${typeof e === 'string' ? e : e instanceof Error ? e.message : String(e)}`)
  } finally {
    saving.value = false
  }
}

// ---- 标题：600ms 防抖走 saveNote 管线（content 不变，只改名；同 NoteEditor 口径） ----
function onTitleInput() {
  if (titleTimer !== null) clearTimeout(titleTimer)
  titleTimer = setTimeout(() => {
    titleTimer = null
    const t = localTitle.value.trim() || '未命名表格'
    if (t !== props.note.title) emit('save', props.note.id, t, props.note.content)
  }, 600)
}

function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    e.stopPropagation()
    if (autoSaveTimer !== null) {
      clearTimeout(autoSaveTimer)
      autoSaveTimer = null
    }
    void saveSheet()
  }
}

// 初次挂载 / 切换表格笔记：销毁旧实例（dirty 先落盘）再重建
watch(
  () => props.note.id,
  () => {
    localTitle.value = props.note.title
    teardownEditor()
    void nextTick(() => void initEditor())
  },
)

onMounted(() => {
  window.addEventListener('keydown', onKeydown, true)
  void nextTick(() => void initEditor())
})

onBeforeUnmount(() => {
  if (titleTimer !== null) clearTimeout(titleTimer)
  window.removeEventListener('keydown', onKeydown, true)
  teardownEditor()
})
</script>

<template>
  <div class="card sheet-panel">
    <header class="se-header">
      <span class="se-sheet-icon" aria-hidden="true">
        <Sheet :size="14" :stroke-width="1.8" />
      </span>
      <input
        v-model="localTitle"
        class="se-title-input"
        type="text"
        maxlength="80"
        placeholder="表格名称"
        @input="onTitleInput"
        @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
      />
      <span class="se-status" role="status">
        <template v-if="saving">保存中…</template>
        <template v-else-if="dirty">编辑中…</template>
        <template v-else>已保存</template>
      </span>
      <button
        class="icon-btn se-act"
        type="button"
        title="移入回收站"
        aria-label="移入回收站"
        @click="emit('delete', note.id)"
      >
        <Trash2 :size="14" :stroke-width="1.8" />
      </button>
    </header>

    <div class="sheet-body">
      <div ref="containerEl" class="sheet-container"></div>
      <div v-if="loading" class="sheet-overlay">
        <LoaderCircle :size="22" :stroke-width="2" class="se-spin" />
        <p>正在加载表格…</p>
      </div>
      <div v-else-if="error" class="sheet-overlay">
        <TriangleAlert :size="22" :stroke-width="2" />
        <p>{{ error }}</p>
        <button class="se-retry" type="button" @click="error = ''; void initEditor()">重试</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 面板布局对齐 NoteEditor 的 .editor-panel（scoped 不互通，自带一份） */
.sheet-panel {
  height: 100%;
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 16px 10px;
  overflow: hidden;
}
/* 头部/标题输入框观感对齐 NoteEditor 的 ed-header / ed-title-input（scoped 不互通，自带一份） */
.se-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}
.se-title-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  font-size: 1em;
  font-weight: 600;
  font-family: inherit;
  color: var(--text-1);
  outline: none;
  padding: 8px 14px;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.se-title-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.se-title-input::placeholder {
  color: var(--text-4);
  font-weight: 400;
}
.icon-btn.se-act {
  width: 28px;
  height: 28px;
  color: var(--text-3);
  flex-shrink: 0;
}
.icon-btn.se-act:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.se-sheet-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-md);
  color: var(--c-green-ink, var(--brand-500));
  background: color-mix(in srgb, var(--c-green, #22c55e) 14%, transparent);
  flex-shrink: 0;
}
.se-status {
  flex-shrink: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
  white-space: nowrap;
}
.sheet-body {
  position: relative;
  flex: 1;
  min-height: 0;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-card);
}
.sheet-container {
  position: absolute;
  inset: 0;
}
.sheet-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  background: color-mix(in srgb, var(--bg-card) 82%, transparent);
  color: var(--text-3);
  font-size: 0.8125rem;
  z-index: 5;
}
.se-spin {
  animation: se-rotate 0.9s linear infinite;
}
@keyframes se-rotate {
  to {
    transform: rotate(360deg);
  }
}
.se-retry {
  padding: 4px 12px;
  border: 1px solid var(--border-1);
  border-radius: var(--radius-sm);
  background: var(--bg-card);
  color: var(--text-2);
  font-size: 0.75rem;
  cursor: pointer;
}
.se-retry:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
</style>
