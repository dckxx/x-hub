<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

export interface ContextMenuItem {
  label: string
  danger?: boolean
  /** 在该项上方渲染一条分隔线（用于分组，如「用 XX 浏览器打开」组） */
  dividerBefore?: boolean
  onClick: () => void
}

const props = defineProps<{
  visible: boolean
  x: number
  y: number
  items: ContextMenuItem[]
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const menuRef = ref<HTMLElement | null>(null)
const pos = ref({ x: 0, y: 0 })
const activeIndex = ref(0)

watch(
  () => props.visible,
  async (v) => {
    if (!v) return
    await nextTick()
    const w = menuRef.value?.offsetWidth ?? 168
    const h = menuRef.value?.offsetHeight ?? 0
    pos.value = {
      x: Math.max(4, Math.min(props.x, window.innerWidth - w - 8)),
      y: Math.max(4, Math.min(props.y, window.innerHeight - h - 8)),
    }
    activeIndex.value = 0
    focusItem(0)
  },
)

function focusItem(i: number) {
  const btns = menuRef.value?.querySelectorAll<HTMLButtonElement>('.ctx-item') ?? []
  btns[i]?.focus()
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  if (props.items.length === 0) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value + 1) % props.items.length
    focusItem(activeIndex.value)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value - 1 + props.items.length) % props.items.length
    focusItem(activeIndex.value)
  } else if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    onItemClick(props.items[activeIndex.value])
  }
}

function globalClose() {
  if (props.visible) emit('close')
}

function onItemClick(item: ContextMenuItem) {
  item.onClick()
  emit('close')
}

onMounted(() => {
  window.addEventListener('click', globalClose)
  window.addEventListener('contextmenu', globalClose)
  window.addEventListener('resize', globalClose)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('click', globalClose)
  window.removeEventListener('contextmenu', globalClose)
  window.removeEventListener('resize', globalClose)
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="menu">
      <div
        v-if="visible"
        ref="menuRef"
        class="ctx-menu"
        :style="{ left: pos.x + 'px', top: pos.y + 'px' }"
        role="menu"
        @click.stop
        @contextmenu.stop
      >
        <template v-for="(item, i) in items" :key="i">
          <div v-if="item.dividerBefore" class="ctx-divider" role="separator" />
          <button
            class="ctx-item"
            :class="{ danger: item.danger }"
            role="menuitem"
            :tabindex="i === activeIndex ? 0 : -1"
            @click="onItemClick(item)"
            @mouseenter="activeIndex = i"
          >
            {{ item.label }}
          </button>
        </template>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ctx-menu {
  position: fixed;
  z-index: 300;
  min-width: 168px;
  padding: 6px;
  background: var(--bg-card);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-dock);
  border: 1px solid var(--border-soft);
}
.ctx-item {
  display: block;
  width: 100%;
  text-align: left;
  border: none;
  background: transparent;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  font-size: 0.8125rem;
  font-family: inherit;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.ctx-item:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ctx-item.danger {
  color: var(--c-red);
}
.ctx-item.danger:hover {
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
  color: var(--c-red);
}
.ctx-divider {
  height: 1px;
  margin: 5px 8px;
  background: var(--border-soft);
}

.menu-enter-active,
.menu-leave-active {
  transition: opacity 0.12s ease-out, transform 0.12s ease-out;
}
.menu-enter-from,
.menu-leave-to {
  opacity: 0;
  transform: scale(0.96);
}
</style>
