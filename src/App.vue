<script setup lang="ts">
// 应用根壳：主窗口渲染完整首页；便签浮窗（sticky-*）、倒计时浮窗（countdown-*）与剪贴板浮层（clipboard）渲染独立小窗
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { onBeforeUnmount, onMounted } from 'vue'
import Index from './index/index.vue'
import DetachedStickyWindow from './components/DetachedStickyWindow.vue'
import CountdownFloat from './components/CountdownFloat.vue'
import ClipboardOverlay from './components/ClipboardOverlay.vue'
import { isTauri } from './api/tauri'

const label = isTauri() ? getCurrentWindow().label : ''
const isMainWindow = label === 'main'
const isStickyWindow = label.startsWith('sticky-')
const isCountdownFloat = label.startsWith('countdown-')
const isClipboardOverlay = label === 'clipboard'

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
  <Index v-else />
</template>
