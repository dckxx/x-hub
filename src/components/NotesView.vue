<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NInput,
  NList,
  NListItem,
  NEmpty,
  NIcon,
  NText,
  NDropdown,
  useMessage,
} from 'naive-ui'
import { AddOutline, EllipsisVerticalOutline } from '@vicons/ionicons5'
import { useStore } from '../stores/workbench'
import type { Note } from '../api/tauri'

const store = useStore()
const message = useMessage()

const selectedId = ref<number | null>(null)
const title = ref('')
const content = ref('')
const saving = ref(false)

const notes = computed(() => store.state.notes)

function formatTime(iso: string) {
  return iso.slice(0, 16).replace('T', ' ')
}

function summary(text: string) {
  const plain = text.replace(/\s+/g, ' ').trim()
  return plain.length > 50 ? plain.slice(0, 50) + '…' : plain
}

async function createNote() {
  const n = await store.addNote('新笔记')
  selectedId.value = n.id
  title.value = n.title
  content.value = n.content
}

function selectNote(note: Note) {
  selectedId.value = note.id
  title.value = note.title
  content.value = note.content
}

let saveTimer: ReturnType<typeof setTimeout> | null = null
watch([title, content], () => {
  if (selectedId.value === null) return
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(async () => {
    saving.value = true
    const noteId = selectedId.value
    try {
      await store.saveNote(noteId!, title.value, content.value)
    } catch (e: unknown) {
      message.error(String(e) || '保存失败')
    } finally {
      saving.value = false
    }
  }, 600)
})

async function removeNote() {
  if (selectedId.value === null) return
  await store.removeNote(selectedId.value)
  selectedId.value = null
  title.value = ''
  content.value = ''
  message.success('笔记已删除')
}
</script>

<template>
  <div class="notes-view">
    <div class="notes-view__list">
      <div class="notes-view__list-header">
        <span class="notes-view__list-title">笔记</span>
        <NButton type="primary" size="small" @click="createNote">
          <template #icon><NIcon :component="AddOutline" /></template>
          新建
        </NButton>
      </div>
      <NList v-if="notes.length" class="notes-view__items">
        <NListItem
          v-for="note in notes"
          :key="note.id"
          :class="{ 'notes-view__item--active': note.id === selectedId }"
          @click="selectNote(note)"
        >
          <div class="notes-view__item">
            <div class="notes-view__item-top">
              <NText strong class="notes-view__item-title">
                {{ note.title || '无标题' }}
              </NText>
              <NDropdown
                :options="[{ label: '删除', key: 'delete' }]"
                trigger="click"
                @select="removeNote"
              >
                <NButton quaternary circle size="tiny" class="notes-view__item-more">
                  <NIcon :component="EllipsisVerticalOutline" />
                </NButton>
              </NDropdown>
            </div>
            <NText depth="3" class="notes-view__item-summary">
              {{ summary(note.content) || '空笔记' }}
            </NText>
            <NText depth="3" class="notes-view__item-time">
              {{ formatTime(note.updated_at) }}
            </NText>
          </div>
        </NListItem>
      </NList>
      <NEmpty v-else description="暂无笔记" size="small" />
    </div>

    <div class="notes-view__editor">
      <template v-if="selectedId !== null">
        <div class="notes-view__editor-header">
          <NInput v-model:value="title" class="notes-view__editor-title" placeholder="笔记标题" />
          <NText depth="3" class="notes-view__editor-save">
            {{ saving ? '保存中…' : '已保存' }}
          </NText>
        </div>
        <NInput
          v-model:value="content"
          type="textarea"
          class="notes-view__editor-content"
          placeholder="开始记录…"
          :autosize="{ minRows: 12, maxRows: 40 }"
        />
      </template>
      <div v-else class="notes-view__editor-empty">
        <NEmpty description="选择或新建一篇笔记" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.notes-view {
  display: flex;
  height: 100%;
}
.notes-view__list {
  width: 260px;
  min-width: 260px;
  border-right: 1px solid rgba(127, 127, 127, 0.15);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.notes-view__list-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 14px 10px;
}
.notes-view__list-title {
  font-size: 15px;
  font-weight: 600;
}
.notes-view__items {
  flex: 1;
  overflow: auto;
}
.notes-view__item {
  padding: 4px 0;
  cursor: pointer;
}
.notes-view__item--active {
  background-color: rgba(32, 128, 240, 0.1);
}
.notes-view__item-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.notes-view__item-title {
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.notes-view__item-summary {
  display: block;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.notes-view__item-time {
  display: block;
  font-size: 11px;
}
.notes-view__item-more {
  visibility: hidden;
}
.notes-view__item:hover .notes-view__item-more {
  visibility: visible;
}
.notes-view__editor {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  padding: 16px 20px;
  gap: 12px;
}
.notes-view__editor-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.notes-view__editor-title {
  flex: 1;
  font-size: 16px;
}
.notes-view__editor-save {
  font-size: 12px;
  white-space: nowrap;
}
.notes-view__editor-content {
  flex: 1;
}
.notes-view__editor-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>
