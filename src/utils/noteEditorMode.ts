/** 速记编辑器模式。wysiwyg = 实时预览（Crepe），split = 分屏，source = 源码。 */
export type NoteEditorMode = 'wysiwyg' | 'split' | 'source'

const MODES: readonly NoteEditorMode[] = ['wysiwyg', 'split', 'source']

export function normalizeNoteEditorMode(value: unknown): NoteEditorMode {
  return typeof value === 'string' && (MODES as readonly string[]).includes(value)
    ? (value as NoteEditorMode)
    : 'wysiwyg'
}

export const NOTE_EDITOR_MODES: readonly { id: NoteEditorMode; label: string }[] = [
  { id: 'wysiwyg', label: '实时预览' },
  { id: 'split', label: '分屏预览' },
  { id: 'source', label: '源码' },
]
