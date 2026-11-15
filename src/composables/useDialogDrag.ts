import { onBeforeUnmount, onMounted } from 'vue'

/**
 * 让应用内所有弹窗（`.modal-card`）都能拖动。
 *
 * 为什么需要：新建资源 / 新建待办 / 新建倒计时这类表单弹窗经常要一边看别处
 * （桌面、浏览器、资源管理器）一边填内容，弹窗只能居中就等于挡住了视线。
 * Windows 自己的对话框也是可以拖走的，这里对齐同样的交互：按住卡片上任意
 * 非输入/非按钮的区域拖走即可，输入框、按钮、下拉、日历照常点。
 *
 * 实现要点：
 * - 事件挂 document（弹窗都 Teleport 到 body，组件内监听覆盖不到）；
 * - 位移用 CSS `translate`（和 `transform` 独立，不会打断 `card-in` 的缩放渐入动画）；
 * - 位移累积在 WeakMap 里：同一次打开期间多次拖动会接着上次的位置走，
 *   弹窗卸载后元素没了，WeakMap 自动回收，下次打开的弹窗从居中开始；
 * - 限制：卡片至少保留 80px 在视口内，拖不丢；
 * - 想整体禁用某个弹窗的拖动：给它的 `.modal-card` 加 `data-no-drag`。
 */

const INTERACTIVE =
  'input, textarea, select, button, a, label, summary, [contenteditable], [role="slider"], [role="combobox"], [role="switch"], [role="radio"], [role="checkbox"], [data-no-drag], .no-drag'

/** 每个弹窗卡片的累计位移（px） */
const offsets = new WeakMap<HTMLElement, { x: number; y: number }>()

export function useDialogDrag() {
  let dragging: {
    card: HTMLElement
    startX: number
    startY: number
    originX: number
    originY: number
    rect: DOMRect
  } | null = null

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0)
      return
    const target = e.target as HTMLElement | null
    if (!target) return
    const card = target.closest<HTMLElement>('.modal-card')
    if (!card || card.hasAttribute('data-no-drag')) return
    // 点在输入框/按钮等控件上：不抢，交给控件自己
    if (target.closest(INTERACTIVE)) return
    // 遮挡层自己的 mousedown（点击遮罩关闭）不会进到这里，卡片内部的才算

    const off = offsets.get(card) ?? { x: 0, y: 0 }
    dragging = {
      card,
      startX: e.clientX,
      startY: e.clientY,
      originX: off.x,
      originY: off.y,
      rect: card.getBoundingClientRect(),
    }
    card.classList.add('dialog-dragging')
    e.preventDefault()
    window.addEventListener('mousemove', onMouseMove)
    window.addEventListener('mouseup', endDrag)
    window.addEventListener('blur', endDrag)
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return
    // 兜底：鼠标在窗口外松开时 WebView 收不到 mouseup，靠 buttons 归零结束拖动，
    // 否则回到窗口里弹窗会「粘」在鼠标上
    if (e.buttons === 0) {
      endDrag()
      return
    }
    const { card, startX, startY, originX, originY, rect } = dragging
    const vw = window.innerWidth
    const vh = window.innerHeight
    const keep = 80

    let x = originX + (e.clientX - startX)
    let y = originY + (e.clientY - startY)

    // 至少留 80px 在视口里（卡片比视口大时也一样能拖）
    const minX = originX + (keep - rect.right)
    const maxX = originX + (vw - keep - rect.left)
    const minY = originY + (keep - rect.bottom)
    const maxY = originY + (vh - keep - rect.top)
    x = Math.min(Math.max(x, minX), maxX)
    y = Math.min(Math.max(y, minY), maxY)

    offsets.set(card, { x, y })
    card.style.translate = `${x}px ${y}px`
  }

  function endDrag() {
    if (!dragging) return
    dragging.card.classList.remove('dialog-dragging')
    dragging = null
    window.removeEventListener('mousemove', onMouseMove)
    window.removeEventListener('mouseup', endDrag)
    window.removeEventListener('blur', endDrag)
  }

  onMounted(() => {
    document.addEventListener('mousedown', onMouseDown)
  })

  onBeforeUnmount(() => {
    document.removeEventListener('mousedown', onMouseDown)
    endDrag()
  })
}
