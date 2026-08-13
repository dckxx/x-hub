<script setup lang="ts">
import {
  computed,
  inject,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  toRef,
  watch,
  type ComponentPublicInstance,
} from 'vue'
import { Pencil, Pin, Plus, Trash2, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import { useStore } from '../stores/workbench'
import type { Snippet } from '../api/tauri'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)
const store = useStore()

// ---- 顶部新增表单 ----
const adding = ref(false)
const newTitle = ref('')
const newContent = ref('')
const newTitleRef = ref<HTMLInputElement | null>(null)

// ---- 行内编辑 ----
const editingId = ref<number | null>(null)
const editTitle = ref('')
const editContent = ref('')
const editTitleRef = ref<HTMLInputElement | null>(null)
function setEditTitle(el: Element | ComponentPublicInstance | null) {
  editTitleRef.value = el instanceof HTMLInputElement ? el : null
}

const canAddSave = computed(
  () => newTitle.value.trim() !== '' && newContent.value.trim() !== '',
)
const canEditSave = computed(
  () => editTitle.value.trim() !== '' && editContent.value.trim() !== '',
)

function openAdd() {
  editingId.value = null
  adding.value = true
  void nextTick(() => newTitleRef.value?.focus())
}

async function saveAdd() {
  const title = newTitle.value.trim()
  const content = newContent.value.trim()
  if (!title || !content) return
  await store.addSnippet(title, content)
  cancelAdd()
  showToast('已添加')
}

function cancelAdd() {
  adding.value = false
  newTitle.value = ''
  newContent.value = ''
}

function startEdit(s: Snippet) {
  adding.value = false
  editingId.value = s.id
  editTitle.value = s.title
  editContent.value = s.content
  void nextTick(() => editTitleRef.value?.focus())
}

async function saveEdit(s: Snippet) {
  const title = editTitle.value.trim()
  const content = editContent.value.trim()
  if (!title || !content) return
  await store.editSnippet(s.id, title, content)
  editingId.value = null
  showToast('已保存')
}

function cancelEdit() {
  editingId.value = null
}

async function togglePin(s: Snippet) {
  await store.toggleSnippetPin(s.id)
}

async function remove(s: Snippet) {
  await store.removeSnippet(s.id)
  if (editingId.value === s.id) editingId.value = null
  showToast('已删除', {
    label: '撤销',
    onClick: async () => {
      await store.addSnippet(s.title, s.content)
      showToast('已恢复提示词')
    },
  })
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.visible) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

// 关闭时重置表单状态，保证下次打开干净
watch(
  () => props.visible,
  (v) => {
    if (!v) {
      adding.value = false
      newTitle.value = ''
      newContent.value = ''
      editingId.value = null
      editTitle.value = ''
      editContent.value = ''
    }
  },
)
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div
          ref="cardRef"
          class="modal-card pm-card"
          role="dialog"
          aria-label="提示词管理"
          aria-modal="true"
        >
          <div class="pm-head">
            <div class="pm-head-title">
              <h2 class="dialog-title">提示词管理</h2>
              <button
                v-if="!adding"
                class="icon-btn pm-head-add"
                type="button"
                title="新增提示词"
                aria-label="新增提示词"
                @click="openAdd"
              >
                <Plus :size="15" :stroke-width="2" />
              </button>
            </div>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" />
            </button>
          </div>

          <!-- 新增表单 -->
          <div v-if="adding" class="pm-form">
            <input
              ref="newTitleRef"
              v-model="newTitle"
              class="field-input"
              type="text"
              maxlength="80"
              placeholder="提示词标题"
              @keydown.esc="cancelAdd"
            />
            <textarea
              v-model="newContent"
              class="pm-textarea"
              rows="3"
              placeholder="提示词内容"
              spellcheck="false"
              @keydown.esc="cancelAdd"
            ></textarea>
            <div class="pm-form-actions">
              <button class="ghost-btn" type="button" @click="cancelAdd">取消</button>
              <button class="pill-btn" type="button" :disabled="!canAddSave" @click="saveAdd">
                保存
              </button>
            </div>
          </div>

          <div v-if="store.state.snippets.length > 0" class="pm-list">
            <div
              v-for="s in store.state.snippets"
              :key="s.id"
              class="pm-row"
              :class="{ editing: editingId === s.id }"
            >
              <template v-if="editingId === s.id">
                <div class="pm-form">
                  <input
                    :ref="setEditTitle"
                    v-model="editTitle"
                    class="field-input"
                    type="text"
                    maxlength="80"
                    placeholder="提示词标题"
                    @keydown.esc="cancelEdit"
                  />
                  <textarea
                    v-model="editContent"
                    class="pm-textarea"
                    rows="3"
                    placeholder="提示词内容"
                    spellcheck="false"
                    @keydown.esc="cancelEdit"
                  ></textarea>
                  <div class="pm-form-actions">
                    <button class="ghost-btn" type="button" @click="cancelEdit">取消</button>
                    <button
                      class="pill-btn"
                      type="button"
                      :disabled="!canEditSave"
                      @click="saveEdit(s)"
                    >
                      保存
                    </button>
                  </div>
                </div>
              </template>

              <template v-else>
                <div class="pm-row-main">
                  <span class="pm-row-title">{{ s.title }}</span>
                  <p class="pm-row-preview">{{ s.content }}</p>
                </div>
                <div class="pm-row-actions">
                  <button
                    class="icon-btn pm-action"
                    :class="{ active: s.is_pinned }"
                    :title="s.is_pinned ? '取消置顶' : '置顶'"
                    :aria-label="s.is_pinned ? '取消置顶' : '置顶'"
                    @click="togglePin(s)"
                  >
                    <Pin :size="14" :stroke-width="2" />
                  </button>
                  <button
                    class="icon-btn pm-action"
                    title="编辑"
                    aria-label="编辑"
                    @click="startEdit(s)"
                  >
                    <Pencil :size="14" :stroke-width="2" />
                  </button>
                  <button
                    class="icon-btn pm-action pm-danger"
                    title="删除"
                    aria-label="删除"
                    @click="remove(s)"
                  >
                    <Trash2 :size="14" :stroke-width="2" />
                  </button>
                </div>
              </template>
            </div>
          </div>

          <div v-else-if="!adding" class="pm-empty">
            <p>暂无提示词，点击右上角 ＋ 添加第一条</p>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.pm-card {
  width: 560px;
  max-height: calc(100vh - 80px);
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
}
.pm-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.dialog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
}
.pm-head-title {
  display: flex;
  align-items: center;
  gap: 6px;
}
.pm-head-add {
  width: 26px;
  height: 26px;
}
.pm-head-add:hover {
  color: var(--brand-500);
}
.pm-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 12px;
}
.pm-textarea {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.6;
  padding: 9px 12px;
  outline: none;
  resize: vertical;
  transition: border-color 0.18s, box-shadow 0.18s, background 0.18s;
}
.pm-textarea:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: color-mix(in srgb, var(--input-bg) 88%, #fff);
}
.pm-textarea::placeholder {
  color: var(--text-4);
}
.pm-form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.pm-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0 -6px;
  padding: 0 6px;
}
.pm-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  border-radius: var(--radius-md);
  transition: background 0.18s;
}
.pm-row:hover {
  background: var(--bg-card-soft);
}
.pm-row.editing {
  background: var(--bg-card-soft);
}
.pm-row-main {
  flex: 1;
  min-width: 0;
}
.pm-row-title {
  display: block;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pm-row-preview {
  margin: 2px 0 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.pm-row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.pm-action.active {
  color: var(--brand-500);
  background: var(--brand-50);
}
.pm-action.pm-danger:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.pm-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 32px 16px;
  text-align: center;
  font-size: 13px;
  color: var(--text-3);
}
.pm-empty p {
  margin: 0;
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
