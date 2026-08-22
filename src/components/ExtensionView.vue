<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core'
import { inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { isTauri, tauriApi } from '../api/tauri'

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

const frameRef = ref<HTMLIFrameElement | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)

/** 解析 Tauri invoke 拒绝字符串为 XHubError 的 code/message（后端约定 CODE: message 前缀） */
const ERROR_CODES = [
  'PERMISSION_DENIED',
  'NOT_FOUND',
  'INVALID_ARGUMENT',
  'IO_ERROR',
  'NETWORK_ERROR',
  'INTERNAL',
] as const

function parseError(err: unknown): { code: string; message: string } {
  const s = String(err)
  const idx = s.indexOf(':')
  if (idx > 0) {
    const code = s.slice(0, idx)
    if ((ERROR_CODES as readonly string[]).includes(code)) {
      return { code, message: s.slice(idx + 1).trim() }
    }
  }
  return { code: 'INTERNAL', message: s }
}

/** 处理扩展 iframe 发来的 xhub RPC：转发到宿主 xhub_call，回传结果 */
function onMessage(e: MessageEvent) {
  const frame = frameRef.value
  // 只处理来自本 iframe 的消息：多实例并存（如多个 module 卡片）时避免互相串扰
  if (!frame || !frame.contentWindow || e.source !== frame.contentWindow) return
  const m = e.data as
    | { __xhub?: boolean; type?: string; id?: number; namespace?: string; method?: string; args?: unknown }
    | undefined
  if (!m || m.__xhub !== true || m.type !== 'call' || typeof m.id !== 'number') return

  const reply = (payload: Record<string, unknown>) => {
    frame.contentWindow?.postMessage({ __xhub: true, type: 'result', id: m.id, ...payload }, '*')
  }

  tauriApi
    .xhubCall(props.extId, m.namespace ?? '', m.method ?? '', m.args ?? {})
    .then((data) => reply({ ok: true, data }))
    .catch((err) => {
      const { code, message } = parseError(err)
      reply({ ok: false, error: { code, message } })
    })
}

onMounted(async () => {
  window.addEventListener('message', onMessage)
  if (!isTauri()) {
    loading.value = false
    error.value = '扩展视图需要在桌面应用中运行'
    return
  }
  try {
    const htmlPath = await tauriApi.readExtensionEntry(props.extId, props.surface ?? null)
    if (frameRef.value) {
      frameRef.value.src = convertFileSrc(htmlPath)
    }
  } catch (e) {
    const { message } = parseError(e)
    error.value = message
    showToast(`打开扩展失败：${message}`)
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('message', onMessage)
})
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
