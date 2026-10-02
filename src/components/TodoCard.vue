<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, provide, ref, watch } from 'vue'
import { ArrowRight, ListTodo, PanelTopClose } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'
import { parseTodoItems } from '../utils/todoParse'
import { useTodoChildren } from '../composables/useTodoChildren'
import { useTodoDrag } from '../composables/useTodoDrag'
import { useTodoRestore } from '../composables/useTodoRestore'
import TodoRow from './TodoRow.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import AppSelect from './AppSelect.vue'
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

const props = defineProps<{
  highlightId?: number | null
  title?: string
  hideTitle?: boolean
  /** 标题栏右侧「打开待办视图 →」：进独立待办视图（标签筛选 / 日历 / 周期待办） */
  onOpenDetail?: () => void
}>()

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

// ---- 列表派生：待办按 置顶 → 逾期 → 今天 → 有日期 → 无日期 分组（置顶独立成组，
// 新增待办只会落进日期组、不会盖到置顶条目上面），组内按手动排序/创建时间倒序 ----
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

/** 父待办 → 子待办列表（与待办浮窗共用，见 useTodoChildren） */
const childrenMap = useTodoChildren()

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
/** 撤销恢复（与待办视图共用一份，见 useTodoRestore）：描述/置顶/标签/周期规则都会回填 */
const restoreTodo = useTodoRestore()

async function removeTodo(t: Todo) {
  const kids = store.state.todos.filter((x) => x.parent_id === t.id)
  // 标签关联是 ON DELETE CASCADE，删除后查不回来 → 删前先抓快照
  const tagIds = store.todoTagIds(t.id)
  await store.deleteTodo(t.id)
  showToast(
    kids.length ? `已删除「${t.title}」及 ${kids.length} 条子待办` : `已删除「${t.title}」`,
    {
      label: '撤销',
      onClick: () => void restoreTodo(t, kids, tagIds).catch(() => showToast('恢复失败，请重试')),
    },
  )
}

provide('todoOpenSchedule', openSchedule)
provide('todoRemoveTodo', removeTodo)
provide('todoChildren', childrenMap)
// 当前视图：子待办拖拽仅待办视图开放（同顶级行的约束）
provide('todoView', view)

// ---- 子行确认弹窗（宿主层单实例）----
// 每行各挂一个 ConfirmDialog 会随行数线性增长（50 行 = 50 个组件 + 50 个挂载点），
// 上移到卡片层：子行只管发请求，弹窗由这里唯一一份渲染。
const rowConfirm = ref<{ todo: Todo; kids: number; onConfirm: () => void } | null>(null)
provide('todoRowConfirm', (todo: Todo, kids: number, onConfirm: () => void) => {
  rowConfirm.value = { todo, kids, onConfirm }
})

function onRowConfirm() {
  const c = rowConfirm.value
  rowConfirm.value = null
  c?.onConfirm()
}

// ---- 组内上下拖动排序 ----
// 指针实现而非 HTML5 DnD：Tauri 主窗口的原生拖放拦截与 WebView 内 HTML5 拖拽互斥
// （dragstart 后收不到 dragover/drop，同笔记块拖拽的处理）。
// 具体逻辑抽在 useTodoDrag —— 待办视图复用同一份，避免两套拖拽实现各改一处。
const bodyRef = ref<HTMLElement | null>(null)
const {
  dragId: todoDragId,
  lineTop: dragLineTop,
  onRowPointerDown,
} = useTodoDrag({
  bodyRef,
  groups: pendingGroups,
  labelOf: (t) => GROUP_META[groupOf(t, new Date())].label,
  enabled: () => view.value === 'pending',
  reorder: (ids) => void store.reorderTodos(ids),
})
provide('todoDragStart', onRowPointerDown)
provide('todoDragId', todoDragId)

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
const selMinOptions = computed(() => minuteOptions(selMin.value))
const remindMinOptions = computed(() => minuteOptions(remindMin.value))
// AppSelect 的值为字符串，时分选项在此转一次（下拉一律走系统组件，不用原生 <select>）
const hourSelectOptions = HOUR_OPTIONS.map((h) => ({ value: String(h.value), label: h.label }))
const selMinSelectOptions = computed(() =>
  selMinOptions.value.map((m) => ({ value: String(m.value), label: m.label })),
)
const remindMinSelectOptions = computed(() =>
  remindMinOptions.value.map((m) => ({ value: String(m.value), label: m.label })),
)

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
  <section class="card todo-card" :aria-label="title ?? '待办'">
    <header class="todo-header" :class="{ 'hd-float': hideTitle }">
      <h3 v-if="!hideTitle" class="todo-title">
        <ListTodo :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '待办' }}</span>
      </h3>
      <div class="todo-header-actions">
        <button
          v-if="props.onOpenDetail"
          class="todo-float"
          type="button"
          title="打开待办视图（标签 / 日历 / 周期）"
          aria-label="打开待办视图"
          @click="props.onOpenDetail()"
        >
          <ArrowRight :size="14" :stroke-width="2" aria-hidden="true" />
        </button>
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
              <AppSelect
                class="sp-select"
                compact
                :model-value="String(selHour)"
                :options="hourSelectOptions"
                aria-label="截止小时"
                @update:model-value="selHour = Number($event)"
              />
              <span class="sp-colon">:</span>
              <AppSelect
                class="sp-select"
                compact
                :model-value="String(selMin)"
                :options="selMinSelectOptions"
                aria-label="截止分钟"
                @update:model-value="selMin = Number($event)"
              />
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
              <AppSelect
                class="sp-select"
                compact
                :model-value="String(remindHour)"
                :options="hourSelectOptions"
                aria-label="提醒小时"
                @update:model-value="remindHour = Number($event)"
              />
              <span class="sp-colon">:</span>
              <AppSelect
                class="sp-select"
                compact
                :model-value="String(remindMin)"
                :options="remindMinSelectOptions"
                aria-label="提醒分钟"
                @update:model-value="remindMin = Number($event)"
              />
            </div>
          </div>
          <div class="sp-actions">
            <button class="sp-btn clear" type="button" @click="clearSchedule">清除</button>
            <button class="sp-btn ok" type="button" @click="applySchedule">确定</button>
          </div>
        </div>
      </template>
    </Teleport>

    <!-- 子行确认弹窗：宿主层单实例（每行各挂一个会随行数线性增长） -->
    <ConfirmDialog
      :visible="rowConfirm != null"
      title="还有子待办未完成"
      :message="`「${rowConfirm?.todo.title ?? ''}」下还有 ${rowConfirm?.kids ?? 0} 条子待办未完成，是否确认完成？`"
      hint="确认后会一并勾选这些子待办；取消则本条待办保持未完成。"
      confirm-text="确认并勾选子项"
      @confirm="onRowConfirm"
      @cancel="rowConfirm = null"
    />
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
/* 关闭标题：表头整条不占位，动作按钮由全局 .hd-float 悬浮在卡片右上角（见 style.css） */
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
/* 置顶组头品牌色（与待办视图 .tv-group-h.pinned 同语言） */
.todo-group[data-group='置顶'] .glabel {
  color: var(--brand-500);
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
  gap: 10px;
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
  gap: 6px;
}
/* 时分下拉：尺寸走 AppSelect 的紧凑档，这里只管宽度与聚焦环 */
.sp-time-wrap :deep(.sp-select) {
  width: 4.6em;
}
.sp-time-wrap :deep(.sp-select:focus-visible) {
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
