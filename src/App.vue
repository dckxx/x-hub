<script setup lang="ts">
// 应用根壳：主窗口渲染完整首页；便签浮窗窗口（label 以 sticky- 开头）渲染独立小窗
import { getCurrentWindow } from '@tauri-apps/api/window'
import Index from './index/index.vue'
import DetachedStickyWindow from './components/DetachedStickyWindow.vue'
import { isTauri } from './api/tauri'

const isStickyWindow =
  isTauri() && getCurrentWindow().label.startsWith('sticky-')
</script>

<template>
  <DetachedStickyWindow v-if="isStickyWindow" />
  <Index v-else />
</template>
