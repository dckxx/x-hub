<script setup lang="ts">
// 全局搜索浮窗（label "search"）：热键直达的独立小窗，**不唤起主窗**。
//
// 背景：搜索 UI（GlobalSearch.vue）原先挂主窗遮罩层，显示它必须先亮主窗——这就是
// 「按 Ctrl+K 先弹软件、再弹搜索框」的根因。挪进独立窗后热键只动这一个窗口。
//
// 生命周期：首次按热键时后端按需建窗（search_window.rs，BUILDING 单飞 + 后台线程），
// 建好后隐藏常驻复用，关闭只是 hide。窗口常驻 → 本组件全程存在，监听都是常驻监听。
//
// 轻量入口（search.html）：不加载全量 SPA，主题走 useShellThemeConfig 自举，
// 检索由 GlobalSearch 直调命令，快捷键文案从 get_ui_config 取。
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type { ExtensionEntry, Note, Resource, Todo } from '../api/tauri'
import { isTauri, tauriApi } from '../api/tauri'
import GlobalSearch from './GlobalSearch.vue'
import { applyTheme } from '../composables/themeCore'
import { useShellThemeConfig } from '../composables/useShellThemeConfig'
import { useScrollbarHover } from '../composables/useScrollbarHover'

// 主题三轴 + 字号缩放：轻量壳窗口不加载 store，走 get_ui_config 自举
// （口径与主窗 useTheme 逐条对齐）；运行时跟随主窗 useTheme 的 search-theme 推送。
// 玻璃透明度/沉浸模式是主窗壁纸语境参数，跳过——卡片本体走 --bg-float-panel 不透明底。
const shellTheme = useShellThemeConfig({ skipGlass: true })
// 结果列表有滚动，滚动条 hover 显隐要显式接（原先靠 App.vue 那段统一挂）
useScrollbarHover()
document.documentElement.dataset.searchWindow = ''

/** 弹窗可见性：由后端 search-window-shown / hidden 事件驱动，不由窗内状态决定 */
const active = ref(false)
/** 主题是否已应用（首帧防闪：暗色主题下首次唤起，先等主题就绪再渲染卡片，
 *  否则窗口 present 在 get_ui_config 往返前、卡片按默认亮色令牌闪一下再变色） */
const themeReady = ref(false)
/** 卡片右下角的呼出快捷键文案（跟随设置 → 快捷键的自定义配置） */
const shortcutLabel = ref('Ctrl+K')
/** 已安装扩展列表（扩展检索的数据源）：每次唤起重拉一次，装卸扩展后搜得到 */
const extensions = ref<ExtensionEntry[]>([])

let unlistenTheme: (() => void) | null = null
let unlistenShown: (() => void) | null = null
let unlistenHidden: (() => void) | null = null

async function refreshShortcutLabel() {
  try {
    const cfg = await tauriApi.getUiConfig()
    shortcutLabel.value = cfg.search_shortcut || 'Ctrl+K'
  } catch {
    // 忽略：命令未就绪时保留默认文案，不影响检索
  }
}

onMounted(async () => {
  if (!isTauri()) {
    // 浏览器预览不会走到本入口，兜底直接显示以便调试
    themeReady.value = true
    active.value = true
    return
  }
  await refreshShortcutLabel()
  // 主题先于一切 UI 就绪（防首帧闪亮色，见 themeReady 注释）：shown/hidden 监听与
  // 可见性拉取都放在这之后——拉取的 active 置位只有等主题就绪才反映到卡片上，不互踩
  try {
    await shellTheme.refresh()
  } catch {
    // 配置拉取失败保持默认亮色，照样点亮卡片，不能让搜索永远不出来
  }
  themeReady.value = true
  unlistenTheme = await listen<{ mode: string; preset: string; accent: string | null }>(
    'search-theme',
    (e) => applyTheme(e.payload),
  )
  // 唤起：每次重新拉主题与快捷键文案（主窗广播 search-theme 只覆盖「浮窗已存在」
  // 的情形；首次建窗那一刻主窗可能还没广播过）。再打开卡片——active 由 false→true
  // 正好命中 GlobalSearch 对 visible 的 watch（清空关键词/结果）与焦点陷阱（聚焦输入框）。
  unlistenShown = await listen('search-window-shown', async () => {
    try {
      await shellTheme.refresh()
    } catch {
      // 忽略：配置拉取失败保持当前观感，不影响检索
    }
    await refreshShortcutLabel()
    // 扩展列表重拉（list_extensions 是磁盘扫描，按需一次而非驻留轮询）；失败沿用上次
    try {
      extensions.value = await tauriApi.listExtensions()
    } catch {
      // 忽略：列表沿用上次结果
    }
    active.value = true
  })
  // 收起：只隐藏卡片，窗口本身常驻复用（后端已 hide 窗口）
  unlistenHidden = await listen('search-window-hidden', () => {
    active.value = false
  })
  // 就绪后主动拉一次可见性（推拉双保险的「拉」）：建窗完成那一刻 present() 发出的
  // shown 可能比上面的监听注册早（首次建窗 ~1s 后页面才挂载完），推丢了就表现为
  // 「窗口出来了但卡片没渲染」——像没呼出，得再按好几次热键。只置 true 不置 false，
  // 与拉取期间收到的 shown 事件不互踩。
  try {
    if (await tauriApi.searchWindowIsVisible()) active.value = true
  } catch {
    // 忽略：命令未就绪时等下一次 shown 推送
  }
})

onBeforeUnmount(() => {
  unlistenTheme?.()
  unlistenShown?.()
  unlistenHidden?.()
})

/** Esc / 点击卡片空白处：让后端收起窗口（隐藏 + 把焦点还给唤起前的应用） */
function requestClose() {
  active.value = false
  if (isTauri()) void tauriApi.searchWindowHide().catch(() => {})
}

function onOpenResource(r: Resource) {
  // 结果路由在后端：程序/文件/独立窗网页直接启动（主窗不出现），
  // 只有「软件内面板」网页与笔记/待办才需要主窗
  if (isTauri()) void tauriApi.searchOpenResource(r.id).catch(() => {})
  requestClose()
}

function onOpenNote(n: Note) {
  if (isTauri()) void tauriApi.searchOpenNote(n.id).catch(() => {})
  requestClose()
}

function onOpenTodo(t: Todo) {
  if (isTauri()) void tauriApi.searchOpenTodo(t.id).catch(() => {})
  requestClose()
}

function onOpenSettings(section: string) {
  // 后端亮主窗并派发 open-search-settings，主窗切到对应设置分区
  if (isTauri()) void tauriApi.searchOpenSettings(section).catch(() => {})
  requestClose()
}

function onOpenExtension(id: string) {
  // 后端亮主窗并派发 open-search-extension，主窗按该扩展的打开方式分流
  if (isTauri()) void tauriApi.searchOpenExtension(id).catch(() => {})
  requestClose()
}
</script>

<template>
  <GlobalSearch
    :visible="themeReady && active"
    standalone
    :shortcut-label="shortcutLabel"
    :extensions="extensions"
    @close="requestClose"
    @open-resource="onOpenResource"
    @open-note="onOpenNote"
    @open-todo="onOpenTodo"
    @open-settings="onOpenSettings"
    @open-extension="onOpenExtension"
  />
</template>
