// 显式 .ts 扩展名：本模块零依赖，要能被 node --test 直接加载（Node ESM 不解析省略扩展名；
// tsconfig 已开 allowImportingTsExtensions，Vite 两种写法都认）
import { mapInlineProse, mapMarkdownLines, type LineKind } from './markdownLines.ts'

/**
 * 一键美化格式：对 Markdown 真相源做一次确定性的排版整理（纯函数、幂等、零依赖）。
 *
 * 三层规则：
 * 1. 清理    —— 行尾空白（硬换行除外）、连续纯空行收敛、空行占位符归一；
 * 2. 结构统一 —— 无序列表标记统一 `-`、标题前后恰好一个空行、`***`/`___` 分隔线收成 `---`；
 * 3. 盘古之白 —— 中英文 / 中文与数字之间加空格（代码、链接、双链、HTML 一律不碰）。
 *
 * 明确不做的事（有意收窄，避免"美化"变"改内容"）：
 * - 不自动升降标题层级、不做全角/半角标点强转；
 * - 不动 `-`/`- - -` 横线形态的分隔线——它在紧贴段落文本时是 setext 二级标题
 *   （`标题\n---`），强转成 `---` 或补空行都会改变渲染结果；
 * - 不在强调标记（`*`/`_`）与相邻文字之间加空格——`中文**English**中文` 里
 *   唯一安全的口径是只处理 CJK 与字母数字的**直接相邻**，标记内侧永远不碰。
 *
 * 与既有机制的咬合（改动前先读约定 38）：
 * - `\u200B` 整行占位是「用户故意保留的空行」（NoteEditor.normalizeEmptyParagraphs
 *   落库前填充），美化器把它归一为单字符占位但绝不合并、绝不删除；
 * - 行尾奇数个反斜杠是硬换行（Shift+Enter），偶数个是转义反斜杠 + 可能的空格硬换行，
 *   行尾清理必须保住这两种形态，否则所有手动换行被静默合并；
 * - 围栏 / `$$` 公式 / 缩进代码整段跳过（复用 markdownLines 的共享识别，禁止另写一套）。
 */

/** 零宽空格：空行占位符（见 NoteEditor.normalizeEmptyParagraphs） */
const ZWSP = '\u200B'

/** CJK 判定：汉字（含扩展 A/兼容区）+ 假名 + 谚文。刻意排除全角标点——
 *  「，abc」不得变成「， abc」，全角标点与后续文字之间永远不加空格 */
const CJK_CLASS = '\\u3005\\u3007\\u3040-\\u30ff\\u3400-\\u4dbf\\u4e00-\\u9fff\\uf900-\\ufaff\\uac00-\\ud7af'
const RE_CJK = new RegExp(`[${CJK_CLASS}]`)
const RE_CJK_THEN_ALNUM = new RegExp(`([${CJK_CLASS}])([A-Za-z0-9])`, 'g')
const RE_ALNUM_THEN_CJK = new RegExp(`([A-Za-z0-9])([${CJK_CLASS}])`, 'g')

/** 行首标题（ATX）：# 后必须有空格或行尾，`#标签` 是普通文本不是标题 */
const RE_HEADING = /^ {0,3}#{1,6}(?:[ \t]+|$)/

/** 星号/下划线分隔线（横线形态另行处理，见模块注释）。 */
const RE_HR_STAR_UNDERSCORE = /^ {0,3}(?:\*[ \t]*){3,}$|^ {0,3}(?:_[ \t]*){3,}$/

/** 整行占位空行：零宽空格/NBSP（旧方案残留）与空白的任意组合 → 归一为单字符占位 */
const RE_PLACEHOLDER_LINE = /^[ \t]*(?:[\u200b\u00a0][ \t]*)+$/

/** 围栏/引号前缀剥离后，行首 4 空格或制表符视为缩进代码，整行不碰 */
const RE_INDENTED_CODE = /^(?: {4}|\t)/

/** 行内保护名单：单趟交替正则一次性遮蔽——多趟分别遮蔽会在「链接括号内套 URL」
 *  时产生嵌套哨兵，还原一遍会把内层哨兵留在正文里（实测过的坑） */
const RE_MASK_ALL = new RegExp(
  '\\[\\[[^\\]\\n]*\\]\\]' + // [[双链]]：链接键是标题精确匹配，内部加空格会断链
    '|\\]\\[[^\\]\\n]*\\]' + // 引用式链接的引用标签段
    '|\\]\\([^()\\n]*\\)' + // 行内链接/图片的 URL 括号段（相对路径型，无 scheme 的也护住）
    '|https?://[^\\s。，；！？、）】」』》>]+' + // 裸 URL（只停在中式标点/空白——遮蔽宁多勿少，多遮无副作用）
    '|<[^<>\\n]+>', // 内联 HTML 标签与 <https://…> 自动链接
  'g',
)

/** 遮蔽占位符：私有区字符，正文里出现即视为不可见垃圾，冲突概率忽略 */
const MASK_OPEN = '\ue000'
const MASK_CLOSE = '\ue001'
const RE_MASK = /\ue000(\d+)\ue001/g

/** 遮蔽后的 URL 起始边界：CJK 紧贴裸 URL 段（哨兵）补空格用；双链/链接括号段不补 */
const RE_CJK_BEFORE_MASKED = new RegExp(`([${CJK_CLASS}])(\ue000(\\d+)\ue001)`, 'g')

/** front-matter（导入笔记可能携带）整块原样跳过 */
const RE_FRONT_MATTER = /^---\n[\s\S]*?\n---(?:\n|$)/

export function beautifyNoteMarkdown(md: string): string {
  if (!md) return md
  const text = md.replace(/\r\n?/g, '\n')
  // front-matter 只可能出现在文档头，先切出来原样拼回
  const fm = RE_FRONT_MATTER.exec(text)
  const head = fm ? fm[0] : ''
  const body = fm ? text.slice(fm[0].length) : text
  // 围栏/公式行原样返回，只有正文行走行内层规则
  return head + structuralPass(mapMarkdownLines(body, (line, kind) => (kind === 'prose' ? beautifyProseLine(line) : line)))
}

/** 行内层：占位行归一 → 行尾清理 → 前缀剥离/标记统一 → 盘古之白 */
function beautifyProseLine(line: string): string {
  if (RE_PLACEHOLDER_LINE.test(line)) return ZWSP
  const parts = splitLineParts(line)
  // 分隔线、缩进代码整行不碰（含行尾——代码里的行尾空白是内容）
  if (!parts) return line
  // 盘古只作用在行内代码之外的正文段（mapInlineProse 按成对反引号分段）
  const content = mapInlineProse(trimTrailing(parts.content), pangu)
  return parts.prefix + content
}

/**
 * 行尾清理：保住硬换行。奇数个行尾反斜杠 = 末位是硬换行标记（前置空白无意义可清）；
 * 偶数个 = 转义反斜杠，其后 2+ 空格自身是硬换行的空格形态，规范成恰好 2 个。
 * 其余行尾空白直接清掉。
 */
function trimTrailing(line: string): string {
  const bs = /(\\+)([ \t]*)$/.exec(line)
  if (bs) {
    const before = line.slice(0, bs.index).replace(/[ \t]+$/, '')
    if (bs[1].length % 2 === 1) return before + bs[1]
    return before + bs[1] + (bs[2].length >= 2 ? '  ' : '')
  }
  const sp = /[ \t]+$/.exec(line)
  if (!sp) return line
  return sp[0].length >= 2 ? line.slice(0, sp.index) + '  ' : line.slice(0, sp.index)
}

/**
 * 剥离行首标记前缀（引用 > 可嵌套、可与其他标记混合），同时做结构归一：
 * - 无序列表 `* `/`+ ` → `- `（缩进与有序列表的 `1)` 写法保持原样）；
 * - 标题 `#` 后多余空格收成 1 个、去掉行尾闭合 ` #+`；
 * - 分隔线（任何字符形态）与缩进代码返回 null = 整行不碰。
 * 返回的 prefix 已是归一后的形态，content 是待行内处理的正文。
 */
function splitLineParts(line: string): { prefix: string; content: string } | null {
  let prefix = ''
  let rest = line
  for (;;) {
    const quote = /^[ \t]*>/.exec(rest)
    if (!quote) break
    const consumed = /^[ \t]*>[ \t]?/.exec(rest)![0]
    prefix += consumed
    rest = rest.slice(consumed.length)
  }
  if (RE_HR_ANY.test(rest)) return null
  if (RE_INDENTED_CODE.test(rest)) return null
  const list = /^([ \t]*)([-+*]|\d{1,9}[.)])([ \t]+)([\s\S]*)$/.exec(rest)
  if (list) {
    const marker = /^\d/.test(list[2]) ? list[2] : '-'
    prefix += `${list[1]}${marker} `
    rest = list[4]
  } else {
    const heading = /^([ \t]*#{1,6})[ \t]+([\s\S]*)$/.exec(rest)
    if (heading) {
      prefix += `${heading[1]} `
      // 去闭合 #（## H ## → ## H）与行尾空白——ATX 标题行尾没有硬换行语义
      rest = heading[2].replace(/[ \t]+#+[ \t]*$/, '').replace(/[ \t]+$/, '')
    } else {
      // 无标记：保留原有 0-3 空格缩进
      const indent = /^ {1,3}/.exec(rest)
      if (indent) {
        prefix += indent[0]
        rest = rest.slice(indent[0].length)
      }
    }
  }
  return { prefix, content: rest }
}

/** 任何字符形态的分隔线（含紧贴文本时语义为 setext 下划线的横线形态）——一律不碰 */
const RE_HR_ANY = /^ {0,3}(?:[-*+_][ \t]*){3,}$/

/** 盘古之白：先遮蔽不可改写段（双链/URL/链接括号/HTML），再对 CJK↔字母数字直接相邻处加空格 */
function pangu(text: string): string {
  if (!RE_CJK.test(text)) return text
  const masked: string[] = []
  const stashed = text.replace(RE_MASK_ALL, (m) => {
    masked.push(m)
    return `${MASK_OPEN}${masked.length - 1}${MASK_CLOSE}`
  })
  // 「见https://…」补起始空格必须在遮蔽**之后**且只对裸 URL 段生效：放遮蔽前会把
  // [[双链https://…]] 内嵌的 URL 也插上空格，改掉链接键
  const spaced = stashed
    .replace(RE_CJK_BEFORE_MASKED, (m, cjk: string, seg: string, idx: string) =>
      /^https?:\/\//.test(masked[Number(idx)]) ? `${cjk} ${seg}` : m,
    )
    .replace(RE_CJK_THEN_ALNUM, '$1 $2')
    .replace(RE_ALNUM_THEN_CJK, '$1 $2')
  return spaced.replace(RE_MASK, (_, i) => masked[Number(i)])
}

/** 跨行层：连续纯空行收敛、标题前后恰好一个空行、星/下划线分隔线收成 `---` 并保证前方空行 */
function structuralPass(text: string): string {
  const lines = text.split('\n')
  const kinds = collectKinds(text)
  const out: string[] = []
  const isBlank = (s: string) => /^[ \t]*$/.test(s)
  const isPlaceholder = (s: string) => s === ZWSP
  const separated = (s: string) => isBlank(s) || isPlaceholder(s)
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (kinds[i] !== 'prose') {
      out.push(line)
      continue
    }
    if (isBlank(line)) {
      if (out.length > 0 && isBlank(out[out.length - 1])) continue
      out.push(line)
      continue
    }
    if (RE_HEADING.test(line)) {
      if (out.length > 0 && !separated(out[out.length - 1])) out.push('')
      out.push(line)
      continue
    }
    if (RE_HR_STAR_UNDERSCORE.test(line)) {
      // 前方补空行，防 --- 紧贴段落文本被解析成 setext 二级标题
      if (out.length > 0 && !separated(out[out.length - 1]) && !RE_HEADING.test(out[out.length - 1])) {
        out.push('')
      }
      out.push('---')
      continue
    }
    // 标题的下一行若是实体内容，标题后补空行（占位空行本身已是分隔，不补）
    if (out.length > 0 && RE_HEADING.test(out[out.length - 1]) && !isPlaceholder(line)) {
      out.push('')
    }
    out.push(line)
  }
  return out.join('\n')
}

/** 借共享扫描器取每行的围栏/公式/正文标记，供跨行层避开代码内部 */
function collectKinds(text: string): LineKind[] {
  const kinds: LineKind[] = []
  mapMarkdownLines(text, (_line, kind) => {
    kinds.push(kind)
    return _line
  })
  return kinds
}
