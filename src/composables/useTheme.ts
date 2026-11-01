import { watch } from 'vue'
import { useStore } from '../stores/workbench'
import { broadcastThemeToFrames } from './themeTokens'
// 纯应用层（写 CSS 变量 / dataset + 广播）拆到 themeCore，本文件只负责「驱动」：
// watch store 配置调它。轻量壳窗口要主题走 themeCore / useShellThemeConfig，
// 不要 import 本文件——这里 import 了 useStore，会拖进整份 store。
import {
  applyFontScale,
  applyGlassOpacity,
  applyImmersive,
  applyTheme,
  initSystemTheme,
  systemDarkMode,
} from './themeCore'

// 纯函数与系统偏好单例都在 themeCore；这里 re-export 保持既有 import 路径不变
// （ChatWindow / light/chrome.ts 等直接 import { applyTheme } from './useTheme'）。
export { applyFontScale, applyGlassOpacity, applyImmersive, applyTheme, systemDarkMode }

/** 组件挂载时调用一次即完成主题应用与实时跟随（主窗 / 走完整 SPA 的窗口用） */
export function useTheme() {
  initSystemTheme()
  const systemDark = systemDarkMode()
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
    () =>
      [
        store.state.config.wallpaper_immersive,
        store.state.config.wallpaper_path,
      ] as const,
    ([immersive, path]) => {
      // 沉浸模式仅在存在壁纸时生效（壁纸单一套，所有主题共用）
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
