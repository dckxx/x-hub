<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { marked } from 'marked'
import { Eye, Pencil, Tag as TagIcon, Trash2, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import { parseTimestamp } from '../utils/time'

const store = useStore()

const props = defineProps<{
  note: Readonly<Note> | null
}>()

const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
  (e: 'close'): void
}>()

const localTitle = ref('')
const localContent = ref('')
const dirty = ref(false)
const syncing = ref(false)
const cardRef = ref<HTMLElement | null>(null)
const titleInputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(computed(() => props.note !== null), cardRef, titleInputRef)

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

watch(
  () => props.note?.id,
  async () => {
    syncLocal()
    mode.value = 'edit'
    // 加载笔记标签
    if (props.note && isTauri()) {
      noteTags.value = await tauriApi.getNoteTags(props.note.id)
    } else {
      noteTags.value = []
    }
  },
)

async function persistTags() {
  if (!props.note) return
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

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.note) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

function syncLocal() {
  syncing.value = true
  localTitle.value = props.note?.title ?? ''
  localContent.value = props.note?.content ?? ''
  dirty.value = false
}

watch(
  () => props.note?.id,
  () => syncLocal(),
)

watch([localTitle, localContent], () => {
  if (syncing.value) {
    syncing.value = false
    return
  }
  if (!props.note) return
  dirty.value = true
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    emit('save', props.note!.id, localTitle.value, localContent.value)
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
  return `${pad(t.getHours())}:${pad(t.getMinutes())}`
}
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="note" class="modal-mask">
        <div
          ref="cardRef"
          class="modal-card editor-card"
          role="dialog"
          aria-label="笔记编辑"
          aria-modal="true"
        >
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
            <button class="icon-btn" title="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" />
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

          <!-- 标签行 -->
          <div class="tag-row">
            <TagIcon :size="13" :stroke-width="1.8" class="tag-row-icon" />
            <span
              v-for="t in noteTags"
              :key="t.id"
              class="tag-chip"
              :title="`移除标签「${t.name}」`"
              @click="removeTag(t.id)"
            >
              {{ t.name }} ✕
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

          <footer class="ed-footer">
            <span class="ed-status" :class="{ dirty }">
              {{ dirty ? '编辑中…' : `已保存 ${formatSavedTime(note.updated_at)}` }}
            </span>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.editor-card {
  width: 600px;
  max-width: calc(100vw - 48px);
  height: 480px;
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
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
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  font-size: 16px;
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
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  resize: none;
  outline: none;
  font-size: 14px;
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
  font-size: 20px;
}
.md-preview :deep(h2) {
  font-size: 17px;
}
.md-preview :deep(h3) {
  font-size: 15px;
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
  font-size: 12px;
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
  font-size: 13px;
}

/* 标签行 */
.tag-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  padding: 10px 0 0;
  min-height: 34px;
}
.tag-row-icon {
  color: var(--text-4);
  flex-shrink: 0;
}
.tag-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  font-weight: 500;
  color: var(--brand-500);
  background: var(--brand-50);
  border-radius: var(--radius-pill);
  padding: 3px 9px;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.tag-chip:hover {
  background: color-mix(in srgb, var(--c-red) 12%, transparent);
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
  font-size: 13px;
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
  background: var(--bg-card-soft);
  border-radius: var(--radius-sm);
  color: var(--text-1);
  font-size: 12px;
  font-family: inherit;
  padding: 4px 10px;
  outline: none;
}
.tag-input:focus {
  border-color: var(--brand-500);
}

.ed-footer {
  padding-top: 10px;
  border-top: 1px solid var(--border-soft);
  display: flex;
  justify-content: flex-end;
}
.ed-status {
  font-size: 12px;
  color: var(--text-3);
}
.ed-status.dirty {
  color: var(--brand-500);
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
