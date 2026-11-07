/**
 * 农历转换（src/utils/lunar.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（模块零依赖）：
 *   node --test tests/lunar.test.mjs
 *
 * 锚点日均取有据可查的公历 ↔ 农历对应：春节 / 元宵 / 中秋 / 闰六月 / 建表基准日。
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  toLunar,
  lunarCellLabel,
  solarTermOf,
  festivalOf,
  calendarCellLabel,
  calendarDayLabel,
} from '../src/utils/lunar.ts'

test('建表基准日：1900-01-31 = 农历正月初一（庚子年 · 属鼠）', () => {
  const l = toLunar(new Date(1900, 0, 31))
  assert.ok(l)
  assert.equal(l.day, 1)
  assert.equal(l.month, 1)
  assert.equal(l.monthName, '正月')
  assert.equal(l.dayName, '初一')
  assert.equal(l.ganZhiYear, '庚子')
  assert.equal(l.zodiac, '鼠')
  assert.equal(l.isLeap, false)
})

test('2026 春节：2026-02-17 = 丙午马年正月初一，格子标签显月名「正月」', () => {
  const l = toLunar(new Date(2026, 1, 17))
  assert.ok(l)
  assert.equal(l.year, 2026)
  assert.equal(l.day, 1)
  assert.equal(l.ganZhiYear, '丙午')
  assert.equal(l.zodiac, '马')
  assert.equal(lunarCellLabel(new Date(2026, 1, 17)), '正月')
})

test('2026 春节次日：格子标签回落日名「初二」', () => {
  assert.equal(lunarCellLabel(new Date(2026, 1, 18)), '初二')
})

test('2025 元宵：2025-02-12 = 乙巳蛇年正月十五 → 「十五」', () => {
  assert.equal(lunarCellLabel(new Date(2025, 1, 12)), '十五')
})

test('2025 中秋：2025-10-06 = 八月十五 → 「十五」（月名不进格子标签）', () => {
  const l = toLunar(new Date(2025, 9, 6))
  assert.ok(l)
  assert.equal(l.monthName, '八月')
  assert.equal(l.dayName, '十五')
  assert.equal(lunarCellLabel(new Date(2025, 9, 6)), '十五')
})

test('2025 闰六月初一（2025-07-25）：格子标签显「闰六月」（最长的 3 字标签）', () => {
  const l = toLunar(new Date(2025, 6, 25))
  assert.ok(l)
  assert.equal(l.isLeap, true)
  assert.equal(l.month, 6)
  assert.equal(l.monthName, '闰六月')
  assert.equal(lunarCellLabel(new Date(2025, 6, 25)), '闰六月')
})

test('数据表范围外返回 null（1900-01-30 与 2102-01-01），调用方 v-if 守卫', () => {
  assert.equal(toLunar(new Date(1900, 0, 30)), null)
  // 表覆盖到农历 2100 年末（公历 2101 年初，如 2101-01-01 仍是腊月初二），2102 年起才真正超表
  assert.equal(toLunar(new Date(2102, 0, 1)), null)
  assert.equal(lunarCellLabel(new Date(2102, 0, 1)), null)
})

// ---- 节气（锚点均经 lunar-typescript 天文算法独立核对）----

test('节气命中：立春/清明/冬至等锚点日', () => {
  assert.equal(solarTermOf(new Date(2026, 1, 4)), '立春')
  assert.equal(solarTermOf(new Date(2025, 3, 4)), '清明')
  assert.equal(solarTermOf(new Date(2025, 11, 21)), '冬至')
  assert.equal(solarTermOf(new Date(2026, 0, 5)), '小寒')
  assert.equal(solarTermOf(new Date(2026, 8, 23)), '秋分')
})

test('节气边界与未命中：表首/表尾可查，非节气日与表外返回 null', () => {
  assert.equal(solarTermOf(new Date(1900, 1, 4)), '立春')
  assert.equal(solarTermOf(new Date(2100, 11, 22)), '冬至')
  assert.equal(solarTermOf(new Date(2026, 1, 5)), null)
  assert.equal(solarTermOf(new Date(2026, 5, 19)), null)
  assert.equal(solarTermOf(new Date(2101, 0, 5)), null)
})

// ---- 传统节日 ----

test('公历节日：元旦 / 劳动节 / 国庆节', () => {
  assert.equal(festivalOf(new Date(2026, 0, 1)), '元旦')
  assert.equal(festivalOf(new Date(2026, 4, 1)), '劳动节')
  assert.equal(festivalOf(new Date(2026, 9, 1)), '国庆节')
})

test('农历节日：春节 / 元宵 / 中秋（元宵日期按春节 +14 天构造校验）', () => {
  assert.equal(festivalOf(new Date(2026, 1, 17)), '春节')
  assert.equal(festivalOf(new Date(2026, 2, 3)), '元宵')
  assert.equal(festivalOf(new Date(2025, 9, 6)), '中秋')
})

test('除夕按「次日为正月初一」判定：腊月小无年三十的 2022 年（除夕 = 廿九）也要命中', () => {
  assert.equal(toLunar(new Date(2022, 0, 31))?.dayName, '廿九')
  assert.equal(festivalOf(new Date(2022, 0, 31)), '除夕')
})

test('闰月不过节：2023 闰二月初二不是龙抬头，正二月初二才是', () => {
  let leapHit = null
  let normalHit = null
  for (let d = new Date(2023, 1, 1); d <= new Date(2023, 3, 30); d.setDate(d.getDate() + 1)) {
    const l = toLunar(d)
    if (!l || l.month !== 2 || l.day !== 2) continue
    if (l.isLeap) leapHit = festivalOf(d)
    else normalHit = festivalOf(d)
  }
  assert.equal(leapHit, null)
  assert.equal(normalHit, '龙抬头')
})

// ---- 优先级与标签组装 ----

test('日历格标签优先级：节日 > 节气 > 农历日名', () => {
  // 春节：初一显月名的惯例让位给节日（不是「正月」）
  assert.equal(calendarCellLabel(new Date(2026, 1, 17)), '春节')
  // 立春：节气盖过日名
  assert.equal(calendarCellLabel(new Date(2026, 1, 4)), '立春')
  // 中秋：节日盖过「十五」
  assert.equal(calendarCellLabel(new Date(2025, 9, 6)), '中秋')
  // 无节日无节气：初一显月名、其余显日名
  assert.equal(calendarCellLabel(new Date(2026, 10, 9)), '十月')
  assert.equal(calendarCellLabel(new Date(2026, 10, 10)), '初二')
})

test('明细头部标签：节日/节气独立成立，普通日给完整农历', () => {
  assert.equal(calendarDayLabel(new Date(2026, 1, 17)), '春节')
  // 2026-11-07 恰为立冬：节气盖过完整农历
  assert.equal(calendarDayLabel(new Date(2026, 10, 7)), '立冬')
  // 十月初一（2026-11-09）次日：无节日无节气，给完整农历
  assert.equal(calendarDayLabel(new Date(2026, 10, 10)), '十月初二')
  // 公历节日不依赖农历表，表外年份照常成立
  assert.equal(calendarDayLabel(new Date(2102, 0, 1)), '元旦')
})
