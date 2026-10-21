import type { ResourceSubcategory } from '../api/tauri'

/**
 * 速达小类的层级树（书签导入的「/」路径小类 → 逐级嵌套）。
 *
 * 数据层不变：`resource_subcategories.name` 仍是完整路径（如「开发/前端」，UNIQUE 按
 * 全路径），`resources.category` 存的也是全路径——树只是**展示层推导**（按「/」拆段），
 * 不落库、无迁移。顶层 chips 只出一层，更深层级经 children 下钻（浏览器书签目录形态）。
 */

export interface SubcatNode {
  /** 完整路径 = 该层对应的小类全名（如「开发/前端」），筛选/归类都以它为口径 */
  path: string
  /** 本段名（如「前端」），chips 与级联菜单里展示用 */
  name: string
  children: SubcatNode[]
  /** 是否恰好有一条同全路径的小类行且为默认小类（星标口径与旧 chips 一致） */
  isDefault: boolean
}

/**
 * 小类行集合 → 顶层节点树。节点顺序按行表顺序中首次出现排列；隐式祖先节点
 * （只有「开发/前端」行而没有「开发」行时的「开发」）按子行首次出现的位置插入。
 */
export function buildSubcategoryTree(subs: readonly ResourceSubcategory[]): SubcatNode[] {
  const roots: SubcatNode[] = []
  const byPath = new Map<string, SubcatNode>()
  for (const s of subs) {
    const segs = s.name.split('/').map((x) => x.trim()).filter(Boolean)
    if (segs.length === 0) continue
    let path = ''
    let parent: SubcatNode | null = null
    for (const seg of segs) {
      path = path ? `${path}/${seg}` : seg
      let node = byPath.get(path)
      if (!node) {
        node = { path, name: seg, children: [], isDefault: false }
        byPath.set(path, node)
        if (parent) parent.children.push(node)
        else roots.push(node)
      }
      parent = node
    }
    // 全路径完全一致的行才把默认星标带上来（中间推导节点不带）
    if (parent && parent.path === s.name && s.is_default) parent.isDefault = true
  }
  return roots
}

/** 路径的末段名（空态文案等展示用） */
export function subcatLeaf(path: string): string {
  const segs = path.split('/').map((x) => x.trim()).filter(Boolean)
  return segs.length > 0 ? segs[segs.length - 1] : path
}

/** 资源的小类是否归属某路径（精确命中或为其子孙级——选中目录展示其下全部资源） */
export function categoryMatchesPath(category: string | null | undefined, path: string): boolean {
  if (!category) return false
  return category === path || category.startsWith(`${path}/`)
}
