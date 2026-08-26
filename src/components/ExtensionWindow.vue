<script setup lang="ts">
import { computed } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useExtensionFrame } from '../composables/useExtensionFrame'
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

const { frameRef, loading, error } = useExtensionFrame(
  () => extId.value,
  () => null,
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef
</script>

<template>
  <div class="extension-window">
    <div v-if="loading" class="ew-state">
      <p>正在加载扩展…</p>
    </div>
    <div v-else-if="error" class="ew-state">
      <p class="ew-error">{{ error }}</p>
    </div>
    <iframe
      v-show="!loading && !error"
      ref="frameRef"
      class="ew-frame"
      title="扩展窗口"
    />
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
</style>
