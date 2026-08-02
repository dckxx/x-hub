<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

export interface ContextMenuItem {
  label: string
  danger?: boolean
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
  },
)

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
  window.addEventListener('blur', globalClose)
  window.addEventListener('resize', globalClose)
})

onBeforeUnmount(() => {
  window.removeEventListener('click', globalClose)
  window.removeEventListener('contextmenu', globalClose)
  window.removeEventListener('blur', globalClose)
  window.removeEventListener('resize', globalClose)
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
        <button
          v-for="(item, i) in items"
          :key="i"
          class="ctx-item"
          :class="{ danger: item.danger }"
          role="menuitem"
          @click="onItemClick(item)"
        >
          {{ item.label }}
        </button>
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
  font-size: 13px;
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
