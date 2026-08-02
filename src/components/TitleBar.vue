<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { Copy, Minus, Search, Settings, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'

defineEmits<{
  (e: 'search'): void
  (e: 'settings'): void
}>()

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
  if (isTauri()) tauriApi.hideToTray()
}
</script>

<template>
  <div class="title-bar" @mousedown="onDragStart">
    <div class="window-title">
      <svg width="18" height="18" viewBox="0 0 32 32" fill="none">
        <rect width="32" height="32" rx="6" fill="var(--brand-500)"/>
        <path d="M8 16h16M16 8v16" stroke="white" stroke-width="3" stroke-linecap="round"/>
      </svg>
      <span class="title-text">x-hub</span>
    </div>
    <div class="window-controls">
      <button class="tool-btn" title="全局搜索 (Ctrl+K)" @click="$emit('search')">
        <Search :size="15" :stroke-width="1.8" />
      </button>
      <button class="tool-btn" title="设置" @click="$emit('settings')">
        <Settings :size="15" :stroke-width="1.8" />
      </button>
      <div class="tool-divider"></div>
      <button class="win-btn minimize" title="最小化" @click="minimize">
        <Minus :size="15" :stroke-width="1.8" color="var(--text-2)" />
      </button>
      <button
        class="win-btn maximize"
        :title="isMaximized ? '还原' : '最大化'"
        @click="toggleMaximize"
      >
        <Square v-if="!isMaximized" :size="14" :stroke-width="1.8" color="var(--text-2)" />
        <Copy v-else :size="14" :stroke-width="1.8" color="var(--text-2)" />
      </button>
      <button class="win-btn close" title="关闭（最小化至托盘）" @click="close">
        <X :size="15" :stroke-width="1.8" color="var(--text-2)" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 40px;
  background: var(--bg-card);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 16px;
  flex-shrink: 0;
  border-radius: 8px 8px 0 0;
  border-bottom: 1px solid var(--border-soft);
}
.window-title {
  display: flex;
  align-items: center;
  gap: 10px;
}
.title-text {
  font-size: 14px;
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
  background: rgba(0, 0, 0, 0.04);
  color: var(--text-1);
}
[data-theme="dark"] .tool-btn:hover {
  background: rgba(255, 255, 255, 0.08);
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
  cursor: pointer;
  transition: background 0.15s;
}
.win-btn:hover {
  background: rgba(0, 0, 0, 0.04);
}
.win-btn.close:hover {
  background: #E81123;
}
.win-btn.close:hover svg path {
  stroke: #fff;
}
</style>
