/**
 * 生成节气数据表 src/utils/solarTermData.ts（**自动生成，勿手改**）：
 *   node scripts/gen-solar-terms.mjs
 *
 * 数据源：lunar-typescript（devDependency，天文算法）。逐日扫描 1900–2100 每一天，
 * 当天恰逢节气日则记名——结果逐年校验（24 个节气各出现一次、落在预期月份）后
 * 压缩为定长字符串：每年 24 项 × 2 位日号，顺序同 TERM_NAMES。
 */
import { createRequire } from 'node:module'
import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const { Solar } = require('lunar-typescript')

const TERM_NAMES = [
  '小寒', '大寒', '立春', '雨水', '惊蛰', '春分', '清明', '谷雨',
  '立夏', '小满', '芒种', '夏至', '小暑', '大暑', '立秋', '处暑',
  '白露', '秋分', '寒露', '霜降', '立冬', '小雪', '大雪', '冬至',
]
const TERM_MONTHS = [1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12]

const YEAR0 = 1900
const YEAR1 = 2100

/** 当天恰逢节气日返回节气名，否则 null（lunar-typescript 的 wholeDay 语义） */
function termOfDay(y, m, d) {
  const cur = Solar.fromYmd(y, m, d).getLunar().getCurrentJieQi(true)
  return cur ? cur.getName() : null
}

const chunks = []
for (let y = YEAR0; y <= YEAR1; y++) {
  const found = new Map()
  for (let m = 1; m <= 12; m++) {
    const days = new Date(y, m, 0).getDate()
    for (let d = 1; d <= days; d++) {
      const name = termOfDay(y, m, d)
      if (!name) continue
      if (found.has(name)) throw new Error(`${y} 年 ${name} 出现两次`)
      found.set(name, { m, d })
    }
  }
  if (found.size !== 24) throw new Error(`${y} 年只扫到 ${found.size} 个节气（应为 24）`)
  let chunk = ''
  for (let i = 0; i < 24; i++) {
    const hit = found.get(TERM_NAMES[i])
    if (!hit) throw new Error(`${y} 年缺节气 ${TERM_NAMES[i]}`)
    if (hit.m !== TERM_MONTHS[i]) {
      throw new Error(`${y} 年 ${TERM_NAMES[i]} 落在 ${hit.m} 月，预期 ${TERM_MONTHS[i]} 月`)
    }
    chunk += String(hit.d).padStart(2, '0')
  }
  chunks.push(chunk)
  process.stderr.write(`${y} ok\n`)
}

const table = chunks.join('')
const header = `\
/**
 * 节气数据表（1900–2100，公历年）——**自动生成，勿手改**。
 * 再生成：node scripts/gen-solar-terms.mjs（数据源 lunar-typescript 天文算法）。
 *
 * SOLAR_TERM_DAYS：每年 24 项 × 2 位日号，按 SOLAR_TERM_NAMES 顺序拼接（月份见
 * SOLAR_TERM_MONTHS，与索引一一对应）。取值：偏移 = (年 − 1900) × 24 + 节气序号，
 * 切 2 位字符转数字即当月日号。
 */
export const SOLAR_TERM_YEAR0 = ${YEAR0}

/** 节气名，索引即表内序号（每年从 1 月的小寒起，到 12 月的冬至止） */
export const SOLAR_TERM_NAMES = ${JSON.stringify(TERM_NAMES)}

/** 各节气所属公历月份（与 SOLAR_TERM_NAMES 同序） */
export const SOLAR_TERM_MONTHS = ${JSON.stringify(TERM_MONTHS)}

/** 1900–2100 全部节气日号，共 ${YEAR1 - YEAR0 + 1} × 24 × 2 = ${table.length} 字符 */
export const SOLAR_TERM_DAYS = '${table}'
`
const outPath = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'utils', 'solarTermData.ts')
writeFileSync(outPath, header, 'utf8')
process.stderr.write(`写入 ${outPath}（${table.length} 字符）\n`)
