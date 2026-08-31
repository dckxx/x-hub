<script setup lang="ts">
import { computed, inject, nextTick, ref, type ComputedRef, type ComponentPublicInstance } from 'vue'
import {
  AlertTriangle,
  Bell,
  CalendarDays,
  Check,
  Plus,
  Sun,
  Trash2,
  X,
} from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'
import { dueBadge, fmtHM } from '../utils/todoSchedule'

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
  }>(),
  { isSub: false, highlightId: null },
)

const store = useStore()
const openSchedule = inject<(t: Todo, el: HTMLElement) => void>('todoOpenSchedule', () => {})
const removeTodo = inject<(t: Todo) => void>('todoRemoveTodo', () => {})
/** 卡片层统一构建的 父id → 子待办 列表（创建时间倒序），避免每行各自过滤 */
const childrenMap = inject<ComputedRef<Map<number, Todo[]>>>(
  'todoChildren',
  computed(() => new Map()),
)

const PRIORITY_LABELS = ['普通', '重要', '紧急'] as const
const PRIORITY_BG = ['var(--todo-pri-default)', 'var(--c-yellow-soft)', 'var(--c-red-soft)'] as const

// 子待办（仅一层）
const kids = computed(() => childrenMap.value.get(props.todo.id) ?? [])
const doneKids = computed(() => kids.value.filter((k) => k.done).length)

const badge = computed(() => (props.todo.done ? null : dueBadge(props.todo, new Date())))
const remindOn = computed(() => !props.todo.done && props.todo.remind_at != null)
const showProgress = computed(() => !props.isSub && !props.todo.done && kids.value.length > 0)

function onBadgeClick(e: MouseEvent) {
  const el = e.currentTarget
  if (el instanceof HTMLElement) openSchedule(props.todo, el)
}

async function toggle() {
  await store.toggleTodo(props.todo.id)
}

async function cyclePriority() {
  await store.updateTodo(props.todo.id, props.todo.title, (props.todo.priority + 1) % 3)
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

// ---- 长内容悬浮全文（超过 5 行截断时） ----
const tip = ref<{ visible: boolean; title: string; x: number; y: number }>({
  visible: false,
  title: '',
  x: 0,
  y: 0,
})

function showTip(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement
  if (!el || el.scrollHeight <= el.clientHeight + 2) return
  const rect = el.getBoundingClientRect()
  tip.value = { visible: true, title: props.todo.title, x: rect.left, y: rect.bottom + 6 }
}

function hideTip() {
  tip.value.visible = false
}
</script>

<template>
  <div
    class="todo-row"
    :class="{ done: todo.done, sub: isSub, highlight: todo.id === highlightId }"
    :data-todo-id="todo.id"
  >
    <button
      class="todo-check"
      :class="{ checked: todo.done, sub: isSub }"
      type="button"
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
      <span
        v-else
        class="todo-label"
        :data-tip="todo.title"
        @dblclick="startEdit"
        @mouseenter="showTip"
        @mouseleave="hideTip"
      >{{ todo.title }}</span>

      <div v-if="!todo.done" class="todo-badges">
        <button
          v-if="badge"
          class="todo-badge"
          :class="badge.kind"
          type="button"
          title="点击设置截止/提醒"
          @click="onBadgeClick"
        >
          <AlertTriangle v-if="badge.kind === 'over'" :size="10" :stroke-width="2" />
          <Sun v-else-if="badge.kind === 'today'" :size="10" :stroke-width="2" />
          <CalendarDays v-else :size="10" :stroke-width="2" />
          {{ badge.text }}
        </button>
        <button
          v-else
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
        <span
          v-if="showProgress"
          class="sub-progress"
          :title="`${doneKids}/${kids.length} 个子待办已完成`"
        >
          <span class="bar"><i :style="{ width: (doneKids / kids.length) * 100 + '%' }"></i></span>
          <span class="cnt">{{ doneKids }}/{{ kids.length }}</span>
        </span>
      </div>

      <div v-if="kids.length || addingSub" class="todo-subs">
        <TodoRow
          v-for="k in kids"
          :key="k.id"
          :todo="k"
          is-sub
          :highlight-id="highlightId"
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
      </div>
    </div>

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
      class="todo-del"
      type="button"
      :title="kids.length ? '删除（级联删除子待办）' : '删除'"
      :aria-label="kids.length ? '删除（级联删除子待办）' : '删除'"
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
        {{ tip.title }}
      </div>
    </Transition>
  </Teleport>
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
.todo-label {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 5;
  overflow: hidden;
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
  margin-top: 3px;
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
  line-height: 16px;
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
.todo-badge.remind:hover {
  transform: none;
  filter: none;
}
.todo-badge-add {
  display: inline-flex;
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
  line-height: 16px;
  opacity: 0;
  transition: opacity 0.18s, background 0.18s, color 0.18s;
  font-family: inherit;
}
.todo-row:hover .todo-badge-add,
.todo-row:focus-within .todo-badge-add {
  opacity: 1;
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
.todo-sub-cancel {
  position: static;
  opacity: 1;
}

/* ---- 行尾按钮 ---- */
.todo-subadd,
.todo-del {
  flex-shrink: 0;
  align-self: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.18s, background 0.18s, color 0.18s;
}
.todo-subadd {
  color: var(--text-4);
}
.todo-del {
  color: var(--text-3);
}
.todo-row:hover .todo-subadd,
.todo-row:hover .todo-del,
.todo-row:focus-within .todo-subadd,
.todo-row:focus-within .todo-del {
  opacity: 1;
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
