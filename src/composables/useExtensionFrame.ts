import { convertFileSrc } from '@tauri-apps/api/core'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { isTauri, tauriApi } from '../api/tauri'

const ERROR_CODES = [
  'PERMISSION_DENIED',
  'NOT_FOUND',
  'INVALID_ARGUMENT',
  'IO_ERROR',
  'NETWORK_ERROR',
  'INTERNAL',
] as const

/** 解析 Tauri invoke 拒绝字符串为 XHubError 的 code/message（后端约定 CODE: message 前缀） */
export function parseXHubError(err: unknown): { code: string; message: string } {
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

/**
 * 扩展前端框架的核心逻辑：iframe 加载扩展入口（宿主注入 window.xhub）+ postMessage RPC 桥。
 *
 * view / window / drawer / module 四种形态共用：每种形态各开一个 iframe，
 * 经 `read_extension_entry` 拿到注入桥脚本的临时 HTML 后加载，宿主侧把
 * 扩展发来的 `xhub call` 转发到 `xhub_call` 命令并回传结果。
 *
 * @param getExtId  扩展 id（延迟求值，供消息处理器与加载共用）
 * @param getSurface 形态（module/view/window/drawer；null = 用 manifest.kind 默认）
 * @param onError    加载失败回调（用于 toast 提示）
 */
export function useExtensionFrame(
  getExtId: () => string,
  getSurface: () => string | null,
  onError?: (message: string) => void,
) {
  const frameRef = ref<HTMLIFrameElement | null>(null)
  const loading = ref(true)
  const error = ref<string | null>(null)

  /** 处理扩展 iframe 发来的 xhub RPC：转发到宿主 xhub_call，回传结果 */
  function onMessage(e: MessageEvent) {
    const frame = frameRef.value
    // 只处理来自本 iframe 的消息：多实例并存（多个 module 卡片 / drawer）时避免互相串扰
    if (!frame || !frame.contentWindow || e.source !== frame.contentWindow) return
    const m = e.data as
      | { __xhub?: boolean; type?: string; id?: number; namespace?: string; method?: string; args?: unknown }
      | undefined
    if (!m || m.__xhub !== true || m.type !== 'call' || typeof m.id !== 'number') return

    const reply = (payload: Record<string, unknown>) => {
      frame.contentWindow?.postMessage({ __xhub: true, type: 'result', id: m.id, ...payload }, '*')
    }

    tauriApi
      .xhubCall(getExtId(), m.namespace ?? '', m.method ?? '', m.args ?? {})
      .then((data) => reply({ ok: true, data }))
      .catch((err) => {
        const { code, message } = parseXHubError(err)
        reply({ ok: false, error: { code, message } })
      })
  }

  async function load() {
    loading.value = true
    error.value = null
    if (!isTauri()) {
      loading.value = false
      error.value = '扩展视图需要在桌面应用中运行'
      return
    }
    try {
      const htmlPath = await tauriApi.readExtensionEntry(getExtId(), getSurface())
      if (frameRef.value) frameRef.value.src = convertFileSrc(htmlPath)
    } catch (e) {
      const { message } = parseXHubError(e)
      error.value = message
      onError?.(message)
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    window.addEventListener('message', onMessage)
    void load()
  })
  onBeforeUnmount(() => {
    window.removeEventListener('message', onMessage)
  })

  return { frameRef, loading, error }
}
