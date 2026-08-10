<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ArrowRight, Gauge, RefreshCw } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ onOpenDetail?: () => void }>()

const store = useStore()
const detecting = ref(false)

const REFRESH_INTERVAL = 5 * 60 * 1000
let refreshTimer: ReturnType<typeof setInterval> | null = null

const summary = computed(() => store.state.usageSummary)
const listening = computed(() => store.state.usageListening)

// 人类可读格式化：1234567 → 1.2M
function fmt(n: number | null | undefined): string {
  const v = n ?? 0
  if (v >= 1_000_000_000) return (v / 1_000_000_000).toFixed(1) + 'B'
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + 'M'
  if (v >= 1_000) return (v / 1_000).toFixed(1) + 'K'
  return String(v)
}

function fmtCost(c: number | null | undefined): string {
  const v = c ?? 0
  return '$' + (v >= 100 ? v.toFixed(0) : v.toFixed(2))
}

async function onDetect() {
  detecting.value = true
  try {
    await store.refreshUsage()
  } finally {
    detecting.value = false
  }
}

const hasData = computed(() => !!summary.value && summary.value.record_count > 0)

const todayTotal = computed(() => {
  const s = summary.value
  return (s?.today_input ?? 0) + (s?.today_cache_input ?? 0) + (s?.today_output ?? 0)
})

const metrics = computed(() => [
  { label: '非缓存输入', value: fmt(summary.value?.today_input) },
  { label: '缓存输入', value: fmt(summary.value?.today_cache_input) },
  { label: '输出', value: fmt(summary.value?.today_output) },
])

const split = computed(() => {
  const total = todayTotal.value || 1
  const s = summary.value
  return [
    { label: '非缓存', value: (s?.today_input ?? 0) / total },
    { label: '缓存', value: (s?.today_cache_input ?? 0) / total },
    { label: '输出', value: (s?.today_output ?? 0) / total },
  ]
})

onMounted(async () => {
  if (!store.state.usageSummary) await onDetect()
  refreshTimer = setInterval(onDetect, REFRESH_INTERVAL)
})

onUnmounted(() => {
  if (refreshTimer) clearInterval(refreshTimer)
  refreshTimer = null
})
</script>

<template>
  <section class="card token-stats" aria-label="Token 统计">
    <header class="ts-header">
      <h3 class="ts-title">
        <Gauge :size="15" :stroke-width="2" aria-hidden="true" />
        <span>Token 统计</span>
        <span
          class="ts-live-dot"
          :class="{ listening }"
          :title="listening ? '正在监听本机 token 用量' : '未监听（未检测到 opencode 数据）'"
          aria-hidden="true"
        ></span>
      </h3>
      <div class="ts-actions">
        <button
          class="ts-refresh"
          type="button"
          title="刷新用量"
          aria-label="刷新用量"
          :disabled="detecting"
          @click="onDetect"
        >
          <RefreshCw :size="13" :stroke-width="2" aria-hidden="true" :class="{ spinning: detecting }" />
        </button>
        <button class="ts-more" type="button" @click="props.onOpenDetail?.()">
          查看详情
          <ArrowRight :size="13" :stroke-width="2" aria-hidden="true" />
        </button>
      </div>
    </header>

    <template v-if="hasData">
      <p class="ts-date-label">今日用量</p>
      <div class="ts-total">
        <span class="ts-total-value">{{ fmt(todayTotal) }}</span>
        <span class="ts-total-suffix">tokens</span>
        <span class="ts-total-cost">≈ {{ fmtCost(summary?.today_cost) }}</span>
      </div>
      <div class="ts-split" aria-hidden="true">
        <div
          v-for="seg in split"
          :key="seg.label"
          class="ts-split-seg"
          :class="`ts-split-${seg.label}`"
          :style="{ flex: seg.value }"
        ></div>
      </div>
      <div class="ts-metrics">
        <div v-for="m in metrics" :key="m.label" class="ts-metric">
          <span class="ts-metric-value">{{ m.value }}</span>
          <span class="ts-metric-label">{{ m.label }}</span>
        </div>
      </div>
    </template>

    <div v-else class="ts-empty">
      <p class="ts-empty-title">未检测到 AI 用量</p>
      <p class="ts-empty-sub">读取本机 opencode 数据库统计 token 用量</p>
      <button class="ghost-btn" type="button" :disabled="detecting" @click="onDetect">
        {{ detecting ? '检测中…' : '重新检测' }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.token-stats {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px;
  min-height: 0;
}
.ts-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}
.ts-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.ts-live-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-pill);
  background: var(--c-gray);
  transition: background 0.2s;
  flex-shrink: 0;
}
.ts-live-dot.listening {
  background: var(--c-green);
  animation: ts-blink 1.4s ease-in-out infinite;
}
@keyframes ts-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.25; }
}
.ts-actions {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.ts-refresh {
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
  transition: background .18s, color .18s, transform .18s;
}
.ts-refresh:hover:not(:disabled) {
  background: var(--bg-card-soft);
  color: var(--brand-500);
  transform: translateY(-1px);
}
.ts-refresh:active:not(:disabled) {
  transform: scale(.94);
}
.ts-refresh:disabled {
  cursor: default;
  opacity: .6;
}
.ts-refresh .spinning {
  animation: ts-spin 1s linear infinite;
}
@keyframes ts-spin {
  to { transform: rotate(360deg); }
}
.ts-more {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  transition: color 0.18s;
}
.ts-more:hover {
  color: var(--brand-500);
}
.ts-date-label {
  margin: 0 0 8px;
  font-size: 11px;
  color: var(--text-4);
}
.ts-total {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-bottom: 8px;
}
.ts-total-value {
  font-size: 32px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.03em;
  color: var(--text-1);
  line-height: 1;
}
.ts-total-suffix {
  font-size: 12px;
  color: var(--text-4);
}
.ts-total-cost {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-3);
}
.ts-split {
  display: flex;
  gap: 3px;
  height: 6px;
  margin-bottom: 14px;
  border-radius: var(--radius-pill);
  overflow: hidden;
}
.ts-split-seg {
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  opacity: 0.35;
  transition: flex 0.4s;
}
.ts-split-seg.ts-split-缓存 { opacity: 0.7; }
.ts-split-seg.ts-split-输出 { opacity: 1; }
.ts-metrics {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}
.ts-metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 8px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  min-width: 0;
}
.ts-metric-value {
  font-size: 18px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
}
.ts-metric-label {
  font-size: 11px;
  color: var(--text-3);
  white-space: nowrap;
}
.ts-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.ts-empty-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.ts-empty-sub {
  margin: 0 0 6px;
  font-size: 11px;
  color: var(--text-4);
}
</style>
