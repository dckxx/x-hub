<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { Search } from 'lucide-vue-next'
import type { Note, Resource, Todo } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'openResource', r: Resource): void
  (e: 'openNote', n: Note): void
  (e: 'openTodo', t: Todo): void
}>()

const store = useStore()

const keyword = ref('')
const results = ref<{ resources: Resource[]; notes: Note[]; todos: Todo[] }>({
  resources: [],
  notes: [],
  todos: [],
})
const searched = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)
const cardRef = ref<HTMLElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, inputRef)

interface FlatItem {
  type: 'resource' | 'note' | 'todo'
  key: string
  resource?: Resource
  note?: Note
  todo?: Todo
}

const flatResults = computed<FlatItem[]>(() => [
  ...results.value.resources.map((r) => ({ type: 'resource' as const, key: 'r' + r.id, resource: r })),
  ...results.value.notes.map((n) => ({ type: 'note' as const, key: 'n' + n.id, note: n })),
  ...results.value.todos.map((t) => ({ type: 'todo' as const, key: 't' + t.id, todo: t })),
])

const activeIndex = ref(-1)

watch(flatResults, (list) => {
  activeIndex.value = list.length > 0 ? 0 : -1
})

function scrollActiveIntoView() {
  const item = flatResults.value[activeIndex.value]
  if (!item) return
  document
    .querySelector<HTMLElement>(`[data-search-key="${item.key}"]`)
    ?.scrollIntoView({ block: 'nearest' })
}

function openActive() {
  const item = flatResults.value[activeIndex.value]
  if (!item) return
  if (item.type === 'resource' && item.resource) emit('openResource', item.resource)
  else if (item.type === 'note' && item.note) emit('openNote', item.note)
  else if (item.type === 'todo' && item.todo) emit('openTodo', item.todo)
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  const list = flatResults.value
  if (list.length === 0) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value + 1) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value - 1 + list.length) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    openActive()
  }
}

let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.visible,
  (v) => {
    if (v) {
      keyword.value = ''
      results.value = { resources: [], notes: [], todos: [] }
      searched.value = false
      activeIndex.value = -1
    }
  },
)

watch(keyword, (kw) => {
  if (searchTimer) clearTimeout(searchTimer)
  const trimmed = kw.trim()
  if (!trimmed) {
    results.value = { resources: [], notes: [], todos: [] }
    searched.value = false
    return
  }
  searchTimer = setTimeout(async () => {
    results.value = await store.searchAll(trimmed)
    searched.value = true
  }, 300)
})

function kindText(kind: Resource['kind']): string {
  if (kind === 'app') return '程序'
  if (kind === 'web') return '网页'
  return '文件'
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div class="search-card" ref="cardRef" role="dialog" aria-label="全局搜索" aria-modal="true">
          <div class="search-input-row">
            <Search class="search-icon" :size="17" :stroke-width="1.8" />
            <input
              ref="inputRef"
              v-model="keyword"
              class="search-input"
              type="text"
              placeholder="搜索资源与笔记…"
            />
            <kbd class="esc-hint">ESC</kbd>
          </div>

          <div class="search-results" role="listbox" aria-label="搜索结果">
            <!-- 资源 -->
            <template v-if="results.resources.length > 0">
              <p class="result-group-title" role="presentation">速达</p>
              <div
                v-for="(r, idx) in results.resources"
                :key="'r' + r.id"
                class="result-item"
                :class="{ active: idx === activeIndex }"
                :data-search-key="'r' + r.id"
                role="option"
                :aria-selected="idx === activeIndex"
                @click="emit('openResource', r)"
                @mouseenter="activeIndex = idx"
              >
                <span class="result-badge" :class="r.kind">
                  {{ kindText(r.kind) }}
                </span>
                <span class="result-name">{{ r.name }}</span>
                <span class="result-sub">{{ r.target }}</span>
              </div>
            </template>

            <!-- 笔记 -->
            <template v-if="results.notes.length > 0">
              <p class="result-group-title" role="presentation">速记</p>
              <div
                v-for="(n, idx) in results.notes"
                :key="'n' + n.id"
                class="result-item"
                :class="{ active: results.resources.length + idx === activeIndex }"
                :data-search-key="'n' + n.id"
                role="option"
                :aria-selected="results.resources.length + idx === activeIndex"
                @click="emit('openNote', n)"
                @mouseenter="activeIndex = results.resources.length + idx"
              >
                <span class="result-badge note-badge">笔记</span>
                <span class="result-name">{{ n.title }}</span>
                <span class="result-sub">
                  {{ n.content.replace(/\s+/g, ' ').slice(0, 60) }}
                </span>
              </div>
            </template>

            <!-- 待办 -->
            <template v-if="results.todos.length > 0">
              <p class="result-group-title" role="presentation">待办</p>
              <div
                v-for="(t, idx) in results.todos"
                :key="'t' + t.id"
                class="result-item"
                :class="{ active: results.resources.length + results.notes.length + idx === activeIndex }"
                :data-search-key="'t' + t.id"
                role="option"
                :aria-selected="results.resources.length + results.notes.length + idx === activeIndex"
                @click="emit('openTodo', t)"
                @mouseenter="activeIndex = results.resources.length + results.notes.length + idx"
              >
                <span class="result-badge todo-badge">{{ t.done ? '已完成' : '待完成' }}</span>
                <span class="result-name">{{ t.title }}</span>
                <span class="result-sub">{{ ['普通', '重要', '紧急'][t.priority] ?? '普通' }}优先级</span>
              </div>
            </template>

            <!-- 状态 -->
            <div v-if="searched && results.resources.length === 0 && results.notes.length === 0 && results.todos.length === 0" class="empty-state">
              <span style="font-size: 26px">🔍</span>
              <p>未找到「{{ keyword.trim() }}」相关内容</p>
            </div>
            <div
              v-else-if="!keyword.trim()"
              class="empty-state"
              style="padding: 24px 16px"
            >
              <p style="color: var(--text-4)">输入关键词检索速达资源、速记与待办</p>
              <div class="shortcut-hints">
                <span><kbd>Ctrl</kbd> + <kbd>K</kbd> 唤起搜索</span>
                <span><kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Space</kbd> 显示/隐藏窗口</span>
                <span><kbd>Esc</kbd> 关闭</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.search-card {
  width: 560px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  overflow: hidden;
  animation: card-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
}
@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(10px) scale(0.97);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.search-input-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-soft);
}
.search-icon {
  color: var(--text-3);
  flex-shrink: 0;
}
.search-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: 15px;
  font-family: inherit;
  color: var(--text-1);
}
.search-input::placeholder {
  color: var(--text-4);
}
.esc-hint {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--text-4);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 2px 6px;
  font-family: inherit;
}

.search-results {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 12px 16px;
}
.result-group-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-3);
  padding: 8px 10px 4px;
}
.result-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.12s;
}
.result-item:hover,
.result-item.active {
  background: var(--brand-50);
}
.result-badge {
  flex-shrink: 0;
  width: 34px;
  text-align: center;
  font-size: 10px;
  font-weight: 600;
  border-radius: 6px;
  padding: 3px 0;
}
.result-badge.app {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.result-badge.web {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.result-badge.file {
  background: var(--c-purple-soft);
  color: var(--c-purple-ink);
}
.note-badge {
  background: var(--c-yellow-soft);
  color: var(--c-yellow-ink);
}
.todo-badge {
  background: var(--c-gray-soft);
  color: var(--c-gray-ink);
}
.shortcut-hints {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 12px;
  font-size: 12px;
  color: var(--text-3);
}
.shortcut-hints kbd {
  font-family: inherit;
  font-size: 11px;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  color: var(--text-2);
}
.result-name {
  flex-shrink: 0;
  max-width: 160px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.result-sub {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
