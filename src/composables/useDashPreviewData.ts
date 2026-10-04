import { computed, ref } from 'vue'
import { useStore } from '../stores/workbench'
import { describeWeather } from '../utils/weather'
import { toLunar } from '../utils/lunar'
import { randomLocalQuote } from '../utils/quotes'
import {
  compareByOrder,
  dueBadge,
  GROUP_META,
  groupOf,
  isoKey,
  startOfDay,
  type DueBadge,
} from '../utils/todoSchedule'
import { sudaCustomConfigured, sudaCustomItems } from '../utils/sudaCustom'
import type { Countdown, Note, Resource, SudaCustomModuleConfig } from '../api/tauri'

/**
 * 布局编辑器预览的共享派生数据。
 *
 * 为什么单独一层：一张预览卡对应一个组件实例，画布里 9~12 个格子会把「过滤 + 排序全量
 * 待办 / 资源」重复做十几遍（待办上千条时编辑器打开明显变慢）。提到模块作用域的 computed
 * 上，Vue 会缓存并让所有实例共享同一次计算。
 *
 * 口径铁律：所有过滤 / 排序 / 文案都照抄对应真实卡片（CountdownCard / RecentBar /
 * NotesOverviewCard / TodoOverviewCard / TodoCard），预览绝不能有自己的一套算法——
 * 数字或顺序与真卡不一致，用户会当成新 bug。
 */

const store = useStore()

// ---- 分钟级 tick（仅编辑器在场时运行，由 DashboardLayoutEditor 挂载/卸载时开关）----
// 静态缩印里的时间、农历、倒计时剩余、相对时间需要跟着走，但不必秒级
const previewMinuteTick = ref(Math.floor(Date.now() / 60_000))
let tickTimer: ReturnType<typeof setInterval> | null = null

export function startPreviewTick() {
  if (tickTimer) return
  tickTimer = setInterval(() => {
    previewMinuteTick.value = Math.floor(Date.now() / 60_000)
  }, 30_000)
}

export function stopPreviewTick() {
  if (!tickTimer) return
  clearInterval(tickTimer)
  tickTimer = null
}

/** 当前时间：读一次 tick 建立分钟级依赖，之后每秒级的秒数在每次重算时取实时值 */
const previewDate = computed(() => {
  void previewMinuteTick.value
  return new Date()
})

const pad = (n: number) => String(n).padStart(2, '0')
const WEEKDAYS = ['日', '一', '二', '三', '四', '五', '六'] as const

// ---- 天气（clock big 右上角 / weather 卡两形态共用）----
const weather = computed(() => store.state.weather)
const weatherDesc = computed(() =>
  weather.value ? describeWeather(weather.value.weather_code) : null,
)
const tempText = computed(() =>
  weather.value ? `${Math.round(weather.value.temperature)}°` : '--',
)
const weatherLabel = computed(() => weatherDesc.value?.label ?? '--')
const cityText = computed(() => weather.value?.city ?? '')
const weatherSubText = computed(() => {
  if (!weather.value || !weatherDesc.value) return ''
  return `${weatherDesc.value.label}${weather.value.city ? ` · ${weather.value.city}` : ''}`
})

// ---- 时钟静态形态（形态浮层缩略图用；画布内挂真实 ClockCard）----
const timeText = computed(() => {
  const d = previewDate.value
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
})
const timeHM = computed(() => {
  const d = previewDate.value
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`
})
const secText = computed(() => pad(previewDate.value.getSeconds()))
const dateText = computed(() => {
  const d = previewDate.value
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${WEEKDAYS[d.getDay()]}`
})
const lunar = computed(() => {
  void previewMinuteTick.value
  return toLunar(new Date())
})
/** 语录在共享层只求一次：多实例若各自 randomLocalQuote()，同屏会出现两种「真实语录」 */
const quoteText = computed(() => {
  const custom = store.state.config.clock_quote?.trim()
  if (custom) return custom
  if (store.state.quote?.content) return store.state.quote.content
  void previewMinuteTick.value
  return randomLocalQuote().content
})

/**
 * 相对时间：逐条对齐 NotesOverviewCard 的 fmtTime（含「小时前」需同一天、跨年补年份）。
 * 读 tick 让「刚刚」在下一分钟自然变成「1 分钟前」。
 */
function relTime(iso: string): string {
  void previewMinuteTick.value
  const t = new Date(iso)
  const n = new Date()
  const diffMin = Math.floor((n.getTime() - t.getTime()) / 60000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24 && n.toDateString() === t.toDateString()) return `${diffHour} 小时前`
  if (n.getFullYear() === t.getFullYear()) return `${t.getMonth() + 1}月${t.getDate()}日`
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日`
}

/** 与 CountdownCard 展示同形态的剩余时长：`3天 05:12` / `1:25:00` / `24:59` */
function fmtRemain(c: Countdown): string {
  void previewMinuteTick.value
  const ms = Math.max(0, c.end_at - Date.now())
  const s = Math.floor(ms / 1000)
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = s % 60
  if (d > 0) return `${d}天 ${pad(h)}:${pad(m)}`
  if (h > 0) return `${h}:${pad(m)}:${pad(sec)}`
  return `${pad(m)}:${pad(sec)}`
}

// ---- 系统资源 ----
const sys = computed(() => store.state.systemInfo)
const cpuPct = computed(() => Math.round(sys.value?.cpuUsage ?? 0))
const memPct = computed(() => Math.round(sys.value?.memPercent ?? 0))
const memLabel = computed(() => {
  const s = sys.value
  if (!s) return '—'
  return `${(s.memUsedMb / 1024).toFixed(1)} / ${(s.memTotalMb / 1024).toFixed(1)} GB`
})

// ---- 便签（slot 1 / 2）----
const stickyText = computed<Record<number, string>>(() => {
  const out: Record<number, string> = { 1: '', 2: '' }
  for (const s of store.state.stickies) {
    if (s.slot === 1 || s.slot === 2) out[s.slot] = s.content?.trim() ?? ''
  }
  return out
})

// ---- 速记概览（latest 按 updated_at 降序 + 摘要规则同 NotesOverviewCard.summary）----
const notesCount = computed(() => store.state.notes.length)
const tagCount = computed(() => store.state.tags.length)
const latestNote = computed<Note | null>(
  () =>
    [...store.state.notes].sort((a, b) => b.updated_at.localeCompare(a.updated_at))[0] ?? null,
)
const latestTitle = computed(() => {
  const n = latestNote.value
  if (!n) return ''
  const t = n.title.trim()
  if (t && t !== '无标题笔记') return t
  const text = n.content.replace(/\s+/g, ' ').trim()
  return text ? (text.length > 14 ? text.slice(0, 14) + '…' : text) : '空白笔记'
})

// ---- 待办概览（只算顶级待办，同真卡）----
const topTodos = computed(() => store.state.todos.filter((t) => t.parent_id == null))
const todoTotal = computed(() => topTodos.value.length)
const todoDone = computed(() => topTodos.value.filter((t) => t.done).length)
const todoRate = computed(() =>
  todoTotal.value ? Math.round((todoDone.value / todoTotal.value) * 100) : 0,
)
const todoTodayAdded = computed(() => {
  const n = new Date()
  return topTodos.value.filter(
    (t) => new Date(t.created_at).toDateString() === n.toDateString(),
  ).length
})

// ---- 速达数量 ----
const resCount = computed(() => {
  const r = store.state.resources
  return {
    total: r.length,
    app: r.filter((x) => x.kind === 'app').length,
    web: r.filter((x) => x.kind === 'web').length,
    file: r.filter((x) => x.kind === 'file').length,
  }
})

// ---- 倒计时（end_at 升序，同 CountdownCard.sortedCountdowns：已结束仍在列表，仅状态不同）----
const countdownList = computed(() =>
  [...store.state.countdowns]
    .sort((a, b) => a.end_at - b.end_at || b.id - a.id)
    .slice(0, 8),
)

// ---- 提示词（store 已按置顶 → 复制次数 → 最近复制排好序）----
const snippetList = computed(() => store.state.snippets.slice(0, 12))

// ---- 待办分组（置顶→逾期→今天→有日期→无日期，同 TodoCard.pendingGroups）----
export interface PreviewTodoGroup {
  label: string
  /** 渲染用条目（软上限，防超大列表拖垮编辑器 DOM） */
  items: { id: number; title: string; priority: number; pinned: boolean; badge: DueBadge | null; due_at: number | null }[]
  /** 该组真实条数：计数徽标必须显全量，否则预览数字与真卡不符 */
  total: number
}
const todoGroups = computed<PreviewTodoGroup[]>(() => {
  const today = new Date()
  const pending = topTodos.value.filter((t) => !t.done)
  const out: PreviewTodoGroup[] = []
  for (let g = 0; g < GROUP_META.length; g++) {
    const all = pending.filter((t) => groupOf(t, today) === g).sort(compareByOrder)
    if (!all.length) continue
    out.push({
      label: GROUP_META[g].label,
      total: all.length,
      items: all.slice(0, 8).map((t) => ({
        id: t.id,
        title: t.title,
        priority: t.priority,
        pinned: t.pinned,
        badge: dueBadge(t, today),
        due_at: t.due_at,
      })),
    })
  }
  return out
})
const pendingCount = computed(() => topTodos.value.filter((t) => !t.done).length)
const doneCount = computed(() => topTodos.value.filter((t) => t.done).length)

/**
 * 日历模块缩印的「每天几条」标记：口径照抄真卡 `TodoCalendarCard.realByDay`——
 * **全量**顶级未完成待办按 due_at 落格，不能走上面 todoGroups 的软上限截断
 * （截断会让某些日子在预览里少了点，与真卡不符）。
 * 周期待办的虚拟实例真卡是异步展开的，缩印不做（保持同步渲染，差异已知）。
 */
const todoDayMarks = computed(() => {
  const map = new Map<string, number>()
  for (const t of topTodos.value) {
    if (t.done || t.due_at == null) continue
    const key = isoKey(new Date(t.due_at))
    map.set(key, (map.get(key) ?? 0) + 1)
  }
  return map
})

/**
 * 日历缩印的「含未完成逾期」日期集合：口径照抄真卡 `TodoCalendarCard.overdueByDay`——
 * 未完成且截止日早于今天（startOfDay 比较，同 dueBadge kind 'over'），全量条目判定、
 * 不按截断后的 chip；周期待办虚拟实例同样不参与。读 previewDate 绑定分钟 tick，
 * 编辑器跨午夜打开时「今天」推进后红标记随之消长。
 */
const todoOverdueMarks = computed(() => {
  const today0 = startOfDay(previewDate.value)
  const set = new Set<string>()
  for (const t of topTodos.value) {
    if (t.done || t.due_at == null) continue
    if (startOfDay(new Date(t.due_at)).getTime() < today0.getTime()) {
      set.add(isoKey(new Date(t.due_at)))
    }
  }
  return set
})

// ---- 最近使用（有启动记录 → last_launched_at 倒序，同 RecentBar）----
const recentList = computed<Resource[]>(() =>
  store.state.resources
    .filter((r) => r.last_launched_at)
    .sort(
      (a, b) =>
        new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
    )
    .slice(0, 20),
)

// ---- 自定义速达槽位（suda1..4）----
// 槽位数据按 modId 各不相同，做不成模块级共享 computed；这里只提供取数函数，
// 过滤/排序唯一实现在 utils/sudaCustom.ts（真卡同源），响应式由调用方的 computed 追踪。
function sudaCustomOf(modId: string): {
  cfg: SudaCustomModuleConfig | undefined
  items: Resource[]
  configured: boolean
} {
  const cfg = store.sudaCustomConfigOf(modId)
  return { cfg, items: sudaCustomItems(cfg, store.state.resources), configured: sudaCustomConfigured(cfg) }
}

export const dashPreviewData = {
  previewDate,
  previewMinuteTick,
  timeText,
  timeHM,
  secText,
  dateText,
  lunar,
  quoteText,
  relTime,
  fmtRemain,
  weather,
  weatherDesc,
  tempText,
  weatherLabel,
  cityText,
  weatherSubText,
  cpuPct,
  memPct,
  memLabel,
  stickyText,
  notesCount,
  tagCount,
  latestNote,
  latestTitle,
  todoTotal,
  todoDone,
  todoRate,
  todoTodayAdded,
  resCount,
  countdownList,
  snippetList,
  todoGroups,
  todoDayMarks,
  todoOverdueMarks,
  pendingCount,
  doneCount,
  recentList,
  sudaCustomOf,
}
