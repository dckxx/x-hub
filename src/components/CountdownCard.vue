<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import type { Component } from 'vue'
import { AlarmClock, CalendarDays, CheckCircle2, ChevronDown, ChevronLeft, ChevronRight, ChevronUp, Clock, Pause, PanelTopClose, Play, Plus, Repeat, Timer, Trash2, X } from 'lucide-vue-next'
import {
  TimeFieldRoot,
  TimeFieldInput,
  NumberFieldRoot,
  NumberFieldInput,
  NumberFieldIncrement,
  NumberFieldDecrement,
  DatePickerRoot,
  DatePickerField,
  DatePickerInput,
  DatePickerTrigger,
  DatePickerContent,
  DatePickerCalendar,
  DatePickerHeader,
  DatePickerPrev,
  DatePickerHeading,
  DatePickerNext,
  DatePickerGrid,
  DatePickerGridHead,
  DatePickerGridBody,
  DatePickerGridRow,
  DatePickerHeadCell,
  DatePickerCell,
  DatePickerCellTrigger,
} from 'reka-ui'
import { Time } from '@internationalized/date'
import type { DateValue, TimeValue } from 'reka-ui'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import type { Countdown } from '../api/tauri'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

// 倒计时卡片尺寸（网格单元数 w×h），由工作台布局传入；默认 5×4（推荐布局）
const props = defineProps<{ sizeW?: number; sizeH?: number }>()

// 上限：默认 6 个；卡片为 5×4 时最多 4 个（推荐布局的倒计时卡是 5 列 4 行，仅容得下 2×2）
const MAX_COUNTDOWNS = computed(() =>
  props.sizeW === 5 && props.sizeH === 4 ? 4 : 6,
)

const atLimit = computed(
  () => store.state.countdowns.length >= MAX_COUNTDOWNS.value,
)

// ---- 新建弹窗 ----
const creating = ref(false)
const createCardRef = ref<HTMLElement | null>(null)
useFocusTrap(creating, createCardRef)
const form = ref<{
  name: string
  mode: 'duration' | 'schedule' | 'daily' | 'interval'
  minutes: number
  intervalMinutes: number
}>({
  name: '',
  mode: 'duration',
  minutes: 25,
  intervalMinutes: 60,
})
// 日期/时间字段单独用 shallowRef 持有：DateValue/TimeValue 是含 #private 的名义 class，
// 若放进 ref 的深度解包对象会被 UnwrapRef 拆成结构类型，导致与 Reka UI 组件类型不匹配。
const scheduleDate = shallowRef<DateValue | null>(null)
const scheduleTime = shallowRef<TimeValue | null>(null)
const dailyTime = shallowRef<TimeValue>(new Time(15, 0))
const formError = ref('')
const nameInputRef = ref<HTMLInputElement | null>(null)

const activeCountdowns = computed(() =>
  store.state.countdowns
    .filter((c) => !c.finished)
    .sort((a, b) => a.end_at - b.end_at),
)
const finishedCountdowns = computed(() =>
  store.state.countdowns.filter((c) => c.finished),
)

// ---- 每秒刷新剩余时间 ----
const tick = ref(0)
let timer: ReturnType<typeof setInterval> | null = null
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && creating.value) creating.value = false
}
onMounted(() => {
  timer = setInterval(() => {
    tick.value++
  }, 1000)
  window.addEventListener('keydown', onKeydown)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  window.removeEventListener('keydown', onKeydown)
})

function fmt(n: number): string {
  return String(n).padStart(2, '0')
}

/** 剩余毫秒（暂停时用冻结值） */
function remainingMs(c: Countdown): number {
  void tick.value
  if (c.paused) return c.paused_remaining_ms ?? 0
  return Math.max(c.end_at - Date.now(), 0)
}

function remainingLabel(c: Countdown): string {
  const ms = remainingMs(c)
  const totalSec = Math.floor(ms / 1000)
  const d = Math.floor(totalSec / 86400)
  const h = Math.floor((totalSec % 86400) / 3600)
  const m = Math.floor((totalSec % 3600) / 60)
  const s = totalSec % 60
  if (d > 0) return `${d}天 ${fmt(h)}:${fmt(m)}`
  if (h > 0) return `${h}:${fmt(m)}:${fmt(s)}`
  return `${m}:${fmt(s)}`
}

/** 下次到点时刻（daily/interval 显示日期，once 显示到点时间） */
function dueLabel(c: Countdown): string {
  const d = new Date(c.end_at)
  const now = new Date()
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  const hm = `${fmt(d.getHours())}:${fmt(d.getMinutes())}`
  if (c.repeat_mode === 'once') return sameDay ? `今天 ${hm}` : `${d.getMonth() + 1}/${d.getDate()} ${hm}`
  return c.repeat_mode === 'daily' ? `每天 ${hm}` : `每 ${c.interval_minutes ?? 1} 分钟`
}

const MODE_LABEL: Record<string, string> = {
  once: '一次性',
  daily: '每天',
  interval: '间隔',
}

/** 列表条目前缀图标：按类型区分（一次性 / 每天 / 间隔） */
const MODE_ICON: Record<string, Component> = {
  once: Timer,
  daily: AlarmClock,
  interval: Repeat,
}

function toggleCreate() {
  if (creating.value) {
    creating.value = false
    return
  }
  if (atLimit.value) {
    showToast(`最多创建 ${MAX_COUNTDOWNS.value} 个倒计时，请先删除已结束的`)
    return
  }
  creating.value = true
  formError.value = ''
  form.value.name = ''
  void nextTick(() => nameInputRef.value?.focus())
}

/** 计算下一次 end_at（毫秒） */
function computeEndAt(): { endAt: number; totalMs: number; intervalMinutes: number | null } {
  const m = form.value
  if (m.mode === 'duration') {
    const endAt = Date.now() + m.minutes * 60_000
    return { endAt, totalMs: m.minutes * 60_000, intervalMinutes: null }
  }
  if (m.mode === 'schedule') {
    const cd = scheduleDate.value
    const t = scheduleTime.value
    if (!cd || !t) {
      return { endAt: Date.now(), totalMs: 0, intervalMinutes: null }
    }
    // @internationalized/date 月份是 1-indexed，需转为 JS 的 0-indexed
    const dt = new Date(cd.year, cd.month - 1, cd.day, t.hour, t.minute, 0, 0)
    const endAt = dt.getTime()
    return { endAt, totalMs: Math.max(endAt - Date.now(), 1000), intervalMinutes: null }
  }
  if (m.mode === 'daily') {
    const t = dailyTime.value
    const now = new Date()
    const target = new Date(now.getFullYear(), now.getMonth(), now.getDate(), t.hour, t.minute, 0, 0)
    let endAt = target.getTime()
    if (endAt <= Date.now()) endAt += 24 * 60 * 60 * 1000
    return { endAt, totalMs: 24 * 60 * 60 * 1000, intervalMinutes: null }
  }
  // interval
  const intervalMinutes = Math.max(m.intervalMinutes, 1)
  return { endAt: Date.now() + intervalMinutes * 60_000, totalMs: intervalMinutes * 60_000, intervalMinutes }
}

const repeatModeFor = computed(() => {
  switch (form.value.mode) {
    case 'schedule':
    case 'duration':
      return 'once'
    case 'daily':
      return 'daily'
    default:
      return 'interval'
  }
})

async function onCreate() {
  if (atLimit.value) {
    showToast(`最多创建 ${MAX_COUNTDOWNS.value} 个倒计时，请先删除已结束的`)
    return
  }
  const name = form.value.name.trim()
  if (!name) {
    formError.value = '请输入名称'
    return
  }
  if (form.value.mode === 'schedule' && (!scheduleDate.value || !scheduleTime.value)) {
    formError.value = '请选择日期和时间'
    return
  }
  const { endAt, totalMs, intervalMinutes } = computeEndAt()
  if (form.value.mode === 'schedule' && endAt <= Date.now()) {
    formError.value = '定时时间必须在未来'
    return
  }
  try {
    await store.addCountdown({
      name,
      repeatMode: repeatModeFor.value,
      endAt,
      totalMs,
      intervalMinutes,
    })
    formError.value = ''
    form.value.name = ''
    creating.value = false
  } catch (e) {
    formError.value = String(e)
  }
}

async function onDelete(c: Countdown) {
  await store.removeCountdown(c.id)
}

async function onTogglePause(c: Countdown) {
  await store.toggleCountdownPause(c.id)
}

async function onToggleFloat(c: Countdown) {
  if (c.floated) await store.unfloatCountdown(c.id)
  else await store.floatCountdown(c.id)
}
</script>

<template>
  <section class="card countdown-card" aria-label="倒计时">
    <header class="cc-header">
      <h3 class="cc-title">
        <Timer :size="14" :stroke-width="2" aria-hidden="true" />
        <span>倒计时</span>
      </h3>
      <button
        class="cc-add"
        type="button"
        :disabled="atLimit"
        :title="atLimit ? `最多 ${MAX_COUNTDOWNS} 个倒计时` : '新建倒计时'"
        :aria-label="'新建倒计时'"
        @click="toggleCreate"
      >
        <Plus :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <!-- 进行中列表 -->
    <div v-if="activeCountdowns.length > 0" class="cc-list">
      <div
        v-for="c in activeCountdowns"
        :key="c.id"
        class="cc-item"
        :class="{ paused: c.paused }"
      >
        <div class="cc-mode-icon" :class="c.repeat_mode" :title="MODE_LABEL[c.repeat_mode]">
          <component :is="MODE_ICON[c.repeat_mode] || Timer" :size="15" :stroke-width="2" aria-hidden="true" />
        </div>
        <div class="cc-main">
          <div class="cc-item-top">
            <span class="cc-item-name" :title="c.name">{{ c.name }}</span>
            <span class="cc-badge" :class="c.repeat_mode">{{ MODE_LABEL[c.repeat_mode] }}</span>
          </div>
          <div class="cc-item-meta">
            <span class="cc-remaining" :title="remainingLabel(c)">{{ remainingLabel(c) }}</span>
            <span class="cc-due" :title="dueLabel(c)">{{ dueLabel(c) }}</span>
          </div>
        </div>
        <div class="cc-actions">
          <button
            class="cc-btn"
            :class="{ active: c.paused }"
            :title="c.paused ? '恢复' : '暂停'"
            type="button"
            @click="onTogglePause(c)"
          >
            <Play v-if="c.paused" :size="13" :stroke-width="2" />
            <Pause v-else :size="13" :stroke-width="2" />
          </button>
          <button
            class="cc-btn cc-float"
            :class="{ active: c.floated }"
            :title="c.floated ? '收起浮窗' : '浮窗显示'"
            type="button"
            @click="onToggleFloat(c)"
          >
            <PanelTopClose :size="13" :stroke-width="2" />
          </button>
          <button class="cc-btn cc-del" title="删除" type="button" @click="onDelete(c)">
            <Trash2 :size="13" :stroke-width="2" />
          </button>
        </div>
      </div>
    </div>

    <!-- 已结束灰态 -->
    <div v-if="finishedCountdowns.length > 0" class="cc-finished">
      <div v-for="c in finishedCountdowns" :key="c.id" class="cc-item finished">
        <div class="cc-mode-icon finished" title="已结束">
          <CheckCircle2 :size="15" :stroke-width="2" aria-hidden="true" />
        </div>
        <div class="cc-main">
          <div class="cc-item-top">
            <span class="cc-item-name muted" :title="c.name">{{ c.name }}</span>
            <span class="cc-badge once">已结束</span>
          </div>
          <div class="cc-item-meta">
            <span class="cc-remaining muted">00:00</span>
          </div>
        </div>
        <div class="cc-actions">
          <button class="cc-btn cc-del" title="删除" type="button" @click="onDelete(c)">
            <Trash2 :size="13" :stroke-width="2" />
          </button>
        </div>
      </div>
    </div>

    <div v-if="activeCountdowns.length === 0 && finishedCountdowns.length === 0 && !creating" class="cc-empty">
      <Clock :size="16" :stroke-width="2" aria-hidden="true" />
      <p>还没有倒计时</p>
      <p class="cc-empty-sub">点「新建」添加一个，支持时长 / 定时 / 每天 / 间隔，最多 {{ MAX_COUNTDOWNS }} 个</p>
    </div>

    <!-- 新建倒计时弹窗（Teleport 必须在 section 内部，保证组件是单根节点，
         否则 class/grid 定位无法从父组件继承，.dash-usage.countdown-card 样式失效） -->
    <Teleport to="body">
    <Transition name="mask">
      <div v-if="creating" class="modal-mask" @mousedown.self="creating = false">
        <div ref="createCardRef" class="modal-card cc-dialog" role="dialog" aria-label="新建倒计时" aria-modal="true">
          <div class="cc-dialog-head">
            <h3 class="cc-dialog-title">新建倒计时</h3>
            <button class="icon-btn" type="button" title="关闭" @click="creating = false">
              <X :size="14" :stroke-width="2" />
            </button>
          </div>

          <input
            ref="nameInputRef"
            v-model="form.name"
            class="cc-input cc-name"
            type="text"
            placeholder="名称（如 喝水 / 下班 / 番茄钟）"
            spellcheck="false"
            @keydown.enter="onCreate"
          />
          <div class="cc-mode-tabs" role="tablist" aria-label="倒计时类型">
            <button
              v-for="opt in [
                { v: 'duration', l: '时长' },
                { v: 'schedule', l: '定时' },
                { v: 'daily', l: '每天' },
                { v: 'interval', l: '间隔' },
              ] as const"
              :key="opt.v"
              class="cc-mode-tab"
              :class="{ active: form.mode === opt.v }"
              type="button"
              role="tab"
              :aria-selected="form.mode === opt.v"
              @click="form.mode = opt.v"
            >
              {{ opt.l }}
            </button>
          </div>

          <div class="cc-fields">
            <template v-if="form.mode === 'duration'">
              <label class="cc-field">
                <span class="cc-field-label">时长（分钟）</span>
                <NumberFieldRoot
                  v-model="form.minutes"
                  :min="1"
                  :max="1440"
                  :step="5"
                  :step-snapping="false"
                  :format-options="{ style: 'decimal', maximumFractionDigits: 0 }"
                  disable-wheel-change
                  class="cc-number-field"
                >
                  <NumberFieldDecrement class="cc-num-btn">
                    <ChevronDown :size="12" :stroke-width="2" />
                  </NumberFieldDecrement>
                  <NumberFieldInput class="cc-input cc-num" />
                  <NumberFieldIncrement class="cc-num-btn">
                    <ChevronUp :size="12" :stroke-width="2" />
                  </NumberFieldIncrement>
                </NumberFieldRoot>
              </label>
            </template>
            <template v-else-if="form.mode === 'schedule'">
              <div class="cc-field">
                <span class="cc-field-label">日期</span>
                <DatePickerRoot
                  v-model="scheduleDate"
                  locale="zh-CN"
                  granularity="day"
                  class="cc-date-picker"
                >
                  <DatePickerField class="cc-date-picker-field cc-input" v-slot="{ segments }">
                    <template v-for="segment in segments" :key="segment.part">
                      <span
                        v-if="segment.part === 'literal'"
                        class="cc-picker-segment-literal"
                      >{{ segment.value }}</span>
                      <DatePickerInput
                        v-else
                        :part="segment.part"
                        class="cc-picker-segment"
                      >{{ segment.value }}</DatePickerInput>
                    </template>
                    <DatePickerTrigger class="cc-date-picker-trigger" title="打开日历">
                      <CalendarDays :size="14" :stroke-width="2" />
                    </DatePickerTrigger>
                  </DatePickerField>
                  <DatePickerContent class="cc-calendar-content" :side-offset="4">
                    <DatePickerCalendar
                      v-slot="{ weekDays, grid }"
                      class="cc-calendar"
                    >
                      <DatePickerHeader class="cc-calendar-header">
                        <DatePickerPrev class="cc-calendar-nav">
                          <ChevronLeft :size="16" :stroke-width="2" />
                        </DatePickerPrev>
                        <DatePickerHeading class="cc-calendar-heading" />
                        <DatePickerNext class="cc-calendar-nav">
                          <ChevronRight :size="16" :stroke-width="2" />
                        </DatePickerNext>
                      </DatePickerHeader>
                      <DatePickerGrid class="cc-calendar-grid">
                        <DatePickerGridHead>
                          <DatePickerGridRow class="cc-calendar-row">
                            <DatePickerHeadCell
                              v-for="day in weekDays"
                              :key="day"
                              class="cc-calendar-weekday"
                            >
                              {{ day }}
                            </DatePickerHeadCell>
                          </DatePickerGridRow>
                        </DatePickerGridHead>
                        <DatePickerGridBody>
                          <DatePickerGridRow
                            v-for="(monthGrid, monthIndex) in grid"
                            :key="monthIndex"
                            class="cc-calendar-row"
                          >
                            <DatePickerCell
                              v-for="cell in monthGrid.cells"
                              :key="cell.toString()"
                              :date="cell"
                              class="cc-calendar-cell"
                            >
                              <DatePickerCellTrigger
                                :day="cell"
                                :month="cell"
                                class="cc-calendar-cell-trigger"
                              />
                            </DatePickerCell>
                          </DatePickerGridRow>
                        </DatePickerGridBody>
                      </DatePickerGrid>
                    </DatePickerCalendar>
                  </DatePickerContent>
                </DatePickerRoot>
              </div>
              <div class="cc-field">
                <span class="cc-field-label">时间</span>
                <TimeFieldRoot
                  v-model="scheduleTime"
                  locale="zh-CN"
                  :hour-cycle="24"
                  granularity="minute"
                  class="cc-time-field cc-input cc-time-input"
                >
                  <template #default="{ segments }">
                    <TimeFieldInput
                      v-for="segment in segments"
                      :key="segment.part"
                      :part="segment.part"
                      class="cc-segment"
                    >
                      {{ segment.value }}
                    </TimeFieldInput>
                  </template>
                </TimeFieldRoot>
              </div>
            </template>
            <template v-else-if="form.mode === 'daily'">
              <div class="cc-field">
                <span class="cc-field-label">每天时刻</span>
                <TimeFieldRoot
                  v-model="dailyTime"
                  locale="zh-CN"
                  :hour-cycle="24"
                  granularity="minute"
                  class="cc-time-field cc-input cc-time-input"
                >
                  <template #default="{ segments }">
                    <TimeFieldInput
                      v-for="segment in segments"
                      :key="segment.part"
                      :part="segment.part"
                      class="cc-segment"
                    >
                      {{ segment.value }}
                    </TimeFieldInput>
                  </template>
                </TimeFieldRoot>
              </div>
            </template>
            <template v-else>
              <label class="cc-field">
                <span class="cc-field-label">每 N 分钟</span>
                <NumberFieldRoot
                  v-model="form.intervalMinutes"
                  :min="1"
                  :max="1440"
                  :step="5"
                  :step-snapping="false"
                  :format-options="{ style: 'decimal', maximumFractionDigits: 0 }"
                  disable-wheel-change
                  class="cc-number-field"
                >
                  <NumberFieldDecrement class="cc-num-btn">
                    <ChevronDown :size="12" :stroke-width="2" />
                  </NumberFieldDecrement>
                  <NumberFieldInput class="cc-input cc-num" />
                  <NumberFieldIncrement class="cc-num-btn">
                    <ChevronUp :size="12" :stroke-width="2" />
                  </NumberFieldIncrement>
                </NumberFieldRoot>
              </label>
            </template>
          </div>

          <p v-if="formError" class="cc-error">{{ formError }}</p>
          <div class="cc-dialog-actions">
            <button class="ghost-btn" type="button" @click="creating = false">取消</button>
            <button class="cc-submit" type="button" @click="onCreate">创建</button>
          </div>
        </div>
      </div>
    </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.countdown-card {
  /* 最小高度 = header 36 + 3 行列表(每行 ~56px×3 + gap 8×2=16) + padding 12×2 ≈ 244px，
     内容不足时保持 248px 稳定布局；存在已结束条目时按需增高，避免行高被压缩导致内容裁切 */
  min-height: 248px;
  display: flex;
  flex-direction: column;
  padding: 12px;
  overflow: hidden;
}
.cc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}
.cc-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.cc-title :deep(svg) {
  color: var(--brand-500);
}
.cc-add {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  transition: color 0.18s, background 0.18s;
  padding: 0;
}
.cc-add:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.cc-add:disabled {
  color: var(--text-4);
  cursor: not-allowed;
}

/* 新建弹窗 */
.cc-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.cc-dialog-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}
.cc-dialog-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
}
.cc-dialog .cc-input {
  box-sizing: border-box;
}
.cc-dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.cc-dialog .cc-submit {
  align-self: auto;
}
.cc-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  padding: 7px 10px;
  outline: none;
}
.cc-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.cc-num {
  max-width: 90px;
}
.cc-mode-tabs {
  display: flex;
  gap: 4px;
}
.cc-mode-tab {
  flex: 1;
  border: 1px solid var(--border-soft);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  padding: 5px 0;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.cc-mode-tab:hover {
  color: var(--brand-500);
}
.cc-mode-tab.active {
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: transparent;
  font-weight: 600;
}
.cc-fields {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.cc-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
}
.cc-field-label {
  font-size: 0.6875rem;
  color: var(--text-4);
}
.cc-error {
  margin: 0;
  font-size: 0.75rem;
  color: var(--c-red);
}
.cc-submit {
  align-self: flex-end;
  border: none;
  background: var(--brand-500);
  color: var(--text-on-accent);
  font-size: 0.75rem;
  font-weight: 600;
  padding: 6px 16px;
  border-radius: var(--radius-pill);
  cursor: pointer;
}
.cc-submit:hover {
  background: var(--brand-600);
}

/* 列表：两列自适应行数（上限 6 个 = 3 行），每行最小 48px 保证条目内容完整（icon 32 + padding 8×2），
   行数随条目数量变化，不存在空行占位；不因已结束区块而把行高压扁导致内容裁切。
   列表高度随内容自适应（不撑满剩余空间），使紧邻的已结束区块跟随在下方，
   多余高度留在卡片底部，避免「进行中」与「已结束」两行之间被拉开一大段空白。 */
.cc-list {
  flex: 0 0 auto;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-auto-rows: auto;
  gap: 8px;
  align-content: start;
}
.cc-item {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 48px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  /* 与卡片底色拉开层级：轻阴影，避免白底条目融进白底卡片看不清 */
  box-shadow: var(--shadow-item);
  transition: opacity 0.15s;
  position: relative;
}
.cc-item.paused {
  opacity: 0.62;
}
.cc-mode-icon {
  flex-shrink: 0;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  /* 同 .cc-badge：底色以实心白基混入，防壁纸透底压暗 */
  background: color-mix(in srgb, var(--accent) 14%, var(--bg-card-solid));
  /* 图标用比背景深一档的强调色（brand-600），避免与同色相背景融成一片 */
  color: var(--brand-600);
  border: 1px solid var(--border-soft);
}
.cc-mode-icon.interval {
  background: color-mix(in srgb, var(--c-green) 14%, var(--bg-card-solid));
  color: var(--c-green-ink);
}
.cc-mode-icon.finished {
  background: var(--bg-card-solid);
  color: var(--text-4);
}
.cc-main {
  flex: 1;
  min-width: 0;
}
.cc-item-top {
  display: flex;
  align-items: center;
  gap: 6px;
}
.cc-item-name {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cc-item-name.muted {
  color: var(--text-3);
}
.cc-badge {
  flex-shrink: 0;
  font-size: 0.625rem;
  line-height: 1;
  padding: 2px 7px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-solid);
  color: var(--text-3);
  border: 1px solid var(--border-soft);
}
/* 底色以 --bg-card-solid（亮色近实心白）为基色混入色相，而非 transparent 色洗：
   壁纸/低玻璃透明度下半透明底会被透出的壁纸压暗，深色文字便看不清 */
.cc-badge.daily {
  background: color-mix(in srgb, var(--accent) 14%, var(--bg-card-solid));
  color: var(--brand-600);
  border-color: transparent;
}
.cc-badge.interval {
  background: color-mix(in srgb, var(--c-green) 14%, var(--bg-card-solid));
  color: var(--c-green-ink);
  border-color: transparent;
}
.cc-item-meta {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-top: 2px;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
}
.cc-remaining {
  font-size: 0.8125rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
  flex-shrink: 0;
}
.cc-remaining.muted {
  color: var(--text-4);
}
.cc-due {
  font-size: 0.6875rem;
  color: var(--text-4);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 操作按钮：默认隐藏（让左侧名称/剩余时间完整展示），悬停条目时浮现并盖在内容右侧；
   绝对定位不占布局宽度，故隐藏后内容可用空间更大 */
.cc-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  padding: 2px;
  border-radius: 8px;
  background: var(--bg-card-solid);
  box-shadow: var(--shadow-sm, 0 1px 3px rgba(0, 0, 0, 0.12));
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.15s;
}
.cc-item:hover .cc-actions,
.cc-item:focus-within .cc-actions {
  opacity: 1;
  pointer-events: auto;
}
.cc-btn {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 6px;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.cc-btn:hover {
  background: var(--bg-card-solid);
  color: var(--text-1);
}
.cc-btn.active {
  color: var(--brand-500);
}
/* 浮窗按钮与便签脱离按钮一致：品牌色 hover / 激活填充 */
.cc-float {
  width: 26px;
  height: 26px;
}
.cc-float:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.cc-float.active {
  color: var(--brand-500);
  background: var(--brand-50);
}
.cc-float.active svg {
  stroke: var(--brand-500);
  fill: color-mix(in srgb, var(--brand-500) 18%, transparent);
}
.cc-btn.cc-del:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}

/* 已结束灰态：底部区块，与上方列表行间距保持一致（8px）
   （若与 .cc-list 同为 flex:1，二者各占一半 → 主列表行高减半、条目挤压溢出） */
.cc-finished {
  flex: 0 0 auto;
  margin-top: 8px;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-auto-rows: minmax(48px, auto);
  gap: 8px;
}

.cc-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  color: var(--text-4);
  min-height: 0;
}
.cc-empty p {
  margin: 0;
  font-size: 0.75rem;
}
.cc-empty-sub {
  font-size: 0.6875rem !important;
}

/* Reka UI NumberField 样式 */
.cc-number-field {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.cc-num-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  padding: 0;
}
.cc-num-btn:hover {
  background: var(--bg-card-solid);
  color: var(--text-1);
}
.cc-num-btn:active {
  color: var(--brand-500);
}
.cc-num-btn[data-disabled] {
  color: var(--text-4);
  cursor: not-allowed;
}

/* Reka UI DateField / TimeField 样式 */
.cc-date-field,
.cc-time-field {
  display: block;
  width: 100%;
}
.cc-date-input,
.cc-time-input {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: 32px;
}

/* DateField / TimeField segment 样式（通过 :deep 穿透） */
:deep([data-reka-date-field-segment]),
:deep([data-reka-time-field-segment]) {
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
  outline: none;
  border-radius: 3px;
  padding: 0 1px;
}
:deep([data-reka-date-field-segment]:focus),
:deep([data-reka-time-field-segment]:focus) {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
:deep([data-reka-date-field-segment][data-reka-date-field-segment='literal']),
:deep([data-reka-time-field-segment][data-reka-time-field-segment='literal']) {
  color: var(--text-4);
}
:deep([data-reka-date-field-segment][data-placeholder]),
:deep([data-reka-time-field-segment][data-placeholder]) {
  color: var(--text-4);
}

/* DatePicker 样式 */
.cc-date-picker {
  display: block;
  width: 100%;
}
.cc-date-picker-field {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 8px;
  min-height: 32px;
}
.cc-date-picker-input {
  display: flex;
  align-items: center;
  gap: 2px;
  flex: 1;
}
.cc-picker-segment {
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
  outline: none;
  border-radius: 3px;
  padding: 0 1px;
}
.cc-picker-segment-literal {
  color: var(--text-4);
  padding: 0 2px;
}
.cc-date-picker-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
  padding: 0;
  flex-shrink: 0;
}
.cc-date-picker-trigger:hover {
  color: var(--brand-500);
  background: var(--bg-card-soft);
}

/* DatePicker 日历弹出层样式（瞬态层，允许 backdrop-filter）
   注意：DatePickerContent 由 reka-ui 经 Portal 渲染到 <body>，其容器元素
   不会继承本组件的 scoped data-v 属性（日历内部元素会），scoped 规则会全部
   失效（含 z-index），导致日历被 modal-mask(z-index:100) 盖住 → 必须用 :global() */
:global(.cc-calendar-content) {
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--frost-edge), var(--shadow-dock);
  padding: 12px;
  z-index: 110;
  min-width: 260px;
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
}
.cc-calendar {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.cc-calendar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.cc-calendar-nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
  padding: 0;
}
.cc-calendar-nav:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.cc-calendar-nav[data-disabled] {
  color: var(--text-4);
  cursor: not-allowed;
  opacity: 0.4;
}
.cc-calendar-heading {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  text-align: center;
  flex: 1;
}
.cc-calendar-grid {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.cc-calendar-row {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 2px;
}
.cc-calendar-weekday {
  font-size: 0.6875rem;
  font-weight: 500;
  color: var(--text-4);
  text-align: center;
  padding: 4px 0;
}
.cc-calendar-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}
.cc-calendar-cell-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: none;
  background: transparent;
  border-radius: 8px;
  color: var(--text-1);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  padding: 0;
}
.cc-calendar-cell-trigger:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.cc-calendar-cell-trigger[data-selected] {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.cc-calendar-cell-trigger[data-today] {
  color: var(--brand-500);
  font-weight: 600;
}
.cc-calendar-cell-trigger[data-today][data-selected] {
  color: var(--text-on-accent);
}
.cc-calendar-cell-trigger[data-outside-view] {
  color: var(--text-4);
  opacity: 0.5;
}
.cc-calendar-cell-trigger[data-disabled] {
  color: var(--text-4);
  opacity: 0.4;
  cursor: not-allowed;
}

/* DatePicker Input segment 样式（穿透 Reka UI） */
:deep([data-reka-date-picker-field-segment]) {
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
  outline: none;
  border-radius: 3px;
  padding: 0 1px;
}
:deep([data-reka-date-picker-field-segment]:focus) {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
:deep([data-reka-date-picker-field-segment][data-reka-date-picker-field-segment='literal']) {
  color: var(--text-4);
}
:deep([data-reka-date-picker-field-segment][data-placeholder]) {
  color: var(--text-4);
}
</style>
