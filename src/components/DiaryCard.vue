<script setup lang="ts">
import { inject, nextTick, ref, watch } from 'vue'
import { NotebookPen } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'

const props = defineProps<{ title?: string; hideTitle?: boolean }>()

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
)

const DIARY_FOLDER_NAME = '日记'
const draft = ref('')
const saving = ref(false)
const inputEl = ref<HTMLTextAreaElement | null>(null)

/** 标题 = 入库时刻的「年月日 时:分:秒」 */
function formatDiaryTitle(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

/** 单行起、随内容长高；+2 补 border-box 上下边框，避免差 2px 出滚动条 */
function autosize() {
  const el = inputEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight + 2}px`
}

watch(draft, () => autosize())

/** 回车保存；Shift+Enter 换行；IME 组合中的回车是选字确认，不触发保存 */
function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing) return
  e.preventDefault()
  void saveDiary()
}

/** 存入速记「日记」文件夹：树根按名查找、缺失即建（普通文件夹，改删自由）；
 *  成功 = 立马清空输入框；失败 = 保留原文 + 报错 toast */
async function saveDiary() {
  const content = draft.value
  if (saving.value || !content.trim()) return
  saving.value = true
  try {
    let folder = store.state.noteFolders.find(
      (f) => f.parent_id == null && f.name === DIARY_FOLDER_NAME,
    )
    if (!folder) folder = await store.createNoteFolder(DIARY_FOLDER_NAME, null)
    await store.addNote(formatDiaryTitle(new Date()), folder.id, content)
    draft.value = ''
    nextTick(autosize)
  } catch (e) {
    console.error('save diary failed:', e)
    showToast?.('日记保存失败，请重试')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="card diary-card" :aria-label="title ?? '日记'">
    <header v-if="!hideTitle" class="diary-header">
      <h3 class="diary-title">
        <NotebookPen :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '日记' }}</span>
      </h3>
    </header>
    <div class="diary-input-wrap">
      <textarea
        ref="inputEl"
        v-model="draft"
        class="diary-input"
        placeholder="记一笔今天的日记，回车保存"
        rows="1"
        spellcheck="false"
        @keydown="onKeydown"
      ></textarea>
    </div>
  </section>
</template>

<style scoped>
.diary-card {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 12px;
  /* 日记模块字号：全局基准 × 模块系数（与便签同款口径，设置未接入时回退 1） */
  font-size: calc(1rem * var(--fs-diary, 1));
}
.diary-header {
  display: flex;
  align-items: center;
  margin-bottom: 8px;
  flex-shrink: 0;
}
.diary-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
}
.diary-title :deep(svg) {
  color: var(--brand-500);
}
.diary-input-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
}
.diary-input {
  flex: 1;
  width: 100%;
  min-height: 0;
  /* 长高到卡片上限后转内部滚动 */
  max-height: 100%;
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
  overflow-y: auto;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
}
.diary-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: color-mix(in srgb, var(--input-bg) 88%, #fff);
}
.diary-input::placeholder {
  color: var(--text-4);
}
</style>
