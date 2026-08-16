<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { RefreshCw } from 'lucide-vue-next'
import type { UsageDaily, UsageProvider, UsageRecord } from '../api/tauri'
import { useStore } from '../stores/workbench'

const store = useStore()
const loading = ref(false)
const page = ref(0)
const pageSize = ref(12)
const detailSectionRef = ref<HTMLElement | null>(null)
const detail = computed(() => store.state.usageDetail)
const summary = computed(() => store.state.usageSummary)

function fmt(n: number | null | undefined): string {
  const v = n ?? 0
  if (v >= 1_000_000_000) return (v / 1_000_000_000).toFixed(1) + 'B'
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + 'M'
  if (v >= 1_000) return (v / 1_000).toFixed(1) + 'K'
  return String(v)
}

const today = computed(() => [
  { label: '今日输入', value: fmt(summary.value?.today_input), sub: '非缓存' },
  { label: '今日缓存', value: fmt(summary.value?.today_cache_input), sub: '缓存输入' },
  { label: '今日输出', value: fmt(summary.value?.today_output), sub: '生成' },
  { label: '今日调用次数', value: fmt(summary.value?.today_count), sub: '条' },
])

const daily = computed<UsageDaily[]>(() => [...(detail.value?.daily ?? [])])

function dailyHeight(d: UsageDaily): number {
  const total = d.input + d.cache_input + d.output
  const max = Math.max(1, ...daily.value.map((x) => x.input + x.cache_input + x.output))
  return Math.max(4, Math.round((total / max) * 100))
}

const maxProvider = computed(() => {
  const list = detail.value?.providers ?? []
  const max = Math.max(1, ...list.map((p) => p.input + p.cache_input + p.output))
  return max
})

function fmtTime(ms: number): string {
  const d = new Date(ms)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function modelLabel(r: UsageRecord): string {
  if (r.model) return r.model.length > 30 ? r.model.slice(0, 30) + '…' : r.model
  return r.provider ?? '未知'
}

async function load(offset = 0) {
  loading.value = true
  try {
    await store.loadUsageDetail(7, pageSize.value, offset)
  } finally {
    loading.value = false
  }
}

function computePageSize() {
  const section = detailSectionRef.value
  if (!section) return
  // 真实 DOM 测量：优先量出标题/表头/行/分页条的实际高度，避免估算误差留下空隙
  const title = section.querySelector<HTMLElement>('.uv-section-title')
  const thead = section.querySelector<HTMLElement>('.uv-table thead')
  const row = section.querySelector<HTMLElement>('.uv-table tbody tr')
  const pager = section.querySelector<HTMLElement>('.uv-pager')
  const titleH = title ? title.offsetHeight + 12 : 33 // + margin-bottom 12px
  const theadH = thead?.offsetHeight ?? 33
  const rowH = row?.offsetHeight ?? 31
  const pagerH = pager ? pager.offsetHeight + 12 : 0 // + margin-top 12px
  const paddingAndGap = 32 // section padding 上下 16px
  const usableHeight = section.clientHeight - titleH - theadH - pagerH - paddingAndGap
  const newSize = Math.max(5, Math.min(50, Math.floor(usableHeight / rowH)))
  if (newSize !== pageSize.value) {
    const oldSize = pageSize.value
    pageSize.value = newSize
    const currentOffset = page.value * oldSize
    const newPage = Math.floor(currentOffset / newSize)
    page.value = newPage
    load(newPage * newSize)
  }
}

let resizeObserver: ResizeObserver | null = null

// 数据加载完成后 DOM 已渲染真实行高，重新精测 pageSize（窗口未变化时仅此一处会重算）
watch(
  () => detail.value?.records.length ?? 0,
  () => {
    void nextTick(computePageSize)
  },
)

async function onRefresh() {
  loading.value = true
  try {
    const r = await store.refreshUsage()
    if (r.listening) await load()
  } finally {
    loading.value = false
  }
}

function nextPage() {
  if (!detail.value) return
  if ((page.value + 1) * pageSize.value < detail.value.total) {
    page.value += 1
    load(page.value * pageSize.value)
  }
}

function prevPage() {
  if (page.value > 0) {
    page.value -= 1
    load(page.value * pageSize.value)
  }
}

onMounted(() => {
  computePageSize()
  load()
  if (detailSectionRef.value) {
    resizeObserver = new ResizeObserver(() => computePageSize())
    resizeObserver.observe(detailSectionRef.value)
  }
})
</script>

<template>
  <div class="usage-view">
    <header class="uv-header">
      <h2 class="uv-title">AI 用量统计</h2>
      <button class="ghost-btn" type="button" :disabled="loading" @click="onRefresh">
        <RefreshCw :size="14" :stroke-width="2" aria-hidden="true" :class="{ spinning: loading }" />
        {{ loading ? '同步中…' : '刷新' }}
      </button>
    </header>

    <!-- 今日四数字卡 -->
    <div class="uv-cards">
      <div v-for="c in today" :key="c.label" class="uv-card">
        <span class="uv-card-value">{{ c.value }}</span>
        <span class="uv-card-label">{{ c.label }}<em>{{ c.sub }}</em></span>
      </div>
    </div>

    <div class="uv-main">
      <!-- 近 7 日趋势 -->
      <section class="uv-section uv-trend">
        <h3 class="uv-section-title">近 7 日趋势</h3>
        <div v-if="daily.length" class="uv-chart">
          <div v-for="d in daily" :key="d.date" class="uv-chart-col">
            <div class="uv-chart-bar-wrap">
              <div class="uv-chart-bar" :style="{ height: dailyHeight(d) + '%' }" :title="`${d.date} 输入 ${fmt(d.input)} · 缓存 ${fmt(d.cache_input)} · 输出 ${fmt(d.output)}`">
                <span class="uv-chart-val">{{ fmt(d.input + d.cache_input + d.output) }}</span>
              </div>
            </div>
            <span class="uv-chart-date">{{ d.date.slice(5) }}</span>
          </div>
        </div>
        <p v-else class="uv-empty">暂无数据</p>
      </section>

      <!-- Provider 排行 -->
      <section class="uv-section uv-providers-sec">
        <h3 class="uv-section-title">Provider 排行</h3>
        <div v-if="(detail?.providers ?? []).length" class="uv-providers">
          <div v-for="p in (detail?.providers ?? []) as UsageProvider[]" :key="p.provider" class="uv-provider">
            <div class="uv-provider-top">
              <span class="uv-provider-name">{{ p.provider }}</span>
              <span class="uv-provider-nums">
                {{ fmt(p.input + p.cache_input) }} 输入 · {{ fmt(p.output) }} 输出
              </span>
            </div>
            <div class="uv-provider-bar">
              <div class="uv-provider-fill" :style="{ width: ((p.input + p.cache_input + p.output) / maxProvider) * 100 + '%' }"></div>
            </div>
          </div>
        </div>
        <p v-else class="uv-empty">暂无数据</p>
      </section>

      <!-- 明细列表 -->
      <section ref="detailSectionRef" class="uv-section uv-detail">
        <h3 class="uv-section-title">明细（共 {{ detail?.total ?? 0 }} 条）</h3>
        <div v-if="(detail?.records ?? []).length" class="uv-table-wrap">
        <table class="uv-table">
          <thead>
            <tr>
              <th>时间</th>
              <th>模型</th>
              <th>Provider</th>
              <th class="num">输入</th>
              <th class="num">缓存</th>
              <th class="num">输出</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in (detail?.records ?? []) as UsageRecord[]" :key="`${r.time_created}-${r.session_id}`">
              <td>{{ fmtTime(r.time_created) }}</td>
              <td class="model">{{ modelLabel(r) }}</td>
              <td>{{ r.provider ?? '未知' }}</td>
              <td class="num">{{ fmt(r.tokens_input) }}</td>
              <td class="num">{{ fmt(r.tokens_cache_read) }}</td>
              <td class="num">{{ fmt(r.tokens_output) }}</td>
            </tr>
          </tbody>
        </table>
        <div class="uv-pager">
          <button class="ghost-btn" type="button" :disabled="page === 0" @click="prevPage">上一页</button>
          <span class="uv-pager-info">第 {{ page + 1 }} 页</span>
          <button
            class="ghost-btn"
            type="button"
            :disabled="(page + 1) * pageSize >= (detail?.total ?? 0)"
            @click="nextPage"
          >
            下一页
          </button>
        </div>
      </div>
        <p v-else class="uv-empty">暂无数据</p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.usage-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-5);
  overflow: hidden;
}
.uv-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.uv-title {
  flex: 1;
  margin: 0;
  font-size: 18px;
  font-weight: 700;
  color: var(--text-1);
}
.spinning {
  animation: uv-spin 1s linear infinite;
}
@keyframes uv-spin {
  to { transform: rotate(360deg); }
}
.uv-cards {
  flex-shrink: 0;
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-3);
}
.uv-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-4);
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  box-shadow: var(--frost-edge), var(--shadow-card);
}
.uv-card-value {
  font-size: 24px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
  color: var(--text-1);
}
.uv-card-label {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: 12px;
  color: var(--text-3);
}
.uv-card-label em {
  font-style: normal;
  font-size: 11px;
  color: var(--text-4);
}
.uv-main {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr);
  grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
  gap: var(--space-4);
}
.uv-trend {
  grid-column: 1;
  grid-row: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.uv-trend .uv-chart {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  gap: 10px;
  padding-top: 8px;
}
.uv-providers-sec {
  grid-column: 1;
  grid-row: 2;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.uv-providers-sec .uv-providers {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}
.uv-detail {
  grid-column: 2;
  grid-row: 1 / 3;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.uv-detail .uv-table-wrap {
  flex: 1;
  min-height: 0;
  overflow-y: hidden;
  overflow-x: auto;
}
.uv-section {
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--frost-edge), var(--shadow-card);
  padding: var(--space-4);
}
.uv-section-title {
  margin: 0 0 var(--space-3);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-2);
}
.uv-chart {
  display: flex;
  align-items: flex-end;
  gap: 10px;
}
.uv-chart-col {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.uv-chart-bar-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  width: 100%;
}
.uv-chart-bar {
  width: 100%;
  max-width: 34px;
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  background: linear-gradient(180deg, var(--brand-500), var(--brand-600));
  display: flex;
  align-items: flex-start;
  justify-content: center;
  transition: height 0.3s;
  min-height: 3px;
}
.uv-chart-bar:has(.uv-chart-val:empty) {
  min-height: 4px;
}
.uv-chart-val {
  font-size: 10px;
  color: var(--text-on-accent);
  padding: 4px 2px 2px;
  white-space: nowrap;
  transform: scale(0.9);
}
.uv-chart-date {
  font-size: 10px;
  color: var(--text-4);
}
.uv-providers {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.uv-provider-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 5px;
}
.uv-provider-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.uv-provider-nums {
  font-size: 11px;
  color: var(--text-3);
}
.uv-provider-bar {
  height: 6px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.uv-provider-fill {
  height: 100%;
  border-radius: var(--radius-pill);
  background: linear-gradient(90deg, var(--brand-500), var(--c-green));
  transition: width 0.3s;
}
.uv-table-wrap {
  overflow-x: auto;
}
.uv-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
.uv-table th {
  text-align: left;
  padding: 6px 10px;
  color: var(--text-4);
  font-weight: 600;
  border-bottom: 1px solid var(--border-soft);
  white-space: nowrap;
}
.uv-table td {
  padding: 6px 10px;
  color: var(--text-2);
  border-bottom: 1px solid var(--border-soft);
  white-space: nowrap;
}
.uv-table td.model {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.uv-table .num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.uv-pager {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: var(--space-3);
  justify-content: center;
}
.uv-pager-info {
  font-size: 12px;
  color: var(--text-3);
}
.uv-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  color: var(--text-4);
  font-size: 13px;
  padding: 16px;
}
</style>
