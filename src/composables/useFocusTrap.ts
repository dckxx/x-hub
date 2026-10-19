import { onBeforeUnmount, watch, type Ref } from 'vue'

/**
 * 当前打开着的陷阱栈（后进先出）。弹窗可以叠加（市场详情 → 反馈提交、设置 → 确认框），
 * **只允许栈顶处理 Tab**：否则下层陷阱看到焦点在子弹窗里，会把它抢回自己的容器，
 * 两层互相顶、Tab 在两边来回跳。上层关闭后栈顶自然回落到下层，无需重新注册。
 */
const trapStack: symbol[] = []

/**
 * 弹窗焦点陷阱：打开时聚焦初始元素、Tab 循环在弹窗内、关闭时归还焦点。
 * 配合 aria-modal 使用，确保键盘用户不会 Tab 穿出遮罩到后台页面。
 */
export function useFocusTrap(
  active: Ref<boolean>,
  container: Ref<HTMLElement | null>,
  initialFocus?: Ref<HTMLElement | null>,
) {
  const trapId = Symbol('focus-trap')
  let lastFocused: HTMLElement | null = null

  const FOCUSABLE_SELECTOR = [
    'a[href]',
    'button:not([disabled])',
    'input:not([disabled])',
    'textarea:not([disabled])',
    'select:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
  ].join(', ')

  function trapTab(e: KeyboardEvent) {
    if (e.key !== 'Tab') return
    // 叠加弹窗时由最上层全权处理，下层让位
    if (trapStack[trapStack.length - 1] !== trapId) return
    const root = container.value
    if (!root) return
    const focusables = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
    if (focusables.length === 0) return
    const first = focusables[0]
    const last = focusables[focusables.length - 1]
    const activeEl = document.activeElement
    if (!root.contains(activeEl)) {
      e.preventDefault()
      first.focus()
      return
    }
    if (e.shiftKey && activeEl === first) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && activeEl === last) {
      e.preventDefault()
      first.focus()
    }
  }

  watch(
    active,
    (v) => {
      if (v) {
        lastFocused = document.activeElement as HTMLElement | null
        trapStack.push(trapId)
        // 等弹窗渲染完成后聚焦初始元素
        requestAnimationFrame(() => {
          const target =
            initialFocus?.value ??
            container.value?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR)
          target?.focus()
        })
        document.addEventListener('keydown', trapTab, true)
      } else {
        const wasTop = trapStack[trapStack.length - 1] === trapId
        const i = trapStack.lastIndexOf(trapId)
        if (i !== -1) trapStack.splice(i, 1)
        document.removeEventListener('keydown', trapTab, true)
        // 只有自己是最上层时才归还焦点：下层在子弹窗打开期间关闭，不该把焦点抢回来
        if (wasTop) lastFocused?.focus?.()
        lastFocused = null
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    const i = trapStack.lastIndexOf(trapId)
    if (i !== -1) trapStack.splice(i, 1)
    document.removeEventListener('keydown', trapTab, true)
  })
}
