<script setup lang="ts">
// AI 对话独立窗口（label "chat"）：无边框 + 自制标题栏的玻璃小窗，与主窗内嵌抽屉互斥
// ——由设置「AI 助手 → 以独立窗口打开 AI 对话」决定形态（标题栏按钮 / Ctrl+Shift+K /
// 悬浮球「AI 对话」入口都按该开关分流）。
//
// 窗口生命周期：后端启动预创建 + 隐藏常驻（约定 41 铁律），关闭只是 hide，本组件全程
// 存在，唤起走 show。因此组件内所有监听都是「常驻监听」，不随开关重建。
import { onBeforeUnmount, onMounted, provide, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { Pin, PinOff, X } from 'lucide-vue-next'
import ChatPanel from './ChatPanel.vue'
import { isTauri, tauriApi } from '../api/tauri'
import { applyTheme, useTheme } from '../composables/useTheme'
import { useStore } from '../stores/workbench'

const store = useStore()
const panelRef = ref<InstanceType<typeof ChatPanel> | null>(null)
const pinned = ref(false)
const appWindow = isTauri() ? getCurrentWindow() : null

// 主题三轴 + 字号缩放：启动自取（独立 WebView 不经过主窗 store 初始化链路），
// 运行时由主窗 useTheme 的 chat-theme 推送跟随
useTheme()
document.documentElement.dataset.chatWindow = ''

// ---- 窗内轻提示：ChatPanel 的「未配置模型」等提示经此渲染（主窗的 provide 到这里不可见） ----
const toast = ref('')
let toastTimer: ReturnType<typeof setTimeout> | null = null
function showToast(msg: string) {
  toast.value = msg
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toast.value = ''
  }, 2600)
}
provide('showToast', showToast)

let unlistenTheme: (() => void) | null = null
let unlistenShown: (() => void) | null = null
let unlistenHidden: (() => void) | null = null

onMounted(async () => {
  if (!isTauri()) return
  // 拖动收尾监听挂 window：按住标题栏移出元素后仍能收到 move/up（见下方拖动注释）
  window.addEventListener('mousemove', onWinMouseMove)
  window.addEventListener('mouseup', onWinMouseUp)
  try {
    // 主题的唯一来源是 useTheme 对 config 三件套的 watch：refreshConfig 更新 config
    // 即触发 applyTheme，不再直调 getThemeConfig（避免同一主题应用多遍）
    await store.refreshConfig()
  } catch {
    // 忽略：命令未就绪时用默认配置渲染
  }
  try {
    const st = await tauriApi.chatWindowGetState()
    pinned.value = st.pinned
  } catch {
    // 忽略：取不到状态时保持非置顶
  }
  unlistenTheme = await listen<{ mode: string; preset: string; accent: string | null }>(
    'chat-theme',
    (e) => applyTheme(e.payload),
  )
  // 唤起时重新拉配置与对话数据：期间可能改过模型配置或在主窗新建过会话
  // （只拉 config，业务数据由 ChatPanel.refresh 自己拉，不整库搬运）
  unlistenShown = await listen('chat-window-shown', async () => {
    try {
      await store.refreshConfig()
      const st = await tauriApi.chatWindowGetState()
      pinned.value = st.pinned
    } catch {
      // 忽略：保持当前视图
    }
    panelRef.value?.refresh()
    panelRef.value?.focusInput()
  })
  // 隐藏时卸载会话活堆（内存优化，ChatPanel.unload）：消息 DOM 随聊天增长，
  // Low 内存级别吐不掉活数据；会话在 SQLite，唤起时 refresh() 重拉
  unlistenHidden = await listen('chat-window-hidden', () => {
    panelRef.value?.unload()
  })
})

onBeforeUnmount(() => {
  if (toastTimer) clearTimeout(toastTimer)
  window.removeEventListener('mousemove', onWinMouseMove)
  window.removeEventListener('mouseup', onWinMouseUp)
  unlistenTheme?.()
  unlistenShown?.()
  unlistenHidden?.()
})

// ---- 标题栏：拖动（指针实际位移后才启动，避免点击误触发系统模态拖动） ----
// move/up 必须挂 window：只绑 .cw-bar 时，按住移出标题栏（进聊天区/出窗口）后
// mouseup 收不到，dragPending 残留，之后无按键划过标题栏就会凭空触发拖窗
const DRAG_THRESHOLD = 4
let dragPending: { x: number; y: number } | null = null

function onBarMouseDown(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const t = e.target as HTMLElement
  if (t.closest('button')) return
  dragPending = { x: e.screenX, y: e.screenY }
}
function onWinMouseMove(e: MouseEvent) {
  if (!dragPending || !appWindow) return
  const dx = e.screenX - dragPending.x
  const dy = e.screenY - dragPending.y
  if (dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD) {
    dragPending = null
    void appWindow.startDragging()
  }
}
function onWinMouseUp() {
  dragPending = null
}

async function togglePin() {
  pinned.value = !pinned.value
  if (!isTauri()) return
  try {
    await tauriApi.chatWindowSetPinned(pinned.value)
  } catch {
    pinned.value = !pinned.value
    showToast('置顶切换失败')
  }
}

// 关闭 = 隐藏常驻（后端拦截 CloseRequested 后统一走 hide，见 chat_window.rs）
async function onClose() {
  if (!isTauri()) return
  await tauriApi.chatWindowClose().catch(() => {})
}

// 「模型设置」→ 唤出主窗并定位到设置 → AI 助手 → 供应商；section="account" = 账号页（积分入口用）
async function onOpenModelSettings(section: 'providers' | 'account' = 'providers') {
  if (!isTauri()) return
  await tauriApi.chatWindowOpenSettings(section).catch(() => {})
}
</script>

<template>
  <div class="cw-root">
    <div class="cw-frame">
      <div class="cw-bar" @mousedown="onBarMouseDown">
        <span class="cw-title">AI 对话</span>
        <div class="cw-actions">
          <button
            class="cw-btn"
            :class="{ on: pinned }"
            :title="pinned ? '取消置顶' : '置顶'"
            :aria-pressed="pinned"
            type="button"
            @click="togglePin"
          >
            <PinOff v-if="pinned" :size="14" />
            <Pin v-else :size="14" />
          </button>
          <button class="cw-btn cw-close" title="关闭窗口" type="button" @click="onClose">
            <X :size="14" />
          </button>
        </div>
      </div>

      <div class="cw-body">
        <ChatPanel
          ref="panelRef"
          mode="window"
          @open-model-settings="() => onOpenModelSettings()"
          @open-account-settings="() => onOpenModelSettings('account')"
        />
      </div>

      <Transition name="cw-toast">
        <div v-if="toast" class="cw-toast" role="status">{{ toast }}</div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
/* 透明窗 + 四周留白：给落影留出绘制空间，圆角由 .cw-frame 承担 */
.cw-root {
  width: 100%;
  height: 100vh;
  padding: 10px;
  box-sizing: border-box;
  overflow: hidden;
}
.cw-frame {
  position: relative;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: var(--bg-chat-panel);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dock);
  overflow: hidden;
}
.cw-bar {
  flex: 0 0 38px;
  height: 38px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 8px 0 12px;
  border-bottom: 1px solid var(--border-soft);
  cursor: default;
  -webkit-user-select: none;
  user-select: none;
}
.cw-title {
  font-size: 0.8125rem;
  font-weight: 650;
  color: var(--text-1);
  letter-spacing: 0.01em;
}
.cw-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 4px;
}
.cw-btn {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.cw-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.cw-btn.on {
  color: var(--brand-500);
  background: var(--brand-50);
}
.cw-close:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.cw-body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.cw-toast {
  position: absolute;
  left: 50%;
  bottom: 74px;
  transform: translateX(-50%);
  max-width: 86%;
  padding: 7px 14px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  font-size: 0.75rem;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  z-index: 20;
}
.cw-toast-enter-active,
.cw-toast-leave-active {
  transition: opacity 0.18s ease-out, transform 0.18s ease-out;
}
.cw-toast-enter-from,
.cw-toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}
</style>
