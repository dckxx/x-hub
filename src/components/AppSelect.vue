<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import { Check, ChevronDown } from 'lucide-vue-next'

export interface AppSelectOption {
  value: string
  label: string
  /** 分组标题：同一分组的选项在弹出层中合并展示为「标题 + 选项」 */
  group?: string
}

// 触发器按钮需接收外部 class/style（宽度控制），关闭属性自动继承
defineOptions({ inheritAttrs: false })

const props = defineProps<{
  modelValue: string
  options: readonly AppSelectOption[]
  ariaLabel?: string
  /** 弹出层最小宽度（px），默认与触发器同宽 */
  menuMinWidth?: number
}>()

const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>()

const open = ref(false)
const activeIndex = ref(0)
const keyboardNav = ref(false)
const triggerRef = ref<HTMLButtonElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)
const popupId = useId()
const pos = ref({ x: 0, y: 0, width: 0, openUp: false })

const selectedLabel = computed(
  () => props.options.find((o) => o.value === props.modelValue)?.label ?? props.modelValue,
)

watch(open, async (v) => {
  if (!v) return
  activeIndex.value = Math.max(0, props.options.findIndex((o) => o.value === props.modelValue))
  keyboardNav.value = false
  await nextTick()
  positionMenu()
})

/** 弹出层定位：宽度至少能容纳全部选项；下方空间不足时向上展开，并做视口钳制 */
function positionMenu() {
  const trigger = triggerRef.value
  const menu = menuRef.value
  if (!trigger || !menu) return
  const tr = trigger.getBoundingClientRect()
  const gap = 6
  // 先松开固定宽度让菜单按内容自然撑开，量出真实内容宽度（选项 nowrap 不会换行）。
  // 之前直接量 menu.scrollWidth：菜单宽度被钉在触发器窄宽度上，长选项会被裁成省略号看不见字。
  const prevWidth = menu.style.width
  menu.style.width = 'auto'
  const contentWidth = menu.offsetWidth
  const wanted = Math.max(tr.width, contentWidth, props.menuMinWidth ?? 0, 200)
  // 视口钳制：极端窄窗口下仍保证右侧留边，溢出文本以省略号兜底
  const menuWidth = Math.min(wanted, window.innerWidth - 16)
  // 按最终宽度重测高度（钳制导致换行时取实际高度，offsetWidth 不含 transform，不受滑入动画缩放影响）
  menu.style.width = menuWidth + 'px'
  const menuHeight = menu.offsetHeight
  menu.style.width = prevWidth
  const x = Math.max(8, Math.min(tr.left, window.innerWidth - menuWidth - 8))
  const spaceBelow = window.innerHeight - tr.bottom - 8
  const spaceAbove = tr.top - 8
  let y: number
  let openUp = false
  if (spaceBelow >= menuHeight + gap) {
    y = tr.bottom + gap
  } else if (spaceAbove >= menuHeight + gap) {
    y = Math.max(8, tr.top - menuHeight - gap)
    openUp = true
  } else {
    y = Math.max(8, Math.min(tr.bottom + gap, window.innerHeight - menuHeight - 8))
  }
  pos.value = { x, y, width: menuWidth, openUp }
}

function closeMenu() {
  open.value = false
  triggerRef.value?.focus()
}

function selectOption(option: AppSelectOption) {
  emit('update:modelValue', option.value)
  open.value = false
  triggerRef.value?.focus()
}

function onOptionHover(i: number) {
  keyboardNav.value = false
  activeIndex.value = i
}

function onTriggerKeydown(e: KeyboardEvent) {
  if (props.options.length === 0) return
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopPropagation()
    if (!open.value) {
      open.value = true
      return
    }
    keyboardNav.value = true
    activeIndex.value =
      e.key === 'ArrowDown'
        ? (activeIndex.value + 1) % props.options.length
        : (activeIndex.value - 1 + props.options.length) % props.options.length
    return
  }
  if (!open.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    closeMenu()
  } else if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    e.stopPropagation()
    selectOption(props.options[activeIndex.value])
  }
}

// 焦点不在触发器上时的键盘兜底（如鼠标点击打开后的场景）
function onWindowKeydown(e: KeyboardEvent) {
  if (!open.value) return
  if (props.options.length === 0) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopImmediatePropagation()
    closeMenu()
    return
  }
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopImmediatePropagation()
    keyboardNav.value = true
    activeIndex.value =
      e.key === 'ArrowDown'
        ? (activeIndex.value + 1) % props.options.length
        : (activeIndex.value - 1 + props.options.length) % props.options.length
  } else if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    e.stopImmediatePropagation()
    selectOption(props.options[activeIndex.value])
  }
}

function onWindowClick() {
  if (open.value) open.value = false
}

function onWindowResize() {
  if (open.value) positionMenu()
}

onMounted(() => {
  window.addEventListener('click', onWindowClick)
  window.addEventListener('keydown', onWindowKeydown)
  window.addEventListener('resize', onWindowResize)
})

onBeforeUnmount(() => {
  window.removeEventListener('click', onWindowClick)
  window.removeEventListener('keydown', onWindowKeydown)
  window.removeEventListener('resize', onWindowResize)
})
</script>

<template>
  <button
    ref="triggerRef"
    v-bind="$attrs"
    type="button"
    class="app-select-trigger"
    :class="{ open }"
    :aria-label="ariaLabel"
    :aria-expanded="open"
    aria-haspopup="listbox"
    :aria-controls="open ? popupId : undefined"
    :aria-activedescendant="open ? `${popupId}-opt-${activeIndex}` : undefined"
    @click.stop="open = !open"
    @keydown="onTriggerKeydown"
  >
    <span class="app-select-label" :title="selectedLabel">{{ selectedLabel }}</span>
    <ChevronDown
      class="app-select-chevron"
      :size="14"
      :stroke-width="2"
      aria-hidden="true"
    />
  </button>

  <Teleport to="body">
    <Transition name="menu">
      <div
        v-if="open"
        ref="menuRef"
        :id="popupId"
        class="app-select-menu"
        :class="{ 'open-up': pos.openUp }"
        :style="{ left: pos.x + 'px', top: pos.y + 'px', width: pos.width + 'px' }"
        role="listbox"
        :aria-label="ariaLabel"
        @click.stop
      >
        <template v-for="(option, i) in options" :key="option.value">
          <div
            v-if="option.group && (i === 0 || options[i - 1].group !== option.group)"
            class="app-select-group"
            :title="option.group"
          >
            {{ option.group }}
          </div>
          <button
            :id="`${popupId}-opt-${i}`"
            class="app-select-option"
            :class="{
              active: i === activeIndex,
              keyboard: keyboardNav && i === activeIndex,
              selected: option.value === modelValue,
            }"
            role="option"
            type="button"
            :aria-selected="option.value === modelValue"
            @click="selectOption(option)"
            @mouseenter="onOptionHover(i)"
          >
            <span class="app-select-option-label" :title="option.label">{{ option.label }}</span>
            <Check
              v-if="option.value === modelValue"
              class="app-select-check"
              :size="14"
              :stroke-width="2"
              aria-hidden="true"
            />
          </button>
        </template>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.app-select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 38px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
}
.app-select-trigger:hover {
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
}
.app-select-trigger[aria-expanded='true'] {
  border-color: color-mix(in srgb, var(--brand-500) 45%, transparent);
}
.app-select-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-select-chevron {
  flex-shrink: 0;
  color: var(--text-3);
  transition: transform 0.15s ease-out;
}
.app-select-trigger.open .app-select-chevron {
  transform: rotate(180deg);
}

.app-select-menu {
  position: fixed;
  z-index: 300;
  padding: 6px;
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-dock);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
}
.app-select-menu.open-up {
  transform-origin: bottom center;
}
.app-select-group {
  padding: 6px 12px 4px;
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.03em;
  color: var(--text-4);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.app-select-option.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.app-select-option.active.keyboard {
  box-shadow: var(--shadow-focus);
}
.app-select-option.selected {
  color: var(--brand-500);
  font-weight: 600;
}
.app-select-option-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.app-select-check {
  flex-shrink: 0;
  color: var(--brand-500);
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
