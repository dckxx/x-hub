// 第二级：由 entry.js 的模块 URL 解析（修复前丢扩展前缀 → 404）
import { level2 } from './level2.js'

export async function level1() {
  return 'level1(' + (await level2()) + ')'
}
