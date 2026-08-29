<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { Copy, MessageSquare, Minus, Pin, PinOff, Search, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

defineEmits<{
  (e: 'search'): void
  (e: 'chat'): void
}>()

const alwaysOnTop = computed(() => store.state.config.window.always_on_top)

function toggleAlwaysOnTop() {
  void store.setAlwaysOnTop(!alwaysOnTop.value)
}

const appWindow = isTauri() ? getCurrentWindow() : null

// ---- 窗口拖动：data-tauri-drag-region 只对 mousedown 的精确目标生效，
// 点击标题栏内的子元素（svg/span）时不触发；改用 startDragging 统一处理
function onDragStart(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return
  appWindow.startDragging()
}

// ---- 最大化状态（切换图标：最大化 ⇄ 还原） ----
const isMaximized = ref(false)
let unlistenResize: (() => void) | null = null

async function refreshMaximized() {
  if (!appWindow) return
  isMaximized.value = await appWindow.isMaximized()
}

onMounted(async () => {
  if (!appWindow) return
  await refreshMaximized()
  unlistenResize = await appWindow.onResized(() => refreshMaximized())
})

onBeforeUnmount(() => {
  unlistenResize?.()
})

function minimize() {
  if (isTauri()) tauriApi.minimizeWindow()
}

function toggleMaximize() {
  if (isTauri()) tauriApi.toggleMaximize()
}

function close() {
  if (isTauri()) {
    tauriApi.hideToTray()
    // 首次关闭时提示已最小化到托盘，避免用户误以为应用退出了
    if (!localStorage.getItem('tray-hint-shown')) {
      localStorage.setItem('tray-hint-shown', '1')
      showToast('已最小化到系统托盘，右键托盘图标可退出')
    }
  }
}
</script>

<template>
  <div class="title-bar" @mousedown="onDragStart">
    <div class="window-title">
      <span class="title-text">X-Hub</span>
    </div>
    <div class="window-controls">
      <button class="tool-btn" title="全局搜索 (Ctrl+K)" @click="$emit('search')">
        <Search :size="15" :stroke-width="1.8" />
      </button>
      <button class="tool-btn" title="AI 对话 (Ctrl+Shift+K)" @click="$emit('chat')">
        <MessageSquare :size="15" :stroke-width="1.8" />
      </button>
      <div class="tool-divider"></div>
      <button
        class="win-btn top-btn"
        :class="{ active: alwaysOnTop }"
        :title="alwaysOnTop ? '取消窗口置顶' : '窗口置顶'"
        @click="toggleAlwaysOnTop"
      >
        <Pin v-if="!alwaysOnTop" :size="14" :stroke-width="1.8" />
        <PinOff v-else :size="14" :stroke-width="1.8" />
      </button>
      <button class="win-btn minimize" title="最小化" @click="minimize">
        <Minus :size="15" :stroke-width="1.8" />
      </button>
      <button
        class="win-btn maximize"
        :title="isMaximized ? '还原' : '最大化'"
        @click="toggleMaximize"
      >
        <Square v-if="!isMaximized" :size="14" :stroke-width="1.8" />
        <Copy v-else :size="14" :stroke-width="1.8" />
      </button>
      <button class="win-btn close" title="关闭（最小化至托盘）" @click="close">
        <X :size="15" :stroke-width="1.8" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 48px;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 18px;
  flex-shrink: 0;
}
/* 铬件（标题栏）始终全透明：与背景（渐变/壁纸）构成同一个连续平面，表面只属于卡片（ADR 0003） */
.window-title {
  display: flex;
  align-items: center;
  gap: 10px;
}
.title-text {
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  letter-spacing: -0.2px;
}
.window-controls {
  display: flex;
  align-items: center;
  height: 100%;
}
.tool-btn {
  width: 38px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.tool-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
[data-theme="dark"] .tool-btn:hover {
  background: var(--brand-50);
  color: var(--text-1);
}
.tool-divider {
  width: 1px;
  height: 18px;
  background: var(--border-soft);
  margin: 0 4px;
}
.win-btn {
  width: 46px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.win-btn:hover {
  background: var(--bg-card-soft);
}
.win-btn.top-btn.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.top-btn.active:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
</style>
