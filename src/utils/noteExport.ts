import type { Note, NoteFolder } from '../api/tauri'

/**
 * 速记随手导出的纯函数工具（单条/整夹 × Markdown/HTML/PDF，2026-10-22 #t69）。
 * 与 Rust 侧口径对齐的少量前端逻辑：子树圈定、保存对话框默认文件名清洗。
 * 渲染（md→html）动态 import utils/markdownHtml，勿在此静态引入 marked/DOMPurify。
 */

export type ExportFormat = 'markdown' | 'html' | 'pdf'

/** 文件夹 id 的子树（含自身）；环路与超深靠 visited + 上限剪枝（与后端 subtree_folder_ids 同口径） */
export function subtreeFolderIds(
  folders: readonly Pick<NoteFolder, 'id' | 'parent_id'>[],
  rootId: number,
): number[] {
  const out = [rootId]
  let i = 0
  while (i < out.length) {
    const cur = out[i]
    for (const f of folders) {
      if (f.parent_id === cur && !out.includes(f.id)) out.push(f.id)
    }
    i++
    if (out.length > 10000) break
  }
  return out
}

/** 子树内的活笔记（store.state.notes 已只含未删除） */
export function notesOfSubtree(
  notes: readonly Note[],
  folders: readonly NoteFolder[],
  rootId: number,
): Note[] {
  const ids = subtreeFolderIds(folders, rootId)
  return notes.filter((n) => n.folder_id != null && ids.includes(n.folder_id))
}

/** 另存为对话框的默认文件名：标题清洗（Windows 非法字符/结尾点空格），空标题回退 */
export function sanitizeExportFilename(title: string, ext: string): string {
  let s = title
    .replace(/[\\/:*?"<>|]/g, ' ')
    .replace(/[\x00-\x1f]/g, ' ')
    .trim()
  while (s.endsWith('.') || s.endsWith(' ')) s = s.slice(0, -1)
  if (!s) s = '无标题笔记'
  const e = ext.replace(/^\./, '')
  return s.toLowerCase().endsWith(`.${e.toLowerCase()}`) ? s : `${s}.${e}`
}
