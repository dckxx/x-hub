/**
 * 农历（阴历）转换工具：公历日期 → 农历年月日 + 干支/生肖 + 节气 + 传统节日。
 * 采用经典数据表算法（1900–2100），lunarInfo[y-1900] 位编码当年闰月与大小月。
 * 节气来自 scripts/gen-solar-terms.mjs 生成的同范围数据表（solarTermData.ts）。
 * 仅用于展示「今日阴阳历」与日历格标签等形态，无外部依赖。
 */
import {
  SOLAR_TERM_DAYS,
  SOLAR_TERM_MONTHS,
  SOLAR_TERM_NAMES,
  SOLAR_TERM_YEAR0,
} from './solarTermData.ts'

// 1900–2100 共 201 项。每项含义：
//   bit 15..4  某月是否大月(30天)
//   bit 3..0   闰月月份（0 = 无闰月）
//   0x10000    闰月是否大月
const LUNAR_INFO = [
  0x04bd8, 0x04ae0, 0x0a570, 0x054d5, 0x0d260, 0x0d950, 0x16554, 0x056a0, 0x09ad0, 0x055d2,
  0x04ae0, 0x0a5b6, 0x0a4d0, 0x0d250, 0x1d255, 0x0b540, 0x0d6a0, 0x0ada2, 0x095b0, 0x14977,
  0x04970, 0x0a4b0, 0x0b4b5, 0x06a50, 0x06d40, 0x1ab54, 0x02b60, 0x09570, 0x052f2, 0x04970,
  0x06566, 0x0d4a0, 0x0ea50, 0x06e95, 0x05ad0, 0x02b60, 0x186e3, 0x092e0, 0x1c8d7, 0x0c950,
  0x0d4a0, 0x1d8a6, 0x0b550, 0x056a0, 0x1a5b4, 0x025d0, 0x092d0, 0x0d2b2, 0x0a950, 0x0b557,
  0x06ca0, 0x0b550, 0x15355, 0x04da0, 0x0a5d0, 0x14573, 0x052d0, 0x0a9a8, 0x0e950, 0x06aa0,
  0x0aea6, 0x0ab50, 0x04b60, 0x0aae4, 0x0a570, 0x05260, 0x0f263, 0x0d950, 0x05b57, 0x056a0,
  0x096d0, 0x04dd5, 0x04ad0, 0x0a4d0, 0x0d4d4, 0x0d250, 0x0d558, 0x0b540, 0x0b5a0, 0x195a6,
  0x095b0, 0x049b0, 0x0a974, 0x0a4b0, 0x0b27a, 0x06a50, 0x06d40, 0x0af46, 0x0ab60, 0x09570,
  0x04af5, 0x04970, 0x064b0, 0x074a3, 0x0ea50, 0x06b58, 0x055c0, 0x0ab60, 0x096d5, 0x092e0,
  0x0c960, 0x0d954, 0x0d4a0, 0x0da50, 0x07552, 0x056a0, 0x0abb7, 0x025d0, 0x092d0, 0x0cab5,
  0x0a950, 0x0b4a0, 0x0baa4, 0x0ad50, 0x055d9, 0x04ba0, 0x0a5b0, 0x15176, 0x052b0, 0x0a930,
  0x07954, 0x06aa0, 0x0ad50, 0x05b52, 0x04b60, 0x0a6e6, 0x0a4e0, 0x0d260, 0x0ea65, 0x0d530,
  0x05aa0, 0x076a3, 0x096d0, 0x04afb, 0x04ad0, 0x0a4d0, 0x1d0b6, 0x0d250, 0x0d520, 0x0dd45,
  0x0b5a0, 0x056d0, 0x055b2, 0x049b0, 0x0a577, 0x0a4b0, 0x0aa50, 0x1b255, 0x06d20, 0x0ada0,
  0x14b63, 0x09370, 0x049f8, 0x04970, 0x064b0, 0x168a6, 0x0ea50, 0x06b20, 0x1a6c4, 0x0aae0,
  0x092e0, 0x0d2e3, 0x0c960, 0x0d557, 0x0d4a0, 0x0da50, 0x05d55, 0x056a0, 0x0a6d0, 0x055d4,
  0x052d0, 0x0a9b8, 0x0a950, 0x0b4a0, 0x0b6a6, 0x0ad50, 0x055a0, 0x0aba4, 0x0a5b0, 0x052b0,
  0x0b273, 0x06930, 0x07337, 0x06aa0, 0x0ad50, 0x14b55, 0x04b60, 0x0a570, 0x054e4, 0x0d160,
  0x0e968, 0x0d520, 0x0daa0, 0x16aa6, 0x056d0, 0x04ae0, 0x0a9d4, 0x0a2d0, 0x0d150, 0x0f252,
  0x0d520,
]

const HEAVENLY_STEMS = ['甲', '乙', '丙', '丁', '戊', '己', '庚', '辛', '壬', '癸']
const EARTHLY_BRANCHES = ['子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戌', '亥']
const ZODIAC = ['鼠', '牛', '虎', '兔', '龙', '蛇', '马', '羊', '猴', '鸡', '狗', '猪']
const MONTH_NAMES = ['正', '二', '三', '四', '五', '六', '七', '八', '九', '十', '冬', '腊']
const DAY_NAMES = [
  '初一', '初二', '初三', '初四', '初五', '初六', '初七', '初八', '初九', '初十',
  '十一', '十二', '十三', '十四', '十五', '十六', '十七', '十八', '十九', '二十',
  '廿一', '廿二', '廿三', '廿四', '廿五', '廿六', '廿七', '廿八', '廿九', '三十',
]

/** 基准日：农历 1900-01-01 = 公历 1900-01-31 */
const BASE_MS = new Date(1900, 0, 31).getTime()

function leapMonth(y: number): number {
  return LUNAR_INFO[y - 1900] & 0xf
}

function leapDays(y: number): number {
  return leapMonth(y) ? (LUNAR_INFO[y - 1900] & 0x10000 ? 30 : 29) : 0
}

function monthDays(y: number, m: number): number {
  return LUNAR_INFO[y - 1900] & (0x10000 >> m) ? 30 : 29
}

function lunarYearDays(y: number): number {
  let sum = 348
  for (let i = 0x8000; i > 0x8; i >>= 1) {
    sum += LUNAR_INFO[y - 1900] & i ? 1 : 0
  }
  return sum + leapDays(y)
}

export interface LunarDate {
  year: number
  /** 农历月份 1–12 */
  month: number
  /** 农历月名，如「七月」；闰月前缀「闰」 */
  monthName: string
  /** 农历日名，如「廿五」 */
  dayName: string
  /** 农历日数 1–30 */
  day: number
  isLeap: boolean
  /** 干支年，如「丙午」 */
  ganZhiYear: string
  /** 生肖，如「马」 */
  zodiac: string
  /** 简短标题，如「七月廿五」 */
  short: string
}

/** 公历 Date → 农历（当地时区） */
export function toLunar(date: Date): LunarDate | null {
  let offset = Math.floor((date.getTime() - BASE_MS) / 86400000)
  if (offset < 0) return null

  // 逐年扣除天数定位农历年；循环退出后 y 指向 offset 落入的那一年（正月初一恰为边界时 y 即新年）
  let y = 1900
  let days = 0
  for (; y < 2101 && offset > 0; y++) {
    days = lunarYearDays(y)
    offset -= days
  }
  // 2101 年及以后超出数据表范围：与 1900 前同款返回 null（调用方 v-if 守卫），避免脏数据渲染
  if (offset > 0) return null
  if (offset < 0) {
    offset += days
    y--
  }
  const year = y

  let isLeap = false
  let i
  let monthDaysCount = 0
  for (i = 1; i < 13 && offset > 0; i++) {
    if (leapMonth(year) > 0 && i === leapMonth(year) + 1 && !isLeap) {
      --i
      isLeap = true
      monthDaysCount = leapDays(year)
    } else {
      monthDaysCount = monthDays(year, i)
    }
    if (isLeap && i === leapMonth(year) + 1) isLeap = false
    offset -= monthDaysCount
  }
  if (offset === 0 && leapMonth(year) > 0 && i === leapMonth(year) + 1) {
    if (isLeap) isLeap = false
    else {
      isLeap = true
      --i
    }
  }
  if (offset < 0) {
    offset += monthDaysCount
    --i
  }
  const month = i
  const day = offset + 1

  return {
    year,
    month,
    monthName: `${isLeap ? '闰' : ''}${MONTH_NAMES[month - 1]}月`,
    dayName: DAY_NAMES[day - 1],
    day,
    isLeap,
    ganZhiYear: `${HEAVENLY_STEMS[(year - 4) % 10]}${EARTHLY_BRANCHES[(year - 4) % 12]}`,
    zodiac: ZODIAC[(year - 4) % 12],
    short: `${MONTH_NAMES[month - 1]}月${DAY_NAMES[day - 1].replace(/^初/, '')}`,
  }
}

/**
 * 日历格用短标签（月历惯例）：农历每月初一显月名（「七月」/「闰七月」），其余显日名（「初二…三十」）。
 * 1900–2100 之外返回 null（toLunar 同口径），调用方 v-if 守卫。
 */
export function lunarCellLabel(date: Date): string | null {
  const l = toLunar(date)
  if (!l) return null
  return l.day === 1 ? l.monthName : l.dayName
}

// ---- 节气（表见 solarTermData.ts，公历 1900–2100）----

/** 当天恰逢节气日返回节气名（「立春」「冬至」…），否则 null。1900–2100 之外一律 null。 */
export function solarTermOf(date: Date): string | null {
  const y = date.getFullYear()
  const offset = y - SOLAR_TERM_YEAR0
  const totalYears = SOLAR_TERM_DAYS.length / 24 / 2
  if (offset < 0 || offset >= totalYears) return null
  const m = date.getMonth() + 1
  const d = date.getDate()
  for (let i = 0; i < 24; i++) {
    if (SOLAR_TERM_MONTHS[i] !== m) continue
    const start = (offset * 24 + i) * 2
    if (Number(SOLAR_TERM_DAYS.slice(start, start + 2)) === d) return SOLAR_TERM_NAMES[i]
  }
  return null
}

// ---- 传统节日（农历节日 + 固定公历节日；法定放假安排是逐年行政数据，不在此表）----

const LUNAR_FESTIVALS: Record<string, string> = {
  '1-1': '春节',
  '1-15': '元宵',
  '2-2': '龙抬头',
  '5-5': '端午',
  '7-7': '七夕',
  '8-15': '中秋',
  '9-9': '重阳',
  '12-8': '腊八',
}
const SOLAR_FESTIVALS: Record<string, string> = {
  '1-1': '元旦',
  '3-8': '妇女节',
  '3-12': '植树节',
  '5-1': '劳动节',
  '6-1': '儿童节',
  '9-10': '教师节',
  '10-1': '国庆节',
}

/** 当天恰逢传统节日返回节日名，否则 null。闰月不重复过节（闰五月的初五不是端午）。 */
export function festivalOf(date: Date): string | null {
  const solarHit = SOLAR_FESTIVALS[`${date.getMonth() + 1}-${date.getDate()}`] ?? null
  const l = toLunar(date)
  let lunarHit: string | null = null
  if (l) {
    // 除夕 = 农历年最后一天（腊月大三十、小廿九）：「次日恰为正月初一」直接判定，不依赖大小月；
    // 放在 isLeap 早退之前——极罕见的闰腊月最后一天同样是除夕
    if (l.month === 12) {
      const next = toLunar(new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1))
      if (next && next.month === 1 && next.day === 1) lunarHit = '除夕'
    }
    if (!lunarHit && !l.isLeap) lunarHit = LUNAR_FESTIVALS[`${l.month}-${l.day}`] ?? null
  }
  // 同日撞车（如 2020-10-01 国庆×中秋）农历节日胜出：公历节日人人皆知，农历错过日历就没了
  return lunarHit ?? solarHit
}

/**
 * 日历格标签，优先级：节日 > 节气 > 初一显月名 > 农历日名。
 * 单坑位格子不可能同显两条，撞车时按此序取舍；全不命中返回 null（1900–2100 之外）。
 */
export function calendarCellLabel(date: Date): string | null {
  return festivalOf(date) ?? solarTermOf(date) ?? lunarCellLabel(date)
}

/** 日历「当天明细」头部标签：节日 > 节气 > 完整农历（「九月廿九」）——头部有完整公历日期，节名可独立成立 */
export function calendarDayLabel(date: Date): string | null {
  const head = festivalOf(date) ?? solarTermOf(date)
  if (head) return head
  const l = toLunar(date)
  return l ? `${l.monthName}${l.dayName}` : null
}
