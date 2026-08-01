<script setup lang="ts">
import { NButton, NSpace } from 'naive-ui'
import { tauriApi, isTauri } from '../api/tauri'

const emit = defineEmits<{
  (e: 'toggle-search'): void
}>()

function minimize() {
  if (isTauri()) tauriApi.minimizeWindow()
}

function toggleMaximize() {
  if (isTauri()) tauriApi.toggleMaximize()
}

function close() {
  if (isTauri()) tauriApi.hideToTray()
}

function onSearch() {
  emit('toggle-search')
}
</script>

<template>
  <div class="title-bar" data-tauri-drag-region>
    <div class="title-bar__left">
      <span class="title-bar__logo">WB</span>
      <span class="title-bar__title">个人效率工作台</span>
    </div>
    <div class="title-bar__actions">
      <NButton text class="title-bar__search-btn" title="全局搜索 (Ctrl+K)" @click="onSearch">
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
        <span class="title-bar__search-hint">Ctrl K</span>
      </NButton>
      <NSpace class="title-bar__win-btns" :size="0">
        <button class="win-btn" title="最小化" @click="minimize">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 12h14"/></svg>
        </button>
        <button class="win-btn" title="最大化/还原" @click="toggleMaximize">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="5" y="5" width="14" height="14" rx="1"/></svg>
        </button>
        <button class="win-btn win-btn--close" title="关闭（最小化至托盘）" @click="close">
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg>
        </button>
      </NSpace>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  -webkit-user-select: none;
  user-select: none;
  border-bottom: 1px solid rgba(127, 127, 127, 0.15);
}
.title-bar__left {
  display: flex;
  align-items: center;
  gap: 8px;
}
.title-bar__logo {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: linear-gradient(135deg, #18a058 0%, #2080f0 100%);
  color: #fff;
  font-size: 10px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
}
.title-bar__title {
  font-size: 13px;
  font-weight: 500;
}
.title-bar__actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.title-bar__search-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 12px;
}
.title-bar__search-btn:hover {
  background-color: rgba(127, 127, 127, 0.12);
}
.title-bar__search-hint {
  font-size: 11px;
  opacity: 0.6;
}
.title-bar__win-btns {
  display: flex;
}
.win-btn {
  width: 42px;
  height: 32px;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.8;
}
.win-btn:hover {
  background-color: rgba(127, 127, 127, 0.15);
}
.win-btn--close:hover {
  background-color: #e81123;
  color: #fff;
  opacity: 1;
}
</style>
