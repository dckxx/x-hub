import { ref, watch } from 'vue'
import { useStore } from '../stores/workbench'
import { broadcastThemeToFrames } from './themeTokens'

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
  // 主题变化后广播给所有活动扩展 iframe（扩展页面实时跟随宿主换色/换主题）
  requestAnimationFrame(() => broadcastThemeToFrames())
}

interface FontScale {
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
  watch(
    () => store.state.config.glass_opacity,
    (v) => {
      applyGlassOpacity(v)
      // 玻璃透明度乘进 --frost-* 的 alpha（影响 --xhub-surface），需重新广播令牌给扩展
      requestAnimationFrame(() => broadcastThemeToFrames())
    },
    { immediate: true },
  )
  watch(
    () => [store.state.config.wallpaper_immersive, store.state.config.wallpaper_path] as const,
    ([immersive, path]) => {
      applyImmersive(immersive && !!path)
      // 沉浸模式/壁纸在场改变扩展令牌的壁纸状态与表面 alpha，同步广播
      requestAnimationFrame(() => broadcastThemeToFrames())
    },
    { immediate: true },
  )
  watch(
    () =>
      [
        store.state.config.font_scale,
        store.state.config.font_sticky,
        store.state.config.font_notes,
        store.state.config.font_prompt,
        store.state.config.font_todo,
      ] as const,
    ([global, sticky, notes, prompt, todo]) =>
      applyFontScale({ global, sticky, notes, prompt, todo }),
    { immediate: true },
  )
}

export function systemDarkMode() { return systemDark }
