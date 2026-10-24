/**
 * 内置《使用说明》的分章工具（零依赖，可被 node --test 直接加载）。
 *
 * 为什么单独一个模块：设置 → 使用说明 面板要「渲染正文 + 左侧章节目录 + 目录内搜索」，
 * 三者必须共用同一份切块口径 —— 各写一套必然出现「目录里有这一章、正文里跳不到」。
 * 同时把口径锁进单测：`tests/guideToc.test.mjs` 直接拿 `docs/user-guide.md` 跑一遍。
 *
 * 围栏 / `$$` 公式的识别复用 `markdownLines.ts`（约定：行级规则共用一处围栏识别）。
 */
import { mapMarkdownLines } from './markdownLines.ts'

export interface GuideSection {
  /** 锚点 id（GitHub 风格 slug，重复标题自动加 -2/-3） */
  id: string
  /** 纯文本标题（已剥掉 Markdown 标记） */
  title: string
  level: number
  /** 标题下方到下一个标题之间的 Markdown 正文 */
  body: string
}

export interface GuideChapter extends GuideSection {
  /** 本章第一个三级标题之前的正文（很多章直接就是正文，没有小节） */
  body: string
  sections: GuideSection[]
}

export interface GuideDocument {
  /** 一级标题（文档大标题） */
  title: string
  /** 大标题与第一个二级标题之间的内容（适用版本、阅读约定等） */
  preamble: string
  chapters: GuideChapter[]
}

/** 标题里的行内标记剥掉，只留给人看的文字（目录与正文标题显示同一份） */
function stripInlineMarkdown(text: string): string {
  return text
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`+([^`]*)`+/g, '$1')
    .replace(/(\*\*|__|~~)/g, '')
    .trim()
}

/**
 * GitHub 风格锚点：小写 → 去掉标点/emoji（保留字母、数字、汉字、空格、`-`、`_`）→ 空格转连字符。
 * 例：`1. 认识 x-hub` → `1-认识-x-hub`；`5.8 一键美化与 AI 深度整理` → `58-一键美化与-ai-深度整理`。
 */
export function slugifyHeading(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .replace(/\s+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-+|-+$/g, '')
}

const HEADING_RE = /^(#{1,3})\s+(.+?)\s*$/
const CLOSING_HASHES_RE = /\s*#+\s*$/

interface RawSection {
  level: number
  title: string
  lines: string[]
}

/** Markdown → 章节树：一级标题作文档标题，二级为章、三级为节（更深层级并入所属小节正文） */
export function splitGuideDocument(md: string): GuideDocument {
  const raws: RawSection[] = []
  const preambleLines: string[] = []
  let title = ''
  let current: RawSection | null = null

  mapMarkdownLines(md, (line, kind) => {
    if (kind === 'prose') {
      const m = HEADING_RE.exec(line)
      if (m) {
        const level = m[1].length
        const text = stripInlineMarkdown(m[2].replace(CLOSING_HASHES_RE, ''))
        if (level === 1) {
          title = text
          current = null
          return line
        }
        current = { level, title: text, lines: [] }
        raws.push(current)
        return line
      }
    }
    if (current) current.lines.push(line)
    else preambleLines.push(line)
    return line
  })

  // 标题 id 全文档去重（GitHub 同款：重名依次 -2、-3）
  const used = new Map<string, number>()
  const withId = raws.map((raw) => {
    const base = slugifyHeading(raw.title) || 'section'
    const seen = used.get(base) ?? 0
    used.set(base, seen + 1)
    return { ...raw, id: seen === 0 ? base : `${base}-${seen + 1}` }
  })

  const chapters: GuideChapter[] = []
  for (const item of withId) {
    // 文档自带的「目录」章要丢掉：面板左侧会按正文推导一份目录，留着就是两份目录打架
    if (item.level === 2 && /^(目录|contents|table of contents)$/i.test(item.title)) continue
    const body = tidyBody(item.lines)
    if (item.level === 2) {
      chapters.push({ id: item.id, title: item.title, level: 2, body, sections: [] })
      continue
    }
    // 三级标题：挂到最近的二级章；文档若从小节直接起头（异常结构），补一个同名章兜底
    let chapter = chapters[chapters.length - 1]
    if (!chapter) {
      chapter = { id: item.id, title: item.title, level: 2, body: '', sections: [] }
      chapters.push(chapter)
      continue
    }
    chapter.sections.push({ id: item.id, title: item.title, level: item.level, body })
  }

  return { title, preamble: tidyBody(preambleLines), chapters }
}

/** 正文收口：连续空行压成一个空行（渲染上等价，DOM 小一半），两端空白去掉 */
function tidyBody(lines: string[]): string {
  return lines.join('\n').replace(/\n{3,}/g, '\n\n').replace(/^\n+|\n+$/g, '')
}

/** 关键词命中数（大小写不敏感、不重叠）。目录内搜索按「章 / 节」各自计数。 */
export function countOccurrences(text: string, query: string): number {
  const needle = query.trim().toLowerCase()
  if (!needle) return 0
  const hay = text.toLowerCase()
  let hits = 0
  let at = 0
  while ((at = hay.indexOf(needle, at)) !== -1) {
    hits += 1
    at += needle.length
  }
  return hits
}
