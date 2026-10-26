/**
 * 速记「表格笔记」的轻量引用层（只有常量与纯函数，无 exceljs/Univer 依赖）。
 * SpeednoteView / NoteFolderTree / NoteEditor 等表面静态引入本模块；
 * 重依赖（exceljs/Univer）全部留在 noteSheet.ts，由调用方动态 import ——
 * 表格编辑器数 MB 级，绝不能进速记主包。
 *
 * 表格笔记的数据形态：notes 表 kind='sheet'，content = 裸 xhub-sheet 协议 URL
 * （落盘文件在数据根 notes/files/<内容哈希>.<ext>），展示名就是笔记标题。
 */

export const SHEET_URL_PREFIX = 'http://xhub-sheet.localhost/'

/** 表格笔记的落盘文件名口径：16 位小写十六进制哈希 + 白名单扩展（与 Rust 端一致） */
export function sheetStoredNameFromUrl(url: string): string | null {
  if (!url.startsWith(SHEET_URL_PREFIX)) return null
  const tail = url.slice(SHEET_URL_PREFIX.length)
  return /^([0-9a-f]{16})\.(xlsx|csv)$/.test(tail) ? tail : null
}

/** 从表格笔记正文（裸 URL）提取落盘文件名；非表格正文返回 null */
export function sheetStoredNameFromContent(content: string): string | null {
  return sheetStoredNameFromUrl(content.trim())
}

/** 表格笔记判定：kind='sheet' 且正文能解出合法落盘名 */
export function isSheetNote(note: { kind?: string; content: string }): boolean {
  return note.kind === 'sheet' && sheetStoredNameFromContent(note.content) != null
}

/**
 * CSV 行解析（RFC 4180 口径的轻量实现）：双引号包裹 / 引号转义（"" → "）/
 * 逗号与 CRLF 分隔；尾行无换行符也成行。只处理文本层，BOM 由调用方剥。
 */
export function parseCsvRows(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let inQuotes = false
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]
    if (inQuotes) {
      if (ch === '"') {
        if (text[i + 1] === '"') { field += '"'; i++ } else inQuotes = false
      } else field += ch
    } else if (ch === '"') {
      inQuotes = true
    } else if (ch === ',') {
      row.push(field); field = ''
    } else if (ch === '\n' || ch === '\r') {
      if (ch === '\r' && text[i + 1] === '\n') i++
      row.push(field); field = ''
      rows.push(row); row = []
    } else {
      field += ch
    }
  }
  if (field !== '' || row.length > 0) { row.push(field); rows.push(row) }
  return rows
}
