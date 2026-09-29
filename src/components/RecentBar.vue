<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, watch } from 'vue'
import { ArrowRight, Flame, Globe } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Resource } from '../api/tauri'
import { iconSrc, accentOf, useResourceIcon } from '../composables/useResourceIcon'

// 标题可由工作台自定义布局覆盖：title = 自定义文案，hideTitle = 关闭标题行
defineProps<{ title?: string; hideTitle?: boolean }>()

const emit = defineEmits<{ (e: 'goSuda'): void }>()

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

// ---- 尺寸自适应：卡片固定 72px、间距 10px，按容器实际宽×高算能放几张就显示几张 ----
// 窄格（如 3×1）只显示一行几个；格子加高变宽后自动换行、按行数多显示（3×3 = 3 列 × 3 行）
const CHIP_W = 72
const CHIP_H = 80
const GAP = 10
const bodyRef = ref<HTMLElement | null>(null)
const visibleCount = ref(10)
let resizeObserver: ResizeObserver | null = null

function recomputeVisible() {
  const el = bodyRef.value
  if (!el) return
  const w = el.clientWidth
  const h = el.clientHeight
  if (w <= 0) return
  const cols = Math.max(1, Math.floor((w + GAP) / (CHIP_W + GAP)))
  // 高度还没量到（首帧）时先按一行算，下一帧 ResizeObserver 会纠正
  const rows = h > 0 ? Math.max(1, Math.floor((h + GAP) / (CHIP_H + GAP))) : 1
  visibleCount.value = cols * rows
}

watch(bodyRef, (el) => {
  resizeObserver?.disconnect()
  if (el) {
    resizeObserver = new ResizeObserver(recomputeVisible)
    resizeObserver.observe(el)
  }
  recomputeVisible()
})
onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
})

const recent = computed<Resource[]>(() =>
  [...store.state.resources]
    .filter((r) => r.last_launched_at)
    .sort(
      (a, b) =>
        new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
    )
    .slice(0, visibleCount.value),
)

// ---- 图标渲染（与 Suda 共用 useResourceIcon，保证一致） ----
const { onIconError, showImageIcon, showWebFallbackIcon, iconText, fileIconOf, accentFor } =
  useResourceIcon()

async function onOpen(r: Resource) {
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法启动「${r.name}」：${String(e)}`)
  }
}
</script>

<template>
  <section class="card recent-bar" :aria-label="title ?? '最近使用'">
    <header class="rb-header" :class="{ 'hd-float': hideTitle }">
      <h3 v-if="!hideTitle" class="rb-title">
        <Flame :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '最近使用' }}</span>
      </h3>
      <button
        class="rb-more"
        type="button"
        title="全部速达"
        aria-label="全部速达"
        @click="emit('goSuda')"
      >
        <ArrowRight :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <div v-if="recent.length" ref="bodyRef" class="rb-body">
      <div
        v-for="r in recent"
        :key="r.id"
        class="rb-card"
        role="button"
        tabindex="0"
        :title="`启动「${r.name}」`"
        @click="onOpen(r)"
        @keydown.enter="onOpen(r)"
        @keydown.space.prevent="onOpen(r)"
      >
        <span
          class="rb-icon"
          :style="showImageIcon(r) ? {} : { background: accentFor(r).soft }"
        >
          <img
            v-if="showImageIcon(r)"
            class="rb-img"
            :src="iconSrc(r.icon!)"
            alt=""
            @error="onIconError(r)"
          />
          <Globe
            v-else-if="showWebFallbackIcon(r)"
            :size="24"
            :stroke-width="1.7"
            :style="{ color: 'var(--c-green-ink)' }"
          />
          <component
            v-else-if="r.kind === 'file'"
            :is="fileIconOf(r)"
            :size="24"
            :stroke-width="1.7"
            :style="{ color: accentFor(r).strong }"
          />
          <span v-else :style="{ color: accentOf(r.name).text }">{{ iconText(r) }}</span>
        </span>
        <span class="rb-name">{{ r.name }}</span>
      </div>
    </div>

    <div v-else class="rb-empty">
      <p class="rb-empty-text">暂无最近使用</p>
      <button class="ghost-btn rb-empty-btn" type="button" @click="emit('goSuda')">
        去速达添加
      </button>
    </div>
  </section>
</template>

<style scoped>
.recent-bar {
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
}
.rb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
/* 关闭标题：表头整条不占位，动作按钮由全局 .hd-float 悬浮在卡片右上角（见 style.css） */
.rb-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.rb-title :deep(svg) {
  color: var(--brand-500);
}
.rb-more {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.18s, color 0.18s;
}
.rb-more:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}
.rb-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-wrap: wrap;
  align-content: flex-start;
  gap: 10px;
  /* 格子放不下时滚动兜底，而不是把换行后的图标静默裁掉 */
  overflow-y: auto;
  overscroll-behavior: contain;
  padding-bottom: 2px;
}
.rb-card {
  flex-shrink: 0;
  width: 72px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 8px 4px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.15s, transform 0.15s;
}
.rb-card:hover {
  background: var(--bg-card-soft);
  transform: translateY(-1px);
}
.rb-icon {
  width: 42px;
  height: 42px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 1.0625rem;
  font-weight: 700;
  flex-shrink: 0;
}
.rb-img {
  width: 42px;
  height: 42px;
  border-radius: 12px;
  object-fit: contain;
}
.rb-name {
  max-width: 100%;
  font-size: 0.6875rem;
  font-weight: 500;
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rb-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
}
.rb-empty-text {
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
.rb-empty-btn {
  padding: 6px 14px;
}
</style>
