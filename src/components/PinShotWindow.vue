<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'

// 贴图窗口（盯住功能）：截图钉在屏幕上置顶展示，可拖动、随时参考。
// 窗口由 Rust 预创建复用（recorder.rs::init，pin-shot-{0..2}），每次贴图：
// 后端定位 + 物理尺寸 + show → 本组件挂载后 recorderPinTake 拉取 asset URL 显示；
// 点 × 关闭 = 后端清 slot + 删 pins 文件 + hide（运行期只显隐，约定 41）。
document.documentElement.dataset.pinShot = ''

const appWindow = isTauri() ? getCurrentWindow() : null
const src = ref('')
const loadFailed = ref(false)
// 鼠标是否悬停在贴图上：关闭按钮 v-show 由它驱动——CSS :hover 在透明窗口上
// 命中测试不可靠（透明像素也算命中/不命中随 WebView2 版本漂），JS 事件最稳
const hovered = ref(false)

/** 整图拖动（约定 6：startDragging + mousedown 监听，非 data-tauri-drag-region） */
function onMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  // 关闭按钮上的按下不触发拖动
  if ((e.target as HTMLElement).closest('.ps-close')) return
  if (isTauri()) void appWindow?.startDragging()
}

async function close() {
  if (!isTauri()) return
  try {
    await tauriApi.recorderPinClose()
  } catch {
    // 后端已不可达时窗口也无意义，直接隐藏兜底
    await appWindow?.hide()
  }
}

/** 拉取本窗口的贴图（pin-shot-shown 事件触发；窗口复用、组件只挂载一次，
 *  onMounted 时的初次拉取发生在启动时、slot 还是空的，必须靠事件驱动） */
async function fetchPin() {
  if (!isTauri()) return
  try {
    const path = await tauriApi.recorderPinTake()
    // 换图先清旧 src，避免上一张的残影闪现
    src.value = ''
    loadFailed.value = false
    src.value = convertFileSrc(path)
  } catch {
    loadFailed.value = true
  }
}

/** Esc 关闭贴图（钉住时窗口获得焦点；点别处焦点让出后再按 Esc 不生效，点回贴图即可） */
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    void close()
  }
}

let unlistenShown: (() => void) | null = null

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  if (!isTauri()) return
  try {
    // 每次钉屏 Rust 都 emit pin-shot-shown（窗口 show 后），组件收到即拉取新图
    unlistenShown = await listen('pin-shot-shown', () => void fetchPin())
  } catch {
    // 监听失败只影响重复钉同一窗口，首次仍可用
  }
  void fetchPin()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  unlistenShown?.()
})
</script>

<template>
  <div
    class="ps-root"
    @mousedown="onMouseDown"
    @mouseenter="hovered = true"
    @mouseleave="hovered = false"
  >
    <img v-if="src" class="ps-img" :src="src" draggable="false" alt="贴图" />
    <div v-else-if="loadFailed" class="ps-fallback">贴图加载失败</div>
    <button v-show="hovered" class="ps-close" type="button" title="关闭贴图" @click="close">
      <X :size="13" :stroke-width="2.2" />
    </button>
  </div>
</template>

<style scoped>
.ps-root {
  position: fixed;
  inset: 0;
  user-select: none;
  background: transparent;
  /* 拖动光标 + 细描边让贴图在任何壁纸上可辨识 */
  cursor: move;
  outline: 1px solid rgba(255, 255, 255, 0.25);
  outline-offset: -1px;
}
.ps-img {
  display: block;
  width: 100vw;
  height: 100vh;
  /* 贴图 = 截图原样展示；Windows 缩放下载入模糊感可用高质量插值缓解 */
  image-rendering: -webkit-optimize-contrast;
  pointer-events: none;
}
.ps-fallback {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(16, 18, 30, 0.9);
  color: #ff8b8b;
  font-size: 0.75rem;
}
.ps-close {
  position: absolute;
  top: 2px;
  right: 2px;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 5px;
  background: rgba(16, 18, 30, 0.72);
  color: #e8eaf2;
  cursor: pointer;
  transition: opacity 0.15s;
}
.ps-close:hover {
  background: rgba(245, 69, 69, 0.9);
  color: #fff;
}
</style>
