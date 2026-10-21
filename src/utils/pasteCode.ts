/**
 * 粘贴自动成码：识别剪贴板纯文本是否「像代码」并猜测语言。
 * 供 NoteEditor 在 paste 的 capture 阶段调用——判定为代码 → preventDefault 后直接插入
 * 代码块；不像代码（散文/Markdown/链接列表）→ 放行走默认管线（markdown/html 解析）。
 * 背景：从其它 AI 复制代码按钮拿到的是裸代码纯文本（无 ``` 围栏），选中复制还带
 * text/html，Milkdown 的两条解析路都只会落成普通段落，表现为「粘贴的代码没有格式」。
 * 纯函数零依赖，tests/pasteCode.test.mjs 锁口径（node --test 直载 TS 源，import 带扩展名）。
 */

export interface FencedCode {
  code: string
  language: string
}

/**
 * 整段文本恰为一个 ```/~~~ 围栏块时解出语言与内容；夹在散文里的围栏不认
 * （返回 null 走默认管线，避免误吞围栏外的文字）。
 */
export function unwrapFencedCode(raw: string): FencedCode | null {
  const text = raw.replace(/\r\n/g, '\n').replace(/^\n+/, '').replace(/\s+$/, '')
  // 语言行尾允许空格/制表符（```js␠ 这类从文档里复制常见的尾随空白）
  const m = /^(`{3,}|~{3,})([\w.+-]*)[ \t]*\n([\s\S]*?)\n?\1\s*$/.exec(text)
  if (!m) return null
  return { code: m[3].replace(/\n+$/, ''), language: m[2] || '' }
}

/** 强代码特征词（刻意排除 if/for/while/do/as/from/in 等常见英文词，防散文误判） */
const CODE_KEYWORDS = new Set([
  'const', 'let', 'function', 'def', 'elif', 'lambda', 'fn', 'mut', 'impl', 'struct',
  'typedef', 'namespace', 'std', 'cout', 'endl', 'printf', 'scanf', 'println', 'require',
  'await', 'async', 'typeof', 'yield', 'nil', 'sudo', 'npm', 'npx', 'pnpm', 'yarn', 'pip',
  'git', 'docker', 'kubectl', 'curl', 'wget', 'chmod', 'chown', 'grep', 'awk', 'sed',
  'apt', 'brew', 'cd', 'mkdir', 'select', 'where', 'join', 'having', 'limit', 'offset',
  'primary', 'foreign', 'null', 'true', 'false', 'none', 'void', 'int', 'float', 'double',
  'bool', 'return', 'import', 'export', 'class', 'public', 'private', 'static', 'throws',
  'throw', 'try', 'catch', 'finally', 'package', 'func', 'var', 'echo', 'update', 'delete',
])

/** 代码专属符号序列（逗号句号等散文也有的不算） */
const CODE_SYMBOLS = ['=>', '===', '!==', '->', '::', '&&', '||', '${', '</', '++', '--']

/** CLI 动词：行首出现即算命令行（AI 给的安装/部署命令块的常见形态是多行连续命令） */
const COMMAND_VERBS = new Set([
  'git', 'npm', 'npx', 'pnpm', 'yarn', 'pip', 'python', 'python3', 'node', 'cd', 'sudo',
  'apt', 'apt-get', 'brew', 'docker', 'kubectl', 'curl', 'wget', 'chmod', 'chown', 'export',
  'alias', 'source', 'tar', 'unzip', 'ssh', 'scp', 'cargo', 'echo', 'mkdir', 'ls', 'rm',
])

/**
 * 判定多行纯文本是否「像代码」。保守取向：
 * - 单行不认（口令/链接/短句歧义太大，宁可走默认管线）；
 * - CJK 占比 > 25% 不认（中文为主的段落绝不劫持；代码夹中文注释不受影响）；
 * - Markdown 结构（标题/列表/引用/表格行占多数）与含围栏的混合文本不认；
 * - 其余按信号打分（缩进/行尾标点/关键词/符号/赋值/注释/提示符…），≥4 分才认。
 * 误判代价已定价：一次 Ctrl+Z 即回到纯文本。
 */
export function looksLikeCode(raw: string): boolean {
  const text = raw.replace(/\r\n/g, '\n').trim()
  if (!text || !text.includes('\n')) return false
  if (/`{3,}|~{3,}/.test(text)) return false // 围栏只认「整段即围栏」，混合文本不碰
  const nonEmpty = text.split('\n').filter((l) => l.trim() !== '')
  if (nonEmpty.length < 2) return false
  // Markdown 结构文本交给默认管线
  const mdLines = nonEmpty.filter((l) =>
    /^(#{1,6}\s|>\s|\s*[-*+]\s|\s*\d+[.)]\s|\s*\|.*\|)/.test(l),
  )
  if (mdLines.length * 2 >= nonEmpty.length) return false
  // CJK 为主 → 散文/资料
  const glyphs = text.replace(/\s/g, '')
  const cjk = (text.match(/[\u4e00-\u9fff\u3040-\u30ff\uac00-\ud7af]/g) ?? []).length
  if (glyphs.length > 0 && cjk / glyphs.length > 0.25) return false
  // 纯链接列表不是代码
  if (nonEmpty.every((l) => /^(https?:\/\/|www\.)\S+$/i.test(l.trim()))) return false

  let score = 0
  // 缩进（首行缩进可能是散文排版，任意行缩进即给分，多行缩进加成）
  const indented = nonEmpty.filter((l) => /^([ \t]{2,}|\t)/.test(l))
  if (indented.length > 0) score += 2
  if (indented.length >= 3) score += 1
  // 行尾代码标点（; { } ,）
  const punctEnd = nonEmpty.filter((l) => /[;{}]\s*$/.test(l) || /,\s*$/.test(l)).length
  score += Math.min(2, punctEnd)
  // 强代码关键词（去重计数）
  const words = text.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []
  const kw = new Set(words.filter((w) => CODE_KEYWORDS.has(w.toLowerCase())))
  score += Math.min(3, kw.size)
  // 命令行：多行以 CLI 动词开头（≥2 行即强信号；单行只给少量分，散文祈使句不致达标）
  const cliLines = nonEmpty.filter(
    (l) => COMMAND_VERBS.has(l.trim().split(/\s+/)[0]?.toLowerCase() ?? ''),
  )
  if (cliLines.length >= 2) score += 3
  else if (cliLines.length === 1) score += 1
  // SQL：全大写关键字行 ≥2（区分大小写，英文祈使句 "Update the docs" 不致命中）
  const sqlCaps = nonEmpty.filter((l) =>
    /^\s*(SELECT|INSERT|UPDATE|DELETE|CREATE|DROP|ALTER|FROM|WHERE|JOIN|ORDER|GROUP|HAVING|LIMIT|SET|VALUES)\b/.test(
      l,
    ),
  )
  if (sqlCaps.length >= 2) score += 2
  // 代码符号序列
  let sym = 0
  for (const s of CODE_SYMBOLS) if (text.includes(s)) sym++
  score += Math.min(2, sym)
  // 赋值行 / key: value 对 / 行尾冒号（yaml）
  if (nonEmpty.filter((l) => /\S+\s*=[^=]/.test(l)).length >= 2) score += 1
  if (nonEmpty.some((l) => /^\s*[\w.$-]+\s*:\s+\S/.test(l))) score += 1
  if (nonEmpty.filter((l) => /[;:]\s*$/.test(l)).length >= 2) score += 1
  // shell 痕迹：shebang > 提示符 > 命令行旗标 / 注释行
  if (nonEmpty.some((l) => /^#!\s*\//.test(l))) score += 3
  else if (nonEmpty.some((l) => /^\$\s/.test(l))) score += 2
  if (/(^|\s)--?[a-zA-Z][\w-]+/.test(text)) score += 1
  if (nonEmpty.some((l) => /^\s*(\/\/|#|\/\*)/.test(l))) score += 1
  // 负信号：散文句读与超长句
  const sentenceEnds = nonEmpty.filter((l) => /[.!?…]["')\]]*\s*$/.test(l.trim())).length
  if (sentenceEnds >= 2) score -= 2
  const longLines = nonEmpty.filter((l) => l.trim().split(/\s+/).length >= 9).length
  if (longLines * 2 >= nonEmpty.length) score -= 2
  return score >= 4
}

/** 从裸代码猜语言（按歧义度从低到高逐类排除；认不出返回空串，代码块不带语言标注） */
export function guessCodeLanguage(raw: string): string {
  const t = raw.replace(/\r\n/g, '\n').trim()
  if (!t) return ''
  if (/^[[{]/.test(t) && /[\]}]$/.test(t)) {
    try {
      JSON.parse(t)
      return 'json'
    } catch {
      /* 不是合法 JSON，继续猜 */
    }
  }
  if (/<!doctype html|<html[\s>]/i.test(t)) return 'html'
  if (
    /^#!.*(bash|zsh|sh)\b/m.test(t) ||
    /^\$\s/m.test(t) ||
    /^\s*(sudo|apt|brew|npm|npx|pnpm|yarn|pip|git|docker|kubectl|curl|wget|mkdir|chmod|export)\b/m.test(t)
  ) {
    return 'bash'
  }
  if (/^\s*def\s+\w*\(|^\s*(import|from)\s+[\w.]+\s*$|^\s*print\(|^\s*elif\b/.test(t)) {
    return 'python'
  }
  if (/\bfn\s+\w+|let\s+mut\b|\bimpl\s+\w+/.test(t)) return 'rust'
  if (/\bfunc\s+\w+|package\s+main|fmt\.\w+\(/.test(t)) return 'go'
  if (/public\s+(static\s+)?(class|void)\s|System\.out\.print/.test(t)) return 'java'
  if (/using\s+System|Console\.Write|namespace\s+\w+/.test(t)) return 'csharp'
  if (/#include\s*<|std::|cout\s*<</.test(t)) return 'cpp'
  if (/#include\s*"|printf\(/.test(t)) return 'c'
  if (/\bselect\b[\s\S]*\bfrom\b|\binsert\s+into|\bcreate\s+table|\bupdate\s+\w+\s+set/i.test(t)) {
    return 'sql'
  }
  if (/^(\s*)[.#][\w-]+\s*\{/m.test(t) && /:\s*[^;{}]+;/m.test(t)) return 'css'
  if (/(interface|type)\s+\w+\s*[={]|:\s*(string|number|boolean|any)\b/.test(t)) {
    return 'typescript'
  }
  if (/\b(const|let|var|function)\b|=>|console\.(log|error|warn)|require\(|document\./.test(t)) {
    return 'javascript'
  }
  if (/^[\w.-]+:\s/m.test(t) && !/[{};]/.test(t)) return 'yaml'
  return ''
}
