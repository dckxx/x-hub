<script setup lang="ts">
/**
 * 标题栏小部件的段式渲染（status 档 `segments`）。
 *
 * 宿主只认几何原语：text / icon / progress / bars / actions / stack，
 * 按数组顺序横向排布；`width` 固定宽、`grow` 吃剩余宽度。
 * 这里不理解任何业务语义（不叫「封面」「频谱」「下载进度」），
 * 所以扩展不需要宿主为它加字段——要什么就拼什么。
 *
 * 递归自引用渲染 stack 的 children（Rust 侧已把嵌套限制在 2 层）。
 */
import type { TitlebarAction, TitlebarBarsSegment, TitlebarProgressSegment, TitlebarSegment, TitlebarStackSegment } from '../composables/useTitlebarWidget'

// extWidget 是 readonly(ref) 快照，段数组整体只读；这里只渲染不修改，直接收 readonly
defineProps<{ segments: readonly TitlebarSegment[] }>()
const emit = defineEmits<{ action: [id: string] }>()

/** status 档条形类段的缺省宽度（px）：没给 width 也不 grow 时用它 */
const DEFAULT_METER_WIDTH = 96

function segStyle(seg: TitlebarSegment): Record<string, string> {
  const style: Record<string, string> = {}
  if (typeof seg.width === 'number') style.width = `${seg.width}px`
  else if (!seg.grow && (seg.type === 'progress' || seg.type === 'bars' || seg.type === 'stack')) {
    style.width = `${DEFAULT_METER_WIDTH}px`
  }
  return style
}

function barScale(seg: TitlebarBarsSegment): string[] {
  return seg.values.map((v) => `scaleY(${Math.min(1, Math.max(0, v))})`)
}

function progressScale(seg: TitlebarProgressSegment): string {
  return `scaleX(${Math.min(1, Math.max(0, seg.value))})`
}

function childrenOf(seg: TitlebarStackSegment): readonly TitlebarSegment[] {
  return seg.children || []
}

function onAction(a: TitlebarAction) {
  emit('action', a.id)
}
</script>

<template>
  <template v-for="(seg, i) in segments" :key="i">
    <span
      v-if="seg.type === 'text'"
      class="tb-seg-text"
      :class="[`tb-tone-${seg.tone || 'primary'}`, { 'tb-grow': seg.grow }]"
      :style="segStyle(seg)"
      :title="seg.text"
    >{{ seg.text }}</span>
    <span v-else-if="seg.type === 'icon'" class="tb-seg-icon" :style="segStyle(seg)" aria-hidden="true">{{ seg.glyph }}</span>
    <span
      v-else-if="seg.type === 'progress'"
      class="tb-seg-progress"
      :class="{ 'tb-grow': seg.grow }"
      :style="segStyle(seg)"
      aria-hidden="true"
    ><i :style="{ transform: progressScale(seg) }"></i></span>
    <span
      v-else-if="seg.type === 'bars'"
      class="tb-seg-bars"
      :class="{ 'tb-grow': seg.grow }"
      :style="segStyle(seg)"
      aria-hidden="true"
    ><i v-for="(t, j) in barScale(seg)" :key="j" :style="{ transform: t }"></i></span>
    <span v-else-if="seg.type === 'actions'" class="tb-seg-actions" :class="{ 'tb-grow': seg.grow }" :style="segStyle(seg)">
      <button v-for="a in seg.items" :key="a.id" class="tb-act" @click="onAction(a)">{{ a.label }}</button>
    </span>
    <span
      v-else-if="seg.type === 'stack'"
      class="tb-seg-stack"
      :class="{ 'tb-grow': seg.grow }"
      :style="segStyle(seg)"
    >
      <TitlebarSegments :segments="childrenOf(seg)" @action="emit('action', $event)" />
    </span>
  </template>
</template>

<style scoped>
/* 弹性段：吃剩余宽度。同一行通常只给一个段，否则按 flex-grow 比例分 */
.tb-grow {
  flex: 1 1 auto;
  min-width: 0;
}
/* ---- 文本段 ---- */
.tb-seg-text {
  flex: 0 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tb-seg-text.tb-tone-primary {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-1);
}
.tb-seg-text.tb-tone-muted {
  font-size: 11px;
  color: var(--text-3);
}
/* ---- 图标段（字符 / emoji，随主题文字色） ---- */
.tb-seg-icon {
  font-size: 14px;
  line-height: 1;
  flex-shrink: 0;
}
/* ---- 进度线段：轨道 + scaleX 驱动的填充（周期更新不触发布局） ---- */
.tb-seg-progress {
  position: relative;
  display: block;
  height: 2px;
  border-radius: 1px;
  background: var(--border-soft);
  overflow: hidden;
  flex-shrink: 0;
}
.tb-seg-progress i {
  display: block;
  height: 100%;
  background: var(--brand-500);
  transform-origin: left;
}
/* ---- 高度条组：一排细条均分整宽、底对齐。高度经 scaleY 驱动（transform 不触发布局，
   见约定 19 的同类口径），origin 在底边、从下往上生长 ---- */
.tb-seg-bars {
  display: flex;
  align-items: stretch;
  gap: 2px;
  height: 8px;
  flex-shrink: 0;
}
.tb-seg-bars i {
  flex: 1;
  min-width: 1px;
  background: var(--brand-500);
  border-radius: 1px 1px 0 0;
  transform-origin: bottom;
  transition: transform 0.12s linear;
}
/* 窄窗口减少可见条数（只留前 N 根）：保证每根仍是 ≥1px 细条，
   而不是被 min-width 撑破容器后与相邻段重叠 */
@media (max-width: 980px) {
  .tb-seg-bars i:nth-child(n + 25) {
    display: none;
  }
}
@media (max-width: 860px) {
  .tb-seg-bars i:nth-child(n + 19) {
    display: none;
  }
}
/* ---- 按钮组 ---- */
.tb-seg-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.tb-act {
  height: 22px;
  padding: 0 8px;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  background: transparent;
  font-size: 11px;
  color: var(--text-2);
  cursor: pointer;
  white-space: nowrap;
  transition: border-color 0.15s, color 0.15s;
}
.tb-act:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
/* ---- 纵向叠放容器：children 依次上下排列，各自撑满宽度 ---- */
.tb-seg-stack {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 2px;
  flex-shrink: 0;
  height: 100%;
  box-sizing: border-box;
}
</style>
