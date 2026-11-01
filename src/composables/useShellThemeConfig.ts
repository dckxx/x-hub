import { ref, watch } from 'vue'
import { isTauri, tauriApi, type AppConfig } from '../api/tauri'
// 只 import 纯应用层：这里绝不能走 useTheme（它 import 了 useStore，会把整份
// store 拖进 renderer，轻量入口就白做了）
import {
  applyFontScale,
  applyGlassOpacity,
  applyImmersive,
  applyTheme,
} from './themeCore'

/**
 * 轻量壳窗口（chat.html / clipboard.html）的主题自举。
 *
 * 背景：这些浮窗以前加载 index.html，`useTheme()` 直接 watch 全量 store；改走
 * 轻量入口后不能再拖整个 store（Vue 应用实例 + 组件树 + 全量数据拷贝正是要减掉的
 * 活堆）。这里只拿 `get_ui_config` 的那一份配置，字段口径与 useTheme 的三个 watch
 * **逐条对齐**——主题三轴、玻璃透明度、沉浸模式、字号缩放四组，漏一项都会让浮窗
 * 与主窗观感不一致（历史上 chrome 入口就漏了玻璃透明度/字号）。
 *
 * 与 `useTheme` 的分工：主窗仍用 `useTheme`（要 watch store 的实时改动 + 广播扩展
 * iframe）；独立壳窗口用本函数，只管自己的根元素。主窗的运行时推送（chat-theme /
 * clipboard-theme 等事件）由各组件自行 listen 后调 `applyTheme`，不走这里。
 */
export function useShellThemeConfig() {
  const config = ref<AppConfig | null>(null)

  function applyAll(cfg: AppConfig) {
    applyTheme({ mode: cfg.theme_mode, preset: cfg.theme_preset, accent: cfg.accent_color })
    applyGlassOpacity(cfg.glass_opacity)
    // 沉浸模式仅在存在壁纸时生效（口径同 useTheme）
    applyImmersive(cfg.wallpaper_immersive && !!cfg.wallpaper_path)
    applyFontScale({
      global: cfg.font_scale,
      sticky: cfg.font_sticky,
      notes: cfg.font_notes,
      prompt: cfg.font_prompt,
      todo: cfg.font_todo,
    })
  }

  async function refresh() {
    if (!isTauri()) return
    try {
      config.value = await tauriApi.getUiConfig()
    } catch {
      // 命令未就绪/失败：保持当前观感（默认令牌），不阻断窗口渲染
    }
  }

  watch(config, (cfg) => {
    if (cfg) applyAll(cfg)
  })

  void refresh()
  return { config, refresh }
}
