import { onBeforeUnmount, watch, type Ref } from 'vue'

/**
 * 弹窗焦点陷阱：打开时聚焦初始元素、Tab 循环在弹窗内、关闭时归还焦点。
 * 配合 aria-modal 使用，确保键盘用户不会 Tab 穿出遮罩到后台页面。
 */
export function useFocusTrap(
  active: Ref<boolean>,
  container: Ref<HTMLElement | null>,
  initialFocus?: Ref<HTMLElement | null>,
) {
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
        // 等弹窗渲染完成后聚焦初始元素
        requestAnimationFrame(() => {
          const target =
            initialFocus?.value ??
            container.value?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR)
          target?.focus()
        })
        document.addEventListener('keydown', trapTab, true)
      } else {
        document.removeEventListener('keydown', trapTab, true)
        lastFocused?.focus?.()
        lastFocused = null
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    document.removeEventListener('keydown', trapTab, true)
  })
}
