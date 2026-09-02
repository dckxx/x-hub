/**
 * 待办排期（截止日期/提醒）纯工具：分组、徽标文案、日历网格。
 * 与 TodoCard 原型 docs/prototypes/todo-schedule-prototype.html 的规则保持一致。
 */

export type DueBadgeKind = 'over' | 'today' | 'tmr' | 'date'

export interface DueBadge {
  kind: DueBadgeKind
  text: string
}

/** 待办分组序号：0 逾期 → 1 今天 → 2 有日期 → 3 无日期 */
export const GROUP_COUNT = 4

export const GROUP_META: ReadonlyArray<{ label: string }> = [
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

export function groupOf(t: { due_at: number | null }, today: Date): number {
  if (t.due_at == null) return 3
  const d = startOfDay(new Date(t.due_at))
  if (d.getTime() < startOfDay(today).getTime()) return 0
  if (isoKey(d) === isoKey(today)) return 1
  return 2
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
 * 截止徽标：逾期(红) → 今天(橙，末尾时段只显「今天」) → 明天(品牌色) → M月D日(灰)
 */
export function dueBadge(t: { due_at: number | null }, today: Date): DueBadge | null {
  if (t.due_at == null) return null
  const due = new Date(t.due_at)
  const d = startOfDay(due)
  const today0 = startOfDay(today)
  if (d.getTime() < today0.getTime()) return { kind: 'over', text: `逾期 ${fmtDay(d)}` }
  const diff = Math.round((d.getTime() - today0.getTime()) / 86_400_000)
  if (diff === 0) {
    // 视为「当天结束」的截止（默认 23:59）不显示具体时间
    const endOfDay = due.getHours() === 23 && due.getMinutes() > 50
    return { kind: 'today', text: endOfDay ? '今天' : `今天 ${fmtHM(t.due_at)}` }
  }
  if (diff === 1) return { kind: 'tmr', text: '明天' }
  return { kind: 'date', text: fmtDay(d) }
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
