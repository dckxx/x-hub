/**
 * 把「带序号列表」的文本拆成多条待办正文。
 *
 * 规则（只靠序号，不靠分隔符）：
 *  - 序号形如 `1.` `1、` `1)` `1．`（数字 + 列表标点），标点后允许空格；
 *  - 序号需位于行首或空白/分隔符（`;；,，、`）之后；
 *  - 标点后不能紧跟数字，用于排除 `3.14`、`2.0` 这类小数/版本号；
 *  - 识别到 ≥2 个序号才拆分，拆出的每条去掉序号、只留正文；
 *    否则（无序号或只有 1 个序号）原样返回一条，不做任何改动。
 *
 * @example
 * parseTodoItems('1. 待办1 2. 待办2 3. 待办3') // ['待办1', '待办2', '待办3']
 * parseTodoItems('1. a; 2. b; 3. c;')          // ['a', 'b', 'c']
 * parseTodoItems('3.14 是圆周率')               // ['3.14 是圆周率']
 * parseTodoItems('随便写的一条')                // ['随便写的一条']
 */
export function parseTodoItems(text: string): string[] {
  const s = text.trim()
  if (!s) return []

  // 序号标记：行首/空白/分隔符 + 1~3 位数字 + 列表标点 + 可选空白，且标点后不是数字
  const markerRe = /(?:^|[\s;；,，、])(\d{1,3})\s*[.．、)]\s*(?!\d)/g

  const cutPoints: number[] = [] // 每个序号标记的起始位置（含其前导空白/分隔符）
  const starts: number[] = [] // 每条正文的起始位置（序号标记之后）
  let m: RegExpExecArray | null
  while ((m = markerRe.exec(s)) !== null) {
    cutPoints.push(m.index)
    starts.push(markerRe.lastIndex)
  }

  // 少于 2 个序号：不视为序号列表，整体当作一条
  if (starts.length < 2) return [s]

  const items: string[] = []
  for (let i = 0; i < starts.length; i++) {
    const stop = i + 1 < cutPoints.length ? cutPoints[i + 1] : s.length
    // 去掉末尾残留的分隔符/空白（如 "1. a; 2. b" 中 a 后的分号）
    const content = s.slice(starts[i], stop).replace(/[\s;；,，、]+$/, '').trim()
    if (content) items.push(content)
  }

  // 极端情况（如 "1. 2. 3." 全空）退化为原样一条
  return items.length > 0 ? items : [s]
}
