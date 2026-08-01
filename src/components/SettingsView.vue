<script setup lang="ts">
import { ref } from 'vue'
import {
  NCard,
  NSpace,
  NRadioGroup,
  NRadioButton,
  NSwitch,
  NText,
  useMessage,
} from 'naive-ui'
import { useStore } from '../stores/workbench'
import { tauriApi, isTauri } from '../api/tauri'

const store = useStore()
const message = useMessage()

const theme = ref<'light' | 'dark'>(store.state.config.theme === 'dark' ? 'dark' : 'light')
const alwaysOnTop = ref(store.state.config.window.always_on_top)

async function onThemeChange(value: 'light' | 'dark') {
  theme.value = value
  await store.setTheme(value)
  message.success(value === 'dark' ? '已切换深色主题' : '已切换浅色主题')
}

async function onAlwaysOnTopChange(value: boolean) {
  alwaysOnTop.value = value
  await store.setAlwaysOnTop(value)
  message.success(value ? '窗口已置顶' : '已取消置顶')
}

async function testGlobalShortcut() {
  if (isTauri()) {
    await tauriApi.toggleWindowVisibility()
  } else {
    message.info('浏览器预览环境：请使用 Ctrl+Shift+Space 测试全局快捷键')
  }
}
</script>

<template>
  <div class="settings-view">
    <h2 class="settings-view__title">系统设置</h2>
    <NSpace vertical :size="16" class="settings-view__cards">
      <NCard title="外观">
        <NSpace align="center" justify="space-between">
          <NText>主题模式</NText>
          <NRadioGroup v-model:value="theme" @update:value="onThemeChange">
            <NRadioButton value="light">亮色</NRadioButton>
            <NRadioButton value="dark">暗色</NRadioButton>
          </NRadioGroup>
        </NSpace>
      </NCard>

      <NCard title="窗口">
        <NSpace vertical :size="16">
          <NSpace align="center" justify="space-between">
            <NText>窗口置顶</NText>
            <NSwitch :value="alwaysOnTop" @update:value="onAlwaysOnTopChange" />
          </NSpace>
          <NSpace align="center" justify="space-between">
            <NText>全局快捷键唤起/隐藏窗口（Ctrl+Shift+Space）</NText>
            <NText depth="3" class="settings-view__shortcut-test" @click="testGlobalShortcut">
              点击测试
            </NText>
          </NSpace>
        </NSpace>
      </NCard>

      <NCard title="数据存储">
        <NText depth="3" class="settings-view__storage">
          业务数据存储于本地 SQLite 数据库，基础配置存储于本地 JSON 文件。数据仅保存在本机，不上传云端。
        </NText>
      </NCard>
    </NSpace>
  </div>
</template>

<style scoped>
.settings-view {
  padding: 20px 24px;
  max-width: 720px;
}
.settings-view__title {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 16px;
}
.settings-view__cards {
  width: 100%;
}
.settings-view__shortcut-test {
  cursor: pointer;
  text-decoration: underline;
}
.settings-view__storage {
  font-size: 13px;
  line-height: 1.7;
}
</style>
