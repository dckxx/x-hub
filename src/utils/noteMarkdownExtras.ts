import { remarkStringifyOptionsCtx } from '@milkdown/kit/core'
import type { MilkdownPlugin } from '@milkdown/kit/ctx'
import { footnoteDefinitionSchema, footnoteReferenceSchema } from '@milkdown/kit/preset/gfm'
import { $markSchema, $nodeSchema, $remark, $view } from '@milkdown/kit/utils'
import type { Ctx } from '@milkdown/kit/ctx'

/**
 * 速记在 Crepe（实时预览 / 分屏只读预览）上补的 Markdown 扩展。
 * 删除线 ~~ 和脚注 [^id] 由已启用的 GFM 负责；这里补 `<mark>`、`<details>`，
 * 以及 ```mermaid 的图表预览。
 */

interface MdNode {
  type: string
  value?: string
  summary?: string
  children?: MdNode[]
}

const OPEN_MARK = /^<mark\b[^>]*>$/i
const CLOSE_MARK = /^<\/mark>$/i
const MARK_WHOLE = /^<mark\b[^>]*>([\s\S]*)<\/mark>$/i
const OPEN_DETAILS = /^<details\b/i

function decodeBasic(text: string): string {
  return text
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
}

function stitchMark(parent: MdNode) {
  const children = parent.children
  if (!children) return
  for (let i = 0; i < children.length; i++) {
    const node = children[i]
    const value = node.type === 'html' ? (node.value ?? '').trim() : ''
    const whole = value.match(MARK_WHOLE)
    if (whole) {
      children.splice(i, 1, {
        type: 'highlight',
        children: [{ type: 'text', value: decodeBasic(whole[1]) }],
      })
      continue
    }
    if (!OPEN_MARK.test(value)) continue
    let end = -1
    for (let j = i + 1; j < children.length; j++) {
      const next = children[j]
      if (next.type === 'html' && CLOSE_MARK.test((next.value ?? '').trim())) {
        end = j
        break
      }
    }
    if (end < 0) continue
    const inner = children.slice(i + 1, end)
    children.splice(i, end - i + 1, { type: 'highlight', children: inner })
  }
}

/** 块级 HTML 进编辑器后常常包在段落里，不能只认裸的 html 节点。 */
function htmlText(node: MdNode): string | null {
  if (node.type === 'html') return (node.value ?? '').trim()
  if (node.type === 'paragraph' && node.children?.length === 1 && node.children[0].type === 'html') {
    return (node.children[0].value ?? '').trim()
  }
  return null
}

const DETAILS_TOKEN = /<\/?details\b[^>]*>|<summary\b[^>]*>[\s\S]*?<\/summary>|<summary\b[^>]*>|<\/summary>/i

interface DetailsScan {
  depth: number
  summary: string | null
  /** 扫到了与最外层起始标签配对的结束标签。 */
  closed: boolean
  /** 配对结束标签之前、仍属于这一层正文的源码（内层标签保留）。 */
  before: string
  /** 配对结束标签之后的源码。 */
  after: string
}

/** 围栏代码和行内代码里的标签不算结构。遮罩只换成空格，下标与原文对齐。 */
function maskMarkdownCode(raw: string): string {
  const chars = raw.split('')
  const cover = (from: number, to: number) => {
    for (let k = from; k < to && k < chars.length; k++) {
      if (chars[k] !== '\n' && chars[k] !== '\r') chars[k] = ' '
    }
  }
  let i = 0
  while (i < chars.length) {
    const atLine = i === 0 || chars[i - 1] === '\n'
    if (atLine) {
      const fence = /^( {0,3})(`{3,}|~{3,})[^\n]*\r?\n/.exec(raw.slice(i))
      if (fence) {
        const marker = fence[2][0]
        const min = fence[2].length
        let j = i + fence[0].length
        let end = -1
        while (j < raw.length) {
          const lineEnd = raw.indexOf('\n', j)
          const stop = lineEnd < 0 ? raw.length : lineEnd
          const line = raw.slice(j, stop).replace(/\r$/, '')
          if (new RegExp(`^ {0,3}\\${marker}{${min},}[ \\t]*$`).test(line)) {
            end = lineEnd < 0 ? raw.length : lineEnd + 1
            break
          }
          j = lineEnd < 0 ? raw.length : lineEnd + 1
        }
        // 没写结束围栏时不能把后面的 </details> 一起遮掉，否则整段折叠块都收不起来。
        if (end < 0) {
          i += 1
          continue
        }
        cover(i, end)
        i = end
        continue
      }
    }
    if (chars[i] === '`') {
      const marker = /^`+/.exec(raw.slice(i))?.[0]
      if (marker) {
        const closeAt = raw.indexOf(marker, i + marker.length)
        if (closeAt >= 0) {
          const end = closeAt + marker.length
          cover(i, end)
          i = end
          continue
        }
      }
    }
    i += 1
  }
  return chars.join('')
}

/**
 * 按嵌套层级扫描一段 HTML。depth 从 0 开始时，这段必须以 `<details` 开头。
 * 只剥掉最外层的 `<details>` 和它自己的 `<summary>`，内层折叠块原样留在正文里。
 */
function scanDetailsHtml(raw: string, depth: number, summary: string | null): DetailsScan {
  const masked = maskMarkdownCode(raw)
  let i = 0
  let nextDepth = depth
  let nextSummary = summary
  let closed = false
  let closeEnd = -1
  const before: string[] = []
  while (i < raw.length && !closed) {
    const match = DETAILS_TOKEN.exec(masked.slice(i))
    if (!match || match.index == null) {
      if (nextDepth > 0) before.push(raw.slice(i))
      break
    }
    const at = i + match.index
    const tag = raw.slice(at, at + match[0].length)
    if (nextDepth > 0) before.push(raw.slice(i, at))
    if (/^<details\b/i.test(tag)) {
      if (nextDepth > 0) before.push(tag)
      nextDepth += 1
      i = at + tag.length
      continue
    }
    if (/^<\/details>/i.test(tag)) {
      nextDepth -= 1
      if (nextDepth <= 0) {
        closed = true
        closeEnd = at + tag.length
        break
      }
      before.push(tag)
      i = at + tag.length
      continue
    }
    const summaryInner = /^<summary\b[^>]*>([\s\S]*?)<\/summary>/i.exec(tag)
    if (summaryInner && nextSummary == null && nextDepth === 1) {
      nextSummary = decodeBasic(summaryInner[1].trim()) || '详情'
      i = at + tag.length
      continue
    }
    if (nextDepth > 0) before.push(tag)
    i = at + tag.length
  }
  return {
    depth: nextDepth,
    summary: nextSummary,
    closed,
    before: before.join(''),
    after: closed ? raw.slice(closeEnd) : '',
  }
}

let detailsDepth = 0

/** 先按 Markdown 解析，围栏和行内代码里的标签不会被当成折叠块。 */
function parseMixed(source: string, parse: ((source: string) => MdNode) | undefined): MdNode[] {
  if (!source.trim()) return []
  if (!parse || detailsDepth > 32) {
    return [{ type: 'paragraph', children: [{ type: 'text', value: source.trim() }] }]
  }
  const tree = parse(source)
  detailsDepth += 1
  try {
    extendMarkdownTree(tree, parse)
  } finally {
    detailsDepth -= 1
  }
  return tree.children ?? []
}

function htmlValue(node: MdNode): string | null {
  return node.type === 'html' ? (node.value ?? '').trim() : null
}

/** `<summary>` 被拆成相邻节点时，把标题从正文里提出来。 */
function liftSummary(nodes: MdNode[]): { summary: string; rest: MdNode[] } | null {
  const first = nodes[0] ? htmlValue(nodes[0]) : null
  if (first == null) return null
  const whole = /^<summary\b[^>]*>([\s\S]*)<\/summary>$/i.exec(first)
  if (whole) return { summary: decodeBasic(whole[1].trim()) || '详情', rest: nodes.slice(1) }
  if (!/^<summary\b[^>]*>$/i.test(first)) return null
  const parts: string[] = []
  for (let index = 1; index < nodes.length; index++) {
    const html = htmlValue(nodes[index])
    if (html != null && /^<\/summary>$/i.test(html)) {
      return { summary: decodeBasic(parts.join('').trim()) || '详情', rest: nodes.slice(index + 1) }
    }
    if (nodes[index].type !== 'text') return null
    parts.push(nodes[index].value ?? '')
  }
  return null
}

function asBlockBody(nodes: MdNode[]): MdNode[] {
  if (nodes.length === 0) return []
  const inline = nodes.every((node) => (
    node.type === 'text'
    || node.type === 'html'
    || node.type === 'strong'
    || node.type === 'emphasis'
    || node.type === 'inlineCode'
    || node.type === 'link'
  ))
  return inline ? [{ type: 'paragraph', children: nodes }] : nodes
}

function finishDetails(summary: string | null, nodes: MdNode[]): MdNode {
  const lifted = summary == null ? liftSummary(nodes) : null
  return detailsNode(lifted?.summary ?? summary, asBlockBody(lifted ? lifted.rest : nodes))
}

function detailsNode(summary: string | null, body: MdNode[]): MdNode {
  return {
    type: 'details',
    summary: summary ?? '详情',
    children: body.length > 0 ? body : [{ type: 'paragraph', children: [] }],
  }
}

function stitchDetails(parent: MdNode, parse: ((source: string) => MdNode) | undefined) {
  const children = parent.children
  if (!children) return
  for (let i = 0; i < children.length; i++) {
    const raw = htmlText(children[i])
    if (raw == null || !OPEN_DETAILS.test(raw)) continue
    const opening = scanDetailsHtml(raw, 0, null)
    if (opening.closed) {
      const extra = parseMixed(opening.after, parse)
      children.splice(i, 1, finishDetails(opening.summary, parseMixed(opening.before, parse)), ...extra)
      continue
    }
    let depth = opening.depth
    let summary = opening.summary
    let end = -1
    let trailing = ''
    let after = ''
    const middle: MdNode[] = []
    for (let j = i + 1; j < children.length; j++) {
      const nextRaw = htmlText(children[j])
      if (nextRaw == null) {
        middle.push(children[j])
        continue
      }
      const scanned = scanDetailsHtml(nextRaw, depth, summary)
      depth = scanned.depth
      summary = scanned.summary
      if (!scanned.closed) {
        middle.push(children[j])
        continue
      }
      end = j
      trailing = scanned.before
      after = scanned.after
      break
    }
    if (end < 0) continue
    const body = [...parseMixed(opening.before, parse), ...middle, ...parseMixed(trailing, parse)]
    const extra = parseMixed(after, parse)
    children.splice(i, end - i + 1, finishDetails(summary, body), ...extra)
  }
}

const INLINE_PARENTS = new Set(['paragraph', 'heading'])

/** 折叠块是块级节点。留在段落里时编辑器会整段丢弃，所以拆成前后段落夹着折叠块。 */
function splitDetailsParagraphs(parent: MdNode) {
  const children = parent.children
  if (!children) return
  for (let i = 0; i < children.length; i++) {
    const node = children[i]
    if (!INLINE_PARENTS.has(node.type) || !node.children?.some((child) => child.type === 'details')) continue
    const parts: MdNode[] = []
    let inline: MdNode[] = []
    const flush = () => {
      if (inline.length === 0) return
      const blank = inline.every((child) => child.type === 'text' && !(child.value ?? '').trim())
      if (!blank) parts.push({ type: node.type, children: inline })
      inline = []
    }
    for (const child of node.children ?? []) {
      if (child.type === 'details') {
        flush()
        parts.push(child)
      } else {
        inline.push(child)
      }
    }
    flush()
    children.splice(i, 1, ...parts)
    i += parts.length - 1
  }
}

/** 把 `<mark>` / `<details>` 收成编辑器认识的节点。脚注交给 GFM，这里不动。 */
export function extendMarkdownTree(tree: MdNode, parse?: (source: string) => MdNode) {
  const walk = (node: MdNode) => {
    if (!node.children) return
    stitchDetails(node, parse)
    stitchMark(node)
    for (const child of node.children) walk(child)
    splitDetailsParagraphs(node)
  }
  walk(tree)
}

let stitching = false

export const noteMarkdownExtrasPlugin = $remark(
  'noteMarkdownExtras',
  () => () =>
    function (this: { parse?: (source: string) => MdNode }, tree: MdNode) {
      // parse 会重新跑到这个插件。嵌套折叠块的正文只在外层 walk 里收，避免套住自己。
      if (stitching) return
      const parse = typeof this.parse === 'function'
        ? (source: string) => {
            stitching = true
            try {
              return this.parse!(source)
            } finally {
              stitching = false
            }
          }
        : undefined
      extendMarkdownTree(tree, parse)
    },
)

export const highlightSchema = $markSchema('highlight', () => ({
  parseDOM: [{ tag: 'mark' }],
  toDOM: () => ['mark', 0],
  parseMarkdown: {
    match: (node: MdNode) => node.type === 'highlight',
    runner: (state, node, mark) => {
      state.openMark(mark)
      state.next(node.children)
      state.closeMark(mark)
    },
  },
  toMarkdown: {
    match: (mark) => mark.type.name === 'highlight',
    runner: (state, mark) => {
      state.withMark(mark, 'highlight')
    },
  },
}))

export const detailsSchema = $nodeSchema('details_block', () => ({
  group: 'block',
  content: 'block+',
  defining: true,
  attrs: {
    summary: { default: '详情', validate: 'string' },
  },
  parseDOM: [
    {
      tag: 'details.xhub-details',
      getAttrs: (dom) => {
        if (!(dom instanceof HTMLElement)) return { summary: '详情' }
        return { summary: dom.querySelector('summary')?.textContent || '详情' }
      },
      contentElement: '.xhub-details-body',
    },
  ],
  toDOM: (node) => [
    'details',
    { class: 'xhub-details' },
    ['summary', node.attrs.summary],
    ['div', { class: 'xhub-details-body' }, 0],
  ],
  parseMarkdown: {
    match: (node: MdNode) => node.type === 'details',
    runner: (state, node, type) => {
      state.openNode(type, { summary: node.summary || '详情' })
      state.next(node.children)
      state.closeNode()
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === 'details_block',
    runner: (state, node) => {
      state.openNode('details', undefined, { summary: node.attrs.summary }).next(node.content).closeNode()
    },
  },
}))

function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/** 序列化回 `<mark>` / `<details>`，切换模式时源码不会丢掉这两种写法。 */
export function markdownExtraStringify(ctx: Ctx) {
  ctx.update(remarkStringifyOptionsCtx, (prev) => ({
    ...prev,
    handlers: {
      ...prev.handlers,
      highlight: (node: never, _parent: unknown, state: { containerPhrasing: (node: never, info: unknown) => string }, info: object) => {
        const value = state.containerPhrasing(node, { ...info, before: '<', after: '>' })
        return `<mark>${value}</mark>`
      },
      details: (node: { summary?: string }, _parent: unknown, state: { containerFlow: (node: unknown, info: unknown) => string }, info: object) => {
        const summary = escapeHtml(String(node.summary ?? '详情'))
        const body = state.containerFlow(node, info)
        return `<details><summary>${summary}</summary>\n\n${body}\n\n</details>`
      },
    },
  }))
}

const footnoteOrigin = new WeakMap<Element, HTMLElement>()

function footnoteRoot(el: Element | null): Element | null {
  return el?.closest('.milkdown') ?? null
}

function jumpTo(el: Element | null) {
  el?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

/** 正文里的上标：按钮可 Tab 聚焦，Enter / 空格 / 点击都跳到文末对应脚注。 */
const footnoteReferenceView = $view(footnoteReferenceSchema.node, () => (node) => {
  const dom = document.createElement('sup')
  dom.className = 'xhub-fn-ref'
  const jump = document.createElement('button')
  jump.type = 'button'
  jump.className = 'xhub-fn-jump'
  const applyLabel = (label: string) => {
    dom.dataset.label = label
    jump.textContent = label
    jump.setAttribute('aria-label', label ? `跳到脚注 ${label}` : '跳到脚注')
    jump.title = '跳到脚注'
  }
  applyLabel(String(node.attrs.label ?? ''))
  jump.addEventListener('click', (event) => {
    event.preventDefault()
    event.stopPropagation()
    const root = footnoteRoot(dom)
    if (!root) return
    footnoteOrigin.set(root, dom)
    const label = CSS.escape(dom.dataset.label ?? '')
    jumpTo(root.querySelector(`.xhub-fn[data-label="${label}"]`))
  })
  dom.append(jump)
  return {
    dom,
    update: (next) => {
      if (next.type.name !== 'footnote_reference') return false
      applyLabel(String(next.attrs.label ?? ''))
      return true
    },
    stopEvent: () => true,
    ignoreMutation: () => true,
  }
})

/** 文末脚注：序号、正文，右侧返回箭头点回刚才的上标。 */
const footnoteDefinitionView = $view(footnoteDefinitionSchema.node, () => (node) => {
  const dom = document.createElement('div')
  dom.className = 'xhub-fn'
  dom.dataset.label = String(node.attrs.label ?? '')

  const label = document.createElement('span')
  label.className = 'xhub-fn-label'
  label.textContent = `${node.attrs.label}.`

  const contentDOM = document.createElement('div')
  contentDOM.className = 'xhub-fn-body'

  const back = document.createElement('button')
  back.type = 'button'
  back.className = 'xhub-fn-back'
  back.title = '返回正文'
  back.setAttribute('aria-label', '返回脚注所在位置')
  back.innerHTML =
    '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 14-4-4 4-4"/><path d="M5 10h9a5 5 0 0 1 5 5v1"/></svg>'
  back.addEventListener('click', (event) => {
    event.preventDefault()
    event.stopPropagation()
    const root = footnoteRoot(dom)
    if (!root) return
    const key = dom.dataset.label ?? ''
    const remembered = footnoteOrigin.get(root)
    const same = remembered?.isConnected && remembered.dataset.label === key ? remembered : null
    const fallback = root.querySelector(`sup.xhub-fn-ref[data-label="${CSS.escape(key)}"]`)
    jumpTo(same ?? fallback)
  })

  dom.append(label, contentDOM, back)
  return {
    dom,
    contentDOM,
    update: (next) => {
      if (next.type.name !== 'footnote_definition') return false
      dom.dataset.label = String(next.attrs.label ?? '')
      label.textContent = `${next.attrs.label}.`
      return true
    },
    stopEvent: (event) => back.contains(event.target as Node),
    ignoreMutation: (mutation) => !contentDOM.contains(mutation.target),
  }
})

export const noteMarkdownExtraPlugins: MilkdownPlugin[] = [
  noteMarkdownExtrasPlugin,
  highlightSchema,
  detailsSchema,
  footnoteReferenceView,
  footnoteDefinitionView,
].flat()

let mermaidSeq = 0
let mermaidQueue: Promise<unknown> = Promise.resolve()

/** Mermaid 默认把 svg 写成 width="100%"，宽图会被预览栏压得很矮。按 viewBox 用真实像素，字号才和图表本身一致。 */
function sizeMermaidSvg(svg: string): string {
  const box = svg.match(/viewBox="\s*[\d.eE+-]+\s+[\d.eE+-]+\s+([\d.eE+-]+)\s+([\d.eE+-]+)\s*"/)
  if (!box) return svg
  const width = Math.ceil(Number(box[1]))
  const height = Math.ceil(Number(box[2]))
  if (!Number.isFinite(width) || !Number.isFinite(height)) return svg
  return svg.replace(/<svg\b([^>]*)>/, (_match, attrs: string) => {
    const next = attrs
      .replace(/\swidth="[^"]*"/, '')
      .replace(/\sheight="[^"]*"/, '')
    return `<svg${next} width="${width}" height="${height}">`
  })
}

/** ```mermaid 代码块的预览。LaTeX 预览仍由 Crepe 自己包在外层，这里只认 mermaid。 */
export function renderMermaidPreview(
  language: string,
  content: string,
  applyPreview: (value: null | string | HTMLElement) => void,
): null | undefined {
  if (language.toLowerCase() !== 'mermaid' || !content.trim()) return null
  const seq = ++mermaidSeq
  // mermaid 的 initialize / render 是全局的。两张图同时画，后完成的旧结果会盖住新图。
  mermaidQueue = mermaidQueue.catch(() => undefined).then(async () => {
    const dark = document.documentElement.getAttribute('data-theme') === 'dark'
    try {
      const mermaid = (await import('mermaid')).default
      mermaid.initialize({
        startOnLoad: false,
        // antiscript 允许节点里的 HTML 文字（菱形、箭头标签才铺得开），脚本仍会被去掉。
        // 预览面板还会再用一层 SVG 消毒。strict 会关掉 htmlLabels，图就变成挤在一起的纯矢量字。
        securityLevel: 'antiscript',
        look: 'classic',
        theme: dark ? 'dark' : 'default',
        fontFamily: 'inherit',
        flowchart: {
          htmlLabels: true,
          curve: 'basis',
          padding: 16,
          nodeSpacing: 40,
          rankSpacing: 55,
        },
        themeVariables: {
          // `|渲染|` 这种箭头标签的底，不要一块实色挡在线上
          edgeLabelBackground: 'transparent',
        },
      })
      const { svg } = await mermaid.render(`xhubMmd${seq.toString(36)}`, content)
      applyPreview(sizeMermaidSvg(svg))
    } catch (error) {
      const message = error instanceof Error ? error.message : '图表无法渲染'
      applyPreview(`<pre class="xhub-mermaid-error">${escapeHtml(message)}</pre>`)
    }
  })
  return undefined
}
