<script setup lang="ts">
import { isTauri, tauriApi } from '../api/tauri'

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
  <div class="title-bar" data-tauri-drag-region>
    <div class="window-title">
      <svg width="18" height="18" viewBox="0 0 32 32" fill="none">
        <rect width="32" height="32" rx="6" fill="#4F46E5"/>
        <path d="M8 16h16M16 8v16" stroke="white" stroke-width="3" stroke-linecap="round"/>
      </svg>
      <span class="title-text">x-hub</span>
    </div>
    <div class="window-controls">
      <button class="win-btn minimize" title="最小化" @click="minimize">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <path d="M17 21h12" stroke="#4B5563" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
      <button class="win-btn maximize" title="最大化/还原" @click="toggleMaximize">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <rect x="17" y="14" width="12" height="10" rx="1" stroke="#4B5563" stroke-width="1.5"/>
        </svg>
      </button>
      <button class="win-btn close" title="关闭（最小化至托盘）" @click="close">
        <svg width="46" height="40" viewBox="0 0 46 40" fill="none">
          <path d="M18 15l10 10M28 15L18 25" stroke="#4B5563" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 40px;
  background: var(--titlebar);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 16px;
  flex-shrink: 0;
  border-radius: 8px 8px 0 0;
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
}
.window-title {
  display: flex;
  align-items: center;
  gap: 10px;
}
.title-text {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.2px;
}
.window-controls {
  display: flex;
  align-items: center;
  height: 100%;
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
.win-btn:hover { background: rgba(0, 0, 0, 0.04); }
.win-btn.close:hover { background: #E81123; }
.win-btn.close:hover svg path { stroke: #fff; }
</style>
