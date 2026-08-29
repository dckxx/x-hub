<script setup lang="ts">
import { computed, inject, onBeforeUnmount, ref, watch } from 'vue'
import { ArrowRight, Flame, Globe } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Resource } from '../api/tauri'
import { iconSrc, accentOf, useResourceIcon } from '../composables/useResourceIcon'

const emit = defineEmits<{ (e: 'goSuda'): void }>()

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

// ---- 宽度自适应：卡片固定 72px、间距 10px，按容器实际宽度算能放下几张就显示几张 ----
const CHIP_W = 72
const GAP = 10
const bodyRef = ref<HTMLElement | null>(null)
const visibleCount = ref(10)
let resizeObserver: ResizeObserver | null = null

function recomputeVisible() {
  const w = bodyRef.value?.clientWidth ?? 0
  if (w > 0) visibleCount.value = Math.max(1, Math.floor((w + GAP) / (CHIP_W + GAP)))
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
  <section class="card recent-bar" aria-label="最近使用">
    <header class="rb-header">
      <h3 class="rb-title">
        <Flame :size="14" :stroke-width="2" aria-hidden="true" />
        <span>最近使用</span>
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
  gap: 10px;
  overflow-x: auto;
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
  background: var(--bg-card);
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
