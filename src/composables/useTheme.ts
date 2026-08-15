import { ref, watch } from 'vue'
import { useStore } from '../stores/workbench'

// 系统偏好监听：三个窗口（主窗/便签浮窗/倒计时浮窗）共用同一模块级单例
const systemDark = ref(false)
let mq: MediaQueryList | null = null
let mqHandler: ((e: MediaQueryListEvent) => void) | null = null

function initSystemTheme() {
  if (mq || typeof window === 'undefined' || !window.matchMedia) return
  mq = window.matchMedia('(prefers-color-scheme: dark)')
  systemDark.value = mq.matches
  mqHandler = (e) => { systemDark.value = e.matches }
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
}

/** 组件挂载时调用一次即完成主题应用与实时跟随 */
export function useTheme() {
  initSystemTheme()
  const store = useStore()
  watch(
    () => [store.state.config.theme_mode, store.state.config.theme_preset, store.state.config.accent_color] as const,
    ([mode, preset, accent]) => applyTheme({ mode, preset, accent }),
    { immediate: true },
  )
  watch(systemDark, () => {
    if (store.state.config.theme_mode === 'system') {
      applyTheme({ mode: 'system', preset: store.state.config.theme_preset, accent: store.state.config.accent_color })
    }
  })
}

export function systemDarkMode() { return systemDark }
