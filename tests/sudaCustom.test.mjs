/**
 * 工作台「自定义速达」槽位内容口径（src/utils/sudaCustom.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（零运行时依赖）：
 *   node --test tests/sudaCustom.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  SUDA_CUSTOM_SOURCE_OPTIONS,
  sudaCustomConfigured,
  sudaCustomItems,
  sudaCustomSourceLabel,
} from '../src/utils/sudaCustom.ts'

/** 最小资源夹具：只带口径相关字段 */
function res(id, extra = {}) {
  return {
    id,
    kind: 'app',
    name: `资源${id}`,
    target: '',
    category: null,
    zone_id: null,
    ...extra,
  }
}

test('pinned：按 resource_ids 顺序输出，已删除的资源自动跳过', () => {
  const resources = [res(1), res(2), res(3)]
  const cfg = { id: 'suda1', source: 'pinned', subcategory: '', zone_id: null, resource_ids: [3, 9, 1] }
  assert.deepEqual(
    sudaCustomItems(cfg, resources).map((r) => r.id),
    [3, 1],
  )
  assert.equal(sudaCustomConfigured(cfg), true)
  assert.equal(sudaCustomConfigured({ ...cfg, resource_ids: [] }), false)
})

test('app/web/file：整个大类，store 顺序', () => {
  const resources = [res(1, { kind: 'web' }), res(2, { kind: 'app' }), res(3, { kind: 'web' })]
  for (const kind of ['app', 'web', 'file']) {
    const cfg = { id: 'suda1', source: kind, subcategory: '', zone_id: null, resource_ids: [] }
    assert.deepEqual(
      sudaCustomItems(cfg, resources).map((r) => r.id),
      kind === 'app' ? [2] : kind === 'web' ? [1, 3] : [],
    )
    assert.equal(sudaCustomConfigured(cfg), true)
  }
})

test('subcategory：小类路径命中或为其子孙级；未配置为空', () => {
  const resources = [
    res(1, { category: '开发' }),
    res(2, { category: '开发/前端' }),
    res(3, { category: '开发/前端/工具' }),
    res(4, { category: '娱乐' }),
  ]
  const cfg = { id: 'suda1', source: 'subcategory', subcategory: '开发/前端', zone_id: null, resource_ids: [] }
  assert.deepEqual(
    sudaCustomItems(cfg, resources).map((r) => r.id),
    [2, 3],
  )
  assert.equal(sudaCustomConfigured(cfg), true)
  assert.deepEqual(sudaCustomItems({ ...cfg, subcategory: '' }, resources), [])
  assert.equal(sudaCustomConfigured({ ...cfg, subcategory: '' }), false)
})

test('zone：分区成员实时过滤（store 序），未指定分区 = 空', () => {
  const resources = [
    res(1, { zone_id: 7 }),
    res(2, { zone_id: null }),
    res(3, { zone_id: 7 }),
    res(4, { zone_id: 8 }),
  ]
  const cfg = { id: 'suda1', source: 'zone', subcategory: '', zone_id: 7, resource_ids: [] }
  assert.deepEqual(
    sudaCustomItems(cfg, resources).map((r) => r.id),
    [1, 3],
  )
  // 分区被删（delete_zone 单事务置 NULL）或配置残留悬空 id：成员自然清空，不出内容
  assert.deepEqual(sudaCustomItems({ ...cfg, zone_id: 99 }, resources), [])
  assert.equal(sudaCustomConfigured(cfg), true)
  assert.equal(sudaCustomConfigured({ ...cfg, zone_id: null }), false)
})

test('未配置槽位（undefined）：空列表 + 未配置 + 空来源名', () => {
  assert.deepEqual(sudaCustomItems(undefined, [res(1)]), [])
  assert.equal(sudaCustomConfigured(undefined), false)
  assert.equal(sudaCustomSourceLabel(undefined), '')
})

test('来源选项含「分区」，来源名可反查', () => {
  assert.ok(SUDA_CUSTOM_SOURCE_OPTIONS.some((o) => o.value === 'zone' && o.label === '分区'))
  const cfg = { id: 'suda1', source: 'zone', subcategory: '', zone_id: null, resource_ids: [] }
  assert.equal(sudaCustomSourceLabel(cfg), '分区')
})
