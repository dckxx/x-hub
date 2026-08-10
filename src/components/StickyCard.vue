<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { StickyNote } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ slot: 1 | 2 }>()

const store = useStore()
const content = ref('')
let saveTimer: ReturnType<typeof setTimeout> | null = null

// 外部数据（初始加载 / 保存回显）同步到本地
watch(
  () => store.state.stickies.find((s) => s.slot === props.slot)?.content ?? '',
  (v) => {
    if (v === content.value) return
    content.value = v
  },
  { immediate: true },
)

// 输入即保存（600ms 防抖，与 NoteEditor 一致）
watch(content, () => {
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    store.saveSticky(props.slot, content.value)
  }, 600)
})

onBeforeUnmount(() => {
  if (saveTimer) clearTimeout(saveTimer)
})
</script>

<template>
  <section class="card sticky-card" :aria-label="`便签 ${slot}`">
    <header class="sticky-header">
      <h3 class="sticky-title">
        <StickyNote :size="14" :stroke-width="2" aria-hidden="true" />
        <span>便签</span>
      </h3>
    </header>
    <textarea
      v-model="content"
      class="sticky-input"
      placeholder="随手记…"
      spellcheck="false"
    ></textarea>
  </section>
</template>

<style scoped>
.sticky-card {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 12px;
}
.sticky-header {
  display: flex;
  align-items: center;
  margin-bottom: 8px;
  flex-shrink: 0;
}
.sticky-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.sticky-title :deep(svg) {
  color: var(--brand-500);
}
.sticky-input {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  resize: none;
  outline: none;
  font-size: 12px;
  line-height: 1.6;
  font-family: inherit;
  color: var(--text-2);
  padding: 8px;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
}
.sticky-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: var(--bg-card-solid);
}
.sticky-input::placeholder {
  color: var(--text-4);
}
</style>
