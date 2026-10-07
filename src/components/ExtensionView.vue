<script setup lang="ts">
import { computed, inject } from 'vue'
import { Puzzle } from 'lucide-vue-next'
import {
  isUnauthorizedMessage,
  requestOpenExtensionSettings,
  useExtensionFrame,
} from '../composables/useExtensionFrame'

const props = defineProps<{
  extId: string
  surface?: string | null
  /** 工作台模块形态 id（module 形态多形态时生效；经 URL query + postMessage 传给扩展） */
  variant?: string | null
  /** 强制重载计数：宿主每次「打开该扩展」都递增，点击同一个已打开的扩展也触发 iframe 重新导航 */
  reloadKey?: number
  /** module 卡片表头文案（宿主侧已解析：用户自定义标题 ?? manifest.name） */
  title?: string
  /** 关闭 module 卡片表头（扩展作者可声明默认关闭，用户在布局编辑器里也可按卡片覆盖） */
  hideTitle?: boolean
  onOpenSurface?: (surface: string) => void
}>()

// module 形态 = 工作台卡片，套用宿主 .card 玻璃外观；view 形态 = 整页，容器透明透出宿主页面渐变
const isModule = computed(() => props.surface === 'module')

// 宿主表头只给 module 卡片（view / window / drawer 是整页容器，标题栏由外层负责）
const showHeader = computed(() => isModule.value && props.hideTitle !== true)

const emit = defineEmits<{
  close: []
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const { frameRef, loading, error } = useExtensionFrame(
  () => props.extId,
  () => props.surface ?? null,
  (msg) => showToast(`打开扩展失败：${msg}`),
  props.onOpenSurface,
  () => props.reloadKey ?? 0,
  () => props.variant ?? null,
)
// frameRef 仅用于模板 ref 绑定（vue-tsc 不把模板 ref 计为读取，此处显式保留引用通过 noUnusedLocals）
void frameRef

// service 后端未授权时宿主直接拦截入口加载（扩展页面不会运行）：
// 错误态给「去授权」直达权限设置，用户不必自己找入口
const unauthorized = computed(() => isUnauthorizedMessage(error.value))
</script>

<template>
  <div class="extension-view" :class="{ card: isModule }">
    <!-- module 卡片表头：与内置模块卡一致（标题 + 品牌色图标），iframe 填满其下剩余空间 -->
    <header v-if="showHeader" class="ev-header">
      <h3 class="ev-title">
        <Puzzle :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? extId }}</span>
      </h3>
    </header>
    <div v-if="loading" class="ev-state">
      <p>正在加载扩展…</p>
    </div>
    <div v-else-if="error" class="ev-state">
      <p class="ev-error">{{ error }}</p>
      <div class="ev-actions">
        <button v-if="unauthorized" class="ghost-btn" type="button" @click="requestOpenExtensionSettings(extId)">
          去授权
        </button>
        <button class="ghost-btn" type="button" @click="emit('close')">返回</button>
      </div>
    </div>
    <iframe
      v-show="!loading && !error"
      ref="frameRef"
      class="ev-frame"
      title="扩展视图"
      sandbox="allow-scripts allow-same-origin allow-downloads"
    />
  </div>
</template>

<style scoped>
.extension-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
/* module 卡片：复用宿主 .card 玻璃表面 + 边框 + 圆角 + 阴影，与工作台其他卡片严格统一 */
.extension-view.card {
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--frost-edge), var(--shadow-card);
}
.ev-header {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  /* 左右与扩展入口自身的 12px 内边距对齐；下方留 0：iframe 里由扩展自己留白 */
  padding: 12px 12px 0;
}
.ev-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
  min-width: 0;
}
.ev-title span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ev-title svg {
  color: var(--brand-500);
  flex-shrink: 0;
}
.ev-frame {
  flex: 1;
  /* iframe 是替换元素：flex 项的自动最小尺寸取它的固有高度（无 height 属性时 = 150px），
     矮卡片（< 150px）里它不肯收缩，会溢出卡片被 overflow: hidden 裁掉底部。
     必须显式 min-height: 0 才允许它跟着内容区收缩。 */
  min-height: 0;
  width: 100%;
  border: 0;
  background: transparent;
}
.ev-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.8125rem;
}
.ev-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ev-error {
  color: var(--c-red);
}
</style>
