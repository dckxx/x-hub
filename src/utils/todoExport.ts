/**
 * 待办视图「一键导出」的 Markdown 组装纯函数。
 * 输入是视图层（TodoView）按当前筛选（范围/标签/未完成/已完成）过滤好的结构化数据，
 * 这里只负责排版：分组标题 + 复选框清单 + 元信息圆括号 + 子待办缩进。
 * 零依赖（同 markdownBeautify 的单测口径，node --test 直接加载 TS 源），
 * Todo 相关类型与格式化（dueBadge/repeatLabel…）留在视图层组装成 badges 字符串。
 */

export interface TodoExportItem {
  title: string
  done: boolean
  /** 圆括号内的元信息片段（截止/完成时间/优先级/周期），按传入顺序以「 · 」连接 */
  badges: string[]
  /** 标签名，导出为「#名字」追加在 badges 之后 */
  tagNames: string[]
  /** 子待办（递归缩进两格，同款复选框语法） */
  children: TodoExportItem[]
  /** 描述（轻量 Markdown 原文；空串/纯空白省略，逐行缩进两格承接在条目下） */
  description?: string
}

export interface TodoExportGroup {
  label: string
  items: TodoExportItem[]
}

export interface TodoExportMeta {
  /** H1 范围名：今天 / 本周 / 本月 / 全部 */
  scope: string
  /** 导出时刻文案，如「2026年11月7日 14:30」 */
  time: string
  /** 筛选维度描述片段（未完成/已完成、标签：…）；空数组 = 无补充说明 */
  filters: string[]
}

/** 单条待办 → 行数组（含描述与子待办）。depth 决定缩进档，顶级为 0 */
function itemLines(item: TodoExportItem, depth: number): string[] {
  const indent = '  '.repeat(depth)
  // 排版口径：时间/优先级/周期留在标题后的圆括号里（一个视觉单元），
  // 标签移出括号、以 #hashtag 跟在行尾——分类语义独立，阅读器里也可点击跳转
  const badge = item.badges.length ? `（${item.badges.join(' · ')}）` : ''
  const tags = item.tagNames.length ? ` ${item.tagNames.map((n) => `#${n}`).join(' ')}` : ''
  const head = `${indent}- [${item.done ? 'x' : ' '}] ${item.title.trim()}${badge}${tags}`
  const lines = [head]
  const desc = (item.description ?? '').trim()
  if (desc) {
    // 空一行再按「条目内容缩进档」（比标记深一档）承接，渲染器里才是独立段落
    //（否则会被吞进条目所在行的同一段文字）
    lines.push('')
    const bodyIndent = '  '.repeat(depth + 1)
    for (const l of desc.split('\n')) lines.push(`${bodyIndent}${l.trimEnd()}`)
  }
  for (const c of item.children) lines.push(...itemLines(c, depth + 1))
  return lines
}

/** 组装整份 Markdown。条目为空时输出头部 + 空态说明（按钮置灰是常态防线，这里只兜底） */
export function buildTodosMarkdown(meta: TodoExportMeta, groups: TodoExportGroup[]): string {
  const total = groups.reduce((n, g) => n + g.items.length, 0)
  const filters = meta.filters.length ? ` · ${meta.filters.join(' · ')}` : ''
  const lines: string[] = [
    `# 待办导出 · ${meta.scope}`,
    '',
    `> ${meta.time} 导出${filters} · 共 ${total} 条`,
  ]
  for (const g of groups) {
    if (!g.items.length) continue
    lines.push('', `## ${g.label}（${g.items.length} 条）`, '')
    for (const item of g.items) lines.push(...itemLines(item, 0))
  }
  if (!total) lines.push('', '（当前筛选下没有待办）')
  return `${lines.join('\n')}\n`
}
