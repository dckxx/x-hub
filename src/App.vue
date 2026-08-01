<script setup lang="ts">
import { ref } from 'vue'
import {
  NConfigProvider,
  NLayout,
  NLayoutHeader,
  NLayoutContent,
  NSpace,
  NText,
  NSwitch,
  NMessageProvider,
  darkTheme,
  lightTheme,
} from 'naive-ui'
import Dashboard from './components/Dashboard.vue'

const darkMode = ref(false)
</script>

<template>
  <n-config-provider :theme="darkMode ? darkTheme : lightTheme">
    <n-message-provider>
      <n-layout position="absolute" class="full-layout">
        <n-layout-header bordered class="header">
          <n-space align="center" justify="space-between">
            <n-text strong class="title">Tauri2 + Vite + Vue3 + NaiveUI</n-text>
            <n-space align="center">
              <n-text depth="3">深色模式</n-text>
              <n-switch v-model:value="darkMode" />
            </n-space>
          </n-space>
        </n-layout-header>
        <n-layout-content class="content" content-style="padding: 24px;">
          <Dashboard />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>

<style scoped>
.full-layout {
  height: 100vh;
}
.header {
  padding: 0 24px;
  height: 56px;
  display: flex;
  align-items: center;
}
.title {
  font-size: 18px;
}
.content {
  display: flex;
  justify-content: center;
}
</style>
