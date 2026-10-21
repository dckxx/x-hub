/**
 * Markdown 行级扫描的共用地基（零依赖）：围栏 / `$$` 公式识别 + 行内代码分段。
 *
 * 从 markdownHtml.ts 抽出：所有「逐行改写 Markdown 但必须绕开代码」的规则
 * （序列化痕迹回收、分屏预览转义、一键美化）都要共用这一处围栏识别，
 * 禁止各写一套——判定口径漂移就会出现「这边动了代码那边没动」。
 */

export const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})(.*)$/

export type LineKind = 'fence' | 'math' | 'prose'

/** 围栏和 `$$` 公式整段原样跳过，只改普通行。所有行级规则共用这一处。 */
export function mapMarkdownLines(text: string, fn: (line: string, kind: LineKind) => string): string {
  const lines = text.split('\n')
  let fence: { char: string; len: number } | null = null
  let math = false
  const out = lines.map((line) => {
    const mark = FENCE_LINE.exec(line)
    if (mark && !(mark[1][0] === '`' && mark[2].includes('`'))) {
      const token = mark[1]
      const bare = mark[2].trim() === ''
      if (!fence) {
        fence = { char: token[0], len: token.length }
      } else if (fence.char === token[0] && token.length >= fence.len && bare) {
        fence = null
      }
      return fn(line, 'fence')
    }
    if (fence) return fn(line, 'fence')
    if (line.trim() === '$$') {
      math = !math
      return fn(line, 'math')
    }
    if (math) return fn(line, 'math')
    return fn(line, 'prose')
  })
  return out.join('\n')
}

/** 行内代码用成对反引号包住，中间的 `\*`、`<tag>` 是代码，不能按正文改。 */
export function mapInlineProse(line: string, fn: (prose: string) => string): string {
  const parts = line.split(/(`+)/)
  let inCode = false
  return parts
    .map((part) => {
      if (/^`+$/.test(part)) {
        inCode = !inCode
        return part
      }
      return inCode ? part : fn(part)
    })
    .join('')
}
