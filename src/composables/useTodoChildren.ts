import { computed, type ComputedRef } from 'vue'
import { useStore } from '../stores/workbench'
import type { Todo } from '../api/tauri'
import { compareChildOrder } from '../utils/todoSchedule'

/**
 * 父待办 → 子待办列表（仅一层）：sort_order 优先，未排序按创建时间正序
 * （先加的在上，新增子待办追加到末尾，见 compareChildOrder）。
 * 待办卡与待办浮窗共用；一次建 Map 供所有 TodoRow 查找，避免每行各自过滤全部待办。
 */
export function useTodoChildren(): ComputedRef<Map<number, Todo[]>> {
  const store = useStore()
  return computed(() => {
    const map = new Map<number, Todo[]>()
    for (const t of store.state.todos) {
      if (t.parent_id == null) continue
      const list = map.get(t.parent_id)
      if (list) list.push(t)
      else map.set(t.parent_id, [t])
    }
    for (const list of map.values()) {
      list.sort(compareChildOrder)
    }
    return map
  })
}
