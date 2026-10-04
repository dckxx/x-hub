<script setup lang="ts">
import { computed, inject, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { Check, GripVertical, LayoutGrid, X } from 'lucide-vue-next'
import {
  DASH_COLS,
  MIN_SIZE,
  TITLE_MAX,
  dashFitState,
  dashModuleDef,
  dashModuleTitle,
  dashModuleVariants,
  dashPlacementHideTitle,
  dashPlacementTitle,
  dashVariantDef,
  findFreeSpot,
  isLivePreview,
  useDashboardLayout,
  type DashPlacement,
  type DashVariantDef,
} from '../composables/useDashboardLayout'
import { startPreviewTick, stopPreviewTick } from '../composables/useDashPreviewData'
import ClockCard from './ClockCard.vue'
import DashModulePreview from './DashModulePreview.vue'
import WeatherCard from './WeatherCard.vue'

/**
 * 工作台布局编辑器（形态驱动 + 所见即所得）：
 * - 12×15 棋盘，列间距 / 卡片圆角与真实工作台一致（gap 16、radius-lg），画布即真实工作台的等比缩略
 * - 模块库：多形态模块先选形态再拖入（拖入按所选形态推荐尺寸落位）
 * - 画布：clock/weather 挂载真实组件，其余模块用 DashModulePreview（结构逐一对齐真实卡片，
 *   尺寸按 --dp-k 等比缩印）；每格带适配徽标
 *   绿=正好铺满 / 黄=紧凑可读 / 蓝=弹性空间 / 红=低于最小（缩放钳制到形态最小尺寸）
 * - 卡片 ⇄ 切换形态：格子小于新形态最小尺寸自动补足并就近让位
 * - 草稿语义：进入快照，确认 commitEdit 落盘，未确认切走 cancelEdit 回滚
 */

const emit = defineEmits<{ (e: 'done'): void }>()

const layout = useDashboardLayout()
const showToast = inject<(msg: string) => void>('showToast', () => {})

/** 画布列间距与真实工作台 .dash-grid 取齐（16px），保证缩印比例一致 */
const GAP = 16
const REAL_GAP = 16
const MIN_ROWS = 15

const canvasRef = ref<HTMLElement | null>(null)
const gridRef = ref<HTMLElement | null>(null)

// ---- 等比缩印系数 ----
// 预览内容一律按「真实卡片像素 × k」绘制，k = 编辑器每列像素 ÷ 真实工作台每列像素。
// 真实每列像素按主区宽度推算：窗口宽 − 实际侧栏宽（收起 56 / 展开 220，直接读 DOM，
// 侧栏开合会改变画布宽并顺带触发本函数重算）− 右内边距 20 − 列间距。
const dpK = ref(0.78)
const realColPx = ref(96)

function sidebarWidth(): number {
  const el = document.querySelector('.sidebar') as HTMLElement | null
  const w = el?.offsetWidth ?? 0
  // 窄屏（<720px）侧栏叠在内容上、宽度不再是网格列宽的一部分，异常值一律回退收起态
  return w >= 40 && w <= 320 ? w : 56
}

function recomputeScale() {
  const el = gridRef.value
  if (!el) return
  const w = el.clientWidth
  if (w <= 0) return
  const editorCol = (w - (DASH_COLS - 1) * GAP) / DASH_COLS
  const dashWidth = Math.max(640, window.innerWidth - sidebarWidth() - 20)
  realColPx.value = (dashWidth - (DASH_COLS - 1) * REAL_GAP) / DASH_COLS
  dpK.value = Math.min(1, Math.max(0.5, editorCol / realColPx.value))
}

let resizeObs: ResizeObserver | null = null

/** 形态浮层缩略图：框宽代表该形态的推荐宽度，故系数按「缩略框宽 ÷ 真实卡片宽」单独算 */
function thumbK(vd: DashVariantDef): number {
  const boxW = 62
  const realW = vd.idealW * realColPx.value + (vd.idealW - 1) * REAL_GAP
  return Math.min(1, Math.max(0.34, boxW / realW))
}
function thumbRatio(vd: DashVariantDef): string {
  return `${vd.idealW} / ${vd.idealH}`
}

// 模块库当前选中的形态（多形态模块才有意义）
const libVariant = ref<Record<string, string>>({})

function curVariantOf(id: string): DashVariantDef | undefined {
  return dashVariantDef(id, libVariant.value[id])
}
function curVariantId(id: string): string {
  return curVariantOf(id)?.id ?? dashModuleDef(id)?.defaultVariant ?? ''
}
function selectLibVariant(id: string, vId: string) {
  libVariant.value = { ...libVariant.value, [id]: vId }
}

function variantName(p: DashPlacement): string {
  return dashVariantDef(p.id, p.variant)?.name ?? ''
}
function hasVariants(id: string): boolean {
  return dashModuleVariants(id).length > 1
}

// ---- 填充审计 ----
const auditOn = ref(false)

// ---- 拖拽状态（Pointer 事件实现，HTML5 DnD 在 WebView2 下不稳定） ----
interface DragState {
  mode: 'move' | 'resize'
  id: string
  source: 'library' | 'canvas'
  w: number
  h: number
  col: number
  row: number
  origX: number
  origY: number
  origW: number
  origH: number
  startX: number
  startY: number
  started: boolean
}

const drag = ref<DragState | null>(null)
const ghostPos = ref({ x: 0, y: 0 })
const isOver = ref(false)
const preview = ref<{ x: number; y: number; w: number; h: number } | null>(null)
const previewBad = ref(false)
const draggingId = ref<string | null>(null)
const dragFitLabel = ref('')

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

function fitClass(p: DashPlacement) {
  return `fit-${dashFitState(p).level}`
}

function cellFromPoint(clientX: number, clientY: number) {
  const el = gridRef.value
  if (!el) return null
  const rect = el.getBoundingClientRect()
  const px = clientX - rect.left
  const py = clientY - rect.top
  const cellW = (rect.width - (DASH_COLS - 1) * GAP) / DASH_COLS
  const col = Math.min(Math.max(Math.floor(px / (cellW + GAP)), 0), DASH_COLS - 1)
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
  const vd = dashVariantDef(id, source === 'library' ? libVariant.value[id] : undefined)
  if (!vd) return
  e.preventDefault()
  const p = source === 'canvas' ? layout.placements.value.find((q) => q.id === id) : undefined
  drag.value = {
    mode: 'move',
    id,
    source,
    // 画布内移动沿用模块当前实际尺寸（用户可能已缩放过），库区拖入用所选形态推荐尺寸
    w: p?.w ?? vd.idealW,
    h: p?.h ?? vd.idealH,
    col: 0,
    row: 0,
    origX: p?.x ?? 0,
    origY: p?.y ?? 0,
    origW: p?.w ?? vd.idealW,
    origH: p?.h ?? vd.idealH,
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

function minSizeOf(id: string, variant?: string): { minW: number; minH: number } {
  const vd = dashVariantDef(id, variant)
  return { minW: vd?.minW ?? MIN_SIZE, minH: vd?.minH ?? MIN_SIZE }
}

function onPointerMove(e: PointerEvent) {
  if (!drag.value) return
  const d = drag.value
  ghostPos.value = { x: e.clientX, y: e.clientY }
  if (!d.started) {
    if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) < 6) return
    d.started = true
  }
  const inside = isInsideCanvas(e.clientX, e.clientY)
  isOver.value = inside
  if (!inside) {
    preview.value = null
    previewBad.value = false
    dragFitLabel.value = ''
    return
  }
  const cell = cellFromPoint(e.clientX, e.clientY)
  if (!cell) return
  if (d.mode === 'resize') {
    preview.value = null
    const p = layout.placements.value.find((q) => q.id === d.id)
    if (!p) return
    const { minW, minH } = minSizeOf(p.id, p.variant)
    const nw = Math.min(Math.max(cell.col - p.x + 1, minW), DASH_COLS - p.x)
    const nh = Math.max(cell.row - p.y + 1, minH)
    d.w = nw
    d.h = nh
    // 实时反馈：拖拽过程中同步改模块尺寸，让画布即时看到大小变化
    p.w = nw
    p.h = nh
    const fit = dashFitState(p)
    dragFitLabel.value = `${nw}×${nh} · ${fit.label}`
    return
  }
  const col = Math.min(Math.max(cell.col, 0), DASH_COLS - d.w)
  d.col = col
  d.row = cell.row
  const others = d.source === 'canvas' ? layout.placements.value.filter((q) => q.id !== d.id) : layout.placements.value
  const spot = findFreeSpot(others, d.w, d.h, col, cell.row)
  preview.value = { x: spot.x, y: spot.y, w: d.w, h: d.h }
  const probe: DashPlacement = { id: d.id, x: spot.x, y: spot.y, w: d.w, h: d.h, variant: d.source === 'library' ? libVariant.value[d.id] : undefined }
  const fit = dashFitState(probe)
  previewBad.value = fit.level === 'below'
  dragFitLabel.value = `${d.w}×${d.h} · ${fit.label}`
}

function onPointerUp(e: PointerEvent) {
  if (!drag.value) return
  const d = drag.value
  drag.value = null
  isOver.value = false
  preview.value = null
  previewBad.value = false
  draggingId.value = null
  dragFitLabel.value = ''

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
    layout.addModule(d.id, col, cell.row, libVariant.value[d.id])
  } else {
    layout.moveModule(d.id, col, cell.row)
  }
}

function remove(id: string) {
  layout.removeModule(id)
}

// ---- 形态切换浮层 ----
const variantPop = ref<DashPlacement | null>(null)
const popStyle = ref({ left: '0px', top: '0px' })

/** 浮层定位：优先贴在格子右侧，越界则翻到左侧 / 上移，两边都留 8px 边距 */
async function positionPop(e: PointerEvent, sel: string, fallbackW = 280, fallbackH = 220) {
  await nextTick()
  const btn = (e.currentTarget as HTMLElement) ?? null
  const cellEl = btn?.closest('.le-cell') as HTMLElement | null
  if (!cellEl) return
  const r = cellEl.getBoundingClientRect()
  const pop = document.querySelector(sel) as HTMLElement | null
  const pw = pop?.offsetWidth ?? fallbackW
  const ph = pop?.offsetHeight ?? fallbackH
  let left = r.right + 8
  let top = r.top
  if (left + pw > window.innerWidth - 8) left = Math.max(8, r.left - pw - 8)
  if (top + ph > window.innerHeight - 8) top = Math.max(8, window.innerHeight - ph - 8)
  popStyle.value = { left: `${left}px`, top: `${top}px` }
}

async function openVariantPop(p: DashPlacement, e: PointerEvent) {
  closeTitlePop()
  variantPop.value = p
  popStyle.value = { left: '0px', top: '0px' }
  await positionPop(e, '.le-pop', 280, 220)
}

function closeVariantPop() {
  variantPop.value = null
}

function applyVariant(vId: string) {
  if (!variantPop.value) return
  layout.setModuleVariant(variantPop.value.id, vId)
  closeVariantPop()
}

// ---- 标题浮层（关闭标题行 / 自定义标题文案） ----
const titlePop = ref<DashPlacement | null>(null)
const titleDraft = ref('')
const titleHideDraft = ref(false)

/** 只有卡片自带表头的模块才谈得上改标题：时钟/天气是纯内容卡（扩展 module 也有宿主表头，可改） */
function canTitle(id: string): boolean {
  return id !== 'clock' && id !== 'weather'
}

async function openTitlePop(p: DashPlacement, e: PointerEvent) {
  closeVariantPop()
  titlePop.value = p
  titleDraft.value = p.title ?? ''
  titleHideDraft.value = dashPlacementHideTitle(p)
  popStyle.value = { left: '0px', top: '0px' }
  await positionPop(e, '.le-tpop', 260, 180)
}

function closeTitlePop() {
  titlePop.value = null
}

/** 输入即生效（编辑器是草稿语义：取消 / 关掉编辑器会整体回滚） */
function applyTitle() {
  if (!titlePop.value) return
  layout.setModuleTitle(titlePop.value.id, titleDraft.value, titleHideDraft.value)
}

/** 恢复默认：清空自定义标题，表头显隐回到「跟随作者默认」（扩展卡才有作者默认；内置卡 = 显示） */
function resetTitle() {
  if (!titlePop.value) return
  titleDraft.value = ''
  layout.setModuleTitle(titlePop.value.id, '', null)
  titleHideDraft.value = dashPlacementHideTitle(titlePop.value)
}

function fitsMin(p: DashPlacement, vd: DashVariantDef): boolean {
  return p.w >= vd.minW && p.h >= vd.minH
}

// 全局关闭：点击浮层外（含点击其它 ⇄ 按钮重新打开）
function onGlobalPointerDown(e: PointerEvent) {
  const t = e.target as HTMLElement
  if (variantPop.value && !t.closest('.le-pop') && !t.closest('[data-variant-btn]')) {
    closeVariantPop()
  }
  if (titlePop.value && !t.closest('.le-tpop') && !t.closest('[data-title-btn]')) {
    closeTitlePop()
  }
}

// ---- 提交 / 回滚 ----
const committed = ref(false)

function confirmDone() {
  committed.value = true
  layout.commitEdit()
  emit('done')
}

// ---- 默认布局快照：存为默认（立即落盘，不随草稿取消回滚）/ 恢复默认（回到草稿，确认后生效） ----
function onSaveDefault() {
  if (!layout.saveDefaultLayout()) {
    showToast('画布为空，先拖入模块再存为默认布局')
    return
  }
  showToast('已存为默认布局，之后可随时一键恢复')
}

function onRestoreDefault() {
  if (!layout.restoreDefaultLayout()) {
    showToast('默认布局已失效，请重新「存为默认」')
    return
  }
  showToast('已恢复为默认布局，点「确认」生效')
}

onMounted(() => {
  layout.beginEdit()
  // 预览里的时间 / 农历 / 倒计时剩余按分钟推进（离开编辑器即停，无常驻定时器）
  startPreviewTick()
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('pointerdown', onGlobalPointerDown, true)
  recomputeScale()
  if (gridRef.value) {
    resizeObs = new ResizeObserver(recomputeScale)
    resizeObs.observe(gridRef.value)
  }
})
onUnmounted(() => {
  stopPreviewTick()
  resizeObs?.disconnect()
  resizeObs = null
  if (!committed.value) layout.cancelEdit()
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  window.removeEventListener('pointerdown', onGlobalPointerDown, true)
})

function previewComponent(id: string) {
  return id === 'clock' ? ClockCard : WeatherCard
}
</script>

<template>
  <div class="le-root">
    <header class="le-header">
      <div class="le-header-left">
        <h2 class="le-title">自定义布局</h2>
        <span class="le-hint">
          12×15 · 先选形态再拖入 · 拖动换位 · 右下角缩放（钳制到形态最小）· 点 ⇄ 切形态 · 点 × 移除
        </span>
      </div>
      <div class="le-header-right">
        <label class="le-audit">
          <input v-model="auditOn" type="checkbox" />
          填充审计徽标
        </label>
        <button class="ghost-btn" type="button" @click="layout.clear()">清空</button>
        <button class="ghost-btn" type="button" @click="layout.applyPreset()">推荐布局</button>
        <button
          class="ghost-btn"
          type="button"
          title="把画布当前布局存为默认布局，之后可随时一键恢复"
          @click="onSaveDefault"
        >存为默认</button>
        <button
          class="ghost-btn"
          type="button"
          :disabled="!layout.hasSavedDefault.value"
          title="把画布恢复为保存的默认布局（需点「确认」生效）"
          @click="onRestoreDefault"
        >恢复默认</button>
        <button class="pill-btn" type="button" @click="confirmDone">
          <Check :size="14" :stroke-width="2.5" aria-hidden="true" />
          确认
        </button>
      </div>
    </header>

    <div class="le-body">
      <!-- 左侧：模块库（多形态模块先选形态） -->
      <aside class="le-library" aria-label="模块库">
        <p class="le-lib-title">模块库</p>
        <div
          v-for="m in layout.available.value"
          :key="m.id"
          class="le-lib-item"
          @pointerdown="startMove(m.id, 'library', $event)"
        >
          <div class="le-lib-head">
            <GripVertical :size="14" :stroke-width="2" aria-hidden="true" />
            <span class="le-lib-name">{{ m.title }}</span>
            <span class="le-lib-size">{{ curVariantOf(m.id)?.idealW }}×{{ curVariantOf(m.id)?.idealH }}</span>
          </div>
          <div v-if="m.variants.length > 1" class="le-lib-variants">
            <button
              v-for="vd in m.variants"
              :key="vd.id"
              type="button"
              class="le-vchip"
              :class="{ on: curVariantId(m.id) === vd.id }"
              @pointerdown.stop="selectLibVariant(m.id, vd.id)"
            >{{ vd.name }}</button>
          </div>
          <div class="le-lib-meta">
            <span class="le-lib-pill">最小 {{ curVariantOf(m.id)?.minW }}×{{ curVariantOf(m.id)?.minH }}</span>
            <span class="le-lib-pill">推荐 {{ curVariantOf(m.id)?.idealW }}×{{ curVariantOf(m.id)?.idealH }}</span>
            <span class="le-lib-desc">{{ curVariantOf(m.id)?.desc }}</span>
          </div>
        </div>
        <p v-if="layout.available.value.length === 0" class="le-lib-empty">
          所有模块都已放置
        </p>
      </aside>

      <!-- 右侧：等比缩略画布 -->
      <div
        ref="canvasRef"
        class="le-canvas"
        :class="{ over: isOver, empty: layout.placements.value.length === 0 }"
      >
        <div
          ref="gridRef"
          class="le-grid"
          :class="{ 'audit-on': auditOn }"
          :style="{
            gridTemplateRows: `repeat(${rowCount}, minmax(0, 1fr))`,
            '--dp-k': dpK,
          }"
        >
          <div
            v-for="p in layout.placements.value"
            :key="p.id"
            class="le-cell"
            :class="[fitClass(p), { dragging: draggingId === p.id }]"
            :style="cellStyle(p)"
            @pointerdown="startMove(p.id, 'canvas', $event)"
          >
            <!-- 真实组件预览（clock/weather 所见即所得） -->
            <component
              :is="previewComponent(p.id)"
              v-if="isLivePreview(p.id)"
              :variant="p.variant"
              :preview="true"
              class="le-live"
            />
            <!-- 其余模块：真实卡片结构的等比缩印（--dp-k 由画布列宽推得） -->
            <div v-else class="le-pv">
              <DashModulePreview
                :mod-id="p.id"
                :variant="p.variant"
                :title="p.title"
                :hide-title="dashPlacementHideTitle(p)"
              />
            </div>

            <span class="le-cell-tag">{{ dashPlacementTitle(p) }}<template v-if="variantName(p)"> · {{ variantName(p) }}</template></span>
            <div class="le-cell-ctrl">
              <button
                v-if="hasVariants(p.id)"
                type="button"
                class="le-cell-btn"
                data-variant-btn
                :title="`切换${dashModuleTitle(p.id)}形态`"
                @pointerdown.stop="openVariantPop(p, $event)"
              >⇄</button>
              <button
                v-if="canTitle(p.id)"
                type="button"
                class="le-cell-btn"
                :class="{ on: dashPlacementHideTitle(p) }"
                data-title-btn
                :title="`设置${dashModuleTitle(p.id)}标题`"
                :aria-label="`设置${dashModuleTitle(p.id)}标题`"
                @pointerdown.stop="openTitlePop(p, $event)"
              >Aa</button>
              <button
                type="button"
                class="le-cell-btn le-cell-remove"
                :title="`移除${dashModuleTitle(p.id)}`"
                :aria-label="`移除${dashModuleTitle(p.id)}`"
                @pointerdown.stop="remove(p.id)"
              ><X :size="13" :stroke-width="2" aria-hidden="true" /></button>
            </div>
            <span class="le-cell-badge" :class="'b-' + dashFitState(p).level">{{ dashFitState(p).label }}</span>
            <span class="le-cell-size">{{ p.w }}×{{ p.h }}</span>
            <span
              class="le-cell-resize"
              :aria-label="`调整${dashModuleTitle(p.id)}尺寸`"
              @pointerdown.stop="startResize(p.id, $event)"
            ></span>
          </div>
          <div
            v-if="preview"
            class="le-preview"
            :class="{ bad: previewBad }"
            :style="{ gridColumn: `${preview.x + 1} / span ${preview.w}`, gridRow: `${preview.y + 1} / span ${preview.h}` }"
          ></div>
        </div>
        <p v-if="layout.placements.value.length === 0" class="le-empty-hint">
          <LayoutGrid :size="16" :stroke-width="2" aria-hidden="true" />
          从左侧拖入模块开始搭建
        </p>
      </div>
    </div>

    <!-- 拖拽浮层 -->
    <Teleport to="body">
      <div
        v-if="drag"
        class="le-ghost"
        :style="{ left: ghostPos.x + 'px', top: ghostPos.y + 'px' }"
      >
        {{ dashModuleTitle(drag.id) }}
        <span class="le-ghost-size">{{ dragFitLabel || `${drag.w}×${drag.h}` }}</span>
      </div>
    </Teleport>

    <!-- 形态切换浮层 -->
    <Teleport to="body">
      <div v-if="variantPop" class="le-pop" :style="popStyle">
        <p class="le-pop-title">
          {{ dashModuleTitle(variantPop.id) }} · 选择展示形态
        </p>
        <div
          v-for="vd in dashModuleVariants(variantPop.id)"
          :key="vd.id"
          class="le-pop-opt"
          :class="{ on: variantPop.variant === vd.id }"
          @click="applyVariant(vd.id)"
        >
          <div
            class="le-pop-thumb"
            :style="{ aspectRatio: thumbRatio(vd), '--dp-k': thumbK(vd) }"
          >
            <div class="le-pop-thumb-in">
              <DashModulePreview :mod-id="variantPop.id" :variant="vd.id" />
            </div>
          </div>
          <div class="le-pop-info">
            <div class="le-pop-name">
              {{ vd.name }}
              <span v-if="variantPop.variant === vd.id" class="le-pop-cur">当前</span>
            </div>
            <div class="le-pop-desc">{{ vd.desc }}</div>
            <div class="le-pop-fit" :class="{ ok: fitsMin(variantPop, vd) }">
              {{ fitsMin(variantPop, vd) ? '✓' : '⚠' }}
              当前格 {{ variantPop.w }}×{{ variantPop.h }} · 最小 {{ vd.minW }}×{{ vd.minH }} · 推荐 {{ vd.idealW }}×{{ vd.idealH }}
            </div>
          </div>
        </div>
        <p class="le-pop-foot">切换形态后，若格子小于新形态的最小尺寸将自动补足并就近让位。</p>
      </div>
    </Teleport>

    <!-- 标题浮层：关闭标题行 / 自定义标题文案 -->
    <Teleport to="body">
      <div v-if="titlePop" class="le-pop le-tpop" :style="popStyle">
        <p class="le-pop-title">
          {{ dashModuleTitle(titlePop.id) }} · 标题
        </p>
        <input
          v-model="titleDraft"
          class="le-tinput"
          type="text"
          :maxlength="TITLE_MAX"
          :placeholder="dashModuleTitle(titlePop.id)"
          :aria-label="`${dashModuleTitle(titlePop.id)}自定义标题`"
          @input="applyTitle"
        />
        <label class="le-tcheck">
          <input v-model="titleHideDraft" type="checkbox" @change="applyTitle" />
          <span>不显示标题行</span>
        </label>
        <p class="le-t-hint">
          留空 = 用默认标题「{{ dashModuleTitle(titlePop.id) }}」；关掉标题行后卡片内容顶到上内边距，不再留标题行的空档。
        </p>
        <div class="le-t-foot">
          <button class="le-pop-btn" type="button" @click="resetTitle">恢复默认</button>
          <button class="le-pop-btn primary" type="button" @click="closeTitlePop">完成</button>
        </div>
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
.le-audit {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 0.72rem;
  color: var(--text-3);
  cursor: pointer;
  white-space: nowrap;
}
.le-audit input {
  accent-color: var(--brand-500);
}

.le-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--space-4);
  overflow: hidden;
}

/* ===== 左侧模块库 ===== */
.le-library {
  flex-shrink: 0;
  width: 224px;
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
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-solid);
  padding: 8px 10px;
  cursor: grab;
  user-select: none;
  touch-action: none;
  transition: border-color 0.15s;
}
.le-lib-item:hover {
  border-color: var(--brand-500);
}
.le-lib-item:active {
  cursor: grabbing;
}
.le-lib-head {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--text-2);
}
.le-lib-name {
  flex: 1;
  font-size: 0.8125rem;
  font-weight: 600;
}
.le-lib-size {
  font-size: 0.6875rem;
  color: var(--brand-500);
  font-variant-numeric: tabular-nums;
  opacity: 0.8;
}
.le-lib-variants {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}
.le-vchip {
  font-size: 0.66rem;
  font-weight: 600;
  color: var(--text-3);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  padding: 2px 9px;
  cursor: pointer;
  transition: all 0.12s;
}
.le-vchip:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.le-vchip.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: var(--text-on-accent);
}
.le-lib-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
}
.le-lib-pill {
  font-size: 0.62rem;
  color: var(--text-3);
  background: var(--bg-card-soft);
  border-radius: 4px;
  padding: 1px 6px;
  font-variant-numeric: tabular-nums;
}
.le-lib-desc {
  font-size: 0.62rem;
  color: var(--text-4);
  width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.le-lib-empty {
  margin: 8px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  text-align: center;
}

/* ===== 右侧预览画布 ===== */
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
  gap: 16px;
  height: 100%;
}

/* 卡片格：与真实工作台 .dash-cell 里的 .card 同款材质（玻璃面 + 12px 圆角 + 落影），
 * 适配状态用彩色描边只在 hover / 开启填充审计时出现，避免画布被四色边框糊满而失真 */
.le-cell {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  box-shadow: var(--frost-edge), var(--shadow-card);
  cursor: grab;
  user-select: none;
  touch-action: none;
  overflow: hidden;
  container-type: size;
  min-width: 0;
  min-height: 0;
}
.le-cell:hover {
  border-color: var(--brand-500);
}
.le-cell.dragging {
  opacity: 0.4;
}
.le-cell:active {
  cursor: grabbing;
}
.le-cell.fit-below {
  border-color: var(--c-red-ink);
}
.le-cell:hover.fit-mid,
.audit-on .le-cell.fit-mid {
  border-color: var(--c-yellow-ink);
}
.le-cell:hover.fit-room,
.audit-on .le-cell.fit-room {
  border-color: var(--c-blue-ink);
}
.le-cell:hover.fit-ideal,
.audit-on .le-cell.fit-ideal {
  border-color: var(--c-green-ink);
}

.le-live,
.le-pv {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.le-live :deep(.card),
.le-live :deep(.clock-card),
.le-live :deep(.weather-card) {
  height: 100%;
  border: none;
  box-shadow: none;
  background: transparent;
  border-radius: var(--radius-lg);
}

.le-cell-tag {
  position: absolute;
  left: 4px;
  top: 4px;
  z-index: 5;
  max-width: 62%;
  font-size: 0.55rem;
  font-weight: 600;
  color: var(--text-3);
  background: color-mix(in srgb, var(--bg-card-solid) 72%, transparent);
  border: 1px solid var(--border-soft);
  border-radius: 4px;
  padding: 0 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
}
.le-cell-ctrl {
  position: absolute;
  top: 3px;
  right: 3px;
  z-index: 6;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.12s;
}
.le-cell:hover .le-cell-ctrl {
  opacity: 1;
}
.le-cell-btn {
  width: 16px;
  height: 16px;
  border: none;
  border-radius: 4px;
  background: var(--scrim);
  color: #fff;
  font-size: 0.68rem;
  line-height: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  padding: 0;
}
.le-cell-btn:hover {
  background: var(--brand-500);
}
/* 已关闭标题行的模块：按钮常亮，一眼看出哪些卡没标题 */
.le-cell-btn.on {
  background: var(--brand-500);
}
.le-cell-remove {
  width: 18px;
  height: 18px;
}
.le-cell-badge {
  position: absolute;
  left: 4px;
  bottom: 4px;
  z-index: 5;
  font-size: 0.56rem;
  font-weight: 700;
  border-radius: 4px;
  padding: 0 5px;
  display: inline-flex;
  align-items: center;
  white-space: nowrap;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.12s;
}
.le-cell:hover .le-cell-badge,
.audit-on .le-cell-badge {
  opacity: 1;
}
.le-cell-badge.b-below {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.le-cell-badge.b-mid {
  background: var(--c-yellow-soft);
  color: var(--c-yellow-ink);
}
.le-cell-badge.b-ideal {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.le-cell-badge.b-room {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.le-cell-size {
  position: absolute;
  right: 4px;
  bottom: 4px;
  z-index: 5;
  font-size: 0.56rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--text-3);
  background: color-mix(in srgb, var(--bg-card-solid) 72%, transparent);
  border: 1px solid var(--border-soft);
  border-radius: 4px;
  padding: 0 4px;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.12s;
}
.le-cell:hover .le-cell-size {
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
  z-index: 7;
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
  opacity: 0.75;
}
.le-preview {
  border: 2px dashed var(--brand-500);
  border-radius: var(--radius-lg);
  background: var(--brand-50);
  opacity: 0.75;
  pointer-events: none;
  z-index: 2;
}
.le-preview.bad {
  border-color: var(--c-red-ink);
  background: var(--c-red-soft);
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

/* ===== 拖拽浮层 ===== */
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
  opacity: 0.88;
  font-variant-numeric: tabular-nums;
}

/* ===== 形态切换浮层 ===== */
.le-pop {
  position: fixed;
  z-index: 1100;
  width: 286px;
  max-width: calc(100vw - 16px);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dock);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.le-pop-title {
  margin: 0 0 2px;
  font-size: 0.72rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.le-pop-opt {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 7px 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all 0.12s;
}
.le-pop-opt:hover {
  border-color: var(--brand-500);
  background: var(--brand-50);
}
.le-pop-opt.on {
  border-color: var(--brand-500);
  background: var(--brand-50);
  box-shadow: inset 0 0 0 1px var(--brand-500);
}
/* 缩略框按该形态真实宽高比（idealW / idealH）成形，内容按 thumbK 等比缩印真实卡片 */
.le-pop-thumb {
  flex-shrink: 0;
  width: 62px;
  height: auto;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-soft);
  background: var(--frost-surface);
  overflow: hidden;
  container-type: size;
  position: relative;
}
.le-pop-thumb-in {
  width: 100%;
  height: 100%;
}
.le-pop-info {
  min-width: 0;
  flex: 1;
}
.le-pop-name {
  font-size: 0.78rem;
  font-weight: 700;
  color: var(--text-1);
  display: flex;
  align-items: center;
  gap: 6px;
}
.le-pop-cur {
  font-size: 0.6rem;
  color: var(--brand-500);
  background: var(--brand-50);
  border-radius: var(--radius-pill);
  padding: 0 6px;
}
.le-pop-desc {
  font-size: 0.66rem;
  color: var(--text-3);
  margin-top: 1px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.le-pop-fit {
  font-size: 0.64rem;
  margin-top: 2px;
  display: inline-flex;
  gap: 4px;
  align-items: center;
  color: var(--c-red-ink);
}
.le-pop-fit.ok {
  color: var(--c-green-ink);
}
.le-pop-foot {
  margin: 0;
  font-size: 0.64rem;
  color: var(--text-3);
  line-height: 1.5;
  border-top: 1px solid var(--border-soft);
  padding-top: 6px;
}

/* ---- 标题浮层 ---- */
.le-tpop {
  width: 268px;
}
.le-tinput {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 9px;
  font-size: 0.78rem;
  color: var(--text-1);
  background: var(--bg-inset, var(--bg-card));
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  outline: none;
}
.le-tinput:focus {
  border-color: var(--brand-500);
}
.le-tcheck {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 0.74rem;
  color: var(--text-2);
  cursor: pointer;
  padding: 2px 1px;
}
.le-tcheck input {
  width: 14px;
  height: 14px;
  accent-color: var(--brand-500);
  cursor: pointer;
}
.le-t-hint {
  margin: 0;
  font-size: 0.64rem;
  color: var(--text-3);
  line-height: 1.5;
}
.le-t-foot {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
  border-top: 1px solid var(--border-soft);
  padding-top: 8px;
  margin-top: 2px;
}
.le-pop-btn {
  padding: 5px 10px;
  font-size: 0.72rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-soft);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
}
.le-pop-btn:hover {
  border-color: var(--brand-500);
  color: var(--brand-600, var(--brand-500));
}
.le-pop-btn.primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.le-pop-btn.primary:hover {
  color: #fff;
  filter: brightness(1.05);
}
</style>
