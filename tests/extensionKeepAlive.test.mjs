/**
 * 扩展常驻后台口径（src/utils/extensionKeepAlive.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码（零运行时依赖）：
 *   node --test tests/extensionKeepAlive.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  effectiveKeepAliveIds,
  EXT_KEEP_ALIVE_CAP,
  hasKeepAliveSlot,
} from '../src/utils/extensionKeepAlive.ts'

test('effectiveKeepAliveIds：keep_alive ∪ autostart，保序去重', () => {
  assert.deepEqual(
    effectiveKeepAliveIds({
      extension_keep_alive: ['a', 'b'],
      extension_autostart: ['b', 'c'],
    }),
    ['a', 'b', 'c'],
  )
})

test('effectiveKeepAliveIds：两列表都缺省回空数组，脏值（空串）剔除', () => {
  assert.deepEqual(effectiveKeepAliveIds({}), [])
  assert.deepEqual(effectiveKeepAliveIds({ extension_keep_alive: ['', 'a', ''] }), ['a'])
})

test('hasKeepAliveSlot：未满员可加，满员拒绝', () => {
  const ids = ['a', 'b', 'c', 'd', 'e']
  assert.equal(ids.length, EXT_KEEP_ALIVE_CAP)
  const full = { extension_keep_alive: ids }
  assert.equal(hasKeepAliveSlot(full), false)
  // 排除自己后正好腾出一个名额（对已常驻扩展调整自身开关的判据）
  assert.equal(hasKeepAliveSlot(full, 'a'), true)
})

test('hasKeepAliveSlot：autostart 也计入上限（随应用启动隐含常驻）', () => {
  const full = { extension_autostart: ['a', 'b', 'c', 'd', 'e'] }
  assert.equal(hasKeepAliveSlot(full), false)
  assert.equal(hasKeepAliveSlot({ extension_keep_alive: ['a'], extension_autostart: ['b'] }), true)
})

