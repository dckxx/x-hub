/**
 * 待办导出 Markdown 组装（src/utils/todoExport.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（模块零依赖）：
 *   node --test tests/todoExport.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { buildTodosMarkdown } from '../src/utils/todoExport.ts'

const META = { scope: '本周', time: '2026年11月7日 14:30', filters: ['未完成', '标签：工作'] }

test('整份结构：H1 范围 + meta 引用行 + 分组 H2（带条数）+ 复选框清单', () => {
  const md = buildTodosMarkdown(META, [
    {
      label: '置顶',
      items: [
        {
          title: '交付季度报告',
          done: false,
          badges: ['截止 2026年11月7日 18:00', '紧急'],
          tagNames: ['工作'],
          children: [
            { title: '收集数据', done: false, badges: [], tagNames: [], children: [] },
            { title: '画图表', done: true, badges: ['已完成 2026年11月6日 21:00'], tagNames: [], children: [] },
          ],
        },
      ],
    },
    {
      label: '逾期',
      items: [
        { title: '回复客户邮件', done: false, badges: ['截止 2026年11月5日 14:00'], tagNames: ['工作', '重要客户'], children: [] },
      ],
    },
  ])
  assert.equal(
    md,
    [
      '# 待办导出 · 本周',
      '',
      '> 2026年11月7日 14:30 导出 · 未完成 · 标签：工作 · 共 2 条',
      '',
      '## 置顶（1 条）',
      '',
      '- [ ] 交付季度报告（截止 2026年11月7日 18:00 · 紧急） #工作',
      '  - [ ] 收集数据',
      '  - [x] 画图表（已完成 2026年11月6日 21:00）',
      '',
      '## 逾期（1 条）',
      '',
      '- [ ] 回复客户邮件（截止 2026年11月5日 14:00） #工作 #重要客户',
      '',
    ].join('\n'),
  )
})

test('已完成条目打 [x]，无徽标无标签时不输出圆括号与井号', () => {
  const md = buildTodosMarkdown(META, [
    { label: '无日期', items: [{ title: '随手记一笔', done: true, badges: [], tagNames: [], children: [] }] },
  ])
  const line = md.split('\n').find((l) => l.startsWith('- ['))
  assert.equal(line, '- [x] 随手记一笔')
})

test('只有标签没有徽标时：标题后直接跟 #标签（无空括号）', () => {
  const md = buildTodosMarkdown(META, [
    { label: '今天', items: [{ title: '买咖啡豆', done: false, badges: [], tagNames: ['生活'], children: [] }] },
  ])
  assert.ok(md.includes('- [ ] 买咖啡豆 #生活\n'))
})

test('描述逐行缩进承接，且与条目头之间空一行', () => {
  const md = buildTodosMarkdown(META, [
    {
      label: '今天',
      items: [
        {
          title: '写周报',
          done: false,
          badges: [],
          tagNames: [],
          children: [],
          description: '第一段要点\n第二行',
        },
      ],
    },
  ])
  assert.ok(md.includes('- [ ] 写周报\n\n  第一段要点\n  第二行\n'))
})

test('filters 为空时引用行没有多余的「 · 」', () => {
  const md = buildTodosMarkdown({ scope: '全部', time: '2026年11月7日 10:00', filters: [] }, [])
  assert.equal(
    md,
    '# 待办导出 · 全部\n\n> 2026年11月7日 10:00 导出 · 共 0 条\n\n（当前筛选下没有待办）\n',
  )
})

test('空分组不输出 H2（视图层已过滤空组，这里兜底）', () => {
  const md = buildTodosMarkdown(META, [{ label: '本周', items: [] }])
  assert.ok(!md.includes('## 本周'))
  assert.ok(md.includes('（当前筛选下没有待办）'))
})
