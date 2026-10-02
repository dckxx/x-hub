<script setup lang="ts">
// 速达小类管理（ADR 0012）：按大类分组的小类库管理——行内改名、拖拽排序、设默认、删除
// （删除时条目自动改挂默认小类，后端单事务）、批量删除。速达页只放小类选择器，管理集中在这里。
// 小类名支持「/」层级（书签导入的目录树），行内按「祖先 / 叶名」展示。
import { computed, inject, onBeforeUnmount, ref } from 'vue'
import { Check, GripVertical, ListChecks, Plus, Star, Trash2, X } from 'lucide-vue-next'
import { useStore } from '../../stores/workbench'
import type { ResourceSubcategory } from '../../api/tauri'
import { reportClientError } from '../../utils/error-report'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const KINDS: { kind: 'app' | 'web' | 'file'; label: string; hint: string }[] = [
  { kind: 'app', label: '应用', hint: '扫描/手动添加的程序' },
  { kind: 'web', label: '网页', hint: '网址收藏（书签导入的小类带 / 层级）' },
  { kind: 'file', label: '文件', hint: '已内置 7 个初始小类' },
]

/** 每大类一条新增输入框的草稿 */
const drafts = ref<Record<string, string>>({})
const newName = (kind: string) => drafts.value[kind] ?? ''
function setNewName(kind: string, v: string) {
  drafts.value = { ...drafts.value, [kind]: v }
}

const error = ref('')

async function add(kind: 'app' | 'web' | 'file') {
  const name = newName(kind).trim()
  if (!name) return
  try {
    await store.addSubcategory(kind, name)
    setNewName(kind, '')
    error.value = ''
  } catch (e) {
    error.value = String(e).replace(/^DUP:/, '').trim() || String(e)
  }
}

/** 行内改名：blur/回车提交；未变化或为空则还原 */
async function rename(sub: { id: number; name: string }, ev: Event) {
  const el = ev.target as HTMLInputElement
  const next = el.value.trim()
  if (!next || next === sub.name) {
    el.value = sub.name
    return
  }
  try {
    await store.editSubcategory(sub.id, next)
    error.value = ''
  } catch (e) {
    el.value = sub.name
    error.value = String(e).replace(/^DUP:/, '').trim() || String(e)
  }
}

/** 两段式删除确认：第一次点击把删除图标变成「√ 勾」，3s 内再点才真删（再点勾 = 确认） */
const confirmDeleteId = ref<number | null>(null)
let confirmTimer = 0
function onDeleteClick(id: number) {
  if (confirmDeleteId.value === id) {
    confirmDeleteId.value = null
    window.clearTimeout(confirmTimer)
    store
      .removeSubcategory(id)
      .then(() => (error.value = ''))
      .catch((e) => (error.value = String(e)))
    return
  }
  confirmDeleteId.value = id
  window.clearTimeout(confirmTimer)
  confirmTimer = window.setTimeout(() => (confirmDeleteId.value = null), 3000)
}

async function onSetDefault(id: number) {
  try {
    await store.setDefaultSubcategory(id)
    error.value = ''
  } catch (e) {
    error.value = String(e)
  }
}

// ---- 批量删除：按大类进入勾选模式（勾选 + 全选 + 删除所选，删除按钮同样两段式确认） ----
const batchKind = ref<'app' | 'web' | 'file' | null>(null)
const batchChecked = ref<Set<number>>(new Set())
const batchDeleting = ref(false)
/** 批量删除的两段式确认态（true = 已点一次，按钮变勾） */
const batchConfirmArmed = ref(false)
let batchConfirmTimer = 0

function enterBatch(kind: 'app' | 'web' | 'file') {
  batchKind.value = kind
  batchChecked.value = new Set()
  batchConfirmArmed.value = false
}

function exitBatch() {
  batchKind.value = null
  batchChecked.value = new Set()
  batchConfirmArmed.value = false
  window.clearTimeout(batchConfirmTimer)
}

function toggleBatch(id: number) {
  const next = new Set(batchChecked.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  batchChecked.value = next
  batchConfirmArmed.value = false
}

/** 当前大类在勾选模式下的可见行（用于全选判定） */
function batchRows(kind: 'app' | 'web' | 'file'): ResourceSubcategory[] {
  const ids = displayIds(kind)
  const map = new Map(store.subcategoriesOf(kind).map((s) => [s.id, s]))
  return ids.map((id) => map.get(id)).filter((s): s is ResourceSubcategory => !!s)
}

function batchToggleAll(kind: 'app' | 'web' | 'file') {
  const rows = batchRows(kind)
  const all = rows.length > 0 && rows.every((s) => batchChecked.value.has(s.id))
  batchChecked.value = all ? new Set() : new Set(rows.map((s) => s.id))
  batchConfirmArmed.value = false
}

/** 删除所选：两段式（第一次点变勾，再点执行）。逐条删除（每条后端单事务、条目改挂默认） */
function onBatchDeleteClick() {
  if (batchDeleting.value) return
  if (batchConfirmArmed.value) {
    batchConfirmArmed.value = false
    window.clearTimeout(batchConfirmTimer)
    void doBatchDelete()
    return
  }
  batchConfirmArmed.value = true
  window.clearTimeout(batchConfirmTimer)
  batchConfirmTimer = window.setTimeout(() => (batchConfirmArmed.value = false), 3000)
}

async function doBatchDelete() {
  const kind = batchKind.value
  if (!kind) return
  // 先落快照：删除过程会改 store 列表，边删边读集合会漏
  const snapshot = batchRows(kind).filter((s) => batchChecked.value.has(s.id))
  if (snapshot.length === 0) return
  batchDeleting.value = true
  let done = 0
  const failed: string[] = []
  for (const s of snapshot) {
    try {
      await store.removeSubcategory(s.id)
      done++
    } catch (e) {
      failed.push(s.name)
      void reportClientError('批量删除小类失败', e)
    }
  }
  batchDeleting.value = false
  batchChecked.value = new Set()
  const parts = [`已删除 ${done} 个小类（条目已改挂默认小类）`]
  if (failed.length > 0) parts.push(`${failed.length} 个删除失败：${failed.join('、')}`)
  showToast(parts.join('，'))
  if (failed.length > 0) error.value = `批量删除部分失败：${failed.join('、')}`
}

// ---- 拖拽排序（指针实现；主窗口原生拖放拦截与 HTML5 DnD 互斥，见约定 14） ----
const dragKind = ref<'app' | 'web' | 'file' | null>(null)
/** 拖拽中的本地预览顺序（仅拖拽的那个大类），提交后清空回读 store */
const preview = ref<Record<string, number[]>>({})
let dragFromIndex = 0

function displayIds(kind: 'app' | 'web' | 'file'): number[] {
  const override = preview.value[kind]
  if (override) return override
  return store.subcategoriesOf(kind).map((s) => s.id)
}

function onHandleDown(e: PointerEvent, kind: 'app' | 'web' | 'file', index: number) {
  // 勾选模式下不排序（手势与点选冲突）
  if (batchKind.value === kind) return
  e.preventDefault()
  dragKind.value = kind
  dragFromIndex = index
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragUp)
}

function onDragMove(e: PointerEvent) {
  const kind = dragKind.value
  if (!kind) return
  const el = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-sub-row]')
  if (!(el instanceof HTMLElement)) return
  if (el.dataset.kind !== kind) return
  const to = Number(el.dataset.index)
  const ids = displayIds(kind)
  if (Number.isNaN(to) || to === dragFromIndex || to < 0 || to >= ids.length) return
  const next = [...ids]
  next.splice(dragFromIndex, 1)
  next.splice(to, 0, ids[dragFromIndex])
  dragFromIndex = to
  preview.value = { ...preview.value, [kind]: next }
}

function onDragUp() {
  const kind = dragKind.value
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  dragKind.value = null
  if (!kind) return
  const ids = displayIds(kind)
  preview.value = {}
  store
    .reorderSubcategories(kind, ids)
    .then(() => (error.value = ''))
    .catch((e) => (error.value = String(e)))
}

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  window.clearTimeout(confirmTimer)
  window.clearTimeout(batchConfirmTimer)
})

/** 展示列表：预览序优先（仅拖拽中的那个大类），其余按 store 排序 */
const groups = computed(() =>
  KINDS.map(({ kind, label, hint }) => {
    const ids = displayIds(kind)
    const map = new Map(store.subcategoriesOf(kind).map((s) => [s.id, s]))
    const list = ids.map((id) => map.get(id)).filter((s): s is ResourceSubcategory => !!s)
    return { kind, label, hint, list }
  }),
)
</script>

<template>
  <div class="sub-mgr">
    <p v-if="error" class="sub-mgr-error">{{ error }}</p>
    <div v-for="g in groups" :key="g.kind" class="sub-mgr-group">
      <div class="sub-mgr-head">
        <span class="sub-mgr-title">{{ g.label }}</span>
        <span v-if="batchKind !== g.kind" class="sub-mgr-hint">{{ g.hint }}</span>
        <!-- 勾选模式工具条：已选计数 + 全选 + 两段式删除 + 退出 -->
        <template v-else>
          <span class="sub-mgr-hint">已选 {{ batchChecked.size }} 项</span>
          <button class="ghost-btn sub-mgr-batch-btn" type="button" @click="batchToggleAll(g.kind)">
            {{ batchRows(g.kind).length > 0 && batchRows(g.kind).every((s) => batchChecked.has(s.id)) ? '全不选' : '全选' }}
          </button>
          <button
            class="ghost-btn sub-mgr-batch-btn danger"
            :class="{ armed: batchConfirmArmed }"
            type="button"
            :disabled="batchChecked.size === 0 || batchDeleting"
            :title="batchConfirmArmed ? '再点一次确认删除所选' : '删除所选小类（条目改挂默认小类）'"
            @click="onBatchDeleteClick"
          >
            <Check v-if="batchConfirmArmed" :size="12" :stroke-width="2.6" />
            {{ batchDeleting ? '删除中…' : batchConfirmArmed ? '确认删除' : `删除所选（${batchChecked.size}）` }}
          </button>
          <button class="sub-mgr-btn" type="button" title="退出批量删除" aria-label="退出批量删除" @click="exitBatch">
            <X :size="13" :stroke-width="2.2" />
          </button>
        </template>
        <button
          v-if="batchKind !== g.kind && g.list.length > 0"
          class="ghost-btn sub-mgr-batch-btn"
          type="button"
          title="勾选多个小类后一并删除"
          @click="enterBatch(g.kind)"
        >
          <ListChecks :size="12" :stroke-width="2" />
          批量删除
        </button>
      </div>
      <div
        v-for="(s, i) in g.list"
        :key="s.id"
        class="sub-mgr-row"
        data-sub-row
        :data-kind="g.kind"
        :data-index="i"
      >
        <template v-if="batchKind === g.kind">
          <button
            class="sub-mgr-check"
            :class="{ on: batchChecked.has(s.id) }"
            type="button"
            role="checkbox"
            :aria-checked="batchChecked.has(s.id)"
            :aria-label="`选择小类 ${s.name}`"
            @click="toggleBatch(s.id)"
          >
            <Check v-if="batchChecked.has(s.id)" :size="11" :stroke-width="3" />
          </button>
        </template>
        <span
          v-else
          class="sub-mgr-drag"
          title="拖拽排序"
          @pointerdown="onHandleDown($event, g.kind, i)"
        >
          <GripVertical :size="13" :stroke-width="2" />
        </span>
        <input
          class="sub-mgr-name"
          :value="s.name"
          spellcheck="false"
          maxlength="60"
          :title="s.name"
          @change="rename(s, $event)"
          @keydown.enter="($event.target as HTMLInputElement).blur()"
        />
        <button
          class="sub-mgr-btn"
          :class="{ 'is-default': s.is_default }"
          type="button"
          :title="s.is_default ? '默认小类：新建资源未指定时自动归入（点击换默认）' : '设为默认小类'"
          @click="onSetDefault(s.id)"
        >
          <Star :size="13" :stroke-width="2.2" />
        </button>
        <button
          class="sub-mgr-btn sub-mgr-del"
          :class="{ 'is-confirm': confirmDeleteId === s.id }"
          type="button"
          :title="
            confirmDeleteId === s.id
              ? '再点一次确认删除（条目将改挂默认小类）'
              : '删除小类（需点两下：第一下变 √ 确认，第二下删除；条目改挂默认小类）'
          "
          :aria-label="confirmDeleteId === s.id ? '确认删除小类' : '删除小类'"
          @click="onDeleteClick(s.id)"
        >
          <!-- 第一击把垃圾桶换成 √ 勾：让「再点一下是确认」有明确视觉信号 -->
          <Check v-if="confirmDeleteId === s.id" :size="13" :stroke-width="2.6" />
          <Trash2 v-else :size="13" :stroke-width="2.2" />
        </button>
      </div>
      <div v-if="batchKind !== g.kind" class="sub-mgr-row sub-mgr-add">
        <span class="sub-mgr-drag is-placeholder"><Plus :size="13" :stroke-width="2" /></span>
        <input
          class="sub-mgr-name"
          :value="newName(g.kind)"
          type="text"
          maxlength="60"
          placeholder="新增小类名称，可用 / 分层级（如 开发/前端），回车添加"
          spellcheck="false"
          @input="setNewName(g.kind, ($event.target as HTMLInputElement).value)"
          @keydown.enter="add(g.kind)"
        />
        <button class="sub-mgr-btn sub-mgr-add-btn" type="button" title="添加小类" @click="add(g.kind)">
          添加
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sub-mgr {
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin-top: 4px;
}

.sub-mgr-error {
  margin: 0;
  font-size: 12px;
  color: var(--c-red);
}

.sub-mgr-group {
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  padding: 10px 12px 12px;
  background: var(--bg-card-soft);
}

.sub-mgr-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.sub-mgr-title {
  font-size: 13px;
  font-weight: 650;
  color: var(--text-1);
}

.sub-mgr-hint {
  font-size: 11px;
  color: var(--text-3);
  margin-right: auto;
}

.sub-mgr-batch-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 11.5px;
  color: var(--text-2);
}

.sub-mgr-batch-btn.danger {
  color: var(--c-red);
}

.sub-mgr-batch-btn.danger.armed {
  background: var(--c-red);
  border-color: var(--c-red);
  color: var(--text-on-accent);
}

.sub-mgr-batch-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.sub-mgr-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 0;
}

.sub-mgr-drag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 22px;
  color: var(--text-4);
  cursor: grab;
  touch-action: none;
}

.sub-mgr-drag:active {
  cursor: grabbing;
}

.sub-mgr-drag.is-placeholder {
  cursor: default;
  opacity: 0.45;
}

/* 勾选模式的行首选择框（占拖拽把手位） */
.sub-mgr-check {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  margin: 0 2px;
  border: 1.5px solid var(--border-strong);
  border-radius: 5px;
  background: var(--bg-card-solid);
  color: var(--text-on-accent);
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.sub-mgr-check.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
}

.sub-mgr-name {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--text-1);
  font-size: 12.5px;
  outline: none;
  transition: border-color 0.15s, background 0.15s;
}

.sub-mgr-name:hover {
  background: var(--bg-card);
}

.sub-mgr-name:focus {
  border-color: var(--brand-500);
  background: var(--bg-card);
  box-shadow: 0 0 0 2px var(--brand-50);
}

.sub-mgr-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-4);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.sub-mgr-btn:hover {
  background: var(--bg-card);
  color: var(--text-2);
}

.sub-mgr-btn.is-default {
  color: var(--c-yellow);
}

.sub-mgr-del.is-confirm {
  background: var(--c-red);
  color: var(--text-on-accent);
}

.sub-mgr-add {
  margin-top: 4px;
}

.sub-mgr-add .sub-mgr-name {
  border-color: var(--border-soft);
  background: var(--bg-card);
}

.sub-mgr-add-btn {
  width: auto;
  padding: 0 10px;
  font-size: 11.5px;
  color: var(--text-2);
  border: 1px solid var(--border-soft);
}
</style>
