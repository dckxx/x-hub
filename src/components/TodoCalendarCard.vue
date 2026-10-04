<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowRight, CalendarDays, ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Todo, TodoOccurrence } from '../api/tauri'
import { calendarGrid, dueBadge, isoKey, parseServerDate } from '../utils/todoSchedule'

/**
 * 工作台「日历」模块：月历形态展示待办分布（含周期待办的**虚拟实例**）。
 * 只读呈现——点整卡进待办视图做新建与改期；周期规则的展开结果来自 Rust 侧命令。
 */
const props = defineProps<{ onOpenDetail?: () => void; title?: string; hideTitle?: boolean }>()

const store = useStore()

const cursor = ref(new Date())
const occurrences = ref<TodoOccurrence[]>([])
/** 工作台卡片常驻、不会重新挂载：用分钟 tick 推进「今天」，跨午夜后高亮与逾期判定才跟着走
 *  （与 ClockCard 的 minuteTick 同一口径；只在日期真的变了才赋值，避免无谓重算） */
const today = ref(new Date())
let dayTimer: ReturnType<typeof setInterval> | null = null

const cells = computed(() => calendarGrid(cursor.value, today.value))
const monthLabel = computed(() => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`)

const topTodos = computed(() => store.state.todos.filter((t) => t.parent_id == null))

/** 真实条目落格：未完成按 due_at；**已完成也显示**——有截止落原日期，
 *  没截止则落在完成当天（completed_at，UTC 字符串需补 Z 解析），用删除线淡显。
 *  同一格内未完成排在已完成前面（MAX_CHIPS 截断时不至于把待办挤掉）。 */
const realByDay = computed(() => {
  const map = new Map<string, Todo[]>()
  for (const t of topTodos.value) {
    const at = t.due_at ?? (t.done ? parseServerDate(t.completed_at)?.getTime() ?? null : null)
    if (at == null) continue
    const key = isoKey(new Date(at))
    const list = map.get(key)
    if (list) list.push(t)
    else map.set(key, [t])
  }
  for (const list of map.values()) list.sort((a, b) => Number(a.done) - Number(b.done))
  return map
})

const virtualByDay = computed(() => {
  const map = new Map<string, TodoOccurrence[]>()
  for (const o of occurrences.value) {
    const key = isoKey(new Date(o.at_ms))
    const list = map.get(key)
    if (list) list.push(o)
    else map.set(key, [o])
  }
  return map
})

const titleOf = computed(() => {
  const map = new Map<number, string>()
  for (const t of store.state.todos) map.set(t.id, t.title)
  return map
})

/** 请求序号：快速翻月时先发的响应可能后到，旧月份结果会盖掉新月份 */
let occurrenceSeq = 0

async function load() {
  const first = cells.value[0]
  const from = new Date(`${first.key}T00:00:00`)
  const to = new Date(from)
  to.setDate(to.getDate() + 42)
  const seq = ++occurrenceSeq
  try {
    const list = await store.expandTodoOccurrences(from.getTime(), to.getTime() - 1)
    if (seq !== occurrenceSeq) return
    occurrences.value = list
  } catch {
    // 卡片是概览：失败就当作「没有周期实例」，不弹提示打扰用户（待办视图里有明确提示）
    if (seq === occurrenceSeq) occurrences.value = []
  }
}

/** 周期规则签名：待办视图或扩展桥改了周期规则 / 滚动了一轮之后，
 *  常驻卡片的虚拟实例必须重算，否则日历上留着过期的虚线实例 */
const repeatSignature = computed(() =>
  store.state.todos
    .filter((t) => t.parent_id == null && !t.done && t.repeat_mode !== 'once')
    .map((t) => `${t.id}:${t.due_at}:${t.repeat_mode}:${t.repeat_done_count}`)
    .join('|'),
)

onMounted(() => {
  void load()
  dayTimer = setInterval(() => {
    const d = new Date()
    if (d.toDateString() !== today.value.toDateString()) today.value = d
  }, 60_000)
})
onBeforeUnmount(() => {
  if (dayTimer) clearInterval(dayTimer)
})
watch([cursor, today, repeatSignature], () => void load())

function shift(delta: number, e: MouseEvent) {
  e.stopPropagation()
  const d = new Date(cursor.value)
  d.setMonth(d.getMonth() + delta, 1)
  cursor.value = d
}

/** 每格最多渲染的 chip 数（真实条目优先，剩余额度给虚拟实例） */
const MAX_CHIPS = 2

/** 每格要渲染的 chip 与「还有几条」：真实 + 虚拟合计口径，
 *  余数必须按**实际渲染条数**算——真实 2 条 + 虚拟 2 条时只画 2 条、显示 +2，
 *  不能按「各自截断」算成画 4 条还显示 +2。 */
const chipsByDay = computed(() => {
  const map = new Map<string, { real: Todo[]; virtual: TodoOccurrence[]; more: number }>()
  for (const c of cells.value) {
    const real = realByDay.value.get(c.key) ?? []
    const virtual = virtualByDay.value.get(c.key) ?? []
    const realShown = real.slice(0, MAX_CHIPS)
    const virtualShown = virtual.slice(0, Math.max(0, MAX_CHIPS - realShown.length))
    map.set(c.key, {
      real: realShown,
      virtual: virtualShown,
      more: real.length + virtual.length - realShown.length - virtualShown.length,
    })
  }
  return map
})
</script>

<template>
  <section class="card todo-cal" :aria-label="title ?? '日历'" @click="props.onOpenDetail?.()">
    <header class="tc-header" :class="{ 'hd-float': hideTitle }">
      <h3 v-if="!hideTitle" class="tc-title">
        <CalendarDays :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '日历' }}</span>
      </h3>
      <span class="tc-month">{{ monthLabel }}</span>
      <span class="tc-spacer"></span>
      <button class="tc-nav" type="button" aria-label="上个月" @click="shift(-1, $event)">
        <ChevronLeft :size="13" :stroke-width="2" />
      </button>
      <button class="tc-nav" type="button" aria-label="下个月" @click="shift(1, $event)">
        <ChevronRight :size="13" :stroke-width="2" />
      </button>
      <ArrowRight v-if="!hideTitle" class="tc-more" :size="14" :stroke-width="2" aria-hidden="true" />
    </header>

    <div class="tc-grid">
      <div v-for="d in ['一', '二', '三', '四', '五', '六', '日']" :key="d" class="tc-dow">{{ d }}</div>
      <div
        v-for="c in cells"
        :key="c.key"
        class="tc-cell"
        :class="{ out: c.out, today: c.today }"
      >
        <span class="tc-day">{{ c.day }}</span>
        <div class="tc-chips">
          <span
            v-for="t in (chipsByDay.get(c.key)?.real ?? [])"
            :key="'r' + t.id"
            class="tc-chip real"
            :class="{ done: t.done, late: !t.done && dueBadge(t, today)?.kind === 'over' }"
            :title="t.done ? `${t.title}（已完成）` : t.title"
          >{{ t.title }}</span>
          <span
            v-for="o in (chipsByDay.get(c.key)?.virtual ?? [])"
            :key="'v' + o.todo_id + o.at_ms"
            class="tc-chip virtual"
            :title="`${titleOf.get(o.todo_id) ?? '周期待办'}（虚拟实例）`"
          >{{ titleOf.get(o.todo_id) ?? '周期待办' }}</span>
          <span v-if="(chipsByDay.get(c.key)?.more ?? 0) > 0" class="tc-more-cnt">
            +{{ chipsByDay.get(c.key)?.more }}
          </span>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.todo-cal {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
  cursor: pointer;
}
.tc-header {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
}
.tc-title {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  white-space: nowrap;
  color: var(--text-1);
}
.tc-title span {
  overflow: hidden;
  text-overflow: ellipsis;
}
.tc-title :deep(svg) {
  flex-shrink: 0;
  color: var(--brand-500);
}
.tc-month {
  font-size: 0.6875rem;
  white-space: nowrap;
  color: var(--text-3);
}
.tc-spacer {
  flex: 1;
}
.tc-nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.tc-nav:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.tc-more {
  color: var(--text-4);
}
.tc-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  grid-template-rows: 14px repeat(6, minmax(0, 1fr));
  gap: 2px;
}
.tc-dow {
  font-size: 0.5625rem;
  line-height: 14px;
  font-weight: 700;
  color: var(--text-4);
  text-align: center;
}
.tc-cell {
  min-width: 0;
  min-height: 0;
  padding: 2px 3px;
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  background: var(--bg-card-soft);
  overflow: hidden;
}
.tc-cell.out {
  opacity: 0.4;
}
.tc-cell.today {
  border-color: var(--brand-500);
}
.tc-day {
  /* 独立行盒避免继承正文行高，紧凑格子也能容纳完整日期。 */
  display: block;
  font-size: 0.5625rem;
  line-height: 12px;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.tc-chips {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.tc-chip {
  display: block;
  padding: 0 3px;
  font-size: 0.5rem;
  line-height: 1.5;
  border-radius: 3px;
  border: 1px solid var(--brand-500);
  background: var(--brand-50);
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-chip.virtual {
  border-style: dashed;
  background: transparent;
  color: var(--text-3);
}
.tc-chip.late {
  border-color: var(--c-red-ink);
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.tc-chip.done {
  border-color: var(--border-soft);
  background: transparent;
  color: var(--text-4);
  text-decoration: line-through;
}
.tc-more-cnt {
  font-size: 0.5rem;
  color: var(--text-4);
}
/* 宿主 .dash-cell 已提供尺寸容器；窄卡片将月份放到第二行。
 * 隐藏标题时沿用悬浮工具栏，避免改变其定位与交互。 */
@container (max-width: 320px) {
  .tc-header:not(.hd-float) {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 22px 22px;
    row-gap: 2px;
  }
  .tc-header:not(.hd-float) .tc-title {
    grid-column: 1;
    grid-row: 1;
  }
  .tc-header:not(.hd-float) .tc-month {
    grid-column: 1 / -1;
    grid-row: 2;
    line-height: 14px;
  }
  .tc-header:not(.hd-float) .tc-nav {
    grid-row: 1;
  }
  .tc-header:not(.hd-float) .tc-spacer,
  .tc-header:not(.hd-float) .tc-more {
    display: none;
  }
}
</style>
