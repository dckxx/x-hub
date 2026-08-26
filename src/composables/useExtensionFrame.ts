import { convertFileSrc } from '@tauri-apps/api/core'
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { isTauri, tauriApi } from '../api/tauri'
import {
  broadcastExtensionEvent,
  collectThemeTokens,
  registerExtensionFrame,
  routeExtensionCall,
  routeExtensionCallResult,
  unregisterExtensionFrame,
} from './themeTokens'

const ERROR_CODES = [
  'PERMISSION_DENIED',
  'NOT_FOUND',
  'INVALID_ARGUMENT',
  'IO_ERROR',
  'NETWORK_ERROR',
  'INTERNAL',
] as const

/** 扩展 iframe 白屏看门狗超时（毫秒）：入口返回后超过此时长仍无任何桥消息则判失败 */
const EXT_LOAD_TIMEOUT_MS = 8000

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
 * 把扩展入口的绝对文件路径转成 iframe 可加载的 asset 协议 URL。
 *
 * convertFileSrc 全量 encodeURIComponent，会把盘符冒号 → %3A、反斜杠 → %5C，
 * 整条路径被压成单段 URL，入口内的相对资源（Vite 的 module script / css）会被
 * 解析到 host 根路径而 403（白屏）。这里改为：反斜杠统一成 `/` 后逐段编码，
 * 保留 `/` 作路径分隔符，冒号/空格/中文/#/% 等字符仍被正确 percent 编码，
 * Rust 侧 asset 协议 percent_decode 后可还原成原始路径。
 */
function toAssetUrl(filePath: string): string {
  const probe = convertFileSrc('x')
  const base = probe.slice(0, -1) // 去掉末尾占位字符，得到协议前缀（http(s)://asset.localhost/ 或 asset://localhost/）
  const encoded = filePath
    .replace(/\\/g, '/')
    .split('/')
    .map((seg) => encodeURIComponent(seg))
    .join('/')
  return base + encoded
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
  onOpenSurface?: (surface: string) => void,
) {
  const frameRef = ref<HTMLIFrameElement | null>(null)
  const loading = ref(true)
  const error = ref<string | null>(null)

  // 看门狗状态：扩展 iframe 是否已回传任意桥消息（桥脚本运行即算“已就绪”）
  let frameAlive = false
  let watchdogTimer: number | undefined

  /** 处理扩展 iframe 发来的 xhub RPC：转发到宿主 xhub_call，回传结果 */
  function onMessage(e: MessageEvent) {
    const frame = frameRef.value
    // 只处理来自本 iframe 的消息：多实例并存（多个 module 卡片 / drawer）时避免互相串扰
    if (!frame || !frame.contentWindow || e.source !== frame.contentWindow) return
    const m = e.data as
      | {
          __xhub?: boolean
          type?: string
          id?: number
          namespace?: string
          method?: string
          args?: unknown
          surface?: unknown
          event?: string
          payload?: unknown
          from?: string
          targetId?: string
          data?: unknown
          ok?: boolean
          error?: unknown
        }
      | undefined
    if (!m || m.__xhub !== true) return

    // 桥脚本首次回包即证明扩展入口已成功执行（白屏 = 桥脚本根本没跑起来）
    frameAlive = true
    if (watchdogTimer !== undefined) {
      clearTimeout(watchdogTimer)
      watchdogTimer = undefined
    }

    // 扩展 module 请求打开自身某个形态（view/window/drawer）：通用能力，任何扩展 module 均可使用
    if (m.type === 'open') {
      onOpenSurface?.(String(m.surface || 'view'))
      return
    }

    // 扩展自定义事件广播：先经 Rust 校验 events 权限，再转发给其它扩展 iframe
    if (m.type === 'xhub-emit') {
      const event = String(m.event ?? '')
      const payload = m.payload
      tauriApi
        .xhubCall(getExtId(), 'events', 'emit', { event, payload })
        .then(() => broadcastExtensionEvent(getExtId(), event, payload))
        .catch(() => {
          /* 未声明 events 权限或调用失败：静默忽略 */
        })
      return
    }

    // 跨扩展调用：先经 Rust 校验目标扩展的 expose 白名单，再路由到目标 iframe
    if (m.type === 'xhub-call') {
      const frame = frameRef.value
      if (!frame) return
      const requestId = m.id ?? 0
      const targetId = String(m.targetId ?? '')
      const method = String(m.method ?? '')
      const payload = m.payload
      tauriApi
        .xhubCall(getExtId(), 'runtime', 'callExtension', { targetId, method })
        .then(() => routeExtensionCall(frame, requestId, targetId, method, payload))
        .catch(() => {
          frame.contentWindow?.postMessage(
            {
              __xhub: true,
              type: 'xhub-call-result',
              id: requestId,
              ok: false,
              error: { message: `无权调用 ${targetId}.${method}` },
            },
            '*',
          )
        })
      return
    }

    // 目标扩展的调用结果回传给发起方
    if (m.type === 'xhub-call-result') {
      routeExtensionCallResult(m.id ?? 0, m.ok === true, m.data, m.error)
      return
    }

    const reply = (payload: Record<string, unknown>) => {
      frame.contentWindow?.postMessage({ __xhub: true, type: 'result', id: m.id, ...payload }, '*')
    }

    // 主题查询：主题状态在前端 CSS 变量里，宿主直接回包，不走 Rust
    if (m.type === 'call' && m.namespace === 'theme' && m.method === 'get') {
      reply({ ok: true, data: collectThemeTokens() })
      return
    }

    if (m.type !== 'call' || typeof m.id !== 'number') return

    tauriApi
      .xhubCall(getExtId(), m.namespace ?? '', m.method ?? '', m.args ?? {})
      .then((data) => reply({ ok: true, data }))
      .catch((err) => {
        const { code, message } = parseXHubError(err)
        reply({ ok: false, error: { code, message } })
      })
  }

  /** iframe 加载级错误（网络/协议层失败，如 asset 协议 404/403 之外的 ERR_*）→ 落日志并提示 */
  function onFrameError() {
    const detail = `extId=${getExtId()} surface=${getSurface() ?? ''} url=${frameRef.value?.src ?? ''}`
    if (!error.value) {
      error.value = `扩展 ${getExtId()} 加载失败`
      onError?.(error.value)
    }
    void tauriApi.logClientError({ message: '扩展 iframe 加载失败', detail })
  }

  async function load() {
    loading.value = true
    error.value = null
    frameAlive = false
    if (watchdogTimer !== undefined) {
      clearTimeout(watchdogTimer)
      watchdogTimer = undefined
    }
    if (!isTauri()) {
      loading.value = false
      error.value = '扩展视图需要在桌面应用中运行'
      return
    }
    try {
      const htmlPath = await tauriApi.readExtensionEntry(getExtId(), getSurface())
      if (frameRef.value) {
        frameRef.value.src = toAssetUrl(htmlPath)
        // 看门狗：入口 HTML 已返回但 iframe 在超时内没有任何桥消息（桥脚本未运行）
        // → 判定白屏，落日志并给出友好提示，而不是永远停在空白页
        watchdogTimer = window.setTimeout(() => {
          if (frameAlive || error.value) return
          const detail = `extId=${getExtId()} surface=${getSurface() ?? ''} url=${frameRef.value?.src ?? ''}`
          error.value = '扩展加载失败（页面空白），请查看日志定位原因'
          onError?.(error.value)
          void tauriApi.logClientError({ message: '扩展 iframe 白屏', detail })
        }, EXT_LOAD_TIMEOUT_MS)
      }
    } catch (e) {
      const { message } = parseXHubError(e)
      error.value = message
      onError?.(message)
      void tauriApi.logClientError({
        message: '扩展入口加载失败',
        detail: `extId=${getExtId()} surface=${getSurface() ?? ''} | ${message}`,
      })
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    window.addEventListener('message', onMessage)
    registerExtensionFrame(frameRef.value, getExtId())
    if (frameRef.value) frameRef.value.addEventListener('error', onFrameError)
    void load()
  })
  onBeforeUnmount(() => {
    if (watchdogTimer !== undefined) clearTimeout(watchdogTimer)
    if (frameRef.value) frameRef.value.removeEventListener('error', onFrameError)
    unregisterExtensionFrame(frameRef.value)
    window.removeEventListener('message', onMessage)
  })

  return { frameRef, loading, error }
}
