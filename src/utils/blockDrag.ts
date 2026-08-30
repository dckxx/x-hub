import { NodeSelection, TextSelection } from '@milkdown/kit/prose/state'
import type { EditorView } from '@milkdown/kit/prose/view'

/**
 * 速记编辑器块级拖拽（六点把手）的指针实现。
 *
 * plugin-block 自带的拖拽走 HTML5 DnD（dragstart 写入 dataTransfer → dragover/drop 完成搬移），
 * 与主窗口 dragDropEnabled 的原生文件拖放拦截互斥（见 AGENTS 约定 14）：Tauri 窗口内拖拽启动后
 * 收不到任何 dragover/drop，表现为「拖得动、落不下」。这里在捕获阶段拦掉把手上的原生 dragstart，
 * 改用 pointer 事件跟踪光标 + 单事务搬移顶层块，浏览器与 Tauri 行为一致，速达原生拖入不受影响。
 *
 * 依赖 Crepe 把手 DOM 结构：.milkdown-block-handle 内两个 .operation-item，
 * 第 1 个是加号（新增块）、第 2 个才是拖拽把手——升级 Crepe 需复核该顺序。
 */

const HANDLE_SELECTOR = '.milkdown-block-handle'

interface BlockDragState {
  view: EditorView
  /** 被拖顶层块的文档范围 [from, to) */
  from: number
  to: number
  /** 被拖块的 DOM，用于压暗反馈 */
  nodeEl: HTMLElement | null
  /** 最近一次光标 Y（滚动时补偿重算插入线） */
  lastY: number
  /** 当前插入边界；null = 不合法（悬停自身内部或编辑器外） */
  boundary: number | null
}

/** 共享插入线：挂在 body 上用 fixed 定位，避免被卡片 overflow 裁剪 */
let lineEl: HTMLDivElement | null = null

function ensureLine(): HTMLDivElement {
  if (!lineEl) {
    lineEl = document.createElement('div')
    lineEl.style.cssText = [
      'position:fixed',
      'z-index:90',
      'height:3px',
      'border-radius:2px',
      'background:var(--brand-500, #5b5bf5)',
      'box-shadow:0 0 6px color-mix(in srgb, var(--brand-500, #5b5bf5) 55%, transparent)',
      'pointer-events:none',
      'display:none',
    ].join(';')
    document.body.appendChild(lineEl)
  }
  return lineEl
}

/**
 * 依据 clientY 解析光标处的顶层块范围。
 * posAtCoords 的 inside 命中叶子/文本块后爬到 depth-1 祖先（与 Crepe 顶层块语义一致：
 * 整个列表/引用/代码块作为一个块搬移）；恰好落在两块间隙时取其后紧邻的块做半块判定。
 */
function topLevelRangeAt(
  view: EditorView,
  clientY: number,
): { from: number; to: number; el: HTMLElement | null } | null {
  const domRect = view.dom.getBoundingClientRect()
  if (clientY < domRect.top - 8 || clientY > domRect.bottom + 8) return null
  // 探针 x 取内容水平中心：pm 左缘附近处于内边距区，posAtCoords 会粗解析到文档开头
  const coords = view.posAtCoords({ left: domRect.left + domRect.width / 2, top: clientY })
  if (!coords) return null
  const doc = view.state.doc
  let pos = coords.pos
  if (typeof coords.inside === 'number' && coords.inside >= 0) pos = coords.inside
  let $pos
  try {
    $pos = doc.resolve(pos)
  } catch {
    return null
  }
  const from = $pos.depth >= 1 ? $pos.before(1) : $pos.pos
  const node = doc.nodeAt(from)
  if (!node) return null
  const el = view.nodeDOM(from)
  return { from, to: from + node.nodeSize, el: el instanceof HTMLElement ? el : null }
}

/** 插入边界：块上半 → 块前，下半 → 块后；悬停被拖块内部时返回 null（不可落） */
function boundaryAt(
  view: EditorView,
  clientY: number,
  from: number,
  to: number,
): number | null {
  const range = topLevelRangeAt(view, clientY)
  if (!range) return null
  let boundary: number
  if (range.el) {
    const r = range.el.getBoundingClientRect()
    boundary = clientY < r.top + r.height / 2 ? range.from : range.to
  } else {
    boundary = range.from
  }
  if (boundary > from && boundary < to) return null
  return boundary
}

function updateLine(view: EditorView, boundary: number | null) {
  const line = ensureLine()
  if (boundary == null) {
    line.style.display = 'none'
    return
  }
  let rect: { top: number; bottom: number }
  try {
    rect = view.coordsAtPos(boundary)
  } catch {
    line.style.display = 'none'
    return
  }
  const pmRect = view.dom.getBoundingClientRect()
  line.style.display = 'block'
  line.style.top = `${(rect.top + rect.bottom) / 2 - 1.5}px`
  line.style.left = `${pmRect.left + 4}px`
  line.style.width = `${Math.max(0, pmRect.width - 8)}px`
}

/** 单事务完成搬移（一个撤销步骤），移动后光标落在被移动块上 */
function performMove(view: EditorView, from: number, to: number, boundary: number) {
  const node = view.state.doc.nodeAt(from)
  if (!node || from + node.nodeSize !== to) return
  if (boundary === from || boundary === to) return
  const tr = view.state.tr
  tr.delete(from, to)
  const at = boundary > to ? boundary - (to - from) : boundary
  tr.insert(at, node)
  const inserted = tr.doc.nodeAt(at)
  if (inserted && (inserted.isAtom || inserted.isLeaf)) {
    tr.setSelection(NodeSelection.create(tr.doc, at))
  } else {
    tr.setSelection(TextSelection.near(tr.doc.resolve(at)))
  }
  view.dispatch(tr.scrollIntoView())
}

export function attachBlockDrag(getView: () => EditorView | null): () => void {
  let drag: BlockDragState | null = null

  function endDrag() {
    if (drag?.nodeEl) drag.nodeEl.style.opacity = ''
    drag = null
    if (lineEl) lineEl.style.display = 'none'
  }

  /** 容器内第 1 个 operation-item 是加号、第 2 个才是把手（与 Crepe 渲染顺序绑定） */
  function findGrip(target: EventTarget | null): Element | null {
    if (!(target instanceof Element)) return null
    const container = target.closest(HANDLE_SELECTOR)
    if (!container) return null
    const item = target.closest('.operation-item')
    if (!item || !container.contains(item)) return null
    const items = container.querySelectorAll('.operation-item')
    return items.length > 1 && items[1] === item ? items[1] : null
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || drag) return
    const grip = findGrip(e.target)
    if (!grip) return
    const view = getView()
    if (!view || !view.editable || view.composing) return
    const container = grip.closest(HANDLE_SELECTOR)
    if (!container) return
    const rect = container.getBoundingClientRect()
    const range = topLevelRangeAt(view, rect.top + rect.height / 2)
    if (!range || range.to <= range.from) return
    if (!view.state.doc.nodeAt(range.from)) return
    drag = {
      view,
      from: range.from,
      to: range.to,
      nodeEl: range.el,
      lastY: e.clientY,
      boundary: null,
    }
    if (drag.nodeEl) drag.nodeEl.style.opacity = '0.35'
    // 捕获指针：快速拖动/拖出窗口后仍能收到 move/up，避免拖拽状态卡死
    try {
      grip.setPointerCapture(e.pointerId)
    } catch {
      /* 捕获失败仅影响窗口外松手，不影响窗口内拖拽 */
    }
  }

  function onMouseDown(e: MouseEvent) {
    // 拖拽中抑制默认行为（文本选区）；把手容器的 mousedown 监听（plugin-block 选中块）不受影响
    if (drag) e.preventDefault()
  }

  function onPointerMove(e: PointerEvent) {
    if (!drag) return
    drag.lastY = e.clientY
    drag.boundary = boundaryAt(drag.view, e.clientY, drag.from, drag.to)
    updateLine(drag.view, drag.boundary)
  }

  function onPointerUp() {
    if (!drag) return
    const d = drag
    endDrag()
    if (d.boundary != null) performMove(d.view, d.from, d.to, d.boundary)
  }

  function onKeyDown(e: KeyboardEvent) {
    if (drag && e.key === 'Escape') endDrag()
  }

  function onScroll() {
    if (!drag) return
    drag.boundary = boundaryAt(drag.view, drag.lastY, drag.from, drag.to)
    updateLine(drag.view, drag.boundary)
  }

  // 原生 HTML5 拖拽在 Tauri 里必然「拖得动落不下」，且会抢占鼠标让 pointer 跟踪失效，直接拦掉
  function onDragStart(e: DragEvent) {
    const target = e.target
    if (target instanceof Element && target.closest(HANDLE_SELECTOR)) {
      e.preventDefault()
      e.stopPropagation()
    }
  }

  const capture = { capture: true }
  window.addEventListener('pointerdown', onPointerDown, capture)
  window.addEventListener('mousedown', onMouseDown, capture)
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('pointercancel', endDrag)
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('blur', endDrag)
  window.addEventListener('scroll', onScroll, capture)
  window.addEventListener('dragstart', onDragStart, capture)

  return () => {
    window.removeEventListener('pointerdown', onPointerDown, capture)
    window.removeEventListener('mousedown', onMouseDown, capture)
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUp)
    window.removeEventListener('pointercancel', endDrag)
    window.removeEventListener('keydown', onKeyDown)
    window.removeEventListener('blur', endDrag)
    window.removeEventListener('scroll', onScroll, capture)
    window.removeEventListener('dragstart', onDragStart, capture)
    endDrag()
  }
}
