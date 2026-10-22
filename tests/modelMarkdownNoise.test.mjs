/**
 * 模型输出转义噪声回收（src/utils/markdownLines.ts 的 cleanModelMarkdownNoise）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（模块零依赖，Node 24 无需额外工具链）：
 *   node --test tests/modelMarkdownNoise.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { cleanModelMarkdownNoise } from '../src/utils/markdownLines.ts'

test('词中 _ 与 ~ 的转义全量回收（用户报告的原始形状）', () => {
  // 行尾空格被模型写成 &#x20;：还原后还是那个空格
  assert.equal(cleanModelMarkdownNoise('A-\\_\\~!p:@&#x20;'), 'A-_~!p:@ ')
  assert.equal(cleanModelMarkdownNoise('A-\\_\\~!p:@ &#x20;'), 'A-_~!p:@  ')
})

test('&#x20; 实体还原为普通空格（大小写均认）', () => {
  assert.equal(cleanModelMarkdownNoise('结尾空格&#x20;'), '结尾空格 ')
  assert.equal(cleanModelMarkdownNoise('结尾空格&#X20;'), '结尾空格 ')
  assert.equal(cleanModelMarkdownNoise('a&#x20;b'), 'a b')
})

test('独立边界上的 \\* 回收，成对 \\*..\\* 保守保留（还原会变斜体）', () => {
  assert.equal(cleanModelMarkdownNoise('5 \\* 3 = 15'), '5 * 3 = 15')
  assert.equal(cleanModelMarkdownNoise('\\*强调\\* 的字面星号不动'), '\\*强调\\* 的字面星号不动')
})

test('围栏、$$ 公式与行内代码里的转义是正文，原样跳过', () => {
  const fenced = '```\nA-\\_\\~!p:@ &#x20;\n```'
  assert.equal(cleanModelMarkdownNoise(fenced), fenced)
  const math = '$$\nA-\\_\\~!p:@ &#x20;\n$$'
  assert.equal(cleanModelMarkdownNoise(math), math)
  assert.equal(cleanModelMarkdownNoise('前文 `A-\\_\\~!p:@` 后文'), '前文 `A-\\_\\~!p:@` 后文')
})

test('没有噪声的正文原样返回', () => {
  const clean = '# 标题\n\n普通正文，带 **加粗** 与 [链接](https://example.com)。'
  assert.equal(cleanModelMarkdownNoise(clean), clean)
})
