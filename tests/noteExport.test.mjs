/**
 * 速记随手导出的前端纯函数测试（src/utils/noteExport.ts）。
 * 直接 node:test + 原生类型剥离跑 TS 源码（模块同 modelMarkdownNoise 惯例，Node 24+）：
 *   node --test tests/noteExport.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import { subtreeFolderIds, notesOfSubtree, sanitizeExportFilename } from '../src/utils/noteExport.ts'

test('subtreeFolderIds 圈定自身 + 全部后代（跨层）', () => {
  const folders = [
    { id: 1, parent_id: null },
    { id: 2, parent_id: 1 },
    { id: 3, parent_id: 2 },
    { id: 4, parent_id: null },
  ]
  assert.deepEqual(subtreeFolderIds(folders, 1).sort(), [1, 2, 3])
  assert.deepEqual(subtreeFolderIds(folders, 3), [3])
})

test('subtreeFolderIds 对 parent 环路不发散（visited 剪枝）', () => {
  const folders = [
    { id: 1, parent_id: 2 },
    { id: 2, parent_id: 1 },
  ]
  const r = subtreeFolderIds(folders, 1)
  assert.equal(r.length, 2) // 1、2 各进一次，环路不再追加
})

test('notesOfSubtree 只留子树内、且带 folder_id 的笔记', () => {
  const folders = [
    { id: 1, parent_id: null },
    { id: 2, parent_id: 1 },
  ]
  const notes = [
    { id: 10, folder_id: 1 },
    { id: 11, folder_id: 2 },
    { id: 12, folder_id: 9 },
    { id: 13, folder_id: null },
  ]
  const ids = notesOfSubtree(notes, folders, 1).map((n) => n.id).sort()
  assert.deepEqual(ids, [10, 11])
})

test('sanitizeExportFilename 清洗 Windows 非法字符与结尾点空格', () => {
  assert.equal(sanitizeExportFilename('a<b>:c/d\\e', 'md'), 'a b  c d e.md')
  assert.equal(sanitizeExportFilename('  结尾点... ', 'md'), '结尾点.md')
  assert.equal(sanitizeExportFilename('...', 'html'), '无标题笔记.html')
  assert.equal(sanitizeExportFilename('', 'pdf'), '无标题笔记.pdf')
})

test('sanitizeExportFilename 已带正确扩展名时不重复追加（大小写不敏感）', () => {
  assert.equal(sanitizeExportFilename('笔记.md', 'md'), '笔记.md')
  assert.equal(sanitizeExportFilename('笔记.MD', 'md'), '笔记.MD')
  assert.equal(sanitizeExportFilename('笔记.md', 'html'), '笔记.md.html')
})
