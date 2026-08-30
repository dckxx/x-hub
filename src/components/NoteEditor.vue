<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { Crepe } from '@milkdown/crepe'
import '@milkdown/crepe/theme/common/style.css'
import '@milkdown/crepe/theme/frame.css'
import { editorViewCtx } from '@milkdown/kit/core'
import { Tag as TagIcon, Trash2 } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { attachBlockDrag } from '../utils/blockDrag'
import { deriveNoteTitle } from '../utils/markdown'
import { saveNoteImageFile } from '../utils/noteImage'
import { parseTimestamp } from '../utils/time'

/**
 * 速记编辑器：Milkdown Crepe 所见即所得（Markdown 为真相源，序列化结果经 600ms 防抖落盘）。
 * 图片（粘贴/拖拽/点击上传）统一由 Crepe 的上传管线处理：plugin-upload 的 handlePaste/
 * handleDrop + ImageBlock 的 onUpload 配置 → saveNoteImageFile（notes/images + xhub-note 协议）。
 * 注意勿再自建 DOM paste 监听——plugin-upload 已处理粘贴，叠加监听会导致图片重复插入。
 * 块拖拽（六点把手）为指针实现（utils/blockDrag.ts），绕开 Tauri 原生拖放对 HTML5 DnD 的拦截。
 */

const props = defineProps<{
  note: Readonly<Note> | null
}>()

const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
}>()

const store = useStore()

const rootEl = ref<HTMLDivElement>()

let crepe: Crepe | null = null
let mounting = false
/** 挂载期间又切换了笔记：完成后需按最新笔记重挂一次（否则编辑器停留旧内容、防抖保存会跨笔记污染） */
let remountQueued = false
let detachBlockDrag: (() => void) | null = null

const localTitle = ref('')
const localContent = ref('')
const dirty = ref(false)

// ---- 生命周期 ----
onBeforeUnmount(() => {
  flushPendingSave()
  void destroyEditor()
})

async function destroyEditor() {
  detachBlockDrag?.()
  detachBlockDrag = null
  const c = crepe
  crepe = null
  if (c) {
    try {
      await c.destroy()
    } catch (e) {
      console.warn('Crepe 销毁异常', e)
    }
  }
  if (rootEl.value) rootEl.value.innerHTML = ''
}

async function mountEditor(content: string) {
  if (!rootEl.value) return
  if (mounting) {
    // Crepe 异步初始化期间再次切换笔记时不能静默吞掉挂载请求——记下重挂，本次完成后按最新笔记重来
    remountQueued = true
    return
  }
  mounting = true
  const wantId = props.note?.id ?? null
  try {
    await destroyEditor()
    const c = new Crepe({
      root: rootEl.value,
      defaultValue: content,
      // AI 特性需外部模型服务，保持纯本地
      features: { [Crepe.Feature.AI]: false },
      featureConfigs: {
        [Crepe.Feature.ImageBlock]: {
          onUpload: saveNoteImageFile,
          inlineOnUpload: saveNoteImageFile,
          blockOnUpload: saveNoteImageFile,
          // Crepe 默认文案为英文，以下统一汉化
          inlineUploadButton: '上传',
          inlineUploadPlaceholderText: '或粘贴图片链接',
          blockUploadButton: '上传图片',
          blockUploadPlaceholderText: '或粘贴图片链接',
          blockConfirmButton: '确认',
          blockCaptionPlaceholderText: '填写图片说明',
        },
        [Crepe.Feature.BlockEdit]: {
          textGroup: {
            label: '文本',
            text: { label: '正文' },
            h1: { label: '一级标题' },
            h2: { label: '二级标题' },
            h3: { label: '三级标题' },
            h4: { label: '四级标题' },
            h5: { label: '五级标题' },
            h6: { label: '六级标题' },
            quote: { label: '引用' },
            divider: { label: '分割线' },
          },
          listGroup: {
            label: '列表',
            bulletList: { label: '无序列表' },
            orderedList: { label: '有序列表' },
            taskList: { label: '任务列表' },
          },
          advancedGroup: {
            label: '插入',
            image: { label: '图片' },
            codeBlock: { label: '代码块' },
            table: { label: '表格' },
            math: { label: '公式' },
          },
        },
        [Crepe.Feature.Placeholder]: {
          text: '开始记录…',
        },
        [Crepe.Feature.LinkTooltip]: {
          inputPlaceholder: '粘贴链接…',
        },
        [Crepe.Feature.Toolbar]: {
          boldLabel: '加粗',
          italicLabel: '斜体',
          strikethroughLabel: '删除线',
          codeLabel: '行内代码',
          linkLabel: '链接',
          latexLabel: '公式',
        },
      },
    })
    await c.create()
    if (!rootEl.value || (props.note?.id ?? null) !== wantId) {
      // create 期间笔记已切换/组件已卸载：本次实例作废，队列重挂最新笔记（不挂监听、不接管拖拽）
      remountQueued = true
      try {
        await c.destroy()
      } catch {
        /* 丢弃的实例，销毁异常无需处理 */
      }
      return
    }
    // create 之后再挂监听，避免初始化本身触发一次 markdownUpdated 造成假保存
    c.on((listener) => {
      listener.markdownUpdated((_ctx, markdown) => {
        onEdited(markdown)
      })
    })
    crepe = c
    // 块拖拽（六点把手）指针实现：create 完成后从 ctx 取 EditorView 接管把手拖拽
    c.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      detachBlockDrag = attachBlockDrag(() => view)
    })
  } catch (e) {
    console.error('Crepe 初始化失败', e)
  } finally {
    mounting = false
    if (remountQueued && props.note && rootEl.value) {
      remountQueued = false
      void mountEditor(props.note.content ?? '')
    }
  }
}

// ---- 笔记切换 / 防抖保存 ----
// 定时器与标签状态声明必须在 watch 之前：immediate 回调在 setup 阶段同步执行，后置声明会触发 TDZ
let saveTimer: ReturnType<typeof setTimeout> | null = null
let lastNoteId: number | null = null
const noteTags = ref<Tag[]>([])
const tagInputVisible = ref(false)
const tagInput = ref('')

// 同时观察 rootEl：immediate 在 setup 阶段触发时模板尚未渲染（rootEl 为空），
// flush:'post' 保证组件渲染出编辑器容器后再执行挂载
watch(
  [() => props.note?.id, rootEl],
  async ([, el]) => {
    if (!props.note || !el) {
      flushPendingSave()
      void destroyEditor()
      syncLocal()
      noteTags.value = []
      return
    }
    syncLocal()
    void mountEditor(props.note.content ?? '')
    // 加载笔记标签
    if (isTauri()) {
      noteTags.value = await tauriApi.getNoteTags(props.note.id)
    } else {
      noteTags.value = []
    }
  },
  { immediate: true, flush: 'post' },
)

function syncLocal() {
  localTitle.value = props.note?.title ?? ''
  localContent.value = props.note?.content ?? ''
  dirty.value = false
}

/** 立即落盘防抖中未保存的编辑（若存在），并取消挂起的定时器 */
function flushPendingSave() {
  if (!saveTimer) return
  clearTimeout(saveTimer)
  saveTimer = null
  if (dirty.value && lastNoteId !== null) {
    dirty.value = false
    emit('save', lastNoteId, localTitle.value, localContent.value)
  }
}

function onEdited(markdown: string) {
  if (!props.note) return
  localContent.value = markdown
  // 标题还是默认值时，从正文首行（标题行/前几个字）自动派生；用户一旦改过标题即不再接管
  if (localTitle.value === '' || localTitle.value === '无标题笔记') {
    const derived = deriveNoteTitle(markdown)
    if (derived) localTitle.value = derived
  }
  scheduleSave()
}

function scheduleSave() {
  if (!props.note) return
  dirty.value = true
  if (saveTimer) clearTimeout(saveTimer)
  lastNoteId = props.note.id
  saveTimer = setTimeout(() => {
    saveTimer = null
    if (props.note) {
      emit('save', props.note.id, localTitle.value, localContent.value)
    }
  }, 600)
}

watch(
  () => props.note?.updated_at,
  () => {
    dirty.value = false
  },
)

// ---- 标签 ----
async function persistTags() {
  if (!props.note || !isTauri()) return
  await tauriApi.setNoteTags(props.note.id, noteTags.value.map((t) => t.id))
}

async function addTag(tag: Tag) {
  if (noteTags.value.some((t) => t.id === tag.id)) return
  noteTags.value.push(tag)
  await persistTags()
}

function removeTag(tagId: number) {
  noteTags.value = noteTags.value.filter((t) => t.id !== tagId)
  void persistTags()
}

async function submitTagInput() {
  const name = tagInput.value.trim()
  if (!name) {
    tagInputVisible.value = false
    return
  }
  try {
    const t = await store.createTag(name)
    await addTag(t)
    tagInput.value = ''
  } catch (e) {
    console.error('创建标签失败', e)
  }
  tagInputVisible.value = false
}

function formatSavedTime(iso: string): string {
  const t = new Date(parseTimestamp(iso))
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${t.getFullYear()}年${t.getMonth() + 1}月${t.getDate()}日${pad(t.getHours())}:${pad(t.getMinutes())}:${pad(t.getSeconds())}`
}
</script>

<template>
  <div class="card editor-panel">
    <!-- 空状态 -->
    <div v-if="!note" class="editor-empty">
      <p>选择或新建笔记</p>
    </div>

    <!-- 编辑器内容 -->
    <template v-else>
      <header class="ed-header">
        <input
          v-model="localTitle"
          class="ed-title-input"
          type="text"
          maxlength="80"
          placeholder="笔记标题"
          @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
        />
        <button
          class="icon-btn del"
          title="删除笔记"
          aria-label="删除笔记"
          @click="emit('delete', note.id)"
        >
          <Trash2 :size="14" :stroke-width="1.8" />
        </button>
      </header>

      <div ref="rootEl" class="crepe-root"></div>

      <!-- 底栏：左标签行 + 右保存状态 -->
      <footer class="ed-footer">
        <div class="tag-row">
          <TagIcon :size="13" :stroke-width="1.8" class="tag-row-icon" />
          <span
            v-for="t in noteTags"
            :key="t.id"
            class="tag-chip"
          >
            {{ t.name }}
            <button
              class="tag-chip-x"
              type="button"
              :title="`移除标签「${t.name}」`"
              :aria-label="`移除标签「${t.name}」`"
              @click="removeTag(t.id)"
            >
              ✕
            </button>
          </span>
          <template v-if="tagInputVisible">
            <input
              v-model="tagInput"
              class="tag-input"
              type="text"
              maxlength="20"
              placeholder="标签名，回车确认"
              @keydown.enter.prevent="submitTagInput"
              @keydown.esc="tagInputVisible = false"
            />
          </template>
          <button
            v-else
            class="tag-add"
            title="添加标签"
            aria-label="添加标签"
            @click="tagInputVisible = true"
          >
            +
          </button>
        </div>

        <span class="ed-status" :class="{ dirty }">
          {{ dirty ? '编辑中…' : `已保存 ${formatSavedTime(note.updated_at)}` }}
        </span>
      </footer>
    </template>
  </div>
</template>

<style scoped>
.editor-panel {
  height: 100%;
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 16px 24px 12px;
  overflow: hidden;
  /* 速记模块字号：全局基准 × 模块系数 */
  font-size: calc(1rem * var(--fs-notes, 1));
}

.editor-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-3);
  font-size: 0.875em;
}

.ed-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.ed-title-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  font-size: 1em;
  font-weight: 600;
  font-family: inherit;
  color: var(--text-1);
  outline: none;
  padding: 8px 14px;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.ed-title-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}

.ed-title-input::placeholder {
  color: var(--text-4);
  font-weight: 400;
}

.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

.crepe-root {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
}

.crepe-root :deep(.milkdown) {
  height: 100%;
  overflow-y: auto;
}

.ed-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-top: 6px;
  border-top: 1px solid var(--border-soft);
}

/* 标签行 */
.tag-row {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

.tag-row-icon {
  color: var(--text-4);
  flex-shrink: 0;
}

.tag-chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: 0.6875em;
  font-weight: 500;
  color: var(--brand-500);
  background: var(--brand-50);
  border-radius: var(--radius-pill);
  padding: 3px 6px 3px 9px;
}

.tag-chip-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  font-size: calc(0.625rem * var(--fs-notes, 1));
  line-height: 1;
  padding: 0;
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}

.tag-chip-x:hover {
  background: color-mix(in srgb, var(--c-red) 14%, transparent);
  color: var(--c-red);
}

.tag-add {
  width: 26px;
  height: 26px;
  border: 1px dashed var(--text-4);
  background: transparent;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 0.8125em;
  cursor: pointer;
  transition: border-color 0.12s, color 0.12s;
}

.tag-add:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}

.tag-input {
  width: 130px;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-sm);
  color: var(--text-1);
  font-size: 0.75em;
  font-family: inherit;
  padding: 4px 10px;
  outline: none;
}

.tag-input:focus {
  border-color: var(--brand-500);
}

.ed-status {
  flex-shrink: 0;
  font-size: 0.75em;
  color: var(--text-3);
}

.ed-status.dirty {
  color: var(--brand-500);
}
</style>

<style>
/* Crepe 主题变量对齐应用设计令牌（全局块：高优先级选择器压过 frame.css 的 .milkdown 定义）。
   亮色基线 + [data-theme="dark"] 暗色覆盖，替代 Crepe 缺失的动态主题切换（Milkdown #1839） */
.crepe-root .milkdown {
  --crepe-base-font-size: calc(15px * var(--fs-notes, 1));
  --crepe-color-background: transparent;
  --crepe-color-on-background: var(--text-1);
  --crepe-color-surface: var(--input-bg);
  --crepe-color-surface-low: var(--input-bg);
  --crepe-color-on-surface: var(--text-2);
  --crepe-color-on-surface-variant: var(--text-3);
  /* outline 同时承担图标色（工具栏/块把手/链接气泡）与发丝线：必须用中性灰墨，
     不能映射 --border-soft（亮色为 55% 白，白图标叠白底工具栏不可见） */
  --crepe-color-outline: var(--text-3);
  --crepe-color-primary: var(--text-1);
  --crepe-color-inverse: var(--bg-card);
  --crepe-color-on-inverse: var(--text-2);
  --crepe-color-inline-code: var(--brand-500);
}

[data-theme='dark'] .crepe-root .milkdown {
  --crepe-color-secondary: #4d4d4d;
  --crepe-color-on-secondary: #d6d6d6;
  --crepe-color-hover: #232323;
  --crepe-color-selected: #2f2f2f;
  --crepe-color-inline-area: #2b2b2b;
}

/* 透底态（壁纸+透明，白墨形态）：工具栏/斜杠菜单/链接气泡/图片说明换深玻璃实底。
   浮层底原为 30% 烟玻璃（--input-bg），叠在亮部照片上时白图标（--text-1/--text-3 均翻白）会糊掉，
   这里收成近实底深玻璃 + 白系图标，与白墨态 toast/对话面板同一处理手法 */
html[data-wallpaper-clear='1'] .crepe-root .milkdown {
  --crepe-color-surface: rgba(28, 29, 41, 0.92);
  --crepe-color-surface-low: rgba(28, 29, 41, 0.92);
  --crepe-color-on-surface: rgba(255, 255, 255, 0.92);
  --crepe-color-on-surface-variant: rgba(255, 255, 255, 0.74);
  --crepe-color-outline: rgba(255, 255, 255, 0.62);
  --crepe-color-primary: #ffffff;
  --crepe-color-secondary: rgba(255, 255, 255, 0.14);
  --crepe-color-on-secondary: rgba(255, 255, 255, 0.92);
  --crepe-color-inverse: rgba(28, 29, 41, 0.95);
  --crepe-color-on-inverse: rgba(255, 255, 255, 0.92);
  --crepe-color-hover: rgba(255, 255, 255, 0.14);
  --crepe-color-selected: rgba(255, 255, 255, 0.22);
}

/* 引用块：Crepe 默认 padding-left 40px，文字离左侧引用条太远，收紧到贴条显示 */
.crepe-root .milkdown .ProseMirror blockquote {
  padding-left: 12px;
}
</style>
