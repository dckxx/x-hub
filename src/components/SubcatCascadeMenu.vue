<script setup lang="ts">
// 小类级联下拉（书签式逐级选择）：挂在速达小类 chips 的三角形按钮下，悬停展开下一级。
// - 每一级都按**实测矩形 fixed 定位**：顶层锚在触发按钮下（坐标由 Suda 实测传入，且 Teleport
//   到 body——小类行是 overflow-x:auto 的滚动容器，就地渲染会被裁剪；沉浸模式 .card 的
//   backdrop-filter 还会改写 fixed 的包含块）；子级锚在悬停行右侧。
//   ⚠️ 子级不能以 absolute 嵌在父菜单里走纯 CSS 级联：父菜单带 max-height + overflow-y:auto
//   （列表超高可滚），overflow 滚动容器会裁剪 absolute 后代——实测二级整层只露出一条缝、
//   完全不可见。fixed 后代不受祖先 overflow 裁剪（父菜单位于 body 下、无 transform/filter
//   祖先，包含块仍是视口），因此子级同为 fixed、坐标由悬停行矩形实测（openChild）。
// - 展开状态由 JS 管理：mouseenter/focusin 开、mouseleave 关、本列表 scroll 时收起
//   （fixed 子级不随父列表滚动，滚动后坐标即失效）。子菜单是父行的 DOM 后代，pointer
//   移入子菜单不会触发父行 mouseleave，行 → 菜单的斜向移动天然不断链。
// - 点击任意行即选中该路径——筛选含其子孙级（口径见 utils/subcategoryTree.ts）。
import { computed, ref } from 'vue'
import { ChevronRight } from 'lucide-vue-next'
import type { SubcatNode } from '../utils/subcategoryTree'

const props = defineProps<{
  /** 本级要列的节点（顶层菜单传顶层节点的 children，子菜单递归传下一层 children） */
  nodes: SubcatNode[]
  /** 当前选中的小类全路径（命中行高亮） */
  activePath: string
  /** fixed 锚点（视口坐标）：顶层来自触发按钮实测，子级来自悬停行矩形实测 */
  x: number
  y: number
  /** 顶层实例：Teleport 到 body（子级挂在父菜单内即可，不必也无处需要 Teleport） */
  teleport?: boolean
}>()

const emit = defineEmits<{ (e: 'select', path: string): void }>()

/** 菜单高度估算（行 ≈ 28px + 上下 padding/border 12，封顶 max-height 260 + 12）：
 *  top 锚点按它钳制，贴底触发时菜单不至于整层坠出视口外 */
const estHeight = computed(() => Math.min(props.nodes.length * 28 + 12, 272))

const posStyle = computed(() => ({
  position: 'fixed' as const,
  left: `${Math.max(8, Math.min(props.x, window.innerWidth - 156))}px`,
  top: `${Math.max(8, Math.min(props.y, window.innerHeight - estHeight.value - 8))}px`,
}))

// ---- 下级开合（本级持有的状态：openPath 行的子菜单在展） ----
const openPath = ref<string | null>(null)
const childPos = ref({ x: 0, y: 0 })

function openChild(n: SubcatNode, e: MouseEvent | FocusEvent) {
  if (!n.children.length) {
    // 悬停/聚焦到无下级的行：收起已展开的兄弟子菜单（经典级联行为）
    openPath.value = null
    return
  }
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  // 右侧放不下就翻到行左侧（min-width 132，留 4px 压边重叠，行 → 菜单移动不断链）
  const flip = rect.right + 152 > window.innerWidth
  childPos.value = {
    x: flip ? Math.max(8, rect.left - 128) : rect.right - 4,
    y: Math.max(8, Math.min(rect.top - 5, window.innerHeight - Math.min(n.children.length * 28 + 12, 272) - 8)),
  }
  openPath.value = n.path
}

function closeChild(n: SubcatNode) {
  if (openPath.value === n.path) openPath.value = null
}

function isActive(n: SubcatNode): boolean {
  return props.activePath === n.path || props.activePath.startsWith(`${n.path}/`)
}
</script>

<template>
  <Teleport to="body" :disabled="!teleport">
    <!-- SFC 文件名即自引用组件名：SubcatCascadeMenu 递归渲染下一级。
         mousedown.stop：窗口级「点外部关闭」监听不能把菜单内的按下当外部点击，
         否则菜单在 click 前就被拆掉、选择丢失；
         scroll 收起：fixed 子级不随本列表滚动，滚了坐标就作废 -->
    <ul
      class="sub-cascade"
      :style="posStyle"
      role="menu"
      @mousedown.stop
      @scroll.passive="openPath = null"
    >
      <li
        v-for="n in nodes"
        :key="n.path"
        class="sub-menu-item"
        role="none"
        @mouseenter="openChild(n, $event)"
        @mouseleave="closeChild(n)"
      >
        <button
          type="button"
          class="sub-menu-row"
          :class="{ active: isActive(n) }"
          role="menuitem"
          :aria-expanded="n.children.length ? openPath === n.path : undefined"
          :title="n.path"
          @focusin="openChild(n, $event)"
          @click="emit('select', n.path)"
        >
          <span class="sub-menu-name">{{ n.name }}</span>
          <ChevronRight
            v-if="n.children.length"
            :size="12"
            :stroke-width="2.2"
            class="sub-menu-arrow"
            aria-hidden="true"
          />
        </button>
        <SubcatCascadeMenu
          v-if="n.children.length && openPath === n.path"
          :nodes="n.children"
          :active-path="activePath"
          :x="childPos.x"
          :y="childPos.y"
          @select="emit('select', $event)"
        />
      </li>
    </ul>
  </Teleport>
</template>

<style scoped>
.sub-cascade {
  z-index: 130;
  min-width: 132px;
  max-height: 260px;
  overflow-y: auto;
  margin: 0;
  padding: 5px;
  list-style: none;
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-dock);
}
.sub-menu-item {
  position: relative;
}
.sub-menu-row {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 5px 8px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font: inherit;
  font-size: 0.75rem;
  color: var(--text-2);
  text-align: left;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.sub-menu-row:hover,
.sub-menu-row:focus-visible {
  background: var(--brand-50);
  color: var(--brand-500);
  outline: none;
}
.sub-menu-row.active {
  color: var(--brand-500);
  font-weight: 600;
}
.sub-menu-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sub-menu-arrow {
  flex-shrink: 0;
  color: var(--text-4);
}
.sub-menu-row:hover .sub-menu-arrow {
  color: var(--brand-500);
}
</style>
