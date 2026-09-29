<script setup lang="ts">
import { computed, inject, nextTick, ref, shallowRef, type ComputedRef, type ComponentPublicInstance, type Ref } from 'vue'
import {
  AlertTriangle,
  AlignLeft,
  Bell,
  CalendarDays,
  Check,
  ChevronDown,
  ChevronRight,
  Plus,
  Repeat,
  Pin,
  Sun,
  Trash2,
  X,
} from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'
import {
  compareChildOrder,
  dueBadge,
  doneAtLabel,
  fmtHM,
  repeatEndLabel,
  repeatLabel,
} from '../utils/todoSchedule'
import { useTodoSubDrag } from '../composables/useTodoSubDrag'
import ConfirmDialog from './ConfirmDialog.vue'

/**
 * 待办行（TodoCard 的递归子组件）：父条目与子待办共用一套行渲染。
 * 子待办嵌在父行 .todo-mid 内缩进展示，规则与 docs/prototypes/todo-schedule-prototype.html 一致：
 * 排期徽标（逾期红/今天橙/明天品牌色/其他灰）点击弹出排期层（由卡片提供）；
 * 删除经卡片统一处理（父条目级联删子 + 撤销）。
 */
const props = withDefaults(
  defineProps<{
    todo: Todo
    /** 子待办行：小号勾选、无优先级圆点、无「+」按钮 */
    isSub?: boolean
    highlightId?: number | null
    /** 拖拽中的子待办行（由宿主父行传入；多根组件无法继承 class，走 prop） */
    dragging?: boolean
  }>(),
  { isSub: false, highlightId: null, dragging: false },
)

const emit = defineEmits<{ subdrag: [e: PointerEvent] }>()

const store = useStore()
/** 轻提示（卡片层提供；浮窗等宿主没有时为 noop） */
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)
/** 宿主层单实例确认弹窗（TodoCard 提供）；浮窗等宿主没有时回退到本行自带的那一个 */
const requestRowConfirm = inject<
  ((t: Todo, kids: number, onConfirm: () => void) => void) | null
>('todoRowConfirm', null)
/** 排期弹层（卡片层提供）：浮窗等宿主没有弹层时为 null，日期徽标转只读展示、隐藏「日期」按钮 */
const openSchedule = inject<((t: Todo, el: HTMLElement) => void) | null>('todoOpenSchedule', null)
const canSchedule = openSchedule != null
const removeTodo = inject<(t: Todo) => void>('todoRemoveTodo', () => {})
/** 卡片层统一构建的 父id → 子待办 列表（创建时间倒序），避免每行各自过滤 */
const childrenMap = inject<ComputedRef<Map<number, Todo[]>>>(
  'todoChildren',
  computed(() => new Map()),
)
/** 组内上下拖动：仅顶级行上报给卡片层（子待办不支持拖动），由卡片决定落点与持久化 */
const dragStart = inject<(t: Todo, e: PointerEvent) => void>('todoDragStart', () => {})
const dragId = inject<Ref<number | null>>('todoDragId', ref(null))
/** 当前视图（卡片提供）：子待办拖拽仅待办视图开放，同顶级行 */
const view = inject<Ref<'pending' | 'done'>>('todoView', ref('pending'))

const PRIORITY_LABELS = ['普通', '重要', '紧急'] as const
const PRIORITY_BG = ['var(--todo-pri-default)', 'var(--c-yellow-soft)', 'var(--c-red-soft)'] as const

// 子待办（仅一层）
const kids = computed(() => childrenMap.value.get(props.todo.id) ?? [])
const doneKids = computed(() => kids.value.filter((k) => k.done).length)

// ---- 子待办折叠/展开：默认展开；行内瞬态状态，不持久化（切走再回来仍展开） ----
const collapsed = ref(false)

function toggleCollapse() {
  collapsed.value = !collapsed.value
}

const badge = computed(() => (props.todo.done ? null : dueBadge(props.todo, new Date())))
/** 已完成行的完成时间徽标（勾选时后端写入 completed_at；null = 旧数据缺失，行内不显示） */
const doneAt = computed(() => (props.todo.done ? doneAtLabel(props.todo.completed_at, new Date()) : null))
/** 周期待办：规则文案 + 结束条件（空串 = 一次性待办） */
const repeatText = computed(() => (props.todo.repeat_mode === 'once' ? '' : repeatLabel(props.todo)))
const repeatTitle = computed(() => {
  if (props.todo.repeat_mode === 'once') return ''
  const end = repeatEndLabel(props.todo)
  const base = `${repeatLabel(props.todo)}${end ? ` · ${end}` : ''}`
  return `${base} · 已累计完成 ${props.todo.repeat_done_count} 次（勾选=完成本轮，日期滚到下一轮）`
})
/** 该条待办的标签（待办专属定义，与笔记标签无关） */
const tagChips = computed(() => {
  const ids = store.todoTagIds(props.todo.id)
  if (!ids.length) return []
  return store.state.todoTags.filter((t) => ids.includes(t.id))
})
const remindOn = computed(() => !props.todo.done && props.todo.remind_at != null)
const showProgress = computed(() => !props.isSub && !props.todo.done && kids.value.length > 0)

function onBadgeClick(e: MouseEvent) {
  if (!canSchedule) return
  const el = e.currentTarget
  if (el instanceof HTMLElement) openSchedule(props.todo, el)
}

/** 点击图钉立即取消置顶（置顶与日期无关，取消后回到按日期分组的位置） */
function unpin() {
  void store
    .setTodoPinned(props.todo.id, false)
    .then(() => showToast(`「${props.todo.title}」已取消置顶`))
    .catch(() => showToast('取消置顶失败，请重试'))
}

function onRowPointerDown(e: PointerEvent) {
  const target = e.target as HTMLElement | null
  // 子待办行嵌在父行 DOM 内，按下会冒泡到父行监听——
  // 只响应最近 .todo-row 恰为当前行的按下，避免拖子待办误拖起父待办
  if (target && target.closest('.todo-row') !== e.currentTarget) return
  if (props.isSub) {
    // 子待办行：上报给宿主父行（kids 列表与容器都在父行实例上）做组内拖拽
    emit('subdrag', e)
    return
  }
  dragStart(props.todo, e)
}

/** 提交中标记：快速双击会让同一条连续翻转两次（视觉上「勾了又弹回」） */
const busy = ref(false)

async function toggle() {
  const wasDone = props.todo.done
  // 勾父带子：父待办标记完成时，未完成的子待办一并勾上（取消完成不连带走子待办，保留各自进度）。
  // 有未完成子项时先确认——静默连勾用户看不见，容易误以为只勾了父条目。
  if (!wasDone && kids.value.some((k) => !k.done)) {
    const n = kids.value.filter((k) => !k.done).length
    if (requestRowConfirm) {
      // 宿主（卡片/视图）有单实例弹窗就用它，本行不再各挂一个
      requestRowConfirm(props.todo, n, () => void applyToggle())
      return
    }
    confirmKids.value = n
    showKidsConfirm.value = true
    return
  }
  await applyToggle()
}

async function applyToggle() {
  if (busy.value) return
  busy.value = true
  try {
    const wasDone = props.todo.done
    // 周期待办：勾选 = 「完成本轮」，日期滚到下一轮（不置 done、不进已完成列表）。
    // 就地滚动后给一条可撤销提示，否则用户会以为点了没反应。
    if (!wasDone && props.todo.repeat_mode !== 'once') {
      const updated = await store.completeTodoRecurring(props.todo.id)
      if (updated) {
        const next = updated.repeat_mode === 'once' ? '周期已结束' : `已滚到 ${nextLabel(updated)}`
        showToast(next, {
          label: '撤销',
          onClick: () => void store.undoTodoRecurring(props.todo.id),
        })
      }
      return
    }
    await store.toggleTodo(props.todo.id)
    if (wasDone) return
    for (const k of kids.value) {
      if (!k.done) await store.toggleTodo(k.id)
    }
  } catch {
    showToast('待办状态没能保存，请重试')
  } finally {
    busy.value = false
  }
}

/** 滚动后的下一个截止文案：今天 / 明天 / M月D日 */
function nextLabel(t: Todo): string {
  if (t.due_at == null) return '下一轮'
  const d = new Date(t.due_at)
  const badge = dueBadge({ due_at: t.due_at }, new Date())
  if (badge && (badge.kind === 'today' || badge.kind === 'tmr')) return badge.text
  return `${d.getMonth() + 1}月${d.getDate()}日`
}

/** 确认弹窗状态：未完成子待办数量（> 0 时弹窗） */
const showKidsConfirm = ref(false)
const confirmKids = ref(0)

async function onKidsConfirm() {
  showKidsConfirm.value = false
  await applyToggle()
}

function onKidsCancel() {
  showKidsConfirm.value = false
}

async function cyclePriority() {
  await store.updateTodo(props.todo.id, props.todo.title, (props.todo.priority + 1) % 3)
}

// ---- 子待办拖拽排序 / 跨父改挂 ----
// 指针实现原因同卡片层顶级行拖拽：Tauri 主窗口原生拖放拦截与 HTML5 DnD 互斥。
// 子行 pointerdown 经 subdrag 事件上报到宿主父行：移动超阈值才激活。
// 落点不再限于本父行——用 elementFromPoint 命中任意顶级行，落到它的子列表；
// 命中空 / 折叠父行时，共享的 dropTargetParentId 让该行临时展开出放置区。
// 仅「未完成」视图开放（同顶级行）。
const { subDragId, dropTargetParentId, dropLineClientY, dropLineLeft, dropLineWidth } =
  useTodoSubDrag()
/** 本行是否为当前拖拽源（只有源行绘制那唯一一条插入线） */
const ownsSubDrag = ref(false)

const isSubDropTarget = computed(
  () => !props.isSub && subDragId.value != null && dropTargetParentId.value === props.todo.id,
)

/** 目标父 id → 其子待办（按 compareChildOrder，与 childrenMap 同口径） */
function kidsOfParent(parentId: number): Todo[] {
  return store.state.todos.filter((t) => t.parent_id === parentId).sort(compareChildOrder)
}

let subDragState: {
  id: number
  /** 拖拽项当前所属父（区分同父重排 vs 跨父改挂） */
  sourceParentId: number
  /** 插入下标（相对目标父下含被拖项的可见数组）；null = 未命中有效落点 */
  insert: number | null
  targetParentId: number | null
} | null = null

function onSubDragStart(k: Todo, e: PointerEvent) {
  if (e.button !== 0 || view.value !== 'pending') return
  const target = e.target as HTMLElement | null
  // 勾选/删除等交互控件上按下不启动拖拽
  if (target?.closest('button, textarea, input, a, [data-no-drag]')) return
  const el = e.currentTarget as HTMLElement
  const startX = e.clientX
  const startY = e.clientY
  let active = false
  let dead = false
  el.addEventListener('pointermove', onMove)
  el.addEventListener('pointerup', onUp)
  el.addEventListener('pointercancel', onUp)
  // window 兜底：激活前未捕获指针，快速甩动时 up 不落在行上也能清理
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)

  function begin(): boolean {
    // 捕获指针：拖出窗口松开也能收到 pointerup，不会悬挂在拖拽态
    try {
      el.setPointerCapture(e.pointerId)
    } catch {
      /* 指针已释放时忽略，window 监听兜底场景极少 */
    }
    subDragState = { id: k.id, sourceParentId: props.todo.id, insert: null, targetParentId: null }
    subDragId.value = k.id
    dropTargetParentId.value = props.todo.id
    ownsSubDrag.value = true
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
    updateSubDragLine(ev.clientX, ev.clientY)
  }

  function onUp() {
    el.removeEventListener('pointermove', onMove)
    el.removeEventListener('pointerup', onUp)
    el.removeEventListener('pointercancel', onUp)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    // 捕获成功时同一 pointerup 可能到两次（el 目标阶段 + window 冒泡）；
    // 归零 active 保证 finishSubDrag 只执行一次
    if (!active) return
    active = false
    finishSubDrag()
  }
}

/**
 * 指针位置 → 落点：取最靠近的顶级 `.todo-row`，用其子列表算插入下标，
 * 并把插入线（视口坐标）写进共享态；未命中顶级行则清空落点。
 */
function updateSubDragLine(clientX: number, clientY: number) {
  const ds = subDragState
  if (!ds) return
  const under = document.elementFromPoint(clientX, clientY) as HTMLElement | null
  // 命中子行时 closest 会越过它找到所属顶级行；拖到顶级行本身即落该父下
  const targetRow = under?.closest<HTMLElement>('.todo-row:not(.sub)') ?? null
  const pidRaw = targetRow?.dataset.todoId
  if (!targetRow || !pidRaw) {
    ds.insert = null
    ds.targetParentId = null
    dropTargetParentId.value = null
    dropLineClientY.value = null
    return
  }
  const pid = Number(pidRaw)
  const subsEl = targetRow.querySelector<HTMLElement>('.todo-subs')
  const rowRect = targetRow.getBoundingClientRect()
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
    insert = kidsOfParent(pid).length
    edgeY = rowRect.bottom
    left = rowRect.left + 24
    width = Math.max(rowRect.width - 24, 40)
  }
  ds.insert = insert
  ds.targetParentId = pid
  dropTargetParentId.value = pid
  dropLineClientY.value = edgeY
  dropLineLeft.value = left
  dropLineWidth.value = width
}

/** 松开落点：同父 = 重排；跨父 = 改挂（写 parent_id + 目标父下整列 sort_order） */
function finishSubDrag() {
  const ds = subDragState
  document.body.classList.remove('todo-row-dragging')
  subDragId.value = null
  dropTargetParentId.value = null
  dropLineClientY.value = null
  ownsSubDrag.value = false
  subDragState = null
  if (!ds || ds.insert == null || ds.targetParentId == null) return
  const targetKids = kidsOfParent(ds.targetParentId)
  if (ds.targetParentId === ds.sourceParentId) {
    // 同父：插入下标相对「含被拖项」的数组，先移除再插入需换算
    const ids = targetKids.map((x) => x.id)
    const from = ids.indexOf(ds.id)
    if (from < 0) return
    const final = ds.insert > from ? ds.insert - 1 : ds.insert
    if (final === from) return
    const without = ids.filter((_, i) => i !== from)
    without.splice(final, 0, ds.id)
    void store.reorderTodos(without)
  } else {
    const ids = targetKids.map((x) => x.id)
    ids.splice(Math.min(ds.insert, ids.length), 0, ds.id)
    void store.moveTodoChild(ds.id, ds.targetParentId, ids).catch(() => showToast('移动失败，请重试'))
  }
}

// ---- 行内编辑（双击内容，600ms 自动保存体系外的显式提交） ----
const editing = ref(false)
const editText = ref('')
const editInputRef = ref<HTMLTextAreaElement | null>(null)
function setEditInput(el: Element | ComponentPublicInstance | null) {
  editInputRef.value = el instanceof HTMLTextAreaElement ? el : null
}

function startEdit() {
  editing.value = true
  editText.value = props.todo.title
  nextTick(() => {
    const el = editInputRef.value
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
    el.focus()
  })
}

function autoResizeEdit(e: Event) {
  const el = e.target as HTMLTextAreaElement
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight}px`
}

function commitEdit() {
  if (!editing.value) return
  editing.value = false
  const title = editText.value.trim()
  if (!title || title === props.todo.title) return
  void store.updateTodo(props.todo.id, title, props.todo.priority)
}

/** 回车保存（Shift+回车换行，组合键不拦截）；Esc 取消见模板 */
function onEditKeydown(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return
  e.preventDefault()
  commitEdit()
}

// ---- 添加子待办 ----
const addingSub = ref(false)
const subText = ref('')
const subInputRef = ref<HTMLInputElement | null>(null)
function setSubInput(el: Element | ComponentPublicInstance | null) {
  subInputRef.value = el instanceof HTMLInputElement ? el : null
}

function toggleSubAdd() {
  addingSub.value = !addingSub.value
  subText.value = ''
  if (addingSub.value) {
    // 折叠中点「+」先展开，保证输入行与既有子待办一起可见
    collapsed.value = false
    nextTick(() => {
      const el = subInputRef.value
      el?.focus()
      el?.scrollIntoView({ block: 'nearest' })
    })
  }
}

function commitSub() {
  const title = subText.value.trim()
  if (title) void store.createTodo(title, props.todo.id)
  addingSub.value = false
  subText.value = ''
}

function onSubKeydown(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.key === 'Enter') commitSub()
}

/** 失焦即提交（有内容）或收起（空），点取消按钮用 mousedown.prevent 跳过 blur */
function onSubBlur() {
  if (!addingSub.value) return
  const title = subText.value.trim()
  if (title) commitSub()
  else addingSub.value = false
}

// ---- 悬浮卡片：标题被截断时补全文案 + 展示描述（Markdown 渲染） ----
// 描述在 0.6.5 只有编辑弹层能写、列表里没有任何展示位（用户写了看不到），
// 这里用「鼠标挪到行上悬浮展示」补齐：不占行高，也不打断列表排布。
const tip = ref<{ visible: boolean; title: string; x: number; y: number }>({
  visible: false,
  title: '',
  x: 0,
  y: 0,
})

// marked + DOMPurify 按需加载：浮窗是独立窗口（只用到 TodoRow），
// 静态引入会把这两个库塞进浮窗首屏包；首次悬浮时才拉，之后常驻。
const md = shallowRef<typeof import('../utils/markdownHtml') | null>(null)
let mdLoading = false
function ensureMd() {
  if (md.value || mdLoading) return
  mdLoading = true
  void import('../utils/markdownHtml').then((m) => {
    md.value = m
  })
}
const descHtml = computed(() => (md.value ? md.value.renderMarkdown(props.todo.description) : ''))

function onRowEnter(e: MouseEvent) {
  const row = e.currentTarget as HTMLElement | null
  if (!row) return
  const label = row.querySelector<HTMLElement>('.todo-label')
  // 标题被 5 行截断时才有必要补全文案（否则与行内文字重复）
  const clamped = label != null && label.scrollHeight > label.clientHeight + 2
  const hasDesc = props.todo.description.trim() !== ''
  if (!clamped && !hasDesc) return
  if (hasDesc) ensureMd()
  const rect = row.getBoundingClientRect()
  tip.value = {
    visible: true,
    title: clamped ? props.todo.title : '',
    x: rect.left,
    y: rect.bottom + 6,
  }
}

function hideTip() {
  tip.value.visible = false
}
</script>

<template>
  <div
    class="todo-row"
    :class="{ done: todo.done, sub: isSub, highlight: todo.id === highlightId, dragging: todo.id === dragId || dragging, 'sub-drop-target': isSubDropTarget }"
    :data-todo-id="todo.id"
    @pointerdown="onRowPointerDown"
    @mouseenter="onRowEnter"
    @mouseleave="hideTip"
  >
    <button
      class="todo-check"
      :class="{ checked: todo.done, sub: isSub }"
      type="button"
      :disabled="busy"
      :title="todo.done ? '取消完成' : '标记完成'"
      :aria-label="todo.done ? '取消完成' : '标记完成'"
      @click="toggle"
    >
      <Check v-if="todo.done" :size="isSub ? 9 : 11" :stroke-width="3" />
    </button>

    <button
      v-if="!isSub"
      class="todo-priority"
      type="button"
      :style="{ background: PRIORITY_BG[todo.priority] }"
      :title="'优先级：' + PRIORITY_LABELS[todo.priority] + '，点击切换'"
      :aria-label="'优先级：' + PRIORITY_LABELS[todo.priority] + '，点击切换'"
      @click="cyclePriority"
    ></button>

    <div class="todo-mid">
      <textarea
        v-if="editing"
        :ref="setEditInput"
        v-model="editText"
        class="todo-edit"
        rows="1"
        :aria-label="'编辑待办：' + todo.title"
        @input="autoResizeEdit"
        @keydown.enter="onEditKeydown"
        @keydown.esc="editing = false"
        @blur="commitEdit"
      ></textarea>
      <div v-else class="todo-line">
        <!-- 图钉绝对定位在首行「第一个字」前面（只缩进首行），不参与换行后的行首缩进 -->
        <span class="todo-mainline" :class="{ 'has-pin': todo.pinned }">
          <button
            v-if="todo.pinned"
            class="todo-pin"
            type="button"
            aria-label="取消置顶"
            title="已置顶（与日期无关，固定在「置顶」区），点击取消"
            @click.stop="unpin"
          >
            <Pin :size="12" :stroke-width="2.2" />
          </button>
          <span
            class="todo-label"
            @dblclick="startEdit"
          >{{ todo.title }}</span>
        </span>
        <!-- 有补充说明的行给一个小标记：否则「写了描述」在列表里完全看不出来 -->
        <span
          v-if="todo.description.trim()"
          class="todo-desc-mark"
          title="有补充说明（鼠标移到这一行查看）"
          aria-hidden="true"
        >
          <AlignLeft :size="10" :stroke-width="2.2" />
        </span>

        <!-- 徽标与标题同一 flex 行：短标题尾随同行，长标题放不下自动换行兜底 -->
        <div v-if="!todo.done" class="todo-badges">
          <button
            v-if="badge"
            class="todo-badge"
            :class="[badge.kind, { static: !canSchedule }]"
            type="button"
            :title="canSchedule ? '点击设置截止/提醒' : undefined"
            @click="onBadgeClick"
          >
            <AlertTriangle v-if="badge.kind === 'over'" :size="10" :stroke-width="2" />
            <Sun v-else-if="badge.kind === 'today'" :size="10" :stroke-width="2" />
            <CalendarDays v-else :size="10" :stroke-width="2" />
            {{ badge.text }}
          </button>
          <button
            v-else-if="canSchedule"
            class="todo-badge-add"
            type="button"
            title="设置截止日期/提醒"
            @click="onBadgeClick"
          >
            <CalendarDays :size="10" :stroke-width="2" />
            <span>日期</span>
          </button>
          <span v-if="remindOn && todo.remind_at != null" class="todo-badge remind" title="到点弹提醒">
            <Bell :size="10" :stroke-width="2" />
            提醒 {{ fmtHM(todo.remind_at) }}
          </span>
          <span v-if="repeatText" class="todo-badge repeat" :title="repeatTitle">
            <Repeat :size="10" :stroke-width="2" />
            {{ repeatText }}
          </span>
          <span v-if="tagChips.length" class="todo-tags">
            <span v-for="tag in tagChips" :key="tag.id" class="todo-tag" :title="tag.name">
              <i class="dot" :style="{ background: tag.color || 'var(--brand-500)' }"></i>{{ tag.name }}
            </span>
          </span>
          <span
            v-if="showProgress"
            class="sub-progress"
            :title="`${doneKids}/${kids.length} 个子待办已完成`"
          >
            <span class="bar"><i :style="{ width: (doneKids / kids.length) * 100 + '%' }"></i></span>
            <span class="cnt">{{ doneKids }}/{{ kids.length }}</span>
          </span>
        </div>

        <!-- 父级操作按钮：跟在标题/徽标后面（不挂在整行最右），有子待办时不会沉到整块底部 -->
        <!-- 折叠/展开子待办：默认展开；折叠态图标常驻可见，否则找不到展开入口 -->
        <button
          v-if="!isSub && kids.length"
          class="todo-collapser"
          :class="{ collapsed }"
          type="button"
          :title="collapsed ? '展开子待办' : '折叠子待办'"
          :aria-label="collapsed ? '展开子待办' : '折叠子待办'"
          :aria-expanded="!collapsed"
          @click="toggleCollapse"
        >
          <ChevronRight v-if="collapsed" :size="12" :stroke-width="2.2" />
          <ChevronDown v-else :size="12" :stroke-width="2.2" />
        </button>
        <button
          v-if="!isSub"
          class="todo-subadd"
          type="button"
          :title="addingSub ? '收起' : '添加子待办'"
          :aria-label="addingSub ? '收起子待办输入' : '添加子待办'"
          @mousedown.prevent
          @click="toggleSubAdd"
        >
          <X v-if="addingSub" :size="12" :stroke-width="2.4" />
          <Plus v-else :size="12" :stroke-width="2.4" />
        </button>
        <button
          v-if="!isSub"
          class="todo-del"
          type="button"
          :title="kids.length ? '删除（级联删除子待办）' : '删除'"
          :aria-label="kids.length ? '删除（级联删除子待办）' : '删除'"
          @click="removeTodo(todo)"
        >
          <Trash2 :size="12" :stroke-width="2" />
        </button>

        <!-- 已完成行：完成时间徽标挂在行尾（margin-left:auto 靠右，hover 按钮在它左侧出现不挤动它） -->
        <span
          v-if="doneAt"
          class="todo-done-at"
          :title="`完成于 ${doneAt.full}`"
        >
          <Check :size="10" :stroke-width="2.4" />
          {{ doneAt.text }}
        </span>
      </div>

      <!-- 折叠时隐藏子待办列表；addingSub 打开时输入行必须可见（toggleSubAdd 已先展开，此处兜底）；
           子待办拖拽悬停到本行时临时展开（空 / 折叠父行也要给出放置区） -->
      <div
        v-if="(kids.length && !collapsed) || addingSub || isSubDropTarget"
        class="todo-subs"
      >
        <TodoRow
          v-for="k in kids"
          :key="k.id"
          :todo="k"
          is-sub
          :highlight-id="highlightId"
          :dragging="k.id === subDragId"
          @subdrag="onSubDragStart(k, $event)"
        />
        <div v-if="addingSub" class="todo-sub-input-row">
          <span class="sub-dot" aria-hidden="true"></span>
          <input
            :ref="setSubInput"
            v-model="subText"
            class="todo-sub-input"
            placeholder="输入子待办，回车确认"
            aria-label="添加子待办"
            @keydown.enter.prevent="onSubKeydown"
            @keydown.esc="addingSub = false"
            @blur="onSubBlur"
          />
          <button
            class="todo-del todo-sub-cancel"
            type="button"
            title="取消"
            aria-label="取消添加子待办"
            @mousedown.prevent
            @click="addingSub = false"
          >
            <X :size="12" :stroke-width="2.4" />
          </button>
        </div>
        <div v-if="isSubDropTarget && kids.length === 0 && !addingSub" class="todo-sub-drop-hint">
          松手放到这里作为子待办
        </div>
      </div>
    </div>

    <!-- 子待办的删除按钮仍挂行尾（父级按钮已移入标题行内） -->
    <button
      v-if="isSub"
      class="todo-del"
      type="button"
      title="删除"
      aria-label="删除子待办"
      @click="removeTodo(todo)"
    >
      <Trash2 :size="12" :stroke-width="2" />
    </button>
  </div>

  <Teleport to="body">
    <Transition name="tip">
      <div
        v-if="tip.visible"
        class="todo-tip"
        :style="{ left: tip.x + 'px', top: tip.y + 'px' }"
        role="tooltip"
      >
        <div v-if="tip.title" class="todo-tip-title">{{ tip.title }}</div>
        <div v-if="descHtml" class="todo-tip-md" v-html="descHtml"></div>
      </div>
    </Transition>
  </Teleport>

  <!-- 跨父拖拽插入线：只由拖拽源行绘制一条，fixed 定位（视口坐标） -->
  <Teleport to="body">
    <div
      v-if="ownsSubDrag && dropLineClientY != null"
      class="todo-sub-drop-line"
      :style="{ left: dropLineLeft + 'px', top: dropLineClientY + 'px', width: dropLineWidth + 'px' }"
      aria-hidden="true"
    ></div>
  </Teleport>

  <ConfirmDialog
    :visible="showKidsConfirm"
    title="还有子待办未完成"
    :message="`「${props.todo.title}」下还有 ${confirmKids} 条子待办未完成，是否确认完成？`"
    hint="确认后会一并勾选这些子待办；取消则本条待办保持未完成。"
    confirm-text="确认并勾选子项"
    @confirm="onKidsConfirm"
    @cancel="onKidsCancel"
  />
</template>

<style scoped>
.todo-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px;
  border-radius: var(--radius-sm);
  transition: background 0.3s;
}
.todo-row:hover {
  background: var(--bg-card-soft);
}
.todo-row.highlight {
  background: var(--brand-50);
  transition: background 0.5s;
}
/* 拖拽中的行半透明让位（落点由卡片层/宿主父行的插入线指示） */
.todo-row.dragging {
  opacity: 0.35;
}
/* 拖拽期间全局禁选 + 抓手光标（body 在组件外，用 :global 逃出 scoped）。
   与 TodoCard 重复声明是有意的：待办浮窗窗口不加载卡片样式，这里保证
   浮窗内子待办拖拽也有抓手光标与禁选 */
:global(body.todo-row-dragging) {
  cursor: grabbing;
  user-select: none;
  -webkit-user-select: none;
}
/* 子行嵌在父行 .todo-mid（父待办首字列）内，不再额外缩进，
   让子复选框与父待办第一个字对齐 */
.todo-row.sub {
  padding-left: 0;
}

.todo-check {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border: 1.5px solid var(--border-strong);
  border-radius: var(--radius-pill);
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-on-accent);
  padding: 0;
  cursor: pointer;
  transition: background 0.18s, border-color 0.18s, transform 0.18s;
}
.todo-check:hover {
  border-color: var(--brand-500);
  background: var(--brand-50);
}
.todo-check:active {
  transform: scale(0.9);
}
.todo-check.checked {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.todo-check.sub {
  width: 15px;
  height: 15px;
  border-width: 1.2px;
}

.todo-priority {
  flex-shrink: 0;
  width: 10px;
  height: 10px;
  margin-top: 5px;
  border: none;
  border-radius: 50%;
  padding: 0;
  cursor: pointer;
  transition: transform 0.18s, filter 0.18s;
}
.todo-priority:hover {
  transform: scale(1.35);
  filter: brightness(0.97);
}
.todo-priority:active {
  transform: scale(0.92);
}

.todo-mid {
  flex: 1;
  min-width: 0;
}
/* 标题 + 徽标同一 flex 行：徽标尾随标题末尾，放不下时整组换行到下一行 */
.todo-line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  column-gap: 6px;
  row-gap: 2px;
  min-width: 0;
}
.todo-label {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 5;
  overflow: hidden;
  flex: 0 1 auto;
  min-width: 0;
  font-size: 0.8125em;
  line-height: 1.45;
  color: var(--text-1);
  white-space: pre-wrap;
  word-break: break-word;
  cursor: text;
}
.todo-row.sub .todo-label {
  font-size: 0.75em;
  color: var(--text-2);
}
.todo-row.done .todo-label {
  text-decoration: line-through;
  opacity: 0.6;
  color: var(--text-3);
}
/* 有补充说明的行标记：只做提示，不可点（点击/双击语义留给标题本身） */
.todo-desc-mark {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  color: var(--text-4);
  cursor: default;
}

/* 已完成行的完成时间徽标：与日期徽标同语言的弱化灰 pill，靠右对齐；
   margin-left:auto 把它推到行尾，hover 操作按钮在它左侧出现，不会挤动它 */
.todo-done-at {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  flex-shrink: 0;
  margin-left: auto;
  padding: 1px 8px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-3);
  font-size: 0.625em;
  font-weight: 600;
  line-height: 14px;
  cursor: default;
}

.todo-edit {
  width: 100%;
  border: 1px solid var(--brand-500);
  border-radius: 6px;
  background: var(--bg-card-solid);
  color: var(--text-1);
  font-size: 0.8125em;
  line-height: 1.45;
  font-family: inherit;
  padding: 2px 6px;
  outline: none;
  box-shadow: var(--shadow-focus);
  resize: none;
  overflow-y: auto;
  min-height: 22px;
  max-height: 40vh;
}

/* ---- 徽标行 ---- */
.todo-badges {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.todo-badge {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  border: none;
  border-radius: var(--radius-pill);
  padding: 1px 8px;
  font-size: 0.625em;
  font-weight: 600;
  line-height: 14px;
  cursor: pointer;
  transition: filter 0.18s, transform 0.18s;
  font-family: inherit;
}
.todo-badge:hover {
  filter: brightness(0.96);
  transform: translateY(-1px);
}
.todo-badge:active {
  transform: scale(0.96);
}
.todo-badge.over {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.todo-badge.today {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.todo-badge.tmr {
  background: var(--brand-50);
  color: var(--brand-500);
}
.todo-badge.date {
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.todo-badge.remind {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
  cursor: default;
}
/* 周期徽标：蓝色（与倒计时/重复语义一致），只读展示 */
.todo-badge.repeat {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
  cursor: default;
}
.todo-badge.repeat:hover {
  transform: none;
  filter: none;
}
/* 图钉 + 标题：图钉绝对定位在首行第一个字前面，只有首行缩进（text-indent 只作用于首行） */
.todo-mainline {
  position: relative;
  display: block;
  flex: 0 1 auto;
  min-width: 0;
}
.todo-mainline.has-pin .todo-label {
  text-indent: 17px;
}
/* 置顶图钉：行首标识，颜色与「置顶」分区标题一致 */
.todo-pin {
  position: absolute;
  left: 0;
  top: 3px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--brand-500);
  cursor: pointer;
}
.todo-pin:hover {
  color: var(--c-red-ink);
}
/* 待办标签：小圆点 + 名字 */
.todo-tags {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.todo-tag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 0 6px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  color: var(--text-3);
  font-size: 0.625em;
  font-weight: 600;
}
.todo-tag .dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.todo-badge.remind:hover {
  transform: none;
  filter: none;
}
/* 无排期弹层的宿主（浮窗）：日期徽标只读展示 */
.todo-badge.static {
  cursor: default;
}
.todo-badge.static:hover {
  transform: none;
  filter: none;
}
.todo-badge-add {
  display: none;
  align-items: center;
  gap: 3px;
  border: none;
  background: transparent;
  border-radius: var(--radius-pill);
  padding: 1px 6px;
  font-size: 0.625em;
  font-weight: 500;
  color: var(--text-4);
  cursor: pointer;
  line-height: 14px;
  transition: background 0.18s, color 0.18s;
  font-family: inherit;
}
/* hover 才占位渲染：避免隐形徽标在标题较长时挤出一行幻影空行 */
.todo-row:hover .todo-badge-add,
.todo-row:focus-within .todo-badge-add {
  display: inline-flex;
}
.todo-badge-add:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}

/* ---- 子待办进度 ---- */
.sub-progress {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.sub-progress .bar {
  width: 46px;
  height: 4px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.sub-progress .bar i {
  display: block;
  height: 100%;
  background: var(--brand-500);
  border-radius: var(--radius-pill);
  transition: width 0.3s;
}
.sub-progress .cnt {
  font-size: 0.625em;
  font-weight: 600;
  color: var(--text-3);
}

/* ---- 子待办区 ---- */
.todo-subs {
  margin-top: 2px;
}
/* 空 / 折叠父行在拖拽悬停时给出的放置区提示 */
.todo-sub-drop-hint {
  margin: 2px 0 2px 0;
  padding: 4px 8px;
  border: 1px dashed var(--brand-500);
  border-radius: var(--radius-sm);
  color: var(--brand-500);
  font-size: 0.6875em;
  line-height: 1.4;
  opacity: 0.9;
  pointer-events: none;
}
/* 跨父拖拽落点父行高亮 */
.todo-row.sub-drop-target {
  background: var(--brand-50);
  box-shadow: inset 0 0 0 1px var(--brand-500);
}
/* 跨父拖拽插入线：fixed 定位于视口，由拖拽源行 Teleport 到 body 绘制 */
.todo-sub-drop-line {
  position: fixed;
  height: 2px;
  border-radius: 1px;
  background: var(--brand-500);
  box-shadow: 0 0 6px var(--brand-glow);
  pointer-events: none;
  z-index: 30;
}
.todo-sub-drop-line::before {
  content: '';
  position: absolute;
  left: -1px;
  top: -2px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--brand-500);
}
.todo-sub-input-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 6px 2px 0;
}
.sub-dot {
  width: 15px;
  height: 15px;
  border: 1.2px solid var(--border-strong);
  border-radius: 50%;
  flex-shrink: 0;
  display: inline-block;
}
.todo-sub-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--border-soft);
  border-radius: 6px;
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.75em;
  line-height: 1.4;
  font-family: inherit;
  padding: 3px 8px;
  outline: none;
  transition: border-color 0.18s, box-shadow 0.18s;
}
.todo-sub-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.todo-sub-input::placeholder {
  color: var(--text-4);
}
/* 双类提升特异性：压过后声明的 .todo-del 默认隐藏（display:none） */
.todo-del.todo-sub-cancel {
  position: static;
  display: flex; /* 输入行内常驻，不随 hover 显隐 */
}

/* ---- 行内操作按钮：父级跟在标题/徽标后，子级删除仍挂行尾 ---- */
/* 默认不渲染（display:none 才不占布局）：隐形占位在标题换行到按钮位置时会
   挤出整行空白，与日期徽标同规则——hover / 键盘聚焦行时才渲染 */
.todo-collapser,
.todo-subadd,
.todo-del {
  flex-shrink: 0;
  align-self: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  display: none;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: background 0.18s, color 0.18s;
}
.todo-collapser {
  color: var(--text-3);
}
.todo-subadd {
  color: var(--text-4);
}
.todo-del {
  color: var(--text-3);
}
.todo-row:hover .todo-collapser,
.todo-row:hover .todo-subadd,
.todo-row:hover .todo-del,
.todo-row:focus-within .todo-collapser,
.todo-row:focus-within .todo-subadd,
.todo-row:focus-within .todo-del {
  display: flex;
}
/* 折叠态常驻显示（不随 hover 隐藏）：重新展开的入口必须随时可见 */
.todo-collapser.collapsed {
  display: flex;
}
/* 拖拽期间锁定按钮渲染：hover 引发的行高变化会干扰落点指示线。
   注意：选择器必须整体包进一个 :global() ——「:global(前缀) 后代」写法会被
   Tailwind4/lightningcss 管线吃掉后代部分，编译成 body 本体 display:none（整页消失） */
:global(body.todo-row-dragging .todo-badge-add),
:global(body.todo-row-dragging .todo-collapser),
:global(body.todo-row-dragging .todo-subadd),
:global(body.todo-row-dragging .todo-del) {
  display: none;
}
.todo-collapser:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.todo-subadd:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.todo-del:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}

.todo-tip {
  position: fixed;
  z-index: 900;
  max-width: 340px;
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-dock);
  font-size: 0.75rem;
  line-height: 1.5;
  color: var(--text-1);
  white-space: pre-wrap;
  word-break: break-word;
  pointer-events: none;
}
/* 有描述时放宽一点：Markdown 列表/引用在 340px 里换行太碎 */
.todo-tip:has(.todo-tip-md) {
  max-width: 420px;
}
.todo-tip-title {
  font-weight: 600;
  margin-bottom: 4px;
}
/* 描述正文（v-html 注入，标记不在本组件作用域内 → 必须 :deep） */
.todo-tip-md {
  white-space: normal;
}
.todo-tip-md :deep(p) {
  margin: 0 0 6px;
}
.todo-tip-md :deep(p:last-child) {
  margin-bottom: 0;
}
.todo-tip-md :deep(ul),
.todo-tip-md :deep(ol) {
  margin: 0 0 6px;
  padding-left: 18px;
}
.todo-tip-md :deep(li) {
  margin: 2px 0;
}
.todo-tip-md :deep(blockquote) {
  margin: 0 0 6px;
  padding: 2px 0 2px 8px;
  border-left: 2px solid var(--border-strong);
  color: var(--text-2);
}
.todo-tip-md :deep(code) {
  padding: 0 4px;
  border-radius: 4px;
  background: var(--bg-card-soft);
  font-size: 0.92em;
}
.todo-tip-md :deep(pre) {
  margin: 0 0 6px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  overflow-x: auto;
}
.todo-tip-md :deep(pre code) {
  padding: 0;
  background: transparent;
}
.todo-tip-md :deep(a) {
  color: var(--brand-500);
}
.todo-tip-md :deep(h1),
.todo-tip-md :deep(h2),
.todo-tip-md :deep(h3) {
  margin: 0 0 4px;
  font-size: 0.8125rem;
}
.todo-tip-md :deep(hr) {
  margin: 6px 0;
  border: none;
  border-top: 1px solid var(--border-soft);
}
.todo-tip-md :deep(img) {
  max-width: 100%;
}
.tip-enter-active,
.tip-leave-active {
  transition: opacity 0.15s ease-out, transform 0.15s ease-out;
}
.tip-enter-from,
.tip-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
