/**
 * 笔记列表刷新的补拉判定（src/utils/noteRefresh.ts）单测。
 * 直接以 node:test 加载 TS 源码（零运行时依赖）：
 *   node --test tests/noteRefresh.test.mjs
 *
 * 锁住的口径（2026-11-04 修「AI 助手说已记录、速记里看不到、重启才出现」）：
 * `updated_at` 变了就必须补拉正文——只按 id 命中就沿用本地旧正文，
 * 外部写入（AI 工具 / 云同步 / 其它窗口）永远进不了界面。
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { needsFullNoteFetch } from '../src/utils/noteRefresh.ts'

test('本地没有这条笔记 → 补拉（外部新建的条目）', () => {
  assert.equal(needsFullNoteFetch(undefined, { updated_at: '2026-11-04 01:00:00.000000', content: '' }), true)
})

test('updated_at 变了 → 补拉（正文被外部改过）', () => {
  const old = { updated_at: '2026-11-04 01:00:00.000000', content: '旧正文' }
  const meta = { updated_at: '2026-11-04 02:00:00.000000', content: '' }
  assert.equal(needsFullNoteFetch(old, meta), true)
})

test('updated_at 没变 → 不补拉（本地正文即最新，省一次 get_note）', () => {
  const ts = '2026-11-04 01:00:00.000000'
  assert.equal(needsFullNoteFetch({ updated_at: ts, content: '正文' }, { updated_at: ts, content: '' }), false)
})

test('元信息正文恒为空串也必须按版本判断，不能按正文判断', () => {
  // list_meta 的 content 字面量返回 ''；若拿「正文是否为空」做判据会每次都误判为需要补拉
  // 或（更糟）永远把空正文写回界面。判据只能是 updated_at。
  const ts = '2026-11-04 01:00:00.000000'
  assert.equal(needsFullNoteFetch({ updated_at: ts, content: '' }, { updated_at: ts, content: '' }), false)
  assert.equal(
    needsFullNoteFetch({ updated_at: ts, content: '' }, { updated_at: '2026-11-04 01:00:01.000000', content: '' }),
    true,
  )
})
