/**
 * 源码 / 分屏里，光标在行尾按回车时补上对应结构。
 * 实时预览用同一套行文本判断，再交给编辑器变成节点。
 */

export type LineShortcut =
  | { type: 'code'; language: string }
  | { type: 'math' }
  | { type: 'image'; caption: string; src: string }
  | { type: 'table-size'; cols: number; rows: number }
  | { type: 'table-row'; cells: string[] }
  | { type: 'heading'; level: number; text: string; prefix: number }
  | { type: 'blockquote'; text: string }
  | { type: 'bullet'; text: string }
  | { type: 'ordered'; order: number; text: string }
  | { type: 'task'; checked: boolean; text: string }
  | { type: 'hr' }

const MAX_COLS = 10
const MAX_ROWS = 20

export function expandOnEnter(
  value: string,
  cursor: number,
): { value: string; cursor: number } | null {
  const lineStart = value.lastIndexOf('\n', cursor - 1) + 1
  const lineBreak = value.indexOf('\n', cursor)
  const lineEnd = lineBreak === -1 ? value.length : lineBreak
  if (cursor !== lineEnd) return null
  const line = value.slice(lineStart, lineEnd)
  if (insideCodeFence(value, lineStart)) return null

  if (/^```[A-Za-z0-9_+-]*$/.test(line)) {
    if (nextContentLine(value, lineEnd)?.startsWith('```')) return null
    return insertAfter(value, lineEnd, '\n\n```', 1)
  }
  if (line === '$$') {
    if (insideMathBlock(value, lineStart)) return null
    if (nextContentLine(value, lineEnd) === '$$') return null
    return insertAfter(value, lineEnd, '\n\n$$', 1)
  }

  const image = incompleteImage(line)
  if (image) return replaceLine(value, lineStart, lineEnd, image.text, image.cursor)

  const size = tableSize(line)
  if (size) {
    const text = emptyTable(size.cols, Math.max(size.rows, 2))
    return replaceLine(value, lineStart, lineEnd, text, 2)
  }

  const cells = pipeRow(line)
  const prev = previousLine(value, lineStart)
  if (
    cells &&
    !isSeparator(cells) &&
    !isSeparatorLine(nextContentLine(value, lineEnd)) &&
    !(prev && pipeRow(prev))
  ) {
    const sep = pipeLine(cells.map(() => '---'))
    const blank = pipeLine(cells.map(() => ''))
    return insertAfter(value, lineEnd, `\n${sep}\n${blank}`, 1 + sep.length + 1 + 2)
  }
  return null
}

/** 整行就是快捷标记时，实时预览按回车转成对应节点。已写完的图片语法也算。 */
export function matchWysiwygLine(line: string): LineShortcut | null {
  const code = /^```([A-Za-z0-9_+-]*)$/.exec(line)
  if (code) return { type: 'code', language: code[1] ?? '' }
  if (line === '$$') return { type: 'math' }

  const image = imageShortcut(line)
  if (image) return { type: 'image', caption: image.caption, src: image.src }

  const size = tableSize(line)
  if (size) return { type: 'table-size', cols: size.cols, rows: Math.max(size.rows, 2) }

  const cells = pipeRow(line)
  if (cells && !isSeparator(cells)) return { type: 'table-row', cells }
  return blockShortcut(line)
}

/** 整行是标题、引用、列表或分隔线时，回车转成对应块（Milkdown 原本只认标记后的空格）。 */
function blockShortcut(line: string): LineShortcut | null {
  const task = /^[-+*]\s+\[([ xX])\](?:\s+(.*))?$/.exec(line)
  if (task) return { type: 'task', checked: task[1].toLowerCase() === 'x', text: task[2] ?? '' }

  const heading = /^(#{1,6})(?:\s+(.*))?$/.exec(line)
  if (heading) {
    const text = heading[2] ?? ''
    return {
      type: 'heading',
      level: heading[1].length,
      text,
      prefix: text ? line.length - text.length : line.length,
    }
  }

  if (/^(-{3,}|\*{3,}|_{3,})$/.test(line)) return { type: 'hr' }

  const quote = /^>\s*(.*)$/.exec(line)
  if (quote) return { type: 'blockquote', text: quote[1] ?? '' }

  const ordered = /^(\d{1,9})\.(?:\s+(.*))?$/.exec(line)
  if (ordered) {
    const order = Number(ordered[1])
    if (order >= 0) return { type: 'ordered', order, text: ordered[2] ?? '' }
  }

  const bullet = /^[-+*](?:\s+(.*))?$/.exec(line)
  if (bullet) return { type: 'bullet', text: bullet[1] ?? '' }
  return null
}

function insertAfter(
  value: string,
  lineEnd: number,
  insertion: string,
  cursorOffset: number,
): { value: string; cursor: number } {
  return {
    value: value.slice(0, lineEnd) + insertion + value.slice(lineEnd),
    cursor: lineEnd + cursorOffset,
  }
}

function replaceLine(
  value: string,
  lineStart: number,
  lineEnd: number,
  text: string,
  cursorInText: number,
): { value: string; cursor: number } {
  return {
    value: value.slice(0, lineStart) + text + value.slice(lineEnd),
    cursor: lineStart + cursorInText,
  }
}

function insideCodeFence(value: string, lineStart: number): boolean {
  let open = false
  for (const line of value.slice(0, lineStart).split('\n')) {
    if (line.startsWith('```')) open = !open
  }
  return open
}

function insideMathBlock(value: string, lineStart: number): boolean {
  let open = false
  for (const line of value.slice(0, lineStart).split('\n')) {
    if (line === '$$') open = !open
  }
  return open
}

function previousLine(value: string, lineStart: number): string | null {
  if (lineStart === 0) return null
  const prevEnd = lineStart - 1
  const prevStart = value.lastIndexOf('\n', prevEnd - 1) + 1
  return value.slice(prevStart, prevEnd)
}

function nextContentLine(value: string, lineEnd: number): string | null {
  if (lineEnd >= value.length) return null
  for (const line of value.slice(lineEnd + 1).split('\n')) {
    if (line.length > 0) return line
  }
  return null
}

function incompleteImage(line: string): { text: string; cursor: number } | null {
  if (line === '!' || line === '![') return { text: '![]()', cursor: 4 }
  const altOpen = /^!\[([^\]]*)$/.exec(line)
  if (altOpen) {
    const text = `![${altOpen[1]}]()`
    return { text, cursor: text.length - 1 }
  }
  const altClosed = /^!\[([^\]]*)\]$/.exec(line)
  if (altClosed) {
    const text = `![${altClosed[1]}]()`
    return { text, cursor: text.length - 1 }
  }
  const srcOpen = /^!\[([^\]]*)\]\(([^)]*)$/.exec(line)
  if (srcOpen) {
    const text = `![${srcOpen[1]}](${srcOpen[2]})`
    return { text, cursor: text.length - 1 }
  }
  return null
}

function imageShortcut(line: string): { caption: string; src: string } | null {
  if (line === '!' || line === '![') return { caption: '', src: '' }
  const altOpen = /^!\[([^\]]*)$/.exec(line)
  if (altOpen) return { caption: altOpen[1], src: '' }
  const altClosed = /^!\[([^\]]*)\]$/.exec(line)
  if (altClosed) return { caption: altClosed[1], src: '' }
  const full = /^!\[([^\]]*)\]\(([^)]*)\)$/.exec(line)
  if (full) return { caption: full[1] ?? '', src: full[2] ?? '' }
  const srcOpen = /^!\[([^\]]*)\]\(([^)]*)$/.exec(line)
  if (srcOpen) return { caption: srcOpen[1] ?? '', src: srcOpen[2] ?? '' }
  return null
}

function tableSize(line: string): { cols: number; rows: number } | null {
  const matched = /^\|(\d+)[xX](\d+)\|$/.exec(line)
  if (!matched) return null
  const cols = Number(matched[1])
  const rows = Number(matched[2])
  if (cols < 1 || rows < 1 || cols > MAX_COLS || rows > MAX_ROWS) return null
  return { cols, rows }
}

function pipeRow(line: string): string[] | null {
  if (!line.startsWith('|') || !line.endsWith('|') || line.length < 2) return null
  const cells = line.slice(1, -1).split('|').map((cell) => cell.trim())
  if (cells.length < 2) return null
  return cells
}

function isSeparator(cells: string[]): boolean {
  return cells.every((cell) => /^:?-{3,}:?$/.test(cell))
}

function isSeparatorLine(line: string | null): boolean {
  if (!line) return false
  const cells = pipeRow(line)
  return cells != null && isSeparator(cells)
}

function pipeLine(cells: string[]): string {
  return `| ${cells.join(' | ')} |`
}

function emptyTable(cols: number, rows: number): string {
  const blank = pipeLine(Array.from({ length: cols }, () => ''))
  const sep = pipeLine(Array.from({ length: cols }, () => '---'))
  const body = Array.from({ length: rows - 1 }, () => blank)
  return [blank, sep, ...body].join('\n')
}
