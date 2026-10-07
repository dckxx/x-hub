<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, provide, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { ListTodo, Pin, PinOff, X } from 'lucide-vue-next'
import { isTauri, type Todo } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useTheme } from '../composables/useTheme'
import { parseTodoItems } from '../utils/todoParse'
import { comparePinnedFirst } from '../utils/todoSchedule'
import { useTodoChildren } from '../composables/useTodoChildren'
import TodoRow from './TodoRow.vue'

const store = useStore()

// ---- 子待办支持（与工作台待办卡共用 TodoRow）----
// 不提供 todoOpenSchedule：浮窗无排期弹层，TodoRow 内日期徽标转只读、隐藏「日期」按钮；
// 不提供 todoDragStart：顶级行按下仍走窗口拖动，子待办组内拖拽由 TodoRow 自带。
const childrenMap = useTodoChildren()
provide('todoChildren', childrenMap)
provide('todoRemoveTodo', (t: Todo) => {
  // 浮窗无 toast 撤销体系，直接删（后端级联删子待办）
  void store.deleteTodo(t.id)
})

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

// 与待办卡片同一排序规则的平铺版：置顶条目浮到最前（浮窗不分组），
// 其余手动拖过的（sort_order）优先，再按创建时间倒序
const pendingTodos = computed(() =>
  store.state.todos.filter((t) => !t.done && t.parent_id == null).sort(comparePinnedFirst),
)

async function onAdd() {
  const v = input.value.trim()
  if (!v) return
  // 支持「1. a 2. b 3. c」这类序号列表一次拆成多条；非序号文本原样一条
  await Promise.all(parseTodoItems(v).map((title) => store.createTodo(title)))
  input.value = ''
}

// 回车提交（Shift/组合键不拦截）；IME 组合期间不提交，避免误触
function onAddKeydown(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return
  e.preventDefault()
  void onAdd()
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
  // 子待办行是行内交互区（勾选/编辑/组内拖拽），让给 TodoRow；编辑框同理不触发窗口拖动
  if (target.closest('button, input, textarea, .todo-row.sub')) return
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

// ---- 边缘拖拽改变窗口大小 ----
// 无边框窗口没有系统缩放边，用 8 个隐形边缘区手动触发系统级 resize；
// stopPropagation 拦掉冒泡，避免同时进入「拖动窗口」流程
// 与 @tauri-apps/api window 的 ResizeDirection 同构（该类型未导出，此处本地声明）
type ResizeDirection = 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West'
const RESIZE_DIRECTIONS: ResizeDirection[] = [
  'North',
  'South',
  'East',
  'West',
  'NorthEast',
  'NorthWest',
  'SouthEast',
  'SouthWest',
]

function onResizeStart(e: MouseEvent, dir: ResizeDirection) {
  if (!appWindow || e.button !== 0) return
  e.preventDefault()
  e.stopPropagation()
  void appWindow.startResizeDragging(dir)
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
        v-model="input"
        class="tf-input"
        rows="1"
        placeholder="添加待办，回车确认"
        aria-label="添加待办"
        @keydown.enter="onAddKeydown"
      ></textarea>
    </div>

    <div class="tf-body">
      <div v-if="pendingTodos.length === 0" class="tf-empty">
        <p>暂无待办</p>
      </div>

      <!-- 与工作台待办卡同一行组件：子待办缩进/折叠/级联勾选/组内拖拽全部一致 -->
      <div v-else>
        <TodoRow v-for="t in pendingTodos" :key="t.id" :todo="t" />
      </div>
    </div>

    <!-- 8 方向隐形缩放边缘（仅 Tauri 窗口内渲染） -->
    <template v-if="appWindow">
      <div
        v-for="dir in RESIZE_DIRECTIONS"
        :key="dir"
        class="rz"
        :class="'rz-' + dir.toLowerCase()"
        @mousedown="onResizeStart($event, dir)"
      ></div>
    </template>
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
  position: relative;
}
/* 缩放边缘区：边 6px、角 14px，叠加在内容之上但平时不可见 */
.rz {
  position: absolute;
  z-index: 60;
}
.rz-north {
  top: 0;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.rz-south {
  bottom: 0;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.rz-east {
  top: 0;
  bottom: 0;
  right: 0;
  width: 6px;
  cursor: ew-resize;
}
.rz-west {
  top: 0;
  bottom: 0;
  left: 0;
  width: 6px;
  cursor: ew-resize;
}
.rz-northeast {
  top: 0;
  right: 0;
  width: 14px;
  height: 14px;
  cursor: nesw-resize;
}
.rz-southwest {
  bottom: 0;
  left: 0;
  width: 14px;
  height: 14px;
  cursor: nesw-resize;
}
.rz-northwest {
  top: 0;
  left: 0;
  width: 14px;
  height: 14px;
  cursor: nwse-resize;
}
.rz-southeast {
  bottom: 0;
  right: 0;
  width: 14px;
  height: 14px;
  cursor: nwse-resize;
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
  margin: 0 -4px;
  padding: 0 4px;
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
