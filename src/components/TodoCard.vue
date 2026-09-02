<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, provide, ref, watch } from 'vue'
import { ListTodo, PanelTopClose } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'
import { parseTodoItems } from '../utils/todoParse'
import TodoRow from './TodoRow.vue'
import {
  addDays,
  calendarGrid,
  compareByOrder,
  defaultRemindTime,
  fmtDay,
  fmtHM,
  GROUP_COUNT,
  GROUP_META,
  groupOf,
  HOUR_OPTIONS,
  isoKey,
  minuteOptions,
  nextMonday,
  startOfDay,
} from '../utils/todoSchedule'

const props = defineProps<{ highlightId?: number | null }>()

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const view = ref<'pending' | 'done'>('pending')
const input = ref('')

// 全局搜索跳转高亮
const highlight = ref<number | null>(null)
let highlightTimer: ReturnType<typeof setTimeout> | null = null

// ---- 列表派生：待办按 逾期 → 今天 → 有日期 → 无日期 分组，组内按创建时间倒序 ----
interface TodoGroup {
  label: string
  items: Todo[]
}

const topPending = computed(() =>
  store.state.todos.filter((t) => !t.done && t.parent_id == null),
)
const topDone = computed(() =>
  store.state.todos
    .filter((t) => t.done && t.parent_id == null)
    .sort((a, b) => (b.completed_at ?? '').localeCompare(a.completed_at ?? '')),
)

const pendingGroups = computed<TodoGroup[]>(() => {
  const now = new Date()
  const groups: TodoGroup[] = []
  for (let g = 0; g < GROUP_COUNT; g++) {
    const items = topPending.value
      .filter((t) => groupOf(t, now) === g)
      .sort(compareByOrder)
    if (items.length) groups.push({ label: GROUP_META[g].label, items })
  }
  return groups
})

/** 父待办 → 子待办列表（创建时间倒序）。
 *  一次建 Map 供所有 TodoRow 查找，避免每行各自过滤全部待办 */
const childrenMap = computed(() => {
  const map = new Map<number, Todo[]>()
  for (const t of store.state.todos) {
    if (t.parent_id == null) continue
    const list = map.get(t.parent_id)
    if (list) list.push(t)
    else map.set(t.parent_id, [t])
  }
  for (const list of map.values()) {
    list.sort((a, b) => b.created_at.localeCompare(a.created_at))
  }
  return map
})

// ---- 新增：回车建待办，粘贴「1. a 2. b」序号列表一次拆成多条 ----
async function onAdd() {
  const v = input.value.trim()
  if (!v) return
  const items = parseTodoItems(v)
  const created = await Promise.all(items.map((title) => store.createTodo(title)))
  input.value = ''
  if (created.length > 1) showToast(`已拆成 ${created.length} 条待办`)
  const lastId = created[created.length - 1]?.id
  if (lastId != null) flashHighlight(lastId)
}

// 回车提交（Shift/组合键不拦截，保留换行等默认行为）；IME 组合期间不提交，避免误触
function onAddKeydown(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return
  e.preventDefault()
  void onAdd()
}

// ---- 删除（父条目级联删子）+ 撤销恢复，由 TodoRow 经 provide 调用 ----
async function removeTodo(t: Todo) {
  const kids = store.state.todos.filter((x) => x.parent_id === t.id)
  await store.deleteTodo(t.id)
  showToast(
    kids.length ? `已删除「${t.title}」及 ${kids.length} 条子待办` : `已删除「${t.title}」`,
    { label: '撤销', onClick: () => void restoreTodo(t, kids) },
  )
}

/** 重建父条目后再挂回子待办，恢复创建时间/优先级/排期/完成状态 */
async function restoreTodo(parent: Todo, kids: readonly Todo[]) {
  const p = await store.createTodo(parent.title, null, parent.created_at)
  if (parent.priority !== 0) await store.updateTodo(p.id, parent.title, parent.priority)
  if (parent.due_at != null || parent.remind_at != null) {
    await store.scheduleTodo(p.id, parent.due_at, parent.remind_at)
  }
  if (parent.done) await store.toggleTodo(p.id)
  for (const k of kids) {
    const c = await store.createTodo(k.title, p.id, k.created_at)
    if (k.priority !== 0) await store.updateTodo(c.id, k.title, k.priority)
    if (k.due_at != null || k.remind_at != null) {
      await store.scheduleTodo(c.id, k.due_at, k.remind_at)
    }
    if (k.done) await store.toggleTodo(c.id)
  }
  showToast('已恢复待办')
}

provide('todoOpenSchedule', openSchedule)
provide('todoRemoveTodo', removeTodo)
provide('todoChildren', childrenMap)

// ---- 组内上下拖动排序 ----
// 指针实现而非 HTML5 DnD：Tauri 主窗口的原生拖放拦截（dragDropEnabled）与
// WebView 内 HTML5 拖拽互斥，dragstart 后收不到 dragover/drop（同笔记块拖拽的处理）。
// 语义：仅限同一分组内上下移动（分组由截止日期决定，跨组移动没有排序意义）；
// 落点后把整组 id 顺序写入 sort_order（组内未排序时保持创建时间倒序不动它）。
const todoDragId = ref<number | null>(null)
provide('todoDragStart', onRowPointerDown)
provide('todoDragId', todoDragId)

const bodyRef = ref<HTMLElement | null>(null)
/** 插入指示线：相对 .todo-body 内容的 y 坐标；null = 不显示（拖出组外） */
const dragLineTop = ref<number | null>(null)
let dragState: {
  id: number
  /** 被拖项在组内可见行中的下标 */
  fromIndex: number
  groupLabel: string
  groupEl: HTMLElement
  /** 插入下标（相对含被拖项的可见数组）；null = 拖出组外 */
  insert: number | null
} | null = null

/** TodoRow 顶级行 pointerdown 上报入口；捕获指针，移动超阈值才进入拖拽 */
function onRowPointerDown(t: Todo, e: PointerEvent) {
  if (e.button !== 0 || view.value !== 'pending') return
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
    const label = GROUP_META[groupOf(t, new Date())].label
    const groupEl =
      bodyRef.value?.querySelector<HTMLElement>(`[data-group="${CSS.escape(label)}"]`) ?? null
    if (!groupEl) return false
    const g = pendingGroups.value.find((x) => x.label === label)
    const fromIndex = g ? g.items.findIndex((x) => x.id === t.id) : -1
    if (fromIndex < 0) return false
    dragState = { id: t.id, fromIndex, groupLabel: label, groupEl, insert: null }
    todoDragId.value = t.id
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

/** 指针所在位置 → 插入指示线 y（组内容坐标）+ 记录插入下标 */
function updateDragLine(clientY: number) {
  const ds = dragState
  const body = bodyRef.value
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
  dragLineTop.value =
    edgeY == null ? null : edgeY - body.getBoundingClientRect().top + body.scrollTop
}

/** 松开落点：换算目标顺序，整组写回 sort_order */
function finishDrag() {
  const ds = dragState
  document.body.classList.remove('todo-row-dragging')
  todoDragId.value = null
  dragState = null
  dragLineTop.value = null
  if (!ds || ds.insert == null || ds.fromIndex < 0) return
  const g = pendingGroups.value.find((x) => x.label === ds.groupLabel)
  if (!g) return
  const ids = g.items.map((x) => x.id)
  // 插入下标相对「含被拖项」的数组；先移除再插入需换算
  const final = ds.insert > ds.fromIndex ? ds.insert - 1 : ds.insert
  if (final === ds.fromIndex) return
  const without = ids.filter((_, i) => i !== ds.fromIndex)
  without.splice(final, 0, ds.id)
  void store.reorderTodos(without)
}

function flashHighlight(id: number) {
  highlight.value = id
  if (highlightTimer) clearTimeout(highlightTimer)
  highlightTimer = setTimeout(() => {
    highlight.value = null
  }, 2200)
}

// ---- 截止/提醒排期弹层（日历 + 时间 + 提醒开关） ----
const POP_WIDTH = 288

const popTodoId = ref<number | null>(null)
const popRef = ref<HTMLElement | null>(null)
const popPos = ref({ x: 0, y: 0 })

const calCursor = ref<Date>(startOfDay(new Date()))
const selDay = ref<Date | null>(null)
const selHour = ref(23)
const selMin = ref(59)
const remindOn = ref(false)
const remindHour = ref(9)
const remindMin = ref(0)

const WEEK_LABELS = ['一', '二', '三', '四', '五', '六', '日'] as const
const calCells = computed(() => calendarGrid(calCursor.value, new Date()))
const selHourOptions = HOUR_OPTIONS
const selMinOptions = computed(() => minuteOptions(selMin.value))
const remindMinOptions = computed(() => minuteOptions(remindMin.value))

function openSchedule(t: Todo, anchor: HTMLElement) {
  const today = new Date()
  const base = t.due_at != null ? new Date(t.due_at) : today
  calCursor.value = startOfDay(base)
  selDay.value = startOfDay(base)
  const due = t.due_at != null ? new Date(t.due_at) : null
  selHour.value = due ? due.getHours() : 23
  selMin.value = due ? due.getMinutes() : 59
  remindOn.value = t.remind_at != null
  if (t.remind_at != null) {
    const r = new Date(t.remind_at)
    remindHour.value = r.getHours()
    remindMin.value = r.getMinutes()
  } else if (due) {
    const d = defaultRemindTime(due)
    remindHour.value = d.hour
    remindMin.value = d.minute
  } else {
    remindHour.value = 9
    remindMin.value = 0
  }
  popTodoId.value = t.id
  positionPopup(anchor)
}

/** 锚点下方展开，空间不足翻到上方；水平方向钳制在视口内 */
function positionPopup(anchor: HTMLElement) {
  const rect = anchor.getBoundingClientRect()
  const x = Math.max(8, Math.min(rect.left, window.innerWidth - POP_WIDTH - 8))
  let y = rect.bottom + 8
  popPos.value = { x, y }
  nextTick(() => {
    const el = popRef.value
    if (!el) return
    const h = el.offsetHeight
    if (y + h > window.innerHeight - 8) {
      y = Math.max(8, rect.top - h - 8)
      popPos.value = { x, y }
    }
  })
}

function closeSchedule() {
  popTodoId.value = null
}

function quickDay(kind: 'today' | 'tmr' | 'week') {
  const today = new Date()
  selDay.value =
    kind === 'today'
      ? startOfDay(today)
      : kind === 'tmr'
        ? addDays(startOfDay(today), 1)
        : nextMonday(today)
  calCursor.value = startOfDay(selDay.value)
}

function pickDay(key: string) {
  const [y, m, d] = key.split('-').map(Number)
  selDay.value = new Date(y, m - 1, d)
  calCursor.value = startOfDay(selDay.value)
}

function navMonth(delta: number) {
  calCursor.value = new Date(calCursor.value.getFullYear(), calCursor.value.getMonth() + delta, 1)
}

async function applySchedule() {
  const id = popTodoId.value
  if (id == null || !selDay.value) {
    closeSchedule()
    return
  }
  const due = new Date(selDay.value)
  due.setHours(selHour.value, selMin.value, 0, 0)
  let remind: number | null = null
  if (remindOn.value) {
    const r = new Date(selDay.value)
    r.setHours(remindHour.value, remindMin.value, 0, 0)
    remind = r.getTime()
  }
  await store.scheduleTodo(id, due.getTime(), remind)
  closeSchedule()
  showToast(
    remind != null
      ? `已设截止 ${fmtDay(due)} ${fmtHM(due.getTime())}，提醒 ${fmtHM(remind)}`
      : `已设截止 ${fmtDay(due)} ${fmtHM(due.getTime())}`,
  )
}

async function clearSchedule() {
  const id = popTodoId.value
  if (id == null) return
  await store.scheduleTodo(id, null, null)
  closeSchedule()
  showToast('已清除截止与提醒')
}

// 弹层打开期间 Esc 关闭
function onPopKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') closeSchedule()
}
watch(popTodoId, (v) => {
  if (v != null) window.addEventListener('keydown', onPopKeydown)
  else window.removeEventListener('keydown', onPopKeydown)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onPopKeydown)
})

// 切换视图时收起弹层与瞬态状态
watch(view, () => closeSchedule())

// 全局搜索跳转高亮
watch(
  () => props.highlightId,
  (id) => {
    if (highlightTimer) {
      clearTimeout(highlightTimer)
      highlightTimer = null
    }
    if (id == null) return
    const t = store.state.todos.find((x) => x.id === id)
    if (!t) return
    view.value = t.done ? 'done' : 'pending'
    highlight.value = id
    nextTick(() => {
      const el = document.querySelector<HTMLElement>(`[data-todo-id="${id}"]`)
      el?.scrollIntoView({ block: 'nearest' })
    })
    highlightTimer = setTimeout(() => {
      highlight.value = null
    }, 3000)
  },
  { immediate: true },
)
</script>

<template>
  <section class="card todo-card" aria-label="待办">
    <header class="todo-header">
      <h3 class="todo-title">
        <ListTodo :size="14" :stroke-width="2" aria-hidden="true" />
        <span>待办</span>
      </h3>
      <div class="todo-header-actions">
        <button
          class="todo-float"
          type="button"
          :title="'待办浮窗'"
          :aria-label="'待办浮窗'"
          @click="store.toggleTodoFloat()"
        >
          <PanelTopClose :size="14" :stroke-width="2" aria-hidden="true" />
        </button>
        <div class="filter-tabs todo-seg" role="tablist" aria-label="视图切换">
          <button
            class="filter-tab filter-tab--primary"
            :class="{ active: view === 'pending' }"
            role="tab"
            :aria-selected="view === 'pending'"
            @click="view = 'pending'"
          >
            待办 {{ topPending.length }}
          </button>
          <button
            class="filter-tab filter-tab--primary"
            :class="{ active: view === 'done' }"
            role="tab"
            :aria-selected="view === 'done'"
            @click="view = 'done'"
          >
            已完成 {{ topDone.length }}
          </button>
        </div>
      </div>
    </header>

    <div v-if="view === 'pending'" class="todo-add">
      <textarea
        v-model="input"
        class="todo-input"
        rows="1"
        placeholder="添加待办，回车确认"
        aria-label="添加待办"
        @keydown.enter="onAddKeydown"
      ></textarea>
      <p class="todo-input-hint">回车：新建待办　·　粘贴序号列表（1. 2. 3.）可拆成多条</p>
    </div>

    <div ref="bodyRef" class="todo-body">
      <template v-if="view === 'pending'">
        <div v-if="pendingGroups.length === 0" class="empty-state todo-empty">
          <p>今天要做什么？</p>
          <p>按回车快速添加</p>
        </div>
        <div v-for="g in pendingGroups" :key="g.label" class="todo-group" :data-group="g.label">
          <div class="todo-group-head">
            <span class="glabel">{{ g.label }}</span>
            <span class="gline"></span>
            <span class="gcount">{{ g.items.length }}</span>
          </div>
          <TodoRow
            v-for="t in g.items"
            :key="t.id"
            :todo="t"
            :highlight-id="highlight"
          />
        </div>
        <div
          v-if="dragLineTop != null"
          class="todo-drag-line"
          :style="{ top: dragLineTop + 'px' }"
          aria-hidden="true"
        ></div>
      </template>

      <template v-else>
        <div v-if="topDone.length === 0" class="empty-state todo-empty">
          <p>暂无已完成</p>
        </div>
        <div v-else class="todo-group">
          <TodoRow
            v-for="t in topDone"
            :key="t.id"
            :todo="t"
            :highlight-id="highlight"
          />
        </div>
      </template>
    </div>

    <Teleport to="body">
      <template v-if="popTodoId != null">
        <div class="pop-mask" @click="closeSchedule"></div>
        <div
          ref="popRef"
          class="schedule-pop"
          role="dialog"
          aria-label="截止日期与提醒"
          :style="{ left: popPos.x + 'px', top: popPos.y + 'px', width: POP_WIDTH + 'px' }"
        >
          <div class="sp-title">截止日期与提醒</div>
          <div class="sp-quick">
            <button class="sp-chip" type="button" @click="quickDay('today')">今天</button>
            <button class="sp-chip" type="button" @click="quickDay('tmr')">明天</button>
            <button class="sp-chip" type="button" @click="quickDay('week')">下周一</button>
          </div>
          <div class="sp-cal">
            <div class="sp-cal-head">
              <span class="sp-cal-title">
                {{ calCursor.getFullYear() }}年{{ calCursor.getMonth() + 1 }}月
              </span>
              <div class="sp-cal-nav">
                <button class="sp-nav-btn" type="button" aria-label="上个月" @click="navMonth(-1)">
                  ‹
                </button>
                <button class="sp-nav-btn" type="button" aria-label="下个月" @click="navMonth(1)">
                  ›
                </button>
              </div>
            </div>
            <div class="sp-week">
              <span v-for="w in WEEK_LABELS" :key="w">{{ w }}</span>
            </div>
            <div class="sp-days">
              <button
                v-for="c in calCells"
                :key="c.key"
                class="sp-day"
                :class="{
                  out: c.out,
                  today: c.today,
                  sel: selDay != null && isoKey(selDay) === c.key,
                  dim: c.key < isoKey(new Date()),
                }"
                type="button"
                @click="pickDay(c.key)"
              >
                {{ c.day }}
              </button>
            </div>
          </div>
          <div class="sp-time-row">
            <label>截止时间</label>
            <div class="sp-time-wrap">
              <select v-model.number="selHour" class="sp-select" aria-label="截止小时">
                <option v-for="h in selHourOptions" :key="h.value" :value="h.value">
                  {{ h.label }}
                </option>
              </select>
              <span class="sp-colon">:</span>
              <select v-model.number="selMin" class="sp-select" aria-label="截止分钟">
                <option v-for="m in selMinOptions" :key="m.value" :value="m.value">
                  {{ m.label }}
                </option>
              </select>
            </div>
          </div>
          <div class="sp-remind">
            <div class="r-label"><b>提醒我</b>到点弹系统通知</div>
            <button
              class="sp-toggle"
              :class="{ on: remindOn }"
              type="button"
              role="switch"
              :aria-checked="remindOn"
              aria-label="提醒开关"
              @click="remindOn = !remindOn"
            ><i></i></button>
          </div>
          <div v-if="remindOn" class="sp-remind">
            <div class="r-label">提醒时间</div>
            <div class="sp-time-wrap">
              <select v-model.number="remindHour" class="sp-select" aria-label="提醒小时">
                <option v-for="h in selHourOptions" :key="h.value" :value="h.value">
                  {{ h.label }}
                </option>
              </select>
              <span class="sp-colon">:</span>
              <select v-model.number="remindMin" class="sp-select" aria-label="提醒分钟">
                <option v-for="m in remindMinOptions" :key="m.value" :value="m.value">
                  {{ m.label }}
                </option>
              </select>
            </div>
          </div>
          <div class="sp-actions">
            <button class="sp-btn clear" type="button" @click="clearSchedule">清除</button>
            <button class="sp-btn ok" type="button" @click="applySchedule">确定</button>
          </div>
        </div>
      </template>
    </Teleport>
  </section>
</template>

<style scoped>
.todo-card {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
  --todo-pri-default: #c6cad4;
  /* 待办模块字号：全局基准 × 模块系数 */
  font-size: calc(1rem * var(--fs-todo, 1));
}
[data-theme='dark'] .todo-card {
  --todo-pri-default: #52525f;
}
.todo-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.todo-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  white-space: nowrap;
  margin: 0;
}
.todo-title :deep(svg) {
  color: var(--brand-500);
}
.todo-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.todo-float {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  transition: color 0.18s, background 0.18s;
}
.todo-float:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.todo-seg {
  gap: 4px;
  flex-shrink: 0;
}
.todo-seg .filter-tab {
  padding: 4px 8px;
  font-size: 0.6875em;
}

.todo-add {
  margin-bottom: 8px;
}
.todo-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125em;
  padding: 7px 10px;
  outline: none;
  transition: border-color 0.18s, box-shadow 0.18s, background 0.18s;
  display: block;
  resize: none;
  line-height: 1.45;
  overflow-y: auto;
}
.todo-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: color-mix(in srgb, var(--input-bg) 88%, #fff);
}
.todo-input::placeholder {
  color: var(--text-4);
}
.todo-input-hint {
  margin: 4px 0 0;
  font-size: 0.625em;
  color: var(--text-4);
  line-height: 1.5;
}

.todo-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  margin: 0 -4px;
  padding: 0 4px;
  position: relative; /* 拖拽插入线的定位基准 */
}

/* 拖拽插入线（绝对定位于 .todo-body，随内容滚动） */
.todo-drag-line {
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
.todo-drag-line::before {
  content: '';
  position: absolute;
  left: -1px;
  top: -2px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--brand-500);
}
/* 拖拽期间全局禁选 + 抓手光标（body 在组件外，用 :global 逃出 scoped） */
:global(body.todo-row-dragging) {
  cursor: grabbing;
  user-select: none;
  -webkit-user-select: none;
}

/* ---- 分组 ---- */
.todo-group {
  margin-top: 12px;
}
.todo-group:first-child {
  margin-top: 0;
}
.todo-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 6px 4px;
}
.todo-group-head .glabel {
  font-size: 0.65625em;
  font-weight: 600;
  color: var(--text-3);
  letter-spacing: 0.02em;
}
.todo-group-head .gline {
  flex: 1;
  height: 1px;
  background: var(--border-soft);
}
.todo-group-head .gcount {
  font-size: 0.625em;
  color: var(--text-4);
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 0 7px;
  line-height: 15px;
}

.todo-empty {
  padding: 28px 8px;
}
.todo-empty p {
  margin: 0;
  font-size: 0.75em;
  color: var(--text-4);
}
.todo-empty p:first-child {
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-3);
}

/* ---- 排期弹层（Teleport 到 body，瞬态表面可用 backdrop-filter） ---- */
.pop-mask {
  position: fixed;
  inset: 0;
  z-index: 40;
  background: transparent;
}
.schedule-pop {
  position: fixed;
  z-index: 50;
  font-size: calc(1rem * var(--fs-todo, 1));
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dock);
  padding: 14px;
  animation: sp-pop-in 0.16s cubic-bezier(0.16, 1, 0.3, 1);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
}
@keyframes sp-pop-in {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.97);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
.sp-title {
  font-size: 0.75em;
  font-weight: 700;
  color: var(--text-1);
  margin-bottom: 8px;
}
.sp-quick {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  margin-bottom: 10px;
}
.sp-chip {
  border: 1px solid var(--border-strong);
  background: var(--bg-card-soft);
  color: var(--text-2);
  border-radius: var(--radius-pill);
  padding: 3px 10px;
  font-size: 0.6875em;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.18s, color 0.18s, border-color 0.18s;
}
.sp-chip:hover {
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
}

.sp-cal {
  margin-bottom: 10px;
}
.sp-cal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}
.sp-cal-title {
  font-size: 0.75em;
  font-weight: 600;
  color: var(--text-1);
}
.sp-cal-nav {
  display: flex;
  gap: 4px;
}
.sp-nav-btn {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.875em;
  line-height: 1;
}
.sp-nav-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.sp-week,
.sp-days {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}
.sp-week span {
  text-align: center;
  font-size: 0.625em;
  color: var(--text-4);
  font-weight: 600;
  padding: 3px 0;
}
.sp-day {
  border: none;
  background: transparent;
  border-radius: 6px;
  height: 27px;
  font-size: 0.6875em;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
  position: relative;
}
.sp-day:hover {
  background: var(--bg-card-soft);
}
.sp-day.out {
  color: var(--text-4);
  opacity: 0.45;
}
.sp-day.today {
  color: var(--brand-500);
  font-weight: 700;
}
.sp-day.sel {
  background: var(--brand-500);
  color: var(--text-on-accent);
  font-weight: 600;
}
.sp-day.sel:hover {
  background: var(--brand-600);
}
.sp-day.dim::after {
  content: '';
  position: absolute;
  left: 50%;
  bottom: 2px;
  transform: translateX(-50%);
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: var(--c-red-soft);
}

.sp-time-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.sp-time-row label {
  font-size: 0.6875em;
  color: var(--text-3);
  width: 48px;
  flex-shrink: 0;
}
.sp-time-wrap {
  display: flex;
  align-items: center;
  gap: 4px;
}
.sp-select {
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  color: var(--text-1);
  border-radius: 6px;
  padding: 4px 6px;
  font-size: 0.75em;
  outline: none;
  width: 4em;
  text-align: center;
  font-family: inherit;
}
.sp-select:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.sp-colon {
  color: var(--text-3);
  font-size: 0.75em;
}

.sp-remind {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  margin-bottom: 12px;
}
.sp-remind .r-label {
  font-size: 0.6875em;
  color: var(--text-2);
  flex: 1;
}
.sp-remind .r-label b {
  display: block;
  color: var(--text-1);
  font-weight: 600;
  margin-bottom: 2px;
}
.sp-toggle {
  position: relative;
  width: 30px;
  height: 18px;
  flex-shrink: 0;
  border-radius: var(--radius-pill);
  background: var(--border-strong);
  cursor: pointer;
  transition: background 0.18s;
  border: none;
  padding: 0;
}
.sp-toggle.on {
  background: var(--brand-500);
}
.sp-toggle i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #fff;
  transition: transform 0.18s;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
}
.sp-toggle.on i {
  transform: translateX(12px);
}

.sp-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.sp-btn {
  border: none;
  border-radius: var(--radius-pill);
  padding: 5px 14px;
  font-size: 0.6875em;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.18s, color 0.18s, transform 0.18s;
}
.sp-btn:hover {
  transform: translateY(-1px);
}
.sp-btn:active {
  transform: scale(0.96);
}
.sp-btn.clear {
  background: transparent;
  color: var(--text-4);
}
.sp-btn.clear:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.sp-btn.ok {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.sp-btn.ok:hover {
  background: var(--brand-600);
  box-shadow: 0 4px 12px var(--brand-glow);
}
</style>
