<script setup lang="ts">
import { computed } from 'vue'
import { ArrowRight, FileText } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ onOpenDetail?: () => void }>()

const store = useStore()

const notes = computed(() => store.state.notes)
const tags = computed(() => store.state.tags)

// 最近编辑：按 updated_at 降序取第一条
const latest = computed(() => {
  if (notes.value.length === 0) return null
  return [...notes.value].sort((a, b) => b.updated_at.localeCompare(a.updated_at))[0]
})

function fmtTime(iso: string): string {
  const t = new Date(iso)
  const now = new Date()
  const diffMin = Math.floor((now.getTime() - t.getTime()) / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24 && sameDay(now, t)) return `${diffHour} 小时前`
  if (sameYear(now, t)) return `${t.getMonth() + 1}月${t.getDate()}日`
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日`
}

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}
function sameYear(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear()
}

function summary(title: string, content: string): string {
  const t = title.trim()
  if (t && t !== '无标题笔记') return t
  const text = content.replace(/\s+/g, ' ').trim()
  return text ? (text.length > 14 ? text.slice(0, 14) + '…' : text) : '空白笔记'
}
</script>

<template>
  <section class="card notes-overview" aria-label="速记统计">
    <header class="no-header">
      <h3 class="no-title">
        <FileText :size="14" :stroke-width="2" aria-hidden="true" />
        <span>速记统计</span>
      </h3>
      <button
        class="no-more"
        type="button"
        title="去速记"
        aria-label="去速记"
        @click="props.onOpenDetail?.()"
      >
        <ArrowRight :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <template v-if="notes.length > 0">
      <div class="no-metrics">
        <div class="no-metric">
          <span class="no-metric-value">{{ notes.length }}</span>
          <span class="no-metric-label">笔记</span>
        </div>
        <div class="no-metric">
          <span class="no-metric-value">{{ tags.length }}</span>
          <span class="no-metric-label">标签</span>
        </div>
      </div>
      <div class="no-latest">
        <span class="no-latest-label">最近编辑</span>
        <p class="no-latest-title" :title="latest ? latest.title : ''">
          {{ latest ? summary(latest.title, latest.content) : '' }}
        </p>
        <span v-if="latest" class="no-latest-time">{{ fmtTime(latest.updated_at) }}</span>
      </div>
    </template>

    <div v-else class="no-empty">
      <p class="no-empty-title">还没有速记</p>
      <p class="no-empty-sub">新建速记后会在这里展示统计</p>
    </div>
  </section>
</template>

<style scoped>
.notes-overview {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
}
.no-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.no-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.no-title :deep(svg) {
  color: var(--brand-500);
}
.no-more {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.18s, color 0.18s;
}
.no-more:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}
.no-metrics {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
  margin-bottom: 12px;
}
.no-metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 8px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  min-width: 0;
}
.no-metric-value {
  font-size: 1.375rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
}
.no-metric-label {
  font-size: 0.6875rem;
  color: var(--text-3);
}
.no-latest {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.no-latest-label {
  font-size: 0.6875rem;
  color: var(--text-4);
}
.no-latest-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.no-latest-time {
  font-size: 0.6875rem;
  color: var(--text-4);
}
.no-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.no-empty-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-2);
}
.no-empty-sub {
  margin: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}
</style>
