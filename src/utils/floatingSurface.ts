/**
 * 「现在屏幕上有浮层吗」的唯一判定（主窗 ESC 就近收起用，2026-11-05）。
 *
 * 场景：主窗监听 `keydown` 的 ESC，按下去按「速达内嵌网页 → AI 对话抽屉 → 收进托盘」
 * 的顺序就近收。但弹窗/右键菜单/下拉/斜杠菜单自己也要吃 ESC——它们先处理并把事件
 * `preventDefault`，主窗这层就不该再动手，否则「关个确认框顺手把窗口收进托盘」。
 *
 * 旧实现把 class 名单硬编码在 index.vue 的 `hasFloatingSurface()` 里，新增浮层忘了
 * 加名字就会误收主窗。现在统一成两件事：
 *   1. 新写的浮层在根元素上加 `data-floating-surface`（显式声明，最可靠）；
 *   2. 历史浮层靠下面的 class 名单兜底（改名要同步这里）。
 *
 * 只做「有没有」判断，不判断在最上层——ESC 的语义就是「先关最上面的那个」，
 * 只要还有浮层开着，主窗这一层就一律让行。
 */

/** 浮层选择器：显式标记优先，其后是历史 class 名单 */
export const FLOATING_SURFACE_SELECTOR = [
  '[data-floating-surface]',
  '.modal-mask', // 通用弹窗遮罩（ConfirmDialog 以及各表单弹窗）
  '.ctx-menu', // 右键菜单
  '.cp-menu', // 对话面板的会话/模型菜单
  '.app-select-menu', // 通用下拉（AppSelect 的 Portal 弹层）
  '.milkdown-slash-menu[data-show="true"]', // 编辑器斜杠菜单
  '.milkdown-link-preview[data-show="true"]', // 编辑器链接气泡
].join(', ')

/**
 * 是否还有浮层开着。
 * @param doc 便于测试注入；默认当前 document
 */
export function hasFloatingSurface(doc: Document = document): boolean {
  if (!doc || typeof doc.querySelector !== 'function') return false
  return doc.querySelector(FLOATING_SURFACE_SELECTOR) !== null
}
