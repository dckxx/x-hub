<script setup lang="ts">
// 应用根壳：主窗口渲染完整首页；便签浮窗（sticky-*）与倒计时浮窗（countdown-*）渲染独立小窗
import { getCurrentWindow } from '@tauri-apps/api/window'
import Index from './index/index.vue'
import DetachedStickyWindow from './components/DetachedStickyWindow.vue'
import CountdownFloat from './components/CountdownFloat.vue'
import { isTauri } from './api/tauri'

const label = isTauri() ? getCurrentWindow().label : ''
const isStickyWindow = label.startsWith('sticky-')
const isCountdownFloat = label.startsWith('countdown-')
</script>

<template>
  <CountdownFloat v-if="isCountdownFloat" />
  <DetachedStickyWindow v-else-if="isStickyWindow" />
  <Index v-else />
</template>
