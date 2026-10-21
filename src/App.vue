<script setup lang="ts">
// 应用根壳：主窗口渲染完整首页；便签浮窗（sticky-*）、倒计时浮窗（countdown-*)与剪贴板浮层（clipboard）渲染独立小窗
// ⚠️ 路由必须用 webview label 而非 window label（getCurrentWindow().label）：速达独立浏览器
// 的 chrome 页是池窗口（suda-web-{i}）的子 webview，窗口 label 是 suda-web-{i}、webview label
// 才是 suda-web-{i}-chrome——用窗口 label 会匹配不上 chrome 正则，落到兜底把整个工作台
// 渲染进浏览器窗口（实测踩过）。单 webview 窗口两类 label 恒等，统一用 webview label 无副作用。
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { listen } from '@tauri-apps/api/event'
import { onBeforeUnmount, onMounted } from 'vue'
import Index from './index/index.vue'
import DetachedStickyWindow from './components/DetachedStickyWindow.vue'
import CountdownFloat from './components/CountdownFloat.vue'
import ClipboardOverlay from './components/ClipboardOverlay.vue'
import ExtensionWindow from './components/ExtensionWindow.vue'
import PromptFloat from './components/PromptFloat.vue'
import TodoFloat from './components/TodoFloat.vue'
import FloatingBallWindow from './components/FloatingBallWindow.vue'
import ChatWindow from './components/ChatWindow.vue'
import NoticeOverlay from './components/NoticeOverlay.vue'
import BrowserChrome from './components/BrowserChrome.vue'
import UpdateCheckDialog from './components/UpdateCheckDialog.vue'
import WindowResizeEdges from './components/WindowResizeEdges.vue'
import { isTauri } from './api/tauri'

const label = isTauri() ? getCurrentWebview().label : ''
const isMainWindow = label === 'main'
const isStickyWindow = label.startsWith('sticky-')
const isCountdownFloat = label.startsWith('countdown-')
const isClipboardOverlay = label === 'clipboard'
const isExtensionWindow = label.startsWith('ext-')
const isPromptFloat = label === 'prompt-float'
const isTodoFloat = label === 'todo-float'
const isFloatingBall = label === 'floating-ball'
const isChatWindow = label === 'chat'
const isNoticeWindow = label === 'notice'
// 速达独立应用内浏览器（ADR 0011）：chrome webview 渲染顶栏；content webview 只加载
// 外站页面，不会加载本 SPA，但万一误加载也渲染空白兜底（绝不能落到 Index 整窗 UI）
const isSudaBrowserChrome = /^suda-web-\d+-chrome$/.test(label)
const isSudaBrowserContent = /^suda-web-\d+-content$/.test(label)

// 任务管理器身份：Win11 任务管理器的 WebView2 列表按「页面标题」命名各 renderer，
// 这些窗口共用 index.html 时全部显示「个人效率工作台」分不清谁是谁——按 label
// 设置 document.title。只改页面标题，不影响任务栏/Alt+Tab 的原生窗口标题
// （那来自 tauri 侧的窗口 title，wry 不监听 DocumentTitleChanged）。
// 悬浮球/通知/chrome 页走轻量入口，各自 html 里已带独立标题，不经这里。
if (isTauri()) {
  document.title = isMainWindow
    ? 'x-hub 主窗'
    : isChatWindow
      ? 'x-hub 对话'
      : isClipboardOverlay
        ? 'x-hub 剪贴板'
        : isPromptFloat
          ? 'x-hub 提示词'
          : isTodoFloat
            ? 'x-hub 待办'
            : isExtensionWindow
              ? 'x-hub 扩展'
              : isStickyWindow
                ? 'x-hub 便签'
                : isCountdownFloat
                  ? 'x-hub 倒计时'
                  : document.title
}

// 主窗口：记录最后聚焦的可编辑元素。剪贴板浮层粘贴到主窗口输入框时，
// Rust 侧会派发 clipboard-paste-request（带内容），这里直接把内容插回原输入框。
// 不依赖窗口激活/焦点时序——WebView2 内恢复焦点后再注入 Ctrl+V 并不可靠。
let lastEditable: HTMLElement | null = null
let unlistenPasteRequest: (() => void) | null = null

function onFocusIn(e: FocusEvent) {
  const t = e.target as HTMLElement
  if (t.closest('input, textarea, [contenteditable]')) lastEditable = t
}

// 光标处插入文本并派发 input 事件驱动 v-model（无焦点时 setRangeText 同样生效）
function insertTextAtCaret(el: HTMLElement, text: string) {
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) {
    el.focus()
    const start = el.selectionStart ?? el.value.length
    const end = el.selectionEnd ?? start
    el.setRangeText(text, start, end, 'end')
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
    return
  }
  if (el.isContentEditable) {
    const sel = window.getSelection()
    if (sel && sel.rangeCount > 0 && el.contains(sel.anchorNode)) {
      const range = sel.getRangeAt(0)
      range.deleteContents()
      range.insertNode(document.createTextNode(text))
      range.collapse(false)
      sel.removeAllRanges()
      sel.addRange(range)
    } else {
      el.appendChild(document.createTextNode(text))
    }
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }))
  }
}

// ---- 滚动条 hover 显隐：鼠标悬停到可滚动区域时给该滚动容器加 .scrollbar-hover ----
let hoveredScroller: HTMLElement | null = null
let scrollRaf: number | null = null
let pendingTarget: Element | null = null

function findScrollable(el: Element | null): HTMLElement | null {
  let cur = el as HTMLElement | null
  while (cur && cur !== document.documentElement) {
    const s = getComputedStyle(cur)
    const y = s.overflowY
    const x = s.overflowX
    const canY = (y === 'auto' || y === 'scroll' || y === 'overlay') && cur.scrollHeight > cur.clientHeight + 1
    const canX = (x === 'auto' || x === 'scroll' || x === 'overlay') && cur.scrollWidth > cur.clientWidth + 1
    if (canY || canX) return cur
    cur = cur.parentElement
  }
  return null
}

function onScrollPointerMove(e: PointerEvent) {
  // 同步捕获 target：rAF 回调里事件对象可能已被复用，不能再读 e.target
  pendingTarget = e.target as Element | null
  if (scrollRaf != null) return
  scrollRaf = requestAnimationFrame(() => {
    scrollRaf = null
    const target = pendingTarget
    pendingTarget = null
    const scroller = findScrollable(target)
    if (scroller === hoveredScroller) return
    hoveredScroller?.classList.remove('scrollbar-hover')
    hoveredScroller = scroller
    scroller?.classList.add('scrollbar-hover')
  })
}

function onScrollMouseLeave() {
  hoveredScroller?.classList.remove('scrollbar-hover')
  hoveredScroller = null
}

onMounted(async () => {
  // 滚动条 hover 显隐（所有窗口 + 浏览器预览均生效）
  document.addEventListener('pointermove', onScrollPointerMove, { passive: true })
  document.addEventListener('mouseleave', onScrollMouseLeave)

  if (!isTauri() || !isMainWindow) return
  document.addEventListener('focusin', onFocusIn)
  unlistenPasteRequest = await listen('clipboard-paste-request', (e) => {
    const payload = (e.payload ?? {}) as { content?: string; html?: string | null }
    if (!payload.content) return
    const target = lastEditable
    if (target && document.contains(target)) {
      insertTextAtCaret(target, payload.content)
    }
  })
})

onBeforeUnmount(() => {
  document.removeEventListener('pointermove', onScrollPointerMove)
  document.removeEventListener('mouseleave', onScrollMouseLeave)
  document.removeEventListener('focusin', onFocusIn)
  unlistenPasteRequest?.()
})
</script>

<template>
  <ClipboardOverlay v-if="isClipboardOverlay" />
  <CountdownFloat v-else-if="isCountdownFloat" />
  <DetachedStickyWindow v-else-if="isStickyWindow" />
  <ExtensionWindow v-else-if="isExtensionWindow" />
  <PromptFloat v-else-if="isPromptFloat" />
  <TodoFloat v-else-if="isTodoFloat" />
  <FloatingBallWindow v-else-if="isFloatingBall" />
  <ChatWindow v-else-if="isChatWindow" />
  <NoticeOverlay v-else-if="isNoticeWindow" />
  <BrowserChrome v-else-if="isSudaBrowserChrome" />
  <div v-else-if="isSudaBrowserContent" class="suda-content-blank" />
  <Index v-else />
  <!-- 主窗 8 方向隐形缩放边缘：0.8.1 起 shadow=false 后宿主收不到边缘命中，拖边调大小失效， -->
  <!-- 用透明覆盖条 + startResizeDragging 补回（最大化/全屏自动隐藏，见组件注释） -->
  <WindowResizeEdges v-if="isMainWindow" />
  <UpdateCheckDialog v-if="isMainWindow" />
</template>
