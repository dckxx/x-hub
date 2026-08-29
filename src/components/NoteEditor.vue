<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { marked } from 'marked'
import { Eye, Pencil, StickyNote, Tag as TagIcon, Trash2 } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { parseTimestamp } from '../utils/time'

const store = useStore()

const props = defineProps<{
  note: Readonly<Note> | null
}>()

const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
}>()

const localTitle = ref('')
const localContent = ref('')
const dirty = ref(false)
const syncing = ref(false)

// ---- Markdown 编辑/预览 ----
const mode = ref<'edit' | 'preview'>('edit')

const previewHtml = computed(() => {
  if (mode.value !== 'preview') return ''
  return marked.parse(localContent.value, { async: false }) as string
})

// ---- 标签 ----
const noteTags = ref<Tag[]>([])
const tagInputVisible = ref(false)
const tagInput = ref('')

// immediate：视图切换回来（或从全局搜索打开）时编辑器带着已选中的笔记重新挂载，
// id 不再变化，必须在挂载时同步一次，否则列表高亮选中而编辑器空白，且单条笔记无法通过重新点击触发恢复
watch(
  () => props.note?.id,
  async () => {
    // note 失效（关闭/删除/切换离开）前，把防抖中未落盘的编辑立即落盘，避免丢失
    if (!props.note) flushPendingSave()
    syncLocal()
    mode.value = 'edit'
    // 加载笔记标签
    if (props.note && isTauri()) {
      noteTags.value = await tauriApi.getNoteTags(props.note.id)
    } else {
      noteTags.value = []
    }
  },
  { immediate: true },
)

async function persistTags() {
  if (!props.note) return
  if (!isTauri()) return
  await tauriApi.setNoteTags(props.note.id, noteTags.value.map((t) => t.id))
}

async function addTag(tag: Tag) {
  if (noteTags.value.some((t) => t.id === tag.id)) return
  noteTags.value.push(tag)
  await persistTags()
}

function removeTag(tagId: number) {
  noteTags.value = noteTags.value.filter((t) => t.id !== tagId)
  void persistTags()
}

async function submitTagInput() {
  const name = tagInput.value.trim()
  if (!name) {
    tagInputVisible.value = false
    return
  }
  try {
    const t = await store.createTag(name)
    await addTag(t)
    tagInput.value = ''
  } catch (e) {
    console.error('创建标签失败', e)
  }
  tagInputVisible.value = false
}

let saveTimer: ReturnType<typeof setTimeout> | null = null
let lastNoteId: number | null = null

onBeforeUnmount(() => {
  flushPendingSave()
})

function syncLocal() {
  syncing.value = true
  localTitle.value = props.note?.title ?? ''
  localContent.value = props.note?.content ?? ''
  dirty.value = false
}

/** 立即落盘防抖中未保存的编辑（若存在），并取消挂起的定时器 */
function flushPendingSave() {
  if (!saveTimer) return
  clearTimeout(saveTimer)
  saveTimer = null
  if (dirty.value && lastNoteId !== null) {
    dirty.value = false
    emit('save', lastNoteId, localTitle.value, localContent.value)
  }
}

watch([localTitle, localContent], () => {
  if (syncing.value) {
    syncing.value = false
    return
  }
  if (!props.note) return
  dirty.value = true
  if (saveTimer) clearTimeout(saveTimer)
  lastNoteId = props.note.id
  saveTimer = setTimeout(() => {
    saveTimer = null
    if (props.note) {
      emit('save', props.note.id, localTitle.value, localContent.value)
    }
  }, 600)
})

watch(
  () => props.note?.updated_at,
  () => {
    dirty.value = false
  },
)

function formatSavedTime(iso: string): string {
  const t = new Date(parseTimestamp(iso))
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日${pad(t.getHours())}:${pad(t.getMinutes())}:${pad(t.getSeconds())}`
}
</script>

<template>
  <div class="card editor-panel">
    <!-- 空状态 -->
    <div v-if="!note" class="editor-empty">
      <StickyNote :size="48" :stroke-width="1.5" />
      <p>选择或新建笔记</p>
    </div>

    <!-- 编辑器内容 -->
    <template v-else>
      <header class="ed-header">
        <input
          ref="titleInputRef"
          v-model="localTitle"
          class="ed-title-input"
          type="text"
          maxlength="80"
          placeholder="笔记标题"
          @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
        />
        <button
          class="icon-btn mode-btn"
          :class="{ active: mode === 'edit' }"
          title="编辑模式"
          @click="mode = 'edit'"
        >
          <Pencil :size="13" :stroke-width="1.8" />
        </button>
        <button
          class="icon-btn mode-btn"
          :class="{ active: mode === 'preview' }"
          title="预览模式"
          @click="mode = 'preview'"
        >
          <Eye :size="13" :stroke-width="1.8" />
        </button>
        <button
          class="icon-btn del"
          title="删除笔记"
          aria-label="删除笔记"
          @click="emit('delete', note.id)"
        >
          <Trash2 :size="14" :stroke-width="1.8" />
        </button>
      </header>

      <textarea
        v-if="mode === 'edit'"
        v-model="localContent"
        class="ed-content"
        placeholder="开始记录…（支持 Markdown）"
        spellcheck="false"
      ></textarea>
      <div
        v-else
        class="ed-content md-preview"
        v-html="previewHtml"
      ></div>

      <!-- 底栏：左标签行 + 右保存状态 -->
      <footer class="ed-footer">
        <div class="tag-row">
          <TagIcon :size="13" :stroke-width="1.8" class="tag-row-icon" />
          <span
            v-for="t in noteTags"
            :key="t.id"
            class="tag-chip"
          >
            {{ t.name }}
            <button
              class="tag-chip-x"
              type="button"
              :title="`移除标签「${t.name}」`"
              :aria-label="`移除标签「${t.name}」`"
              @click="removeTag(t.id)"
            >
              ✕
            </button>
          </span>
          <template v-if="tagInputVisible">
            <input
              v-model="tagInput"
              class="tag-input"
              type="text"
              maxlength="20"
              placeholder="标签名，回车确认"
              @keydown.enter.prevent="submitTagInput"
              @keydown.esc="tagInputVisible = false"
            />
          </template>
          <button
            v-else
            class="tag-add"
            title="添加标签"
            aria-label="添加标签"
            @click="tagInputVisible = true"
          >
            +
          </button>
        </div>

        <span class="ed-status" :class="{ dirty }">
          {{ dirty ? '编辑中…' : `已保存 ${formatSavedTime(note.updated_at)}` }}
        </span>
      </footer>
    </template>
  </div>
</template>

<style scoped>
.editor-panel {
  height: 100%;
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 16px 24px 12px;
  overflow: hidden;
  /* 速记模块字号：全局基准 × 模块系数 */
  font-size: calc(1rem * var(--fs-notes, 1));
}

.editor-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.875em;
}

.editor-empty svg {
  opacity: 0.5;
}

.ed-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.ed-title-input {
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

.ed-title-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}

.ed-title-input::placeholder {
  color: var(--text-4);
  font-weight: 400;
}

.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

.mode-btn.active {
  background: var(--brand-50);
  color: var(--brand-500);
}

.ed-content {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  resize: none;
  outline: none;
  font-size: 0.875em;
  line-height: 1.7;
  font-family: inherit;
  color: var(--text-2);
  padding: 14px;
  transition: border-color 0.15s, box-shadow 0.15s;
  overflow-y: auto;
}

.ed-content:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}

.ed-content::placeholder {
  color: var(--text-4);
}

/* Markdown 预览 */
.md-preview :deep(h1),
.md-preview :deep(h2),
.md-preview :deep(h3) {
  color: var(--text-1);
  margin: 14px 0 8px;
  line-height: 1.4;
}

.md-preview :deep(h1) {
  font-size: calc(1.25rem * var(--fs-notes, 1));
}

.md-preview :deep(h2) {
  font-size: calc(1.0625rem * var(--fs-notes, 1));
}

.md-preview :deep(h3) {
  font-size: calc(0.9375rem * var(--fs-notes, 1));
}

.md-preview :deep(p) {
  margin: 8px 0;
}

.md-preview :deep(ul),
.md-preview :deep(ol) {
  padding-left: 22px;
  margin: 8px 0;
}

.md-preview :deep(ol) {
  list-style: decimal;
}

.md-preview :deep(ol > li) {
  display: list-item;
}

.md-preview :deep(code) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  font-size: calc(0.75rem * var(--fs-notes, 1));
  font-family: 'FiraCode', Consolas, monospace;
}

.md-preview :deep(pre) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 12px;
  overflow-x: auto;
  margin: 10px 0;
}

.md-preview :deep(pre code) {
  background: transparent;
  border: none;
  padding: 0;
}

.md-preview :deep(blockquote) {
  border-left: 3px solid var(--brand-500);
  padding-left: 12px;
  color: var(--text-3);
  margin: 8px 0;
}

.md-preview :deep(a) {
  color: var(--brand-500);
}

.md-preview :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-soft);
  margin: 14px 0;
}

.md-preview :deep(table) {
  border-collapse: collapse;
  margin: 10px 0;
}

.md-preview :deep(th),
.md-preview :deep(td) {
  border: 1px solid var(--border-soft);
  padding: 6px 10px;
  font-size: calc(0.8125rem * var(--fs-notes, 1));
}

/* 标签行 */
.tag-row {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.tag-row-icon {
  color: var(--text-4);
  flex-shrink: 0;
}

.tag-chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 0.6875em;
  font-weight: 500;
  color: var(--brand-500);
  background: var(--brand-50);
  border-radius: var(--radius-pill);
  padding: 3px 6px 3px 9px;
}

.tag-chip-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  font-size: calc(0.625rem * var(--fs-notes, 1));
  line-height: 1;
  padding: 0;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}

.tag-chip-x:hover {
  background: color-mix(in srgb, var(--c-red) 14%, transparent);
  color: var(--c-red);
}

.tag-add {
  width: 26px;
  height: 26px;
  border: 1px dashed var(--text-4);
  background: transparent;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 0.8125em;
  cursor: pointer;
  transition: border-color 0.12s, color 0.12s;
}

.tag-add:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}

.tag-input {
  width: 130px;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-sm);
  color: var(--text-1);
  font-size: 0.75em;
  font-family: inherit;
  padding: 4px 10px;
  outline: none;
}

.tag-input:focus {
  border-color: var(--brand-500);
}

.ed-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-top: 6px;
  border-top: 1px solid var(--border-soft);
}

.ed-status {
  flex-shrink: 0;
  font-size: 0.75em;
  color: var(--text-3);
}

.ed-status.dirty {
  color: var(--brand-500);
}
</style>
