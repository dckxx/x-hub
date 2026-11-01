import { ref } from 'vue'
import { emitTo } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from '../api/tauri'
import { broadcastThemeToFrames } from './themeTokens'

/**
 * 主题应用的纯函数层（不 import store）。
 *
 * 为什么要单独拆出来：轻量壳窗口（chrome.html / ball.html / chat.html …）只想要
 * 「把主题写到根元素」这一个动作，但 `useTheme.ts` 为了 watch 全量配置 import 了
 * `useStore` —— 于是任何 `import { applyTheme } from './useTheme'` 的入口都会把整份
 * store（Vue 应用实例 + 组件树 + 全量数据拷贝）拖进 renderer，正是轻量入口要减掉的
 * 活堆。chrome.html 曾因此 preload 了 workbench-*.js。
 *
 * 分层约定：
 * - 本文件 = 主题的**纯应用**（写 CSS 变量 / dataset + 广播），零 store 依赖；
 * - `useTheme.ts` = 主窗的**驱动**（watch store 配置），re-export 本文件的函数；
 * - `useShellThemeConfig.ts` = 轻量壳窗口的驱动（拉 get_ui_config 后调本文件）。
 * 新增窗口要主题：独立窗口一律走 shell 那套，别 import useTheme。
 */

// 系统偏好监听：同一 WebView 内多个调用点共用模块级单例
const systemDark = ref(false)
let mq: MediaQueryList | null = null
let mqHandler: ((e: MediaQueryListEvent) => void) | null = null

export function initSystemTheme() {
  if (mq || typeof window === 'undefined' || !window.matchMedia) return
  mq = window.matchMedia('(prefers-color-scheme: dark)')
  systemDark.value = mq.matches
  mqHandler = (e) => {
    systemDark.value = e.matches
  }
  mq.addEventListener('change', mqHandler)
}

export function applyTheme(opts: { mode: string; preset: string; accent: string | null; darkOverride?: boolean }) {
  const el = document.documentElement
  const dark = opts.mode === 'dark' || (opts.mode === 'system' && (opts.darkOverride ?? systemDark.value))
  el.dataset.theme = dark ? 'dark' : ''
  el.dataset.preset = opts.preset
  if (opts.accent) {
    // inline --accent 优先级最高：CSS 里 --brand-* 一律引用 var(--accent)
    el.style.setProperty('--accent', opts.accent)
  } else {
    el.style.removeProperty('--accent')
  }
  // 主题变化后广播给所有活动扩展 iframe（扩展页面实时跟随宿主换色/换主题）
  requestAnimationFrame(() => broadcastThemeToFrames())
  // 推送独立 WebView 窗口（悬浮球/通知窗/对话窗各自经 get_theme_config 自取初始值，
  // 此处负责运行时跟随；目标窗未启用/不存在时静默失败）。
  // **只有主窗广播**：独立窗自己 applyTheme 时若再推送，会收到自己发出的事件又触发一次
  // applyTheme → 自激循环（并反复唤醒球/通知窗），故按窗口 label 门控。
  if (isTauri() && isMainWindow()) {
    emitTo('floating-ball', 'floating-ball-theme', { accent: opts.accent, dark }).catch(() => {})
    // 推送通知窗（独立 WebView，运行时跟随主窗主题）
    emitTo('notice', 'notice-theme', {
      mode: opts.mode,
      preset: opts.preset,
      accent: opts.accent,
    }).catch(() => {})
    // 推送 AI 对话独立窗口（独立 WebView，同款机制）
    emitTo('chat', 'chat-theme', {
      mode: opts.mode,
      preset: opts.preset,
      accent: opts.accent,
    }).catch(() => {})
  }
}

// 主窗判定（模块级缓存：一个 WebView 的 label 全程不变）
let mainWindowFlag: boolean | null = null
function isMainWindow(): boolean {
  if (mainWindowFlag === null) {
    mainWindowFlag = getCurrentWindow().label === 'main'
  }
  return mainWindowFlag
}

export interface FontScale {
  global: number
  sticky: number
  notes: number
  prompt: number
  todo: number
}

/** 注入字体缩放 CSS 变量：--fs-global 作用于根字号（rem 基准），--fs-* 作用于各内容模块 */
export function applyFontScale(s: FontScale) {
  const el = document.documentElement
  el.style.setProperty('--fs-global', String(s.global))
  el.style.setProperty('--fs-sticky', String(s.sticky))
  el.style.setProperty('--fs-notes', String(s.notes))
  el.style.setProperty('--fs-prompt', String(s.prompt))
  el.style.setProperty('--fs-todo', String(s.todo))
}

/** 注入卡片玻璃透明度：--glass-dim 乘进 --frost-base 的 alpha（0.4–1.0，1 = 默认不透明） */
export function applyGlassOpacity(v: number) {
  const clamped = Math.min(1, Math.max(0.4, v))
  document.documentElement.style.setProperty('--glass-dim', String(clamped))
}

/** 沉浸模式标记：CSS 侧据此对 .card 启用 backdrop-filter 局部取景模糊（ADR 0003 受控例外） */
export function applyImmersive(on: boolean) {
  const el = document.documentElement
  if (on) {
    el.dataset.immersive = '1'
  } else {
    delete el.dataset.immersive
  }
}

export function systemDarkMode() {
  return systemDark
}
