import { ref } from 'vue'

/**
 * 子待办拖拽的共享瞬态（模块级单例：同一窗口内所有 TodoRow 共享）。
 *
 * 为什么需要共享：跨父拖拽时，「拖拽源行」要能把「落点信息」告诉「目标父行」——
 * 目标父行据此临时展开（空/折叠父行也能给出放置区、渲染插入线）。
 * 拖拽的指针逻辑仍在 TodoRow（源行）里，这里只放共享状态。
 *
 * 说明：待办浮窗是独立窗口（独立 JS 上下文），模块单例天然按窗口隔离。
 */
const subDragId = ref<number | null>(null)
/** 当前落点对应的目标父待办 id（拖到无有效父行时为 null） */
const dropTargetParentId = ref<number | null>(null)
/** 插入线在视口中的 y（fixed 定位，由拖拽源行绘制唯一一条） */
const dropLineClientY = ref<number | null>(null)
const dropLineLeft = ref(0)
const dropLineWidth = ref(0)
/** 升为顶级落点激活（子待办拖到列表空白处）：插入线转虚线 + 「松手升为顶级待办」提示 */
const promoteActive = ref(false)

export function useTodoSubDrag() {
  return { subDragId, dropTargetParentId, dropLineClientY, dropLineLeft, dropLineWidth, promoteActive }
}

export interface ChildDropHit {
  /** 命中的顶级行元素 */
  row: HTMLElement
  /** 该行的待办 id（= 潜在目标父 id） */
  parentId: number
  /** 插入下标（相对目标父下含被拖项的可见子数组；空/折叠父行 = 追加到末尾） */
  insert: number
  /** 插入线几何（视口坐标，fixed 定位） */
  edgeY: number
  left: number
  width: number
}

/**
 * 指针位置 → 子待办落点：取最靠近的顶级 `.todo-row`（命中子行时 closest 会越过它
 * 找到所属顶级行；拖到顶级行本身即落该父下），用其子列表算插入下标与插入线几何。
 * 未命中任何顶级行返回 null（由调用方决定：清空落点，或按「列表空白处 = 升为顶级」处理）。
 *
 * 子待办拖拽（TodoRow）与一级拖拽改挂（useTodoDrag）共用这一份命中/几何逻辑，
 * 两处落点表现（线位、空父展开后的重算）保持一致。
 */
export function resolveChildDrop(
  clientX: number,
  clientY: number,
  kidCountOf: (parentId: number) => number,
): ChildDropHit | null {
  const under = document.elementFromPoint(clientX, clientY) as HTMLElement | null
  const row = under?.closest<HTMLElement>('.todo-row:not(.sub)') ?? null
  const pidRaw = row?.dataset.todoId
  if (!row || !pidRaw) return null
  const parentId = Number(pidRaw)
  const subsEl = row.querySelector<HTMLElement>('.todo-subs')
  const rowRect = row.getBoundingClientRect()
  let insert: number
  let edgeY: number
  let left: number
  let width: number
  if (subsEl) {
    const rect = subsEl.getBoundingClientRect()
    const rows = Array.from(subsEl.querySelectorAll<HTMLElement>(':scope > .todo-row'))
    left = rect.left
    width = rect.width
    insert = rows.length
    edgeY = rect.bottom
    if (rows.length && clientY >= rect.top && clientY <= rect.bottom) {
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
  } else {
    // 空父行 / 折叠父行：共享态会让它临时展开，下一次 move 会重算；
    // 先按「追加到末尾」给落点（空父行即 0）
    insert = kidCountOf(parentId)
    edgeY = rowRect.bottom
    left = rowRect.left + 24
    width = Math.max(rowRect.width - 24, 40)
  }
  return { row, parentId, insert, edgeY, left, width }
}
