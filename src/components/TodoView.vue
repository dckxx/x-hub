<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, provide, ref, watch } from 'vue'
import { Check, ChevronLeft, ChevronRight, ListTodo, Plus, X } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo, TodoOccurrence, TodoTag } from '../api/tauri'
import { useTodoDrag } from '../composables/useTodoDrag'
import { useTodoRestore } from '../composables/useTodoRestore'
import {
  VIEW_GROUP_META,
  addDays,
  calendarGrid,
  compareByOrder,
  compareChildOrder,
  doneAtLabel,
  doneMetaLabel,
  dueBadge,
  fmtHM,
  inHorizon,
  isoKey,
  parseServerDate,
  startOfDay,
  viewGroupOf,
  type Horizon,
} from '../utils/todoSchedule'
import TodoRow from './TodoRow.vue'
import TodoEditDialog from './TodoEditDialog.vue'
import ConfirmDialog from './ConfirmDialog.vue'

/**
 * 待办视图：宽窗左右同屏（列表 40% + 日历 60%），窄窗切单栏 + 双维工具栏。
 * 「范围」（今天/本周/本月/全部）与「展示形态」（列表/日历）是**两个正交维度**，
 * 窄窗只是把同屏改成切换，不会出现「切到日历就丢了范围」。
 * 周期规则的权威解释在 Rust 侧，这里只渲染 `expand_todo_occurrences` 的返回结果。
 */
const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const WIDE_MIN = 720

const horizon = ref<Horizon>('week')
const mode = ref<'list' | 'calendar'>('list')
const tagFilter = ref<number[]>([])
const editing = ref<Todo | null>(null)
const editingOpen = ref(false)
const presetDue = ref<number | null>(null)
/** 行内确认弹窗（宿主层单实例）：子行只发请求，弹窗由这里唯一一份渲染 */
const rowConfirm = ref<{ todo: Todo; kids: number; onConfirm: () => void } | null>(null)
const rootRef = ref<HTMLElement | null>(null)
const narrow = ref(false)

// 日历游标（月视图用月初，周视图用周一）
const cursor = ref(new Date())
const calUnit = ref<'month' | 'week'>('month')
const occurrences = ref<TodoOccurrence[]>([])

/** 视图常驻（托盘驻留 / 开着过夜）时不能让「今天」停在挂载那一刻：
 *  分钟 tick 推进，跨午夜后分组（逾期/今天/本周）、日历高亮与逾期徽标才跟着走。
 *  与 ClockCard 的 minuteTick 同一口径，只在日期真的变了才赋值。 */
const today = ref(new Date())
let dayTimer: ReturnType<typeof setInterval> | null = null

const HORIZONS: Array<{ key: Horizon; label: string }> = [
  { key: 'today', label: '今天' },
  { key: 'week', label: '本周' },
  { key: 'month', label: '本月' },
  { key: 'all', label: '全部' },
]

const allTags = computed(() => store.state.todoTags)

/** 未完成 + 已完成分开：视图默认只看未完成（已完成走列表底部折叠区） */
const showDone = ref(false)

/** 标签命中（OR 语义：选中多个标签时任一命中即显示）。tagFilter 为空 = 不过滤。
 *  列表、日历真实条目、周期虚拟实例共用同一判定，保证两侧口径一致 */
function hasAnyTag(id: number): boolean {
  if (!tagFilter.value.length) return true
  const ids = store.todoTagIds(id)
  return tagFilter.value.some((tid) => ids.includes(tid))
}

function hasTag(t: Todo): boolean {
  return hasAnyTag(t.id)
}

/** 每个标签挂着的未完成待办数：chip 上直接给计数，让「点标签 = 查待办」这件事
 *  一眼可见（此前 chips 太像纯展示徽标，过滤能力没有被发现） */
const tagCounts = computed(() => {
  const map = new Map<number, number>()
  for (const t of topTodos.value) {
    if (t.done) continue
    for (const id of store.todoTagIds(t.id)) map.set(id, (map.get(id) ?? 0) + 1)
  }
  return map
})

const topTodos = computed(() =>
  store.state.todos.filter((t) => t.parent_id == null),
)

const visible = computed(() =>
  topTodos.value.filter((t) => (showDone.value ? t.done : !t.done) && hasTag(t) && (showDone.value || inHorizon(t, horizon.value, today.value))),
)

/** 各范围档的命中数（与 visible 同一过滤基底，只换范围档）。范围切换只影响「未来日期」
 *  的条目——置顶/逾期/无截止在任何范围下都常显（inHorizon 的既定口径），待办多是这几类时
 *  列表肉眼无变化，容易被当成「切了没反应」；按钮上直接给计数，让筛选生效看得见 */
const horizonCounts = computed(() => {
  const map = new Map<Horizon, number>()
  for (const h of HORIZONS) {
    map.set(
      h.key,
      topTodos.value.filter((t) => (showDone.value ? t.done : !t.done) && hasTag(t) && (showDone.value || inHorizon(t, h.key, today.value))).length,
    )
  }
  return map
})

/** 分组：置顶 → 逾期 → 今天 → 本周 → 本月 → 以后 → 无日期（组内按手动排序/创建时间） */
const groups = computed(() => {
  const buckets: Todo[][] = VIEW_GROUP_META.map(() => [])
  for (const t of visible.value) buckets[viewGroupOf(t, today.value)].push(t)
  return VIEW_GROUP_META.map((meta, i) => ({
    label: meta.label,
    items: buckets[i].slice().sort(compareByOrder),
  })).filter((g) => g.items.length > 0)
})

const kidsOf = computed(() => {
  const map = new Map<number, Todo[]>()
  for (const t of store.state.todos) {
    if (t.parent_id == null) continue
    const list = map.get(t.parent_id)
    if (list) list.push(t)
    else map.set(t.parent_id, [t])
  }
  for (const list of map.values()) list.sort(compareChildOrder)
  return map
})

/** 日历：真实条目按 due_at 落格；**已完成也显示**——有截止落原日期，没截止落在完成当天
 *  （completed_at，UTC 字符串补 Z 解析），用删除线淡显。虚拟实例按命令返回时刻落格（虚线）。
 *  同格内未完成排在已完成前面。 */
const realByDay = computed(() => {
  const map = new Map<string, Todo[]>()
  for (const t of topTodos.value) {
    // 与左侧列表同一标签过滤口径：筛选激活时日历也只显示命中条目
    if (!hasTag(t)) continue
    const at = t.due_at ?? (t.done ? parseServerDate(t.completed_at)?.getTime() ?? null : null)
    if (at == null) continue
    const key = isoKey(new Date(at))
    const list = map.get(key)
    if (list) list.push(t)
    else map.set(key, [t])
  }
  for (const list of map.values()) list.sort((a, b) => Number(a.done) - Number(b.done))
  return map
})

/** 含未完成逾期待办的日期（月历格淡红底，与工作台日历卡 TodoCalendarCard.overdueByDay
 *  同口径）：badgeOf 已把已完成排除；按**全量**条目判定，周期待办虚拟实例不参与
 *  （与 chip 的 late 口径一致），全部完成后自动消退。 */
const overdueByDay = computed(() => {
  const set = new Set<string>()
  for (const [key, list] of realByDay.value) {
    if (list.some((t) => badgeOf(t)?.kind === 'over')) set.add(key)
  }
  return set
})

const virtualByDay = computed(() => {
  const map = new Map<string, TodoOccurrence[]>()
  for (const o of occurrences.value) {
    // 周期实例跟着其本体条目的标签走（todo_id → 本体标签命中判定）
    if (!hasAnyTag(o.todo_id)) continue
    const key = isoKey(new Date(o.at_ms))
    const list = map.get(key)
    if (list) list.push(o)
    else map.set(key, [o])
  }
  return map
})

const titleOf = computed(() => {
  const map = new Map<number, string>()
  for (const t of store.state.todos) map.set(t.id, t.title)
  return map
})

/** 日历格内最多直接展示的条数：其余折叠成「…+N」——一格塞十来条会把月历撑爆，
 *  完整清单看格子下方的「当天明细」（点格子即达） */
const CELL_MAX_CHIPS = 2

/** 某个日期格要渲染的 chips：真实条目优先、周期虚拟实例补位，剩余计进折叠口 */
function chipsOf(key: string) {
  const real = realByDay.value.get(key) ?? []
  const virtual = virtualByDay.value.get(key) ?? []
  const shownReal = real.slice(0, CELL_MAX_CHIPS)
  const shownVirtual = virtual.slice(0, CELL_MAX_CHIPS - shownReal.length)
  return {
    real: shownReal,
    virtual: shownVirtual,
    extra: real.length + virtual.length - shownReal.length - shownVirtual.length,
  }
}

/** 右侧日历下方「选中那天」的待办清单：点格子切换（默认今天），未完成在前、已完成在后 */
const selectedDay = ref(isoKey(new Date()))
const selectedDayLabel = computed(() => {
  const [y, m, d] = selectedDay.value.split('-').map(Number)
  return `${y} 年 ${m} 月 ${d} 日`
})
const selectedTodos = computed<Todo[]>(() => realByDay.value.get(selectedDay.value) ?? [])

/** 当天明细拆两段：未完成在前、已完成在后。完成时间只出现在已完成段里，
 *  不和「几点截止」混排——两段时间各占自己的名分。 */
const selectedDayParts = computed(() => {
  const list = selectedTodos.value
  return {
    undone: list.filter((t) => !t.done),
    done: list.filter((t) => t.done),
  }
})

/**
 * 已完成行的时间文案：完成日 = 选中日时只给时刻；跨天完成给相对日期，
 * 避免把「另一天做完的」误读成当天完成。有原截止的补「原定 HH:MM」。
 */
function doneRowTime(t: Todo): string {
  const doneDate = parseServerDate(t.completed_at)
  const doneLabel = doneAtLabel(t.completed_at, today.value)
  const doneText = !doneDate
    ? '已完成'
    : isoKey(doneDate) === selectedDay.value
      ? `完成于 ${fmtHM(doneDate.getTime())}`
      : `完成于 ${doneLabel?.text ?? ''}`
  if (t.due_at == null) return doneText
  const due = new Date(t.due_at)
  const endOfDay = due.getHours() === 23 && due.getMinutes() > 50
  return `${endOfDay ? '' : `原定 ${fmtHM(t.due_at)} · `}${doneText}`
}

const cells = computed(() => calendarGrid(cursor.value, today.value))

const monthLabel = computed(
  () => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`,
)

const weekLabel = computed(() => {
  const start = weekStart(cursor.value)
  const end = new Date(start)
  end.setDate(end.getDate() + 6)
  return `${start.getMonth() + 1}月${start.getDate()}日 – ${end.getMonth() + 1}月${end.getDate()}日`
})

function weekStart(d: Date): Date {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  x.setDate(x.getDate() - ((x.getDay() + 6) % 7))
  return x
}

/** 周视图显示的 7 天 */
const weekCells = computed(() => {
  const start = weekStart(cursor.value)
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(start)
    d.setDate(d.getDate() + i)
    return { key: isoKey(d), date: d }
  })
})

/** 周期实例请求序号：快速翻月/切周时先发的响应可能后到，旧月份结果会盖掉新月份 */
let occurrenceSeq = 0

async function loadOccurrences() {
  const from = calendarGrid(cursor.value, today.value)[0]
  const first = new Date(`${from.key}T00:00:00`)
  const to = new Date(first)
  to.setDate(to.getDate() + 42)
  const seq = ++occurrenceSeq
  try {
    const list = await store.expandTodoOccurrences(first.getTime(), to.getTime() - 1)
    // 已有更新的请求在途：丢弃这次结果，否则日历会闪回旧月份的实例
    if (seq !== occurrenceSeq) return
    occurrences.value = list
  } catch {
    if (seq !== occurrenceSeq) return
    occurrences.value = []
    showToast('周期待办实例没能算出来，日历上的虚线可能不全')
  }
}

/** 周期规则签名：勾掉一轮（due_at 前移）/ 改规则 / 新建周期待办之后，
 *  日历上的虚线虚拟实例必须重算——它们是按 due_at 现算的，不重算会留下错位的旧实例
 *  （与工作台日历卡 TodoCalendarCard 同一口径）。 */
const repeatSignature = computed(() =>
  store.state.todos
    .filter((t) => t.parent_id == null && !t.done && t.repeat_mode !== 'once')
    .map((t) => `${t.id}:${t.due_at}:${t.repeat_mode}:${t.repeat_done_count}`)
    .join('|'),
)

watch([cursor, calUnit, today, repeatSignature], () => void loadOccurrences())

onMounted(() => {
  void loadOccurrences()
  updateNarrow()
  // 观察根元素尺寸而非 window resize：侧栏收起/展开时窗口尺寸不变、内容区却变宽，
  // window 的 resize 根本不触发（useDashboardLayout 已有同款先例）
  if (typeof ResizeObserver !== 'undefined') {
    ro = new ResizeObserver(updateNarrow)
    if (rootRef.value) ro.observe(rootRef.value)
  } else if (typeof window !== 'undefined') {
    window.addEventListener('resize', updateNarrow)
  }
  dayTimer = setInterval(() => {
    const d = new Date()
    if (d.toDateString() !== today.value.toDateString()) today.value = d
  }, 60_000)
})

onBeforeUnmount(() => {
  ro?.disconnect()
  ro = null
  if (typeof window !== 'undefined') window.removeEventListener('resize', updateNarrow)
  if (dayTimer) clearInterval(dayTimer)
  // 拖拽途中切走视图（组件卸载）时把 window 级指针监听一并摘掉，避免残留在全局
  onChipPointerUp()
})

let ro: ResizeObserver | null = null

function updateNarrow() {
  const w = rootRef.value?.clientWidth ?? window.innerWidth
  const next = w < WIDE_MIN
  // 只在真的翻转时赋值：否则 RO 回调 → 改 narrow → 布局变化 → 再次回调，空转
  if (next !== narrow.value) narrow.value = next
}

function shift(delta: number) {
  const d = new Date(cursor.value)
  if (calUnit.value === 'month') d.setMonth(d.getMonth() + delta, 1)
  else d.setDate(d.getDate() + delta * 7)
  cursor.value = d
}

function goToday() {
  cursor.value = new Date()
}

function openNew(dueMs: number | null = null) {
  editing.value = null
  presetDue.value = dueMs
  editingOpen.value = true
}

function openEdit(t: Todo) {
  editing.value = t
  presetDue.value = null
  editingOpen.value = true
}

/** 统一兜底：IPC 失败给一句提示，别让失败只停在控制台（用户会以为点了没反应） */
function run(p: Promise<unknown>, msg = '操作失败，请重试') {
  void p.catch(() => showToast(msg))
}

/** 行内确认弹窗的回调：勾父带子 / 删除的二次确认都经这里（子行只发请求） */
function onRowConfirm() {
  const c = rowConfirm.value
  rowConfirm.value = null
  if (c) void c.onConfirm()
}

// ---- 删除（父条目级联删子）+ 撤销恢复，由 TodoRow 经 provide 调用 ----
// 与 TodoCard 同一套语义（同一份 useTodoRestore）：删除是不可逆操作，给一条可撤销提示兜住误点。
const restoreTodo = useTodoRestore()

async function removeTodo(t: Todo) {
  const kids = store.state.todos.filter((x) => x.parent_id === t.id)
  // 标签关联是 ON DELETE CASCADE，删除后查不回来 → 删前先抓快照
  const tagIds = store.todoTagIds(t.id)
  try {
    await store.deleteTodo(t.id)
  } catch {
    showToast('删除失败，请重试')
    return
  }
  showToast(
    kids.length ? `已删除「${t.title}」及 ${kids.length} 条子待办` : `已删除「${t.title}」`,
    {
      label: '撤销',
      onClick: () => void restoreTodo(t, kids, tagIds).catch(() => showToast('恢复失败，请重试')),
    },
  )
}

// ---- 行渲染全部交给 TodoRow（与卡片/浮窗同一份），这里只提供宿主能力 ----
provide('todoOpenSchedule', (t: Todo) => openEdit(t))
provide('todoRemoveTodo', removeTodo)
provide('todoChildren', kidsOf)
/** 当前展示的是未完成还是已完成（子待办拖拽仅在未完成视图开放，同卡片约束） */
const todoViewRef = computed<'pending' | 'done'>(() => (showDone.value ? 'done' : 'pending'))
provide('todoView', todoViewRef)
provide('todoRowConfirm', (todo: Todo, kids: number, onConfirm: () => void) => {
  rowConfirm.value = { todo, kids, onConfirm }
})

// ---- 组内上下拖动排序（逻辑在 useTodoDrag，与卡片共用）----
const bodyRef = ref<HTMLElement | null>(null)
const {
  dragId: rowDragId,
  lineTop: dragLineTop,
  onRowPointerDown: onRowDragStart,
} = useTodoDrag({
  bodyRef,
  groups,
  labelOf: (t) => VIEW_GROUP_META[viewGroupOf(t, today.value)].label,
  // 已完成视图里顺序没有意义（分组也是按完成状态来的），不开放拖拽
  enabled: () => !showDone.value,
  reorder: (ids) => void store.reorderTodos(ids),
})
provide('todoDragStart', onRowDragStart)
provide('todoDragId', rowDragId)

function toggleTagFilter(id: number) {
  const i = tagFilter.value.indexOf(id)
  if (i >= 0) tagFilter.value.splice(i, 1)
  else tagFilter.value.push(id)
}

async function removeTag(id: number) {
  try {
    await store.deleteTodoTag(id)
    tagFilter.value = tagFilter.value.filter((x) => x !== id)
  } catch {
    showToast('标签没能删除，请重试')
  }
}

/** 删标签会从所有待办上摘掉它，先确认（点 chip 上的 × 不再直接删） */
const removingTag = ref<TodoTag | null>(null)

function confirmRemoveTag() {
  const tag = removingTag.value
  removingTag.value = null
  if (tag) void removeTag(tag.id)
}

/** 日历格子里日期徽标的逾期判定（行内的徽标由 TodoRow 自己算） */
function badgeOf(t: Todo) {
  return t.done ? null : dueBadge(t, today.value)
}

// ---- 日历拖拽改期 ----
// 指针实现而非 HTML5 DnD：Tauri 主窗口的原生拖放拦截与 WebView 内 HTML5 拖拽互斥
// （dragstart 后收不到 dragover/drop），与卡片组内拖拽同一口径。
// 语义：把**实际条目**拖到某天 → 截止改到该天同一时分（提醒若已设，按同一偏移平移）；
// 虚拟实例（周期规则算出的未来轮次）不是落库对象，不参与拖拽。
const dragId = ref<number | null>(null)
const dropDay = ref<string | null>(null)
/** 拖拽刚结束的短暂窗口内吞掉尾随 click，避免「拖完顺带打开编辑弹层」 */
let suppressClickUntil = 0
let chipDrag: { t: Todo; x: number; y: number; active: boolean } | null = null

function onChipPointerDown(t: Todo, e: PointerEvent) {
  if (e.button !== 0) return
  // 已完成的条目只做展示（点击进编辑），不允许拖动改期
  if (t.done) return
  chipDrag = { t, x: e.clientX, y: e.clientY, active: false }
  window.addEventListener('pointermove', onChipPointerMove)
  window.addEventListener('pointerup', onChipPointerUp)
  window.addEventListener('pointercancel', onChipPointerUp)
}

function onChipPointerMove(e: PointerEvent) {
  const d = chipDrag
  if (!d) return
  if (!d.active) {
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) < 5) return
    d.active = true
    dragId.value = d.t.id
    document.body.classList.add('todo-chip-dragging')
    document.getSelection()?.removeAllRanges()
  }
  const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null
  dropDay.value = el?.closest<HTMLElement>('[data-day]')?.dataset.day ?? null
}

function onChipPointerUp() {
  window.removeEventListener('pointermove', onChipPointerMove)
  window.removeEventListener('pointerup', onChipPointerUp)
  window.removeEventListener('pointercancel', onChipPointerUp)
  const d = chipDrag
  const day = dropDay.value
  chipDrag = null
  dragId.value = null
  dropDay.value = null
  document.body.classList.remove('todo-chip-dragging')
  if (!d || !d.active) return
  suppressClickUntil = Date.now() + 350
  if (day == null || d.t.due_at == null) return
  if (isoKey(new Date(d.t.due_at)) === day) return
  run(moveToDay(d.t, day), '改期失败，请重试')
}

async function moveToDay(t: Todo, day: string) {
  const [y, m, d] = day.split('-').map(Number)
  const due = new Date(t.due_at as number)
  const deltaDays = Math.round(
    (new Date(y, m - 1, d).getTime() - startOfDay(due).getTime()) / 86_400_000,
  )
  due.setFullYear(y, m - 1, d)
  const remind = t.remind_at == null ? null : addDays(new Date(t.remind_at), deltaDays).getTime()
  await store.scheduleTodo(t.id, due.getTime(), remind)
  // 虚拟实例是按 due_at 现算的：改期后必须重算，否则虚线实例会留在旧日期直到翻月
  await loadOccurrences()
  showToast(`「${t.title}」已改到 ${m} 月 ${d} 日`)
}

function onChipClick(t: Todo) {
  if (Date.now() < suppressClickUntil) return
  openEdit(t)
}

/** 虚拟实例不是落库对象：不能拖、不能点开，给一句可执行的去处 */
function onVirtualDown(e: PointerEvent) {
  if (e.button !== 0) return
  showToast('这是周期规则算出的未来实例，请到编辑弹层改周期规则')
}
</script>

<template>
  <section ref="rootRef" class="todo-view">
    <div class="tv-body" :class="{ narrow }">
      <!-- 列表卡：视图头 + 双维工具栏 + 列表（与速记视图一致，内容全部落在卡片里） -->
      <section v-show="!narrow || mode === 'list'" class="card tv-list">
        <header class="tv-head">
          <div class="tv-title">
            <ListTodo :size="16" :stroke-width="2" />
            <h2>待办</h2>
          </div>
          <span class="tv-spacer"></span>
          <button type="button" class="tv-primary" @click="openNew()">
            <Plus :size="14" :stroke-width="2.2" />新建待办
          </button>
        </header>

        <!-- 双维工具栏：范围（筛选）× 展示形态（列表/日历） -->
        <div class="tv-toolbar">
          <div class="tv-seg">
            <button
              v-for="h in HORIZONS"
              :key="h.key"
              type="button"
              :class="{ on: horizon === h.key }"
              @click="horizon = h.key"
            >
              {{ h.label }} <span class="cnt">{{ horizonCounts.get(h.key) ?? 0 }}</span>
            </button>
          </div>
          <span class="tv-spacer"></span>
          <button type="button" class="tv-ghost" :class="{ on: showDone }" @click="showDone = !showDone">
            {{ showDone ? '看未完成' : '看已完成' }}
          </button>
          <div v-if="narrow" class="tv-seg">
            <button type="button" :class="{ on: mode === 'list' }" @click="mode = 'list'">列表</button>
            <button type="button" :class="{ on: mode === 'calendar' }" @click="mode = 'calendar'">日历</button>
          </div>
        </div>

        <!-- 标签筛选行：紧贴时间范围下面（2026-10-15 反馈「挪到时间下面」）。标签是
             第二个筛选维度，跟范围切换放同一列才像个过滤器；行尾的「清除」只在
             筛选激活时出现 -->
        <div v-if="allTags.length" class="tv-tagbar">
          <button
            v-for="tag in allTags"
            :key="tag.id"
            type="button"
            class="tv-tagchip"
            :class="{ on: tagFilter.includes(tag.id) }"
            :title="`点击按「${tag.name}」筛选待办（再点取消）`"
            @click="toggleTagFilter(tag.id)"
          >
            <i class="dot" :style="{ background: tag.color || 'var(--brand-500)' }"></i>{{ tag.name }}
            <span class="cnt">{{ tagCounts.get(tag.id) ?? 0 }}</span>
            <span class="x" title="删除该标签" @click.stop="removingTag = tag"><X :size="10" :stroke-width="2.5" /></span>
          </button>
          <button
            v-if="tagFilter.length"
            type="button"
            class="tv-tagclear"
            title="清除标签筛选"
            @click="tagFilter = []"
          >
            清除
          </button>
        </div>

        <div ref="bodyRef" class="tv-scroll">
          <p v-if="!groups.length" class="tv-empty">
            {{ tagFilter.length ? '没有符合所选标签的待办' : '这个范围里没有待办' }}
          </p>
          <div v-for="g in groups" :key="g.label" class="tv-group" :data-group="g.label">
            <div
              class="tv-group-h"
              :class="{ pinned: g.label === '置顶', overdue: g.label === '逾期' }"
            >
              <span>{{ g.label }}</span>
              <span class="cnt">{{ g.items.length }}</span>
              <span class="line"></span>
            </div>
            <!-- 行渲染复用 TodoRow（与卡片/浮窗同一份）：优先级圆点、标签、周期徽标、
                 子待办、行内编辑、删除+撤销、组内拖拽排序全在组件内，视图只提供宿主能力 -->
            <TodoRow v-for="t in g.items" :key="t.id" :todo="t" />
          </div>
          <!-- 组内拖拽的插入指示线（由 useTodoDrag 定位） -->
          <div v-if="dragLineTop != null" class="tv-drag-line" :style="{ top: dragLineTop + 'px' }" />
        </div>
      </section>

      <!-- 日历卡 -->
      <section v-show="!narrow || mode === 'calendar'" class="card tv-cal">
        <div class="tv-cal-h">
          <button type="button" class="tv-icon" aria-label="上一页" @click="shift(-1)">
            <ChevronLeft :size="14" :stroke-width="2" />
          </button>
          <span class="tv-cal-title">{{ calUnit === 'month' ? monthLabel : weekLabel }}</span>
          <button type="button" class="tv-icon" aria-label="下一页" @click="shift(1)">
            <ChevronRight :size="14" :stroke-width="2" />
          </button>
          <button type="button" class="tv-ghost" @click="goToday">今天</button>
          <span class="tv-spacer"></span>
          <div class="tv-seg">
            <button type="button" :class="{ on: calUnit === 'month' }" @click="calUnit = 'month'">月</button>
            <button type="button" :class="{ on: calUnit === 'week' }" @click="calUnit = 'week'">周</button>
          </div>
        </div>

        <div class="tv-legend">
          <span><i class="swatch solid"></i>未完成条目（拖到其他日期可改期）</span>
          <span><i class="swatch dashed"></i>周期规则算出的虚拟实例（不落库，不能拖）</span>
          <span><i class="swatch late"></i>逾期</span>
          <span><i class="swatch done"></i>✓ 已完成（无截止的落完成日）</span>
        </div>

        <div v-if="calUnit === 'month'" class="tv-grid">
          <div v-for="d in ['一', '二', '三', '四', '五', '六', '日']" :key="d" class="tv-dow">{{ d }}</div>
          <div
            v-for="c in cells"
            :key="c.key"
            class="tv-cell"
            :class="{ out: c.out, today: c.today, 'has-overdue': overdueByDay.has(c.key), 'drop-on': dropDay === c.key, sel: selectedDay === c.key }"
            :data-day="c.key"
            @click="selectedDay = c.key"
            @dblclick="openNew(new Date(`${c.key}T23:59:00`).getTime())"
          >
            <span class="tv-day">{{ c.day }}</span>
            <div class="tv-chips">
              <button
                v-for="t in chipsOf(c.key).real"
                :key="'r' + t.id"
                type="button"
                class="tv-chip real"
                :class="{ done: t.done, late: badgeOf(t)?.kind === 'over', dragging: dragId === t.id }"
                :title="t.done ? `${t.title}（${doneMetaLabel(t, today)}）` : `${t.title}（拖动到其他日期可改期）`"
                @pointerdown="onChipPointerDown(t, $event)"
                @click="onChipClick(t)"
                @dblclick.stop
              >
                <Check v-if="t.done" class="tv-chip-check" :size="9" :stroke-width="3" aria-hidden="true" />
                <span class="tv-chip-title">{{ t.title }}</span>
              </button>
              <span
                v-for="o in chipsOf(c.key).virtual"
                :key="'v' + o.todo_id + o.at_ms"
                class="tv-chip virtual"
                :title="`${titleOf.get(o.todo_id) ?? '周期待办'}（周期规则算出的未来实例，不能拖动）`"
                @pointerdown="onVirtualDown"
              >
                <span class="tv-chip-title">{{ titleOf.get(o.todo_id) ?? '周期待办' }}</span>
              </span>
              <!-- 折叠尾巴：只提示数量，点击落到格子上选中当天，完整清单看下方明细 -->
              <span
                v-if="chipsOf(c.key).extra > 0"
                class="tv-chip more"
                :title="`还有 ${chipsOf(c.key).extra} 条没展示，点格子看当天全部待办`"
              >
                …+{{ chipsOf(c.key).extra }}
              </span>
            </div>
          </div>
        </div>

        <div v-else class="tv-week">
          <div
            v-for="c in weekCells"
            :key="c.key"
            class="tv-weekcol"
            :class="{ today: isoKey(c.date) === isoKey(today), 'drop-on': dropDay === c.key, sel: selectedDay === c.key }"
            :data-day="c.key"
            @click="selectedDay = c.key"
          >
            <div class="tv-weekhead">{{ c.date.getMonth() + 1 }}/{{ c.date.getDate() }} 周{{ ['一', '二', '三', '四', '五', '六', '日'][(c.date.getDay() + 6) % 7] }}</div>
            <div class="tv-chips">
              <button
                v-for="t in chipsOf(c.key).real"
                :key="'r' + t.id"
                type="button"
                class="tv-chip real"
                :class="{ done: t.done, late: badgeOf(t)?.kind === 'over', dragging: dragId === t.id }"
                :title="t.done ? `${t.title}（${doneMetaLabel(t, today)}）` : `${t.title}（拖动到其他日期可改期）`"
                @pointerdown="onChipPointerDown(t, $event)"
                @click="onChipClick(t)"
                @dblclick.stop
              >
                <Check v-if="t.done" class="tv-chip-check" :size="9" :stroke-width="3" aria-hidden="true" />
                <span class="tv-chip-title">{{ t.title }}</span>
                <i v-if="t.due_at" class="time">{{ fmtHM(t.due_at) }}</i>
              </button>
              <span
                v-for="o in chipsOf(c.key).virtual"
                :key="'v' + o.todo_id + o.at_ms"
                class="tv-chip virtual"
                :title="`${titleOf.get(o.todo_id) ?? '周期待办'}（周期规则算出的未来实例，不能拖动）`"
                @pointerdown="onVirtualDown"
              >
                <span class="tv-chip-title">{{ titleOf.get(o.todo_id) ?? '周期待办' }}</span>
              </span>
              <span
                v-if="chipsOf(c.key).extra > 0"
                class="tv-chip more"
                :title="`还有 ${chipsOf(c.key).extra} 条没展示，点格子看当天全部待办`"
              >
                …+{{ chipsOf(c.key).extra }}
              </span>
            </div>
          </div>
        </div>

        <!-- 选中那天的待办明细：点日历格子后显示在日历下方（未完成在前、已完成在后） -->
        <div class="tv-dayview">
          <div class="tv-dayview-h">
            <span class="tv-dayview-date">{{ selectedDayLabel }}</span>
            <span class="cnt">{{ selectedTodos.length }}</span>
            <span class="tv-spacer"></span>
            <button
              type="button"
              class="tv-ghost"
              @click="openNew(new Date(`${selectedDay}T23:59:00`).getTime())"
            >加待办</button>
          </div>
          <p v-if="!selectedTodos.length" class="tv-dayview-empty">这天没有待办</p>
          <div v-else class="tv-dayview-list">
            <template v-if="selectedDayParts.undone.length">
              <p class="tv-dv-sub">
                未完成 <span class="cnt">{{ selectedDayParts.undone.length }}</span>
              </p>
              <button
                v-for="t in selectedDayParts.undone"
                :key="t.id"
                type="button"
                class="tv-dayview-row"
                :title="t.title"
                @click="openEdit(t)"
              >
                <span class="tv-dv-mark"></span>
                <span class="tv-dv-title">{{ t.title }}</span>
                <span class="tv-dv-time">{{ t.due_at ? fmtHM(t.due_at) : '' }}</span>
              </button>
            </template>
            <template v-if="selectedDayParts.done.length">
              <p class="tv-dv-sub">
                已完成 <span class="cnt">{{ selectedDayParts.done.length }}</span>
              </p>
              <button
                v-for="t in selectedDayParts.done"
                :key="t.id"
                type="button"
                class="tv-dayview-row done"
                :title="`${t.title}（${doneMetaLabel(t, today)}）`"
                @click="openEdit(t)"
              >
                <span class="tv-dv-mark on">
                  <Check :size="11" :stroke-width="3" />
                </span>
                <span class="tv-dv-title">{{ t.title }}</span>
                <span class="tv-dv-time">{{ doneRowTime(t) }}</span>
              </button>
            </template>
          </div>
        </div>
      </section>
    </div>

    <TodoEditDialog
      :visible="editingOpen"
      :todo="editing"
      :preset-due-ms="presetDue"
      @close="editingOpen = false"
      @saved="loadOccurrences"
    />

    <ConfirmDialog
      :visible="rowConfirm != null"
      title="还有子待办未完成"
      :message="`「${rowConfirm?.todo.title ?? ''}」下还有 ${rowConfirm?.kids ?? 0} 条子待办未完成，是否确认完成？`"
      hint="确认后会一并勾选这些子待办；取消则本条待办保持未完成。"
      confirm-text="确认并勾选子项"
      @confirm="onRowConfirm"
      @cancel="rowConfirm = null"
    />

    <ConfirmDialog
      :visible="removingTag != null"
      title="删除标签"
      :message="`「${removingTag?.name ?? ''}」会从所有待办上摘掉，标签本身也会删除。`"
      hint="这个操作不能撤销。"
      tone="danger"
      confirm-text="删除标签"
      @confirm="confirmRemoveTag"
      @cancel="removingTag = null"
    />
  </section>
</template>

<style scoped>
.todo-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  /* 与速记视图（.view-notes）同一套外边距，卡片贴齐内容区 */
  padding: 0 20px 20px 0;
  overflow: hidden;
}
.tv-head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.tv-title {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--text-1);
}
.tv-title h2 {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 700;
}
.tv-spacer {
  flex: 1;
}
.tv-tags {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.tv-tagchip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  font-size: 0.7rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-3);
  cursor: pointer;
}
.tv-tagchip.on {
  border-color: var(--brand-500);
  color: var(--text-1);
  font-weight: 600;
}
/* chip 上的待办计数：弱化展示，激活态跟着主色走。
   自居中 flex 药丸 + line-height 1：小字号计数与 CJK 标签名行盒度量不同，
   用行高撑盒会让数字视觉上偏高——药丸自身对称包住数字，再由 chip 的
   align-items: center 对齐标签名 */
.tv-tagchip .cnt {
  display: inline-flex;
  align-items: center;
  font-size: 0.62rem;
  font-weight: 600;
  color: var(--text-4);
  background: var(--bg-page);
  border-radius: var(--radius-pill);
  padding: 2px 5px;
  line-height: 1;
}
.tv-tagchip.on .cnt {
  color: var(--brand-500);
}
/* 标签筛选行：时间范围下面一行 chips；「清除」弱化为文字按钮，仅筛选激活时出现 */
.tv-tagbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  /* 水平不加额外内边距：与上方工具栏「今天」按钮同一条 12px 卡片内边距线对齐 */
  padding: 2px 0 10px;
}
.tv-tagclear {
  border: none;
  background: transparent;
  padding: 3px 8px;
  font-size: 0.72rem;
  color: var(--brand-500);
  cursor: pointer;
  border-radius: var(--radius-pill);
}
.tv-tagclear:hover {
  background: var(--brand-50);
}
.tv-tagchip .x {
  display: inline-flex;
  opacity: 0;
  color: var(--text-4);
}
.tv-tagchip:hover .x {
  opacity: 1;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}
.tv-primary {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 6px 13px;
  font-size: 0.78rem;
  font-weight: 600;
  color: #fff;
  background: var(--brand-500);
  border: none;
  border-radius: var(--radius-md);
  cursor: pointer;
}
.tv-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.tv-seg {
  display: inline-flex;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  overflow: hidden;
}
.tv-seg button {
  padding: 4px 11px;
  font-size: 0.72rem;
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.tv-seg button.on {
  background: var(--brand-500);
  color: #fff;
  font-weight: 600;
}
/* 范围档计数（与标签 chip 的 .cnt 同语言）：筛选生效的即时反馈 */
.tv-seg button .cnt {
  margin-left: 3px;
  font-weight: 400;
  opacity: 0.62;
}
.tv-ghost {
  padding: 4px 10px;
  font-size: 0.72rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.tv-ghost.on {
  color: var(--text-1);
  border-color: var(--border-strong);
}
.tv-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 14px;
}
.tv-body.narrow {
  flex-direction: column;
}
/* 卡片布局：两张卡（列表 / 日历）与速记视图一致，内容全部落在卡片里。
   .card 提供毛玻璃底/描边/圆角/阴影，这里只负责内部排版与滚动。 */
.tv-list,
.tv-cal {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
}
/* 左列表整卡滚动照旧；右卡自身不滚——点格子后上下滚动只滚「当天明细」清单，
   不把上面的整个日历带起来（反馈：看当天待办时日历跟着滚走了） */
.tv-list {
  overflow: auto;
}
.tv-cal {
  overflow: hidden;
}
.tv-body:not(.narrow) .tv-list {
  flex: 0 0 40%;
}
/* 列表内容区：头部与工具栏固定，只有分组列表滚动 */
.tv-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  /* 组内拖拽的插入指示线以它为定位基准 */
  position: relative;
}
/* 组内拖拽插入线（绝对定位于 .tv-scroll，随内容滚动）：与卡片 .todo-drag-line 同一套观感 */
.tv-drag-line {
  position: absolute;
  left: 6px;
  right: 6px;
  height: 2px;
  border-radius: 1px;
  background: var(--brand-500);
  box-shadow: 0 0 6px var(--brand-glow);
  pointer-events: none;
  z-index: 5;
}
.tv-drag-line::before {
  content: '';
  position: absolute;
  left: -1px;
  top: -2px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--brand-500);
}
.tv-empty {
  margin: 12px 0;
  font-size: 0.76rem;
  color: var(--text-4);
  text-align: center;
}
.tv-group {
  margin-bottom: 10px;
}
.tv-group-h {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 3px 2px 5px;
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.03em;
  color: var(--text-3);
}
.tv-group-h.pinned {
  color: var(--brand-500);
}
.tv-group-h.overdue {
  color: var(--c-red-ink);
}
.tv-group-h .cnt {
  color: var(--text-4);
}
.tv-group-h .line {
  flex: 1;
  height: 1px;
  background: var(--border-soft);
}
/* 行渲染全部在 TodoRow 里（.todo-* 命名空间），此处只保留本视图自身的样式 */
.tv-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.tv-icon:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.tv-cal {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.tv-cal-h {
  display: flex;
  align-items: center;
  gap: 7px;
}
.tv-cal-title {
  font-size: 0.8rem;
  font-weight: 700;
  color: var(--text-1);
}
.tv-legend {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
  font-size: 0.66rem;
  color: var(--text-4);
}
.tv-legend span {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.swatch {
  width: 14px;
  height: 8px;
  border-radius: 3px;
  border: 1px solid var(--brand-500);
  background: var(--brand-50);
}
.swatch.dashed {
  border-style: dashed;
  background: transparent;
}
.swatch.late {
  border-color: var(--c-red-ink);
  background: var(--c-red-soft);
}
.swatch.done {
  border-color: var(--border-soft);
  background: transparent;
}
.tv-grid {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 4px;
}
.tv-dow {
  font-size: 0.64rem;
  font-weight: 700;
  color: var(--text-4);
  text-align: center;
}
.tv-cell {
  min-height: 74px;
  padding: 4px 5px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-solid);
  overflow: hidden;
}
.tv-cell.out {
  opacity: 0.45;
}
.tv-cell.today {
  border-color: var(--brand-500);
}
/* 含未完成逾期待办的日期：淡红底（与工作台日历卡 .tc-cell.has-overdue 同一口径，
 * 基底取本视图格子的 --bg-card-solid）。改红色比例须两处同步。
 * 用底色而非边框：与「今天 / 选中」的品牌色边框分通道，叠加不冲突。 */
.tv-cell.has-overdue {
  background: color-mix(in srgb, var(--c-red-soft) 50%, var(--bg-card-solid));
}
.tv-cell.sel,
.tv-weekcol.sel {
  border-color: var(--brand-500);
  box-shadow: inset 0 0 0 1px var(--brand-500);
}
.tv-day {
  font-size: 0.66rem;
  color: var(--text-3);
}
.tv-chips {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 2px;
}
/* 折叠尾巴「…+N」：不可交互的纯提示，点击落在日历格上选中当天（完整清单看下方明细） */
.tv-chip.more {
  justify-content: center;
  background: transparent;
  border: 1px dashed var(--border-soft);
  color: var(--text-4);
  text-align: center;
  cursor: default;
}
.tv-chip {
  display: flex;
  align-items: center;
  gap: 3px;
  width: 100%;
  padding: 1px 5px;
  font-size: 0.6rem;
  text-align: left;
  border-radius: 4px;
  border: 1px solid var(--brand-500);
  background: var(--brand-50);
  color: var(--text-1);
  cursor: pointer;
  overflow: hidden;
  white-space: nowrap;
}
.tv-chip-title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 已完成的 ✓：小字号下删除线太弱，图标才是第一眼的「做完了」信号 */
.tv-chip-check {
  flex-shrink: 0;
}
.tv-chip.virtual {
  border-style: dashed;
  background: transparent;
  color: var(--text-3);
  cursor: not-allowed;
}
.tv-chip.real {
  cursor: grab;
}
.tv-chip.dragging {
  opacity: 0.45;
}
:global(body.todo-chip-dragging) {
  cursor: grabbing;
  user-select: none;
}
.tv-cell.drop-on,
.tv-weekcol.drop-on {
  border-color: var(--brand-500);
  background: var(--brand-50);
}
.tv-chip.late {
  border-color: var(--c-red-ink);
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.tv-chip.done {
  border-color: var(--border-soft);
  background: transparent;
  color: var(--text-4);
  text-decoration: line-through;
  cursor: pointer;
}
.tv-chip.real.done {
  cursor: pointer;
}
/* 日历下方「选中那天」明细 */
.tv-dayview {
  margin-top: 4px;
  padding-top: 8px;
  border-top: 1px solid var(--border-soft);
  display: flex;
  flex-direction: column;
  gap: 4px;
  /* 占满日历下方剩余空间；日期头固定在外，只有清单自己滚 */
  flex: 1;
  min-height: 0;
}
.tv-dayview-h {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.72rem;
}
.tv-dayview-date {
  font-weight: 600;
  color: var(--text-1);
}
.tv-dayview-h .cnt {
  color: var(--text-4);
}
.tv-dayview-empty {
  margin: 2px 0;
  font-size: 0.72rem;
  color: var(--text-4);
}
.tv-dayview-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  /* 右卡唯一的滚动区：当天待办多时只滚这份清单 */
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.tv-dayview-row {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 4px 6px;
  font-size: 0.75rem;
  text-align: left;
  color: var(--text-1);
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.tv-dayview-row:hover {
  background: var(--bg-card-soft);
}
.tv-dv-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border: 1.5px solid var(--border-strong);
  border-radius: 50%;
  color: #fff;
}
.tv-dv-mark.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.tv-dv-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tv-dayview-row.done .tv-dv-title {
  color: var(--text-4);
  text-decoration: line-through;
}
.tv-dv-time {
  flex-shrink: 0;
  font-size: 0.68rem;
  color: var(--text-4);
}
/* 当天明细的分段标题：未完成 / 已完成各占一段，完成时间不与截止时间混排 */
.tv-dv-sub {
  display: flex;
  align-items: center;
  gap: 5px;
  margin: 5px 0 1px;
  padding: 0 6px;
  font-size: 0.62rem;
  font-weight: 700;
  color: var(--text-4);
}
.tv-dv-sub:first-child {
  margin-top: 0;
}
.tv-dv-sub .cnt {
  padding: 0 5px;
  font-weight: 600;
  line-height: 14px;
  color: var(--text-4);
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
}
.tv-chip .time {
  flex-shrink: 0;
  margin-left: 4px;
  font-style: normal;
  color: var(--text-4);
}
.tv-week {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 5px;
}
.tv-weekcol {
  min-height: 180px;
  padding: 5px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-solid);
}
.tv-weekcol.today {
  border-color: var(--brand-500);
}
.tv-weekhead {
  font-size: 0.64rem;
  font-weight: 700;
  color: var(--text-3);
  margin-bottom: 4px;
}
</style>
