// 显式 .ts 扩展名：与 markdownBeautify 同约定，能被 node --test 直接加载（Node ESM 不解析省略扩展名）
import { mapInlineProse, mapMarkdownLines } from './markdownLines.ts'

/**
 * 笔记图片语法回收：AI 深度整理（ai_transform_note）会把 `![说明](xhub-note 地址)`
 * 压成裸地址——模型的提示词只硬约束了「逐字保留 URL」，URL 确实一字不差地留住了，
 * 但图片语法被剥掉，而 Crepe 只认 `![...](...)` 才渲染成图片（裸地址被解析成自动链接，
 * 表现为「AI 整理后图片变成一串地址、图片不显示」）。
 *
 * 这里按**整理前的原稿**做确定性回收：原稿里是图片的 URL，在整理结果里无论以
 * 裸地址、`<自动链接>` 还是 `[文字](同一地址)` 的形态出现，都还原成原稿那份图片语法
 * （连 alt/说明一起带回——alt 是 Crepe 的宽高比通道，换成别的会把用户拖过的尺寸重置）。
 *
 * 三条边界（宁漏勿改，同 markdownBeautify 的收窄口径）：
 * - 只认「原稿里确实是图片」的地址：正文里的其它 URL、模型新写的链接一律不动；
 * - 已经写对的图片原样保留，不重写（模型自己写对了就别再动它的 alt/说明）；
 * - 围栏 / `$$` 公式 / 顶层缩进代码 / front-matter 整段跳过——代码里的地址是正文，包成图片就是事故。
 */

/** 遮蔽占位符：私有区字符，与 markdownBeautify 同款（正文里出现即视为不可见垃圾） */
const MASK_OPEN = '\ue000'
const MASK_CLOSE = '\ue001'
const RE_MASK = /\ue000(\d+)\ue001/g

/** 原稿图片语法 `![alt](url "说明")`；url 不含括号/空白/引号 */
const RE_IMAGE_TOKEN = /!\[([^[\]]*)\]\(\s*([^)\s"']+)(?:\s+"([^"]*)")?\s*\)/g

/** 整理结果里既有的图片/链接结构。label 不含方括号：`[![a](b)](c)` 这类嵌套从内层图片开始认 */
const RE_MD_STRUCT = /(!?)\[([^[\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)/g

/** 裸地址/自动链接候选：单趟扫描命中「一串 URL 字符」，再用**整串相等**判是不是笔记图片。
 *  这样「更长的地址」天然不命中（`x.png/extra`、`?u=x.png` 整串都不等于已知地址），
 *  不需要给每个地址生成正则、也不需要前后断言。`<...>` 形态由前半支优先吃掉。 */
const RE_URL_RUN =
  /<https?:\/\/[A-Za-z0-9._~:/?#[\]@!$&()*+,;=%-]+>|https?:\/\/[A-Za-z0-9._~:/?#[\]@!$&()*+,;=%-]+/g

/** 会被 URL 一起吃进来的句末标点（`.` 是合法 URL 字符）：剥掉后再判一次，标点留在原位 */
const RE_TRAILING_PUNCT = /[.,;:!?]+$/

/** front-matter（导入笔记可能携带）整块跳过：键值不是正文，包成图片会毁掉 YAML */
const RE_FRONT_MATTER = /^---\n[\s\S]*?\n---(?:\n|$)/

/** 顶层缩进代码（行首 4 空格或制表符）整行不碰；列表内的深缩进不在粗判范围内 */
const RE_INDENTED_CODE = /^(?: {4}|\t)/

/**
 * 抽取原稿里的笔记图片：地址 → 原样图片语法（同一地址多份取首个）。
 * 只扫正文行正文段（围栏/公式/行内代码里的 `![...]` 是字面文本，不是图片）。
 * 顶层缩进代码同样跳过——与还原侧跳过口径一致（含 4 空格缩进的行可能是嵌套列表也可能是
 * 代码，按代码处理宁漏勿改，同 markdownBeautify 的既定取舍）：否则代码块里出现的地址会
 * 被登记成「原稿图片」，AI 把它搬出代码块后就会被误还原成图片。
 */
export function collectNoteImages(markdown: string): Map<string, string> {
  const images = new Map<string, string>()
  mapMarkdownLines(markdown, (line, kind) => {
    if (kind === 'prose' && !RE_INDENTED_CODE.test(line)) {
      mapInlineProse(line, (prose) => {
        for (const m of prose.matchAll(RE_IMAGE_TOKEN)) {
          if (!images.has(m[2])) images.set(m[2], m[0])
        }
        return prose
      })
    }
    return line
  })
  return images
}

/**
 * 用原稿的图片语法回收整理结果里被压扁的图片地址。
 * 原稿没有图片（或结果为空）时原样返回，零副作用。
 */
export function restoreNoteImageSyntax(original: string, transformed: string): string {
  const images = collectNoteImages(original)
  if (images.size === 0 || !transformed) return transformed
  const text = transformed.replace(/\r\n?/g, '\n')
  const fm = RE_FRONT_MATTER.exec(text)
  const head = fm ? fm[0] : ''
  const body = fm ? text.slice(fm[0].length) : text
  return (
    head +
    mapMarkdownLines(body, (line, kind) => (kind === 'prose' ? restoreLine(line, images) : line))
  )
}

function restoreLine(line: string, images: Map<string, string>): string {
  if (RE_INDENTED_CODE.test(line)) return line
  return mapInlineProse(line, (prose) => restoreProse(prose, images))
}

/** 行内正文段：先封存既有的图片/链接结构，再把剩下的裸地址还原成图片 */
function restoreProse(prose: string, images: Map<string, string>): string {
  const stash: string[] = []
  const keep = (s: string): string => {
    stash.push(s)
    return `${MASK_OPEN}${stash.length - 1}${MASK_CLOSE}`
  }
  const rest = prose.replace(RE_MD_STRUCT, (m, bang: string, _label: string, dest: string) => {
    const url = dest.startsWith('<') && dest.endsWith('>') ? dest.slice(1, -1) : dest
    const token = images.get(url)
    if (!token) return keep(m)
    // 已是图片：原样保留。指向笔记图片的链接：还原成图片——笔记图片是本地协议的嵌入资源，
    // 不可能是「要去访问的外链」，模型把它写成链接同样是丢了图片语义
    return keep(bang === '!' ? m : token)
  })
  if (images.size === 0) return rest
  // 单趟扫描：一条正则吃掉全部裸地址/自动链接候选，逐个查表（整串相等才算命中）
  const out = rest.replace(RE_URL_RUN, (run) => {
    const wrapped = run.startsWith('<')
    const inner = wrapped ? run.slice(1, -1) : run
    const hit = images.get(inner)
    if (hit) return keep(hit)
    // 句末标点会被 URL 字符集吃进来（`.` 合法），剥掉再判一次，标点原样留在后面
    const punct = RE_TRAILING_PUNCT.exec(inner)?.[0] ?? ''
    if (!punct) return run
    const bare = inner.slice(0, -punct.length)
    const hit2 = images.get(bare)
    if (!hit2) return run
    return keep(hit2) + punct
  })
  return out.replace(RE_MASK, (_, i: string) => stash[Number(i)])
}
