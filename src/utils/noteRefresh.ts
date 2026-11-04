/**
 * 笔记列表刷新的补拉判定（纯函数，零依赖）。
 *
 * 背景：`list_notes` 只返回元信息（正文取字面 `''`，见 `repo/note.rs::list_meta`），
 * 所以刷新时不能整表替换——已有条目要保留本地正文，否则编辑器下次切笔记会把空正文
 * 写回库。但「用过就保留旧正文」的判据只能是 **updated_at 没变**：
 * 只按 id 判断会让外部写入（AI 助手工具、云同步、其它窗口）永远进不了界面——
 * 表现为 AI 说「已记录」、速记里却搜不到，重启（全量重读库）才出现。
 *
 * 判据放在这里而不是散在 store 里，是为了让 `tests/noteRefresh.test.mjs` 直接锁口径。
 */

/** 判定所需的最小字段集（Note 与 list_meta 的返回都满足） */
export interface NoteVersion {
  /** 后端时间戳文本；同一篇被外部改过即变化 */
  updated_at: string
  /** 正文（元信息接口里恒为空串） */
  content: string
}

/**
 * 这条笔记是否需要补拉正文全量。
 *
 * - 本地没有 → 是（外部新建的条目）。
 * - `updated_at` 变了 → 是（正文可能被外部改过，本地这份已过期）。
 * - `updated_at` 相同 → 否（本地正文就是最新的，省掉每条一次 `get_note`）。
 */
export function needsFullNoteFetch(old: NoteVersion | undefined, meta: NoteVersion): boolean {
  if (!old) return true
  return old.updated_at !== meta.updated_at
}
