/**
 * 笔记图片语法回收（src/utils/noteImageSyntax.ts）单测。
 * 直接以 node:test 加载 TS 源码（模块零依赖，Node 24 原生类型剥离）：
 *   node --test tests/noteImageSyntax.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { collectNoteImages, restoreNoteImageSyntax } from '../src/utils/noteImageSyntax.ts'

const H1 = 'http://xhub-note.localhost/c8e87d3367a7cedb.png'
const H2 = 'http://xhub-note.localhost/6fdc23f736a6573d.png'
const A = 'https://example.com/article'

test('抽取原稿图片：地址 → 原样语法，含 alt 与说明', () => {
  const md = `相关截图\n\n![1.00](${H1})\n\n![0.75](${H2} "流程说明")`
  const got = collectNoteImages(md)
  assert.equal(got.size, 2)
  assert.equal(got.get(H1), `![1.00](${H1})`)
  assert.equal(got.get(H2), `![0.75](${H2} "流程说明")`)
})

test('抽取原稿图片：围栏/行内代码里的字面图片语法不算图片', () => {
  const md = ['```', `![x](${H1})`, '```', `正文 \`![y](${H2})\``].join('\n')
  assert.equal(collectNoteImages(md).size, 0)
})

test('抽取原稿图片：顶层缩进代码里的字面图片语法不算图片（与还原侧口径一致）', () => {
  const md = `    ![x](${H1})\n`
  assert.equal(collectNoteImages(md).size, 0)
  // 代码块里的地址因此不会被登记：AI 把它搬出代码块也不还原成图片
  const moved = `- ${H1}`
  assert.equal(restoreNoteImageSyntax(md, moved), moved)
})

test('回收：列表项里的裸地址还原成图片（AI 整理的典型产物）', () => {
  const original = `相关截图\n\n![1.00](${H1})\n\n![1.00](${H2})`
  const ai = `## 相关截图\n\n- ${H1}\n- ${H2}\n`
  const out = restoreNoteImageSyntax(original, ai)
  assert.match(out, new RegExp(`- !\\[1\\.00\\]\\(${H1.replace(/[.]/g, '\\.')}\\)`))
  assert.match(out, new RegExp(`- !\\[1\\.00\\]\\(${H2.replace(/[.]/g, '\\.')}\\)`))
  assert.ok(!out.includes(`- ${H1}`), '裸地址行应已被还原')
})

test('回收：自动链接形态 <地址> 也还原', () => {
  const out = restoreNoteImageSyntax(`![1.00](${H1})`, `- <${H1}>`)
  assert.ok(out.includes(`![1.00](${H1})`), out)
  assert.ok(!out.includes(`<${H1}>`), out)
})

test('回收：指向笔记图片的普通链接还原成图片；其它外链不动', () => {
  const out = restoreNoteImageSyntax(
    `![1.00](${H1})\n\n参考 [文档](${A})`,
    `截图 [见这里](${H1}) 与 [文档](${A})`,
  )
  assert.ok(out.includes(`![1.00](${H1})`), out)
  assert.ok(out.includes(`[文档](${A})`), out)
  assert.ok(!out.includes(`[见这里](${H1})`), out)
})

test('回收：已经写对的图片原样保留，不重写 alt/说明', () => {
  const original = `![1.00](${H1})`
  const already = `![0.75](${H1} "已有说明")`
  assert.equal(restoreNoteImageSyntax(original, already), already)
})

test('回收：原稿没有图片时零副作用（正文原样返回）', () => {
  const original = `纯文本笔记，含外链 ${A}`
  const ai = `## 整理\n\n- ${A}\n- 另一条`
  assert.equal(restoreNoteImageSyntax(original, ai), ai)
  assert.equal(restoreNoteImageSyntax('', `- ${H1}`), `- ${H1}`)
})

test('回收：整段正文里的地址（非列表）正常还原', () => {
  const out = restoreNoteImageSyntax(`![1.00](${H1})`, `相关截图：${H1}。`)
  assert.equal(out, `相关截图：![1.00](${H1})。`)
})

test('回收：地址后面跟着更长路径时不误还原', () => {
  const ai = `- ${H1}/extra/path`
  assert.equal(restoreNoteImageSyntax(`![1.00](${H1})`, ai), ai)
})

test('回收：地址嵌在别的 URL 里（前面还接着字符）不误还原', () => {
  const ai = `- https://host/?u=${H1}`
  assert.equal(restoreNoteImageSyntax(`![1.00](${H1})`, ai), ai)
})

test('回收：围栏/$$ 公式/顶层缩进代码里的地址不碰', () => {
  const original = `![1.00](${H1})`
  const fenced = ['```', `参考 ${H1}`, '```'].join('\n')
  assert.equal(restoreNoteImageSyntax(original, fenced), fenced)
  const math = ['$$', H1, '$$'].join('\n')
  assert.equal(restoreNoteImageSyntax(original, math), math)
  const indented = `    ${H1}`
  assert.equal(restoreNoteImageSyntax(original, indented), indented)
})

test('回收：front-matter 整块不动', () => {
  const original = `![1.00](${H1})`
  const md = `---\ncover: ${H1}\n---\n\n正文`
  const out = restoreNoteImageSyntax(original, md)
  assert.ok(out.startsWith(`---\ncover: ${H1}\n---`), out)
})

test('回收：同一地址多份全部还原，且幂等', () => {
  const original = `![1.00](${H1})`
  const ai = `- ${H1}\n- ${H1}`
  const once = restoreNoteImageSyntax(original, ai)
  assert.equal((once.match(/!\[1\.00\]/g) ?? []).length, 2)
  assert.equal(restoreNoteImageSyntax(original, once), once, '幂等：再跑一次不变')
})

test('回收：句末标点与中文相邻时正常还原', () => {
  const out = restoreNoteImageSyntax(`![1.00](${H1})`, `截图见${H1}，共 6 张`)
  assert.equal(out, `截图见![1.00](${H1})，共 6 张`)
})

test('回收：句末 ASCII 句点不被吞进地址，标点保留在图片后', () => {
  const out = restoreNoteImageSyntax(`![1.00](${H1})`, `截图见 ${H1}.`)
  assert.equal(out, `截图见 ![1.00](${H1}).`)
})

test('回收：Markdown 链接目标与裸地址混排时各自正确', () => {
  const out = restoreNoteImageSyntax(
    `![1.00](${H1})`,
    `- [截图](${H1})\n- ${H1}`,
  )
  const lines = out.split('\n')
  assert.equal(lines[0], `- ![1.00](${H1})`)
  assert.equal(lines[1], `- ![1.00](${H1})`)
})
