<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { Note, Resource } from '../api/tauri'
import { useStore } from '../stores/workbench'

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'openResource', r: Resource): void
  (e: 'openNote', n: Note): void
}>()

const store = useStore()

const keyword = ref('')
const results = ref<{ resources: Resource[]; notes: Note[] }>({
  resources: [],
  notes: [],
})
const searched = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)

let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.visible,
  (v) => {
    if (v) {
      keyword.value = ''
      results.value = { resources: [], notes: [] }
      searched.value = false
      setTimeout(() => inputRef.value?.focus(), 30)
    }
  },
)

watch(keyword, (kw) => {
  if (searchTimer) clearTimeout(searchTimer)
  const trimmed = kw.trim()
  if (!trimmed) {
    results.value = { resources: [], notes: [] }
    searched.value = false
    return
  }
  searchTimer = setTimeout(async () => {
    results.value = await store.searchAll(trimmed)
    searched.value = true
  }, 300)
})

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="emit('close')">
        <div class="search-card" role="dialog" aria-label="全局搜索">
          <div class="search-input-row">
            <svg class="search-icon" width="17" height="17" viewBox="0 0 24 24" fill="none">
              <circle cx="11" cy="11" r="7" stroke="currentColor" stroke-width="2" />
              <path d="M20 20l-3.5-3.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
            </svg>
            <input
              ref="inputRef"
              v-model="keyword"
              class="search-input"
              type="text"
              placeholder="搜索资源与笔记…"
              @keydown="onKeydown"
            />
            <kbd class="esc-hint">ESC</kbd>
          </div>

          <div class="search-results">
            <!-- 资源 -->
            <template v-if="results.resources.length > 0">
              <p class="result-group-title">快捷资源</p>
              <div
                v-for="r in results.resources"
                :key="'r' + r.id"
                class="result-item"
                @click="emit('openResource', r)"
              >
                <span class="result-badge" :class="r.kind">
                  {{ r.kind === 'app' ? '程序' : '网页' }}
                </span>
                <span class="result-name">{{ r.name }}</span>
                <span class="result-sub">{{ r.target }}</span>
              </div>
            </template>

            <!-- 笔记 -->
            <template v-if="results.notes.length > 0">
              <p class="result-group-title">笔记</p>
              <div
                v-for="n in results.notes"
                :key="'n' + n.id"
                class="result-item"
                @click="emit('openNote', n)"
              >
                <span class="result-badge note-badge">笔记</span>
                <span class="result-name">{{ n.title }}</span>
                <span class="result-sub">
                  {{ n.content.replace(/\s+/g, ' ').slice(0, 60) }}
                </span>
              </div>
            </template>

            <!-- 状态 -->
            <div v-if="searched && results.resources.length === 0 && results.notes.length === 0" class="empty-state">
              <span style="font-size: 26px">🔍</span>
              <p>未找到「{{ keyword.trim() }}」相关内容</p>
            </div>
            <div
              v-else-if="!keyword.trim()"
              class="empty-state"
              style="padding: 28px 16px"
            >
              <p style="color: var(--text-4)">输入关键词检索快捷资源与笔记</p>
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
  animation: card-in 0.2s cubic-bezier(0.2, 0.9, 0.3, 1.2);
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
  text-transform: uppercase;
  letter-spacing: 0.04em;
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
.result-item:hover {
  background: var(--brand-50);
}
.result-badge {
  flex-shrink: 0;
  width: 34px;
  text-align: center;
  font-size: 10px;
  font-weight: 600;
  color: #fff;
  border-radius: 6px;
  padding: 3px 0;
}
.result-badge.app {
  background: var(--c-blue);
}
.result-badge.web {
  background: var(--c-green);
}
.note-badge {
  background: var(--c-yellow);
  color: #7c5e00;
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
