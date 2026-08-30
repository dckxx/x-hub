<script setup lang="ts">
import { computed, inject } from 'vue'
import { useExtensionFrame } from '../composables/useExtensionFrame'

const props = defineProps<{
  extId: string
  surface?: string | null
  /** 强制重载计数：宿主每次「打开该扩展」都递增，点击同一个已打开的扩展也触发 iframe 重新导航 */
  reloadKey?: number
  onOpenSurface?: (surface: string) => void
}>()

// module 形态 = 工作台卡片，套用宿主 .card 玻璃外观；view 形态 = 整页，容器透明透出宿主页面渐变
const isModule = computed(() => props.surface === 'module')

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
  props.onOpenSurface,
  () => props.reloadKey ?? 0,
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef
</script>

<template>
  <div class="extension-view" :class="{ card: isModule }">
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
/* module 卡片：复用宿主 .card 玻璃表面 + 边框 + 圆角 + 阴影，与工作台其他卡片严格统一 */
.extension-view.card {
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--frost-edge), var(--shadow-card);
}
.ev-frame {
  flex: 1;
  width: 100%;
  border: 0;
  background: transparent;
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
