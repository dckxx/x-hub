<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { Cpu, MemoryStick } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const store = useStore()

const POLL_INTERVAL = 2000
let timer: ReturnType<typeof setInterval> | null = null

const info = () => store.state.systemInfo

async function poll() {
  await store.refreshSystemInfo()
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
        <Cpu :size="14" :stroke-width="2" aria-hidden="true" />
        <span>系统资源</span>
        <span class="sm-live-dot" aria-hidden="true"></span>
      </h3>
    </header>

    <div class="sm-body">
      <div class="sm-item">
        <div class="sm-item-top">
          <span class="sm-item-name">
            <Cpu :size="12" :stroke-width="2" aria-hidden="true" />
            CPU
          </span>
          <span class="sm-item-value">{{ cpuPct() }}<em>%</em></span>
        </div>
        <div class="sm-bar">
          <div
            class="sm-bar-fill"
            :class="{ warn: cpuPct() >= 85 }"
            :style="{ width: cpuPct() + '%' }"
          ></div>
        </div>
      </div>

      <div class="sm-item">
        <div class="sm-item-top">
          <span class="sm-item-name">
            <MemoryStick :size="12" :stroke-width="2" aria-hidden="true" />
            内存
          </span>
          <span class="sm-item-value">{{ memPct() }}<em>%</em></span>
        </div>
        <div class="sm-bar">
          <div
            class="sm-bar-fill"
            :class="{ warn: memPct() >= 85 }"
            :style="{ width: memPct() + '%' }"
          ></div>
        </div>
        <p class="sm-mem-label">{{ memLabel() }}</p>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 紧凑版：约 110px 总高，无趋势图 */
.sys-monitor {
  display: flex;
  flex-direction: column;
  padding: 12px;
}
.sm-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.sm-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.sm-live-dot {
  width: 6px;
  height: 6px;
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
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.sm-item {
  display: flex;
  flex-direction: column;
  gap: 5px;
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
  font-size: 15px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
  line-height: 1;
}
.sm-item-value em {
  font-style: normal;
  font-size: 10px;
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
  height: 100%;
  min-width: 0;
  border-radius: var(--radius-pill);
  background: linear-gradient(90deg, var(--brand-600), var(--brand-500));
  transition: width 0.5s ease-out;
}
.sm-bar-fill.warn {
  background: linear-gradient(90deg, var(--c-orange), var(--c-red));
}
.sm-mem-label {
  margin: 0;
  font-size: 10px;
  line-height: 1.2;
  color: var(--text-3);
}
</style>
