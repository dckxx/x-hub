<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  isUnauthorizedMessage,
  requestOpenExtensionSettings,
  useExtensionFrame,
} from '../composables/useExtensionFrame'
import { useTheme } from '../composables/useTheme'

// 独立扩展窗口也要应用宿主主题：否则根元素无 data-theme/--accent，
// 扩展 iframe 拉到的主题令牌是 :root 默认值，无法跟随用户换色/换主题
useTheme()

// 窗口 label 形如 ext-<id 的 base64url>；独立窗口自带系统标题栏，这里只渲染扩展内容。
// Rust 侧因窗口 label 只允许字母数字与 -/_ 等字符（扩展 id 含点号），用 base64url 编码。
function decodeExtWindowId(label: string): string {
  if (!label.startsWith('ext-')) return label
  let s = label.slice('ext-'.length)
  let b64 = s.replace(/-/g, '+').replace(/_/g, '/')
  while (b64.length % 4) b64 += '='
  try {
    return decodeURIComponent(
      atob(b64)
        .split('')
        .map((c) => '%' + c.charCodeAt(0).toString(16).padStart(2, '0'))
        .join(''),
    )
  } catch {
    return label.slice('ext-'.length) // 兼容旧未编码 label
  }
}

const label = getCurrentWindow().label
const extId = computed(() => decodeExtWindowId(label))

// 独立窗口拿不到主窗 provide 的 `showToast`（跨窗口注入不通用），自带一条最小提示：
// 扩展开外链失败（最典型 = 未声明 `open-url` 权限）时必须让用户看见原因，
// 否则表现就是「点了没反应」——正是这次要修的静默失败。
const toast = ref('')
let toastTimer: number | undefined
function showToast(msg: string) {
  toast.value = msg
  if (toastTimer !== undefined) clearTimeout(toastTimer)
  toastTimer = window.setTimeout(() => {
    toast.value = ''
  }, 2600)
}
onBeforeUnmount(() => {
  if (toastTimer !== undefined) clearTimeout(toastTimer)
})

const { frameRef, loading, error } = useExtensionFrame(
  () => extId.value,
  () => null,
  showToast,
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef

// service 后端未授权时宿主拦截入口加载：错误态给「去授权」（广播全局事件，主窗接手弹设置）
const unauthorized = computed(() => isUnauthorizedMessage(error.value))
</script>

<template>
  <div class="extension-window">
    <div v-if="loading" class="ew-state">
      <p>正在加载扩展…</p>
    </div>
    <div v-else-if="error" class="ew-state">
      <p class="ew-error">{{ error }}</p>
      <button v-if="unauthorized" class="ghost-btn" type="button" @click="requestOpenExtensionSettings(extId)">
        去授权
      </button>
    </div>
    <iframe
      v-show="!loading && !error"
      ref="frameRef"
      class="ew-frame"
      title="扩展窗口"
      sandbox="allow-scripts allow-same-origin allow-downloads"
    />
    <div v-if="toast" class="ew-toast">{{ toast }}</div>
  </div>
</template>

<style scoped>
.extension-window {
  height: 100vh;
  width: 100vw;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: transparent;
}
.ew-frame {
  flex: 1;
  /* 同 ExtensionView：替换元素的自动最小高度 = iframe 固有高度（150px），
     窗口高度小于它时会被裁，显式 min-height: 0 才能收缩 */
  min-height: 0;
  width: 100%;
  border: 0;
  background: transparent;
}
.ew-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 0.8125rem;
}
.ew-error {
  color: var(--c-red);
}
/* 独立窗口的最小 toast：底部居中浮层，样式对齐宿主主窗 toast 的口径（深底浅字 + 圆角 + 落影） */
.ew-toast {
  position: fixed;
  left: 50%;
  bottom: 24px;
  transform: translateX(-50%);
  max-width: min(520px, calc(100vw - 48px));
  padding: 8px 14px;
  border-radius: var(--radius-lg, 10px);
  background: var(--frost-surface, rgba(28, 28, 30, 0.92));
  color: var(--text-1);
  font-size: 0.8125rem;
  box-shadow: var(--shadow-card);
  z-index: 50;
}
</style>
