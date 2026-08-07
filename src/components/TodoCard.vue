<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { Check, Trash2 } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'

const props = defineProps<{ highlightId?: number | null }>()

const store = useStore()

const view = ref<'pending' | 'done'>('pending')
const input = ref('')

// 编辑状态
const editingId = ref<number | null>(null)
const editText = ref('')
const editInputRef = ref<HTMLInputElement | null>(null)
function setEditInput(el: HTMLInputElement | null) {
  editInputRef.value = el
}

// 全局搜索跳转高亮
const highlight = ref<number | null>(null)
let highlightTimer: ReturnType<typeof setTimeout> | null = null

const PRIORITY_COLORS = ['var(--c-gray)', 'var(--c-yellow)', 'var(--c-red)'] as const
const PRIORITY_LABELS = ['普通', '重要', '紧急'] as const

const pendingTodos = computed(() =>
  store.state.todos
    .filter((t) => !t.done)
    .sort((a, b) => b.created_at.localeCompare(a.created_at)),
)
const doneTodos = computed(() =>
  store.state.todos
    .filter((t) => t.done)
    .sort((a, b) => (b.completed_at ?? '').localeCompare(a.completed_at ?? '')),
)
const list = computed(() => (view.value === 'pending' ? pendingTodos.value : doneTodos.value))

async function onAdd() {
  const v = input.value.trim()
  if (!v) return
  await store.createTodo(v)
  input.value = ''
}

async function toggle(t: Todo) {
  await store.toggleTodo(t.id)
}

async function cyclePriority(t: Todo) {
  await store.updateTodo(t.id, t.title, (t.priority + 1) % 3)
}

async function remove(t: Todo) {
  await store.deleteTodo(t.id)
}

function startEdit(t: Todo) {
  editingId.value = t.id
  editText.value = t.title
  nextTick(() => editInputRef.value?.focus())
}

function cancelEdit() {
  editingId.value = null
}

async function commitEdit(id: number) {
  if (editingId.value !== id) return
  editingId.value = null
  const title = editText.value.trim()
  if (!title) return
  const t = store.state.todos.find((x) => x.id === id)
  if (!t || t.title === title) return
  await store.updateTodo(id, title, t.priority)
}

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
      <h3 class="todo-title">待办</h3>
      <div class="filter-tabs todo-seg" role="tablist" aria-label="视图切换">
        <button
          class="filter-tab filter-tab--primary"
          :class="{ active: view === 'pending' }"
          role="tab"
          :aria-selected="view === 'pending'"
          @click="view = 'pending'"
        >
          待办 {{ pendingTodos.length }}
        </button>
        <button
          class="filter-tab filter-tab--primary"
          :class="{ active: view === 'done' }"
          role="tab"
          :aria-selected="view === 'done'"
          @click="view = 'done'"
        >
          已完成 {{ doneTodos.length }}
        </button>
      </div>
    </header>

    <div v-if="view === 'pending'" class="todo-add">
      <input
        v-model="input"
        class="todo-input"
        placeholder="添加待办，回车确认"
        aria-label="添加待办"
        @keydown.enter.prevent="onAdd"
      />
    </div>

    <div class="todo-body">
      <div v-if="list.length === 0" class="empty-state todo-empty">
        <p>{{ view === 'pending' ? '今天要做什么？' : '暂无已完成' }}</p>
        <p v-if="view === 'pending'">按回车快速添加</p>
      </div>

      <template v-else>
        <div
        v-for="t in list"
        :key="t.id"
        class="todo-row"
        :class="{ done: t.done, highlight: t.id === highlight }"
        :data-todo-id="t.id"
      >
        <button
          class="todo-check"
          :class="{ checked: t.done }"
          :title="t.done ? '取消完成' : '标记完成'"
          aria-label="切换完成状态"
          @click="toggle(t)"
        >
          <Check v-if="t.done" :size="11" :stroke-width="3" />
        </button>

        <span
          class="todo-priority"
          :style="{ background: PRIORITY_COLORS[t.priority] }"
          :title="'优先级：' + PRIORITY_LABELS[t.priority]"
          role="button"
          tabindex="0"
          aria-label="循环切换优先级"
          @click="cyclePriority(t)"
          @keydown.enter.prevent="cyclePriority(t)"
        />

        <template v-if="editingId === t.id">
          <input
            :ref="setEditInput"
            v-model="editText"
            class="todo-edit"
            :aria-label="'编辑待办：' + t.title"
            @keydown.enter.prevent="commitEdit(t.id)"
            @keydown.esc="cancelEdit()"
            @blur="commitEdit(t.id)"
          />
        </template>
        <span v-else class="todo-label" :class="{ done: t.done }" @dblclick="startEdit(t)">
          {{ t.title }}
        </span>

        <button
          class="todo-del"
          title="删除"
          aria-label="删除"
          @click="remove(t)"
        >
          <Trash2 :size="12" :stroke-width="2" />
        </button>
        </div>
      </template>
    </div>
  </section>
</template>

<style scoped>
.todo-card {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px;
  min-height: 0;
}
.todo-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}
.todo-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  white-space: nowrap;
  margin: 0;
}
.todo-seg {
  gap: 4px;
  flex-shrink: 0;
}
.todo-seg .filter-tab {
  padding: 4px 10px;
  font-size: 11px;
}

.todo-add {
  margin-bottom: 8px;
}
.todo-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 13px;
  padding: 7px 10px;
  outline: none;
  transition: border-color 0.18s, box-shadow 0.18s, background 0.18s;
}
.todo-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: var(--bg-card-solid);
}
.todo-input::placeholder {
  color: var(--text-4);
}

.todo-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  margin: 0 -4px;
  padding: 0 4px;
}

.todo-row {
  display: flex;
  align-items: center;
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

.todo-check {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
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

.todo-priority {
  flex-shrink: 0;
  width: 8px;
  height: 8px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition: transform 0.18s;
}
.todo-priority:hover {
  transform: scale(1.35);
}

.todo-label {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.todo-label.done {
  text-decoration: line-through;
  opacity: 0.6;
  color: var(--text-3);
}

.todo-edit {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--brand-500);
  border-radius: 6px;
  background: var(--bg-card-solid);
  color: var(--text-1);
  font-size: 13px;
  padding: 2px 6px;
  outline: none;
  box-shadow: var(--shadow-focus);
}

.todo-del {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.18s, background 0.18s, color 0.18s;
}
.todo-row:hover .todo-del,
.todo-row:focus-within .todo-del {
  opacity: 1;
}
.todo-del:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}

.todo-empty {
  padding: 28px 8px;
}
.todo-empty p {
  margin: 0;
  font-size: 12px;
  color: var(--text-4);
}
.todo-empty p:first-child {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-3);
}
</style>
