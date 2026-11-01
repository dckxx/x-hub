import { ref, type ComputedRef, type Ref } from 'vue'
import type { Todo } from '../api/tauri'
import { resolveChildDrop, useTodoSubDrag } from './useTodoSubDrag'

/**
 * 待办组内上下拖动排序 + 一级改挂为子待办（指针实现，卡片与待办视图共用）。
 *
 * 为什么不用 HTML5 DnD：Tauri 主窗口的原生拖放拦截（`dragDropEnabled`）与 WebView 内
 * HTML5 拖拽互斥——`dragstart` 之后收不到 `dragover`/`drop`，表现为「拖得动、落不下」
 * （同笔记块拖拽的处理，见 AGENTS.md 约定 14/38）。
 *
 * 语义：
 * - 排序：仅限**同一分组内**上下移动（分组由截止日期决定，跨组移动没有排序意义）；
 *   松开后把整组 id 顺序写入 `sort_order`（组内原本没手动排过序时保持创建时间倒序不动它）。
 * - 改挂（可选启用）：拖到另一条一级的**标题行中部** = 变为它的子待办（跨组也可以），
 *   标题行上/下缘 = 原有前后插入排序，展开的子待办区域 = 插到其子项之间——
 *   落点命中/线几何与子待办跨父拖拽共用同一份 `resolveChildDrop`。
 *   源自身有子待办时（nestable=false）完全不启用改挂：保持纯排序行为零变化，
 *   否则会造出 UI 不可见的孙级（子待办仅一层）。
 *
 * 调用方需要提供：
 * - `bodyRef`：滚动容器（插入指示线的坐标基准，也是 `data-group` 的查找范围）
 * - `groups`：当前展示的分组（含组内可见顺序）——拖拽只在其中移动
 * - `labelOf`：某条待办属于哪个组（各宿主分组口径不同：卡片 5 组、待办视图 7 组）
 * - `enabled`：是否允许拖拽（卡片只在「未完成」视图开放）
 * - `reorder`：落点后的持久化（一般写 `store.reorderTodos(ids)`）
 * - `childIdsOf` / `nestable` / `moveChild`：三者齐备才启用改挂（见上）
 *
 * 宿主模板约定：分组容器带 `:data-group="组标签"`，行元素 class 为 `.todo-row`
 * （且为分组容器的直接子元素），另需一个用 `lineTop` 定位的插入指示线元素。
 * 改挂落点的行高亮 / 空 / 折叠父行临时展开 / 插入线复用子待办拖拽那套共享状态
 * （useTodoSubDrag），由拖拽源行（TodoRow 的 isTopDragSource）绘制。
 */
export function useTodoDrag(opts: {
  bodyRef: Ref<HTMLElement | null>
  groups: ComputedRef<Array<{ label: string; items: Todo[] }>>
  labelOf: (t: Todo) => string
  enabled: () => boolean
  reorder: (ids: number[]) => void
  /** 改挂：目标父下子项 id（compareChildOrder 口径，与 childrenMap 同源） */
  childIdsOf?: (parentId: number) => number[]
  /** 改挂：该一级待办是否可变为他级的子待办（有子待办 = false） */
  nestable?: (id: number) => boolean
  /** 改挂：落库（含成功/失败 toast 与撤销，由宿主编排） */
  moveChild?: (id: number, newParentId: number, orderedIds: number[]) => void
}) {
  const dragId = ref<number | null>(null)
  /** 插入指示线：相对滚动容器内容的 y 坐标；null = 不显示（拖出组外） */
  const lineTop = ref<number | null>(null)
  // 改挂落点复用子待办拖拽的共享态（行高亮 / 临时展开 / 固定插入线）。
  // 升为顶级（promoteActive）是一级 → 二级的反向手势，只属于子待办拖拽，这里不碰。
  const {
    subDragId: sharedSubDragId,
    dropTargetParentId: sharedDropTargetParentId,
    dropLineClientY: sharedDropLineClientY,
    dropLineLeft: sharedDropLineLeft,
    dropLineWidth: sharedDropLineWidth,
  } = useTodoSubDrag()

  let dragState: {
    id: number
    /** 被拖项在组内可见行中的下标 */
    fromIndex: number
    groupLabel: string
    groupEl: HTMLElement
    /** 插入下标（相对含被拖项的可见数组）；null = 拖出组外 / 改挂落点激活 */
    insert: number | null
    /** 改挂落点：目标父 id（与 insert 互斥；null = 非 改挂 落点） */
    childTargetId: number | null
    /** 改挂落点：目标父下子列表的插入下标 */
    childInsert: number | null
  } | null = null

  const canNest = (id: number) =>
    !!(opts.childIdsOf && opts.nestable && opts.moveChild && opts.nestable(id))

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
      dragState = {
        id: t.id,
        fromIndex,
        groupLabel: label,
        groupEl,
        insert: null,
        childTargetId: null,
        childInsert: null,
      }
      dragId.value = t.id
      // 改挂落点的行高亮（isSubDropTarget）以 sharedSubDragId 非空为门控，置上被拖 id
      sharedSubDragId.value = t.id
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
      updateDragLine(ev.clientX, ev.clientY)
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

  /** 清掉改挂共享态（回到纯排序落点 / 拖拽结束） */
  function clearChildDrop() {
    sharedDropTargetParentId.value = null
    sharedDropLineClientY.value = null
  }

  /**
   * 指针位置 → 落点。
   * 改挂启用时先做全列表命中（resolveChildDrop）：命中其它顶级行按标题带 25/50/25
   * 分流——上缘 = 前插、下缘 = 后插（仅同组），中部与展开子列表区 = 改挂为该行的
   * 子待办；未命中行 / 未启用改挂 / 命中被拖行自身 → 原组内扫描（行为零变化）。
   */
  function updateDragLine(clientX: number, clientY: number) {
    const ds = dragState
    const body = opts.bodyRef.value
    if (!ds || !body) return
    if (canNest(ds.id)) {
      const hit = resolveChildDrop(clientX, clientY, (pid) => opts.childIdsOf!(pid).length)
      // 命中被拖行自身 = 排序语义（落入下方组内扫描，前后插自己即无操作）
      if (hit && hit.parentId !== ds.id) {
        const rowRect = hit.row.getBoundingClientRect()
        const subsEl = hit.row.querySelector<HTMLElement>('.todo-subs')
        // 「标题带」= 行顶到子列表区上缘（无/折叠子列表时即整行）：折叠父行悬停中部
        // 会经共享态临时展开，下一次 move 以展开后的几何重算
        const headerBottom = subsEl ? subsEl.getBoundingClientRect().top : rowRect.bottom
        const headerH = Math.max(headerBottom - rowRect.top, 1)
        const childDrop = () => {
          ds.insert = null
          ds.childTargetId = hit.parentId
          ds.childInsert = hit.insert
          lineTop.value = null
          sharedDropTargetParentId.value = hit.parentId
          sharedDropLineClientY.value = hit.edgeY
          sharedDropLineLeft.value = hit.left
          sharedDropLineWidth.value = hit.width
        }
        if (clientY < rowRect.top + headerH * 0.25) {
          // 标题带上缘：插到它前面（跨组没有排序语义，不出线）
          ds.childTargetId = null
          ds.childInsert = null
          clearChildDrop()
          if (hit.row.closest<HTMLElement>('[data-group]')?.dataset.group === ds.groupLabel) {
            setSiblingDrop(ds, hit.row, 'before', body, rowRect.top)
          } else {
            ds.insert = null
            lineTop.value = null
          }
          return
        }
        if (clientY >= headerBottom - headerH * 0.25 && clientY <= headerBottom) {
          // 标题带下缘：插到它后面（线在整棵子树底部，与旧行为同款）
          ds.childTargetId = null
          ds.childInsert = null
          clearChildDrop()
          if (hit.row.closest<HTMLElement>('[data-group]')?.dataset.group === ds.groupLabel) {
            setSiblingDrop(ds, hit.row, 'after', body, rowRect.bottom)
          } else {
            ds.insert = null
            lineTop.value = null
          }
          return
        }
        // 标题带中部（含展开的子列表区域）：改挂为该行的子待办
        childDrop()
        return
      }
      clearChildDrop()
      ds.childTargetId = null
      ds.childInsert = null
    }
    updateSiblingLine(ds, clientY, body)
  }

  /** 同组内插入落点：把指示线放到命中行前/后（edgeY 由调用方按语义给） */
  function setSiblingDrop(
    ds: NonNullable<typeof dragState>,
    hitRow: HTMLElement,
    at: 'before' | 'after',
    body: HTMLElement,
    edgeY: number,
  ) {
    const rows = Array.from(ds.groupEl.querySelectorAll<HTMLElement>(':scope > .todo-row'))
    const idx = rows.indexOf(hitRow)
    if (idx < 0) {
      ds.insert = null
      lineTop.value = null
      return
    }
    ds.insert = at === 'before' ? idx : idx + 1
    lineTop.value = edgeY - body.getBoundingClientRect().top + body.scrollTop
  }

  /** 原组内扫描（改挂之前的既有行为）：按行矩形中点算插入下标，出组即无落点 */
  function updateSiblingLine(
    ds: NonNullable<typeof dragState>,
    clientY: number,
    body: HTMLElement,
  ) {
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

  /** 松开落点：改挂 = 目标父下整列重写；排序 = 换算目标顺序，整组写回 sort_order */
  function finishDrag() {
    const ds = dragState
    document.body.classList.remove('todo-row-dragging')
    dragId.value = null
    dragState = null
    lineTop.value = null
    clearChildDrop()
    sharedSubDragId.value = null
    if (!ds) return
    // 改挂落点：orderedIds = 目标父下现有子项 + 被拖项插入到落点（拖拽项不在该数组里，无需换算）
    if (
      ds.childTargetId != null &&
      ds.childInsert != null &&
      opts.childIdsOf &&
      opts.moveChild
    ) {
      const ids = opts.childIdsOf(ds.childTargetId).slice()
      ids.splice(Math.min(ds.childInsert, ids.length), 0, ds.id)
      opts.moveChild(ds.id, ds.childTargetId, ids)
      return
    }
    if (ds.insert == null || ds.fromIndex < 0) return
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
