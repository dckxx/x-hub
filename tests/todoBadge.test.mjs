/**
 * 待办截止徽标（src/utils/todoSchedule.ts）单测。
 * 重点：跨年补年份——用户反馈「待办显示日期，不是当前年份时要显示具体年份」。
 *   node --test tests/todoBadge.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { dueBadge } from '../src/utils/todoSchedule.ts'

const at = (y, m, d, h = 12, min = 0) => new Date(y, m - 1, d, h, min).getTime()

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
