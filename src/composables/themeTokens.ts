/**
 * 扩展主题令牌：把宿主的主题（CSS 变量解析后的实际值）收集并广播给扩展 iframe。
 *
 * 扩展入口 HTML 由 `read_extension_entry` 注入桥脚本，桥脚本在 iframe 内
 * 把令牌写成 `--xhub-*` CSS 变量 + `data-xhub-theme`，扩展 CSS 直接
 * `var(--xhub-accent)` 等即可跟随宿主换色/换主题，无需写任何 JS。
 */

/** 扩展 iframe 内暴露的令牌形态 */
export interface XHubThemeTokens {
  mode: 'light' | 'dark'
  preset: string | null
  accent: string
  /** 壁纸状态：扩展据此启停文字光晕等透底可读性样式（桥脚本转写为 data-xhub-wallpaper* 属性） */
  wallpaper: { on: boolean; clear: boolean; immersive: boolean }
  tokens: Record<string, string>
}

/** 宿主根元素变量名 → 扩展令牌字段名（对齐桥脚本 applyTheme 的 map） */
const TOKEN_MAP: Record<string, string> = {
  accent: '--accent',
  brand: '--brand-500',
  brandSoft: '--brand-50',
  bgCard: '--bg-card-solid',
  surface: '--frost-surface',
  text1: '--text-1',
  text2: '--text-2',
  text3: '--text-3',
  border: '--border-soft',
  red: '--c-red',
  green: '--c-green',
  yellow: '--c-yellow',
  blue: '--c-blue',
  orange: '--c-orange',
  radiusLg: '--radius-lg',
}

/** 读取宿主主题令牌（颜色值已由 getComputedStyle 计算） */
export function collectThemeTokens(): XHubThemeTokens {
  const root = document.documentElement
  // 壁纸态可读性覆盖（压墨/白墨的 --text-*）作用在 .sidebar/.title-bar/main 作用域而非根元素，
  // 必须从 main 读取，扩展拿到的文字令牌才会跟随壁纸模式翻转
  const scope = document.querySelector('main') ?? root
  const cs = getComputedStyle(scope)
  const tokens: Record<string, string> = {}
  for (const [key, varName] of Object.entries(TOKEN_MAP)) {
    tokens[key] = cs.getPropertyValue(varName).trim()
  }
  // 页面背景是渐变（预设 --app-bg 覆盖，否则 --bg-page-surface），不是纯色 --bg-page：
  // view/window/drawer 形态用 --xhub-bg-page 铺底，才能与宿主其他 View 的页面背景严格一致
  const appBg = cs.getPropertyValue('--app-bg').trim()
  tokens.bgPage = appBg || cs.getPropertyValue('--bg-page-surface').trim()
  const mode = root.dataset.theme === 'dark' ? 'dark' : 'light'
  const preset = root.dataset.preset ?? null
  const accent = cs.getPropertyValue('--accent').trim()
  return {
    mode,
    preset,
    accent,
    wallpaper: {
      on: root.dataset.wallpaper === '1',
      clear: root.dataset.wallpaperClear === '1',
      immersive: root.dataset.immersive === '1',
    },
    tokens,
  }
}

/** 活动扩展 iframe 注册表：frame → 扩展 id（主题广播 + 扩展间事件路由共用） */
const activeFrames = new Map<HTMLIFrameElement, string>()

export function registerExtensionFrame(el: HTMLIFrameElement | null, extId: string) {
  if (el) activeFrames.set(el, extId)
}

export function unregisterExtensionFrame(el: HTMLIFrameElement | null) {
  if (el) activeFrames.delete(el)
}

/** 把当前主题广播给所有活动扩展 iframe（iframe 内桥脚本负责应用） */
export function broadcastThemeToFrames() {
  const theme = collectThemeTokens()
  for (const frame of activeFrames.keys()) {
    frame.contentWindow?.postMessage({ __xhub: true, type: 'theme', theme }, '*')
  }
}

/**
 * 把一条扩展自定义事件广播给所有其它扩展 iframe（跳过来源自己）。
 * 权限校验（events.emit 需 manifest 声明 `events` 权限）已在调用方经 Rust 完成。
 */
export function broadcastExtensionEvent(fromExtId: string, event: string, payload: unknown) {
  for (const [frame, extId] of activeFrames) {
    if (extId === fromExtId) continue
    frame.contentWindow?.postMessage(
      { __xhub: true, type: 'event', event, payload, from: fromExtId },
      '*',
    )
  }
}

// ---------------------------------------------------------------------------
// 跨扩展调用 RPC：调用方 iframe → 主窗口 → 目标 iframe → 主窗口 → 调用方
// ---------------------------------------------------------------------------

/** 待回传的跨扩展调用：requestId → 调用方 frame（模块级，跨 useExtensionFrame 实例共享） */
const pendingCalls = new Map<number, HTMLIFrameElement>()

/** 把一次跨扩展调用路由到目标扩展 iframe；目标未打开则直接回错误 */
export function routeExtensionCall(
  fromFrame: HTMLIFrameElement,
  requestId: number,
  targetId: string,
  method: string,
  payload: unknown,
) {
  for (const [frame, extId] of activeFrames) {
    if (extId === targetId) {
      pendingCalls.set(requestId, fromFrame)
      frame.contentWindow?.postMessage(
        { __xhub: true, type: 'xhub-call-req', id: requestId, method, payload },
        '*',
      )
      return
    }
  }
  fromFrame.contentWindow?.postMessage(
    {
      __xhub: true,
      type: 'xhub-call-result',
      id: requestId,
      ok: false,
      error: { message: `目标扩展「${targetId}」未打开，请先在扩展中心打开它再调用` },
    },
    '*',
  )
}

/** 把目标扩展的调用结果回传给发起调用的 iframe */
export function routeExtensionCallResult(
  requestId: number,
  ok: boolean,
  data: unknown,
  error: unknown,
) {
  const fromFrame = pendingCalls.get(requestId)
  if (!fromFrame) return
  pendingCalls.delete(requestId)
  fromFrame.contentWindow?.postMessage(
    { __xhub: true, type: 'xhub-call-result', id: requestId, ok, data, error },
    '*',
  )
}
