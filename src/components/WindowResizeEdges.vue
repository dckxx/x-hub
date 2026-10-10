<script setup lang="ts">
// 主窗 8 方向隐形缩放边缘。
// 背景：v0.8.1 起 main 窗 shadow=false（1cda658 去左右下黑边）。Windows 无边框窗口的
// 边缘拖拽缩放靠 shadow=true 时宿主窗口那圈阴影 insets 的 WM_NCHITTEST 命中检测；
// shadow=false 后 webview 铺满整个窗口，宿主永远收不到边缘命中 → 拖边调大小全废
// （黑边和 resize 本来就是同一条边）。解法与待办浮窗（TodoFloat.vue）同款：
// 透明覆盖条 mousedown 时调 startResizeDragging，系统直接接管进模态 resize 循环，
// 不依赖宿主窗口自身的命中检测（TodoFloat 已久经验证）。
// 最大化/全屏时隐藏：系统本就不许最大化窗口拖边调大小，且避免边缘条盖住内容
// 最外沿挡点击（如最右侧滚动条的外 8px）；还原窗口后自动恢复。
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

// 与 @tauri-apps/api window 的 ResizeDirection 同构（该类型未导出，本地声明，同 TodoFloat）
type ResizeDirection =
  | 'East'
  | 'North'
  | 'NorthEast'
  | 'NorthWest'
  | 'South'
  | 'SouthEast'
  | 'SouthWest'
  | 'West'
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

const appWindow = getCurrentWindow()
const blocked = ref(false)

async function refreshBlocked() {
  try {
    blocked.value = (await appWindow.isMaximized()) || (await appWindow.isFullscreen())
  } catch {
    blocked.value = false
  }
}

let unlisten: (() => void) | null = null

onMounted(async () => {
  await refreshBlocked()
  // 尺寸变化（最大化/还原/全屏切换）都会触发 Resized，按当前状态显隐边缘条
  unlisten = await appWindow.onResized(() => {
    void refreshBlocked()
  })
})

onBeforeUnmount(() => {
  unlisten?.()
})

function onResizeStart(e: MouseEvent, dir: ResizeDirection) {
  if (e.button !== 0) return
  e.preventDefault()
  e.stopPropagation()
  void appWindow.startResizeDragging(dir)
}
</script>

<template>
  <template v-if="!blocked">
    <div
      v-for="dir in RESIZE_DIRECTIONS"
      :key="dir"
      class="win-rz"
      :class="'win-rz-' + dir.toLowerCase()"
      @mousedown="onResizeStart($event, dir)"
    ></div>
  </template>
</template>

<style scoped>
/* 缩放边缘区：边 6px、角 14px，透明叠加在内容之上。z-index 取 90：
   盖过普通内容与标题栏顶沿，但低于全屏遮罩类弹窗（z 200+），弹窗边缘仍可点遮罩 */
.win-rz {
  position: fixed;
  z-index: 90;
}
.win-rz-north {
  top: 0;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.win-rz-south {
  bottom: 0;
  left: 0;
  right: 0;
  height: 6px;
  cursor: ns-resize;
}
.win-rz-east {
  top: 0;
  bottom: 0;
  right: 0;
  width: 6px;
  cursor: ew-resize;
}
.win-rz-west {
  top: 0;
  bottom: 0;
  left: 0;
  width: 6px;
  cursor: ew-resize;
}
.win-rz-northeast {
  top: 0;
  right: 0;
  width: 14px;
  height: 14px;
  cursor: nesw-resize;
}
.win-rz-southwest {
  bottom: 0;
  left: 0;
  width: 14px;
  height: 14px;
  cursor: nesw-resize;
}
.win-rz-northwest {
  top: 0;
  left: 0;
  width: 14px;
  height: 14px;
  cursor: nwse-resize;
}
.win-rz-southeast {
  bottom: 0;
  right: 0;
  width: 14px;
  height: 14px;
  cursor: nwse-resize;
}
</style>
