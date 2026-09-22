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

/**
 * 单独成行的 `<br>` 在 CommonMark 里会开启 HTML 块，一直吞到下一个空行。
 * 围栏、标题因此变成普通文本。换成空行后，后面的块按正常 Markdown 解析。
 * 行内的 `hello<br />world` 不动。
 */
export function loosenHtmlBreaks(text: string): string {
  if (!/<br\b/i.test(text)) return text
  return text.replace(/^[ \t]*<br\s*\/?\s*>[ \t]*$/gim, '')
}

/** 速记分屏的只读预览。按 CommonMark/GFM 渲染（不把单个换行强转成 <br>，与 Crepe 序列化对齐）。 */
export function renderNoteMarkdown(text: string): string {
  if (!text) return ''
  return DOMPurify.sanitize(
    marked.parse(loosenHtmlBreaks(text), { async: false, gfm: true, renderer: noteRenderer }) as string,
  )
}
