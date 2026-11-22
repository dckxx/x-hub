/**
 * 待办截止徽标与完成时间文案（src/utils/todoSchedule.ts）单测。
 * 重点：跨年补年份——用户反馈「待办显示日期，不是当前年份时要显示具体年份」；
 * 以及已完成条目在日历里的时间说明必须把「截止」和「完成于」分开写。
 *   node --test tests/todoBadge.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { doneMetaLabel, dueBadge, fmtDueFull, inDoneHorizon, inHorizon, nextRepeatLabel } from '../src/utils/todoSchedule.ts'

const at = (y, m, d, h = 12, min = 0) => new Date(y, m - 1, d, h, min).getTime()
/** 后端 completed_at 的存储形态：本地时刻 → 无时区后缀的 UTC 字符串 */
const serverStr = (d) => d.toISOString().slice(0, 23).replace('T', ' ')

test('同年截止：只显示月日', () => {
  const today = new Date(2026, 9, 8) // 2026-10-08
  assert.deepEqual(dueBadge({ due_at: at(2026, 10, 20, 23, 59) }, today), {
    kind: 'date',
    text: '10月20日',
  })
})

test('跨年截止：补年份', () => {
  const today = new Date(2026, 9, 8)
  assert.deepEqual(dueBadge({ due_at: at(2027, 1, 3) }, today), {
    kind: 'date',
    text: '2027年1月3日',
  })
})

test('逾期跨年：补年份', () => {
  const today = new Date(2026, 0, 5) // 2026-01-05
  assert.deepEqual(dueBadge({ due_at: at(2025, 12, 30) }, today), {
    kind: 'over',
    text: '逾期 2025年12月30日',
  })
})

test('逾期同年：仍只显示月日', () => {
  const today = new Date(2026, 5, 20)
  assert.deepEqual(dueBadge({ due_at: at(2026, 5, 1) }, today), {
    kind: 'over',
    text: '逾期 5月1日',
  })
})

test('今天 / 明天：不补年份', () => {
  const today = new Date(2026, 11, 31) // 2026-12-31，明天已跨年
  assert.deepEqual(dueBadge({ due_at: at(2026, 12, 31, 9) }, today), {
    kind: 'today',
    text: '今天 09:00',
  })
  assert.deepEqual(dueBadge({ due_at: at(2027, 1, 1, 9) }, today), {
    kind: 'tmr',
    text: '明天',
  })
})

test('无截止：返回 null', () => {
  assert.equal(dueBadge({ due_at: null }, new Date()), null)
})

test('完成时间说明：有截止的条目标清「截止 … · 完成于 …」', () => {
  const now = new Date(2026, 9, 17, 20, 0)
  assert.equal(
    doneMetaLabel(
      { due_at: at(2026, 10, 16, 18), completed_at: serverStr(new Date(2026, 9, 17, 14, 32)) },
      now,
    ),
    '截止 2026年10月16日 18:00 · 完成于 2026年10月17日 14:32',
  )
})

test('完成时间说明：无截止的标注「原无截止」，落格原因不含糊', () => {
  assert.equal(
    doneMetaLabel(
      { due_at: null, completed_at: serverStr(new Date(2026, 9, 17, 14, 32)) },
      new Date(2026, 9, 17, 20, 0),
    ),
    '完成于 2026年10月17日 14:32（原无截止）',
  )
})

test('完成时间说明：旧数据缺完成时间时不编造时刻', () => {
  const now = new Date(2026, 9, 17, 20, 0)
  assert.equal(
    doneMetaLabel({ due_at: at(2026, 10, 16, 18), completed_at: null }, now),
    '截止 2026年10月16日 18:00 · 已完成',
  )
  assert.equal(doneMetaLabel({ due_at: null, completed_at: null }, now), '已完成')
})

test('完整截止文案：日期型截止（23:5x）不显示时刻，带时刻的照常显示', () => {
  assert.equal(fmtDueFull(at(2026, 10, 20, 23, 59)), '2026年10月20日')
  assert.equal(fmtDueFull(at(2026, 10, 20, 9, 5)), '2026年10月20日 09:05')
})

test('周期滚动文案：今天/明天直接说，其余给月日（跨年补年份），无截止说「下一轮」', () => {
  const now = new Date(2026, 9, 17, 20, 0)
  assert.equal(nextRepeatLabel({ due_at: at(2026, 10, 17, 23, 59) }, now), '今天')
  assert.equal(nextRepeatLabel({ due_at: at(2026, 10, 18, 9, 30) }, now), '明天')
  assert.equal(nextRepeatLabel({ due_at: at(2026, 10, 25, 9, 30) }, now), '10月25日')
  assert.equal(nextRepeatLabel({ due_at: at(2027, 1, 3, 9, 30) }, now), '2027年1月3日')
  assert.equal(nextRepeatLabel({ due_at: null }, now), '下一轮')
})

test('未完成范围筛选：时间锚 = 截止，没截止按创建时间；置顶/逾期仍任何范围常显', () => {
  const today = new Date(2026, 10, 18, 12) // 2026-11-18 周三，本周一 = 11-16
  const createdAt = (y, m, d, h = 9, min = 5) => serverStr(new Date(y, m - 1, d, h, min))

  // 有截止：按截止落档
  const dueToday = { due_at: at(2026, 11, 18, 18), created_at: createdAt(2026, 10, 1) }
  const dueFriday = { due_at: at(2026, 11, 20), created_at: createdAt(2026, 10, 1) }
  const dueNextMonday = { due_at: at(2026, 11, 23), created_at: createdAt(2026, 10, 1) }
  assert.equal(inHorizon(dueToday, 'today', today), true)
  assert.equal(inHorizon(dueFriday, 'today', today), false)
  assert.equal(inHorizon(dueFriday, 'week', today), true)
  assert.equal(inHorizon(dueNextMonday, 'week', today), false)

  // 没截止：按创建时间落档——新建待办（创建于今天）任何档都能看见，不会「保存后消失」
  const freshNoDue = { due_at: null, created_at: createdAt(2026, 11, 18, 8) }
  assert.equal(inHorizon(freshNoDue, 'today', today), true)
  assert.equal(inHorizon(freshNoDue, 'week', today), true)
  // 无截止且创建于两周前：不属于今天/本周，只在同月档与「全部」出现
  const oldNoDue = { due_at: null, created_at: createdAt(2026, 11, 4) }
  assert.equal(inHorizon(oldNoDue, 'today', today), false)
  assert.equal(inHorizon(oldNoDue, 'week', today), false)
  assert.equal(inHorizon(oldNoDue, 'month', today), true)

  // 常显保障不回退：逾期（有截止且已过）与置顶在任何范围都可见
  const overdue = { due_at: at(2026, 11, 10), created_at: createdAt(2026, 10, 1) }
  assert.equal(inHorizon(overdue, 'today', today), true)
  assert.equal(inHorizon(overdue, 'week', today), true)
  const pinnedFuture = { due_at: at(2026, 12, 20), created_at: createdAt(2026, 10, 1), pinned: true }
  assert.equal(inHorizon(pinnedFuture, 'today', today), true)
  assert.equal(inHorizon(pinnedFuture, 'week', today), true)
})

test('已完成范围筛选：同一时间锚（截止??创建），无置顶/逾期常显，两时间都缺只在「全部」', () => {
  const today = new Date(2026, 10, 18, 12) // 2026-11-18 周三，本周一 = 11-16
  const createdAt = (y, m, d, h = 9, min = 5) => serverStr(new Date(y, m - 1, d, h, min))

  // 有截止：锚在原计划的档——逾期很久才完成的不会灌进今天/本周
  const doneDueToday = { due_at: at(2026, 11, 18, 18), created_at: createdAt(2026, 10, 1) }
  const doneDueLastWeek = { due_at: at(2026, 11, 13), created_at: createdAt(2026, 10, 1) }
  assert.equal(inDoneHorizon(doneDueToday, 'today', today), true)
  assert.equal(inDoneHorizon(doneDueToday, 'week', today), true)
  assert.equal(inDoneHorizon(doneDueLastWeek, 'today', today), false)
  assert.equal(inDoneHorizon(doneDueLastWeek, 'week', today), false)
  assert.equal(inDoneHorizon(doneDueLastWeek, 'month', today), true)

  // 没截止：按创建时间落档
  const doneNoDueFresh = { due_at: null, created_at: createdAt(2026, 11, 16, 8) }
  assert.equal(inDoneHorizon(doneNoDueFresh, 'week', today), true)
  const doneNoDueOld = { due_at: null, created_at: createdAt(2026, 10, 20) }
  assert.equal(inDoneHorizon(doneNoDueOld, 'week', today), false)
  assert.equal(inDoneHorizon(doneNoDueOld, 'all', today), true)

  // 已完成不做置顶/逾期常显：逾期条目与置顶条目都严格按锚点落档
  const doneOverdue = { due_at: at(2026, 11, 10), created_at: createdAt(2026, 10, 1), pinned: true }
  assert.equal(inDoneHorizon(doneOverdue, 'today', today), false)
  assert.equal(inDoneHorizon(doneOverdue, 'week', today), false)

  // created_at 缺失（旧数据极端情况）只在「全部」可见
  const noTime = { due_at: null, created_at: '' }
  assert.equal(inDoneHorizon(noTime, 'today', today), false)
  assert.equal(inDoneHorizon(noTime, 'all', today), true)
})
