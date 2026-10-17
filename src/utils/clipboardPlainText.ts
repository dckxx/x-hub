import type { Fragment, Node as ProseNode, ResolvedPos, Slice } from '@milkdown/kit/prose/model'

/**
 * 所见即所得复制的纯文本序列化（速记编辑器 Ctrl+C / Ctrl+X 用）。
 *
 * 背景：Crepe 内置 milkdown 的 clipboard 插件，把 ProseMirror 的 clipboardTextSerializer
 * 覆盖成「选区序列化成 Markdown」——选区里只要含链接、多个段落等非纯文本节点，粘贴出去
 * 的就是 Markdown 源码：`\*`、`\[` 转义符、邮箱变 `[a@b.com](mailto:a@b.com)` 或 `<a@b.com>`。
 * 这里改按「屏幕上看到的样子」产纯文本：段落间空行、列表逐行带符号、代码块原样、链接只留
 * 可见文字、空行占位（零宽空格）剥掉。text/html 不受影响（仍走 ProseMirror 默认的 DOM
 * 序列化），粘贴到支持富文本的目标（Word / 富文本框）时加粗、链接等样式照旧保留。
 *
 * 结构分两层：核心 blocksToPlainText 只认下面的纯结构（node --test 可直接测，
 * 与 markdownBeautify 同一套约定）；ProseMirror 节点由 fragmentToPlainText 递归转换适配
 * （只 import type，运行时零依赖）。
 */

export interface PlainNode {
  name: string
  /** 文本节点内容 */
  text?: string
  /** 行内节点（非块） */
  inline?: boolean
  /** 文本块（段落/标题等，children 全为行内节点） */
  textblock?: boolean
  /** 代码块：内容按字面输出，不做行内处理 */
  code?: boolean
  attrs?: Record<string, unknown>
  children?: PlainNode[]
}

function isBlock(n: PlainNode): boolean {
  return n.inline !== true
}

/** 行内内容 → 纯文本：链接只留可见文字（mark 不进结构，天然丢弃）、硬换行落成换行、行内图片取 alt */
function inlinePlainText(nodes: PlainNode[] | undefined): string {
  if (!nodes) return ''
  let out = ''
  for (const n of nodes) {
    if (n.name === 'text') {
      out += n.text ?? ''
      continue
    }
    if (n.name === 'hard_break') {
      out += '\n'
      continue
    }
    if (n.name === 'image-inline') {
      const alt = n.attrs?.alt
      out += typeof alt === 'string' ? alt : ''
      continue
    }
    out += n.children?.length ? inlinePlainText(n.children) : n.text ?? ''
  }
  return out
}

/** 列表项标记：任务列表带勾选态、有序列表用条目 label（无则父级序号递增）、无序列表圆点 */
function listMarker(listName: string, item: PlainNode, order: number): string {
  const attrs = item.attrs ?? {}
  if (typeof attrs.checked === 'boolean') return attrs.checked ? '☑ ' : '☐ '
  if (listName === 'ordered_list') {
    const label = attrs.label
    return typeof label === 'string' && label.trim() ? `${label.trim()} ` : `${order}. `
  }
  return '• '
}

/** 列表项 → 若干行：首行带标记，项内后续段落/嵌套列表缩进两格 */
function itemLines(item: PlainNode, marker: string, indent: string): string[] {
  const out: string[] = []
  let first = true
  for (const child of item.children ?? []) {
    if (!isBlock(child)) continue
    if (child.name === 'bullet_list' || child.name === 'ordered_list') {
      out.push(...blockLines(child, indent + '  '))
      continue
    }
    const lines = blockLines(child, first ? indent : indent + '  ')
    if (first && lines.length) {
      lines[0] = indent + marker + lines[0].slice(indent.length)
      first = false
    }
    out.push(...lines)
  }
  if (first) out.push(indent + marker.trimEnd())
  return out
}

/** 表格 → 每行一条、单元格以制表符分隔（纯文本表格的通行形态，粘进 Excel 直接成分列数据） */
function tableLines(node: PlainNode): string[] {
  const rows: string[] = []
  const walk = (n: PlainNode) => {
    if (n.name === 'table_row' || n.name === 'table_header_row') {
      rows.push(
        (n.children ?? [])
          .map((cell) =>
            (cell.children ?? []).filter(isBlock).map((b) => blockLines(b, '').join(' ')).join(' '),
          )
          .join('\t'),
      )
      return
    }
    n.children?.forEach(walk)
  }
  walk(node)
  return rows
}

/** 块节点 → 若干行；顶层块与块之间由调用方以空行分隔（对应段落的视觉间距） */
function blockLines(node: PlainNode, indent: string): string[] {
  if (node.code) return (node.text ?? inlinePlainText(node.children)).split('\n').map((l) => indent + l)
  if (node.textblock) return inlinePlainText(node.children).split('\n').map((l) => indent + l)
  switch (node.name) {
    case 'bullet_list':
    case 'ordered_list': {
      const lines: string[] = []
      let order = typeof node.attrs?.order === 'number' ? node.attrs.order : 1
      for (const item of node.children ?? []) {
        lines.push(...itemLines(item, listMarker(node.name, item, order), indent))
        if (node.name === 'ordered_list') order += 1
      }
      return lines
    }
    case 'blockquote':
      // 引用条在纯文本里无法复现，退化为内部内容逐行
      return flattenBlocks(node.children, indent)
    case 'hr':
      return []
    case 'table':
    case 'table_header_row':
    case 'table_row':
      return tableLines(node)
    case 'image-block': {
      const caption = node.attrs?.caption
      const c = typeof caption === 'string' ? caption.trim() : ''
      return c ? [indent + c] : []
    }
  }
  // 兜底：未知块有子块按块铺平，行内子节点并成一行，叶子取文本（math 块等）
  if (node.children?.length) {
    if (node.children.every((c) => !isBlock(c))) return [indent + inlinePlainText(node.children)]
    return flattenBlocks(node.children, indent)
  }
  const t = node.text ?? ''
  return t ? t.split('\n').map((l) => indent + l) : []
}

/** 同级块铺平（引用内部等紧凑场景，块间单换行） */
function flattenBlocks(nodes: PlainNode[] | undefined, indent: string): string[] {
  const out: string[] = []
  for (const b of nodes ?? []) {
    if (!isBlock(b)) continue
    out.push(...blockLines(b, indent))
  }
  return out
}

/** 纯结构 → 纯文本。空行占位（零宽空格）整体剥掉：复制出去的空行就是真空行，不带隐形字符 */
export function blocksToPlainText(blocks: PlainNode[]): string {
  // 段内选区（纯行内片段）没有块级结构，直接行内序列化
  if (blocks.length > 0 && blocks.every((n) => !isBlock(n))) {
    return stripInvisible(inlinePlainText(blocks))
  }
  const groups: string[] = []
  for (const b of blocks) {
    if (!isBlock(b)) continue
    const lines = blockLines(b, '')
    groups.push(lines.join('\n'))
  }
  return stripInvisible(groups.join('\n\n'))
}

/** 零宽空格是空行占位符（见 NoteEditor normalizeEmptyParagraphs），复制出去必须剥离 */
function stripInvisible(text: string): string {
  return text.replace(/\u200B/g, '')
}

function toPlainNode(node: ProseNode): PlainNode {
  if (node.isText) return { name: 'text', text: node.text ?? '', inline: true }
  const attrs = node.attrs as Record<string, unknown>
  if (node.isLeaf) return { name: node.type.name, inline: !node.isBlock, attrs, text: node.textContent }
  if (node.isTextblock) {
    return {
      name: node.type.name,
      textblock: true,
      code: node.type.spec.code === true,
      attrs,
      text: node.textContent,
      children: childrenToPlain(node),
    }
  }
  return { name: node.type.name, attrs, children: childrenToPlain(node) }
}

function childrenToPlain(node: ProseNode): PlainNode[] | undefined {
  const out: PlainNode[] = []
  node.content.forEach((child) => {
    out.push(toPlainNode(child))
  })
  return out
}

/** ProseMirror 复制片段 → 所见即所得纯文本（挂到 view props 的 clipboardTextSerializer 用） */
export function fragmentToPlainText(fragment: Fragment): string {
  const plain: PlainNode[] = []
  fragment.forEach((child) => {
    plain.push(toPlainNode(child))
  })
  return blocksToPlainText(plain)
}

/**
 * 复制序列化总入口（clipboardTextSerializer 直接挂这个）。
 *
 * 按选区跨不跨块分流，对应「整体复制保持结构、单个内容复制不带格式」：
 * - 段内选区（起止在同一文本块，双击/拖选一个词一段字）=「只复制这一个内容」：
 *   只输出该块的行内文本，不带列表圆点/序号/勾选等任何块级标记——复制账号密码
 *   粘进登录框不能带上「• 」。⚠️ 不能直接序列化 slice：本工程 prosemirror-state 的
 *   TextSelection.content() 走基类 doc.slice(from, to, true)，slice 从文档层连着
 *   bullet_list → list_item → 段落整条祖先链，按结构走必带列表标记。
 * - 跨块选区（拖过多行、Ctrl+A 整体复制）=「复制一段/整篇」：保留块级结构
 *   （圆点、勾选态、嵌套缩进、表格制表符），粘出去仍是列表的样子。
 */
export function copyPlainText(slice: Slice, selection: { $from: ResolvedPos; $to: ResolvedPos }): string {
  const { $from, $to } = selection
  if ($from.sameParent($to) && $from.parent.isTextblock) {
    // parent.cut 只留选中那截行内内容（代码块 textblock 也走这里，按字面输出）
    return blocksToPlainText([toPlainNode($from.parent.cut($from.parentOffset, $to.parentOffset))])
  }
  return fragmentToPlainText(slice.content)
}
