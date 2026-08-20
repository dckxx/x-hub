<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { Check, GripVertical, LayoutGrid, X } from 'lucide-vue-next'
import {
  DASH_COLS,
  MIN_SIZE,
  dashModuleDef,
  dashModuleTitle,
  findFreeSpot,
  useDashboardLayout,
  type DashPlacement,
} from '../composables/useDashboardLayout'

const emit = defineEmits<{ (e: 'done'): void }>()

const layout = useDashboardLayout()

// 预览网格：12 列 × 15 行棋盘，行高 1fr 均分画布高度（编辑器即等比缩略图，窗口缩放自适应）
const GAP = 8
const MIN_ROWS = 15

const canvasRef = ref<HTMLElement | null>(null)
const gridRef = ref<HTMLElement | null>(null)

// 拖拽改用 Pointer 事件实现（HTML5 DnD 在 Tauri WebView2 下不稳定，会出现禁用图标 / 不落盘）
interface DragState {
  mode: 'move' | 'resize'
  id: string
  source: 'library' | 'canvas'
  w: number
  h: number
  col: number
  row: number
  // 起始快照：移动/缩放被碰撞拒绝时回退用
  origX: number
  origY: number
  origW: number
  origH: number
  // 拖动阈值：pointerdown 后位移超过阈值才真正开始，点击（无位移）不触发移动
  startX: number
  startY: number
  started: boolean
}

const drag = ref<DragState | null>(null)
const ghostPos = ref({ x: 0, y: 0 })
const isOver = ref(false)
// 落位预览虚框：库区拖入 / 画布内移动时显示「松手后将落到的实际位置」（目标被占自动向下找空位）
const preview = ref<{ x: number; y: number; w: number; h: number } | null>(null)
// 正在被拖动（移动）的卡片 id，用于半透明高亮
const draggingId = ref<string | null>(null)

const rowCount = computed(() => {
  let m = MIN_ROWS
  for (const p of layout.placements.value) {
    m = Math.max(m, p.y + p.h)
  }
  if (preview.value) m = Math.max(m, preview.value.y + preview.value.h)
  return m
})

function cellStyle(p: DashPlacement) {
  return {
    gridColumn: `${p.x + 1} / span ${p.w}`,
    gridRow: `${p.y + 1} / span ${p.h}`,
  }
}

function cellFromPoint(clientX: number, clientY: number) {
  const el = gridRef.value
  if (!el) return null
  const rect = el.getBoundingClientRect()
  const px = clientX - rect.left
  const py = clientY - rect.top
  const cellW = (rect.width - (DASH_COLS - 1) * GAP) / DASH_COLS
  const col = Math.min(Math.max(Math.floor(px / (cellW + GAP)), 0), DASH_COLS - 1)
  // 行高 = 网格实际高度均分到 rowCount 行（1fr 自适应），直接反推行号；
  // 拖到内容下方空白处时把行号钳制到 rowCount，落在现有内容最底行之后、紧贴向下扩展
  const rows = rowCount.value
  const cellH = (rect.height - (rows - 1) * GAP) / rows
  const row = Math.min(Math.max(Math.floor(py / (cellH + GAP)), 0), rows)
  return { col, row }
}

function isInsideCanvas(clientX: number, clientY: number) {
  const el = canvasRef.value
  if (!el) return false
  const rect = el.getBoundingClientRect()
  return (
    clientX >= rect.left && clientX <= rect.right && clientY >= rect.top && clientY <= rect.bottom
  )
}

function startMove(id: string, source: 'library' | 'canvas', e: PointerEvent) {
  const def = dashModuleDef(id)
  if (!def) return
  e.preventDefault()
  const p = source === 'canvas' ? layout.placements.value.find((q) => q.id === id) : undefined
  drag.value = {
    mode: 'move',
    id,
    source,
    // 画布内移动沿用模块当前实际尺寸（用户可能已缩放过），库区拖入用目录默认尺寸
    w: p?.w ?? def.w,
    h: p?.h ?? def.h,
    col: 0,
    row: 0,
    origX: p?.x ?? 0,
    origY: p?.y ?? 0,
    origW: p?.w ?? def.w,
    origH: p?.h ?? def.h,
    startX: e.clientX,
    startY: e.clientY,
    started: false,
  }
  if (source === 'canvas') draggingId.value = id
  ghostPos.value = { x: e.clientX, y: e.clientY }
}

function startResize(id: string, e: PointerEvent) {
  const p = layout.placements.value.find((q) => q.id === id)
  if (!p) return
  e.preventDefault()
  e.stopPropagation()
  drag.value = {
    mode: 'resize',
    id,
    source: 'canvas',
    w: p.w,
    h: p.h,
    col: p.x,
    row: p.y,
    origX: p.x,
    origY: p.y,
    origW: p.w,
    origH: p.h,
    startX: e.clientX,
    startY: e.clientY,
    started: false,
  }
  ghostPos.value = { x: e.clientX, y: e.clientY }
}

function onPointerMove(e: PointerEvent) {
  if (!drag.value) return
  const d = drag.value
  ghostPos.value = { x: e.clientX, y: e.clientY }
  // 拖动阈值：位移不足 6px 视为点击，不进入拖动
  if (!d.started) {
    if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < 6) return
    d.started = true
  }
  const inside = isInsideCanvas(e.clientX, e.clientY)
  isOver.value = inside
  if (!inside) {
    preview.value = null
    return
  }
  const cell = cellFromPoint(e.clientX, e.clientY)
  if (!cell) return
  if (d.mode === 'resize') {
    preview.value = null
    const p = layout.placements.value.find((q) => q.id === d.id)
    if (!p) return
    const nw = Math.min(Math.max(cell.col - p.x + 1, MIN_SIZE), DASH_COLS - p.x)
    const nh = Math.max(cell.row - p.y + 1, MIN_SIZE)
    d.w = nw
    d.h = nh
    // 实时反馈：拖拽过程中同步改模块尺寸，让画布即时看到大小变化
    p.w = nw
    p.h = nh
  } else {
    const col = Math.min(Math.max(cell.col, 0), DASH_COLS - d.w)
    d.col = col
    d.row = cell.row
    if (d.source === 'canvas') {
      // 画布内移动：不实时改卡片位置，只显示虚框占位（实际落位，目标被占自动向下找空位），
      // 松手后才吸入；被拖动卡片保持原位并半透明
      const others = layout.placements.value.filter((q) => q.id !== d.id)
      const spot = findFreeSpot(others, d.w, d.h, col, cell.row)
      preview.value = { x: spot.x, y: spot.y, w: d.w, h: d.h }
    } else {
      // 库区拖入：实时预览实际落位（目标被占时自动向下找空位）
      const spot = findFreeSpot(layout.placements.value, d.w, d.h, col, cell.row)
      preview.value = { x: spot.x, y: spot.y, w: d.w, h: d.h }
    }
  }
}

function onPointerUp(e: PointerEvent) {
  if (!drag.value) return
  const d = drag.value
  drag.value = null
  isOver.value = false
  preview.value = null
  draggingId.value = null

  // 纯点击（未超过拖动阈值）：不移动、不落位、不缩放
  if (!d.started) return

  if (d.mode === 'resize') {
    const ok = layout.resizeModule(d.id, d.w, d.h)
    if (!ok) {
      // 碰撞被拒 → 回退到起始尺寸
      const p = layout.placements.value.find((q) => q.id === d.id)
      if (p) {
        p.w = d.origW
        p.h = d.origH
      }
    }
    return
  }

  if (!isInsideCanvas(e.clientX, e.clientY)) {
    // 拖回库区 = 从画布移除
    if (d.source === 'canvas') layout.removeModule(d.id)
    return
  }
  const cell = cellFromPoint(e.clientX, e.clientY)
  if (!cell) return
  const col = Math.min(cell.col, DASH_COLS - d.w)
  if (d.source === 'library') {
    layout.addModule(d.id, col, cell.row)
  } else {
    // 移动：目标被占自动向下找空位（moveModule 内部处理），不弹回
    layout.moveModule(d.id, col, cell.row)
  }
}

function remove(id: string) {
  layout.removeModule(id)
}

// 是否已点「确认」：onUnmounted 据此判断提交还是回滚
const committed = ref(false)

function confirmDone() {
  committed.value = true
  layout.commitEdit()
  emit('done')
}

onMounted(() => {
  // 进入编辑器即快照当前布局，编辑期间的变更只进草稿、不落盘
  layout.beginEdit()
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
})
onUnmounted(() => {
  // 未点「确认」就切走：丢弃草稿、恢复编辑前的布局
  if (!committed.value) layout.cancelEdit()
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
})
</script>

<template>
  <div class="le-root">
    <header class="le-header">
      <div class="le-header-left">
        <h2 class="le-title">自定义布局</h2>
        <span class="le-hint">推荐布局 12×15 · 拖入模块 · 拖动换位 · 右下角缩放（最小 2×2）· 点 × 移除</span>
      </div>
      <div class="le-header-right">
        <button class="ghost-btn" type="button" @click="layout.clear()">清空</button>
        <button class="ghost-btn" type="button" @click="layout.applyPreset()">推荐布局</button>
        <button class="pill-btn" type="button" @click="confirmDone">
          <Check :size="14" :stroke-width="2.5" aria-hidden="true" />
          确认
        </button>
      </div>
    </header>

    <div class="le-body">
      <!-- 左侧：模块库 -->
      <aside class="le-library" aria-label="模块库">
        <p class="le-lib-title">模块库</p>
        <div
          v-for="m in layout.available.value"
          :key="m.id"
          class="le-lib-item"
          @pointerdown="startMove(m.id, 'library', $event)"
        >
          <GripVertical :size="14" :stroke-width="2" aria-hidden="true" />
          <span class="le-lib-name">{{ m.title }}</span>
          <span class="le-lib-size">{{ m.w }}×{{ m.h }}</span>
        </div>
        <p v-if="layout.available.value.length === 0" class="le-lib-empty">
          所有模块都已放置
        </p>
      </aside>

      <!-- 右侧：主界面等比缩放预览 -->
      <div
        ref="canvasRef"
        class="le-canvas"
        :class="{ over: isOver, empty: layout.placements.value.length === 0 }"
      >
        <div
          ref="gridRef"
          class="le-grid"
          :style="{ gridTemplateRows: `repeat(${rowCount}, minmax(0, 1fr))` }"
        >
          <div
            v-for="p in layout.placements.value"
            :key="p.id"
            class="le-cell"
            :class="{ dragging: draggingId === p.id }"
            :style="cellStyle(p)"
            @pointerdown="startMove(p.id, 'canvas', $event)"
          >
            <GripVertical :size="14" :stroke-width="2" class="le-cell-grip" aria-hidden="true" />
            <span class="le-cell-name">{{ dashModuleTitle(p.id) }}</span>
            <span class="le-cell-size">{{ p.w }}×{{ p.h }}</span>
            <button
              class="le-cell-remove"
              type="button"
              :aria-label="`移除${dashModuleTitle(p.id)}`"
              @pointerdown.stop
              @click="remove(p.id)"
            >
              <X :size="13" :stroke-width="2" aria-hidden="true" />
            </button>
            <span
              class="le-cell-resize"
              :aria-label="`调整${dashModuleTitle(p.id)}尺寸`"
              @pointerdown.stop="startResize(p.id, $event)"
            ></span>
          </div>
          <div
            v-if="preview"
            class="le-preview"
            :style="{ gridColumn: `${preview.x + 1} / span ${preview.w}`, gridRow: `${preview.y + 1} / span ${preview.h}` }"
          ></div>
        </div>
        <p v-if="layout.placements.value.length === 0" class="le-empty-hint">
          <LayoutGrid :size="16" :stroke-width="2" aria-hidden="true" />
          从左侧拖入模块开始搭建
        </p>
      </div>
    </div>

    <!-- 拖拽/缩放跟随浮层 -->
    <Teleport to="body">
      <div
        v-if="drag"
        class="le-ghost"
        :style="{ left: ghostPos.x + 'px', top: ghostPos.y + 'px' }"
      >
        {{ dashModuleTitle(drag.id) }}
        <span class="le-ghost-size">{{ drag.w }}×{{ drag.h }}</span>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.le-root {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  overflow: hidden;
}
.le-header {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.le-header-left {
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
}
.le-title {
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-1);
  flex-shrink: 0;
}
.le-hint {
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.le-header-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.le-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--space-4);
  overflow: hidden;
}

/* 左侧模块库 */
.le-library {
  flex-shrink: 0;
  width: 220px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: var(--space-4);
  overflow-y: auto;
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
}
.le-lib-title {
  margin: 0 0 4px;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.le-lib-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-solid);
  color: var(--text-2);
  cursor: grab;
  user-select: none;
  touch-action: none;
  transition: border-color 0.15s, color 0.15s, transform 0.15s;
}
.le-lib-item:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.le-lib-item:active {
  cursor: grabbing;
  transform: scale(0.98);
}
.le-lib-name {
  flex: 1;
  font-size: 0.8125rem;
  font-weight: 600;
}
.le-lib-size {
  font-size: 0.6875rem;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.le-lib-empty {
  margin: 8px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  text-align: center;
}

/* 右侧预览画布 */
.le-canvas {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: auto;
  padding: var(--space-4);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  position: relative;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.le-canvas.over {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.le-grid {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: 8px;
  height: 100%;
}
.le-cell {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  border: 1px solid var(--brand-300, var(--brand-500));
  border-radius: var(--radius-md);
  background: var(--brand-50);
  color: var(--brand-500);
  cursor: grab;
  user-select: none;
  touch-action: none;
  overflow: hidden;
}
.le-cell.dragging {
  opacity: 0.35;
}
.le-cell:active {
  cursor: grabbing;
}
.le-cell-grip {
  flex-shrink: 0;
  opacity: 0.55;
}
.le-cell-name {
  flex: 1;
  font-size: 0.8125rem;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.le-cell-size {
  flex-shrink: 0;
  font-size: 0.6875rem;
  color: var(--brand-500);
  opacity: 0.75;
  font-variant-numeric: tabular-nums;
  margin-right: 4px;
}
.le-cell-remove {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--brand-500);
  cursor: pointer;
  opacity: 0.6;
  transition: background 0.15s, opacity 0.15s;
}
.le-cell-remove:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
  opacity: 1;
}
.le-cell-resize {
  position: absolute;
  right: 0;
  bottom: 0;
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
  touch-action: none;
}
.le-cell-resize::after {
  content: '';
  position: absolute;
  right: 3px;
  bottom: 3px;
  width: 6px;
  height: 6px;
  border-right: 2px solid var(--brand-500);
  border-bottom: 2px solid var(--brand-500);
  border-bottom-right-radius: 2px;
  opacity: 0.7;
}
.le-preview {
  border: 2px dashed var(--brand-500);
  border-radius: var(--radius-md);
  background: var(--brand-50);
  opacity: 0.7;
  pointer-events: none;
}
.le-empty-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  margin: 0;
  font-size: 0.8125rem;
  color: var(--text-3);
  pointer-events: none;
}

/* 拖拽/缩放跟随浮层 */
.le-ghost {
  position: fixed;
  z-index: 999;
  pointer-events: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--brand-500);
  color: var(--text-on-accent);
  font-size: 0.8125rem;
  font-weight: 600;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-dock);
  transform: translate(12px, 12px);
}
.le-ghost-size {
  font-size: 0.6875rem;
  opacity: 0.85;
  font-variant-numeric: tabular-nums;
}
</style>
