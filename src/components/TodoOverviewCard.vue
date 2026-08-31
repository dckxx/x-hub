<script setup lang="ts">
import { computed } from 'vue'
import { ArrowRight, ListTodo } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ onOpenDetail?: () => void }>()

const store = useStore()

// 只统计顶级待办：子待办挂在父条目下展示进度，不进总盘子
const todos = computed(() => store.state.todos.filter((t) => t.parent_id == null))

const total = computed(() => todos.value.length)
const doneCount = computed(() => todos.value.filter((t) => t.done).length)
const pendingCount = computed(() => total.value - doneCount.value)

// 完成率（无待办时按 100% 展示，避免 0/0 的 NaN）
const doneRate = computed(() => {
  if (total.value === 0) return 1
  return doneCount.value / total.value
})

// 今日新增：created_at 是今天
const todayAdded = computed(() => {
  const now = new Date()
  return todos.value.filter((t) => sameDay(new Date(t.created_at), now)).length
})

function sameDay(a: Date, b: Date) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}

const pendingLabel = computed(() => (pendingCount.value > 0 ? `还有 ${pendingCount.value} 项待完成` : '今日待办已清空'))
</script>

<template>
  <section class="card todo-overview" aria-label="待办概览">
    <header class="to-header">
      <h3 class="to-title">
        <ListTodo :size="14" :stroke-width="2" aria-hidden="true" />
        <span>待办概览</span>
      </h3>
      <button
        class="to-more"
        type="button"
        title="去待办"
        aria-label="去待办"
        @click="props.onOpenDetail?.()"
      >
        <ArrowRight :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <template v-if="total > 0">
      <div class="to-rate">
        <div class="to-rate-ring" :style="{ '--rate': doneRate * 360 }" aria-hidden="true">
          <div class="to-rate-inner">
            <span class="to-rate-value">{{ Math.round(doneRate * 100) }}%</span>
            <span class="to-rate-label">完成率</span>
          </div>
        </div>
        <p class="to-rate-note">{{ pendingLabel }}</p>
      </div>
      <div class="to-metrics">
        <div class="to-metric">
          <span class="to-metric-value">{{ total }}</span>
          <span class="to-metric-label">总待办</span>
        </div>
        <div class="to-metric">
          <span class="to-metric-value">{{ todayAdded }}</span>
          <span class="to-metric-label">今日新增</span>
        </div>
        <div class="to-metric">
          <span class="to-metric-value">{{ doneCount }}</span>
          <span class="to-metric-label">已完成</span>
        </div>
      </div>
    </template>

    <div v-else class="to-empty">
      <p class="to-empty-title">还没有待办</p>
      <p class="to-empty-sub">添加待办后会在这里展示概览</p>
    </div>
  </section>
</template>

<style scoped>
.todo-overview {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
}
.to-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.to-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.to-title :deep(svg) {
  color: var(--brand-500);
}
.to-more {
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
.to-more:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}
.to-rate {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 4px 0 10px;
}
.to-rate-ring {
  width: 84px;
  height: 84px;
  border-radius: 50%;
  background: conic-gradient(
    var(--brand-500) var(--rate),
    var(--bg-card-soft) 0
  );
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.4s;
}
.to-rate-inner {
  width: 62px;
  height: 62px;
  border-radius: 50%;
  background: var(--bg-card-solid);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
}
.to-rate-value {
  font-size: 1.125rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
  line-height: 1;
}
.to-rate-label {
  font-size: 0.625rem;
  color: var(--text-4);
}
.to-rate-note {
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
.to-metrics {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}
.to-metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 8px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  min-width: 0;
}
.to-metric-value {
  font-size: 1.125rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
}
.to-metric-label {
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
}
.to-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.to-empty-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-2);
}
.to-empty-sub {
  margin: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}
</style>
