import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
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

/**
 * 恢复可见时距上次隐藏超过该时长才重载 iframe（毫秒）。
 * 窗口长时间不可见（隐藏到托盘/最小化/完全遮挡）后，WebView2 会挂起或丢弃跨源
 * iframe（asset.localhost，独立渲染进程）的渲染状态，恢复前台后扩展区表现为空白；
 * 重新导航是唯一可靠恢复手段。短时切换（alt-tab 几十秒内回来）不重载，
 * 避免丢扩展内未提交的输入状态。
 */
const RESUME_RELOAD_AFTER_MS = 60_000

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
 * 错误信息是否为「service 后端未授权」类：宿主在 read_extension_entry 就拦截未信任的
 * service 扩展（PERMISSION_DENIED: 本地后端尚未授权…），扩展页面根本不会加载，
 * 因此「去授权」入口必须挂在宿主自己的错误态上，而不是扩展页面里。
 */
export function isUnauthorizedMessage(msg: string | null | undefined): boolean {
  return /PERMISSION_DENIED|尚未授权|未获信任/.test(msg ?? '')
}

/**
 * 请求宿主打开某扩展的设置/授权弹窗：广播全局事件（与桥 API runtime.openPermissions
 * 同一条通道），由主窗 index.vue 接手——切到扩展中心并打开该扩展的设置弹窗。
 * 从扩展独立窗发起同样成立（主窗常驻监听）。
 */
export function requestOpenExtensionSettings(extId: string) {
  if (!isTauri()) return
  void import('@tauri-apps/api/event')
    .then(({ emit }) => emit('open-extension-settings', extId))
    .catch(() => {})
}

/**
 * 扩展入口 URL 由后端 `read_extension_entry` 直接返回（`xhub-ext` 协议，逐段 percent 编码），
 * 前端不再自行拼 asset 协议地址：扩展 origin 因此与承载宿主数据的 `asset.localhost` 不同源，
 * 扩展无法直接读取数据根下的数据库与配置（见 docs/adr/0008-extension-content-origin-isolation.md）。
 */

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
 * @param getReloadKey 强制重载计数（可选）：宿主每次「打开某扩展」递增，点击同一个
 *                     已打开的扩展时 extId/surface 均不变，靠该计数触发重新导航
 * @param getVariant 工作台模块形态 id（可选，module 形态多形态时生效）：入口 URL 追加
 *                   `xhub-variant` query（首帧可读），变化时 postMessage 广播给扩展
 */
export function useExtensionFrame(
  getExtId: () => string,
  getSurface: () => string | null,
  onError?: (message: string) => void,
  onOpenSurface?: (surface: string) => void,
  getReloadKey?: () => number,
  getVariant?: () => string | null,
) {
  const frameRef = ref<HTMLIFrameElement | null>(null)
  const loading = ref(true)
  const error = ref<string | null>(null)
  // 只信任后端返回的入口来源，不随 iframe 自行导航改变。
  let expectedOrigin = ''
  let loadGeneration = 0
  let disposed = false
  let unlistenPermissions: (() => void) | undefined

  /** 把当前形态 id 广播给扩展 iframe（桥脚本写 data-xhub-variant + CSS 变量 + 派发事件） */
  function broadcastVariant() {
    const frame = frameRef.value
    const variant = getVariant?.() ?? null
    if (!frame || !frame.contentWindow || !expectedOrigin) return
    frame.contentWindow.postMessage({ __xhub: true, type: 'variant', variant }, expectedOrigin)
  }

  // 看门狗状态：扩展 iframe 是否已回传任意桥消息（桥脚本运行即算“已就绪”）
  let frameAlive = false
  let watchdogTimer: number | undefined

  // 后台久置恢复：记录隐藏时刻（初始化即隐藏，如 --autostart-hidden 静默驻留托盘的场景
  // 不会有 hidden 迁移事件，也纳入统计）
  let hiddenAt: number | undefined = document.visibilityState === 'hidden' ? Date.now() : undefined

  /** 恢复可见且后台超阈值 → 重载 iframe，把被 WebView2 丢弃/挂起的渲染状态拉回来 */
  function onVisibilityChange() {
    if (document.visibilityState === 'hidden') {
      hiddenAt = Date.now()
      return
    }
    if (hiddenAt === undefined) return
    const elapsed = Date.now() - hiddenAt
    hiddenAt = undefined
    if (elapsed >= RESUME_RELOAD_AFTER_MS) void load()
  }

  /** 处理扩展 iframe 发来的 xhub RPC：转发到宿主 xhub_call，回传结果 */
  function onMessage(e: MessageEvent) {
    const frame = frameRef.value
    // 只处理来自本 iframe 的消息：多实例并存（多个 module 卡片 / drawer）时避免互相串扰
    if (!frame || !frame.contentWindow || e.source !== frame.contentWindow
      || !expectedOrigin || e.origin !== expectedOrigin) return
    const replyOrigin = expectedOrigin
    const m = e.data as
      | {
          __xhub?: boolean
          type?: string
          id?: number
          namespace?: string
          method?: string
          args?: unknown
          surface?: unknown
          url?: unknown
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
    const firstAlive = !frameAlive
    frameAlive = true
    if (firstAlive) {
      // **到这里才撤「加载中」**：拿到入口 URL ≠ 页面画出来了。
      // 早撤会留下「加载态已消失、扩展内容还没渲染」的空白窗口——那正是"点开扩展先空白、
      // 等一下才进主页面"的观感来源（大扩展要跑几百 ms 的 JS 才出首帧）。
      loading.value = false
    }
    if (watchdogTimer !== undefined) {
      clearTimeout(watchdogTimer)
      watchdogTimer = undefined
    }
    // 首条桥消息时补发当前形态：iframe 加载完成前 postMessage 无监听者会被丢弃，
    // URL query 又固化为加载时的旧形态——不补发则扩展停留在旧形态直到下次重载
    if (firstAlive && getVariant) broadcastVariant()

    // 扩展 module 请求打开自身某个形态（view/window/drawer）：通用能力，任何扩展 module 均可使用
    if (m.type === 'open') {
      onOpenSurface?.(String(m.surface || 'view'))
      return
    }

    // 扩展请求用系统默认浏览器打开外链（桥 API `xhub.openExternal`）。
    // 打开动作必须由宿主执行：扩展 iframe 自己开不了新窗口（wry 拒绝 NewWindowRequested，
    // 见 extension.rs 桥脚本里的说明）。只放行 http/https，挡掉 javascript: 等危险协议。
    if (m.type === 'open-external') {
      const url = String(m.url ?? '')
      if (/^https?:\/\//i.test(url) && isTauri()) {
        tauriApi.xhubCall(getExtId(), 'runtime', 'openExternal', { url }).catch((err) => {
          const { code, message } = parseXHubError(err)
          // **必须给可见提示**：以前这里只落日志，于是"点链接没反应"完全查不出原因。
          // 最典型的触发：扩展没在 manifest 里声明 `open-url` 权限。
          onError?.(
            code === 'PERMISSION_DENIED'
              ? '该扩展未声明 open-url 权限，无法打开外链'
              : message,
          )
          void tauriApi.logClientError({ message: '扩展外链打开失败', detail: `extId=${getExtId()} url=${url} | ${message}` })
        })
      }
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
            replyOrigin,
          )
        })
      return
    }

    // 目标扩展的调用结果回传给发起方
    if (m.type === 'xhub-call-result') {
      routeExtensionCallResult(frame, m.id ?? 0, m.ok === true, m.data, m.error)
      return
    }

    const reply = (payload: Record<string, unknown>) => {
      frame.contentWindow?.postMessage({ __xhub: true, type: 'result', id: m.id, ...payload }, replyOrigin)
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
    const generation = ++loadGeneration
    expectedOrigin = ''
    unregisterExtensionFrame(frameRef.value)
    // 先销毁旧页面，权限撤销后不得让隐藏 iframe 继续按旧 CSP 联网。
    if (frameRef.value) frameRef.value.src = 'about:blank'
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
      const entryUrl = await tauriApi.readExtensionEntry(getExtId(), getSurface())
      if (generation !== loadGeneration || disposed) return
      if (frameRef.value) {
        const parsed = new URL(entryUrl)
        expectedOrigin = parsed.origin === 'null' ? `${parsed.protocol}//${parsed.host}` : parsed.origin
        const variant = getVariant?.() ?? null
        // 形态经 URL query 随入口首帧到达（桥脚本运行前即可读 location.search），
        // 后续切换走 postMessage 广播（见下方 watch），两种通道互补
        const url = variant
          ? `${entryUrl}?xhub-variant=${encodeURIComponent(variant)}`
          : entryUrl
        frameRef.value.src = url
        registerExtensionFrame(frameRef.value, getExtId(), expectedOrigin)
        // 看门狗：入口 HTML 已返回但 iframe 在超时内没有任何桥消息（桥脚本未运行）
        // → 判定白屏，落日志并给出友好提示，而不是永远停在空白页
        watchdogTimer = window.setTimeout(() => {
          if (frameAlive || error.value) return
          const detail = `extId=${getExtId()} surface=${getSurface() ?? ''} url=${frameRef.value?.src ?? ''}`
          error.value = '扩展加载失败（页面空白），请查看日志定位原因'
          loading.value = false // 超时也要撤掉加载态，别让界面永远停在「加载中」
          onError?.(error.value)
          void tauriApi.logClientError({ message: '扩展 iframe 白屏', detail })
        }, EXT_LOAD_TIMEOUT_MS)
      }
    } catch (e) {
      if (disposed || generation !== loadGeneration) return
      const { message } = parseXHubError(e)
      error.value = message
      onError?.(message)
      void tauriApi.logClientError({
        message: '扩展入口加载失败',
        detail: `extId=${getExtId()} surface=${getSurface() ?? ''} | ${message}`,
      })
      loading.value = false // 出错立刻撤掉，交给错误态显示
    }
    // 成功路径**不**在这里撤 loading：等第一条桥消息（见 onMessage 的 firstAlive）
    // 或看门狗超时。这正是修「点开扩展先空白」的关键——具体原因见上面 firstAlive 处的注释。
  }

  // ---- 开发扩展热重载：轮询开发目录内容戳，变化即重载当前 iframe ----
  // 仅当当前扩展确实是「开发扩展」时才重载（已装扩展不受本机源码目录变动影响）。
  // 戳由后端对源码目录树（跳过 node_modules / 隐藏目录）的「相对路径 + mtime」算 FNV，
  // 因此改 HTML/CSS/JS 都会触发——已装扩展那套 extensions_stamp 只盯 manifest，不够用。
  const DEV_POLL_MS = 1500
  let devPollTimer: number | undefined
  let devStamp: number | null = null

  async function devPollTick() {
    if (!isTauri()) return
    try {
      const stamp = await tauriApi.devExtensionsStamp()
      if (!stamp) return // 0 = 没有登记任何本机源码目录
      if (devStamp === null) {
        devStamp = stamp
        return
      }
      if (stamp === devStamp) return
      devStamp = stamp
      const status = await tauriApi.getDevModeStatus()
      if (status.extensions.some((d) => d.id === getExtId())) {
        void load()
      }
    } catch {
      // 轮询失败不影响正常使用
    }
  }

  onMounted(() => {
    if (isTauri()) {
      void import('@tauri-apps/api/event').then(({ listen }) => listen<string>(
        'extension-permissions-changed', (event) => {
          if (event.payload === getExtId()) void load()
        },
      )).then((unlisten) => {
        if (disposed) unlisten()
        else unlistenPermissions = unlisten
      }).catch(() => {})
    }
    window.addEventListener('message', onMessage)
    document.addEventListener('visibilitychange', onVisibilityChange)
    registerExtensionFrame(frameRef.value, getExtId())
    if (frameRef.value) frameRef.value.addEventListener('error', onFrameError)
    void load()
    devPollTimer = window.setInterval(() => void devPollTick(), DEV_POLL_MS)
  })
  onBeforeUnmount(() => {
    disposed = true
    expectedOrigin = ''
    loadGeneration++
    unlistenPermissions?.()
    if (watchdogTimer !== undefined) clearTimeout(watchdogTimer)
    if (devPollTimer !== undefined) clearInterval(devPollTimer)
    if (frameRef.value) frameRef.value.removeEventListener('error', onFrameError)
    document.removeEventListener('visibilitychange', onVisibilityChange)
    unregisterExtensionFrame(frameRef.value)
    window.removeEventListener('message', onMessage)
  })

  // 扩展 id / 形态 / 强制重载计数变化（主区在扩展之间来回切换时 ExtensionView 复用同一个
  // iframe；点击同一个已打开的扩展时仅 reloadKey 递增）：重新注册帧映射 + 重新加载入口，
  // 否则 iframe 停留在上一个扩展或已丢失的旧内容，表现为“切换没反应”或空白
  watch([getExtId, getSurface, getReloadKey ?? (() => 0)], () => {
    unregisterExtensionFrame(frameRef.value)
    registerExtensionFrame(frameRef.value, getExtId())
    void load()
  })

  // 模块形态变化：不重载 iframe（避免闪白/丢状态），仅广播新形态给扩展自行切换内容
  if (getVariant) {
    watch(getVariant, () => broadcastVariant())
  }

  return { frameRef, loading, error }
}
