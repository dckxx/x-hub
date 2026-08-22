<script setup lang="ts">
import { inject } from 'vue'
import { useExtensionFrame } from '../composables/useExtensionFrame'

const props = defineProps<{
  extId: string
  surface?: string | null
}>()

const emit = defineEmits<{
  close: []
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const { frameRef, loading, error } = useExtensionFrame(
  () => props.extId,
  () => props.surface ?? null,
  (msg) => showToast(`打开扩展失败：${msg}`),
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef
</script>

<template>
  <div class="extension-view">
    <div v-if="loading" class="ev-state">
      <p>正在加载扩展…</p>
    </div>
    <div v-else-if="error" class="ev-state">
      <p class="ev-error">{{ error }}</p>
      <button class="ghost-btn" type="button" @click="emit('close')">返回</button>
    </div>
    <iframe
      v-show="!loading && !error"
      ref="frameRef"
      class="ev-frame"
      title="扩展视图"
    />
  </div>
</template>

<style scoped>
.extension-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.ev-frame {
  flex: 1;
  width: 100%;
  border: 0;
  background: var(--bg-page);
}
.ev-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.8125rem;
}
.ev-error {
  color: var(--c-red);
}
</style>
