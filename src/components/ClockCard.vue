<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { Quote, Timer } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Countdown } from '../api/tauri'

const store = useStore()

const now = ref(new Date())
// 倒计时环形进度按秒刷新；时钟本体按 30s 刷新即可
const tick = ref(0)
let timer: ReturnType<typeof setInterval> | null = null
let tickTimer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  timer = setInterval(() => {
    now.value = new Date()
  }, 30_000)
  tickTimer = setInterval(() => {
    tick.value++
  }, 1000)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  if (tickTimer) clearInterval(tickTimer)
})

const WEEKDAYS = ['日', '一', '二', '三', '四', '五', '六'] as const

const timeText = computed(() => {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(now.value.getHours())}:${pad(now.value.getMinutes())}`
})

const dateText = computed(() => {
  const d = now.value
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${WEEKDAYS[d.getDay()]}`
})

const QUOTE = computed(
  () => store.state.config.clock_quote?.trim() || '日拱一卒，功不唐捐。',
)

// ---- 最近一个进行中的倒计时（环形进度） ----
const nearest = computed<Countdown | null>(() => {
  const active = store.state.countdowns
    .filter((c) => !c.finished)
    .sort((a, b) => a.end_at - b.end_at)
  return active[0] ?? null
})

// 仅当中上区块为「倒计时」时，时钟卡片才显示最近倒计时环形进度
const showCountdownRing = computed(
  () => store.state.config.dashboard_mid_content === 'countdown',
)

function fmt(n: number): string {
  return String(n).padStart(2, '0')
}

function remainingMs(c: Countdown): number {
  void tick.value
  if (c.paused) return c.paused_remaining_ms ?? 0
  return Math.max(c.end_at - Date.now(), 0)
}

const ringRatio = computed(() => {
  void tick.value
  const c = nearest.value
  if (!c) return 0
  const total = Math.max(c.total_ms, 1)
  if (c.paused) {
    return Math.min(Math.max((c.paused_remaining_ms ?? 0) / total, 0), 1)
  }
  return Math.min(Math.max((c.end_at - Date.now()) / total, 0), 1)
})

const ringLabel = computed(() => {
  const c = nearest.value
  if (!c) return ''
  const ms = remainingMs(c)
  const totalSec = Math.floor(ms / 1000)
  const h = Math.floor(totalSec / 3600)
  const m = Math.floor((totalSec % 3600) / 60)
  const s = totalSec % 60
  if (h > 0) return `${h}:${fmt(m)}:${fmt(s)}`
  return `${m}:${fmt(s)}`
})

// 环形进度（SVG）：周长 2πr，stroke-dashoffset 随剩余比例变化
const RING_R = 30
const RING_C = 2 * Math.PI * RING_R
</script>

<template>
  <section class="card clock-card" aria-label="时钟">
    <div class="clock-main">
      <div class="clock-time">{{ timeText }}</div>
      <div class="clock-date">{{ dateText }}</div>
      <div class="clock-quote">
        <Quote :size="12" :stroke-width="2" aria-hidden="true" />
        <span :title="QUOTE">{{ QUOTE }}</span>
      </div>
    </div>

    <!-- 最近倒计时环形进度 -->
    <div v-if="nearest && showCountdownRing" class="clock-countdown" :title="nearest.name">
      <svg class="cd-ring" viewBox="0 0 72 72" aria-hidden="true">
        <circle class="cd-ring-bg" cx="36" cy="36" :r="RING_R" />
        <circle
          class="cd-ring-fg"
          :class="{ paused: nearest.paused }"
          cx="36"
          cy="36"
          :r="RING_R"
          :stroke-dasharray="RING_C"
          :stroke-dashoffset="RING_C * (1 - ringRatio)"
        />
      </svg>
      <div class="cd-center">
        <Timer :size="11" :stroke-width="2" />
        <span class="cd-label">{{ ringLabel }}</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.clock-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px;
}
.clock-main {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.clock-time {
  font-size: 1.875rem;
  font-weight: 700;
  line-height: 1.1;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
}
.clock-date {
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--text-3);
}
.clock-quote {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  padding-top: 10px;
  border-top: 1px solid var(--border-soft);
  font-size: 0.75rem;
  line-height: 1.5;
  color: var(--text-3);
  max-width: 180px;
}
.clock-quote :deep(svg) {
  flex-shrink: 0;
  color: var(--brand-500);
}
.clock-quote span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 倒计时环形进度 */
.clock-countdown {
  flex-shrink: 0;
  position: relative;
  width: 72px;
  height: 72px;
}
.cd-ring {
  width: 100%;
  height: 100%;
  transform: rotate(-90deg);
}
.cd-ring-bg {
  fill: none;
  stroke: var(--border-soft);
  stroke-width: 5;
}
.cd-ring-fg {
  fill: none;
  stroke: var(--brand-500);
  stroke-width: 5;
  stroke-linecap: round;
  transition: stroke-dashoffset 0.8s ease-out;
}
.cd-ring-fg.paused {
  stroke: var(--text-4);
}
.cd-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  color: var(--brand-500);
  pointer-events: none;
}
.cd-label {
  font-size: 0.6875rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
}
</style>
