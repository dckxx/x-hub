import { ref } from 'vue'

/**
 * 子待办拖拽的共享瞬态（模块级单例：同一窗口内所有 TodoRow 共享）。
 *
 * 为什么需要共享：跨父拖拽时，「拖拽源行」要能把「落点信息」告诉「目标父行」——
 * 目标父行据此临时展开（空/折叠父行也能给出放置区、渲染插入线）。
 * 拖拽的指针逻辑仍在 TodoRow（源行）里，这里只放共享状态。
 *
 * 说明：待办浮窗是独立窗口（独立 JS 上下文），模块单例天然按窗口隔离。
 */
const subDragId = ref<number | null>(null)
/** 当前落点对应的目标父待办 id（拖到无有效父行时为 null） */
const dropTargetParentId = ref<number | null>(null)
/** 插入线在视口中的 y（fixed 定位，由拖拽源行绘制唯一一条） */
const dropLineClientY = ref<number | null>(null)
const dropLineLeft = ref(0)
const dropLineWidth = ref(0)

export function useTodoSubDrag() {
  return { subDragId, dropTargetParentId, dropLineClientY, dropLineLeft, dropLineWidth }
}
