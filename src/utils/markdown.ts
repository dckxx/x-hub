/**
 * Markdown 轻量纯文本化：用于笔记列表摘要、全局搜索片段、标题派生等纯展示场景（非渲染）。
 */
export function markdownPlainText(md: string, maxLen = 60): string {
  return md
    .replace(/```[\s\S]*?(```|$)/g, ' ') // 围栏代码块（未闭合也算）
    .replace(/`([^`]*)`/g, '$1') // 行内代码保留内容
    .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ') // 图片整体剔除
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // 链接保留文字
    .replace(/^\s{0,3}>\s?/gm, '') // 引用标记
    .replace(/^\s{0,3}[-*+]\s+/gm, '') // 无序列表标记
    .replace(/^\s{0,3}\d+[.)]\s+/gm, '') // 有序列表标记
    .replace(/^#{1,6}\s+/gm, '') // 标题标记
    .replace(/(\*\*\*|\*\*|\*|___|__|_|~~)/g, '') // 行内强调标记
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, maxLen)
}

/**
 * 从正文派生笔记标题：取首个有内容的行（标题行 / 普通行均可）的纯文本，
 * 超长截断加省略号。仅在标题还是默认值（空 / 无标题笔记）时由编辑器调用。
 */
export function deriveNoteTitle(markdown: string): string {
  for (const raw of markdown.split('\n')) {
    const line = raw.trim()
    if (!line) continue
    const text = markdownPlainText(line, 31)
    if (text) return text.length > 30 ? `${text.slice(0, 30)}…` : text
  }
  return ''
}
