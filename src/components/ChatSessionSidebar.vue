<script setup lang="ts">
import { computed, ref } from 'vue'
import { Pencil, Plus, Trash2 } from 'lucide-vue-next'
import type ChatPanel from './ChatPanel.vue'
import type { ChatSession } from '../api/tauri'

/**
 * 「AI 助手」视图的左侧会话列表（自 ChatView 拆出，见约定 33）：
 * 宿主常驻的 chat-layer 容器里与唯一 ChatPanel 并排（view 形态才显示）。
 * 会话状态由 ChatPanel 自持（同一 SQLite），实例经 props 传入，这里只做列表交互；
 * defineExpose 暴露的 ref 属性（sessions / activeSessionId）访问时自动解包。
 */

const props = defineProps<{
  /** 常驻 ChatPanel 的实例（宿主持有模板 ref，挂载前为 null，方法调用全部 ?. 防御） */
  panel: InstanceType<typeof ChatPanel> | null
}>()

const sessions = computed<ChatSession[]>(() => props.panel?.sessions ?? [])
const activeSessionId = computed<number | null>(() => props.panel?.activeSessionId ?? null)

function onNew() {
  void props.panel?.createSession()
}
function onPick(id: number) {
  void props.panel?.openSession(id)
}
function onDelete(id: number) {
  void props.panel?.deleteSession(id)
}

// ---- 行内重命名（速记文件夹树手感）：Enter/失焦保存，Esc 取消 ----
const renamingId = ref<number | null>(null)
const renameDraft = ref('')

function startRename(s: ChatSession) {
  renamingId.value = s.id
  renameDraft.value = s.title
}
function commitRename() {
  const id = renamingId.value
  const title = renameDraft.value
  renamingId.value = null
  renameDraft.value = ''
  if (!id || !title.trim() || title.trim() === sessions.value.find((x) => x.id === id)?.title) return
  void props.panel?.renameSession(id, title)
}
</script>

<template>
  <aside class="cv-side">
    <button class="cv-new" type="button" @click="onNew">
      <Plus :size="14" :stroke-width="2" aria-hidden="true" />
      新建会话
    </button>

    <div class="cv-list" role="listbox" aria-label="会话列表">
      <div
        v-for="s in sessions"
        :key="s.id"
        class="cv-item"
        :class="{ on: s.id === activeSessionId }"
        role="option"
        :aria-selected="s.id === activeSessionId"
        @click="onPick(s.id)"
      >
        <template v-if="renamingId === s.id">
          <input
            v-model="renameDraft"
            class="cv-rename"
            type="text"
            maxlength="60"
            aria-label="重命名会话"
            @click.stop
            @keydown.enter.prevent="commitRename"
            @keydown.esc="renamingId = null"
            @blur="commitRename"
          />
        </template>
        <template v-else>
          <span class="cv-item-title" :title="s.title">{{ s.title }}</span>
          <span class="cv-item-ops" @click.stop>
            <button class="cv-op" type="button" title="重命名" @click="startRename(s)">
              <Pencil :size="12" :stroke-width="2" aria-hidden="true" />
            </button>
            <button class="cv-op danger" type="button" title="删除会话" @click="onDelete(s.id)">
              <Trash2 :size="12" :stroke-width="2" aria-hidden="true" />
            </button>
          </span>
        </template>
      </div>
      <p v-if="sessions.length === 0" class="cv-empty">还没有会话，点上方「新建会话」开始</p>
    </div>
  </aside>
</template>

<style scoped>
.cv-side {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  /* 左/顶贴边（与速记等视图同口径），仅留右侧 10px 隔开面板、底部 12px */
  padding: 0 10px 12px 0;
}
.cv-new {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.cv-new:hover {
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.cv-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.cv-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.12s;
}
.cv-item:hover {
  background: var(--bg-card-soft);
}
.cv-item.on {
  background: var(--brand-50);
}
.cv-item-title {
  flex: 1;
  min-width: 0;
  font-size: 0.8125rem;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cv-item.on .cv-item-title {
  font-weight: 600;
  color: var(--brand-500);
}
.cv-item-ops {
  flex: none;
  display: none;
  gap: 2px;
}
.cv-item:hover .cv-item-ops,
.cv-item.on:hover .cv-item-ops {
  display: inline-flex;
}
.cv-op {
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  padding: 0;
  font-family: inherit;
  transition: background 0.12s, color 0.12s;
}
.cv-op:hover {
  background: var(--bg-card);
  color: var(--text-1);
}
.cv-op.danger:hover {
  color: var(--c-red);
}
.cv-rename {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--brand-500);
  border-radius: var(--radius-sm);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  padding: 3px 6px;
  outline: none;
}
.cv-empty {
  margin: 0;
  padding: 18px 8px;
  text-align: center;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-4);
}
</style>
