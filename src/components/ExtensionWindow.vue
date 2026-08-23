<script setup lang="ts">
import { computed } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useExtensionFrame } from '../composables/useExtensionFrame'

// 窗口 label 形如 ext-<扩展id>；独立窗口自带系统标题栏，这里只渲染扩展内容
const label = getCurrentWindow().label
const extId = computed(() => (label.startsWith('ext-') ? label.slice('ext-'.length) : label))

const { frameRef, loading, error } = useExtensionFrame(
  () => extId.value,
  () => null,
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef
</script>

<template>
  <div class="extension-window">
    <div v-if="loading" class="ew-state">
      <p>正在加载扩展…</p>
    </div>
    <div v-else-if="error" class="ew-state">
      <p class="ew-error">{{ error }}</p>
    </div>
    <iframe
      v-show="!loading && !error"
      ref="frameRef"
      class="ew-frame"
      title="扩展窗口"
    />
  </div>
</template>

<style scoped>
.extension-window {
  height: 100vh;
  width: 100vw;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg-page);
}
.ew-frame {
  flex: 1;
  width: 100%;
  border: 0;
  background: var(--bg-page);
}
.ew-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 0.8125rem;
}
.ew-error {
  color: var(--c-red);
}
</style>
