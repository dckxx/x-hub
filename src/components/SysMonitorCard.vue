<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Component } from 'vue'
import { Activity, ArrowDown, ArrowUp, ArrowUpDown, Cpu, MemoryStick } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import { useAdaptivePolling } from '../composables/useAdaptivePolling'
import { resolveSysmonMetrics, sysmonMetricDisplay } from '../utils/sysmonMetrics'

// 标题可由工作台自定义布局覆盖：title = 自定义文案，hideTitle = 关闭标题行
defineProps<{ title?: string; hideTitle?: boolean }>()

const store = useStore()

// 卡片根元素：滚出工作台视口时暂停采样
const cardRef = ref<HTMLElement | null>(null)

// 自适应采样：可见且聚焦 2s、失焦 5s、隐藏/滚出视口停；从停止恢复立即补采。
// 隐藏即停的原因：收进托盘后 WebView2 若无节流（现由浏览器兜底钳到 ≥1s），
// 每 2s 的 IPC + sysinfo 采样在后台空烧。visibilitychange/焦点/视口由 composable 统一处理
useAdaptivePolling(() => store.refreshSystemInfo(), {
  activeMs: 2000,
  idleMs: 5000,
  viewport: cardRef,
})

const info = () => store.state.systemInfo

// 显示哪些指标、按什么顺序：配置解析唯一口径在 utils/sysmonMetrics（空配置回退内置默认），
// 入口在布局编辑器 sysmon 卡片格的「显示设置」按钮
const metrics = computed(() => resolveSysmonMetrics(store.state.config.sys_monitor_metrics))

const METRIC_ICONS: Record<string, Component> = {
  cpu: Cpu,
  mem: MemoryStick,
  net_total: Activity,
  cpu_threads: Cpu,
  traffic: ArrowUpDown,
}

const display = (id: string) => sysmonMetricDisplay(id, info())

const memLabel = () => {
  const i = info()
  if (!i) return '—'
  return `${(i.memUsedMb / 1024).toFixed(1)} / ${(i.memTotalMb / 1024).toFixed(1)} GB`
}
</script>

<template>
  <section ref="cardRef" class="card sys-monitor" :aria-label="title ?? '系统资源'">
    <header v-if="!hideTitle" class="sm-header">
      <h3 class="sm-title">
        <Cpu :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '系统资源' }}</span>
        <span class="sm-live-dot" aria-hidden="true"></span>
      </h3>
    </header>

    <div class="sm-body">
      <div v-for="m in metrics" :key="m.id" class="sm-item">
        <div class="sm-row">
          <!-- 网速行：上行/下行一对数值，无文字（目录 label「上传/下载」只出现在显示设置） -->
          <template v-if="m.kind === 'pair'">
            <span class="sm-pair-item">
              <ArrowUp :size="12" :stroke-width="2" aria-hidden="true" />
              <span class="sm-item-value sm-pair-value">{{ display('net_upload').text }}<em>{{ display('net_upload').unit }}</em></span>
            </span>
            <span class="sm-pair-item">
              <ArrowDown :size="12" :stroke-width="2" aria-hidden="true" />
              <span class="sm-item-value sm-pair-value">{{ display('net_download').text }}<em>{{ display('net_download').unit }}</em></span>
            </span>
          </template>
          <template v-else>
            <span class="sm-item-name">
              <component :is="METRIC_ICONS[m.id] ?? Cpu" :size="12" :stroke-width="2" aria-hidden="true" />
              {{ m.label }}
            </span>
            <div v-if="m.kind === 'bar'" class="sm-bar">
              <div
                class="sm-bar-fill"
                :class="{ warn: (display(m.id).pct ?? 0) >= 85 }"
                :style="{ transform: 'scaleX(' + (display(m.id).pct ?? 0) / 100 + ')' }"
              ></div>
            </div>
            <span class="sm-item-value">{{ display(m.id).text }}<em v-if="display(m.id).unit">{{ display(m.id).unit }}</em></span>
          </template>
        </div>
        <p v-if="m.id === 'mem'" class="sm-mem-label">{{ memLabel() }}</p>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 指标区自适应网格：格子窄时一行一个，宽卡自动一行 2~3 个（进度条随之变短）。
   进度条内嵌在指标行内（名称 · 条 · 数值同行）；网速行是一对 ↑↓ 数值无名称。
   默认 3 个指标组在最小卡（2×3 格 ≈140px）完整显示；
   高度实在不够时裁切——「这块不够大」的诚实信号，拉高格子即恢复 */
.sys-monitor {
  display: flex;
  flex-direction: column;
  padding: 12px;
}
.sm-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.sm-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.sm-title :deep(svg) {
  color: var(--brand-500);
}
.sm-live-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-pill);
  background: var(--c-green);
  animation: sm-blink 1.6s ease-in-out infinite;
  flex-shrink: 0;
}
@keyframes sm-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}
.sm-body {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 8px 12px;
  /* 纵向均匀分布：行距与首尾空隙全等，格子高时内容铺满卡片而非堆在顶部 */
  align-content: space-evenly;
  overflow: hidden;
  flex: 1;
  min-height: 0;
}
.sm-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.sm-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}
/* 网速行：上行/下行一对（图标 + 数值），无名称文字 */
.sm-pair-item {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  color: var(--text-3);
}
.sm-pair-value {
  font-size: 0.875rem;
}
.sm-item-name {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex: none;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
  white-space: nowrap;
}
.sm-item-value {
  flex: none;
  font-size: 0.9375rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
  line-height: 1;
  white-space: nowrap;
}
.sm-item-value em {
  font-style: normal;
  font-size: 0.625rem;
  font-weight: 500;
  color: var(--text-3);
  margin-left: 2px;
}
.sm-bar {
  flex: 1;
  min-width: 28px;
  height: 6px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.sm-bar-fill {
  height: 100%;
  width: 100%;
  border-radius: var(--radius-pill);
  background: linear-gradient(90deg, var(--brand-600), var(--brand-500));
  transform-origin: left center;
  transition: transform 0.5s ease-out;
}
.sm-bar-fill.warn {
  background: linear-gradient(90deg, var(--c-orange), var(--c-red));
}
.sm-mem-label {
  margin: 0;
  font-size: 0.625rem;
  line-height: 1.2;
  color: var(--text-3);
  text-align: right;
}
</style>
