<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Pin, PinOff, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

// 从窗口 label 解析槽位（sticky-1 / sticky-2）
const label = isTauri() ? getCurrentWindow().label : 'sticky-1'
const slot = Number(label.replace(/^sticky-/, '')) || 1

const store = useStore()
const content = ref('')
const pinned = ref(true)
let saveTimer: ReturnType<typeof setTimeout> | null = null

// 关闭弹窗
const showDialog = ref(false)
const dialogMode = ref<'restore' | 'delete-only'>('restore') // restore: 还原/删除/取消；delete-only: 删除/取消

// 主题跟随配置
const theme = computed(() => store.state.config.theme)
watch(
  theme,
  (t) => {
    document.documentElement.dataset.theme = t === 'dark' ? 'dark' : ''
  },
  { immediate: true },
)

// 标记为浮窗窗口：body 透明，只显示卡片本体
document.documentElement.dataset.stickyWindow = ''

onMounted(async () => {
  await store.loadInitialData()
  const mine = store.state.detached.find((d) => d.slot === slot)
  content.value = mine?.content ?? ''
  pinned.value = mine?.always_on_top ?? true
})

// 输入即保存（600ms 防抖）
watch(content, () => {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    store.saveDetachedSticky(slot, content.value)
  }, 600)
})

onBeforeUnmount(() => {
  if (saveTimer) clearTimeout(saveTimer)
})

// 置顶切换
function togglePin() {
  pinned.value = !pinned.value
  store.toggleDetachedStickyPin(slot, pinned.value)
}

// 关闭：
// - 空内容 → 直接删除
// - 有关联空闲槽 → 还原/删除/取消
// - 两槽都有内容 → 删除/取消
async function onClose() {
  // 先落盘当前输入，避免丢字
  if (saveTimer) {
    clearTimeout(saveTimer)
    saveTimer = null
    await store.saveDetachedSticky(slot, content.value)
  }
  if (!content.value.trim()) {
    await store.deleteDetachedSticky(slot)
    return
  }
  const stickies = await tauriApi.listStickies()
  const occupied = (s: number) =>
    stickies.find((x) => x.slot === s)?.content.trim().length ?? 0 > 0
  const hasFreeSlot = !occupied(1) || !occupied(2)
  dialogMode.value = hasFreeSlot ? 'restore' : 'delete-only'
  showDialog.value = true
}

async function onRestore() {
  showDialog.value = false
  try {
    await store.restoreDetachedSticky(slot)
  } catch (e) {
    // 两槽都满了：降级为删除确认
    dialogMode.value = 'delete-only'
    showDialog.value = true
  }
}

async function onDelete() {
  showDialog.value = false
  await store.deleteDetachedSticky(slot)
}

function onCancel() {
  showDialog.value = false
}

// 窗口拖动：按下后指针发生实际位移才启动拖动，避免单纯点击误触发
// Windows 模态拖动循环（鼠标松开事件被 WebView 吞掉）导致整个应用卡死
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
  <div class="floating-sticky">
    <header class="fs-header" @mousedown="onMouseDown" @mousemove="onMouseMove" @mouseup="onDragEnd" @mouseleave="onDragEnd">
      <span class="fs-title">便签</span>
      <div class="fs-controls">
        <button
          class="fs-btn"
          :class="{ active: pinned }"
          :title="pinned ? '取消置顶' : '置顶'"
          type="button"
          @click="togglePin"
        >
          <Pin v-if="pinned" :size="13" :stroke-width="2" />
          <PinOff v-else :size="13" :stroke-width="2" />
        </button>
        <button class="fs-btn fs-close" title="关闭" type="button" @click="onClose">
          <X :size="13" :stroke-width="2" />
        </button>
      </div>
    </header>
    <textarea
      v-model="content"
      class="fs-input"
      placeholder="随手记…"
      spellcheck="false"
    ></textarea>

    <Teleport to="body">
      <Transition name="dialog">
        <div v-if="showDialog" class="fs-mask" @mousedown.self="onCancel">
          <div class="fs-dialog">
            <p class="fs-dialog-text">
              {{ dialogMode === 'restore' ? '将便签还原到主面板？' : '两个便签槽位都已有内容，无法还原，只能删除该浮窗。' }}
            </p>
            <div class="fs-dialog-actions">
              <button
                v-if="dialogMode === 'restore'"
                class="fs-dialog-btn primary"
                type="button"
                @click="onRestore"
              >
                还原
              </button>
              <button class="fs-dialog-btn danger" type="button" @click="onDelete">
                删除
              </button>
              <button class="fs-dialog-btn" type="button" @click="onCancel">
                取消
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.floating-sticky {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 10px;
  box-sizing: border-box;
  -webkit-app-region: no-drag;
}
.fs-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
  cursor: move;
}
.fs-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  padding-left: 2px;
}
.fs-controls {
  display: flex;
  gap: 2px;
}
.fs-btn {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 6px;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.fs-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.fs-btn.active {
  color: var(--brand-500);
}
.fs-btn.fs-close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
.fs-input {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: none;
  background: transparent;
  resize: none;
  outline: none;
  font-size: 12px;
  line-height: 1.6;
  font-family: inherit;
  color: var(--text-2);
  padding: 0 2px;
}
.fs-input::placeholder {
  color: var(--text-4);
}

/* 关闭确认弹窗 */
.fs-mask {
  position: fixed;
  inset: 0;
  z-index: 100;
  background: var(--scrim);
  display: flex;
  align-items: center;
  justify-content: center;
  backdrop-filter: blur(4px);
  -webkit-backdrop-filter: blur(4px);
}
.fs-dialog {
  width: 200px;
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: 12px;
  box-shadow: var(--shadow-dock);
  padding: 14px;
  animation: fs-card-in 0.18s ease-out;
}
@keyframes fs-card-in {
  from { opacity: 0; transform: translateY(8px) scale(0.97); }
  to { opacity: 1; transform: translateY(0) scale(1); }
}
.fs-dialog-text {
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-1);
  margin-bottom: 12px;
  word-break: break-all;
}
.fs-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}
.fs-dialog-btn {
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 500;
  padding: 4px 10px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.fs-dialog-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.fs-dialog-btn.primary {
  background: var(--brand-500);
  border-color: transparent;
  color: var(--text-on-accent);
  font-weight: 600;
}
.fs-dialog-btn.primary:hover {
  background: var(--brand-600);
  color: var(--text-on-accent);
}
.fs-dialog-btn.danger {
  color: var(--c-red-ink);
}
.fs-dialog-btn.danger:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
  border-color: transparent;
}

.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 0.18s ease-out;
}
.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}
</style>
