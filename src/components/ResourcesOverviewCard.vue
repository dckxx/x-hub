<script setup lang="ts">
import { computed } from 'vue'
import { ArrowRight, FolderOpen } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ onOpenDetail?: () => void }>()

const store = useStore()

const resources = computed(() => store.state.resources)

const total = computed(() => resources.value.length)
const apps = computed(() => resources.value.filter((r) => r.kind === 'app').length)
const webs = computed(() => resources.value.filter((r) => r.kind === 'web').length)
const files = computed(() => resources.value.filter((r) => r.kind === 'file').length)

const stats = computed(() => [
  { label: '应用', value: apps.value },
  { label: '网页', value: webs.value },
  { label: '文件', value: files.value },
])
</script>

<template>
  <section class="card resources-overview" aria-label="速达数量">
    <header class="ro-header">
      <h3 class="ro-title">
        <FolderOpen :size="14" :stroke-width="2" aria-hidden="true" />
        <span>速达</span>
      </h3>
      <button
        class="ro-more"
        type="button"
        title="去速达"
        aria-label="去速达"
        @click="props.onOpenDetail?.()"
      >
        <ArrowRight :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <template v-if="total > 0">
      <div class="ro-total">
        <span class="ro-total-value">{{ total }}</span>
        <span class="ro-total-suffix">个资源</span>
      </div>
      <div class="ro-metrics">
        <div v-for="s in stats" :key="s.label" class="ro-metric">
          <span class="ro-metric-value">{{ s.value }}</span>
          <span class="ro-metric-label">{{ s.label }}</span>
        </div>
      </div>
      <div class="ro-split" aria-hidden="true">
        <div
          v-for="s in stats"
          :key="s.label"
          class="ro-split-seg"
          :style="{ flex: s.value || 0 }"
        ></div>
      </div>
    </template>

    <div v-else class="ro-empty">
      <p class="ro-empty-title">还没有速达资源</p>
      <p class="ro-empty-sub">添加应用 / 网页 / 文件后会在这里展示数量</p>
    </div>
  </section>
</template>

<style scoped>
.resources-overview {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
}
.ro-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.ro-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.ro-title :deep(svg) {
  color: var(--brand-500);
}
.ro-more {
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
.ro-more:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}
.ro-total {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-bottom: 10px;
}
.ro-total-value {
  font-size: 1.875rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.03em;
  color: var(--text-1);
  line-height: 1;
}
.ro-total-suffix {
  font-size: 0.75rem;
  color: var(--text-4);
}
.ro-split {
  display: flex;
  gap: 3px;
  height: 6px;
  margin-bottom: 12px;
  border-radius: var(--radius-pill);
  overflow: hidden;
}
.ro-split-seg {
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  opacity: 0.35;
}
.ro-split-seg:nth-child(2) { opacity: 0.7; }
.ro-split-seg:nth-child(3) { opacity: 1; }
.ro-metrics {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}
.ro-metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 8px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  min-width: 0;
}
.ro-metric-value {
  font-size: 1.125rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
}
.ro-metric-label {
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
}
.ro-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.ro-empty-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-2);
}
.ro-empty-sub {
  margin: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}
</style>
