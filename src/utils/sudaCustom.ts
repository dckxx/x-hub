import type { Resource, SudaCustomModuleConfig } from '../api/tauri'
import { categoryMatchesPath } from './subcategoryTree'

/**
 * 工作台「自定义速达」槽位（suda1..suda4）的共享口径。
 *
 * 槽位模式同便签 1/2：固定 4 个 module id（DASH_MODULES 各一条，布局层天然单实例），
 * 内容配置存 AppConfig.suda_custom_modules（前端整体落盘），删卡不删配置——
 * 重加同槽位内容还在。
 *
 * 本文件是唯一的内容过滤/排序实现：真实卡片（SudaCustomCard）与布局编辑器缩印
 * （DashModulePreview）都必须走这里——预览数字或顺序与真卡不符会被当成新 bug
 * （见 AGENTS.md 约定 27 预览口径铁律）。
 */

/** 固定槽位 id，与 DASH_MODULES / AppConfig.suda_custom_modules[].id 一一对应 */
export const SUDA_CUSTOM_IDS = ['suda1', 'suda2', 'suda3', 'suda4'] as const

/** 配置弹窗的来源选项（顺序即展示顺序） */
export const SUDA_CUSTOM_SOURCE_OPTIONS = [
  { value: 'pinned', label: '手动挑选' },
  { value: 'app', label: '应用' },
  { value: 'web', label: '网页' },
  { value: 'file', label: '文件' },
  { value: 'subcategory', label: '小类' },
] as const

/** 槽位内容的过滤/排序：
 * - pinned = resource_ids 顺序（勾选顺序即展示顺序；已删除的资源自动跳过）
 * - app / web / file = 整个大类，store 顺序（与速达页「应用/网页/文件」筛选同口径）
 * - subcategory = 小类路径命中配置值或为其子孙级（与速达页小类筛选同口径——选中目录
 *   展示其下全部资源；带「/」层级的小类见 utils/subcategoryTree.ts；未配置 = 空列表） */
export function sudaCustomItems(
  cfg: SudaCustomModuleConfig | undefined,
  resources: readonly Resource[],
): Resource[] {
  if (!cfg) return []
  switch (cfg.source) {
    case 'pinned': {
      const byId = new Map(resources.map((r) => [r.id, r]))
      return cfg.resource_ids
        .map((id) => byId.get(id))
        .filter((r): r is Resource => r !== undefined)
    }
    case 'app':
    case 'web':
    case 'file':
      return resources.filter((r) => r.kind === cfg.source)
    case 'subcategory':
      return cfg.subcategory
        ? resources.filter((r) => categoryMatchesPath(r.category, cfg.subcategory!))
        : []
    default:
      return []
  }
}

/** 槽位是否已配置出内容（决定卡片渲染内容网格还是空态引导） */
export function sudaCustomConfigured(cfg: SudaCustomModuleConfig | undefined): boolean {
  if (!cfg) return false
  switch (cfg.source) {
    case 'pinned':
      return cfg.resource_ids.length > 0
    case 'subcategory':
      return !!cfg.subcategory
    case 'app':
    case 'web':
    case 'file':
      return true
    default:
      return false
  }
}

/** 来源中文名（卡片空态提示与配置弹窗共用一个叫法） */
export function sudaCustomSourceLabel(cfg: SudaCustomModuleConfig | undefined): string {
  if (!cfg) return ''
  return SUDA_CUSTOM_SOURCE_OPTIONS.find((o) => o.value === cfg.source)?.label ?? ''
}
