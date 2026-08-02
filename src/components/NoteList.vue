<script setup lang="ts">
import { computed } from 'vue'
import { Plus, X } from 'lucide-vue-next'
import type { Note } from '../api/tauri'

const props = defineProps<{
  notes: readonly Note[]
  activeId: number | null
}>()

const emit = defineEmits<{
  (e: 'select', id: number): void
  (e: 'create'): void
  (e: 'delete', id: number): void
}>()

const sortedNotes = computed(() =>
  [...props.notes].sort(
    (a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime(),
  ),
)

function formatTime(iso: string): string {
  const t = new Date(iso)
  const now = new Date()
  const diffMs = now.getTime() - t.getTime()
  const diffMin = Math.floor(diffMs / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24 && now.getDate() === t.getDate()) return `${diffHour} 小时前`
  const y = t.getFullYear()
  if (y === now.getFullYear()) return `${t.getMonth() + 1}月${t.getDate()}日`
  return `${y}年${t.getMonth() + 1}月${t.getDate()}日`
}

function summary(n: Note): string {
  const text = n.content.replace(/\s+/g, ' ').trim()
  return text || '空白笔记'
}
</script>

<template>
  <section class="card note-list">
    <header class="nl-header">
      <h2 class="nl-title">速记笔记</h2>
      <button class="icon-btn add" title="新建笔记" @click="emit('create')">
        <Plus :size="15" :stroke-width="2.2" />
      </button>
    </header>

    <div v-if="sortedNotes.length > 0" class="nl-body">
      <div
        v-for="n in sortedNotes"
        :key="n.id"
        class="note-item"
        :class="{ active: n.id === activeId }"
        @click="emit('select', n.id)"
      >
        <div class="note-item-main">
          <span class="note-title">{{ n.title }}</span>
          <span class="note-meta">{{ formatTime(n.updated_at) }}</span>
          <span class="note-summary">{{ summary(n) }}</span>
        </div>
        <button
          class="icon-btn del"
          title="删除笔记"
          @click.stop="emit('delete', n.id)"
        >
          <X :size="13" :stroke-width="2" />
        </button>
      </div>
    </div>

    <div v-else class="empty-state">
      <span style="font-size: 28px">📝</span>
      <p>还没有笔记</p>
      <button class="pill-btn" style="padding: 7px 18px; margin-top: 6px" @click="emit('create')">
        新建笔记
      </button>
    </div>
  </section>
</template>

<style scoped>
.note-list {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 16px;
  min-height: 0;
}
.nl-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 4px;
  margin-bottom: 12px;
}
.nl-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: #fff;
}

.nl-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.note-item {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.15s;
}
.note-item:hover {
  background: var(--bg-card-soft);
}
.note-item.active {
  background: var(--brand-50);
}
.note-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 10px;
  bottom: 10px;
  width: 3px;
  border-radius: 2px;
  background: var(--brand-500);
}
.note-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.note-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.note-meta {
  font-size: 11px;
  color: var(--text-3);
}
.note-summary {
  font-size: 12px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.del {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  opacity: 0;
  margin-top: -2px;
}
.note-item:hover .del {
  opacity: 1;
}
.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}
</style>
