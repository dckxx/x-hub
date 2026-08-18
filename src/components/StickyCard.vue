<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { PanelTopClose, StickyNote } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ slot: 1 | 2 }>()

const store = useStore()
const content = ref('')
let saveTimer: ReturnType<typeof setTimeout> | null = null

// 是否已脱离为浮窗（决定 icon 是「脱离」还是「聚焦」）
const detached = computed(() =>
  store.state.detached.some((d) => d.slot === props.slot),
)

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

// 脱离/聚焦切换：未脱离 → 脱离；已脱离 → 聚焦已有浮窗
async function onDetachClick() {
  if (detached.value) {
    await store.focusDetachedSticky(props.slot)
  } else {
    // 先落盘当前输入，避免防抖缓存丢字
    if (saveTimer) {
      clearTimeout(saveTimer)
      saveTimer = null
      await store.saveSticky(props.slot, content.value)
    }
    try {
      await store.detachSticky(props.slot)
    } catch (e) {
      // 后端已存在浮窗（并发场景），转为聚焦
      await store.focusDetachedSticky(props.slot)
      console.warn('detach sticky fallback to focus:', e)
    }
  }
}
</script>

<template>
  <section class="card sticky-card" :aria-label="`便签 ${slot}`">
    <header class="sticky-header">
      <h3 class="sticky-title">
        <StickyNote :size="14" :stroke-width="2" aria-hidden="true" />
        <span>便签</span>
      </h3>
      <button
        class="icon-btn sticky-detach"
        :class="{ active: detached }"
        :title="detached ? '便签已脱离，点击聚焦浮窗' : '脱离为悬浮便签'"
        :aria-label="detached ? '聚焦悬浮便签' : '脱离为悬浮便签'"
        type="button"
        @click="onDetachClick"
      >
        <PanelTopClose :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
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
  /* 便签模块字号：全局基准 × 模块系数，内部字号用 em 相对此基准 */
  font-size: calc(1rem * var(--fs-sticky, 1));
}
.sticky-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
  flex-shrink: 0;
}
.sticky-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.sticky-title :deep(svg) {
  color: var(--brand-500);
}
.sticky-detach {
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  color: var(--text-3);
}
.sticky-detach:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.sticky-detach.active {
  color: var(--brand-500);
  background: var(--brand-50);
}
.sticky-detach.active svg {
  stroke: var(--brand-500);
  fill: color-mix(in srgb, var(--brand-500) 18%, transparent);
}
.sticky-input {
  flex: 1;
  min-height: 0;
  width: 100%;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  resize: none;
  outline: none;
  font-size: 0.75em;
  line-height: 1.6;
  font-family: inherit;
  color: var(--text-2);
  padding: 8px;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
}
.sticky-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: color-mix(in srgb, var(--input-bg) 88%, #fff);
}
.sticky-input::placeholder {
  color: var(--text-4);
}
</style>
