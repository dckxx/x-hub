<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ArrowRight, Gauge } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ onOpenDetail?: () => void }>()

const store = useStore()
const detecting = ref(false)

const summary = computed(() => store.state.usageSummary)
const listening = computed(() => store.state.usageListening)
const daily = computed(() => store.state.usageDetail?.daily ?? [])

// 人类可读格式化：1234567 → 1.2M
function fmt(n: number | null | undefined): string {
  const v = n ?? 0
  if (v >= 1_000_000_000) return (v / 1_000_000_000).toFixed(1) + 'B'
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + 'M'
  if (v >= 1_000) return (v / 1_000).toFixed(1) + 'K'
  return String(v)
}

function dailyHeight(d: { input: number; cache_input: number; output: number }): number {
  const total = d.input + d.cache_input + d.output
  const max = Math.max(1, ...daily.value.map((x) => x.input + x.cache_input + x.output))
  return Math.max(8, Math.round((total / max) * 100))
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

const metrics = computed(() => [
  { label: '非缓存输入', value: fmt(summary.value?.today_input), color: 'var(--brand-500)' },
  { label: '缓存输入', value: fmt(summary.value?.today_cache_input), color: 'var(--c-green)' },
  { label: '输出', value: fmt(summary.value?.today_output), color: 'var(--c-yellow)' },
])

onMounted(async () => {
  if (!store.state.usageSummary) await onDetect()
  await store.loadUsageDetail(7, 7, 0)
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
      <button class="ts-more" type="button" @click="props.onOpenDetail?.()">
        查看详情
        <ArrowRight :size="13" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <template v-if="hasData">
      <p class="ts-date-label">今日用量</p>
      <div class="ts-metrics">
        <div v-for="m in metrics" :key="m.label" class="ts-metric">
          <span class="ts-metric-value" :style="{ color: m.color }">{{ m.value }}</span>
          <span class="ts-metric-label">{{ m.label }}</span>
        </div>
      </div>
      <div v-if="daily.length" class="ts-chart" aria-hidden="true">
        <div v-for="d in daily" :key="d.date" class="ts-chart-col">
          <div class="ts-chart-bar" :style="{ height: dailyHeight(d) + '%' }" :title="`${d.date} ${fmt(d.input + d.cache_input + d.output)}`"></div>
          <span class="ts-chart-date">{{ d.date.slice(5) }}</span>
        </div>
      </div>
      <div class="ts-footer">
        <span class="ts-foot-item">7日 {{ fmt((summary?.seven_day_input ?? 0) + (summary?.seven_day_cache_input ?? 0)) }}</span>
        <span class="ts-foot-item">本月 {{ fmtCost(summary?.month_cost) }}</span>
        <span class="ts-foot-item ts-source">opencode</span>
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
}
.ts-metric-label {
  font-size: 11px;
  color: var(--text-3);
  white-space: nowrap;
}
.ts-footer {
  margin-top: auto;
  padding-top: 10px;
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11px;
  color: var(--text-3);
}
.ts-chart {
  flex: 1;
  min-height: 48px;
  margin-top: 12px;
  display: flex;
  align-items: flex-end;
  gap: 6px;
}
.ts-chart-col {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  min-width: 0;
}
.ts-chart-bar {
  width: 100%;
  max-width: 26px;
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  background: linear-gradient(180deg, var(--brand-500), var(--brand-600));
  opacity: 0.85;
  transition: height 0.3s;
}
.ts-chart-date {
  font-size: 9px;
  color: var(--text-4);
}
.ts-foot-item {
  white-space: nowrap;
}
.ts-source {
  margin-left: auto;
  padding: 1px 8px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-4);
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
