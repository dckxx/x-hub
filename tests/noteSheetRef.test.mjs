/**
 * 速记表格笔记引用层（src/utils/noteSheetRef.ts）单测。
 * 直接以 node:test 加载 TS 源码（模块零依赖，Node 24 原生类型剥离）：
 *   node --test tests/noteSheetRef.test.mjs
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  SHEET_URL_PREFIX,
  isSheetNote,
  parseCsvRows,
  sheetStoredNameFromContent,
  sheetStoredNameFromUrl,
} from '../src/utils/noteSheetRef.ts'

const VALID_XLSX = `${SHEET_URL_PREFIX}c8e87d3367a7cedb.xlsx`
const VALID_CSV = `${SHEET_URL_PREFIX}c8e87d3367a7cedb.csv`

test('sheetStoredNameFromUrl：合法 xlsx / csv 落盘名原样返回', () => {
  assert.equal(sheetStoredNameFromUrl(VALID_XLSX), 'c8e87d3367a7cedb.xlsx')
  assert.equal(sheetStoredNameFromUrl(VALID_CSV), 'c8e87d3367a7cedb.csv')
})

test('sheetStoredNameFromUrl：非本协议 / 非法文件名一律 null（防借 URL 读数据根外文件）', () => {
  // 非本协议
  assert.equal(sheetStoredNameFromUrl('http://xhub-note.localhost/c8e87d3367a7cedb.png'), null)
  assert.equal(sheetStoredNameFromUrl('https://example.com/c8e87d3367a7cedb.xlsx'), null)
  // 哈希长度/字符不对
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}c8e87d3367a7ced.xlsx`), null)
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}c8e87d3367a7cedX.xlsx`), null)
  // 大写十六进制不收（落盘名恒小写）
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}C8E87D3367A7CEDB.xlsx`), null)
  // 扩展白名单外
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}c8e87d3367a7cedb.xls`), null)
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}c8e87d3367a7cedb.png`), null)
  // 路径穿越 / 多段路径 / query
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}../secret.xlsx`), null)
  assert.equal(sheetStoredNameFromUrl(`${SHEET_URL_PREFIX}a/b/c8e87d3367a7cedb.xlsx`), null)
  assert.equal(sheetStoredNameFromUrl(`${VALID_XLSX}?v=1`), null)
  // 空串 / 无扩展
  assert.equal(sheetStoredNameFromUrl(SHEET_URL_PREFIX), null)
  assert.equal(sheetStoredNameFromUrl(''), null)
})

test('sheetStoredNameFromContent：表格笔记正文（裸 URL，容忍首尾空白）', () => {
  assert.equal(sheetStoredNameFromContent(VALID_XLSX), 'c8e87d3367a7cedb.xlsx')
  assert.equal(sheetStoredNameFromContent(`  ${VALID_CSV}\n`), 'c8e87d3367a7cedb.csv')
  // 普通文本笔记正文不是表格引用
  assert.equal(sheetStoredNameFromContent('正文里提到 http://xhub-sheet.localhost/ 不算'), null)
  assert.equal(sheetStoredNameFromContent(''), null)
})

test('isSheetNote：kind=sheet 且正文可解析才成立', () => {
  assert.equal(isSheetNote({ kind: 'sheet', content: VALID_XLSX }), true)
  // kind 对但正文坏了（文件被清不可判定）不算
  assert.equal(isSheetNote({ kind: 'sheet', content: '不是URL' }), false)
  // 正文对但 kind 没标不算（文本笔记里贴的链接）
  assert.equal(isSheetNote({ kind: 'text', content: VALID_XLSX }), false)
  assert.equal(isSheetNote({ content: VALID_XLSX }), false)
})

test('parseCsvRows：引号包裹 / 转义引号 / 逗号与换行分隔', () => {
  const rows = parseCsvRows('a,"b,c","d""e"\nz, wt\r\n"多\n行"')
  assert.deepEqual(rows, [
    ['a', 'b,c', 'd"e'],
    ['z', ' wt'],
    ['多\n行'],
  ])
})

test('parseCsvRows：尾行无换行符 / 空字段 / 空串', () => {
  assert.deepEqual(parseCsvRows('a,b'), [['a', 'b']])
  assert.deepEqual(parseCsvRows('a,,b\n'), [['a', '', 'b']])
  assert.deepEqual(parseCsvRows(''), [])
  // 只有换行 = 一行空字段
  assert.deepEqual(parseCsvRows('\n'), [['']])
})
