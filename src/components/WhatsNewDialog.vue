<script setup lang="ts">
import { computed } from 'vue'
import { marked } from 'marked'
import { X } from 'lucide-vue-next'

const props = defineProps<{ content: string | null }>()

const emit = defineEmits<{ (e: 'close'): void }>()

const html = computed(() =>
  props.content ? (marked.parse(props.content, { async: false }) as string) : '',
)
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="content" class="modal-mask" role="presentation" @click.self="emit('close')">
        <div class="modal-card whatsnew-card" role="dialog" aria-modal="true" aria-label="更新说明">
          <header class="wn-header">
            <h3 class="wn-title">What's New</h3>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="15" :stroke-width="2" />
            </button>
          </header>
          <div class="wn-body md-body" v-html="html"></div>
          <footer class="wn-footer">
            <button class="pill-btn" type="button" @click="emit('close')">知道了</button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.whatsnew-card {
  width: 520px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
}
.wn-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.wn-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  color: var(--text-1);
}
.wn-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-right: 6px;
  font-size: 0.8125rem;
  line-height: 1.7;
  color: var(--text-2);
}
.wn-footer {
  display: flex;
  justify-content: flex-end;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}

/* 只读静态 Markdown 渲染 */
.md-body :deep(h1) {
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 12px 0 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.md-body :deep(h1:first-child) {
  margin-top: 0;
}
.md-body :deep(h2) {
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 14px 0 8px;
}
.md-body :deep(h3) {
  font-size: 0.84375rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 12px 0 6px;
}
.md-body :deep(p) {
  margin: 6px 0;
}
.md-body :deep(ul),
.md-body :deep(ol) {
  padding-left: 20px;
  margin: 6px 0;
}
.md-body :deep(ol) {
  list-style: decimal;
}
.md-body :deep(ol > li) {
  display: list-item;
}
.md-body :deep(li) {
  margin: 3px 0;
}
.md-body :deep(code) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  font-size: 0.75rem;
  font-family: 'FiraCode', Consolas, monospace;
}
.md-body :deep(pre) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 12px;
  overflow-x: auto;
  margin: 8px 0;
}
.md-body :deep(pre code) {
  background: transparent;
  border: none;
  padding: 0;
}
.md-body :deep(a) {
  color: var(--brand-500);
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
