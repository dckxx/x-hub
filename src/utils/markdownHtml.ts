import DOMPurify from 'dompurify'
import { marked, Renderer } from 'marked'

/**
 * 轻量 Markdown → 安全 HTML（待办描述等**只读展示**用）。
 *
 * 与 `utils/markdown.ts`（纯正则的纯文本化，零依赖）分开两个模块：那边被笔记列表 /
 * 全局搜索等主路径静态引用，这里带 marked + DOMPurify 两个库，混在一起会让它们
 * 被拖进主包与浮窗包；调用方按需动态 import 本模块（见 TodoRow 的悬浮展示）。
 *
 * ⚠️ 必须过 DOMPurify：描述可能来自扩展桥（**不可信内容**），marked 默认不过滤内联 HTML，
 * 而主窗口没有 CSP —— 直接 v-html 时，内容里的 `<img onerror>` / `<svg onload>`
 * 就能执行任意脚本并调用应用的本地命令（同 ChatPanel 对模型输出的处理）。
 *
 * 结果按原文缓存：悬浮展示会反复渲染同一条描述，避免每次鼠标移动都重解析。
 */
const cache = new Map<string, string>()
const CACHE_MAX = 50

export function renderMarkdown(text: string): string {
  if (!text) return ''
  const hit = cache.get(text)
  if (hit != null) return hit
  // breaks: 单个换行即换行（描述多是手写多行文本，不按 Markdown 段落规则合并）
  const html = DOMPurify.sanitize(
    marked.parse(text, { async: false, breaks: true, gfm: true }) as string,
  )
  if (cache.size >= CACHE_MAX) cache.clear()
  cache.set(text, html)
  return html
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/** 围栏代码块带上语言标签和独立底色。默认 `<pre>` 用卡片浅底，叠在预览白底上几乎看不见。 */
const noteRenderer = new Renderer()
noteRenderer.code = ({ text, lang }) => {
  const language = (lang ?? '').trim()
  const label = language ? `<div class="md-code-lang">${escapeHtml(language)}</div>` : ''
  return `<div class="md-code">${label}<pre><code>${escapeHtml(text)}</code></pre></div>`
}

const BR_LINE = /^[ \t]*<br\s*\/?\s*>[ \t]*$/i
const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})(.*)$/
/** Crepe 把分隔线一律写成 `***`。实时预览里用户看到的仍是分隔线，源码应回到 `---`。 */
const HR_STAR = /^[ \t]*(?:\*\*\*|\* \* \*)[ \t]*$/
/** 无序列表标记。只认星号后的空格，避免把行首强调 `*强调*` 改成列表。 */
const LIST_STAR = /^([ \t]*)\* /
/** 空列表项里 Crepe 塞的占位 `<br />`，例如 `* [x] <br />`。 */
const LIST_BR = /^([ \t]*-[ \t]+(?:\[[ xX]\][ \t]+)?)[ \t]*<br\s*\/?\s*>[ \t]*$/i
/** 会开 HTML 标签的 `<`。`1 < 2` 和自动链接 `<https://…>` 不在此列。 */
const RAW_TAG = /<\/?[A-Za-z][^>\n]*>/g

type LineKind = 'fence' | 'math' | 'prose'

/** 围栏和 `$$` 公式整段原样跳过，只改普通行。两边规则必须共用这一处，避免各写一套围栏识别。 */
function mapMarkdownLines(text: string, fn: (line: string, kind: LineKind) => string): string {
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
function mapInlineProse(line: string, fn: (prose: string) => string): string {
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

/**
 * 单独成行的 `<br>` 在 CommonMark 里会开启 HTML 块，一直吞到下一个空行，
 * 后面的围栏、标题因此变成普通文本。换成空行后，后面的块按正常 Markdown 解析。
 * 行内的 `hello<br />world` 不动。围栏和 `$$` 公式里的 `<br />` 是正文，也不能删。
 */
export function loosenHtmlBreaks(text: string): string {
  if (!/<br\b/i.test(text)) return text
  return mapMarkdownLines(text, (line, kind) => (kind === 'prose' && BR_LINE.test(line) ? '' : line))
}

/**
 * Crepe 读回 Markdown 时会改写法：`[链接]` 变成 `\[链接]`，`5 * 3` 变成 `5 \* 3`，
 * 无序列表从 `-` 变成 `*`，分隔线从 `---` 变成 `***`，空段变成 `<br />`。
 * 实时预览里这些都还是原来的样子，一切到分屏，源码就多出反斜杠和星号。
 * 这里把序列化痕迹收回来。围栏、公式、行内代码保持原样。
 */
export function restoreCrepeMarkdown(text: string): string {
  if (!text) return text
  return mapMarkdownLines(text, (line, kind) => {
    if (kind !== 'prose') return line
    if (HR_STAR.test(line)) return '---'
    if (BR_LINE.test(line)) return ''
    const listed = line.replace(LIST_STAR, '$1- ')
    const withoutBreak = listed.replace(LIST_BR, '$1')
    return mapInlineProse(withoutBreak, unescapeCrepeProse)
  })
}

/** 只去掉「显示出来和没写反斜杠一样」的转义。`\*强调\*` 这种成对星号不动，否则会变成斜体。 */
function unescapeCrepeProse(text: string): string {
  return text
    .replace(/\\\[/g, '[')
    .replace(/\\\](?!\()/g, ']')
    .replace(/(^|\s)\\\*(?=\s|$)/g, '$1*')
    .replace(/(^|\s)\\_(?=\s|$)/g, '$1_')
}

/**
 * 实时预览把 `<tag>` 当成文本显示。分屏若直接交给 marked，未知标签会被消毒删掉，
 * 左右就对不上。正文里的标签改成转义后再渲染；围栏和行内代码仍由代码块自己转义。
 */
function escapeProseHtml(text: string): string {
  if (!/</.test(text)) return text
  return mapMarkdownLines(text, (line, kind) => {
    if (kind !== 'prose') return line
    return mapInlineProse(line, (prose) => prose.replace(RAW_TAG, (tag) => `\\<${tag.slice(1, -1)}\\>`))
  })
}

/** 速记分屏的只读预览。按 CommonMark/GFM 渲染（不把单个换行强转成 <br>，与 Crepe 序列化对齐）。 */
export function renderNoteMarkdown(text: string): string {
  if (!text) return ''
  const html = marked.parse(escapeProseHtml(loosenHtmlBreaks(text)), {
    async: false,
    gfm: true,
    renderer: noteRenderer,
  }) as string
  return sanitizeNoteHtml(html)
}

/** 任务列表要留下复选框。只放行 checkbox，其它 input 和事件属性仍然丢掉。 */
function sanitizeNoteHtml(html: string): string {
  const dropNonCheckbox = (node: Node, data: { tagName: string }) => {
    if (data.tagName.toLowerCase() !== 'input') return
    if (node instanceof Element && node.getAttribute('type') === 'checkbox') return
    node.parentNode?.removeChild(node)
  }
  DOMPurify.addHook('uponSanitizeElement', dropNonCheckbox)
  try {
    return DOMPurify.sanitize(html, {
      ADD_TAGS: ['input'],
      ADD_ATTR: ['type', 'checked', 'disabled'],
    })
  } finally {
    DOMPurify.removeHook('uponSanitizeElement', dropNonCheckbox)
  }
}
