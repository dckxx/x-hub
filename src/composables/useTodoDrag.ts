import { ref, type ComputedRef, type Ref } from 'vue'
import type { Todo } from '../api/tauri'

/**
 * 待办组内上下拖动排序（指针实现，卡片与待办视图共用）。
 *
 * 为什么不用 HTML5 DnD：Tauri 主窗口的原生拖放拦截（`dragDropEnabled`）与 WebView 内
 * HTML5 拖拽互斥——`dragstart` 之后收不到 `dragover`/`drop`，表现为「拖得动、落不下」
 * （同笔记块拖拽的处理，见 AGENTS.md 约定 14/38）。
 *
 * 语义：仅限**同一分组内**上下移动（分组由截止日期决定，跨组移动没有排序意义）；
 * 松开后把整组 id 顺序写入 `sort_order`（组内原本没手动排过序时保持创建时间倒序不动它）。
 *
 * 调用方需要提供：
 * - `bodyRef`：滚动容器（插入指示线的坐标基准，也是 `data-group` 的查找范围）
 * - `groups`：当前展示的分组（含组内可见顺序）——拖拽只在其中移动
 * - `labelOf`：某条待办属于哪个组（各宿主分组口径不同：卡片 5 组、待办视图 7 组）
 * - `enabled`：是否允许拖拽（卡片只在「未完成」视图开放）
 * - `reorder`：落点后的持久化（一般写 `store.reorderTodos(ids)`）
 *
 * 宿主模板约定：分组容器带 `:data-group="组标签"`，行元素 class 为 `.todo-row`
 * （且为分组容器的直接子元素），另需一个用 `lineTop` 定位的插入指示线元素。
 */
export function useTodoDrag(opts: {
  bodyRef: Ref<HTMLElement | null>
  groups: ComputedRef<Array<{ label: string; items: Todo[] }>>
  labelOf: (t: Todo) => string
  enabled: () => boolean
  reorder: (ids: number[]) => void
}) {
  const dragId = ref<number | null>(null)
  /** 插入指示线：相对滚动容器内容的 y 坐标；null = 不显示（拖出组外） */
  const lineTop = ref<number | null>(null)

  let dragState: {
    id: number
    /** 被拖项在组内可见行中的下标 */
    fromIndex: number
    groupLabel: string
    groupEl: HTMLElement
    /** 插入下标（相对含被拖项的可见数组）；null = 拖出组外 */
    insert: number | null
  } | null = null

  /** TodoRow 顶级行 pointerdown 上报入口；移动超阈值才进入拖拽 */
  function onRowPointerDown(t: Todo, e: PointerEvent) {
    if (e.button !== 0 || !opts.enabled()) return
    const target = e.target as HTMLElement | null
    // 勾选/优先级/徽标/删除等交互控件上按下不启动拖拽
    if (target?.closest('button, textarea, input, a, [data-no-drag]')) return
    const el = e.currentTarget as HTMLElement
    const startX = e.clientX
    const startY = e.clientY
    let active = false
    let dead = false
    // 不在 pointerdown 就捕获指针：捕获会把后续 click/dblclick 重定向到行元素，
    // 行内标题的双击编辑收不到事件。改为拖拽激活（超阈值）后再捕获。
    el.addEventListener('pointermove', onMove)
    el.addEventListener('pointerup', onUp)
    el.addEventListener('pointercancel', onUp)
    // window 兜底：激活前未捕获指针，快速甩动时第一个 pointermove 可能已在行外、
    // up 也不落在行上——仅靠 el 监听会永久残留（闭包泄漏，且残留的旧坐标 onMove
    // 会在该行下次按下移动时误触发拖拽）。window 监听保证任何松开路径都能清理。
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)

    function begin(): boolean {
      // 捕获指针：拖出窗口松开也能收到 pointerup，不会悬挂在拖拽态
      try {
        el.setPointerCapture(e.pointerId)
      } catch {
        /* 指针已释放时忽略，window 监听兜底场景极少 */
      }
      const label = opts.labelOf(t)
      const groupEl =
        opts.bodyRef.value?.querySelector<HTMLElement>(`[data-group="${CSS.escape(label)}"]`) ??
        null
      if (!groupEl) return false
      const g = opts.groups.value.find((x) => x.label === label)
      const fromIndex = g ? g.items.findIndex((x) => x.id === t.id) : -1
      if (fromIndex < 0) return false
      dragState = { id: t.id, fromIndex, groupLabel: label, groupEl, insert: null }
      dragId.value = t.id
      document.body.classList.add('todo-row-dragging')
      document.getSelection()?.removeAllRanges()
      return true
    }

    function onMove(ev: PointerEvent) {
      if (dead) return
      if (!active) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 5) return
        if (!begin()) {
          dead = true
          return
        }
        active = true
      }
      updateDragLine(ev.clientY)
    }

    function onUp() {
      el.removeEventListener('pointermove', onMove)
      el.removeEventListener('pointerup', onUp)
      el.removeEventListener('pointercancel', onUp)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
      // 捕获成功时同一 pointerup 会先到 el（目标阶段）、再冒泡到 window，可能触发两次；
      // 归零 active 保证 finishDrag 只执行一次
      if (!active) return
      active = false
      finishDrag()
    }
  }

  /** 指针所在位置 → 插入指示线 y（容器内容坐标）+ 记录插入下标 */
  function updateDragLine(clientY: number) {
    const ds = dragState
    const body = opts.bodyRef.value
    if (!ds || !body) return
    const groupRect = ds.groupEl.getBoundingClientRect()
    const rows = Array.from(ds.groupEl.querySelectorAll<HTMLElement>(':scope > .todo-row'))
    let insert: number | null = null
    let edgeY: number | null = null
    if (clientY >= groupRect.top && clientY <= groupRect.bottom && rows.length) {
      edgeY = groupRect.bottom
      insert = rows.length
      for (let i = 0; i < rows.length; i++) {
        const r = rows[i].getBoundingClientRect()
        if (clientY < r.top + r.height / 2) {
          edgeY = r.top
          insert = i
          break
        }
        edgeY = r.bottom
        insert = i + 1
      }
    }
    ds.insert = insert
    lineTop.value =
      edgeY == null ? null : edgeY - body.getBoundingClientRect().top + body.scrollTop
  }

  /** 松开落点：换算目标顺序，整组写回 sort_order */
  function finishDrag() {
    const ds = dragState
    document.body.classList.remove('todo-row-dragging')
    dragId.value = null
    dragState = null
    lineTop.value = null
    if (!ds || ds.insert == null || ds.fromIndex < 0) return
    const g = opts.groups.value.find((x) => x.label === ds.groupLabel)
    if (!g) return
    const ids = g.items.map((x) => x.id)
    // 插入下标相对「含被拖项」的数组；先移除再插入需换算
    const final = ds.insert > ds.fromIndex ? ds.insert - 1 : ds.insert
    if (final === ds.fromIndex) return
    const without = ids.filter((_, i) => i !== ds.fromIndex)
    without.splice(final, 0, ds.id)
    opts.reorder(without)
  }

  return { dragId, lineTop, onRowPointerDown }
}
