<script setup lang="ts">
import { onBeforeUnmount, ref, toRef, watch } from 'vue'
import { useFocusTrap } from '../composables/useFocusTrap'

/**
 * 通用确认弹窗（全局样式 .modal-mask / .modal-card）。
 * 待办勾选父条目时用：子待办还有未完成项，先让用户确认再一并勾选。
 */
const props = withDefaults(
  defineProps<{
    visible: boolean
    title: string
    message: string
    /** 补充说明（小字，可选） */
    hint?: string
    confirmText?: string
    cancelText?: string
    /** danger：确认按钮用警示色（如删除类操作） */
    tone?: 'default' | 'danger'
  }>(),
  {
    hint: '',
    confirmText: '确认',
    cancelText: '取消',
    tone: 'default',
  },
)

const emit = defineEmits<{ confirm: []; cancel: [] }>()

/** 焦点陷阱：Tab 循环留在弹窗内（与其它弹窗同一口径，键盘用户不会 Tab 穿到后台） */
const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('cancel')
  } else if (e.key === 'Enter') {
    e.preventDefault()
    emit('confirm')
  }
}

watch(
  () => props.visible,
  (on) => {
    if (typeof window === 'undefined') return
    if (on) window.addEventListener('keydown', onKeydown)
    else window.removeEventListener('keydown', onKeydown)
  },
  // immediate：挂载时 visible 已为 true 也要挂上监听（通用组件不能假设初始必为 false）
  { immediate: true },
)

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" data-floating-surface @click.self="emit('cancel')">
      <div ref="cardRef" class="modal-card confirm-card" role="dialog" aria-modal="true">
        <h2 class="confirm-title">{{ title }}</h2>
        <p class="confirm-msg">{{ message }}</p>
        <p v-if="hint" class="confirm-hint">{{ hint }}</p>
        <div class="confirm-foot">
          <button type="button" class="confirm-btn" @click="emit('cancel')">
            {{ cancelText }}
          </button>
          <button
            type="button"
            class="confirm-btn confirm-btn--primary"
            :class="{ 'confirm-btn--danger': tone === 'danger' }"
            @click="emit('confirm')"
          >
            {{ confirmText }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.confirm-card {
  width: 360px;
  padding: 20px 22px;
}
.confirm-title {
  margin: 0 0 10px;
  font-size: 0.92rem;
  font-weight: 700;
  color: var(--text-1);
}
.confirm-msg {
  margin: 0;
  font-size: 0.8rem;
  line-height: 1.65;
  color: var(--text-2);
}
.confirm-hint {
  margin: 8px 0 0;
  font-size: 0.72rem;
  line-height: 1.6;
  color: var(--text-4);
}
.confirm-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 18px;
}
.confirm-btn {
  padding: 6px 14px;
  font-size: 0.78rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.18s, color 0.18s, border-color 0.18s;
}
.confirm-btn:hover {
  color: var(--text-1);
  border-color: var(--border-strong);
}
.confirm-btn--primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
  font-weight: 600;
}
.confirm-btn--primary:hover {
  color: #fff;
  filter: brightness(1.06);
}
.confirm-btn--danger {
  background: var(--c-red-ink);
  border-color: var(--c-red-ink);
}
</style>
