<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { Crepe } from '@milkdown/crepe'
import '@milkdown/crepe/theme/common/style.css'
import '@milkdown/crepe/theme/frame.css'
import { editorViewCtx } from '@milkdown/kit/core'
import { TextSelection, type EditorState } from '@milkdown/kit/prose/state'
import { Tag as TagIcon, Trash2 } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { attachBlockDrag } from '../utils/blockDrag'
import { deriveNoteTitle } from '../utils/markdown'
import { getQuickEmojis } from '../utils/emoji'
import { saveNoteImageFile } from '../utils/noteImage'
import { parseTimestamp } from '../utils/time'
import EmojiPicker from './EmojiPicker.vue'

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
          // 把手悬停偏移 16→4px：配合收窄后的编辑区左右内边距（72px），
          // 保证把手（66px 宽 + offset）完整落在边距内，不翻转盖字、不触发横向滚动
          blockHandle: {
            getOffset: () => 4,
          },
          // 斜杠菜单扩展：在「插入」右侧新增「表情」分组
          // 快捷表情（最近使用优先）点击即插入；「更多表情…」打开完整选择器（分类/搜索/最近使用）
          buildMenu: (builder) => {
            const group = builder.addGroup('emoji', '表情')
            getQuickEmojis().forEach((it) => {
              group.addItem(`emoji-${it.e}`, {
                label: it.n,
                icon: it.e,
                onRun: () => insertEmojiText(it.e),
              })
            })
            group.addItem('emoji-more', {
              label: '更多表情…',
              icon: emojiMoreIcon,
              onRun: () => {
                removeSlashQuery()
                emojiPickerVisible.value = true
              },
            })
          },
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

// ---- 表情插入 ----
const emojiPickerVisible = ref(false)

/** 斜杠菜单「更多表情…」的图标（Material mood 风格，fill 型，与 Crepe 自带图标一致） */
const emojiMoreIcon = `
  <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
    <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm3.5-9c.83 0 1.5-.67 1.5-1.5S16.33 8 15.5 8 14 8.67 14 9.5s.67 1.5 1.5 1.5zm-7 0c.83 0 1.5-.67 1.5-1.5S9.33 8 8.5 8 7 8.67 7 9.5 7.67 11 8.5 11zm3.5 6.5c2.33 0 4.31-1.46 5.11-3.5H6.89c.8 2.04 2.78 3.5 5.11 3.5z"/>
  </svg>
`

/** 光标前同一文本块内斜杠指令「/query」的起点；光标不在指令后则返回 null。
 *  斜杠菜单的自定义项（表情）不会像内置项那样清掉指令文本（内置项走 clearTextInCurrentBlock），
 *  插入表情/打开表情选择器前需先定位并删除它 */
function slashQueryStart(state: EditorState, from: number): number | null {
  const $from = state.doc.resolve(from)
  if ($from.depth === 0 || !$from.parent.inlineContent) return null
  const blockStart = $from.start()
  // leafText 占位符保证内联叶子节点（硬换行等）也是 1 字符，字符串下标与文档位置 1:1 对应
  const textBefore = state.doc.textBetween(blockStart, from, '\n', '\uFFFC')
  const m = /(?:^|\s)(\/[^\s]*)$/.exec(textBefore)
  if (!m) return null
  // m[0] 含可选前导（行首零宽或一个空白），指令起点 = 匹配起点 + 前导长度，
  // 即 m.index + (m[0].length - m[1].length)。直接用 m.index + m[1].length
  // 会落在指令中间甚至指令之外，导致替换/删除后斜杠指令残留。
  return blockStart + m.index + (m[0].length - m[1].length)
}

/** 删除光标前的斜杠指令文本（「更多表情…」入口用：先清指令再开选择器，取消选择也不留残渣） */
function removeSlashQuery() {
  const c = crepe
  if (!c) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const start = slashQueryStart(state, state.selection.from)
    if (start == null) return
    view.dispatch(state.tr.delete(start, state.selection.to))
  })
}

/** 在光标处插入文本（表情即纯文本，走 ProseMirror 事务，一次撤销步骤）；
 *  光标停在斜杠指令后时连指令一起替换，斜杠不残留（与其他菜单项行为一致） */
function insertEmojiText(text: string) {
  const c = crepe
  if (!c) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const { from, to } = state.selection
    const start = slashQueryStart(state, from) ?? from
    const tr = state.tr.insertText(text, start, to)
    view.dispatch(tr)
    view.focus()
  })
}

function onPickEmoji(e: string) {
  insertEmojiText(e)
}

// ---- 点击编辑区任意位置聚焦（需求：点空白处把光标落到文末，点在内容上则原地定位） ----
/** 编辑区内的浮层/专属交互元素：块把手、斜杠菜单、工具栏、链接气泡、图片块、间隙光标。
 *  命中这些时让位给各自逻辑，不抢焦点。 */
const EDITOR_FLOAT_UI_SELECTOR =
  '.milkdown-block-handle, .milkdown-slash-menu, .milkdown-toolbar, .milkdown-link-edit, ' +
  '.milkdown-link-preview, .milkdown-image-block, .crepe-image-block, .ProseMirror-gapcursor'

function onEditorAreaMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  const root = rootEl.value
  if (!root) return
  const target = e.target
  if (!(target instanceof Element)) return
  const pm = root.querySelector('.ProseMirror')
  if (!pm) return
  // 点击落在可编辑内容（含其内边距）上：ProseMirror 原生把光标定位到最近可输入点，不干预
  if (pm.contains(target)) return
  if (target.closest(EDITOR_FLOAT_UI_SELECTOR)) return
  // .milkdown 自身且命中滚动条区域（右缘/下缘）：是在拖滚动条，不动焦点
  if (target.classList.contains('milkdown')) {
    const el = target as HTMLElement
    if (e.offsetX >= el.clientWidth || e.offsetY >= el.clientHeight) return
  }
  // 空白/非可编辑区：阻止默认失焦，把光标送到全文最后一个可输入点（文档末尾）
  e.preventDefault()
  crepe?.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    view.dispatch(view.state.tr.setSelection(TextSelection.atEnd(view.state.doc)).scrollIntoView())
    view.focus()
  })
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

      <div ref="rootEl" class="crepe-root" @mousedown.capture="onEditorAreaMouseDown"></div>

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

    <EmojiPicker :visible="emojiPickerVisible" @select="onPickEmoji" @close="emojiPickerVisible = false" />
  </div>
</template>

<style scoped>
.editor-panel {
  height: 100%;
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 12px 16px 10px;
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

/* 编辑区内边距：Crepe 默认 padding 60px 120px 过大（文字距卡片边缘 145px），收紧为上下 20 / 左右 72。
   左右 72px 是块把手悬停空间（把手 66px 宽 + offset 4px，见 featureConfigs 的 blockHandle.getOffset），
   再小会导致把手翻转盖住文字或触发横向滚动 */
.crepe-root .milkdown .ProseMirror {
  padding: 20px 72px;
}

/* 把手容器默认 margin 0 10px：随边距收窄一并去掉，保证把手完整落在边距内 */
.crepe-root .milkdown .milkdown-block-handle {
  margin: 0;
}

/* 斜杠菜单「表情」分组：emoji 字符作为图标（icon 字段），默认 16px 偏小，放大一档；
   svg 图标（更多表情…）尺寸由 CSS width/height 固定，不受 font-size 影响 */
.crepe-root .milkdown-slash-menu .menu-group .milkdown-icon {
  font-size: 20px;
  line-height: 1;
}

/* 引用块：Crepe 默认 padding-left 40px，文字离左侧引用条太远，收紧到贴条显示 */
.crepe-root .milkdown .ProseMirror blockquote {
  padding-left: 12px;
}
</style>
