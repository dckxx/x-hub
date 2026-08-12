<script setup lang="ts">
import { inject } from 'vue'
import { Boxes, Pin, Settings2 } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import type { Snippet } from '../api/tauri'

const props = defineProps<{ onOpenManage?: () => void }>()

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const snippets = () => store.state.snippets

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    // 剪贴板 API 不可用时回退隐藏 textarea + execCommand
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    let ok = false
    try {
      ok = document.execCommand('copy')
    } catch {
      ok = false
    }
    document.body.removeChild(ta)
    return ok
  }
}

async function onCopy(s: Snippet) {
  const ok = await copyText(s.content)
  if (!ok) {
    showToast('复制失败')
    return
  }
  try {
    await store.recordSnippetCopy(s.id)
  } catch {
    // 计数失败不阻塞复制反馈
  }
  showToast('已复制')
}
</script>

<template>
  <section class="card prompt-box" aria-label="提示词百宝箱">
    <header class="pb-header">
      <h3 class="pb-title">
        <Boxes :size="15" :stroke-width="2" aria-hidden="true" />
        <span>提示词</span>
      </h3>
      <button class="pb-more" type="button" @click="props.onOpenManage?.()">
        管理
        <Settings2 :size="13" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <div v-if="snippets().length > 0" class="pb-body">
      <button
        v-for="s in snippets()"
        :key="s.id"
        class="pb-row"
        type="button"
        :title="'点击复制：' + s.title"
        @click="onCopy(s)"
      >
        <span class="pb-row-title">
          <span class="pb-row-title-text">{{ s.title }}</span>
          <Pin
            v-if="s.is_pinned"
            class="pb-row-pin"
            :size="12"
            :stroke-width="2"
            aria-label="已置顶"
          />
        </span>
        <span class="pb-row-preview">{{ s.content }}</span>
      </button>
    </div>

    <div v-else class="pb-empty">
      <p class="pb-empty-title">暂无提示词</p>
      <p class="pb-empty-sub">常用的提示词片段都在这里</p>
      <button class="ghost-btn" type="button" @click="props.onOpenManage?.()">
        添加第一条
      </button>
    </div>
  </section>
</template>

<style scoped>
.prompt-box {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 16px;
  min-height: 0;
}
.pb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}
.pb-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.pb-more {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 12px;
  cursor: pointer;
  transition: color 0.18s;
}
.pb-more:hover {
  color: var(--brand-500);
}
.pb-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0 -4px;
  padding: 0 4px;
}
.pb-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  text-align: left;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.18s;
}
.pb-row:hover {
  background: var(--bg-card-soft);
}
.pb-row:active {
  transform: scale(0.99);
}
.pb-row-title {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}
.pb-row-title-text {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pb-row-pin {
  flex-shrink: 0;
  color: var(--brand-500);
}
.pb-row-preview {
  font-size: 12px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pb-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.pb-empty-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.pb-empty-sub {
  margin: 0 0 6px;
  font-size: 11px;
  color: var(--text-4);
}
</style>
