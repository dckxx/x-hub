import { onBeforeUnmount, onMounted } from 'vue'

/**
 * 滚动条 hover 显隐：鼠标移到任意可滚动容器上时给它加 `.scrollbar-hover`，
 * CSS 侧 `html .scrollbar-hover::-webkit-scrollbar-thumb` 才把滑块显出来。
 *
 * 为什么需要这段：全局滚动条默认完全透明（thumb/track 全透明，见 style.css），
 * 否则常驻卡片满屏滚动条很吵；代价是「不 hover 就找不到滚动条」。注意
 * `::-webkit-scrollbar-thumb:hover` 只在指针真的压在滑块上时生效——而滑块本身
 * 是透明的、压根看不见，所以没有本监听时滚动区等于「无滚动条」。
 *
 * 原先内联在 App.vue 里，只覆盖走 index.html 的窗口；浮窗改轻量入口后
 * （chat.html / clipboard.html）不再经过 App.vue，必须各自显式调用——
 * 任何带滚动区的轻量壳窗口都要调它，否则滚动条永久隐形。
 */
export function useScrollbarHover() {
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

  onMounted(() => {
    document.addEventListener('pointermove', onScrollPointerMove, { passive: true })
    document.addEventListener('mouseleave', onScrollMouseLeave)
  })

  onBeforeUnmount(() => {
    document.removeEventListener('pointermove', onScrollPointerMove)
    document.removeEventListener('mouseleave', onScrollMouseLeave)
    if (scrollRaf != null) {
      cancelAnimationFrame(scrollRaf)
      scrollRaf = null
    }
    hoveredScroller?.classList.remove('scrollbar-hover')
    hoveredScroller = null
  })
}
