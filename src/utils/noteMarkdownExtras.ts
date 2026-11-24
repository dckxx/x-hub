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
const CLOSE_DETAILS = /<\/details>/i
const SUMMARY = /<summary>([\s\S]*?)<\/summary>/i

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

function stitchDetails(parent: MdNode) {
  const children = parent.children
  if (!children) return
  for (let i = 0; i < children.length; i++) {
    const node = children[i]
    const raw = htmlText(node)
    if (raw == null || !OPEN_DETAILS.test(raw)) continue
    if (CLOSE_DETAILS.test(raw)) {
      const summaryMatch = raw.match(SUMMARY)
      const summary = summaryMatch ? decodeBasic(summaryMatch[1].trim()) : '详情'
      const innerText = decodeBasic(
        raw
          .replace(/<summary>[\s\S]*?<\/summary>/i, '')
          .replace(/<\/?details\b[^>]*>/gi, '')
          .trim(),
      )
      children.splice(i, 1, {
        type: 'details',
        summary,
        children: [{ type: 'paragraph', children: [{ type: 'text', value: innerText }] }],
      })
      continue
    }
    let end = -1
    for (let j = i + 1; j < children.length; j++) {
      const nextRaw = htmlText(children[j])
      if (nextRaw != null && CLOSE_DETAILS.test(nextRaw)) {
        end = j
        break
      }
    }
    if (end < 0) continue
    const summaryMatch = raw.match(SUMMARY)
    const summary = summaryMatch ? decodeBasic(summaryMatch[1].trim()) : '详情'
    const inner = children.slice(i + 1, end).filter((item) => {
      return !(item.type === 'html' && CLOSE_DETAILS.test(item.value ?? '') && item === children[end])
    })
    const body = inner.length > 0 ? inner : [{ type: 'paragraph', children: [{ type: 'text', value: '' }] }]
    children.splice(i, end - i + 1, { type: 'details', summary, children: body })
  }
}

/** 把 `<mark>` / `<details>` 收成编辑器认识的节点。脚注交给 GFM，这里不动。 */
export function extendMarkdownTree(tree: MdNode) {
  const walk = (node: MdNode) => {
    if (!node.children) return
    stitchDetails(node)
    stitchMark(node)
    for (const child of node.children) walk(child)
  }
  walk(tree)
}

export const noteMarkdownExtrasPlugin = $remark('noteMarkdownExtras', () => () => (tree: MdNode) => {
  extendMarkdownTree(tree)
})

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

/** 正文里的上标：点一下跳到文末对应脚注。 */
const footnoteReferenceView = $view(footnoteReferenceSchema.node, () => (node) => {
  const dom = document.createElement('sup')
  dom.className = 'xhub-fn-ref'
  dom.dataset.label = String(node.attrs.label ?? '')
  dom.textContent = String(node.attrs.label ?? '')
  dom.title = '跳到脚注'
  const go = (event: Event) => {
    event.preventDefault()
    event.stopPropagation()
    const root = footnoteRoot(dom)
    if (!root) return
    footnoteOrigin.set(root, dom)
    const label = CSS.escape(dom.dataset.label ?? '')
    jumpTo(root.querySelector(`.xhub-fn[data-label="${label}"]`))
  }
  dom.addEventListener('mousedown', go)
  return {
    dom,
    update: (next) => {
      if (next.type.name !== 'footnote_reference') return false
      dom.dataset.label = String(next.attrs.label ?? '')
      dom.textContent = String(next.attrs.label ?? '')
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
  back.addEventListener('mousedown', (event) => {
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
  const dark = document.documentElement.getAttribute('data-theme') === 'dark'
  void import('mermaid').then(async (mod) => {
    const mermaid = mod.default
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
    try {
      const { svg } = await mermaid.render(`xhubMmd${seq.toString(36)}`, content)
      applyPreview(sizeMermaidSvg(svg))
    } catch (error) {
      const message = error instanceof Error ? error.message : '图表无法渲染'
      applyPreview(`<pre class="xhub-mermaid-error">${escapeHtml(message)}</pre>`)
    }
  })
  return undefined
}
