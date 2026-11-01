import type {
  ICellData,
  IObjectArrayPrimitiveType,
  IObjectMatrixPrimitiveType,
  IStyleData,
  IWorkbookData,
  IWorksheetData,
  Nullable,
} from '@univerjs/core'
import {
  BorderStyleTypes,
  CellValueType,
  HorizontalAlign,
  LocaleType,
  VerticalAlign,
  WorksheetHiddenState,
  WrapStrategy,
} from '@univerjs/core'
import '@univerjs/preset-sheets-core/lib/index.css'
import ExcelJS from 'exceljs'
import { isTauri, tauriApi } from '../api/tauri'
import { parseCsvRows, sheetStoredNameFromUrl } from './noteSheetRef'

/**
 * 速记「表格笔记」（x-hub #t70）：
 * - 表格笔记 = notes 表 kind='sheet' 的一行，content 存 xhub-sheet 协议 URL（裸 URL），
 *   文件落盘 notes/files/<内容哈希>.<ext>，与笔记图片通道（notes/images + xhub-note）同构；
 *   保存写回同名文件（协议 no-store，重开即新内容）。
 * - 编辑：Univer（@univerjs/preset-sheets-core，纯前端离线）；xlsx 解析/生成用 exceljs
 *   （Univer 官方 xlsx 导入导出是 Pro 插件且需要转换服务端，与本地优先定位冲突，故自建互转层）。
 * - 互转范围（第一pass）：单元格值 / 公式 / 数字格式 / 字体（加粗斜体字号字色）/ 填充 / 边框 /
 *   对齐换行 / 合并单元格 / 行高列宽 / 冻结窗格 / sheet 名与颜色 / 隐藏 sheet。
 *   图表、图片、透视表、条件格式、数据验证等高级元素第一版不保留（导入时在卡片提示）。
 */

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

async function fileToBase64(file: Blob): Promise<string> {
  return bytesToBase64(new Uint8Array(await file.arrayBuffer()))
}

/**
 * 新建空白表格 / 导入本地 xlsx/csv：落盘后返回 { 展示名, 协议 URL }。
 * 浏览器预览环境无后端（无落盘、无协议），直接报错走 toast。
 */
export async function saveNoteSheetFile(content: Blob | ArrayBuffer, name: string): Promise<{ name: string; url: string }> {
  if (!isTauri()) throw new Error('表格功能需在桌面应用中使用')
  const blob = content instanceof Blob ? content : new Blob([content])
  const b64 = await fileToBase64(blob)
  return await tauriApi.importNoteSheet(b64, name)
}

// ---------------------------------------------------------------------------
// exceljs ↔ Univer 互转（exceljs 走 package.json browser 字段构建）
// ---------------------------------------------------------------------------

type ExcelCell = ExcelJS.Cell

/** exceljs ARGB（如 'FF336699'/'8FFF336699'）→ '#RRGGBB'；主题色/无色返回 null */
function argbToRgb(color: unknown): string | null {
  if (!color || typeof color !== 'object') return null
  const c = color as { argb?: string; theme?: number }
  if (typeof c.argb === 'string' && /^[0-9A-Fa-f]{6,8}$/.test(c.argb)) {
    return `#${c.argb.slice(-6).toUpperCase()}`
  }
  return null // theme 色第一版不映射（需要解析工作簿主题 XML）
}

/** exceljs 边框样式名 → Univer BorderStyleTypes */
function mapBorderStyle(style: string | undefined): BorderStyleTypes | null {
  switch (style) {
    case 'thin': return BorderStyleTypes.THIN
    case 'hair': return BorderStyleTypes.HAIR
    case 'dotted': return BorderStyleTypes.DOTTED
    case 'dashed': return BorderStyleTypes.DASHED
    case 'dashDot': return BorderStyleTypes.DASH_DOT
    case 'dashDotDot': return BorderStyleTypes.DASH_DOT_DOT
    case 'double': return BorderStyleTypes.DOUBLE
    case 'medium': return BorderStyleTypes.MEDIUM
    case 'mediumDashed': return BorderStyleTypes.MEDIUM_DASHED
    case 'mediumDashDot': return BorderStyleTypes.MEDIUM_DASH_DOT
    case 'mediumDashDotDot': return BorderStyleTypes.MEDIUM_DASH_DOT_DOT
    case 'slantDashDot': return BorderStyleTypes.SLANT_DASH_DOT
    case 'thick': return BorderStyleTypes.THICK
    default: return null
  }
}

/** 'A1:B2' → 0-based inclusive {startRow,startColumn,endRow,endColumn} */
function parseRangeA1(range: string): { startRow: number; startColumn: number; endRow: number; endColumn: number } | null {
  const m = /^\$?([A-Z]{1,3})\$?(\d+):\$?([A-Z]{1,3})\$?(\d+)$/i.exec(range.trim())
  if (!m) return null
  const colToIdx = (s: string): number => {
    let n = 0
    for (const ch of s.toUpperCase()) n = n * 26 + (ch.charCodeAt(0) - 64)
    return n - 1
  }
  return {
    startRow: Number(m[2]) - 1,
    startColumn: colToIdx(m[1]),
    endRow: Number(m[4]) - 1,
    endColumn: colToIdx(m[3]),
  }
}

/** JS Date → Excel 1900 日期序列号 */
function dateToSerial(d: Date): number {
  return d.getTime() / 86400000 + 25569
}

/** exceljs 单元格值 → Univer {v, f}；richText 拼接、超链接取文本、共享公式解析主公式 */
function excelValueToUniver(
  cell: ExcelCell,
  sharedMaster: (addr: string) => string | null,
): { v?: ICellData['v']; f?: string; isDate?: boolean } {
  const value = cell.value
  if (value == null) return {}
  if (typeof value === 'number' || typeof value === 'string' || typeof value === 'boolean') {
    return { v: value }
  }
  if (value instanceof Date) return { v: dateToSerial(value), isDate: true }
  if (typeof value === 'object' && 'richText' in value) {
    const text = value.richText.map((rt) => rt.text).join('')
    return { v: text }
  }
  if (typeof value === 'object' && ('formula' in value || 'sharedFormula' in value)) {
    const fv = value as { formula?: string; sharedFormula?: string; result?: unknown }
    let formula: string | null = null
    if (typeof fv.formula === 'string') formula = fv.formula
    else if (typeof fv.sharedFormula === 'string') formula = sharedMaster(fv.sharedFormula)
    const v = excelResultToV(fv.result)
    return { v, f: formula ? `=${formula}` : undefined }
  }
  if (typeof value === 'object' && 'error' in value) {
    const err = value as { error?: string }
    return { v: typeof err.error === 'string' ? err.error : '#VALUE!' }
  }
  if (typeof value === 'object' && 'hyperlink' in value) {
    const hl = value as { text?: string; hyperlink?: string }
    return { v: typeof hl.text === 'string' ? hl.text : '' }
  }
  if (typeof value === 'object' && 'text' in value) {
    // 旧版公式形式 { text, result } / 纯文本对象
    const tv = value as { text?: unknown; result?: unknown }
    if ('result' in value) {
      const v = excelResultToV(tv.result)
      const formula = typeof tv.text === 'string' ? tv.text : null
      return { v, f: formula ? `=${formula}` : undefined }
    }
    return { v: String(tv.text ?? '') }
  }
  return { v: String(value) }
}

function excelResultToV(result: unknown): ICellData['v'] {
  if (result == null) return null
  if (result instanceof Date) return dateToSerial(result)
  if (typeof result === 'number' || typeof result === 'string' || typeof result === 'boolean') return result
  if (typeof result === 'object' && 'error' in (result as Record<string, unknown>)) {
    const e = (result as { error?: unknown }).error
    return typeof e === 'string' ? e : '#VALUE!'
  }
  if (typeof result === 'object' && 'richText' in (result as Record<string, unknown>)) {
    return (result as { richText: { text: string }[] }).richText.map((rt) => rt.text).join('')
  }
  if (typeof result === 'object' && 'text' in (result as Record<string, unknown>)) {
    return String((result as { text?: unknown }).text ?? '')
  }
  return null
}

/** exceljs cell.style → Univer IStyleData（第一版映射子集） */
function excelStyleToUniver(cell: ExcelCell): IStyleData | null {
  const st = cell.style as ExcelJS.Style & {
    numFmt?: string
    font?: ExcelJS.Font
    fill?: ExcelJS.Fill
    border?: Partial<Record<'top' | 'bottom' | 'left' | 'right', { style?: string; color?: unknown } | undefined>>
    alignment?: Partial<ExcelJS.Alignment>
  }
  const out: IStyleData = {}
  let has = false

  if (st.numFmt && st.numFmt !== 'General') {
    out.n = { pattern: st.numFmt }
    has = true
  }
  const font = st.font
  if (font) {
    if (font.name) { out.ff = font.name; has = true }
    if (typeof font.size === 'number') { out.fs = font.size; has = true }
    if (font.bold) { out.bl = 1; has = true }
    if (font.italic) { out.it = 1; has = true }
    if (font.underline) { out.ul = { s: 1 }; has = true }
    if (font.strike) { out.st = { s: 1 }; has = true }
    const fc = argbToRgb(font.color)
    if (fc) { out.cl = { rgb: fc }; has = true }
  }
  const fill = st.fill
  if (fill && typeof fill === 'object' && 'pattern' in fill && (fill as ExcelJS.FillPattern).pattern === 'solid') {
    const fg = argbToRgb((fill as ExcelJS.FillPattern).fgColor)
    if (fg) { out.bg = { rgb: fg }; has = true }
  }
  const border = st.border
  if (border) {
    const sides = { t: border.top, r: border.right, b: border.bottom, l: border.left } as const
    const bd: IStyleData['bd'] = {}
    let anyBorder = false
    for (const [key, side] of Object.entries(sides) as [key: 't' | 'r' | 'b' | 'l', value: { style?: string; color?: unknown } | undefined][]) {
      if (!side?.style) continue
      const s = mapBorderStyle(side.style)
      if (s == null) continue
      bd[key] = { s, cl: { rgb: argbToRgb(side.color) ?? '#000000' } }
      anyBorder = true
    }
    if (anyBorder) { out.bd = bd; has = true }
  }
  const align = st.alignment
  if (align) {
    const htMap: Record<string, HorizontalAlign> = {
      left: HorizontalAlign.LEFT,
      center: HorizontalAlign.CENTER,
      right: HorizontalAlign.RIGHT,
      justify: HorizontalAlign.JUSTIFIED,
      distributed: HorizontalAlign.DISTRIBUTED,
    }
    const vtMap: Record<string, VerticalAlign> = {
      top: VerticalAlign.TOP,
      middle: VerticalAlign.MIDDLE,
      bottom: VerticalAlign.BOTTOM,
    }
    if (align.horizontal && htMap[align.horizontal] != null) { out.ht = htMap[align.horizontal]; has = true }
    if (align.vertical && vtMap[align.vertical] != null) { out.vt = vtMap[align.vertical]; has = true }
    if (align.wrapText) { out.tb = WrapStrategy.WRAP; has = true }
  }
  return has ? out : null
}

/**
 * xlsx 字节 → Univer IWorkbookData。
 * csv 交给 csvToUniver；xlsx 走 exceljs 全量解析。
 */
export async function sheetBytesToUniver(bytes: ArrayBuffer, name: string, ext: string): Promise<IWorkbookData> {
  if (ext === 'csv') return csvToUniver(bytes, name)
  const wb = new ExcelJS.Workbook()
  await wb.xlsx.load(bytes)
  return excelWorkbookToUniver(wb, name)
}

function excelWorkbookToUniver(wb: ExcelJS.Workbook, name: string): IWorkbookData {
  const sheets: Record<string, Partial<IWorksheetData>> = {}
  const sheetOrder: string[] = []
  const styles: Record<string, Nullable<IStyleData>> = {}

  wb.eachSheet((ws) => {
    const id = `sheet-${sheetOrder.length + 1}`
    sheetOrder.push(id)

    // 共享公式：先收集 master 地址 → 公式串（exceljs 的 slave 只有 sharedFormula 指回 master）
    const masterFormulas = new Map<string, string>()
    ws.eachRow((row) => {
      row.eachCell({ includeEmpty: false }, (cell) => {
        const v = cell.value
        if (v && typeof v === 'object' && 'formula' in v) {
          masterFormulas.set(cell.address, (v as { formula?: string }).formula ?? '')
        }
      })
    })

    const cellData: IObjectMatrixPrimitiveType<ICellData> = {}
    const rowData: IObjectArrayPrimitiveType<{ h?: number }> = {}
    let maxRow = 0
    let maxCol = 0

    ws.eachRow({ includeEmpty: false }, (row, rowNumber) => {
      const r = rowNumber - 1
      maxRow = Math.max(maxRow, rowNumber)
      if (row.height) rowData[r] = { h: Math.round(row.height * 4) / 3 } // pt → px
      row.eachCell({ includeEmpty: false }, (cell, colNumber) => {
        const c = colNumber - 1
        maxCol = Math.max(maxCol, colNumber)
        const mapped = excelValueToUniver(cell, (addr) => masterFormulas.get(addr) ?? null)
        if (mapped.v == null && !mapped.f) return
        const data: ICellData = {}
        if (mapped.f) data.f = mapped.f
        if (mapped.v != null) data.v = mapped.v
        const style = excelStyleToUniver(cell)
        if (style) {
          if (mapped.isDate && !style.n) style.n = { pattern: 'yyyy-mm-dd' }
          data.s = style
        }
        if (mapped.f && typeof data.v === 'string' && data.v.startsWith('#')) {
          data.t = CellValueType.FORCE_STRING
        }
        cellData[r] = cellData[r] ?? {}
        cellData[r][c] = data
      })
    })

    const columnData: IObjectArrayPrimitiveType<{ w?: number }> = {}
    ws.columns?.forEach((col, idx) => {
      if (col?.width) columnData[idx] = { w: Math.round(col.width * 7) + 5 } // 字符宽 → px（近似）
    })

    const mergeData = (ws.model?.merges ?? [])
      .map((range) => parseRangeA1(String(range)))
      .filter((x): x is NonNullable<typeof x> => x != null)

    const view = (ws.views?.[0] ?? {}) as { xSplit?: number; ySplit?: number; state?: string }
    const xSplit = view.state === 'frozen' ? (view.xSplit ?? 0) : 0
    const ySplit = view.state === 'frozen' ? (view.ySplit ?? 0) : 0
    const tabColor = argbToRgb(ws.properties?.tabColor)

    sheets[id] = {
      id,
      name: ws.name,
      tabColor: tabColor ?? '',
      hidden: ws.state === 'veryHidden' ? WorksheetHiddenState.VERY_HIDDEN : ws.state === 'hidden' ? WorksheetHiddenState.HIDDEN : WorksheetHiddenState.VISIBLE,
      rowCount: Math.min(Math.max(maxRow + 50, 200), 20000),
      columnCount: Math.min(Math.max(maxCol + 10, 26), 2000),
      defaultRowHeight: 24,
      defaultColumnWidth: 96,
      mergeData,
      cellData,
      rowData,
      columnData,
      freeze: { xSplit, ySplit, startRow: ySplit > 0 ? ySplit - 1 : -1, startColumn: xSplit > 0 ? xSplit - 1 : -1 },
      showGridlines: 1,
    }
  })

  return {
    id: `workbook-${Date.now()}`,
    name,
    appVersion: '',
    locale: LocaleType.ZH_CN,
    styles,
    sheetOrder,
    sheets,
  }
}

/** csv → 单 sheet 的 Univer 数据（自带轻量解析：引号转义 / 逗号分隔；UTF-8 BOM 兼容） */
export function csvToUniver(bytes: ArrayBuffer, name: string): IWorkbookData {
  let text = new TextDecoder('utf-8').decode(bytes)
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1)
  const rows = parseCsvRows(text)
  const cellData: IObjectMatrixPrimitiveType<ICellData> = {}
  rows.forEach((cols, r) => {
    cols.forEach((raw, c) => {
      if (raw === '') return
      const num = Number(raw.replace(/,/g, ''))
      const cell: ICellData = Number.isFinite(num) && raw.trim() !== '' && /^-?[\d,]+(\.\d+)?$/.test(raw.trim())
        ? { v: num }
        : { v: raw }
      cellData[r] = cellData[r] ?? {}
      cellData[r][c] = cell
    })
  })
  return {
    id: `workbook-${Date.now()}`,
    name,
    appVersion: '',
    locale: LocaleType.ZH_CN,
    styles: {},
    sheetOrder: ['sheet-1'],
    sheets: {
      'sheet-1': {
        id: 'sheet-1',
        name: 'Sheet1',
        tabColor: '',
        hidden: WorksheetHiddenState.VISIBLE,
        rowCount: Math.max(rows.length + 50, 200),
        columnCount: Math.max(...rows.map((r) => r.length), 26) + 10,
        defaultRowHeight: 24,
        defaultColumnWidth: 96,
        mergeData: [],
        cellData,
        rowData: {},
        columnData: {},
        freeze: { xSplit: 0, ySplit: 0, startRow: -1, startColumn: -1 },
        showGridlines: 1,
      },
    },
  }
}

// ---------------------------------------------------------------------------
// Univer → xlsx/csv（保存写回 / 另存副本）
// ---------------------------------------------------------------------------

/** Univer 样式 → exceljs style 对象（与导入映射互逆；未映射属性自然丢失） */
function univerStyleToExcel(s: IStyleData): Partial<ExcelJS.Style> {
  const out: Partial<ExcelJS.Style> = {}
  const font: Partial<ExcelJS.Font> = {}
  if (s.ff) font.name = s.ff
  if (typeof s.fs === 'number') font.size = s.fs
  if (s.bl === 1) font.bold = true
  if (s.it === 1) font.italic = true
  if (s.ul?.s === 1) font.underline = true
  if (s.st?.s === 1) font.strike = true
  if (s.cl?.rgb) font.color = { argb: `FF${s.cl.rgb.replace('#', '').toUpperCase()}` }
  if (Object.keys(font).length > 0) out.font = font as ExcelJS.Font

  if (s.bg?.rgb) {
    out.fill = { type: 'pattern', pattern: 'solid', fgColor: { argb: `FF${s.bg.rgb.replace('#', '').toUpperCase()}` } }
  }
  if (s.bd) {
    const border: Partial<ExcelJS.Borders> = {}
    const sides: ['top' | 'right' | 'bottom' | 'left', 't' | 'r' | 'b' | 'l'][] = [
      ['top', 't'], ['right', 'r'], ['bottom', 'b'], ['left', 'l'],
    ]
    for (const [jsKey, uKey] of sides) {
      const side = s.bd[uKey]
      if (!side) continue
      const styleName = borderNumToExcelName(side.s)
      if (!styleName) continue
      border[jsKey] = { style: styleName as ExcelJS.BorderStyle, color: { argb: `FF${(side.cl?.rgb ?? '#000000').replace('#', '').toUpperCase()}` } }
    }
    if (Object.keys(border).length > 0) out.border = border as ExcelJS.Borders
  }
  const alignment: Partial<ExcelJS.Alignment> = {}
  const htName = horizontalAlignName(s.ht)
  if (htName) alignment.horizontal = htName as ExcelJS.Alignment['horizontal']
  const vtName = verticalAlignName(s.vt)
  if (vtName) alignment.vertical = vtName as ExcelJS.Alignment['vertical']
  if (s.tb === WrapStrategy.WRAP) alignment.wrapText = true
  if (Object.keys(alignment).length > 0) out.alignment = alignment as ExcelJS.Alignment
  if (s.n?.pattern) out.numFmt = s.n.pattern
  return out
}

function horizontalAlignName(v: Nullable<HorizontalAlign>): string | null {
  switch (v) {
    case HorizontalAlign.LEFT: return 'left'
    case HorizontalAlign.CENTER: return 'center'
    case HorizontalAlign.RIGHT: return 'right'
    case HorizontalAlign.JUSTIFIED: return 'justify'
    case HorizontalAlign.DISTRIBUTED: return 'distributed'
    default: return null
  }
}

function verticalAlignName(v: Nullable<VerticalAlign>): string | null {
  switch (v) {
    case VerticalAlign.TOP: return 'top'
    case VerticalAlign.MIDDLE: return 'middle'
    case VerticalAlign.BOTTOM: return 'bottom'
    default: return null
  }
}

function borderNumToExcelName(v: number): string | null {
  switch (v as BorderStyleTypes) {
    case BorderStyleTypes.THIN: return 'thin'
    case BorderStyleTypes.HAIR: return 'hair'
    case BorderStyleTypes.DOTTED: return 'dotted'
    case BorderStyleTypes.DASHED: return 'dashed'
    case BorderStyleTypes.DASH_DOT: return 'dashDot'
    case BorderStyleTypes.DASH_DOT_DOT: return 'dashDotDot'
    case BorderStyleTypes.DOUBLE: return 'double'
    case BorderStyleTypes.MEDIUM: return 'medium'
    case BorderStyleTypes.MEDIUM_DASHED: return 'mediumDashed'
    case BorderStyleTypes.MEDIUM_DASH_DOT: return 'mediumDashDot'
    case BorderStyleTypes.MEDIUM_DASH_DOT_DOT: return 'mediumDashDotDot'
    case BorderStyleTypes.SLANT_DASH_DOT: return 'slantDashDot'
    case BorderStyleTypes.THICK: return 'thick'
    default: return null
  }
}

/** Univer IWorkbookData → xlsx/csv 字节 */
export async function univerToSheetBytes(snapshot: IWorkbookData, ext: 'xlsx' | 'csv'): Promise<ArrayBuffer> {
  const wb = new ExcelJS.Workbook()
  const sheetIds = snapshot.sheetOrder.length > 0 ? snapshot.sheetOrder : Object.keys(snapshot.sheets)
  const styleMap = snapshot.styles ?? {}

  for (const sid of sheetIds) {
    const ws = snapshot.sheets[sid]
    if (!ws) continue
    const sheet = wb.addWorksheet(ws.name || `Sheet${wb.worksheets.length + 1}`)

    if (ws.tabColor) sheet.properties.tabColor = { argb: `FF${ws.tabColor.replace('#', '').toUpperCase()}` }
    const hiddenNum = Number(ws.hidden ?? 0)
    if (hiddenNum === WorksheetHiddenState.VERY_HIDDEN) sheet.state = 'veryHidden'
    else if (hiddenNum === WorksheetHiddenState.HIDDEN) sheet.state = 'hidden'

    const cellData = (ws.cellData ?? {}) as Record<string, Record<string, ICellData | undefined> | undefined>
    for (const rKey of Object.keys(cellData)) {
      const r = Number(rKey)
      const row = sheet.getRow(r + 1)
      const cols = cellData[rKey]
      if (!cols) continue
      for (const cKey of Object.keys(cols)) {
        const cell = cols[cKey]
        if (!cell || (cell.v == null && !cell.f)) continue
        const target = row.getCell(Number(cKey) + 1)
        if (cell.f) {
          const formula = cell.f.startsWith('=') ? cell.f.slice(1) : cell.f
          target.value = { formula, result: (cell.v ?? null) as ExcelJS.CellFormulaValue['result'] }
        } else {
          target.value = cell.v as ExcelJS.CellValue
        }
        const style = typeof cell.s === 'object' ? cell.s : styleMap[cell.s ?? '']
        if (style) {
          const mapped = univerStyleToExcel(style)
          if (Object.keys(mapped).length > 0) target.style = { ...target.style, ...mapped }
        }
      }
      row.commit()
    }

    const rowData = (ws.rowData ?? {}) as Record<string, { h?: number } | undefined>
    for (const rKey of Object.keys(rowData)) {
      const h = rowData[rKey]?.h
      if (h) sheet.getRow(Number(rKey) + 1).height = Math.round(h * 3) / 4 // px → pt
    }
    const columnData = (ws.columnData ?? {}) as Record<string, { w?: number } | undefined>
    for (const cKey of Object.keys(columnData)) {
      const w = columnData[cKey]?.w
      if (w) sheet.getColumn(Number(cKey) + 1).width = Math.max(0, (w - 5) / 7) // px → 字符宽（近似）
    }

    for (const m of ws.mergeData ?? []) {
      const a1 = colName(m.startColumn) + (m.startRow + 1)
      const b1 = colName(m.endColumn) + (m.endRow + 1)
      try {
        sheet.mergeCells(a1 === b1 ? a1 : `${a1}:${b1}`)
      } catch {
        // 非法合并范围跳过（理论上 Univer 快照不会给出）
      }
    }

    const freeze = ws.freeze
    if (freeze && (freeze.xSplit > 0 || freeze.ySplit > 0)) {
      sheet.views = [{ state: 'frozen', xSplit: freeze.xSplit, ySplit: freeze.ySplit }]
    }
  }

  if (ext === 'csv') {
    const buf = await wb.csv.writeBuffer()
    return toArrayBuffer(buf)
  }
  const buf = await wb.xlsx.writeBuffer()
  return toArrayBuffer(buf)
}

function colName(idx: number): string {
  let n = idx + 1
  let s = ''
  while (n > 0) {
    const rem = (n - 1) % 26
    s = String.fromCharCode(65 + rem) + s
    n = Math.floor((n - 1) / 26)
  }
  return s
}

function toArrayBuffer(buf: ExcelJS.Buffer): ArrayBuffer {
  const u8 = new Uint8Array(buf as unknown as ArrayBuffer)
  const out = new ArrayBuffer(u8.byteLength)
  new Uint8Array(out).set(u8)
  return out
}

/** 空白工作簿字节（新建空白表格用） */
export async function blankSheetBytes(): Promise<ArrayBuffer> {
  const wb = new ExcelJS.Workbook()
  const ws = wb.addWorksheet('Sheet1')
  ws.getCell('A1').value = ''
  const buf = await wb.xlsx.writeBuffer()
  return toArrayBuffer(buf)
}

/** 从协议 URL 读表格字节（xhub-sheet 协议 no-store，保存后重开即新内容） */
export async function fetchSheetBytes(url: string): Promise<{ bytes: ArrayBuffer; ext: string }> {
  const stored = sheetStoredNameFromUrl(url)
  const ext = stored ? (stored.split('.').pop() ?? 'xlsx') : 'xlsx'
  const resp = await fetch(url)
  if (!resp.ok) throw new Error('表格文档读取失败')
  return { bytes: await resp.arrayBuffer(), ext }
}

/**
 * 随手导出表格笔记为 Excel（.xlsx）的字节载荷（SpeednoteView → export_note_sheet）：
 * - 落盘文件本身是 xlsx → 返回 null，调用方传 null 让 Rust 直拷原文件
 *   （零转换保真最高——互转层第一版不覆盖的图表/图片等高级元素在原文件里仍健在，
 *   也免大文件 base64 过 IPC）；
 * - 落盘文件是 csv → 前端转成 xlsx 字节并 base64（值 + 数字识别，与打开表格同一解析口径，
 *   复用 csvToUniver / univerToSheetBytes，csv 本就无样式可保）。
 */
export async function sheetExcelExportB64(url: string, title: string): Promise<string | null> {
  const { bytes, ext } = await fetchSheetBytes(url)
  if (ext !== 'csv') return null
  const snapshot = csvToUniver(bytes, title)
  const xlsx = await univerToSheetBytes(snapshot, 'xlsx')
  return bytesToBase64(new Uint8Array(xlsx))
}
