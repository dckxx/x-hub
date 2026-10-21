/**
 * 待办截止徽标与完成时间文案（src/utils/todoSchedule.ts）单测。
 * 重点：跨年补年份——用户反馈「待办显示日期，不是当前年份时要显示具体年份」；
 * 以及已完成条目在日历里的时间说明必须把「截止」和「完成于」分开写。
 *   node --test tests/todoBadge.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { doneMetaLabel, dueBadge, fmtDueFull } from '../src/utils/todoSchedule.ts'

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
