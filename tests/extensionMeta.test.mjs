/**
 * 扩展分类口径（src/utils/extensionMeta.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（零运行时依赖）：
 *   node --test tests/extensionMeta.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  CATEGORY_ALL,
  CATEGORY_OTHER,
  EXTENSION_CATEGORIES,
  categoryFilterValues,
  displayCategory,
  matchesCategory,
} from '../src/utils/extensionMeta.ts'

test('筛选行：预设七类固定全量展示，不再出现「未分类」桶', () => {
  const chips = categoryFilterValues([null, undefined, ''])
  assert.deepEqual(chips, [CATEGORY_ALL, ...EXTENSION_CATEGORIES])
  assert.ok(!chips.includes('未分类'))
})

test('筛选行：预设之外的历史/手写分类原样追加（字典序），不许吞', () => {
  const chips = categoryFilterValues(['AI', '效率', '媒体管理', null])
  assert.deepEqual(chips, [CATEGORY_ALL, ...EXTENSION_CATEGORIES, '媒体管理', '效率'])
})

test('筛选行：空分类与其他条件混排不产生额外选项', () => {
  const chips = categoryFilterValues(['AI', '  ', '其他'])
  assert.deepEqual(chips, [CATEGORY_ALL, ...EXTENSION_CATEGORIES])
})

test('命中：没填分类的条目归入「其他」，不再有独立「未分类」命中值', () => {
  assert.ok(matchesCategory(null, CATEGORY_OTHER))
  assert.ok(matchesCategory(undefined, CATEGORY_OTHER))
  assert.ok(matchesCategory('', CATEGORY_OTHER))
  assert.ok(matchesCategory('  ', CATEGORY_OTHER))
  assert.ok(matchesCategory('其他', CATEGORY_OTHER))
  assert.ok(!matchesCategory('AI', CATEGORY_OTHER))
})

test('命中：全部放行一切，其余按名字精确匹配（含预设之外的分类）', () => {
  assert.ok(matchesCategory('效率', CATEGORY_ALL))
  assert.ok(matchesCategory('效率', '效率'))
  assert.ok(!matchesCategory('AI', '效率'))
  assert.ok(!matchesCategory(null, '效率'))
})

test('展示：没填分类显示「其他」，有效分类原样返回', () => {
  assert.equal(displayCategory(null), '其他')
  assert.equal(displayCategory(undefined), '其他')
  assert.equal(displayCategory('  '), '其他')
  assert.equal(displayCategory('AI'), 'AI')
  assert.equal(displayCategory(' 效率 '), '效率')
})
