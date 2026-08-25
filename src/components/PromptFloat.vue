<script setup lang="ts">
import { inject, onBeforeUnmount, onMounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { Boxes, Pin, X } from 'lucide-vue-next'
import { isTauri } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useTheme } from '../composables/useTheme'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

// 标记为提示词浮窗：body 透明，只显示卡片本体
document.documentElement.dataset.promptFloat = ''
useTheme()

let unlisten: (() => void) | null = null

onMounted(async () => {
  await store.loadInitialData()
  if (isTauri()) {
    unlisten = await listen('snippets-changed', () => {
      void store.loadSnippets()
    })
  }
})

onBeforeUnmount(() => {
  unlisten?.()
})

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    let ok = false
    try {
      ok = document.execCommand('copy')
    } catch {
      ok = false
    }
    document.body.removeChild(ta)
    return ok
  }
}

async function onCopy(id: number, content: string) {
  const ok = await copyText(content)
  if (!ok) {
    showToast('复制失败')
    return
  }
  try {
    await store.recordSnippetCopy(id)
  } catch {
    // 计数失败不阻塞复制反馈
  }
  showToast('已复制')
}

async function onClose() {
  if (isTauri()) await getCurrentWindow().close()
}

// 窗口拖动（按下后指针发生实际位移才启动拖动，避免误触）
const appWindow = isTauri() ? getCurrentWindow() : null
const DRAG_THRESHOLD = 4
let dragPending: { x: number; y: number } | null = null

function onMouseDown(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return
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
    class="pf-root"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onDragEnd"
    @mouseleave="onDragEnd"
  >
    <header class="pf-header">
      <h3 class="pf-title">
        <Boxes :size="14" :stroke-width="2" aria-hidden="true" />
        <span>提示词</span>
      </h3>
      <button class="pf-close" title="关闭" aria-label="关闭" @click="onClose">
        <X :size="14" :stroke-width="2" />
      </button>
    </header>

    <div v-if="store.state.snippets.length > 0" class="pf-body">
      <button
        v-for="s in store.state.snippets"
        :key="s.id"
        class="pf-row"
        type="button"
        :title="'点击复制：' + s.title"
        @click="onCopy(s.id, s.content)"
      >
        <span class="pf-row-title">
          <span class="pf-row-title-text" :title="s.title">{{ s.title }}</span>
          <Pin v-if="s.is_pinned" class="pf-row-pin" :size="12" :stroke-width="2" aria-label="已置顶" />
        </span>
        <span class="pf-row-preview" :title="s.content">{{ s.content }}</span>
      </button>
    </div>
    <div v-else class="pf-empty">
      <p>暂无提示词</p>
      <p class="pf-empty-sub">到工作台「提示词」卡片中添加</p>
    </div>
  </div>
</template>

<style scoped>
.pf-root {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px;
  box-sizing: border-box;
  -webkit-app-region: no-drag;
  font-size: calc(1rem * var(--fs-prompt, 1));
}
.pf-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
  cursor: move;
}
.pf-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.pf-title :deep(svg) {
  color: var(--brand-500);
}
.pf-close {
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
.pf-close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
.pf-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0 -4px;
  padding: 0 4px;
}
.pf-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  text-align: left;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.18s;
}
.pf-row:hover {
  background: var(--bg-card-soft);
}
.pf-row:active {
  transform: scale(0.99);
}
.pf-row-title {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}
.pf-row-title-text {
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pf-row-pin {
  flex-shrink: 0;
  color: var(--brand-500);
}
.pf-row-preview {
  font-size: 0.75em;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pf-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  text-align: center;
  color: var(--text-4);
}
.pf-empty p {
  margin: 0;
  font-size: 0.8125em;
}
.pf-empty-sub {
  font-size: 0.6875em !important;
}
</style>
