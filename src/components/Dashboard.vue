<script setup lang="ts">
import { ref } from 'vue'
import {
  NCard,
  NSpace,
  NButton,
  NTag,
  NInput,
  NDataTable,
  useMessage,
} from 'naive-ui'
import { getVersion, getTauriVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'

const message = useMessage()

const inputValue = ref('World')
const appVersion = ref('')
const tauriVersion = ref('')

const columns = [
  { title: '技术栈', key: 'stack' },
  { title: '版本', key: 'version' },
]

const data = [
  { stack: 'Vue', version: '3.x' },
  { stack: 'NaiveUI', version: '2.x' },
  { stack: 'Tauri', version: '2.x' },
]

async function checkVersion() {
  try {
    appVersion.value = await getVersion()
    tauriVersion.value = await getTauriVersion()
    message.success('版本获取成功')
  } catch (e) {
    message.warning('浏览器环境，仅 Tauri 运行时可获取应用版本')
    appVersion.value = 'N/A (browser)'
    tauriVersion.value = 'N/A (browser)'
  }
}

async function callGreet() {
  try {
    const result = await invoke<string>('greet', { name: inputValue.value || 'World' })
    message.success(result)
  } catch (e) {
    message.warning('Rust 命令仅 Tauri 运行时可调用')
  }
}
</script>

<template>
  <n-space vertical :size="20">
    <n-card title="脚手架信息">
      <n-space vertical :size="12">
        <n-tag type="info" size="large" round>
          Vue 3 + Vite + TypeScript + NaiveUI + Tauri 2
        </n-tag>
        <n-space>
          <n-button type="primary" @click="checkVersion">
            获取版本
          </n-button>
          <n-button type="primary" @click="callGreet">
            调用 Rust 命令
          </n-button>
        </n-space>
        <n-space v-if="appVersion || tauriVersion">
          <n-tag type="success">App: {{ appVersion || '未知' }}</n-tag>
          <n-tag type="success">Tauri: {{ tauriVersion || '未知' }}</n-tag>
        </n-space>
      </n-space>
    </n-card>
    <n-card title="示例表单">
      <n-input
        v-model:value="inputValue"
        placeholder="输入名称，用于调用 greet 命令"
        clearable
      />
    </n-card>
    <n-card title="组件示例表格">
      <n-data-table :columns="columns" :data="data" :bordered="true" />
    </n-card>
  </n-space>
</template>
