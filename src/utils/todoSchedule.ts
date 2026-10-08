/**
 * 待办排期（截止日期/提醒）纯工具：分组、徽标文案、日历网格。
 * 与 TodoCard 原型 docs/prototypes/todo-schedule-prototype.html 的规则保持一致。
 */

export type DueBadgeKind = 'over' | 'today' | 'tmr' | 'date'

export interface DueBadge {
  kind: DueBadgeKind
  text: string
}

/** 待办分组序号（卡片/浮窗/编辑器预览共用）：0 置顶 → 1 逾期 → 2 今天 → 3 有日期 → 4 无日期 */
export const GROUP_COUNT = 5

export const GROUP_META: ReadonlyArray<{ label: string }> = [
  { label: '置顶' },
  { label: '逾期' },
  { label: '今天' },
  { label: '有日期' },
  { label: '无日期' },
]

export function startOfDay(d: Date): Date {
  const x = new Date(d)
  x.setHours(0, 0, 0, 0)
  return x
}

export function addDays(d: Date, n: number): Date {
  const x = new Date(d)
  x.setDate(x.getDate() + n)
  return x
}

export function isoKey(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** M月D日 */
export function fmtDay(d: Date): string {
  return `${d.getMonth() + 1}月${d.getDate()}日`
}

/** HH:MM（毫秒时间戳） */
export function fmtHM(ts: number): string {
  const d = new Date(ts)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}`
}

/**
 * 解析后端时间字符串为本地 Date。
 * completed_at 是 SQLite strftime 存的 UTC 字符串（无时区后缀，如 2026-09-27 05:34:01.229），
 * new Date 直读会按本地时区错读，必须补 Z 解析。解析失败返回 null。
 */
export function parseServerDate(s: string | null): Date | null {
  if (!s) return null
  const d = new Date(s.includes('T') ? s : `${s.replace(' ', 'T')}Z`)
  return Number.isNaN(d.getTime()) ? null : d
}

/**
 * 已完成行的「完成时间」徽标文案。
 * 相对口径：今天/昨天 带时刻；同年 M月D日 带时刻；跨年补年份。解析失败返回 null（行内隐藏）。
 */
export function doneAtLabel(
  completedAt: string | null,
  now: Date,
): { text: string; full: string } | null {
  const d = parseServerDate(completedAt)
  if (!d) return null
  const day = startOfDay(d)
  const today0 = startOfDay(now)
  const diff = Math.round((today0.getTime() - day.getTime()) / 86_400_000)
  const hm = ` ${fmtHM(d.getTime())}`
  const text =
    diff === 0
      ? `今天${hm}`
      : diff === 1
        ? `昨天${hm}`
        : d.getFullYear() === now.getFullYear()
          ? `${fmtDay(d)}${hm}`
          : `${d.getFullYear()}年${fmtDay(d)}${hm}`
  return { text, full: `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日${hm}` }
}

export function groupOf(
  t: { pinned?: boolean; due_at: number | null },
  today: Date,
): number {
  // 置顶与日期无关：固定进最顶部「置顶」组（与待办视图 viewGroupOf 同语义）。
  // 若不独立成组，新增待办按「新的在最上」落进同组会排到置顶条目上面。
  if (t.pinned) return 0
  if (t.due_at == null) return 4
  const d = startOfDay(new Date(t.due_at))
  if (d.getTime() < startOfDay(today).getTime()) return 1
  if (isoKey(d) === isoKey(today)) return 2
  return 3
}

/**
 * 组内排序（TodoCard / TodoFloat 等展示层统一使用）：
 * 手动拖过的（sort_order 非空）按 sort_order 升序，未排序的按创建时间倒序排在前
 * （与「新建待办置顶」的默认直觉一致）。拖动会整组赋值，稳态下两组不混排；
 * 万一混排（补值失败等异常路径），未排序条目浮到组顶也是合理兜底。
 */
export function compareByOrder(
  a: { sort_order: number | null; created_at: string },
  b: { sort_order: number | null; created_at: string },
): number {
  const ao = a.sort_order
  const bo = b.sort_order
  if (ao != null && bo != null) return ao - bo
  if (ao == null && bo == null) return b.created_at.localeCompare(a.created_at)
  return ao == null ? -1 : 1
}

/**
 * 平铺列表（待办浮窗等不分组的宿主）排序：置顶条目浮到最前，其余按 compareByOrder。
 * 置顶条目与新增待办共用一个列表时，若不把置顶提前，新建条目会按创建时间倒序盖到它上面。
 */
export function comparePinnedFirst(
  a: { pinned?: boolean; sort_order: number | null; created_at: string },
  b: { pinned?: boolean; sort_order: number | null; created_at: string },
): number {
  const ap = a.pinned === true
  const bp = b.pinned === true
  if (ap !== bp) return ap ? -1 : 1
  return compareByOrder(a, b)
}

/**
 * 子待办排序（方向与顶级待办刻意的相反）：
 * 手动拖过的（sort_order 非空）按 sort_order 升序，未排序的按创建时间**正序**——
 * 先加的在上、新增的子待办追加到末尾，符合「子任务是往下追加的清单」直觉。
 * 「已手动排序」与「未排序」混排时（补值失败等异常路径）未排序的排在其后，
 * 新增子待办同样落在末尾，不会插到已排好的顺序中间。
 */
export function compareChildOrder(
  a: { sort_order: number | null; created_at: string },
  b: { sort_order: number | null; created_at: string },
): number {
  const ao = a.sort_order
  const bo = b.sort_order
  if (ao != null && bo != null) return ao - bo
  if (ao == null && bo == null) return a.created_at.localeCompare(b.created_at)
  return ao == null ? 1 : -1
}

/**
 * 截止徽标：逾期(红) → 今天(橙，末尾时段只显「今天」) → 明天(品牌色) → M月D日(灰)
 * 跨年补年份：截止日不在「当前年」时显示「YYYY年M月D日」，避免跨年条目只看到月日产生歧义。
 */
export function dueBadge(t: { due_at: number | null }, today: Date): DueBadge | null {
  if (t.due_at == null) return null
  const due = new Date(t.due_at)
  const d = startOfDay(due)
  const today0 = startOfDay(today)
  // 「今天/明天」必然是今年（参照日 nearby），只有 over/date 两类可能跨年
  const y = d.getFullYear() !== today0.getFullYear() ? `${d.getFullYear()}年` : ''
  if (d.getTime() < today0.getTime()) return { kind: 'over', text: `逾期 ${y}${fmtDay(d)}` }
  const diff = Math.round((d.getTime() - today0.getTime()) / 86_400_000)
  if (diff === 0) {
    // 视为「当天结束」的截止（默认 23:59）不显示具体时间
    const endOfDay = due.getHours() === 23 && due.getMinutes() > 50
    return { kind: 'today', text: endOfDay ? '今天' : `今天 ${fmtHM(t.due_at)}` }
  }
  if (diff === 1) return { kind: 'tmr', text: '明天' }
  return { kind: 'date', text: `${y}${fmtDay(d)}` }
}

/** 下一个周一：从明天起找（今天恰好是周一时返回下周一，避免「下周一」快捷键选回当天） */
export function nextMonday(today: Date): Date {
  const d = addDays(startOfDay(today), 1)
  while (d.getDay() !== 1) d.setDate(d.getDate() + 1)
  return startOfDay(d)
}

/**
 * 日历网格：以周一为first列的 6×7=42 天（覆盖目标月份）。
 * 返回项 out 标记非本月，today 标记今天。
 */
export function calendarGrid(cursor: Date, today: Date): Array<{ key: string; day: number; out: boolean; today: boolean }> {
  const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1)
  const offset = (first.getDay() + 6) % 7
  const monday = addDays(first, -offset)
  const cells: Array<{ key: string; day: number; out: boolean; today: boolean }> = []
  for (let i = 0; i < 42; i++) {
    const d = addDays(monday, i)
    cells.push({
      key: isoKey(d),
      day: d.getDate(),
      out: d.getMonth() !== cursor.getMonth(),
      today: isoKey(d) === isoKey(today),
    })
  }
  return cells
}

/**
 * 待办视图分组序号：0 置顶 → 1 逾期 → 2 今天 → 3 本周 → 4 本月 → 5 以后 → 6 无日期。
 * 与卡片用的 5 组（GROUP_META）不同：视图有「置顶」区与更细的时间切分，
 * 卡片保持轻量，不跟着改。
 */
export const VIEW_GROUP_COUNT = 7

export const VIEW_GROUP_META: ReadonlyArray<{ label: string }> = [
  { label: '置顶' },
  { label: '逾期' },
  { label: '今天' },
  { label: '本周' },
  { label: '本月' },
  { label: '以后' },
  { label: '无日期' },
]

/** 自然周结束（周日 23:59:59.999） */
function endOfWeek(d: Date): Date {
  const monday = addDays(startOfDay(d), -((d.getDay() + 6) % 7))
  return addDays(monday, 7)
}

export function viewGroupOf(
  t: { pinned: boolean; due_at: number | null },
  today: Date,
): number {
  // 置顶与日期无关：固定进最顶部「置顶」区（周期待办滚动换组也不会掉出去）
  if (t.pinned) return 0
  if (t.due_at == null) return 6
  const due = new Date(t.due_at)
  const d = startOfDay(due)
  const today0 = startOfDay(today)
  if (d.getTime() < today0.getTime()) return 1
  if (d.getTime() === today0.getTime()) return 2
  if (d.getTime() < endOfWeek(today).getTime()) return 3
  if (d.getMonth() === today.getMonth() && d.getFullYear() === today.getFullYear()) return 4
  return 5
}

/** 时间范围视图（horizon）筛选：今天 / 本周 / 本月 / 全部 */
export type Horizon = 'today' | 'week' | 'month' | 'all'

export function inHorizon(
  t: { due_at: number | null; pinned?: boolean },
  horizon: Horizon,
  today: Date,
): boolean {
  if (horizon === 'all') return true
  // 置顶条目与日期无关（固定排在列表最顶部「置顶」区），任何时间范围下都必须可见：
  // 否则「置顶 + 未来日期」的条目在「今天/本周」视图里会整条消失，与置顶语义冲突。
  if (t.pinned) return true
  // 无截止日期的条目不属于任何时间范围，但必须**始终可见**：
  // 待办视图里新建待办默认不设截止，若这里返回 false，新条目保存后会立刻从列表消失
  // （只有切到「全部」才看得见），用户会以为没建成功。
  if (t.due_at == null) return true
  const d = startOfDay(new Date(t.due_at))
  const today0 = startOfDay(today)
  // 逾期条目不属于任何范围，但必须始终可见（否则会「消失」）
  if (d.getTime() < today0.getTime()) return true
  if (horizon === 'today') return d.getTime() === today0.getTime()
  if (horizon === 'week') return d.getTime() < endOfWeek(today).getTime()
  return d.getMonth() === today.getMonth() && d.getFullYear() === today.getFullYear()
}

const WEEKDAY_LABELS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'] as const

/** 周期规则的短文案（行内展示用；规则的权威解释在 Rust 侧） */
export function repeatLabel(t: {
  repeat_mode: string
  repeat_every: number | null
  repeat_unit: string | null
  repeat_weekdays: number | null
  repeat_month_day: number | null
  repeat_month_nth: number | null
  due_at: number | null
}): string {
  const days = (mask: number) =>
    WEEKDAY_LABELS.filter((_, i) => mask & (1 << i)).join('、')
  switch (t.repeat_mode) {
    case 'daily':
      return '每天'
    case 'weekdays':
      return '每个工作日'
    case 'weekly':
      return t.repeat_weekdays ? `每周 ${days(t.repeat_weekdays)}` : '每周'
    case 'monthly': {
      if (t.repeat_month_nth != null) {
        const nth = t.repeat_month_nth === -1 ? '最后' : `第 ${t.repeat_month_nth}`
        const wd = t.repeat_weekdays ? days(t.repeat_weekdays) : ''
        return `每月 ${nth}个${wd}`
      }
      const day = t.repeat_month_day === -1 ? '最后一天' : `${t.repeat_month_day} 日`
      return `每月 ${day}`
    }
    case 'yearly': {
      if (t.due_at == null) return '每年'
      const d = new Date(t.due_at)
      return `每年 ${d.getMonth() + 1} 月 ${d.getDate()} 日`
    }
    case 'custom': {
      const unit = { day: '天', week: '周', month: '个月', year: '年' }[t.repeat_unit ?? 'day'] ?? '天'
      const every = t.repeat_every ?? 1
      if (t.repeat_unit === 'week' && t.repeat_weekdays) {
        return `每 ${every} 周的 ${days(t.repeat_weekdays)}`
      }
      return `每 ${every} ${unit}`
    }
    default:
      return ''
  }
}

/** 结束条件的短文案（空串 = 永不结束） */
export function repeatEndLabel(t: {
  repeat_end_mode: string | null
  repeat_end_at: number | null
  repeat_count: number | null
}): string {
  if (t.repeat_end_mode === 'until' && t.repeat_end_at != null) {
    const d = new Date(t.repeat_end_at)
    return `到 ${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} 为止`
  }
  if (t.repeat_end_mode === 'count' && t.repeat_count != null) {
    return `共 ${t.repeat_count} 次`
  }
  return ''
}

export const HOUR_OPTIONS: ReadonlyArray<{ value: number; label: string }> = Array.from(
  { length: 24 },
  (_, h) => ({ value: h, label: String(h).padStart(2, '0') }),
)

/** 分钟选项：5 分钟步进；当前值不在步进上时（如 23:59）补一个精确项，避免显示漂移 */
export function minuteOptions(selected: number): Array<{ value: number; label: string }> {
  const opts = Array.from({ length: 12 }, (_, i) => ({ value: i * 5, label: String(i * 5).padStart(2, '0') }))
  if (!opts.some((o) => o.value === selected)) {
    opts.push({ value: selected, label: String(selected).padStart(2, '0') })
    opts.sort((a, b) => a.value - b.value)
  }
  return opts
}

/** 提醒默认时刻：截止前 30 分钟（跨小时正确借位），至少落在当天 00:00 */
export function defaultRemindTime(due: Date): { hour: number; minute: number } {
  const total = due.getHours() * 60 + due.getMinutes() - 30
  const clamped = Math.max(0, total)
  return { hour: Math.floor(clamped / 60), minute: clamped % 60 }
}
