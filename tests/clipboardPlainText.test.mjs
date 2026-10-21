/**
 * 所见即所得复制（src/utils/clipboardPlainText.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（核心零运行时依赖，Node 24 无需额外工具链）：
 *   node --test tests/clipboardPlainText.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { blocksToPlainText, copyPlainText } from '../src/utils/clipboardPlainText.ts'

const ZWSP = '\u200B'

function t(text, marks) {
  return { name: 'text', text, inline: true, ...(marks ? { marks } : {}) }
}
function p(...children) {
  return { name: 'paragraph', textblock: true, children }
}
function h(level, ...children) {
  return { name: 'heading', textblock: true, attrs: { level }, children }
}

test('多段落以空行分隔，输出无任何 Markdown 转义符', () => {
  const out = blocksToPlainText([
    p(t('第一段：5 * 3 与 [括号] 原样')),
    p(t('第二段，联系 a@b.com')),
  ])
  assert.equal(out, '第一段：5 * 3 与 [括号] 原样\n\n第二段，联系 a@b.com')
})

test('链接只留可见文字，邮箱不出现尖括号或 mailto', () => {
  // mark 不进入纯结构（toPlainNode 天然丢弃），复制结果只有可见文字
  const out = blocksToPlainText([p(t('官网 ', { type: 'link' }), t('点这里', { type: 'link' }))])
  assert.equal(out, '官网 点这里')
  assert.ok(!out.includes('mailto'))
  assert.ok(!out.includes('<'))
})

test('标题退化为纯文本行（无 # 前缀）', () => {
  assert.equal(blocksToPlainText([h(2, t('二级标题'))]), '二级标题')
})

test('无序列表逐行圆点，有序列表带序号，嵌套缩进两格', () => {
  const out = blocksToPlainText([
    {
      name: 'bullet_list',
      children: [
        { name: 'list_item', children: [p(t('甲'))] },
        {
          name: 'list_item',
          children: [p(t('乙')), { name: 'ordered_list', children: [
            { name: 'list_item', children: [p(t('乙.1'))] },
            { name: 'list_item', children: [p(t('乙.2'))] },
          ] }],
        },
      ],
    },
  ])
  assert.equal(out, '• 甲\n• 乙\n  1. 乙.1\n  2. 乙.2')
})

test('有序列表优先用条目 label，缺省时按父级 order 递增', () => {
  const out = blocksToPlainText([
    {
      name: 'ordered_list',
      attrs: { order: 3 },
      children: [
        { name: 'list_item', attrs: { label: '3.' }, children: [p(t('第三'))] },
        { name: 'list_item', children: [p(t('第四'))] },
      ],
    },
  ])
  assert.equal(out, '3. 第三\n4. 第四')
})

test('任务列表带勾选态符号', () => {
  const out = blocksToPlainText([
    {
      name: 'bullet_list',
      children: [
        { name: 'list_item', attrs: { checked: true }, children: [p(t('已完成'))] },
        { name: 'list_item', attrs: { checked: false }, children: [p(t('未完成'))] },
      ],
    },
  ])
  assert.equal(out, '☑ 已完成\n☐ 未完成')
})

test('代码块按字面多行输出，不拆转义', () => {
  const out = blocksToPlainText([
    { name: 'code_block', code: true, text: 'const a = 1\nconst b = "x\\*y"' },
    p(t('后文')),
  ])
  assert.equal(out, 'const a = 1\nconst b = "x\\*y"\n\n后文')
})

test('表格输出为制表符分隔的行', () => {
  const cell = (t2) => ({ name: 'table_cell', children: [p(t(t2))] })
  const head = (t2) => ({ name: 'table_header', children: [p(t(t2))] })
  const out = blocksToPlainText([
    {
      name: 'table',
      children: [
        { name: 'table_header_row', children: [head('名称'), head('邮箱')] },
        { name: 'table_row', children: [cell('张三'), cell('z@s.com')] },
      ],
    },
  ])
  assert.equal(out, '名称\t邮箱\n张三\tz@s.com')
})

test('段内选区（纯行内片段）直接行内序列化，硬换行落成换行', () => {
  const out = blocksToPlainText([t('前半'), { name: 'hard_break', inline: true }, t('后半')])
  assert.equal(out, '前半\n后半')
})

test('空行占位（零宽空格）剥离，空段保留为空行', () => {
  const out = blocksToPlainText([p(t('上')), p(t(ZWSP)), p(t('下'))])
  assert.equal(out, '上\n\n\n\n下')
  assert.ok(!out.includes(ZWSP))
})

test('引用块退化为内部内容，分隔线与无说明图片各占一个空行分组', () => {
  const out = blocksToPlainText([
    { name: 'blockquote', children: [p(t('引用行'))] },
    { name: 'hr' },
    { name: 'image-block', attrs: { caption: '' } },
  ])
  assert.equal(out, '引用行\n\n\n\n')
})

test('图片块带说明时保留说明文字', () => {
  const out = blocksToPlainText([{ name: 'image-block', attrs: { caption: '截图说明' } }])
  assert.equal(out, '截图说明')
})

// ---- copyPlainText（按选区分流：段内无标记 / 跨块保留结构）----
// 最小 PM 形状 mock：toPlainNode 只认 isText/isLeaf/isTextblock/type.spec.code/
// attrs/textContent/content.forEach；ResolvedPos 只认 sameParent/parent/parentOffset。

function pmTextblock(name, text, { code = false, attrs = {} } = {}) {
  return {
    isText: false,
    isLeaf: false,
    isBlock: true,
    isTextblock: true,
    type: { name, spec: { code } },
    attrs,
    textContent: text,
    content: {
      forEach(cb) {
        Array.from(text).forEach((ch, i) => cb({ isText: true, isLeaf: false, text: ch }, i, i))
      },
    },
    cut(from, to) {
      return pmTextblock(name, text.slice(from, to), { code, attrs })
    },
  }
}

function pmBlock(name, children, attrs = {}) {
  return {
    isText: false,
    isLeaf: false,
    isBlock: true,
    isTextblock: false,
    type: { name, spec: {} },
    attrs,
    textContent: children.map((c) => c.textContent ?? '').join(''),
    content: { forEach(cb) { children.forEach((c, i) => cb(c, i, i)) } },
  }
}

function pmSel(parent, from, to) {
  const pos = (offset) => ({
    parent,
    parentOffset: offset,
    depth: 1,
    sameParent(o) {
      return o.parent === this.parent && o.depth === this.depth
    },
  })
  return { $from: pos(from), $to: pos(to) }
}

const sliceWith = (...nodes) => ({ content: { forEach(cb) { nodes.forEach((n, i) => cb(n, 0, i)) } } })

test('段内选区（列表项里的一个词）不带圆点——复制账号密码直接可用', () => {
  // 真实场景：TextSelection.content() 的 slice 连着 bullet_list 整条祖先链，
  // copyPlainText 必须改走 parent.cut 的行内内容，绝不看列表祖先
  const para = pmTextblock('paragraph', '用户名 admin 密码 123456')
  assert.equal(copyPlainText(sliceWith(pmBlock('bullet_list', [])), pmSel(para, 4, 9)), 'admin')
})

test('段内选区在代码块内按字面输出', () => {
  const code = pmTextblock('code_block', 'const a = "x"', { code: true })
  assert.equal(copyPlainText(sliceWith(), pmSel(code, 6, 13)), 'a = "x"')
})

test('跨块选区（拖过多行 / Ctrl+A 整体复制）保留列表结构', () => {
  const list = pmBlock('bullet_list', [
    pmBlock('list_item', [pmTextblock('paragraph', '账号 admin')]),
    pmBlock('list_item', [pmTextblock('paragraph', '密码 123456')]),
  ])
  const doc = pmBlock('doc', [list])
  // 起止不在同一文本块：sameParent 恒 false
  const sel = {
    $from: { parent: doc, parentOffset: 0, depth: 0, sameParent: () => false },
    $to: { parent: doc, parentOffset: 99, depth: 0, sameParent: () => false },
  }
  assert.equal(copyPlainText(sliceWith(list), sel), '• 账号 admin\n• 密码 123456')
})
