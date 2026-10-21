/**
 * 扩展分类与官方标识的共享口径。
 *
 * 分类有两个来源，筛选时必须能对上：
 *   · 市场清单条目的 `category`（发布时作者自选、随 manifest 透传进清单）；
 *   · 已安装扩展 manifest.json 的 `category` 字段（同一通道落盘）。
 * 预设清单是**固定分类体系**（2026-10-14 定稿七类）：筛选行无论有没有扩展命中都全部展示；
 * 清单/manifest 里出现预设之外的分类名（历史数据、手写 manifest）也追加在后面，不能悄悄吞掉。
 * 没有「未分类」虚拟桶：没填分类的条目在筛选与展示上都归入「其他」（matchesCategory / displayCategory 同一口径）。
 */

/** 分类预设（固定七类，顺序即筛选行展示顺序） */
export const EXTENSION_CATEGORIES: readonly string[] = [
  'AI',
  '实用工具',
  '开发工具',
  '多媒体',
  '休闲游戏',
  '学习天地',
  '其他',
]

/** 「其他」类（预设之一；没填分类的条目归入它） */
export const CATEGORY_OTHER = '其他'

/** 筛选行的「全部」虚拟值 */
export const CATEGORY_ALL = '全部'

/**
 * 某批条目的分类筛选行取值：**预设七类固定全量展示**（顺序即预设顺序）
 * → 数据里预设之外的自定义分类（字典序）。无分类条目不产生独立筛选项（归入「其他」）。
 */
export function categoryFilterValues(cats: (string | null | undefined)[]): string[] {
  const present = new Set<string>()
  for (const c of cats) {
    const v = (c ?? '').trim()
    if (v) present.add(v)
  }
  const extras = [...present].filter((c) => !EXTENSION_CATEGORIES.includes(c)).sort()
  return [CATEGORY_ALL, ...EXTENSION_CATEGORIES, ...extras]
}

/** 条目是否命中当前筛选值（'全部' 放行一切；'其他' 额外兜住没填分类的条目，其余按名字精确匹配） */
export function matchesCategory(entryCat: string | null | undefined, selected: string): boolean {
  if (selected === CATEGORY_ALL) return true
  const v = (entryCat ?? '').trim()
  return selected === CATEGORY_OTHER ? !v || v === CATEGORY_OTHER : v === selected
}

/** 条目分类的展示文案（没填 = 归入「其他」，与筛选口径一致） */
export function displayCategory(entryCat: string | null | undefined): string {
  return (entryCat ?? '').trim() || CATEGORY_OTHER
}

/**
 * 官方扩展判定（市场卡片/详情、已安装卡片共用）。
 *
 * 数据面：服务端在发布时对官方自营账号（admin / OFFICIAL_PUBLISHERS）打 `official` 标记。
 * 兜底：`com.x-hub.*` 是**平台保留命名空间**（服务端关卡强制非官方不得使用），因此老清单
 * 没有该字段时，id 前缀即可判定官方——存量官方扩展不必等重新发布就有标识。
 */
export function isOfficialExt(m: { id: string; official?: boolean; required?: boolean }): boolean {
  return m.official === true || m.required === true || m.id.startsWith('com.x-hub.')
}

/** 安装次数展示文案（清单缺 installCount = 未知，显示占位） */
export function installCountText(n: number | null | undefined): string {
  if (typeof n !== 'number' || !Number.isFinite(n) || n < 0) return '—'
  if (n < 10000) return `${n} 次安装`
  return `${(n / 10000).toFixed(1)} 万次安装`
}
