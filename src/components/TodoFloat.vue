<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, type ComponentPublicInstance } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { Check, ListTodo, Pin, PinOff, Trash2, X } from 'lucide-vue-next'
import { isTauri } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useTheme } from '../composables/useTheme'
import { parseTodoItems } from '../utils/todoParse'

const store = useStore()

// 从窗口 label 取浮窗标识（todo-float），用于置顶切换
const floatLabel = isTauri() ? getCurrentWindow().label : 'todo-float'
const pinned = ref(true)

async function togglePin() {
  const next = !pinned.value
  pinned.value = next
  try {
    await store.toggleFloatPin(floatLabel, next)
  } catch {
    pinned.value = !next
  }
}

// 标记为待办浮窗：body 透明，只显示卡片本体
document.documentElement.dataset.todoFloat = ''
useTheme()

let unlisten: (() => void) | null = null

onMounted(async () => {
  await store.loadInitialData()
  if (isTauri()) {
    unlisten = await listen('todos-changed', () => {
      void store.refreshTodos()
    })
  }
})

onBeforeUnmount(() => {
  unlisten?.()
})

const input = ref('')

// 新增输入框：单行 textarea，随内容自动增高（封顶见 CSS max-height），粘贴多行序号列表时临时撑高
const addInputRef = ref<HTMLTextAreaElement | null>(null)
function setAddInput(el: Element | ComponentPublicInstance | null) {
  addInputRef.value = el instanceof HTMLTextAreaElement ? el : null
}
function autoResizeAdd() {
  const el = addInputRef.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight}px`
}

const pendingTodos = computed(() => store.state.todos.filter((t) => !t.done))

async function onAdd() {
  const v = input.value.trim()
  if (!v) return
  // 支持「1. a 2. b 3. c」这类序号列表一次拆成多条；非序号文本原样一条
  await Promise.all(parseTodoItems(v).map((title) => store.createTodo(title)))
  input.value = ''
  if (addInputRef.value) addInputRef.value.style.height = 'auto'
}

// 回车提交；IME 组合（中文输入法选词）期间不提交，避免误触
function onAddKeydown(e: KeyboardEvent) {
  if (e.isComposing) return
  e.preventDefault()
  void onAdd()
}

async function toggle(id: number) {
  await store.toggleTodo(id)
}

async function remove(id: number) {
  await store.deleteTodo(id)
}

async function onClose() {
  if (isTauri()) await getCurrentWindow().close()
}

// 窗口拖动
const appWindow = isTauri() ? getCurrentWindow() : null
const DRAG_THRESHOLD = 4
let dragPending: { x: number; y: number } | null = null

function onMouseDown(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button, input')) return
  dragPending = { x: e.screenX, y: e.screenY }
}
function onMouseMove(e: MouseEvent) {
  if (!dragPending || !appWindow) return
  const dx = e.screenX - dragPending.x
  const dy = e.screenY - dragPending.y
  if (dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD) {
    dragPending = null
    void appWindow.startDragging()
  }
}
function onDragEnd() {
  dragPending = null
}
</script>

<template>
  <div
    class="tf-root"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onDragEnd"
    @mouseleave="onDragEnd"
  >
    <header class="tf-header">
      <h3 class="tf-title">
        <ListTodo :size="14" :stroke-width="2" aria-hidden="true" />
        <span>待办</span>
      </h3>
      <div class="tf-controls">
        <button
          class="tf-btn"
          :class="{ active: pinned }"
          :title="pinned ? '取消置顶' : '置顶'"
          type="button"
          @click="togglePin"
        >
          <Pin v-if="pinned" :size="13" :stroke-width="2" />
          <PinOff v-else :size="13" :stroke-width="2" />
        </button>
        <button class="tf-btn tf-close" title="关闭" aria-label="关闭" @click="onClose">
          <X :size="13" :stroke-width="2" />
        </button>
      </div>
    </header>

    <div class="tf-add">
      <textarea
        :ref="setAddInput"
        v-model="input"
        class="tf-input"
        rows="2"
        placeholder="添加待办，回车确认（粘贴 1. 2. 3. 序号列表可一次拆多条）"
        aria-label="添加待办"
        @input="autoResizeAdd"
        @keydown.enter.exact="onAddKeydown"
      ></textarea>
    </div>

    <div class="tf-body">
      <div v-if="pendingTodos.length === 0" class="tf-empty">
        <p>暂无待办</p>
      </div>

      <div v-else>
        <div v-for="t in pendingTodos" :key="t.id" class="tf-row">
          <button
            class="tf-check"
            :class="{ checked: t.done }"
            :title="'标记完成'"
            aria-label="标记完成"
            @click="toggle(t.id)"
          >
            <Check v-if="t.done" :size="11" :stroke-width="3" />
          </button>
          <span class="tf-label">{{ t.title }}</span>
          <button class="tf-del" title="删除" aria-label="删除" @click="remove(t.id)">
            <Trash2 :size="12" :stroke-width="2" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tf-root {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px;
  box-sizing: border-box;
  -webkit-app-region: no-drag;
  font-size: calc(1rem * var(--fs-todo, 1));
}
.tf-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
  cursor: move;
}
.tf-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.tf-title :deep(svg) {
  color: var(--brand-500);
}
.tf-controls {
  display: flex;
  gap: 2px;
}
.tf-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: 6px;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.tf-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.tf-btn.active {
  color: var(--brand-500);
}
.tf-btn.tf-close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
.tf-add {
  flex-shrink: 0;
  margin-bottom: 8px;
}
.tf-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125em;
  padding: 7px 10px;
  outline: none;
  box-sizing: border-box;
  transition: border-color 0.18s, box-shadow 0.18s;
  display: block;
  resize: none;
  line-height: 1.45;
  max-height: 110px;
  max-height: calc(5lh + 16px);
  overflow-y: auto;
}
.tf-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.tf-input::placeholder {
  color: var(--text-4);
}
.tf-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0 -4px;
  padding: 0 4px;
}
.tf-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px;
  border-radius: var(--radius-sm);
}
.tf-row:hover {
  background: var(--bg-card-soft);
}
.tf-row.done {
  opacity: 0.6;
}
.tf-check {
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
}
.tf-check.checked {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.tf-label {
  flex: 1;
  min-width: 0;
  font-size: 0.8125em;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tf-row.done .tf-label {
  text-decoration: line-through;
  color: var(--text-3);
}
.tf-del {
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
.tf-row:hover .tf-del,
.tf-row:focus-within .tf-del {
  opacity: 1;
}
.tf-del:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.tf-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-4);
}
.tf-empty p {
  margin: 0;
  font-size: 0.8125em;
}
</style>
