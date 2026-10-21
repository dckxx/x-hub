/**
 * Markdown 轻量纯文本化：用于笔记列表摘要、全局搜索片段、标题派生等纯展示场景（非渲染）。
 */
export function markdownPlainText(md: string, maxLen = 60): string {
  return md
    .replace(/\u200B/g, '') // 零宽空格（空行占位符，见 NoteEditor.normalizeEmptyParagraphs）：JS \s 不含它，不清掉会派生出隐形标题/摘要
    .replace(/```[\s\S]*?(```|$)/g, ' ') // 围栏代码块（未闭合也算）
    .replace(/`([^`]*)`/g, '$1') // 行内代码保留内容
    .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ') // 图片整体剔除
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // 链接保留文字
    // 硬换行的反斜杠形态（Shift+Enter / 粘贴富文本时 <br> 转成 hardbreak，序列化为行尾 \ + 换行）：
    // 必须在下方空白合并之前处理，否则换行被 \s+ 吞掉后会残留字面反斜杠
    .replace(/\\+\n/g, ' ')
    .replace(/^\s{0,3}>\s?/gm, '') // 引用标记
    .replace(/^\s{0,3}[-*+]\s+/gm, '') // 无序列表标记
    .replace(/^\s{0,3}\d+[.)]\s+/gm, '') // 有序列表标记
    .replace(/^#{1,6}\s+/gm, '') // 标题标记
    .replace(/(\*\*\*|\*\*|\*|___|__|_|~~)/g, '') // 行内强调标记
    // CommonMark 转义符（正文里的 3\. 14 → 3.14；先于 <br> 清理，让 \<br> 也能命中）
    .replace(/\\([!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~])/g, '$1')
    // 粘贴残留的 HTML 换行标签与空格实体（摘要场景按语义还原为空白）
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/&nbsp;/gi, ' ')
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
    // 行尾硬换行反斜杠（hardbreak 序列化残留）不能当标题内容
    const line = raw.replace(/\\+$/, '').trim()
    if (!line) continue
    const text = markdownPlainText(line, 31)
    if (text) return text.length > 30 ? `${text.slice(0, 30)}…` : text
  }
  return ''
}
