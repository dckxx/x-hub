<script setup lang="ts">
import { ref, watch } from 'vue'
import type { Note } from '../api/tauri'

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

let saveTimer: ReturnType<typeof setTimeout> | null = null

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
  const t = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(t.getHours())}:${pad(t.getMinutes())}`
}
</script>

<template>
  <section class="card editor">
    <template v-if="note">
      <header class="ed-header">
        <input
          v-model="localTitle"
          class="ed-title-input"
          type="text"
          maxlength="80"
          placeholder="笔记标题"
          @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
        />
        <button class="icon-btn del" title="删除笔记" @click="emit('delete', note.id)">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
            <path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14M10 11v6M14 11v6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </button>
      </header>

      <textarea
        v-model="localContent"
        class="ed-content"
        placeholder="开始记录…"
        spellcheck="false"
      ></textarea>

      <footer class="ed-footer">
        <span class="ed-status" :class="{ dirty }">
          {{ dirty ? '编辑中…' : `已保存 ${formatSavedTime(note.updated_at)}` }}
        </span>
      </footer>
    </template>

    <div v-else class="empty-state editor-empty">
      <span style="font-size: 40px">📓</span>
      <p>选择或新建一篇笔记</p>
      <p style="font-size: 12px; color: var(--text-4)">内容将自动保存到本地</p>
    </div>
  </section>
</template>

<style scoped>
.editor {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
  min-height: 0;
}
.ed-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.ed-title-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  font-size: 17px;
  font-weight: 600;
  font-family: inherit;
  color: var(--text-1);
  outline: none;
  padding: 2px 0;
}
.ed-title-input:focus {
  box-shadow: 0 2px 0 var(--brand-500);
}
.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

.ed-content {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: none;
  background: transparent;
  resize: none;
  outline: none;
  font-size: 14px;
  line-height: 1.7;
  font-family: inherit;
  color: var(--text-2);
}
.ed-content::placeholder {
  color: var(--text-4);
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
.editor-empty {
  flex: 1;
  gap: 10px;
}
</style>
