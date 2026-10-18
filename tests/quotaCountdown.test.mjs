/**
 * 发布配额重置倒计时（src/utils/quotaCountdown.ts）单测。
 * 口径：小时级只到分钟，一小时以内到秒；到点/已过统一显示「即将重置」。
 *   node --test tests/quotaCountdown.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { formatResetClock, formatResetCountdown } from '../src/utils/quotaCountdown.ts'

const NOW = new Date(2026, 9, 17, 20, 0, 0).getTime()
const after = (seconds) => NOW + seconds * 1000

test('倒计时：小时级只到分钟', () => {
  assert.equal(formatResetCountdown(after(3 * 3600 + 25 * 60 + 12), NOW), '3 小时 25 分')
})

test('倒计时：一小时以内到秒', () => {
  assert.equal(formatResetCountdown(after(25 * 60 + 12), NOW), '25 分 12 秒')
})

test('倒计时：最后一分钟', () => {
  assert.equal(formatResetCountdown(after(12), NOW), '12 秒')
})

test('倒计时：整分/整时不留「0 秒 / 0 分」尾巴', () => {
  assert.equal(formatResetCountdown(after(60), NOW), '1 分')
  assert.equal(formatResetCountdown(after(2 * 3600), NOW), '2 小时')
})

test('倒计时：到点或已过 = 即将重置', () => {
  assert.equal(formatResetCountdown(NOW, NOW), '即将重置')
  assert.equal(formatResetCountdown(NOW - 1, NOW), '即将重置')
})

test('重置时刻：按客户端本地时区格式化成 HH:MM', () => {
  assert.equal(formatResetClock(new Date(2026, 9, 18, 0, 0, 0).getTime()), '00:00')
  assert.equal(formatResetClock(new Date(2026, 9, 18, 9, 5, 0).getTime()), '09:05')
})
