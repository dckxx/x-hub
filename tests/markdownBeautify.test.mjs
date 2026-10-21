/**
 * 一键美化格式（src/utils/markdownBeautify.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（模块零依赖，Node 24 无需额外工具链）：
 *   node --test tests/markdownBeautify.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { beautifyNoteMarkdown } from '../src/utils/markdownBeautify.ts'

const ZWSP = '\u200B'

test('盘古之白：中英文/数字之间加空格，双向覆盖', () => {
  assert.equal(beautifyNoteMarkdown('中文English混排'), '中文 English 混排')
  assert.equal(beautifyNoteMarkdown('版本2发布'), '版本 2 发布')
  assert.equal(beautifyNoteMarkdown('GPT-4o模型上线'), 'GPT-4o 模型上线')
  assert.equal(beautifyNoteMarkdown('A中B'), 'A 中 B')
})

test('盘古之白：已规范文本幂等，不重复加空格', () => {
  const once = beautifyNoteMarkdown('使用 IDE 开发第 3 版')
  assert.equal(beautifyNoteMarkdown(once), once)
})

test('盘古之白：全角标点与相邻文字之间不加空格', () => {
  assert.equal(beautifyNoteMarkdown('中文，English'), '中文，English')
  assert.equal(beautifyNoteMarkdown('完成！下一步：验证'), '完成！下一步：验证')
})

test('行内代码与围栏、缩进代码整段不动', () => {
  assert.equal(beautifyNoteMarkdown('前文`中文English`后文'), '前文`中文English`后文')
  const fenced = '```\nconst 中文A = 1\n* item\n# 标题\n```'
  assert.equal(beautifyNoteMarkdown(fenced), fenced)
  const math = '$$\n中文English = 1\n$$'
  assert.equal(beautifyNoteMarkdown(math), math)
  const indented = '正文一段\n\n    const 中文A = 1\n\n下一段'
  assert.equal(beautifyNoteMarkdown(indented), indented)
})

test('双链、链接 URL、引用标签内部不加空格（键值精确匹配不可破坏）', () => {
  assert.equal(beautifyNoteMarkdown('见[[中文A笔记]]和[[Note B]]'), '见[[中文A笔记]]和[[Note B]]')
  assert.equal(beautifyNoteMarkdown('[文A](https://x.com/中文1)'), '[文 A](https://x.com/中文1)')
  assert.equal(beautifyNoteMarkdown('参考[文A][ref1]'), '参考[文 A][ref1]')
})

test('裸 URL：起始边界加空格，URL 内部保持原样', () => {
  assert.equal(
    beautifyNoteMarkdown('详见https://a.com/页面2完。'),
    '详见 https://a.com/页面2完。',
  )
})

test('强调标记内侧不插空格，内容正常加空格', () => {
  assert.equal(beautifyNoteMarkdown('**中文English**'), '**中文 English**')
  assert.equal(beautifyNoteMarkdown('中文**English**中文'), '中文**English**中文')
})

test('硬换行：行尾反斜杠保留，前置空白可清；转义反斜杠与其后空格硬换行分得清', () => {
  assert.equal(beautifyNoteMarkdown('第一行A\\ \n第二行'), '第一行 A\\\n第二行')
  assert.equal(beautifyNoteMarkdown('转义\\\\  \n下一行'), '转义\\\\  \n下一行')
  assert.equal(beautifyNoteMarkdown('空格硬换行  \n下一行'), '空格硬换行  \n下一行')
  assert.equal(beautifyNoteMarkdown('单个尾格 \n下一行'), '单个尾格\n下一行')
})

test('零宽空格占位行：归一为单字符但绝不合并、绝不删除', () => {
  assert.equal(beautifyNoteMarkdown(`a\n${ZWSP}\nb`), `a\n${ZWSP}\nb`)
  assert.equal(beautifyNoteMarkdown(`a\n  ${ZWSP}  \nb`), `a\n${ZWSP}\nb`)
  assert.equal(beautifyNoteMarkdown(`a\n\u00A0\nb`), `a\n${ZWSP}\nb`)
  // 纯空行收敛为 1 个，占位行保持独立（占位行前后的单个结构空行是规范形态，保留）
  assert.equal(beautifyNoteMarkdown(`a\n\n\n\nb`), 'a\n\nb')
  assert.equal(beautifyNoteMarkdown(`a\n\n${ZWSP}\n\nb`), `a\n\n${ZWSP}\n\nb`)
})

test('无序列表标记统一为 -，缩进与有序列表写法保持', () => {
  assert.equal(beautifyNoteMarkdown('* 项A'), '- 项 A')
  assert.equal(beautifyNoteMarkdown('+ 项B'), '- 项 B')
  assert.equal(beautifyNoteMarkdown('  * 嵌套C'), '  - 嵌套 C')
  assert.equal(beautifyNoteMarkdown('1) 第一项D'), '1) 第一项 D')
  assert.equal(beautifyNoteMarkdown('1.   第二项E'), '1. 第二项 E')
  // 行首强调不是列表
  assert.equal(beautifyNoteMarkdown('*强调*文字F'), '*强调*文字 F')
})

test('标题：内部空格归一、去闭合 #，前后保证恰好一个空行', () => {
  assert.equal(beautifyNoteMarkdown('##  标题G  ##'), '## 标题 G')
  assert.equal(beautifyNoteMarkdown('正文\n# 标题H\n续文'), '正文\n\n# 标题 H\n\n续文')
  assert.equal(beautifyNoteMarkdown('# 已隔开\n\n正文'), '# 已隔开\n\n正文')
  // 占位空行本身已是分隔，不再追加
  assert.equal(beautifyNoteMarkdown(`# 标题I\n${ZWSP}\n正文`), `# 标题 I\n${ZWSP}\n正文`)
  // 行首无空格的 #标签 是普通文本（不变成标题；盘古对普通文本照常生效）
  assert.equal(beautifyNoteMarkdown('#标签J'), '#标签 J')
  assert.equal(beautifyNoteMarkdown('#标签'), '#标签')
})

test('分隔线：星/下划线收成 --- 并保证前方空行；横线形态一律不动', () => {
  assert.equal(beautifyNoteMarkdown('段落甲\n\n***\n\n段落乙'), '段落甲\n\n---\n\n段落乙')
  // 紧贴文本时先补空行，避免 --- 变 setext 下划线（--- 之后不强制空行）
  assert.equal(beautifyNoteMarkdown('段落丙\n***\n段落丁'), '段落丙\n\n---\n段落丁')
  // 横线形态紧贴文本 = setext 二级标题，保持原样不动
  assert.equal(beautifyNoteMarkdown('小标题\n---\n正文'), '小标题\n---\n正文')
  assert.equal(beautifyNoteMarkdown('段落戊\n- - -\n段落己'), '段落戊\n- - -\n段落己')
})

test('引用与表格正常处理，标记不动', () => {
  assert.equal(beautifyNoteMarkdown('> 引用文字R'), '> 引用文字 R')
  assert.equal(beautifyNoteMarkdown('>- 引用列表S'), '>- 引用列表 S')
  assert.equal(beautifyNoteMarkdown('| 中文列T | English |'), '| 中文列 T | English |')
  assert.equal(beautifyNoteMarkdown('|---|:---:|'), '|---|:---:|')
})

test('front-matter 整块跳过，正文照常处理', () => {
  const src = '---\ntitle: 中文笔记U\n---\n\n正文English'
  assert.equal(beautifyNoteMarkdown(src), '---\ntitle: 中文笔记U\n---\n\n正文 English')
})

test('整体幂等：混乱样例美化两次结果一致', () => {
  const messy = [
    '#  项目笔记V  ',
    '*  要点一W',
    '*要点二X',
    '',
    '',
    '',
    '中文English正文***',
    '',
    '```',
    'code 中文Y  ',
    '```',
    '##  小节Z  ##',
    '结尾行\\ ',
  ].join('\n')
  const once = beautifyNoteMarkdown(messy)
  assert.equal(beautifyNoteMarkdown(once), once)
})

test('空内容原样返回', () => {
  assert.equal(beautifyNoteMarkdown(''), '')
  assert.equal(beautifyNoteMarkdown('\n\n'), '')
})

test('盘古之白：双链内嵌 URL 不插空格，裸 URL 照常补', () => {
  // 双链键是标题精确匹配：[[笔记https://x.com/a]] 内部加空格 = 改键断链
  assert.equal(
    beautifyNoteMarkdown('见[[笔记https://x.com/a]]尾'),
    '见[[笔记https://x.com/a]]尾',
  )
  // 裸 URL 起始边界照常补空格（URL 本体不动）
  assert.equal(
    beautifyNoteMarkdown('见https://x.com/a_b 尾'),
    '见 https://x.com/a_b 尾',
  )
  // 行内链接的 URL 括号段是遮蔽段：内部与邻接处都不插空格
  assert.equal(
    beautifyNoteMarkdown('看[文档](https://x.com/a_b)说明'),
    '看[文档](https://x.com/a_b)说明',
  )
})
