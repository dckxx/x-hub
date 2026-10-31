<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { Camera, Check, Circle, Clapperboard, Eraser, Pencil, Pin, Play, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type CaptureAnnotation, type RecRegion } from '../api/tauri'

// 选区覆盖窗口：透明全屏，截图与 GIF 录制共用（按 overlay-shown 事件的 mode 切换交互）
document.documentElement.dataset.recorderOverlay = ''

const appWindow = isTauri() ? getCurrentWindow() : null
// 本次模式：shot = 框选→调整→(画笔标注)→确认出图；record = 框选后设帧率/时长再录
const mode = ref<'shot' | 'record'>('record')
// 目标显示器物理原点/尺寸：直接从窗口物理位置读取（事件可能早于监听注册，
// 用 outerPosition/outerSize 兜底，事件仅作刷新）
const mon = ref<{ x: number; y: number; width: number; height: number } | null>(null)

async function syncMonitorBounds() {
  if (!appWindow) return
  try {
    const pos = await appWindow.outerPosition()
    const size = await appWindow.outerSize()
    mon.value = { x: pos.x, y: pos.y, width: size.width, height: size.height }
  } catch {
    // 忽略：非 Tauri 环境
  }
}

// 拖拽选区（CSS 坐标，webview 内）
const drag = ref<{ x0: number; y0: number; x1: number; y1: number; active: boolean }>({
  x0: 0,
  y0: 0,
  x1: 0,
  y1: 0,
  active: false,
})

// 时长（仅录制模式面板可选）；帧率不逐次选择——统一用设置默认值（recorder_fps，
// 后端 recorder_start 收到 null 自取配置），面板只剩时长
const maxSeconds = ref(30)
const TIME_OPTIONS = [10, 15, 30, 60]

/** 选区 CSS 尺寸（min/max 后） */
function cssSize(): { w: number; h: number } {
  return {
    w: Math.abs(drag.value.x1 - drag.value.x0),
    h: Math.abs(drag.value.y1 - drag.value.y0),
  }
}

/** 调整态（截图模式独有）：选框保留，可拖把手调整、标注、确认/钉住/重选 */
const adjusting = ref(false)
/** 标注工具：none = 未在标注（把手可拖）；pen/rect/ellipse = 在选区内拖出对应标注 */
const tool = ref<'none' | 'pen' | 'rect' | 'ellipse'>('none')
/** 已定稿标注：归一化坐标（相对选区左上角 0..1），随选区调整跟随缩放 */
const annotations = ref<CaptureAnnotation[]>([])
/** 拖拽中的形状预览（rect/ellipse） */
const currentShape = ref<CaptureAnnotation | null>(null)
/** 调整态最小边（CSS px），低于该值忽略拖拽防抖成 0 */
const MIN_CSS = 8

function resetSelection() {
  drag.value = { x0: 0, y0: 0, x1: 0, y1: 0, active: false }
  adjusting.value = false
  tool.value = 'none'
  annotations.value = []
  currentShape.value = null
}

/** 物理像素选区（计算用） */
function toPhysicalRegion(): RecRegion | null {
  if (!mon.value || !drag.value.active) return null
  const dpr = window.devicePixelRatio || 1
  const x0 = Math.min(drag.value.x0, drag.value.x1)
  const y0 = Math.min(drag.value.y0, drag.value.y1)
  const x1 = Math.max(drag.value.x0, drag.value.x1)
  const y1 = Math.max(drag.value.y0, drag.value.y1)
  const cssW = x1 - x0
  const cssH = y1 - y0
  if (cssW < MIN_CSS || cssH < MIN_CSS) return null
  return {
    x: Math.round(mon.value.x + x0 * dpr),
    y: Math.round(mon.value.y + y0 * dpr),
    width: Math.round(cssW * dpr),
    height: Math.round(cssH * dpr),
  }
}

// 计算显示尺寸
const sizeText = ref('')
function updateSizeText() {
  const r = toPhysicalRegion()
  sizeText.value = r ? `${r.width} × ${r.height}` : ''
}

// ---------------------------------------------------------------------------
// 指针交互：三态绘制（框选 drawing / 调整 resizing / 画笔 inking）
// ---------------------------------------------------------------------------
type PointerAction = 'drawing' | 'resizing' | 'inking' | 'moving' | null
let action: PointerAction = null
let startX = 0
let startY = 0
/** 正在调整的把手方向：n/s/e/w 组合（如 nw、se） */
let resizeDirs = ''
/** 形状标注的起点锚（归一化） */
let shapeAnchor: [number, number] = [0, 0]
/** 移动选区时的四边快照 */
let moveOrig = { x0: 0, y0: 0, x1: 0, y1: 0 }
/** 自由框选进行中：move 首次超阈值置 true（此刻才清 hover 预选框），up 后复位 */
let freeDrawing = false
/** 用户是否已手动交互：交互后停用窗口自动识别跟随（选框交还用户） */
const interactive = ref(false)
let hoverLockSent = false

/** 首次交互：锁定后端 hover 跟随（fire-and-forget，失败不影响交互） */
function lockHover() {
  if (hoverLockSent) return
  hoverLockSent = true
  interactive.value = true
  if (isTauri()) void tauriApi.recorderHoverLock().catch(() => {})
}

function onPointerDown(e: PointerEvent) {
  if (e.button !== 0) return
  // 录制中不响应框选
  if (recording.value) return
  if ((e.target as HTMLElement).closest('.ro-panel, .ro-tip, .ro-toolbar')) return
  ;(e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId)
  // 按下即锁跟随（防按住期间选框漂移）；单击（选中）vs 拖拽（自由框选）在 pointerup 按位移分流
  lockHover()
  startX = e.clientX
  startY = e.clientY

  // 标注工具激活 + 调整态：在选区内起笔/起形（归一化坐标，越界由后端 clamp）
  if (mode.value === 'shot' && adjusting.value && tool.value !== 'none') {
    action = 'inking'
    if (tool.value === 'pen') {
      annotations.value.push({ kind: 'stroke', points: [] })
      pushInkPoint(e.clientX, e.clientY)
    } else {
      // 矩形/圆形：起点为锚，move 中实时更新包围盒预览
      shapeAnchor = toNorm(e.clientX, e.clientY)
      currentShape.value = { kind: tool.value, x: shapeAnchor[0], y: shapeAnchor[1], w: 0, h: 0 }
    }
    return
  }
  // 调整态：命中把手 = 调整大小；其余（选框内外任意处）拖动 = 整体移动选区。
  // 截图与 GIF 录制都进调整态（录制模式标注工具恒不可用，tool 恒 none 不冲突）
  if (adjusting.value) {
    const h = (e.target as HTMLElement).dataset?.h
    if (h) {
      action = 'resizing'
      resizeDirs = h
    } else {
      action = 'moving'
      moveOrig = { ...drag.value }
    }
    return
  }

  action = 'drawing'
  starting.value = false
  errorMsg.value = ''
  // **不清 hover 框、不重置 drag**：单击（up 时位移小）要选中当前 hover 框进调整态，
  // 按下就清掉它单击就永远失焦。自由框选的重置挪到 move 首次超阈值时（freeDrawing）
}

function onPointerMove(e: PointerEvent) {
  if (!action) return
  if (action === 'drawing') {
    // 位移 >2px 才算开始自由框选：此刻才清掉 hover 预选框、从按下点重新拉框
    if (!freeDrawing) {
      if (Math.abs(e.clientX - startX) <= 2 && Math.abs(e.clientY - startY) <= 2) return
      freeDrawing = true
      resetSelection()
      drag.value = { x0: startX, y0: startY, x1: e.clientX, y1: e.clientY, active: true }
    }
    drag.value.x1 = e.clientX
    drag.value.y1 = e.clientY
    updateSizeText()
    return
  }
  if (action === 'resizing') {
    if (resizeDirs.includes('w')) drag.value.x0 = e.clientX
    if (resizeDirs.includes('e')) drag.value.x1 = e.clientX
    if (resizeDirs.includes('n')) drag.value.y0 = e.clientY
    if (resizeDirs.includes('s')) drag.value.y1 = e.clientY
    const { w, h } = cssSize()
    if (w >= MIN_CSS && h >= MIN_CSS) updateSizeText()
    return
  }
  if (action === 'moving') {
    // 整体平移：四边同步加位移量（把把手拖动与标注留各自的分支）
    const dx = e.clientX - startX
    const dy = e.clientY - startY
    drag.value.x0 = moveOrig.x0 + dx
    drag.value.x1 = moveOrig.x1 + dx
    drag.value.y0 = moveOrig.y0 + dy
    drag.value.y1 = moveOrig.y1 + dy
    return
  }
  // inking：画笔加点 / 形状以起点为锚更新包围盒
  if (tool.value === 'pen') {
    pushInkPoint(e.clientX, e.clientY)
  } else if (currentShape.value) {
    const [nx, ny] = toNorm(e.clientX, e.clientY)
    const s = currentShape.value
    s.x = Math.min(shapeAnchor[0], nx)
    s.y = Math.min(shapeAnchor[1], ny)
    s.w = Math.abs(nx - shapeAnchor[0])
    s.h = Math.abs(ny - shapeAnchor[1])
  }
}

/** CSS 坐标 → 归一化（相对选区左上角，0..1 clamp） */
function toNorm(cssX: number, cssY: number): [number, number] {
  const { w, h } = cssSize()
  const x0 = Math.min(drag.value.x0, drag.value.x1)
  const y0 = Math.min(drag.value.y0, drag.value.y1)
  const nx = w < MIN_CSS ? 0 : Math.min(Math.max((cssX - x0) / w, 0), 1)
  const ny = h < MIN_CSS ? 0 : Math.min(Math.max((cssY - y0) / h, 0), 1)
  return [nx, ny]
}

/** 画笔：CSS 坐标 → 归一化并追加到最后一笔 */
function pushInkPoint(cssX: number, cssY: number) {
  const [nx, ny] = toNorm(cssX, cssY)
  const cur = annotations.value[annotations.value.length - 1]
  if (!cur || cur.kind !== 'stroke') return
  const pts = cur.points ?? (cur.points = [])
  const last = pts[pts.length - 1]
  // 相距 >0.4%（归一化）才记点，压掉静止抖动
  if (!last || Math.abs(last[0] - nx) > 0.004 || Math.abs(last[1] - ny) > 0.004) {
    pts.push([nx, ny])
  }
}

function onPointerUp() {
  if (action === 'drawing') {
    // 位移 <4px = 左键单击：**确认选中**当前框（hover 窗口框）→ 进调整态（工具条出现），
    // 确认截图走工具条 ✓ 或调整态双击；拖拽 = 自由框选（松手同样进调整态）
    updateSizeText()
    const ok = !!toPhysicalRegion()
    if (ok) {
      // 截图模式：松手不立即截，进入调整态（可调把手 / 标注 / 确认或钉住）
      // 松手进调整态：截图模式出工具条；录制模式出时长面板（选框可继续拖把手/整体移动）
      adjusting.value = true
    } else {
      drag.value.active = false
    }
  }
  if (action === 'inking' && currentShape.value) {
    // 形状太小（点击）不保留
    const s = currentShape.value
    if ((s.w ?? 0) > 0.005 && (s.h ?? 0) > 0.005) {
      annotations.value.push(s)
    }
    currentShape.value = null
  }
  action = null
  resizeDirs = ''
  freeDrawing = false
}

// 选区矩形样式
function rectStyle() {
  const { x0, y0, x1, y1 } = drag.value
  // 录制时外扩 4px：红色录制框**整体画在选区矩形外侧**（border 占 -4~-5.5px），
  // GIF 只采选区矩形内部的像素，框线永远不会入画；屏幕上照常显示提示录制范围
  const o = recording.value ? 4 : 0
  const left = Math.min(x0, x1) - o
  const top = Math.min(y0, y1) - o
  const w = Math.abs(x1 - x0) + o * 2
  const h = Math.abs(y1 - y0) + o * 2
  return { left: `${left}px`, top: `${top}px`, width: `${w}px`, height: `${h}px` }
}

/** 画笔预览 polyline points：viewBox 0 0 100 100 + preserveAspectRatio=none 拉伸铺满选区，
 *  vector-effect 保线宽恒定不随拉伸变形 */
function strokePoints(pts: [number, number][]): string {
  return pts.map((p) => `${(p[0] * 100).toFixed(2)},${(p[1] * 100).toFixed(2)}`).join(' ')
}

/** 工具条贴选框下方（间距 10px）；贴近屏幕底部放不下时翻到选框上方；
 *  水平随选框左对齐、右溢出时收回视口内。随选框移动/调整实时跟随 */
const toolbarStyle = computed(() => {
  if (!adjusting.value || !drag.value.active) return { display: 'none' }
  const x0 = Math.min(drag.value.x0, drag.value.x1)
  const y0 = Math.min(drag.value.y0, drag.value.y1)
  const y1 = Math.max(drag.value.y0, drag.value.y1)
  const vw = window.innerWidth
  const vh = window.innerHeight
  const tbH = 44
  const tbW = 300 // 图标钮 + 时长档位（录制面板更宽）的估算宽
  let top = y1 + 10
  if (top + tbH > vh) top = Math.max(y0 - tbH - 10, 4)
  let left = x0
  if (left + tbW > vw - 4) left = Math.max(vw - tbW - 4, 4)
  return { top: `${top}px`, left: `${left}px` }
})

const errorMsg = ref('')
const starting = ref(false)
// 截图确认中（Rust 收窗后异步出图，期间忽略再次框选）
const capturing = ref(false)
// 录制中：选区窗口切换为「红色录制边框 + 点击穿透」，让用户始终知道在录哪里
const recording = ref(false)

/** 已定稿标注 + 拖拽预览合并（发给后端合成） */
function payloadAnnotations(): CaptureAnnotation[] {
  const out = annotations.value.filter((a) => (a.kind === 'stroke' ? (a.points?.length ?? 0) > 0 : (a.w ?? 0) > 0.005 && (a.h ?? 0) > 0.005))
  return out
}

/** 截图：region 传入时直接用（双击全屏路径），否则用当前选框 */
async function captureShot(override?: RecRegion) {
  // 双击 ▶/✓ 的第二击会赶在 Vue 重渲染 disabled 前进来：入口再挡一道，
  // 防止同区域采两次（贴图还会白占一个 slot）
  if (capturing.value) return
  let region = override ?? toPhysicalRegion()
  // 兜底：mon 未就绪（如权限/时序问题）时先重试一次同步窗口边界
  if (!region) {
    await syncMonitorBounds()
    region = toPhysicalRegion()
  }
  if (!region) {
    errorMsg.value = '请先拖拽框选截图区域'
    return
  }
  capturing.value = true
  errorMsg.value = ''
  try {
    await tauriApi.recorderShot(region, payloadAnnotations())
    // 成功：Rust 已收窗 + 复制剪贴板 + 右下角通知，这里无需处理
  } catch (e) {
    // 失败时选区层多半已被收窗（Rust 先收窗再采帧），错误以后端通知为准
    errorMsg.value = String(e)
    capturing.value = false
  }
}

/** 贴图（盯住）：同款采屏 + 复制剪贴板，图钉到选区原位置置顶展示 */
async function pinShot() {
  if (capturing.value) return
  let region = toPhysicalRegion()
  if (!region) {
    await syncMonitorBounds()
    region = toPhysicalRegion()
  }
  if (!region) {
    errorMsg.value = '请先拖拽框选截图区域'
    return
  }
  capturing.value = true
  errorMsg.value = ''
  try {
    await tauriApi.recorderPinShot(region, payloadAnnotations())
    // 成功：贴图窗口已在选区位置显示，选区层由后端收起
  } catch (e) {
    errorMsg.value = String(e)
    capturing.value = false
  }
}

/** 双击：有选框（调整态）= 确认截图；无选框 = 截取整个显示器（快捷全屏路径） */
function onDblClick(e: MouseEvent) {
  if (mode.value !== 'shot' || capturing.value || recording.value) return
  if ((e.target as HTMLElement).closest('.ro-tip, .ro-toolbar')) return
  if (adjusting.value && drag.value.active && toPhysicalRegion()) {
    void captureShot()
    return
  }
  if (mon.value) {
    void captureShot({
      x: mon.value.x,
      y: mon.value.y,
      width: mon.value.width,
      height: mon.value.height,
    })
  }
}

async function startRecord() {
  // 双击 ▶ 的第二击会赶在 Vue 重渲染 disabled 前进来：入口再挡一道，
  // 后端两段锁间隙（120ms 合成等待）可被并发双发钻空子
  if (starting.value) return
  let region = toPhysicalRegion()
  // 兜底：mon 未就绪（如权限/时序问题）时先重试一次同步窗口边界
  if (!region) {
    await syncMonitorBounds()
    region = toPhysicalRegion()
  }
  if (!region) {
    errorMsg.value = '请先拖拽框选录制区域'
    return
  }
  starting.value = true
  errorMsg.value = ''
  try {
    await tauriApi.recorderStart(region, null, maxSeconds.value)
  } catch (e) {
    errorMsg.value = String(e)
    starting.value = false
  }
}

async function cancel() {
  if (isTauri()) await tauriApi.recorderCloseOverlay()
}

/** 工具条 ✗：清掉选区回到重新框选（不关窗） */
function resetToSelect() {
  resetSelection()
  errorMsg.value = ''
  sizeText.value = ''
}

// Esc 取消（录制中忽略，停止走控制条）
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && !recording.value) {
    e.preventDefault()
    void cancel()
  }
}

let unlistenShown: (() => void) | null = null
let unlistenStarted: (() => void) | null = null
let unlistenHidden: (() => void) | null = null
let unlistenHover: (() => void) | null = null

/** 把物理像素矩形（屏幕坐标）换算为选框 CSS 坐标并 clamp 到当前显示器内 */
function applyHoverRect(r: RecRegion) {
  if (!mon.value) return
  const dpr = window.devicePixelRatio || 1
  const cssW = mon.value.width / dpr
  const cssH = mon.value.height / dpr
  const x0 = Math.min(Math.max((r.x - mon.value.x) / dpr, 0), cssW)
  const y0 = Math.min(Math.max((r.y - mon.value.y) / dpr, 0), cssH)
  const x1 = Math.min(Math.max((r.x + r.width as number - mon.value.x) / dpr, 0), cssW)
  const y1 = Math.min(Math.max((r.y + r.height as number - mon.value.y) / dpr, 0), cssH)
  if (x1 - x0 < 8 || y1 - y0 < 8) return
  drag.value = { x0, y0, x1, y1, active: true }
  // hover 预选**不进调整态**（工具条不出）：跟随态下左键单击 = 确认选中此框
  // （进调整态，再双击/✓ 确认截图）；想自定义区域直接拖拽（松手进调整态）
  updateSizeText()
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  if (!isTauri()) return
  await syncMonitorBounds()
  try {
    unlistenShown = await listen('recorder-overlay-shown', (e) => {
      const p = e.payload as {
        x: number; y: number; width: number; height: number
        mode: 'shot' | 'record'
        hover: RecRegion | null
      }
      mon.value = p
      // 重新唤起时按本次模式切换交互并重置选区状态
      mode.value = p.mode === 'shot' ? 'shot' : 'record'
      resetSelection()
      recording.value = false
      starting.value = false
      capturing.value = false
      errorMsg.value = ''
      sizeText.value = ''
      interactive.value = false
      hoverLockSent = false
      // 窗口自动识别：初始预选光标下的窗口（用户可调整/重选）
      if (p.hover) applyHoverRect(p.hover)
    })
    unlistenStarted = await listen('recorder-started', (e) => {
      const p = e.payload as { x: number; y: number; width: number; height: number }
      mon.value = p
      recording.value = true
      // 开始录制必须退出调整态：截图工具条（v-if adjusting）不退出就会残留在
      // 录制画面上（选区层点击穿透不挡操作，但会被 WGC 采进 GIF）
      adjusting.value = false
      tool.value = 'none'
      errorMsg.value = ''
    })
    // 收窗即清态（Rust hide_overlay 时发出）：窗口复用，若留着旧选框等下次 shown
    // 才清，重新唤起的瞬间会先闪出上次的框（闪框 bug）
    unlistenHidden = await listen('recorder-overlay-hidden', () => {
      resetSelection()
      recording.value = false
      starting.value = false
      capturing.value = false
      errorMsg.value = ''
      sizeText.value = ''
    })
    // hover 跟随：光标切到别的窗口时选框自动换到该窗口（首次交互前）
    unlistenHover = await listen('recorder-hover-window', (e) => {
      if (interactive.value || recording.value) return
      applyHoverRect(e.payload as RecRegion)
    })
  } catch {
    // 事件监听失败不阻塞框选/录制（窗口边界已由 syncMonitorBounds 兜底）
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  unlistenShown?.()
  unlistenStarted?.()
  unlistenHidden?.()
  unlistenHover?.()
})
</script>

<template>
  <div
    class="ro-root"
    :class="{ 'pen-mode': tool !== 'none' && adjusting && mode === 'shot', 'adjusting': adjusting && tool === 'none' && mode === 'shot' }"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @dblclick="onDblClick"
  >
    <!-- 暗色遮罩 + 光标（录制中隐藏，避免压暗画面被采进 GIF） -->
    <div v-show="!recording" class="ro-mask"></div>

    <!-- 选区边框（录制中切换为红色录制框 + REC 标识；截图调整态显示把手与画笔层） -->
    <div
      v-if="drag.active"
      class="ro-rect"
      :class="{ 'ro-rect--rec': recording, 'ro-rect--adjust': adjusting }"
      :style="rectStyle()"
    >
      <!-- 标注预览：SVG 拉伸铺满选区，归一化坐标 0..100（vector-effect 保线宽不随拉伸变形） -->
      <svg
        v-if="annotations.length || currentShape"
        class="ro-strokes"
        viewBox="0 0 100 100"
        preserveAspectRatio="none"
      >
        <template v-for="(a, i) in annotations" :key="`a${i}`">
          <polyline
            v-if="a.kind === 'stroke'"
            :points="strokePoints(a.points ?? [])"
            fill="none"
            stroke="#f54545"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
          />
          <rect
            v-else-if="a.kind === 'rect'"
            :x="((a.x ?? 0) * 100).toFixed(2)"
            :y="((a.y ?? 0) * 100).toFixed(2)"
            :width="((a.w ?? 0) * 100).toFixed(2)"
            :height="((a.h ?? 0) * 100).toFixed(2)"
            fill="none"
            stroke="#f54545"
            stroke-width="2.5"
            vector-effect="non-scaling-stroke"
          />
          <ellipse
            v-else
            :cx="(((a.x ?? 0) + (a.w ?? 0) / 2) * 100).toFixed(2)"
            :cy="(((a.y ?? 0) + (a.h ?? 0) / 2) * 100).toFixed(2)"
            :rx="(((a.w ?? 0) / 2) * 100).toFixed(2)"
            :ry="(((a.h ?? 0) / 2) * 100).toFixed(2)"
            fill="none"
            stroke="#f54545"
            stroke-width="2.5"
            vector-effect="non-scaling-stroke"
          />
        </template>
        <!-- 拖拽中的形状预览 -->
        <template v-if="currentShape">
          <rect
            v-if="currentShape.kind === 'rect'"
            :x="((currentShape.x ?? 0) * 100).toFixed(2)"
            :y="((currentShape.y ?? 0) * 100).toFixed(2)"
            :width="((currentShape.w ?? 0) * 100).toFixed(2)"
            :height="((currentShape.h ?? 0) * 100).toFixed(2)"
            fill="none"
            stroke="#f54545"
            stroke-width="2.5"
            stroke-dasharray="4 3"
            vector-effect="non-scaling-stroke"
          />
          <ellipse
            v-else-if="currentShape.kind === 'ellipse'"
            :cx="(((currentShape.x ?? 0) + (currentShape.w ?? 0) / 2) * 100).toFixed(2)"
            :cy="(((currentShape.y ?? 0) + (currentShape.h ?? 0) / 2) * 100).toFixed(2)"
            :rx="(((currentShape.w ?? 0) / 2) * 100).toFixed(2)"
            :ry="(((currentShape.h ?? 0) / 2) * 100).toFixed(2)"
            fill="none"
            stroke="#f54545"
            stroke-width="2.5"
            stroke-dasharray="4 3"
            vector-effect="non-scaling-stroke"
          />
        </template>
      </svg>
      <!-- 8 向调整把手（调整态；录制中隐藏——录制时点击穿透，把手只是视觉噪声） -->
      <template v-if="adjusting && !recording">
        <div class="ro-handle" data-h="nw"></div>
        <div class="ro-handle" data-h="n"></div>
        <div class="ro-handle" data-h="ne"></div>
        <div class="ro-handle" data-h="e"></div>
        <div class="ro-handle" data-h="se"></div>
        <div class="ro-handle" data-h="s"></div>
        <div class="ro-handle" data-h="sw"></div>
        <div class="ro-handle" data-h="w"></div>
      </template>
      <div class="ro-size" v-if="sizeText">{{ sizeText }}</div>
      <div v-if="recording" class="ro-rec-chip">
        <span class="ro-rec-dot"></span>
        REC
      </div>
    </div>

    <!-- 顶部提示条（录制中/调整态隐藏） -->
    <div v-show="!recording && !adjusting" class="ro-tip">
      <Camera v-if="mode === 'shot'" :size="16" :stroke-width="2" aria-hidden="true" />
      <Clapperboard v-else :size="16" :stroke-width="2" aria-hidden="true" />
      <span v-if="mode === 'shot'">单击选中窗口区域，拖拽自由框选，双击确认截图，Esc 取消</span>
      <span v-else>单击选中窗口区域或拖拽框选，再调整位置开始录制</span>
      <span class="ro-tip-sub">Esc 取消</span>
    </div>

    <!-- 截图调整态工具条（贴选框下方，图标钮）——仅截图模式；录制模式有自己的时长面板 -->
    <div v-if="mode === 'shot' && adjusting" class="ro-toolbar" :style="toolbarStyle">
      <button class="ro-tool" type="button" title="画笔（拖拽涂画）" :class="{ on: tool === 'pen' }" @click="tool = tool === 'pen' ? 'none' : 'pen'">
        <Pencil :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool" type="button" title="方框标注（拖拽框出）" :class="{ on: tool === 'rect' }" @click="tool = tool === 'rect' ? 'none' : 'rect'">
        <Square :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool" type="button" title="圆圈标注（拖拽框出）" :class="{ on: tool === 'ellipse' }" @click="tool = tool === 'ellipse' ? 'none' : 'ellipse'">
        <Circle :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <span class="ro-sep"></span>
      <button v-if="annotations.length" class="ro-tool" type="button" title="清除全部标注" @click="annotations = []">
        <Eraser :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool" type="button" title="重新框选" @click="resetToSelect">
        <X :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool ro-pin" type="button" title="钉住：截图钉在屏幕原位置置顶显示，同时复制剪贴板" :disabled="capturing" @click="pinShot">
        <Pin :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool ro-confirm" type="button" title="确认截图：复制到剪贴板" :disabled="capturing" @click="captureShot">
        <Check :size="15" :stroke-width="2.4" aria-hidden="true" />
      </button>
    </div>

    <!-- 控制面板（录制模式且有选区、未录制时显示，贴选框下方可随框移动）。
         帧率不在此选择：统一用设置默认值（recorder_fps）。
         时长/取消/开始录制成一排横排图标钮；跟随态（未单击）下也显示，开始即用当前跟随框 -->
    <div v-if="mode === 'record' && drag.active && !recording" class="ro-panel" :style="toolbarStyle">
      <div class="ro-seg">
        <button
          v-for="t in TIME_OPTIONS"
          :key="t"
          type="button"
          :class="{ on: maxSeconds === t }"
          @click="maxSeconds = t"
        >
          {{ t }}s
        </button>
      </div>
      <span class="ro-sep"></span>
      <button class="ro-tool" type="button" title="取消" @click="cancel">
        <X :size="15" :stroke-width="2" aria-hidden="true" />
      </button>
      <button class="ro-tool ro-confirm" type="button" title="开始录制" :disabled="starting" @click="startRecord">
        <Play :size="15" :stroke-width="2.2" aria-hidden="true" />
      </button>
      <p v-if="errorMsg" class="ro-error ro-error--toolbar">{{ errorMsg }}</p>
    </div>
  </div>
</template>

<style scoped>
.ro-root {
  position: fixed;
  inset: 0;
  cursor: crosshair;
  user-select: none;
  background: transparent;
}
.ro-root.inking,
.ro-root.pen-mode {
  cursor: crosshair;
}
/* 调整态未开标注工具：光标为移动（拖框内外任意处整体平移选区） */
.ro-root.adjusting {
  cursor: move;
}
.ro-mask {
  position: absolute;
  inset: 0;
  background: rgba(8, 10, 18, 0.45);
}
.ro-rect {
  position: absolute;
  border: 1.5px solid var(--brand-500, #5b5bf5);
  background: rgba(91, 91, 245, 0.08);
  pointer-events: none;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.35) inset;
}
/* 调整态：去掉底色只留框，画笔层与把手可见 */
.ro-rect--adjust {
  background: transparent;
}
.ro-strokes {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}
.ro-handle {
  position: absolute;
  width: 12px;
  height: 12px;
  background: #fff;
  border: 1.5px solid var(--brand-500, #5b5bf5);
  border-radius: 2px;
  pointer-events: auto;
  cursor: pointer;
}
.ro-handle[data-h='nw'] { top: -6px; left: -6px; cursor: nwse-resize; }
.ro-handle[data-h='n'] { top: -6px; left: 50%; margin-left: -6px; cursor: ns-resize; }
.ro-handle[data-h='ne'] { top: -6px; right: -6px; cursor: nesw-resize; }
.ro-handle[data-h='e'] { top: 50%; right: -6px; margin-top: -6px; cursor: ew-resize; }
.ro-handle[data-h='se'] { bottom: -6px; right: -6px; cursor: nwse-resize; }
.ro-handle[data-h='s'] { bottom: -6px; left: 50%; margin-left: -6px; cursor: ns-resize; }
.ro-handle[data-h='sw'] { bottom: -6px; left: -6px; cursor: nesw-resize; }
.ro-handle[data-h='w'] { top: 50%; left: -6px; margin-top: -6px; cursor: ew-resize; }
.ro-rect--rec {
  border: 1.5px dashed #ff5a5a;
  background: transparent;
  box-shadow: none;
}
.ro-rec-chip {
  position: absolute;
  top: -28px;
  right: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 10px;
  border-radius: 6px;
  background: #ff5a5a;
  color: #fff;
  font-size: 0.75rem;
  font-weight: 700;
  letter-spacing: 0.05em;
  white-space: nowrap;
}
.ro-rec-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #fff;
  animation: ro-rec-pulse 1.2s infinite;
}
@keyframes ro-rec-pulse {
  0% { box-shadow: 0 0 0 0 rgba(255, 255, 255, 0.6); }
  70% { box-shadow: 0 0 0 6px rgba(255, 255, 255, 0); }
  100% { box-shadow: 0 0 0 0 rgba(255, 255, 255, 0); }
}
.ro-size {
  position: absolute;
  top: -28px;
  left: 0;
  padding: 3px 8px;
  border-radius: 6px;
  background: var(--brand-500, #5b5bf5);
  color: #fff;
  font-size: 0.75rem;
  font-weight: 600;
  white-space: nowrap;
}
.ro-tip {
  position: fixed;
  top: 18px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border-radius: 10px;
  background: rgba(16, 18, 30, 0.85);
  color: #e8eaf2;
  font-size: 0.8125rem;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
}
.ro-tip-sub {
  color: rgba(232, 234, 242, 0.55);
  font-size: 0.75rem;
}
/* 截图调整态工具条（定位由 toolbarStyle 计算：贴选框下方，放不下翻上方） */
.ro-toolbar {
  position: fixed;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 6px;
  border-radius: 10px;
  background: rgba(16, 18, 30, 0.92);
  color: #e8eaf2;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
}
.ro-tool {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border-radius: 7px;
  border: none;
  background: transparent;
  color: rgba(232, 234, 242, 0.85);
  cursor: pointer;
}
.ro-tool:hover {
  background: rgba(232, 234, 242, 0.12);
}
.ro-tool.on {
  background: var(--brand-500, #5b5bf5);
  color: #fff;
}
.ro-tool:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.ro-sep {
  width: 1px;
  height: 20px;
  margin: 0 3px;
  background: rgba(232, 234, 242, 0.18);
}
.ro-confirm {
  background: var(--brand-500, #5b5bf5);
  color: #fff;
}
.ro-confirm:hover {
  filter: brightness(1.08);
  background: var(--brand-500, #5b5bf5);
}
.ro-confirm:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.ro-pin {
  color: #f5a045;
}
.ro-pin:hover {
  background: rgba(245, 160, 69, 0.18);
}
.ro-error--toolbar {
  position: fixed;
  bottom: 74px;
  left: 50%;
  transform: translateX(-50%);
  color: #ff8b8b;
  font-size: 0.75rem;
  background: rgba(16, 18, 30, 0.9);
  padding: 4px 12px;
  border-radius: 6px;
}
/* 录制控制面板：与截图工具条同款横排玻璃条（toolbarStyle 定位，贴选框下方） */
.ro-panel {
  position: fixed;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 6px;
  border-radius: 10px;
  background: rgba(16, 18, 30, 0.92);
  color: #e8eaf2;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.4);
}
.ro-seg {
  display: flex;
  gap: 4px;
}
.ro-seg button {
  min-width: 40px;
  padding: 6px 8px;
  border: 1px solid rgba(232, 234, 242, 0.18);
  border-radius: 7px;
  background: transparent;
  color: rgba(232, 234, 242, 0.85);
  font-size: 0.75rem;
  cursor: pointer;
}
.ro-seg button.on {
  background: var(--brand-500, #5b5bf5);
  border-color: var(--brand-500, #5b5bf5);
  color: #fff;
}
.ro-error {
  margin: 0;
  color: #ff8b8b;
  font-size: 0.75rem;
  text-align: right;
}
</style>
