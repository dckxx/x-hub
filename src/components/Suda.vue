<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  Bookmark,
  Check,
  ChevronLeft,
  ChevronRight,
  FilePlus,
  Globe,
  GripVertical,
  Laptop,
  ListChecks,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  ScanSearch,
  Star,
  Trash2,
  Wrench,
  X,
} from 'lucide-vue-next'
import { isTauri, tauriApi, type InstalledBrowser, type Resource, type ResourceZone } from '../api/tauri'
import { categorize } from '../utils/categories'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'
import { accentOf, fileAccentOf, iconSrc, useResourceIcon } from '../composables/useResourceIcon'
import { useAdaptivePolling } from '../composables/useAdaptivePolling'
import { useSudaDrag } from '../composables/useSudaDrag'
import { useSudaZoneDrag } from '../composables/useSudaZoneDrag'
import { isHttpWebTarget } from '../utils/web'
import { buildSubcategoryTree, categoryMatchesPath, subcatLeaf } from '../utils/subcategoryTree'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import SubcatCascadeMenu from './SubcatCascadeMenu.vue'
import SudaFormDialog from './SudaFormDialog.vue'
import SudaScanDialog, { type ScanItem, type ScanMode } from './SudaScanDialog.vue'

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)
const rootRef = ref<HTMLElement | null>(null)
const hasOverlayModal = computed(() => formVisible.value || menu.value.visible || scanVisible.value)
const { onIconError, showImageIcon, showWebFallbackIcon, iconText, fileIconOf } =
  useResourceIcon()

// ---- 拖拽导入：只预填弹窗，用户确认后才真正添加 ----
const dropping = ref(false)
// 程序解析期间的文件名（exe/lnk 解析需启动 PowerShell，期间保持遮罩提示正在识别）
const parsing = ref<string | null>(null)
const prefill = ref<{
  name?: string
  target?: string
  icon?: string | null
  kind?: 'app' | 'web' | 'file'
  category?: string | null
  isDir?: boolean
} | null>(null)
let unlistenDrop: (() => void) | null = null

onMounted(async () => {
  void installedBrowsers() // 预热浏览器列表，右键菜单即开即用
  if (!isTauri()) return
  const webview = getCurrentWebview()
  unlistenDrop = await webview.onDragDropEvent((event) => {
    const ev = event.payload
    if (hasOverlayModal.value || parsing.value) return
    if (ev.type === 'enter' || ev.type === 'over') {
      if (ev.type === 'over') return
      if (!ev.paths.length) return
      // 整个窗口均可拖拽导入：enter 即亮起全屏遮罩，
      // 释放时不再校验位置（遮罩提示居中，用户常移到提示处释放，二次校验会误丢 drop）
      dropping.value = true
    } else if (ev.type === 'leave') {
      dropping.value = false
    } else if (ev.type === 'drop') {
      dropping.value = false
      const file = ev.paths?.[0]
      if (file) {
        // 程序解析耗时较长：立刻切到“正在识别”遮罩，避免无反馈空白期
        const ext = file.split('.').pop()?.toLowerCase()
        if (ext === 'exe' || ext === 'lnk') {
          parsing.value = file.split(/[\\/]/).pop() ?? file
        }
        void handleDrop(file)
      }
    }
  })
})

onBeforeUnmount(() => {
  unlistenDrop?.()
})

// ---- 拖拽去重：目标路径与现有资源一致即视为重复（统一分隔符/大小写后比较） ----
function normalizeTarget(p: string): string {
  return p.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase()
}
function findDuplicateTarget(target: string): Resource | undefined {
  const key = normalizeTarget(target)
  return store.state.resources.find((r) => r.target && normalizeTarget(r.target) === key)
}

async function handleDrop(file: string) {
  // 命中已有资源的路径直接提示跳过：exe/lnk 还可省去 PowerShell 解析
  const direct = findDuplicateTarget(file)
  if (direct) {
    showToast(`「${direct.name}」已在速达中，已跳过重复添加`)
    return
  }
  const ext = file.split('.').pop()?.toLowerCase()
  if (ext === 'exe' || ext === 'lnk') {
    parsing.value ||= file.split(/[\\/]/).pop() ?? file
    try {
      const info = await tauriApi.parseDroppedPath(file)
      // .lnk 解析出的目标 exe 可能已用别的方式添加过（如另一个指向同一程序的快捷方式）
      const dup = findDuplicateTarget(info.target)
      if (dup) {
        showToast(`「${dup.name}」已在速达中，已跳过重复添加`)
        return
      }
      prefill.value = { name: info.name, target: info.target, icon: info.icon, kind: 'app' }
      editing.value = null
      formVisible.value = true
      showToast(`已识别「${info.name}」，请点击添加确认`)
    } catch (e) {
      showToast(String(e))
    } finally {
      parsing.value = null
    }
    return
  }
  try {
    const info = await tauriApi.inspectPath(file)
    const category = categorize(file, info.is_dir)
    prefill.value = { name: info.name, target: file, kind: 'file', category, isDir: info.is_dir }
    editing.value = null
    formVisible.value = true
    showToast(`已识别「${info.name}」，请点击添加确认`)
  } catch (e) {
    void reportClientError('速达拖拽解析失败', e)
    showToast(String(e))
  }
}

// ---- 运行状态检测：轮询进程名集合，应用已启动时名称左侧显示小绿点 ----
// 进程枚举是重量级操作（sysinfo 全量快照，单次几十毫秒）：可见且聚焦 5s、
// 失焦/滚出视口 15s 慢速档、隐藏完全停（收托盘后无意义且空烧发热），
// 从停止恢复时立即补采一轮。门控统一走 useAdaptivePolling。
const runningNames = ref<Set<string>>(new Set())
const RUNNING_ACTIVE_MS = 5000
const RUNNING_IDLE_MS = 15000

function isRunning(r: Resource): boolean {
  if (r.kind !== 'app' || !r.target) return false
  const file = r.target.split(/[\\/]/).pop()?.toLowerCase()
  return !!file && runningNames.value.has(file)
}

async function refreshRunning() {
  if (!isTauri()) return
  try {
    const names = await tauriApi.getRunningProcesses()
    runningNames.value = new Set(names)
  } catch {
    // 静默失败，下一轮重试
  }
}

useAdaptivePolling(refreshRunning, {
  activeMs: RUNNING_ACTIVE_MS,
  idleMs: RUNNING_IDLE_MS,
  viewport: rootRef,
})

// ---- 分类筛选 ----
type FilterKey = '全部' | '常用' | '应用' | '网页' | '文件'
type SubFilter = 'all' | 'none' | string

const activeFilter = ref<FilterKey>('全部')
/** 大类内的小类筛选：all=全部，none=未归类，其余为小类名（ADR 0012） */
const activeSub = ref<SubFilter>('all')

// 小类筛选行只在大类视图出现；应用/网页/文件各有自己的小类库（允许同名不同义）
const SUB_KIND: Partial<Record<FilterKey, 'app' | 'web' | 'file'>> = {
  应用: 'app',
  网页: 'web',
  文件: 'file',
}

const subTabs = computed(() => {
  const kind = SUB_KIND[activeFilter.value]
  if (!kind) return []
  return store.subcategoriesOf(kind)
})

// 小类层级树（书签导入的「/」路径 → 逐级嵌套）：chips 只出顶层，深层经级联下钻
const subTree = computed(() => buildSubcategoryTree(subTabs.value))

/** 顶级 chip 的高亮口径：精确命中或选中项在它之下（深层选中时父级 chip 也亮着） */
function isSubActive(path: string): boolean {
  return activeSub.value === path || activeSub.value.startsWith(`${path}/`)
}

// ---- 小类级联菜单开合（三角形按钮点开，选行/点外部/Esc 关闭） ----
const subMenuOpen = ref<string | null>(null)
/** 顶层菜单锚点（视口坐标，Teleport 到 body 后按 fixed 定位；打开时由触发按钮实测） */
const subMenuPos = ref({ x: 0, y: 0 })

function toggleSubMenu(path: string, e: MouseEvent) {
  if (subMenuOpen.value === path) {
    subMenuOpen.value = null
    return
  }
  const wrap = (e.currentTarget as HTMLElement).closest('.sub-chip-wrap')
  const rect = (wrap ?? (e.currentTarget as HTMLElement)).getBoundingClientRect()
  subMenuPos.value = { x: rect.left, y: rect.bottom + 5 }
  subMenuOpen.value = path
}

function selectSub(path: string) {
  activeSub.value = path
  subMenuOpen.value = null
}

function onWindowClickCloseSubMenu(e: MouseEvent) {
  // 菜单本体（Teleport 到 body）与触发按钮之外的一律关闭；
  // 菜单内自己的 mousedown 已 stop，这里的 closest 只是双保险
  if (!(e.target as HTMLElement).closest('.sub-chip-wrap, .sub-cascade')) subMenuOpen.value = null
}

function onKeydownCloseSubMenu(e: KeyboardEvent) {
  if (e.key === 'Escape') subMenuOpen.value = null
}

watch(subMenuOpen, (open) => {
  if (open) {
    window.addEventListener('mousedown', onWindowClickCloseSubMenu)
    window.addEventListener('keydown', onKeydownCloseSubMenu)
  } else {
    window.removeEventListener('mousedown', onWindowClickCloseSubMenu)
    window.removeEventListener('keydown', onKeydownCloseSubMenu)
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('mousedown', onWindowClickCloseSubMenu)
  window.removeEventListener('keydown', onKeydownCloseSubMenu)
  window.removeEventListener('resize', syncCatEdge)
})

// ---- 小类行横向滚动（行内 chips 多到溢出时右侧选不到）：滚轮竖转横 + 两端 ‹ › 按钮 ----
const catTabsRef = ref<HTMLElement | null>(null)
const catEdge = ref({ left: false, right: false })

function syncCatEdge() {
  const el = catTabsRef.value
  if (!el) return
  catEdge.value = {
    left: el.scrollLeft > 4,
    right: el.scrollLeft + el.clientWidth < el.scrollWidth - 4,
  }
}

function onCatWheel(e: WheelEvent) {
  const el = catTabsRef.value
  if (!el) return
  // 只在真的横向溢出时拦截竖直滚轮转横向滚动，避免抢占页面纵向滚动
  if (el.scrollWidth <= el.clientWidth + 4) return
  if (e.deltaY !== 0) {
    el.scrollLeft += e.deltaY
    e.preventDefault()
  }
}

/** ‹ › 滚动按钮：一次滚过约两个 chip 的宽度 */
function scrollCatBy(dx: number) {
  catTabsRef.value?.scrollBy({ left: dx, behavior: 'smooth' })
}

onMounted(() => window.addEventListener('resize', syncCatEdge))

// subTabs 是 computed（每次重算出新数组，引用变化即触发），activeFilter 是字符串，都不需要 deep
watch([activeFilter, subTabs], () => requestAnimationFrame(syncCatEdge))

// 切大类时重置小类筛选：同名不同义，跨大类沿用旧名会筛出错误集合
watch(activeFilter, () => {
  activeSub.value = 'all'
  subMenuOpen.value = null
})

function matchSub(r: Resource): boolean {
  if (activeSub.value === 'all') return true
  if (activeSub.value === 'none') return r.category == null
  // 选中目录展示其下全部资源（含子孙级，浏览器目录口径）
  return categoryMatchesPath(r.category, activeSub.value)
}

const visibleResources = computed<Resource[]>(() => {
  const all = store.state.resources
  if (activeFilter.value === '全部') return [...all]
  if (activeFilter.value === '常用') {
    return all
      .filter((r) => r.last_launched_at)
      .slice()
      .sort(
        (a, b) =>
          new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
      )
  }
  const kind = SUB_KIND[activeFilter.value]
  if (kind) return all.filter((r) => r.kind === kind && matchSub(r))
  return []
})

const FILTER_TABS: FilterKey[] = ['全部', '常用', '应用', '网页', '文件']

const emptyTitle = computed(() => {
  if (activeFilter.value === '全部') return '还没有速达资源'
  if (activeSub.value === 'none') return `暂无未归类的${activeFilter.value}`
  if (typeof activeSub.value === 'string' && activeSub.value !== 'all') {
    return `暂无「${subcatLeaf(activeSub.value)}」小类资源`
  }
  return `暂无「${activeFilter.value}」资源`
})

// ---- 长按拖拽排序（#6）：「常用」按最近使用排序，不开放手动排序 ----
const gridRef = ref<HTMLElement | null>(null)
/** v-for 里的网格用函数 ref：平铺块把元素交给平铺拖拽，分区模式下置空即可 */
function setGridRef(el: unknown) {
  gridRef.value = zoneMode.value ? null : (el as HTMLElement | null)
}
const { draggingId, dragOffset, dragOrigin, dropBeforeId, dropAtEnd, onCardPointerDown, swallowClick } =
  useSudaDrag({
    gridRef,
    items: visibleResources,
    enabled: () => activeFilter.value !== '常用' && !zoneMode.value,
    reorder: (ids) => void onReorderVisible(ids),
  })

// ---- 速达分区（「全部」tab 自定义成组陈列，独立于小类）----
interface ZoneBlock {
  /** 分区 id；null = 尾部「未分区」兜底块；'flat' = 无分区时的平铺单块 */
  key: number | string | null
  zone: ResourceZone | null
  items: Resource[]
}

/** 分区模式 = 「全部」tab 且至少建了一个分区；否则保持平铺（行为与分区功能引入前一致） */
const zoneMode = computed(
  () => activeFilter.value === '全部' && store.state.zones.length > 0,
)

/** 按分区陈列的块序列：各分区（store 序）+ 尾部未分区；区内序沿用全局 sort_order */
const zoneGroups = computed<ZoneBlock[]>(() => {
  const blocks: ZoneBlock[] = store.state.zones.map((z) => ({ key: z.id, zone: z, items: [] }))
  const byId = new Map(blocks.map((b) => [b.key as number, b]))
  const unzoned: Resource[] = []
  for (const r of store.state.resources) {
    const b = r.zone_id != null ? byId.get(r.zone_id) : undefined
    // zone_id 指向已删分区理论不该出现（delete 单事务置 NULL），本地兜底进未分区
    if (b) b.items.push(r)
    else unzoned.push(r)
  }
  return [...blocks, { key: null, zone: null, items: unzoned }]
})

/** 渲染块统一出口：分区模式 = 分区块序列；平铺模式 = 单块（现行网格不动） */
const gridBlocks = computed<ZoneBlock[]>(() =>
  zoneMode.value ? zoneGroups.value : [{ key: 'flat', zone: null, items: visibleResources.value }],
)

function zkeyOf(b: ZoneBlock): string {
  return b.key == null ? '' : String(b.key)
}

// ---- 分区模式拖拽：跨区拖 = 改归属 + 插入序，reorder_resources_zoned 单命令原子落盘 ----
const bodyRef = ref<HTMLElement | null>(null)
const zdrag = useSudaZoneDrag({
  bodyRef,
  groups: computed(() => zoneGroups.value.map((b) => ({ key: b.key as number | null, items: b.items }))),
  enabled: () => zoneMode.value,
  commit: (entries) => void store.reorderResourcesZoned(entries),
})

/** 当前生效的拖拽态（两个 composable 按模式二选一，互不同时激活） */
const draggingCardId = computed(() =>
  zoneMode.value ? zdrag.draggingId.value : draggingId.value,
)

function zoneDropBefore(r: Resource, b: ZoneBlock): boolean {
  return zdrag.dropZoneKey.value === zkeyOf(b) && zdrag.dropBeforeId.value === r.id
}

function zoneDropAtEnd(b: ZoneBlock): boolean {
  return zdrag.dropZoneKey.value === zkeyOf(b) && zdrag.dropAtZoneEnd.value
}

function zoneDropActive(b: ZoneBlock): boolean {
  return zdrag.draggingId.value != null && zdrag.dropZoneKey.value === zkeyOf(b)
}

// ---- 分区头拖拽排序：按住头部（非按钮区）移动即拖（头部不是启动目标，无需长按区分） ----
const zoneDragKey = ref<number | null>(null)
/** 插入位（分区序 0..zones.length；zones.length = 未分区之前） */
const zoneDropIndex = ref<number | null>(null)

/** 分区头拖拽的插入位：分区块横向流式并排（可换行），按「行 + 列」推插入位——
 *  指针与某块同行比左右（x 过块中心 = 在它后面），不同行比上下（y 过块中心行 = 在它后面） */
function updateZoneDrop(x: number, y: number) {
  const blocks = Array.from(
    bodyRef.value?.querySelectorAll<HTMLElement>(':scope .suda-zone') ?? [],
  ).filter((b) => b.dataset.zkey !== '') // 未分区块不在候选，仅承接 zones.length 位
  let idx = 0
  for (const b of blocks) {
    const r = b.getBoundingClientRect()
    const sameRow = y >= r.top && y <= r.bottom
    const after = sameRow ? x > (r.left + r.right) / 2 : y > (r.top + r.bottom) / 2
    if (after) idx++
  }
  zoneDropIndex.value = idx
}

function onZoneHeadPointerDown(e: PointerEvent, zoneId: number) {
  if (e.button !== 0 || batchMode.value || !zoneMode.value) return
  const target = e.target as HTMLElement | null
  if (target?.closest('button, input, a, [data-no-drag]')) return
  // 阈值取二维位移：分区块横向并排，纯横向拖动也要能起拖
  const x0 = e.clientX
  const y0 = e.clientY
  let armed = false

  const onMove = (ev: PointerEvent) => {
    if (ev.buttons === 0) {
      onUp()
      return
    }
    if (!armed) {
      if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 4) return
      armed = true
      zoneDragKey.value = zoneId
      document.body.classList.add('suda-dragging')
      document.getSelection()?.removeAllRanges()
    }
    updateZoneDrop(ev.clientX, ev.clientY)
  }

  const onUp = () => {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    if (!armed) return
    const from = store.state.zones.findIndex((z) => z.id === zoneId)
    const to = zoneDropIndex.value
    zoneDragKey.value = null
    zoneDropIndex.value = null
    document.body.classList.remove('suda-dragging')
    if (from < 0 || to == null || to === from || to === from + 1) return
    const ids = store.state.zones.map((z) => z.id)
    const [moved] = ids.splice(from, 1)
    ids.splice(to > from ? to - 1 : to, 0, moved)
    void store.reorderZones(ids)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

// ---- 分区框缩放：右下角把手拖动，尺寸以卡片格为吸附单元（宽度只会是整卡数，不会出现两卡半）；
// cols/rows 是**下限**——空框保持设定占位，内容超出按行自动膨胀（grid-auto-rows 天然生长） ----
const zoneResizing = ref<{ id: number; cols: number; rows: number } | null>(null)

/** 分区当前尺寸：缩放中用预览值，否则用库值；未分区块无尺寸（整行宽自适应） */
function zoneSizeOf(b: ZoneBlock): { cols: number; rows: number } {
  if (!b.zone) return { cols: 3, rows: 2 }
  const pv = zoneResizing.value
  if (pv && pv.id === b.zone.id) return { cols: pv.cols, rows: pv.rows }
  return { cols: b.zone.cols, rows: b.zone.rows }
}

function zoneStyleOf(b: ZoneBlock): Record<string, string> {
  if (!b.zone) return {}
  const { cols, rows } = zoneSizeOf(b)
  return { '--zone-cols': String(cols), '--zone-rows': String(rows) }
}

function onZoneResizeDown(e: PointerEvent, zone: ResourceZone) {
  if (e.button !== 0 || batchMode.value || !zoneMode.value) return
  e.preventDefault()
  const grid = (e.currentTarget as HTMLElement)
    .closest('.suda-zone')
    ?.querySelector<HTMLElement>('.suda-zone-grid')
  const body = bodyRef.value
  if (!grid || !body) return
  const rect = grid.getBoundingClientRect()
  // 与 CSS 常量同步：卡宽 124 / 行高 100 / 间距 10（.suda-card 与 .suda-zone-grid 的 --suda-row-h）
  const CELL_W = 124
  const CELL_H = 100
  const GAP = 10
  // 宽度上限 = 陈列区可用宽度能容纳的整卡数（区内边距 10×2 + 边框 1×2），别拖出横向滚动
  const maxCols = Math.max(1, Math.floor((body.clientWidth - 22 + GAP) / (CELL_W + GAP)))
  const clampCols = (n: number) => Math.max(1, Math.min(12, n, maxCols))
  const clampRows = (n: number) => Math.max(1, Math.min(12, n))
  zoneResizing.value = { id: zone.id, cols: zone.cols, rows: zone.rows }

  const onMove = (ev: PointerEvent) => {
    if (ev.buttons === 0) {
      onUp()
      return
    }
    // 吸附：宽度 n 格 = n*124+(n-1)*10 → n = round((px + 10) / 134)，行同式
    const cols = clampCols(Math.round((ev.clientX - rect.left + GAP) / (CELL_W + GAP)))
    const rows = clampRows(Math.round((ev.clientY - rect.top + GAP) / (CELL_H + GAP)))
    if (zoneResizing.value) zoneResizing.value = { id: zone.id, cols, rows }
  }

  const onUp = () => {
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    const final = zoneResizing.value
    zoneResizing.value = null
    if (final && (final.cols !== zone.cols || final.rows !== zone.rows)) {
      store.resizeZone(final.id, final.cols, final.rows).catch((err) => showToast(String(err)))
    }
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

// ---- 分区 CRUD：新建（自动命名 + 立即行内改名）/ 改名 / 上移下移 / 删除（成员落未分区，可撤销） ----
const zoneRenamingId = ref<number | null>(null)
const zoneRenameText = ref('')
const zoneRenameInput = ref<HTMLInputElement | null>(null)

function setRenameInputRef(el: unknown) {
  zoneRenameInput.value = el instanceof HTMLInputElement ? el : null
}

function startZoneRename(id: number, name: string) {
  zoneRenamingId.value = id
  zoneRenameText.value = name
  void nextTick(() => zoneRenameInput.value?.select())
}

async function commitZoneRename() {
  const id = zoneRenamingId.value
  if (id == null) return
  const name = zoneRenameText.value.trim()
  zoneRenamingId.value = null
  if (!name) return
  const cur = store.state.zones.find((z) => z.id === id)
  if (!cur || cur.name === name) return
  try {
    await store.editZone(id, name)
  } catch (e) {
    showToast(String(e))
  }
}

function cancelZoneRename() {
  zoneRenamingId.value = null
}

async function onCreateZone() {
  // 自动命名「分区 N」，建完立即进入行内改名——创建零输入、命名即时可改
  const names = new Set(store.state.zones.map((z) => z.name))
  let n = 1
  while (names.has(`分区 ${n}`)) n++
  try {
    const z = await store.addZone(`分区 ${n}`)
    startZoneRename(z.id, z.name)
  } catch (e) {
    showToast(String(e))
  }
}

function moveZoneBy(id: number, dir: -1 | 1) {
  const ids = store.state.zones.map((z) => z.id)
  const i = ids.indexOf(id)
  const j = i + dir
  if (i < 0 || j < 0 || j >= ids.length) return
  ;[ids[i], ids[j]] = [ids[j], ids[i]]
  void store.reorderZones(ids)
}

async function onDeleteZone(zone: ResourceZone) {
  const index = store.state.zones.findIndex((z) => z.id === zone.id)
  const memberIds = await store.removeZone(zone.id)
  showToast(`已删除分区「${zone.name}」，${memberIds.length} 项移入未分区`, {
    label: '撤销',
    onClick: async () => {
      try {
        const z = await store.addZone(zone.name)
        if (index >= 0) {
          const ids = store.state.zones.map((x) => x.id).filter((x) => x !== z.id)
          ids.splice(Math.min(index, ids.length), 0, z.id)
          await store.reorderZones(ids)
        }
        if (memberIds.length > 0) await store.setResourcesZone(memberIds, z.id)
        showToast('已恢复分区')
      } catch (e) {
        showToast(String(e))
      }
    },
  })
}

function zoneMenuItems(zone: ResourceZone): ContextMenuItem[] {
  return [
    { label: '重命名', onClick: () => startZoneRename(zone.id, zone.name) },
    { label: '上移', onClick: () => moveZoneBy(zone.id, -1) },
    { label: '下移', onClick: () => moveZoneBy(zone.id, 1) },
    {
      label: '删除分区（成员移入未分区）',
      dividerBefore: true,
      danger: true,
      onClick: () => void onDeleteZone(zone),
    },
  ]
}

function onZoneContext(e: MouseEvent, zone: ResourceZone) {
  e.preventDefault()
  if (batchMode.value) return
  openMenu(e, zoneMenuItems(zone))
}

function onZoneMore(e: MouseEvent, zone: ResourceZone) {
  if (batchMode.value) return
  openMenu(e, zoneMenuItems(zone))
}

// ---- 右键「移动到分区」：点开二级列表（分区 + 未分区；当前归属不出现） ----
function moveResourceToZone(r: Resource, zoneId: number | null) {
  if (r.zone_id === zoneId) return
  void store
    .setResourcesZone([r.id], zoneId)
    .then(() => {
      const name = store.state.zones.find((z) => z.id === zoneId)?.name
      showToast(zoneId == null ? '已移入未分区' : `已移入「${name}」`)
    })
    .catch((e) => showToast(String(e)))
}

/** 二级菜单沿用原右键坐标（ContextMenu 单实例换内容，视觉等同子菜单展开） */
function openZoneMoveMenu(anchor: { clientX: number; clientY: number }, r: Resource) {
  const items: ContextMenuItem[] = store.state.zones
    .filter((z) => z.id !== r.zone_id)
    .map((z) => ({ label: `移入「${z.name}」`, onClick: () => moveResourceToZone(r, z.id) }))
  if (r.zone_id != null) {
    items.push({ label: '移入「未分区」', onClick: () => moveResourceToZone(r, null) })
  }
  if (!items.length) return
  openMenu({ clientX: anchor.clientX, clientY: anchor.clientY } as MouseEvent, items)
}

/** 可见项新顺序 → 全表顺序：可见项占住它在全表里的原有槽位，其余项不动 */
function onReorderVisible(visibleIds: number[]) {
  const all = store.state.resources
  const pos = new Set(visibleIds)
  const slots: number[] = []
  all.forEach((r, i) => {
    if (pos.has(r.id)) slots.push(i)
  })
  const result = all.map((r) => r.id)
  visibleIds.forEach((id, k) => {
    const slot = slots[k]
    if (slot != null) result[slot] = id
  })
  void store.reorderResources(result)
}

/** 被拖卡片跟手飞行的位移；非拖拽态不给 inline 样式，让位给 hover 位移。
 *  卡片拖拽时是 absolute（脱离流，见 .suda-card.is-dragging），所以要先平移到原位再叠加位移。
 *  这里刻意用独立的 `translate` 属性而不是 `transform`：位移必须即时跟手（不能进过渡列表），
 *  而「浮起」的放大交给独立 `scale` 属性做短过渡 —— 两者分开，才能一个即时、一个柔和。
 *  平铺/分区两套拖拽 composable 按当前模式取各自的原位与位移（各分区网格自带
 *  position:relative，跨块拖拽的定位上下文与平铺版一致）。 */
function dragStyleOf(r: Resource) {
  const dragging = zoneMode.value ? zdrag.draggingId.value : draggingId.value
  if (dragging !== r.id) return {}
  const origin = zoneMode.value ? zdrag.dragOrigin.value : dragOrigin.value
  const offset = zoneMode.value ? zdrag.dragOffset.value : dragOffset.value
  const x = origin.x + offset.x
  const y = origin.y + offset.y
  return { translate: `${x}px ${y}px` }
}

/** 拖拽后的 click 吞掉也按模式路由（blockClick 标记在各自的 composable 里） */
function swallowCardClick(): boolean {
  return zoneMode.value ? zdrag.swallowClick() : swallowClick()
}

function onCardClick(r: Resource) {
  // 批量管理模式：点卡片 = 切勾选（不启动资源）
  if (batchMode.value) {
    toggleBatch(r.id)
    return
  }
  if (swallowCardClick()) return
  void onOpen(r)
}

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  // 必须延迟到当前事件派发结束后再置位：ContextMenu 在 window 上监听 contextmenu/click
  // 用于点击别处关闭菜单，若在同一事件派发内同步置位，紧跟的全局关闭监听会在
  // props 更新后立即把菜单关掉（表现为右键无反应）；已开时右键另一资源也无法重定位
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

async function onDeleteResource(r: Resource) {
  await store.removeResource(r.id)
  showToast(`已删除「${r.name}」`, {
    label: '撤销',
    onClick: async () => {
      await store.addResource({
        kind: r.kind,
        name: r.name,
        target: r.target,
        category: r.category,
        icon: r.icon,
        args: r.args,
        // 所属分区可能已被删除：按现存分区兜底，避免恢复时因分区不存在而失败
        zoneId: store.state.zones.some((z) => z.id === r.zone_id) ? r.zone_id : null,
      })
      showToast('已恢复')
    },
  })
}

// ---- 批量管理：勾选 + 批量删除（配合大类/小类筛选形成「筛出一类 → 全选 → 删」的清理流）----
const batchMode = ref(false)
const batchChecked = ref<Set<number>>(new Set())
const batchDeleting = ref(false)
const batchConfirmVisible = ref(false)

function enterBatchMode() {
  batchMode.value = true
  batchChecked.value = new Set()
}

function exitBatchMode() {
  batchMode.value = false
  batchChecked.value = new Set()
}

function toggleBatch(id: number) {
  const next = new Set(batchChecked.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  batchChecked.value = next
}

/** 当前筛选下可见资源是否已全勾（决定「全选/全不选」按钮文案） */
const batchAllChecked = computed(
  () =>
    visibleResources.value.length > 0 &&
    visibleResources.value.every((r) => batchChecked.value.has(r.id)),
)

/** 全选/全不选当前筛选下可见的资源（勾选集里可能含被筛走的，全不选一并清掉） */
function batchToggleAll() {
  batchChecked.value = batchAllChecked.value
    ? new Set()
    : new Set(visibleResources.value.map((r) => r.id))
}

async function doBatchDelete() {
  if (batchDeleting.value) return
  // 先落快照：删除过程会改 store.state.resources，边删边读集合会漏
  const snapshot = store.state.resources.filter((r) => batchChecked.value.has(r.id))
  if (snapshot.length === 0) return
  batchDeleting.value = true
  // 并行删除（后端命令经 DbState 互斥锁串行落库，前端各续体只做原子状态更新）；
  // 快照在启动前已落好，不存在边删边读集合的问题
  const results = await Promise.allSettled(snapshot.map((r) => store.removeResource(r.id)))
  const deleted = results.filter((x) => x.status === 'fulfilled').length
  for (const x of results) {
    if (x.status === 'rejected') void reportClientError('批量删除资源失败', x.reason)
  }
  batchDeleting.value = false
  batchChecked.value = new Set()
  showToast(`已删除 ${deleted} 个资源`, {
    label: '撤销',
    onClick: async () => {
      let restored = 0
      for (const r of snapshot) {
        try {
          await store.addResource({
            kind: r.kind,
            name: r.name,
            target: r.target,
            category: r.category,
            icon: r.icon,
            args: r.args,
            zoneId: store.state.zones.some((z) => z.id === r.zone_id) ? r.zone_id : null,
          })
          restored++
        } catch {
          // 目标已重新存在等，跳过
        }
      }
      showToast(`已恢复 ${restored} 个`)
    },
  })
}

/** 卡片键盘激活（回车/空格）：批量模式切勾选，正常模式打开 */
function onCardKeyActivate(r: Resource) {
  if (batchMode.value) toggleBatch(r.id)
  else void onOpen(r)
}

/** 批量模式下禁用卡片拖拽排序（拖拽手势与点选冲突），只保留点击选择；
 *  分区模式路由到分区拖拽（跨区改归属），平铺模式路由到平铺拖拽（仅重排序） */
function onCardPointer(r: Resource, e: PointerEvent) {
  if (batchMode.value) return
  if (zoneMode.value) zdrag.onCardPointerDown(r, e)
  else onCardPointerDown(r, e)
}

// ---- 指定浏览器打开（网页资源）：列表来自本机已安装浏览器（Rust 注册表枚举） ----
let browserCache: InstalledBrowser[] | null = null

async function installedBrowsers(): Promise<InstalledBrowser[]> {
  if (browserCache === null) {
    try {
      browserCache = isTauri() ? await tauriApi.listInstalledBrowsers() : []
    } catch {
      browserCache = []
    }
  }
  return browserCache
}

async function onOpenWithBrowser(r: Resource, b: InstalledBrowser) {
  try {
    await store.openResourceInBrowser(r.id, b.exe)
  } catch (e) {
    showToast(`无法用「${b.name}」打开：${String(e)}`)
  }
}

async function onOpenInWindow(r: Resource) {
  try {
    await store.openResourceInWindow(r.id)
  } catch (e) {
    showToast(String(e))
  }
}

/** 以管理员身份运行（UAC 确认）：仅「程序」资源有提权语义 */
async function onOpenAsAdmin(r: Resource) {
  try {
    await store.launchResourceAsAdmin(r.id)
  } catch (e) {
    showToast(`无法以管理员身份运行「${r.name}」：${String(e)}`)
  }
}

async function onResourceContext(e: MouseEvent, r: Resource) {
  e.preventDefault()
  // 批量管理模式下不弹单条菜单（避免「打开/编辑」在勾选语境里误触）
  if (batchMode.value) return
  const items: ContextMenuItem[] = [{ label: '打开', onClick: () => onOpen(r) }]
  const isApp = r.kind === 'app'
  if (isApp) {
    items.push({ label: '以管理员身份运行', onClick: () => void onOpenAsAdmin(r) })
  }
  // smb/ftp 等远程协议只有系统能打开：内嵌面板/独立窗口/指定浏览器入口只对 http(s) 出
  let isWeb = false
  if (r.kind === 'web') {
    isWeb = true
    if (isHttpWebTarget(r.target)) {
      // 显式覆盖默认打开方式（默认方式见 设置 → 功能 → 速达）
      items.push({ label: '在内嵌面板打开', dividerBefore: true, onClick: () => store.openWebPanel(r.id) })
      items.push({ label: '在独立窗口打开', onClick: () => void onOpenInWindow(r) })
      const browsers = await installedBrowsers()
      for (const b of browsers) {
        items.push({ label: `用 ${b.name} 打开`, onClick: () => void onOpenWithBrowser(r, b) })
      }
    }
  }
  // 移动到分区：只在建了分区后出现（当前归属不进列表；点开二级菜单沿用本坐标）
  if (store.state.zones.length > 0) {
    items.push({
      label: '移动到分区',
      dividerBefore: isWeb,
      onClick: () => openZoneMoveMenu(e, r),
    })
  }
  items.push({
    label: '编辑',
    dividerBefore: isWeb && store.state.zones.length === 0,
    onClick: () => {
      editing.value = r
      formVisible.value = true
    },
  })
  items.push({
    label: '删除',
    danger: true,
    onClick: () => void onDeleteResource(r),
  })
  openMenu(e, items)
}

// ---- 弹窗 ----
const formVisible = ref(false)
const editing = ref<Resource | null>(null)

async function onOpen(r: Resource) {
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法打开「${r.name}」：${String(e)}`)
  }
}

/** 后台抓取网页图标（favicon）并回填资源：导入书签 / 新增网页后自动补齐站点图标。
 *  抓取不阻塞导入（可能几十个域名、每个最长 8s）；回填前先确认资源还在且仍无图标，
 *  避免覆盖用户这期间的删除/改图标 */
async function fillWebFavicons(created: Resource[]) {
  if (!isTauri() || created.length === 0) return
  // smb/ftp 等远程协议没有站点图标语义，跳过抓取
  const targets = created.filter((r) => isHttpWebTarget(r.target))
  if (targets.length === 0) return
  let map: Record<string, string | null>
  try {
    map = await tauriApi.fetchFavicons(targets.map((r) => r.target))
  } catch (e) {
    void reportClientError('抓取网页图标失败', e)
    return
  }
  // 回填前先确认资源还在且仍无图标，避免覆盖用户这期间的删除/改图标；
  // 各任务目标互不相同且守卫在启动前同步完成，并行安全
  const jobs = targets.flatMap((r) => {
    const icon = map[r.target]
    if (!icon) return []
    const cur = store.state.resources.find((x) => x.id === r.id)
    if (!cur || cur.icon) return []
    return [
      store.editResource({
        id: cur.id,
        kind: cur.kind,
        name: cur.name,
        target: cur.target,
        category: cur.category,
        icon,
        args: cur.args,
        // update 是全对象写：不带 zoneId 会把归属抹成未分区
        zoneId: cur.zone_id,
      }),
    ]
  })
  const results = await Promise.allSettled(jobs)
  const got = results.filter((x) => x.status === 'fulfilled').length
  for (const x of results) {
    if (x.status === 'rejected') void reportClientError('回填网页图标失败', x.reason)
  }
  if (got > 0) showToast(`已获取 ${got} 个网页图标`)
}

/** 打开「添加」弹窗：已在大类视图（应用/网页/文件）时自动预选该大类——
 *  正在筛某个小类就连小类一起带上，新增的资源直接落进当前分类里 */
function openAddForm() {
  editing.value = null
  const kind = SUB_KIND[activeFilter.value]
  prefill.value = kind
    ? {
        kind,
        category:
          activeSub.value !== 'all' && activeSub.value !== 'none' ? activeSub.value : null,
      }
    : null
  formVisible.value = true
}

function onFormSubmit(payload: {
  id?: number
  kind: 'app' | 'web' | 'file'
  name: string
  target: string
  category?: string | null
  icon?: string | null
  args?: string | null
  zoneId?: number | null
}) {
  if (payload.id != null) {
    void store.editResource({ ...payload, id: payload.id })
    showToast(`已更新「${payload.name}」`)
  } else {
    void store.addResource(payload).then((r) => {
      showToast(`已添加「${payload.name}」`)
      // 新增网页且没配图标：后台抓 favicon 自动补齐（远程协议 smb/ftp 无站点图标）
      if (r.kind === 'web' && !r.icon && isHttpWebTarget(r.target)) void fillWebFavicons([r])
    })
  }
  prefill.value = null
}

// ---- 扫描导入（已安装应用 / 桌面 / 浏览器书签）----
const scanVisible = ref(false)
const scanMode = ref<ScanMode>('apps')
const importing = ref(false)

function openScan(mode: ScanMode) {
  scanMode.value = mode
  scanVisible.value = true
}

async function onScanImported(items: ScanItem[], cleanShortcuts = false) {
  if (importing.value) return
  importing.value = true
  let added = 0
  let skipped = 0
  const importedSources: string[] = []
  // 导入成功的网页类资源（书签 / 桌面 .url）：导入完成后后台补抓站点图标
  const createdWeb: Resource[] = []
  // 书签按文件夹归类：小类筛选 chips 以 subcategory 表为准，先补建缺失的 web 小类，
  // 否则资源挂着表里不存在的小类名，在任何小类 chip 和「未归类」下都筛不出来
  let createdCats = 0
  const wantedCats = new Set(
    items.filter((a) => a.kind === 'web' && a.category).map((a) => a.category as string),
  )
  for (const name of wantedCats) {
    if (store.subcategoriesOf('web').some((s) => s.name === name)) continue
    try {
      await store.addSubcategory('web', name)
      createdCats++
    } catch (e) {
      // DUP = 表里已有同名行（列表短暂过期等），归类按名字匹配、无需处理
      if (!String(e).includes('DUP')) void reportClientError('创建书签小类失败', e)
    }
  }
  for (const a of items) {
    // 二次去重保护：目标路径已存在则跳过（弹窗中已禁用，这里兜底）
    if (store.state.resources.some((r) => r.target.toLowerCase() === a.target.toLowerCase())) {
      skipped++
      continue
    }
    try {
      const r = await store.addResource({
        // 桌面上的「文件夹」在速达里没有独立大类，归入「文件」
        kind: a.kind === 'folder' ? 'file' : a.kind,
        name: a.name,
        target: a.target,
        // 书签导入可按浏览器文件夹带小类（弹窗里勾选决定）；其余模式保持默认归类
        category: a.kind === 'web' ? (a.category ?? null) : null,
        icon: a.icon,
        args: null,
        // 导入的资源统一落未分区（用户可再拖进分区）
        zoneId: null,
      })
      added++
      if (r.kind === 'web' && !r.icon) createdWeb.push(r)
      if (a.source) importedSources.push(a.source)
    } catch (e) {
      void reportClientError('速达扫描导入失败', e)
    }
  }
  // 桌面模式可选：导入成功后清理桌面上的 .lnk/.url（Rust 侧有「仅快捷方式 + 仅用户桌面」护栏）
  let cleaned = 0
  if (cleanShortcuts && importedSources.length > 0) {
    try {
      cleaned = await tauriApi.deleteDesktopShortcuts(importedSources)
    } catch (e) {
      void reportClientError('清理桌面快捷方式失败', e)
    }
  }
  importing.value = false
  const parts = [`已添加 ${added} 项`]
  if (skipped > 0) parts.push(`跳过 ${skipped} 项已存在`)
  if (createdCats > 0) parts.push(`新建小类 ${createdCats} 个`)
  if (cleaned > 0) parts.push(`清理桌面快捷方式 ${cleaned} 个`)
  showToast(parts.join('，'))
  // 网页类资源后台补抓站点图标（不阻塞上面的导入反馈，抓到后自动刷新卡片）
  if (createdWeb.length > 0) void fillWebFavicons(createdWeb)
}

// ---- 图标渲染（统一在 useResourceIcon composable） ----

function kindLabel(r: Resource): string {
  // 有小类显示小类名（应用/网页/文件统一），否则回退大类名；
  // 层级小类（书签导入的「开发/前端」）只显示末级，完整路径放卡片 title 里备查
  return r.category
    ? subcatLeaf(r.category)
    : r.kind === 'app'
      ? '应用'
      : r.kind === 'web'
        ? '网页'
        : '文件'
}

function cardAccentStyle(r: Resource) {
  if (r.kind === 'file') {
    const a = fileAccentOf(r.category ?? '其他')
    return {
      '--suda-accent-soft': a.soft,
      '--suda-accent': a.strong,
      '--suda-accent-ink': a.ink,
    }
  }
  const a = accentOf(r.name)
  return {
    '--suda-accent-soft': a.soft,
    '--suda-accent': a.strong,
    '--suda-accent-ink': a.text,
  }
}
</script>

<template>
  <section ref="rootRef" class="card suda">
    <header class="suda-header">
      <h2 class="suda-title">速达</h2>
      <div class="suda-header-actions">
        <!-- 批量管理模式：已选计数 + 全选 + 删除 + 退出 -->
        <template v-if="batchMode">
          <span class="suda-batch-count">已选 {{ batchChecked.size }} 项</span>
          <button class="ghost-btn suda-batch-btn" @click="batchToggleAll">
            {{ batchAllChecked ? '全不选' : '全选' }}
          </button>
          <button
            class="ghost-btn suda-batch-btn danger"
            :disabled="batchChecked.size === 0 || batchDeleting"
            @click="batchConfirmVisible = true"
          >
            <Loader2 v-if="batchDeleting" :size="13" :stroke-width="2" class="spin" />
            删除
          </button>
          <button
            class="icon-btn"
            title="退出批量管理"
            aria-label="退出批量管理"
            @click="exitBatchMode"
          >
            <X :size="15" :stroke-width="2.2" />
          </button>
        </template>
        <template v-else>
          <button
            v-if="visibleResources.length > 0"
            class="icon-btn scan"
            title="批量管理（勾选后可批量删除，配合分类筛选更方便）"
            aria-label="批量管理"
            @click="enterBatchMode"
          >
            <ListChecks :size="15" :stroke-width="2.2" />
          </button>
          <button
            v-if="isTauri()"
            class="icon-btn scan"
            title="扫描已安装应用"
            aria-label="扫描已安装应用"
            @click="openScan('apps')"
          >
            <ScanSearch :size="15" :stroke-width="2.2" />
          </button>
          <button
            v-if="isTauri()"
            class="icon-btn scan"
            title="扫描桌面（用户桌面一层，不递归）"
            aria-label="扫描桌面"
            @click="openScan('desktop')"
          >
            <Laptop :size="15" :stroke-width="2.2" />
          </button>
          <button
            v-if="isTauri()"
            class="icon-btn scan"
            title="导入浏览器书签（Chrome / Edge / Brave / Chromium）"
            aria-label="导入浏览器书签"
            @click="openScan('bookmarks')"
          >
            <Bookmark :size="15" :stroke-width="2.2" />
          </button>
          <button
            class="icon-btn add"
            title="添加"
            @click="openAddForm"
          >
            <Plus :size="15" :stroke-width="2.2" />
          </button>
        </template>
      </div>
    </header>

    <!-- 分类 tabs + 「全部」分区入口 -->
    <div class="suda-tabs-row">
      <nav class="filter-tabs suda-tabs" aria-label="速达分类">
        <button
          v-for="f in FILTER_TABS"
          :key="f"
          class="filter-tab filter-tab--primary"
          :class="{ active: activeFilter === f }"
          @click="activeFilter = f"
        >
          {{ f }}
        </button>
      </nav>
      <button
        v-if="activeFilter === '全部'"
        class="suda-zone-add"
        type="button"
        title="新建分区：「全部」里按分区成组陈列，把资源拖进分区即可归类"
        @click="onCreateZone"
      >
        <Plus :size="12" :stroke-width="2.4" />
        分区
      </button>
    </div>

<!-- 大类小类筛选（ADR 0012）：应用/网页/文件各有小类库；未归类=category 为空。
     书签导入的小类带「/」层级：chips 只出顶层，有下级的带 ▸ 展开级联菜单逐级选择，
     选中某级 = 展示该目录下（含子级）全部资源。小类多到溢出时两端出现 ‹ › 滚动按钮，
     滚轮竖向滑动也可横滚该行 -->
<div
  v-if="SUB_KIND[activeFilter]"
  class="suda-cat-wrap"
  :class="{ 'can-left': catEdge.left, 'can-right': catEdge.right }"
>
  <button
    v-if="catEdge.left"
    class="cat-scroll-btn"
    type="button"
    aria-label="小类列表向左滚动"
    title="还有左侧小类"
    @click="scrollCatBy(-180)"
  >
    <ChevronLeft :size="12" :stroke-width="2.2" />
  </button>
  <nav
    ref="catTabsRef"
    class="filter-tabs suda-cat-tabs"
    aria-label="小类筛选"
    @wheel="onCatWheel"
    @scroll.passive="syncCatEdge"
  >
  <button
    class="filter-tab filter-tab--tag"
    :class="{ active: activeSub === 'all' }"
    @click="activeSub = 'all'"
  >
    全部{{ activeFilter }}
  </button>
  <button
    class="filter-tab filter-tab--tag"
    :class="{ active: activeSub === 'none' }"
    @click="activeSub = 'none'"
  >
    未归类
  </button>
  <span v-for="node in subTree" :key="node.path" class="sub-chip-wrap">
    <button
      class="filter-tab filter-tab--tag"
      :class="{ active: isSubActive(node.path) }"
      :title="node.path"
      @click="selectSub(node.path)"
    >
      <Star
        v-if="node.isDefault"
        class="sub-default-star"
        :size="10"
        :stroke-width="2.4"
        title="默认小类：新增资源未指定小类时自动归入；删除小类时条目也改挂到这里（在 设置 → 功能 → 小类管理 更换）"
        aria-hidden="true"
      />
      {{ node.name }}
    </button>
    <button
      v-if="node.children.length"
      class="sub-chip-arrow"
      :class="{ open: subMenuOpen === node.path }"
      type="button"
      :title="`展开「${node.name}」的下级小类`"
      :aria-label="`展开「${node.name}」的下级小类`"
      :aria-expanded="subMenuOpen === node.path"
      @click.stop="toggleSubMenu(node.path, $event)"
    >
      <ChevronRight :size="11" :stroke-width="2.4" />
    </button>
    <SubcatCascadeMenu
      v-if="node.children.length && subMenuOpen === node.path"
      teleport
      :nodes="node.children"
      :active-path="activeSub"
      :x="subMenuPos.x"
      :y="subMenuPos.y"
      @select="selectSub($event)"
    />
  </span>
  </nav>
  <button
    v-if="catEdge.right"
    class="cat-scroll-btn"
    type="button"
    aria-label="小类列表向右滚动"
    title="还有更多小类"
    @click="scrollCatBy(180)"
  >
    <ChevronRight :size="12" :stroke-width="2.2" />
  </button>
</div>

    <!-- 资源陈列：分区模式 = 分区块横向流式并排（新建往右长、放不下换行）+ 尾部整行「未分区」；
         无分区 = 单块平铺（行为与分区功能引入前一致） -->
    <div ref="bodyRef" class="suda-body">
      <div v-if="zoneMode || visibleResources.length > 0" class="suda-zone-flow">
        <template v-for="(g, gi) in gridBlocks" :key="g.key ?? 'unzoned'">
          <!-- 分区头拖拽排序的插入线（落点=某分区之前；zones.length 位 = 未分区之前） -->
          <div
            v-if="zoneDragKey != null && zoneDropIndex === gi"
            class="suda-zone-insert"
            aria-hidden="true"
          />
          <section
            class="suda-zone"
            :class="{
              flat: !zoneMode,
              'unzoned-block': zoneMode && g.zone == null,
              'is-drop-target': zoneMode && zoneDropActive(g),
              'is-zone-dragging': zoneDragKey === g.key,
              'is-resizing': zoneResizing != null && zoneResizing.id === g.zone?.id,
            }"
            :style="zoneStyleOf(g)"
            :data-zkey="zkeyOf(g)"
          >
            <header
              v-if="zoneMode"
              class="suda-zone-head"
              @pointerdown="g.zone ? onZoneHeadPointerDown($event, g.zone.id) : undefined"
              @contextmenu.prevent="g.zone ? onZoneContext($event, g.zone) : undefined"
            >
              <GripVertical
                v-if="g.zone && !batchMode"
                class="suda-zone-grip"
                :size="14"
                :stroke-width="2"
                aria-hidden="true"
              />
              <template v-if="g.zone">
                <input
                  v-if="zoneRenamingId === g.zone.id"
                  :ref="setRenameInputRef"
                  v-model="zoneRenameText"
                  class="suda-zone-rename"
                  type="text"
                  maxlength="20"
                  placeholder="分区名称"
                  @keydown.enter.prevent="commitZoneRename"
                  @keydown.esc.stop.prevent="cancelZoneRename"
                  @blur="commitZoneRename"
                  @pointerdown.stop
                  @click.stop
                />
                <span
                  v-else
                  class="suda-zone-name"
                  title="双击重命名；按住分区头拖动可调整分区顺序"
                  @dblclick="startZoneRename(g.zone.id, g.zone.name)"
                >
                  {{ g.zone.name }}
                </span>
                <span class="suda-zone-count">{{ g.items.length }}</span>
                <button
                  v-if="!batchMode"
                  class="suda-zone-more"
                  type="button"
                  title="分区操作"
                  aria-label="分区操作"
                  @click.stop="onZoneMore($event, g.zone)"
                >
                  <MoreHorizontal :size="14" :stroke-width="2" />
                </button>
              </template>
              <template v-else>
                <span class="suda-zone-name">未分区</span>
                <span class="suda-zone-count">{{ g.items.length }}</span>
              </template>
            </header>
            <div :ref="setGridRef" class="suda-grid" :class="{ 'suda-zone-grid': zoneMode }">
              <template v-for="r in g.items" :key="r.id">
                <div
                  v-if="!zoneMode && dropBeforeId === r.id"
                  class="suda-drop-slot"
                  aria-hidden="true"
                />
                <div
                  v-if="zoneMode && zoneDropBefore(r, g)"
                  class="suda-drop-slot"
                  aria-hidden="true"
                />
                <div
                  class="suda-card"
                  :class="{
                    'is-dragging': draggingCardId === r.id,
                    'batch-on': batchMode,
                    selected: batchMode && batchChecked.has(r.id),
                  }"
                  :data-id="r.id"
                  :title="r.target"
                  role="button"
                  tabindex="0"
                  :aria-pressed="batchMode ? batchChecked.has(r.id) : undefined"
                  :style="[cardAccentStyle(r), dragStyleOf(r)]"
                  @click="onCardClick(r)"
                  @pointerdown="onCardPointer(r, $event)"
                  @dragstart.prevent
                  @keydown.enter="onCardKeyActivate(r)"
                  @keydown.space.prevent="onCardKeyActivate(r)"
                  @contextmenu="onResourceContext($event, r)"
                >
                  <span v-if="batchMode" class="suda-batch-check" :class="{ on: batchChecked.has(r.id) }">
                    <Check v-if="batchChecked.has(r.id)" :size="12" :stroke-width="3" />
                  </span>
                  <span class="suda-kind" :class="r.kind" :title="r.category ?? kindLabel(r)">{{
                    kindLabel(r)
                  }}</span>
                  <div class="suda-actions">
                    <button
                      class="suda-action"
                      title="编辑"
                      aria-label="编辑"
                      @click.stop="editing = r; formVisible = true"
                    >
                      <Pencil :size="11" :stroke-width="2" />
                    </button>
                    <button
                      class="suda-action del"
                      title="删除"
                      aria-label="删除"
                      @click.stop="onDeleteResource(r)"
                    >
                      <Trash2 :size="11" :stroke-width="2" />
                    </button>
                  </div>
                  <div
                    class="suda-icon"
                    :class="{ 'web-default': showWebFallbackIcon(r) }"
                    :style="
                      showImageIcon(r)
                        ? {}
                        : { background: 'var(--suda-accent-soft)' }
                    "
                  >
                    <img
                      v-if="showImageIcon(r)"
                      class="suda-img"
                      :src="iconSrc(r.icon!)"
                      alt=""
                      draggable="false"
                      @error="onIconError(r)"
                    />
                    <Globe
                      v-else-if="showWebFallbackIcon(r)"
                      class="suda-file-icon"
                      :size="25"
                      :stroke-width="1.7"
                      :style="{ color: 'var(--c-green-ink)' }"
                    />
                    <component
                      v-else-if="r.kind === 'file'"
                      :is="fileIconOf(r)"
                      class="suda-file-icon"
                      :size="25"
                      :stroke-width="1.7"
                      :style="{ color: 'var(--suda-accent)' }"
                    />
                    <span
                      v-else
                      class="suda-letter"
                      :style="{ color: 'var(--suda-accent-ink)' }"
                    >
                      {{ iconText(r) }}
                    </span>
                  </div>
                  <span class="suda-name">
                    <span v-if="isRunning(r)" class="suda-dot" title="运行中" />
                    <span class="suda-name-text" :title="r.name">{{ r.name }}</span>
                  </span>
                </div>
              </template>
              <div v-if="!zoneMode && dropAtEnd" class="suda-drop-slot" aria-hidden="true" />
              <div v-if="zoneMode && zoneDropAtEnd(g)" class="suda-drop-slot" aria-hidden="true" />
            </div>
            <p v-if="zoneMode && g.items.length === 0" class="suda-zone-empty">
              {{ g.zone ? '拖动资源到这里' : '未分区的资源会显示在这里' }}
            </p>
            <!-- 尺寸把手：拖动按整卡格吸附（宽度只会是整卡数）；cols/rows 为下限，内容超出自动按行膨胀 -->
            <span
              v-if="g.zone && !batchMode && zoneMode"
              class="suda-zone-resize"
              title="拖动调整分区大小（按卡片格吸附）"
              @pointerdown="onZoneResizeDown($event, g.zone)"
            />
          </section>
        </template>
      </div>

      <div v-else class="empty-state">
        <Wrench :size="24" :stroke-width="1.7" aria-hidden="true" />
        <p>{{ emptyTitle }}</p>
        <p style="font-size: 0.75rem; color: var(--text-4)">
          拖拽本地文件/程序到窗口，或手动添加快捷链接
        </p>
        <button
          class="pill-btn"
          style="margin-top: 6px"
          @click="openAddForm"
        >
          添加
        </button>
      </div>
    </div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />
    <SudaFormDialog
      :visible="formVisible"
      :editing="editing"
      :prefill="prefill"
      @close="formVisible = false"
      @submit="onFormSubmit"
    />
    <SudaScanDialog
      :visible="scanVisible"
      :mode="scanMode"
      @close="scanVisible = false"
      @imported="onScanImported"
    />
    <!-- 批量删除二次确认（多选的破坏性操作必须有明确的确认步，撤销 toast 只是兜底） -->
    <ConfirmDialog
      :visible="batchConfirmVisible"
      title="批量删除资源"
      :message="`确定删除选中的 ${batchChecked.size} 个资源吗？`"
      hint="只删除速达里的条目，不影响磁盘上的文件；删除后可在提示条里撤销恢复。"
      confirm-text="删除"
      tone="danger"
      @confirm="batchConfirmVisible = false; void doBatchDelete()"
      @cancel="batchConfirmVisible = false"
    />

    <!-- 拖拽导入遮罩（dropping = 拖拽中；parsing = 正在识别程序） -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping || parsing" class="drop-overlay">
          <div class="drop-hint">
            <Loader2 v-if="parsing" :size="34" :stroke-width="1.5" class="spin" />
            <FilePlus v-else :size="34" :stroke-width="1.5" />
            <p v-if="parsing">正在识别…</p>
            <p v-else>释放以添加</p>
            <span v-if="parsing" :title="parsing">{{ parsing }}</span>
            <span v-else>支持本地程序 / 网页 / 任意文件或文件夹</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.suda {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
  min-height: 0;
}
.suda-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.suda-title {
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.suda-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.icon-btn.scan {
  width: 30px;
  height: 30px;
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.icon-btn.scan:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

/* tabs 行：nav + 「分区」入口同排 */
.suda-tabs-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.suda-tabs-row .suda-tabs {
  margin-bottom: 0;
}
.suda-zone-add {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 1px dashed var(--border-strong);
  background: transparent;
  border-radius: var(--radius-pill);
  padding: 4px 12px;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-3);
  cursor: pointer;
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}
.suda-zone-add:hover {
  border-color: var(--brand-500);
  border-style: solid;
  color: var(--brand-500);
  background: var(--brand-50);
}

/* ---- 分区块（「全部」tab 成组陈列）：横向流式并排（新建往右长、放不下换行）；
        未分区与平铺块占满整行；flat = 平铺模式的无框单块 ---- */
.suda {
  /* 分区网格的行高单元（卡片自然高度 98px 上取整留 2px 余量）：行数尺寸以它为基准 */
  --suda-row-h: 100px;
}
.suda-zone-flow {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  gap: 14px;
}
.suda-zone {
  position: relative;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  padding: 8px 10px 10px;
  /* 框宽贴合 cols 列内容（不会整行铺满留一片空框）；未分区块另行放开为整行宽 */
  width: fit-content;
  max-width: 100%;
  transition: border-color 0.15s, background 0.15s, opacity 0.15s;
}
.suda-zone.flat {
  background: transparent;
  border-color: transparent;
  border-radius: 0;
  padding: 0;
  width: 100%;
}
/* 未分区：兜底位不做蒙版填充（比分区更透亮），虚线描边 + 弱化标题与分区拉开区分；
   占满整行（flex-wrap 下自然独占最后一行） */
.suda-zone.unzoned-block {
  background: transparent;
  border: 1.5px dashed var(--border-strong);
  width: 100%;
}
.suda-zone.unzoned-block .suda-zone-name {
  color: var(--text-3);
  font-weight: 500;
}
.suda-zone.unzoned-block .suda-zone-count {
  background: transparent;
  border: 1px solid var(--border-soft);
}
/* 分区网格按格定宽定行：cols 列 × rows 行为**下限**占位（min-height 兜底空框），
   内容超出时 grid-auto-rows 自然生长 = 自动膨胀；吸附单元见 onZoneResizeDown 常量 */
.suda-zone:not(.unzoned-block):not(.flat) .suda-zone-grid {
  grid-template-columns: repeat(var(--zone-cols, 3), 124px);
  grid-auto-rows: var(--suda-row-h);
  min-height: calc(var(--zone-rows, 2) * (var(--suda-row-h) + 10px) - 10px);
  justify-content: start;
}
/* 缩放进行中不做过渡，尺寸跟手即时 */
.suda-zone.is-resizing {
  transition: none;
}
.suda-zone.is-resizing .suda-zone-grid {
  transition: none;
}
/* 尺寸把手：右下角斜纹小方块（悬停分区时更明显），nwse 方向光标 */
.suda-zone-resize {
  position: absolute;
  right: 0;
  bottom: 0;
  width: 18px;
  height: 18px;
  display: flex;
  align-items: flex-end;
  justify-content: flex-end;
  cursor: nwse-resize;
  opacity: 0;
  transition: opacity 0.15s;
  z-index: 5;
}
.suda-zone:hover .suda-zone-resize,
.suda-zone-resize:focus-visible {
  opacity: 1;
}
.suda-zone-resize::after {
  content: '';
  width: 10px;
  height: 10px;
  margin: 0 3px 3px 0;
  border-radius: 2px;
  background: repeating-linear-gradient(
    135deg,
    transparent 0 3px,
    var(--text-4) 3px 4px
  );
}
.suda-zone-resize:hover::after {
  background: repeating-linear-gradient(
    135deg,
    transparent 0 3px,
    var(--brand-500) 3px 4px
  );
}
.suda-zone-head {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
  user-select: none;
  min-height: 26px;
}
.suda-zone.flat .suda-zone-head {
  display: none;
}
.suda-zone-grip {
  color: var(--text-4);
  cursor: grab;
  flex-shrink: 0;
}
.suda-zone-name {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-2);
  letter-spacing: 0.01em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.suda-zone-count {
  flex-shrink: 0;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-4);
  background: var(--bg-card-solid);
  border-radius: var(--radius-pill);
  padding: 1px 8px;
}
.suda-zone-more {
  margin-left: auto;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-4);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.suda-zone-more:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.suda-zone-rename {
  width: 160px;
  padding: 2px 8px;
  border: 1px solid var(--brand-500);
  border-radius: var(--radius-sm);
  background: var(--bg-card-solid);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-weight: 600;
  font-family: inherit;
}
.suda-zone-rename:focus {
  outline: none;
}
.suda-zone-empty {
  padding: 0 2px 2px;
  font-size: 0.75rem;
  color: var(--text-4);
}
/* 分区头拖拽排序的插入线：横向流式并排 → 竖线（随所在行高拉伸） */
.suda-zone-insert {
  width: 3px;
  min-height: 48px;
  align-self: stretch;
  border-radius: 2px;
  background: var(--brand-500);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--brand-500) 18%, transparent);
}
/* 小类行外壳：nav 可横滚，两端 ‹ › 按钮提示还有更多小类（小类多时右侧选不到的修复） */
.suda-cat-wrap {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-bottom: 14px;
  border-bottom: 1px solid var(--border-soft);
}
.suda-cat-tabs {
  flex: 1;
  min-width: 0;
  padding-bottom: 4px;
}
.cat-scroll-btn {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 24px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.cat-scroll-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
/* 有下层级的 chip：名称 + ▸ 组合，级联菜单坐标锚定由 JS 实测（Teleport 到 body） */
.sub-chip-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
}
.sub-chip-arrow {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 22px;
  margin-left: -4px;
  border: none;
  background: transparent;
  border-radius: 4px;
  color: var(--text-4);
  cursor: pointer;
  transition: color 0.12s, background 0.12s;
}
.sub-chip-arrow:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.sub-chip-arrow svg {
  transition: transform 0.15s;
}
.sub-chip-arrow.open {
  color: var(--brand-500);
}
.sub-chip-arrow.open svg {
  transform: rotate(90deg);
}
/* 默认小类星标（含义见 tooltip，设置里可改默认）：置于小类名前；Tailwind preflight 把 svg 置为 block，必须恢复行内否则掉到文字下一行 */
.sub-default-star {
  display: inline-block;
  vertical-align: -1px;
  margin-right: 3px;
  color: var(--c-yellow);
}

.suda-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.suda-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, 124px);
  justify-content: space-between;
  gap: 10px;
  /* 被拖卡片拖拽期间 position:absolute，这里当它的定位上下文 */
  position: relative;
}
.suda-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 7px;
  width: 124px;
  min-width: 0;
  padding: 15px 8px 12px;
  /* 比外层玻璃面板更实的底 + 描边 + 投影，与面板拉开层次 */
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
}
.suda-card:hover .suda-kind,
.suda-card:focus-within .suda-kind {
  color: var(--text-1);
}
.suda-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-hover);
}
/* ---- 批量管理模式：勾选角标占左上（小类标签让位），hover 操作隐藏，选中品牌描边 ---- */
.suda-card.batch-on .suda-kind,
.suda-card.batch-on .suda-actions {
  display: none;
}
.suda-card.selected {
  border-color: var(--brand-500);
}
.suda-batch-check {
  position: absolute;
  top: 6px;
  left: 6px;
  width: 20px;
  height: 20px;
  border: 1.5px solid var(--border-strong);
  border-radius: 6px;
  background: var(--bg-card-solid);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-on-accent);
  pointer-events: none;
  z-index: 2;
}
.suda-batch-check.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.suda-batch-count {
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
  white-space: nowrap;
  margin-right: 2px;
}
.suda-batch-btn {
  padding: 5px 12px;
  font-size: 0.75rem;
}
.suda-batch-btn.danger {
  color: var(--c-red);
}
.suda-batch-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.suda-icon {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.18s ease-out, background 0.18s ease-out;
}
.suda-card:hover .suda-icon {
  transform: scale(1.06);
}
.suda-file-icon {
  background: transparent;
}
.suda-icon.web-default {
  background: var(--c-green-soft);
}
.suda-letter {
  font-size: 1.25rem;
  font-weight: 700;
}
.suda-img {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  object-fit: contain;
}
.suda-name {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-2);
}
.suda-name-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.suda-dot {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--c-green);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--c-green) 22%, transparent);
}
.suda-kind {
  position: absolute;
  top: 6px;
  left: 6px;
  /* 长分类名（书签导入的小类可能很长）封顶省略：不越过居中的图标、不撞右侧 hover 操作钮；
     text-overflow 需要块级容器，不能挂在 inline-flex 上（完整名靠模板里的 title 提示） */
  display: block;
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.625rem;
  font-weight: 600;
  line-height: 1.2;
  color: var(--text-3);
}
.suda-kind.app {
  color: var(--c-blue-ink);
}
.suda-kind.web {
  color: var(--c-green-ink);
}
.suda-kind.file {
  color: var(--c-purple-ink);
}

.suda-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.15s;
}
.suda-card:hover .suda-actions,
.suda-card:focus-within .suda-actions {
  opacity: 1;
}
.suda-action {
  width: 28px;
  height: 28px;
  border: none;
  background: var(--bg-card);
  border-radius: 7px;
  box-shadow: var(--shadow-card);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.suda-action:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.suda-action.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

/* 拖拽导入遮罩 */
.drop-overlay {
  position: fixed;
  inset: 0;
  z-index: 250;
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drop-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 32px 48px;
  background: var(--bg-card);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  border: 2px dashed var(--brand-500);
  color: var(--brand-500);
}
.drop-hint p {
  font-size: 0.9375rem;
  font-weight: 600;
}
.drop-hint span {
  font-size: 0.75rem;
  color: var(--text-3);
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.spin {
  animation: drop-spin 0.9s linear infinite;
}
@keyframes drop-spin {
  to {
    transform: rotate(360deg);
  }
}

.drop-enter-active,
.drop-leave-active {
  transition: opacity 0.15s ease-out;
}
.drop-enter-from,
.drop-leave-to {
  opacity: 0;
}

/* 长按拖拽排序（#6）：跟手飞行 + 落点虚线插槽 */
.suda-card.is-dragging {
  /* 脱离文档流：腾出来的格子由 .suda-drop-slot 补上，网格项数恒为 N。
     否则插槽会多占一格，把后面的卡片整片挤到下一行（拖拽时网格整体跳动）。 */
  position: absolute;
  top: 0;
  left: 0;
  /* 位移走独立 translate 属性（inline），不进过渡列表 —— 跟手必须即时。
     只有「浮起」的放大与阴影走短过渡：否则卡片会在指针处瞬间变大，
     看起来像凭空冒出来（用户反馈的「从右下角飘出来」）。 */
  transition: scale 0.12s ease-out, box-shadow 0.12s ease-out;
  scale: 1.04;
  z-index: 60;
  cursor: grabbing;
  box-shadow: var(--shadow-dock);
}
.suda-card.is-dragging:hover {
  /* 拖拽中不再叠加 hover 的上浮位移，位置完全由指针决定 */
  transform: none;
}
.suda-drop-slot {
  width: 124px;
  /* 跟随同行卡片的高度（不写死，避免比卡片高把整行撑起来）；
     单独占一行时仍保留一个可见的虚线框 */
  align-self: stretch;
  min-height: 88px;
  border: 2px dashed var(--brand-500);
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--brand-500) 6%, transparent);
}
:global(body.suda-dragging) {
  cursor: grabbing;
  user-select: none;
  -webkit-user-select: none;
}
</style>
