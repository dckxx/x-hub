<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'

const props = defineProps<{
  visible: boolean
  title: string
  initialValue?: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'submit', name: string): void
}>()

const name = ref('')
const inputRef = ref<HTMLInputElement | null>(null)

watch(
  () => props.visible,
  (v) => {
    if (v) {
      name.value = props.initialValue ?? ''
      setTimeout(() => inputRef.value?.focus(), 30)
    }
  },
)

function submit() {
  const trimmed = name.value.trim()
  if (!trimmed) return
  emit('submit', trimmed)
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') submit()
  if (e.key === 'Escape') emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="emit('close')">
        <div class="modal-card" role="dialog" :aria-label="title">
          <h2 class="dialog-title">{{ title }}</h2>
          <input
            ref="inputRef"
            v-model="name"
            class="field-input"
            type="text"
            maxlength="30"
            placeholder="输入分组名称"
            @keydown="onKeydown"
          />
          <div class="dialog-actions">
            <button class="ghost-btn btn" @click="emit('close')">取消</button>
            <button class="pill-btn btn" :disabled="!name.trim()" @click="submit">
              确定
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  margin-bottom: 16px;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
}
.btn {
  padding: 7px 20px;
}
.pill-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
