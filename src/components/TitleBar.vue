<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri, tauriApi } from '../api/tauri'

defineEmits<{
  (e: 'search'): void
  (e: 'settings'): void
}>()

const appWindow = isTauri() ? getCurrentWindow() : null

// 无边框窗口拖动：data-tauri-drag-region 只对 mousedown 的精确目标生效，
// 点击标题栏内的子元素（svg/span）时不触发；改用 startDragging 统一处理
function onDragStart(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return
  appWindow.startDragging()
}

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
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
          <circle cx="11" cy="11" r="7" stroke="currentColor" stroke-width="1.8"/>
          <path d="M20 20l-3.5-3.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
        </svg>
      </button>
      <button class="tool-btn" title="设置" @click="$emit('settings')">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.8"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/>
        </svg>
      </button>
      <div class="tool-divider"></div>
      <button class="win-btn minimize" title="最小化" @click="minimize">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <path d="M17 21h12" stroke="var(--text-2)" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
      <button class="win-btn maximize" title="最大化/还原" @click="toggleMaximize">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <rect x="17" y="14" width="12" height="10" rx="1" stroke="var(--text-2)" stroke-width="1.5"/>
        </svg>
      </button>
      <button class="win-btn close" title="关闭（最小化至托盘）" @click="close">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <path d="M18 15l10 10M28 15L18 25" stroke="var(--text-2)" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
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
