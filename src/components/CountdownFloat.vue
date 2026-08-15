<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { Pause, Play, X } from 'lucide-vue-next'
import { isTauri, type Countdown } from '../api/tauri'
import { useStore } from '../stores/workbench'

// 从窗口 label 解析倒计时 id（countdown-{id}）
const label = isTauri() ? getCurrentWindow().label : 'countdown-0'
const id = Number(label.replace(/^countdown-/, '')) || 0

const store = useStore()

// 标记为浮窗窗口：body 透明，只显示圆形水罐本体
document.documentElement.dataset.countdownFloat = ''

const mine = computed(() => store.state.countdowns.find((c) => c.id === id) ?? null)

// 每秒刷新剩余时间
const tick = ref(0)
let timer: ReturnType<typeof setInterval> | null = null

onMounted(async () => {
  await store.loadInitialData()
  if (isTauri()) {
    unlistenFired = await listen<Countdown>('countdown-fired', () => {
      store.refreshCountdowns()
    })
    unlistenChanged = await listen('countdowns-changed', () => {
      store.refreshCountdowns()
    })
  }
  timer = setInterval(() => {
    tick.value++
  }, 1000)
})

let unlistenFired: (() => void) | null = null
let unlistenChanged: (() => void) | null = null

onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  unlistenFired?.()
  unlistenChanged?.()
})

// 主题跟随
const theme = computed(() => store.state.config.theme)
watch(
  theme,
  (t) => {
    document.documentElement.dataset.theme = t === 'dark' ? 'dark' : ''
  },
  { immediate: true },
)

function fmt(n: number): string {
  return String(n).padStart(2, '0')
}

function remainingMs(c: Countdown): number {
  if (c.paused) return c.paused_remaining_ms ?? 0
  return Math.max(c.end_at - Date.now(), 0)
}

const remainingLabel = computed(() => {
  void tick.value
  const c = mine.value
  if (!c) return '00:00'
  const ms = remainingMs(c)
  const totalSec = Math.floor(ms / 1000)
  const h = Math.floor(totalSec / 3600)
  const m = Math.floor((totalSec % 3600) / 60)
  const s = totalSec % 60
  if (h > 0) return `${h}:${fmt(m)}:${fmt(s)}`
  return `${m}:${fmt(s)}`
})

/** 水位比例 0~1：剩余/周期 */
const waterRatio = computed(() => {
  void tick.value
  const c = mine.value
  if (!c) return 0
  if (c.paused) {
    const total = Math.max(c.paused_remaining_ms ?? 0, c.total_ms, 1)
    return Math.min(Math.max((c.paused_remaining_ms ?? 0) / total, 0), 1)
  }
  if (c.total_ms <= 0) return 0
  return Math.min(Math.max((c.end_at - Date.now()) / c.total_ms, 0), 1)
})

const finished = computed(() => mine.value?.finished ?? false)
const paused = computed(() => mine.value?.paused ?? false)
const name = computed(() => mine.value?.name ?? '倒计时')

async function togglePause() {
  await store.toggleCountdownPause(id)
}

async function onClose() {
  await store.unfloatCountdown(id)
}

// 窗口拖动：按下后指针发生实际位移才启动拖动，避免单纯点击误触发
// Windows 模态拖动循环（鼠标松开事件被 WebView 吞掉）导致整个应用卡死
const appWindow = isTauri() ? getCurrentWindow() : null
const DRAG_THRESHOLD = 4
let dragPending: { x: number; y: number } | null = null

function onMouseDown(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return
  dragPending = { x: e.screenX, y: e.screenY }
}

function onMouseMove(e: MouseEvent) {
  if (!dragPending || !appWindow) return
  const dx = e.screenX - dragPending.x
  const dy = e.screenY - dragPending.y
  if (dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD) {
    dragPending = null
    void appWindow.startDragging()
  }
}

function onDragEnd() {
  dragPending = null
}
</script>

<template>
  <div class="cf-root" @mousedown="onMouseDown" @mousemove="onMouseMove" @mouseup="onDragEnd" @mouseleave="onDragEnd">
    <div class="cf-water-tank" :class="{ empty: finished }">
      <!-- 圆形水位：水面随剩余比例下降，双层正弦波滚动 -->
      <div
        class="cf-water"
        :style="{ height: `${Math.round(waterRatio * 100)}%` }"
      >
        <svg class="cf-wave cf-wave-a" viewBox="0 0 1200 60" preserveAspectRatio="none" aria-hidden="true">
          <path
            d="M0,32 C150,4 300,60 450,32 C600,4 750,60 900,32 C1050,4 1200,60 1350,32 L1350,60 L0,60 Z"
            fill="rgba(255,255,255,0.28)"
          />
        </svg>
        <svg class="cf-wave cf-wave-b" viewBox="0 0 1200 60" preserveAspectRatio="none" aria-hidden="true">
          <path
            d="M0,40 C150,12 300,68 450,40 C600,12 750,68 900,40 C1050,12 1200,68 1350,40 L1350,60 L0,60 Z"
            fill="rgba(255,255,255,0.18)"
          />
        </svg>
      </div>

      <!-- 中心内容：名称 + 剩余时间 -->
      <div class="cf-center">
        <span class="cf-name">{{ name }}</span>
        <span class="cf-time" :class="{ paused: paused }">{{ remainingLabel }}</span>
        <span v-if="paused" class="cf-state">已暂停</span>
        <span v-else-if="finished" class="cf-state">已结束</span>
      </div>

      <!-- 悬停才显示操作（居中；样式与便签浮窗一致） -->
      <div class="cf-controls">
        <button
          v-if="!finished"
          class="cf-btn"
          :class="{ active: paused }"
          :title="paused ? '恢复' : '暂停'"
          type="button"
          @click.stop="togglePause"
        >
          <Play v-if="paused" :size="13" :stroke-width="2" />
          <Pause v-else :size="13" :stroke-width="2" />
        </button>
        <button class="cf-btn cf-close" title="收起浮窗" type="button" @click.stop="onClose">
          <X :size="13" :stroke-width="2" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.cf-root {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 8px;
  box-sizing: border-box;
  -webkit-app-region: no-drag;
}

.cf-water-tank {
  position: relative;
  width: 96px;
  height: 96px;
  border-radius: 50%;
  /* 半透明毛玻璃感：使用更透明的背景令牌 */
  background: var(--bg-card-soft);
  border: 2px solid var(--border-soft);
  box-shadow: var(--shadow-dock);
  overflow: hidden;
  cursor: move;
  user-select: none;
}

/* 水体：从底部向上填充，高度=剩余占比 */
.cf-water {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: linear-gradient(180deg, var(--brand-400, #8b8bff), var(--brand-600));
  transition: height 0.8s ease-out;
}

/* 双层正弦波：水面滚动 */
.cf-wave {
  position: absolute;
  left: 0;
  right: 0;
  top: -1px;
  height: 24px;
  width: 200%;
  pointer-events: none;
}
.cf-wave-a {
  animation: wave-slide-a 5s linear infinite;
}
.cf-wave-b {
  animation: wave-slide-b 7s linear infinite;
}
@keyframes wave-slide-a {
  0% { transform: translateX(0); }
  100% { transform: translateX(-50%); }
}
@keyframes wave-slide-b {
  0% { transform: translateX(-25%); }
  100% { transform: translateX(-75%); }
}

.cf-center {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  text-align: center;
  padding: 0 8px;
  pointer-events: none;
}
.cf-name {
  font-size: 10px;
  font-weight: 600;
  color: var(--text-1);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cf-time {
  font-size: 15px;
  font-weight: 800;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
  text-shadow: 0 1px 8px rgba(0, 0, 0, 0.06);
}
.cf-time.paused {
  color: var(--text-3);
}
.cf-state {
  font-size: 9px;
  color: var(--text-4);
}

/* 操作按钮：默认隐藏，悬停浮窗时居中显示（底部居中，不遮挡时间） */
.cf-controls {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 8px;
  z-index: 3;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.15s ease-out;
}
.cf-root:hover .cf-controls {
  opacity: 1;
  pointer-events: auto;
}
.cf-root:hover .cf-center {
  transform: translateY(-6px);
}
.cf-center {
  transition: transform 0.15s ease-out;
}

/* 按钮样式与便签浮窗保持一致（缩小适配新尺寸） */
.cf-btn {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: var(--bg-card-solid);
  border-radius: 50%;
  color: var(--text-2);
  box-shadow: var(--shadow-card);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.cf-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.cf-btn.active {
  color: var(--brand-500);
}
.cf-btn.cf-close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
</style>
