import { ref, type ComputedRef, type Ref } from 'vue'
import type { Resource } from '../api/tauri'

/** 分区模式的渲染块：zone=null 为尾部「未分区」兜底块 */
export interface SudaZoneGroup {
  key: number | null
  items: Resource[]
}

/**
 * 速达「全部」tab 分区模式的长按拖拽（指针实现，与 useSudaDrag 同款约束）：
 * 拖拽跨分区 = 改归属（zone_id）+ 按落点插入序，两者由调用方在
 * reorder_resources_zoned 一条命令里原子落盘——本 composable 只负责手势与落点推演。
 *
 * 与平铺版 useSudaDrag 的差异全部来自「多块网格」：
 * - 落点先定块（指针纵向命中某分区块矩形；落在块间隙时归垂直最近的块，
 *   不存在「没对准 = 丢失操作」），再在块内按行/中心找插入位；
 * - 空分区没有卡片可参照，块本身就是落点（插入到块末尾 = 唯一位置）；
 * - 「未分区」块恒在尾部，也是合法落点（拖出分区 = zone_id 置 null）。
 *
 * 被拖卡片的飞行位移仍由 dragOrigin + dragOffset 决定：各分区网格
 * （.suda-grid）自带 position:relative，offsetLeft/Top 与 absolute 化后的
 * 定位上下文一致，跨块拖拽的跟手位置计算与平铺版完全相同。
 */
export function useSudaZoneDrag(opts: {
  bodyRef: Ref<HTMLElement | null>
  groups: ComputedRef<SudaZoneGroup[]>
  enabled: () => boolean
  commit: (entries: { id: number; zoneId: number | null }[]) => void
}) {
  const draggingId = ref<number | null>(null)
  /** 拖拽位移（px，指针相对按下点） */
  const dragOffset = ref({ x: 0, y: 0 })
  /** 被拖卡片在其分区网格里的原始左上角（px） */
  const dragOrigin = ref({ x: 0, y: 0 })
  /** 落点目标块（分区 id 的字符串形；'' = 未分区块）——与 DOM data-zkey 同口径便于比对 */
  const dropZoneKey = ref<string | null>(null)
  /** 落点插槽：插到目标块内该 id 的卡片之前 */
  const dropBeforeId = ref<number | null>(null)
  /** 落点插槽：插到目标块末尾（空块即整块为落点） */
  const dropAtZoneEnd = ref(false)

  /** 拎起后的那次 click 要吞掉，防止误启动 */
  let blockClick = false

  const LONG_PRESS_MS = 300
  const SLOP_PX = 8
  const MOUSE_SLOP_PX = 4

  function swallowClick(): boolean {
    if (blockClick) {
      blockClick = false
      return true
    }
    return false
  }

  function zoneBlocks(): HTMLElement[] {
    const body = opts.bodyRef.value
    if (!body) return []
    return Array.from(body.querySelectorAll<HTMLElement>(':scope .suda-zone'))
  }

  function onCardPointerDown(r: Resource, e: PointerEvent) {
    if (e.button !== 0 || !opts.enabled()) return
    const target = e.target as HTMLElement | null
    if (target?.closest('button, input, a, [data-no-drag]')) return
    blockClick = false
    const el = e.currentTarget as HTMLElement
    const startX = e.clientX
    const startY = e.clientY
    let armed = false
    let dead = false

    const armTimer = window.setTimeout(() => {
      if (dead) return
      begin()
      armed = true
    }, LONG_PRESS_MS)

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)

    function begin() {
      try {
        el.setPointerCapture(e.pointerId)
      } catch {
        /* 指针已释放时忽略；移动与松开都由 window 监听兜底 */
      }
      draggingId.value = r.id
      dragOrigin.value = { x: el.offsetLeft, y: el.offsetTop }
      dragOffset.value = { x: 0, y: 0 }
      dropZoneKey.value = null
      dropBeforeId.value = null
      dropAtZoneEnd.value = false
      document.body.classList.add('suda-dragging')
      document.getSelection()?.removeAllRanges()
    }

    function onMove(ev: PointerEvent) {
      if (dead) return
      if (ev.buttons === 0) {
        onUp()
        return
      }
      const dx = ev.clientX - startX
      const dy = ev.clientY - startY
      const isMouse = ev.pointerType === 'mouse'
      if (!armed) {
        if (Math.hypot(dx, dy) < (isMouse ? MOUSE_SLOP_PX : SLOP_PX)) return
        if (isMouse) {
          window.clearTimeout(armTimer)
          begin()
          armed = true
        } else {
          // 触摸/笔：保留长按语义；已移动过的松手吞掉 click 防误启动
          dead = true
          blockClick = true
          cleanup()
          return
        }
      }
      dragOffset.value = { x: dx, y: dy }
      updateDrop(ev.clientX, ev.clientY)
    }

    function onUp() {
      if (dead) return
      dead = true
      cleanup()
      if (!armed) return
      blockClick = true
      finishDrag()
    }

    function cleanup() {
      window.clearTimeout(armTimer)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
    }

    /** 指针位置 → (目标块, 块内插入位)。分区块横向流式并排（宽度不一、可换行），
     *  命中判定必须是二维的：先找矩形包含指针的块，间隙归平面上最近的块 */
    function updateDrop(x: number, y: number) {
      const blocks = zoneBlocks()
      if (!blocks.length) return
      let hit: HTMLElement | null = null
      let nearest: HTMLElement | null = null
      let bestDist = Infinity
      for (const b of blocks) {
        const rect = b.getBoundingClientRect()
        if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
          hit = b
          break
        }
        // 指针到矩形的最近距离（钳到矩形边上算），间隙/换行空隙归最近块
        const dx = x < rect.left ? rect.left - x : x > rect.right ? x - rect.right : 0
        const dy = y < rect.top ? rect.top - y : y > rect.bottom ? y - rect.bottom : 0
        const d = dx * dx + dy * dy
        if (d < bestDist) {
          bestDist = d
          nearest = b
        }
      }
      const block = hit ?? nearest
      if (!block) return
      const cards = Array.from(
        block.querySelectorAll<HTMLElement>(':scope .suda-zone-grid > .suda-card'),
      ).filter((c) => c.dataset.id !== String(draggingId.value))
      // 块内阅读序找插入点（同 useSudaDrag：行前/中心左右）
      let insert = cards.length
      for (let i = 0; i < cards.length; i++) {
        const rect = cards[i].getBoundingClientRect()
        const sameRow = y >= rect.top && y <= rect.bottom
        if (
          (!sameRow && y < rect.top + rect.height / 2) ||
          (sameRow && x < rect.left + rect.width / 2)
        ) {
          insert = i
          break
        }
      }
      dropZoneKey.value = block.dataset.zkey ?? ''
      if (insert >= cards.length) {
        dropBeforeId.value = null
        dropAtZoneEnd.value = true
      } else {
        dropBeforeId.value = Number(cards[insert].dataset.id)
        dropAtZoneEnd.value = false
      }
    }

    /** 松开落点：按块重建全表序列（顺序 + 归属）交调用方原子落盘 */
    function finishDrag() {
      const id = draggingId.value
      draggingId.value = null
      dragOffset.value = { x: 0, y: 0 }
      const zoneKey = dropZoneKey.value
      const before = dropBeforeId.value
      const atEnd = dropAtZoneEnd.value
      dropZoneKey.value = null
      dropBeforeId.value = null
      dropAtZoneEnd.value = false
      document.body.classList.remove('suda-dragging')
      if (id == null || zoneKey == null) return
      const groups = opts.groups.value.map((g) => ({
        key: g.key,
        ids: g.items.map((x) => x.id).filter((x) => x !== id),
      }))
      const target = groups.find((g) => (g.key == null ? '' : String(g.key)) === zoneKey)
      if (!target) return
      let insert: number
      if (atEnd) {
        insert = target.ids.length
      } else {
        const idx = target.ids.indexOf(before as number)
        if (idx < 0) return
        insert = idx
      }
      target.ids.splice(insert, 0, id)
      const entries = groups.flatMap((g) => g.ids.map((rid) => ({ id: rid, zoneId: g.key })))
      // 同位松手（顺序与归属都没变）：不落盘
      const cur = opts.groups.value.flatMap((g) => g.items.map((r) => ({ id: r.id, zoneId: g.key })))
      const same =
        entries.length === cur.length &&
        entries.every((x, i) => x.id === cur[i].id && x.zoneId === cur[i].zoneId)
      if (!same) opts.commit(entries)
    }
  }

  return {
    draggingId,
    dragOffset,
    dragOrigin,
    dropZoneKey,
    dropBeforeId,
    dropAtZoneEnd,
    onCardPointerDown,
    swallowClick,
  }
}
