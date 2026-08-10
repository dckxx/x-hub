<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { Cpu, MemoryStick } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const store = useStore()

const POLL_INTERVAL = 2000
const TREND_POINTS = 30
let timer: ReturnType<typeof setInterval> | null = null

const cpuTrend = ref<number[]>([])
const memTrend = ref<number[]>([])

const info = () => store.state.systemInfo

function pushTrend(arr: number[], v: number, cap: number) {
  arr.push(v)
  if (arr.length > cap) arr.shift()
}

async function poll() {
  const r = await store.refreshSystemInfo()
  if (!r) return
  pushTrend(cpuTrend.value, r.cpuUsage, TREND_POINTS)
  pushTrend(memTrend.value, r.memPercent, TREND_POINTS)
}

function pathOf(data: number[]): string {
  const n = data.length
  if (n < 2) return ''
  const w = 100
  const h = 100
  const max = Math.max(100, ...data)
  return data
    .map((v, i) => `${i === 0 ? 'M' : 'L'}${(i / (n - 1)) * w},${h - (v / max) * h}`)
    .join(' ')
}

const cpuPct = () => Math.round(info()?.cpuUsage ?? 0)
const memPct = () => Math.round(info()?.memPercent ?? 0)
const memLabel = () => {
  const i = info()
  if (!i) return '—'
  return `${(i.memUsedMb / 1024).toFixed(1)} / ${(i.memTotalMb / 1024).toFixed(1)} GB`
}

onMounted(async () => {
  await poll()
  timer = setInterval(poll, POLL_INTERVAL)
})

onUnmounted(() => {
  if (timer) clearInterval(timer)
  timer = null
})
</script>

<template>
  <section class="card sys-monitor" aria-label="系统资源">
    <header class="sm-header">
      <h3 class="sm-title">
        <Cpu :size="15" :stroke-width="2" aria-hidden="true" />
        <span>系统资源</span>
        <span class="sm-live-dot" aria-hidden="true"></span>
      </h3>
    </header>

    <div class="sm-body">
      <div class="sm-item">
        <div class="sm-item-top">
          <span class="sm-item-name">
            <Cpu :size="13" :stroke-width="2" aria-hidden="true" />
            CPU
          </span>
          <span class="sm-item-value">{{ cpuPct() }}<em>%</em></span>
        </div>
        <div class="sm-bar">
          <div class="sm-bar-fill" :style="{ transform: 'scaleX(' + cpuPct() / 100 + ')' }"></div>
        </div>
        <svg class="sm-trend" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          <path v-if="cpuTrend.length >= 2" class="sm-trend-line" :d="pathOf(cpuTrend)" />
        </svg>
      </div>

      <div class="sm-item">
        <div class="sm-item-top">
          <span class="sm-item-name">
            <MemoryStick :size="13" :stroke-width="2" aria-hidden="true" />
            内存
          </span>
          <span class="sm-item-value">{{ memPct() }}<em>%</em></span>
        </div>
        <div class="sm-bar">
          <div class="sm-bar-fill" :style="{ transform: 'scaleX(' + memPct() / 100 + ')' }"></div>
        </div>
        <p class="sm-mem-label">{{ memLabel() }}</p>
        <svg class="sm-trend" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          <path v-if="memTrend.length >= 2" class="sm-trend-line" :d="pathOf(memTrend)" />
        </svg>
      </div>
    </div>
  </section>
</template>

<style scoped>
.sys-monitor {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px;
  min-height: 0;
}
.sm-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}
.sm-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.sm-live-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-pill);
  background: var(--c-green);
  animation: sm-blink 1.6s ease-in-out infinite;
  flex-shrink: 0;
}
@keyframes sm-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}
.sm-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 18px;
}
.sm-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sm-item-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}
.sm-item-name {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-2);
}
.sm-item-value {
  font-size: 20px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
  line-height: 1;
}
.sm-item-value em {
  font-style: normal;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-3);
  margin-left: 2px;
}
.sm-bar {
  height: 8px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.sm-bar-fill {
  width: 100%;
  height: 100%;
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  transform-origin: left center;
  transition: transform 0.5s ease-out;
}
.sm-trend {
  width: 100%;
  height: 26px;
  display: block;
}
.sm-trend-line {
  fill: none;
  stroke: var(--c-green);
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}
.sm-mem-label {
  margin: 0;
  font-size: 11px;
  color: var(--text-3);
}
</style>
