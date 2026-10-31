/**
 * 发布弹窗「上一版截图」引用口径（src/utils/shotRefs.ts）单测。
 * 直接以 node:test + 原生类型剥离加载 TS 源码：
 *   node --test tests/shotRefs.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { allShotsReferenced, referencedShotCount, shotBasename } from '../src/utils/shotRefs.ts'

/** 市场清单里的截图 URL（服务端按内容哈希命名） */
const URL_A = 'https://cdn.example.com/screenshots/com.demo.ext/abc123def4567890.png'
const URL_B = 'https://cdn.example.com/screenshots/com.demo.ext/0123456789abcdef.webp'

test('shotBasename：URL 取最后一段文件名，去 query/fragment、统一小写', () => {
  assert.equal(shotBasename(URL_A), 'abc123def4567890.png')
  assert.equal(shotBasename('https://host/a/B.PNG?v=2#x'), 'b.png')
  assert.equal(shotBasename('C:\\Users\\me\\shots\\A.jpg'), 'a.jpg')
  assert.equal(shotBasename(''), '')
})

test('referencedShotCount：只数来自上一版的那几张（本地临时文件与远端 URL 比基名）', () => {
  const picked = [
    'C:\\Users\\me\\AppData\\Local\\Temp\\x-hub-pub-shots\\abc123def4567890.png',
    'C:\\Users\\me\\Pictures\\my-own-shot.png',
  ]
  assert.equal(referencedShotCount([URL_A, URL_B], picked), 1)
})

test('referencedShotCount：没有上一版 / 没选图都是 0', () => {
  assert.equal(referencedShotCount([], ['a.png']), 0)
  assert.equal(referencedShotCount([URL_A], []), 0)
})

test('allShotsReferenced：全部带齐才 true（URL 带查询串也算同一张）', () => {
  const all = ['/tmp/x/abc123def4567890.png', '/tmp/x/0123456789abcdef.webp']
  assert.equal(allShotsReferenced([`${URL_A}?v=2`, URL_B], all), true)
  assert.equal(allShotsReferenced([URL_A, URL_B], [all[0]]), false)
  assert.equal(allShotsReferenced([], []), false)
})
