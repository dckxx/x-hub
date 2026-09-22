<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { Crepe } from '@milkdown/crepe'
import '@milkdown/crepe/theme/common/style.css'
import '@milkdown/crepe/theme/frame.css'
import { editorViewCtx } from '@milkdown/kit/core'
import type { Ctx } from '@milkdown/kit/ctx'
import { imageBlockSchema } from '@milkdown/kit/component/image-block'
import { codeBlockSchema } from '@milkdown/kit/preset/commonmark'
import { createTable } from '@milkdown/kit/preset/gfm'
import type { Node as ProseNode, Schema } from '@milkdown/kit/prose/model'
import type { EditorView } from '@milkdown/kit/prose/view'
import { TextSelection, type EditorState } from '@milkdown/kit/prose/state'
import { Tag as TagIcon, Trash2, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { attachBlockDrag } from '../utils/blockDrag'
import { expandOnEnter, matchWysiwygLine, type LineShortcut } from '../utils/markdownEnter'
import { renderNoteMarkdown } from '../utils/markdownHtml'
import { deriveNoteTitle } from '../utils/markdown'
import { NOTE_EDITOR_MODES, normalizeNoteEditorMode, type NoteEditorMode } from '../utils/noteEditorMode'
import { getQuickEmojis } from '../utils/emoji'
import { saveNoteImageFile } from '../utils/noteImage'
import { parseTimestamp } from '../utils/time'
import EmojiPicker from './EmojiPicker.vue'

/**
 * 速记编辑器：实时预览（Crepe 所见即所得）/ 分屏预览 / 源码。Markdown 为真相源，600ms 防抖落盘。
 * 模式记在配置 note_editor_mode，按用户记住，不按笔记。
 * 行尾回车补结构（三种模式同一套判断）：``` / $$ 闭合，未写完的图片补成 ![]()，
 * |列x行| 或表头行生成表格。实时预览里对应变成代码块、公式块、图片块和表格节点。
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

const mode = ref<NoteEditorMode>(normalizeNoteEditorMode(store.state.config.note_editor_mode))

const rootEl = ref<HTMLDivElement>()

let crepe: Crepe | null = null
let mounting = false
/** 挂载期间又切换了笔记：完成后需按最新笔记重挂一次（否则编辑器停留旧内容、防抖保存会跨笔记污染） */
let remountQueued = false
let detachBlockDrag: (() => void) | null = null

const localTitle = ref('')
const localContent = ref('')
const dirty = ref(false)
const previewHtml = computed(() => renderNoteMarkdown(localContent.value))

// ---- 生命周期 ----
onBeforeUnmount(() => {
  flushPendingSave()
  detachImageListeners()
  // 防御：卸载瞬间可能仍在拖拽/预览中
  window.removeEventListener('pointermove', onResizeMove)
  window.removeEventListener('pointerup', onResizeUp)
  resizeCtx = null
  window.removeEventListener('keydown', onPreviewKeydown)
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
    queuedMarkdown = content
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
    attachImageListeners()
    // 块拖拽（六点把手）指针实现：create 完成后从 ctx 取 EditorView 接管把手拖拽
    c.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      detachBlockDrag = attachBlockDrag(() => view)
    })
    // 已缓存的图片不会再次触发 load：挂载完成后先按 attrs 同步一次宽度
    syncImageWidths()
  } catch (e) {
    console.error('Crepe 初始化失败', e)
  } finally {
    mounting = false
    if (remountQueued && props.note && rootEl.value && mode.value === 'wysiwyg') {
      remountQueued = false
      const next = queuedMarkdown ?? content
      queuedMarkdown = null
      void mountEditor(next)
    } else {
      remountQueued = false
      queuedMarkdown = null
    }
  }
}

// ---- 笔记切换 / 防抖保存 ----
// 定时器与标签状态声明必须在 watch 之前：immediate 回调在 setup 阶段同步执行，后置声明会触发 TDZ
let saveTimer: ReturnType<typeof setTimeout> | null = null
let lastNoteId: number | null = null
/** 挂载进行中又收到新内容时，结束后按这份 Markdown 重挂，避免用过期的笔记正文 */
let queuedMarkdown: string | null = null
const noteTags = ref<Tag[]>([])
const tagInputVisible = ref(false)
const tagInput = ref('')

// flush:'post'：immediate 在 setup 阶段触发时模板尚未渲染，等编辑器容器出来再挂载
watch(
  () => props.note?.id,
  async (id, prev) => {
    // 回调触发时 props.note 已是新笔记，Crepe 里仍是上一篇。先取出上一篇正文再落盘，避免写到新笔记上。
    if (typeof prev === 'number' && prev !== id) {
      if (mode.value === 'wysiwyg') {
        const captured = captureCrepeMarkdown()
        if (captured != null && captured !== localContent.value) {
          localContent.value = captured
          deriveTitleFromContent(captured)
          dirty.value = true
        }
      }
      flushLeavingNote(prev)
    }
    if (!props.note) {
      await destroyEditor()
      syncLocal()
      noteTags.value = []
      return
    }
    syncLocal()
    if (mode.value === 'wysiwyg') {
      await nextTick()
      if (rootEl.value && (props.note?.id ?? null) === id) void mountEditor(localContent.value)
    } else {
      await destroyEditor()
    }
    if (!props.note || props.note.id !== id) return
    if (isTauri()) {
      noteTags.value = await tauriApi.getNoteTags(props.note.id)
    } else {
      noteTags.value = []
    }
  },
  { immediate: true, flush: 'post' },
)

watch(
  () => store.state.config.note_editor_mode,
  (value) => {
    const next = normalizeNoteEditorMode(value)
    if (value !== next) {
      void store.setNoteEditorMode(next)
      return
    }
    if (next !== mode.value) void applyMode(next, false)
  },
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
    emit('save', lastNoteId, normalizeTitle(localTitle.value), localContent.value)
  }
}

/** 空标题落库时归一为默认值，与新建笔记的初始标题一致（列表展示不出现空行） */
function normalizeTitle(title: string): string {
  const t = title.trim()
  return t ? t : '无标题笔记'
}

function onEdited(markdown: string) {
  if (!props.note || mode.value !== 'wysiwyg') return
  adoptMarkdown(markdown)
  // ratio 等图片属性可能经撤销/属性事务变化，同步一次宽度（幂等、无强制布局）
  syncImageWidths()
}

/** 把一份 Markdown 收进当前笔记。内容没变就不重新排保存。 */
function adoptMarkdown(markdown: string) {
  if (!props.note || markdown === localContent.value) return
  localContent.value = markdown
  deriveTitleFromContent(markdown)
  scheduleSave()
}

/** 标题还是默认值时，从正文首行自动派生；用户一旦改过标题即不再接管 */
function deriveTitleFromContent(markdown: string) {
  if (localTitle.value !== '' && localTitle.value !== '无标题笔记') return
  const derived = deriveNoteTitle(markdown)
  if (derived) localTitle.value = derived
}

function captureCrepeMarkdown(): string | null {
  const c = crepe
  if (!c) return null
  try {
    return c.getMarkdown()
  } catch (e) {
    console.warn('读取编辑器 Markdown 失败', e)
    return null
  }
}

/** 离开当前笔记时立刻落盘。id 用离开前的那篇，不能用已经换上来的 props.note。 */
function flushLeavingNote(id: number) {
  if (saveTimer) {
    clearTimeout(saveTimer)
    saveTimer = null
  }
  if (!dirty.value) return
  dirty.value = false
  emit('save', id, normalizeTitle(localTitle.value), localContent.value)
}

function onSourceInput(e: Event) {
  const value = (e.target as HTMLTextAreaElement).value
  adoptMarkdown(value)
}

/** 行尾回车补上代码块、公式、图片、表格。光标留在新结构里。 */
function onSourceKeydown(e: KeyboardEvent) {
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing || e.defaultPrevented) return
  const el = e.target
  if (!(el instanceof HTMLTextAreaElement) || el.selectionStart !== el.selectionEnd) return
  const expanded = expandOnEnter(el.value, el.selectionStart)
  if (!expanded) return
  e.preventDefault()
  adoptMarkdown(expanded.value)
  void nextTick(() => el.setSelectionRange(expanded.cursor, expanded.cursor))
}

/**
 * 实时预览里 Milkdown 要在标记后面加空格才变成对应节点，单独回车只是换行。
 * 捕获阶段接住回车，把整行快捷标记换成代码块、公式块、图片块或表格。
 */
function onCrepeKeydown(e: KeyboardEvent) {
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing || !crepe) return
  const target = e.target
  if (target instanceof HTMLElement && target.closest('input, textarea, .milkdown-slash-menu')) return
  let handled = false
  crepe.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const { $from, empty } = state.selection
    if (!empty) return
    const parent = $from.parent
    if (!parent.isTextblock || parent.type.spec.code) return
    const shortcut = matchWysiwygLine(parent.textContent)
    if (!shortcut) return
    const start = $from.before()
    const end = $from.after()
    if (shortcut.type === 'code' || shortcut.type === 'math') {
      const codeBlock = codeBlockSchema.type(ctx)
      if (!$from.node(-1).canReplaceWith($from.index(-1), $from.indexAfter(-1), codeBlock)) return
      const tr = state.tr.delete($from.start(), $from.end()).setBlockType($from.start(), $from.start(), codeBlock, {
        language: shortcut.type === 'math' ? 'LaTeX' : shortcut.language,
      })
      view.dispatch(tr.scrollIntoView())
      view.focus()
      handled = true
      return
    }
    let node: ProseNode | null = null
    try {
      node = shortcutNode(ctx, state.schema, shortcut)
    } catch (err) {
      console.warn('快捷结构生成失败', err)
      return
    }
    if (!node) return
    if (!$from.node(-1).canReplaceWith($from.index(-1), $from.indexAfter(-1), node.type)) return
    let tr = state.tr.replaceWith(start, end, node)
    const cursor = cursorInShortcut(tr.doc, start, node.nodeSize, shortcut.type)
    if (cursor != null) {
      const sel = TextSelection.findFrom(tr.doc.resolve(cursor), 1, true)
      if (sel) tr = tr.setSelection(sel)
    }
    view.dispatch(tr.scrollIntoView())
    view.focus()
    handled = true
  })
  if (!handled) return
  e.preventDefault()
  e.stopPropagation()
}

function shortcutNode(ctx: Ctx, schema: Schema, shortcut: LineShortcut): ProseNode | null {
  if (shortcut.type === 'image') {
    return imageBlockSchema.type(ctx).create({
      src: shortcut.src,
      caption: shortcut.caption,
      ratio: 1,
    })
  }
  if (shortcut.type === 'table-size') return createTable(ctx, shortcut.rows, shortcut.cols)
  if (shortcut.type === 'table-row') return tableFromCells(schema, shortcut.cells)
  return null
}

function tableFromCells(schema: Schema, cells: string[]): ProseNode | null {
  const table = schema.nodes.table
  const headerRow = schema.nodes.table_header_row
  const header = schema.nodes.table_header
  const row = schema.nodes.table_row
  const cell = schema.nodes.table_cell
  const paragraph = schema.nodes.paragraph
  if (!table || !headerRow || !header || !row || !cell || !paragraph) return null
  const headerCells = cells.map((text) =>
    header.create(null, text ? paragraph.create(null, schema.text(text)) : paragraph.create()),
  )
  const bodyCells = cells.map(() => cell.createAndFill())
  if (bodyCells.some((item) => !item)) return null
  return table.create(null, [
    headerRow.create(null, headerCells),
    row.create(null, bodyCells as ProseNode[]),
  ])
}

/** 表格表头行的光标落到第一个数据格，其余落到新节点内部。 */
function cursorInShortcut(doc: ProseNode, start: number, size: number, type: LineShortcut['type']): number | null {
  if (type !== 'table-row') return Math.min(start + 1, doc.content.size)
  let cellPos: number | null = null
  doc.nodesBetween(start, Math.min(start + size, doc.content.size), (node, pos) => {
    if (cellPos != null) return false
    if (node.type.name === 'table_cell') {
      cellPos = pos + 1
      return false
    }
  })
  return cellPos
}

async function applyMode(next: NoteEditorMode, persist: boolean) {
  if (next === mode.value) return
  if (mode.value === 'wysiwyg') {
    const captured = captureCrepeMarkdown()
    if (captured != null) adoptMarkdown(captured)
  }
  mode.value = next
  if (persist) void store.setNoteEditorMode(next)
  await nextTick()
  if (!props.note) return
  if (next === 'wysiwyg') {
    if (rootEl.value) await mountEditor(localContent.value)
  } else {
    await destroyEditor()
  }
}

function onPreviewClick(e: MouseEvent) {
  const target = e.target
  if (!(target instanceof HTMLImageElement) || !target.src) return
  previewSrc.value = target.currentSrc || target.src
}

/** 标题输入（v-model 之外）：用户修改标题必须同样进入防抖保存链路，
 *  否则只在改正文时才落库——改完标题不改正文，标题永远不会保存（切换/重启即丢失） */
function onTitleInput() {
  scheduleSave()
}

// ---- 图片尺寸接管（宽度语义）----
// Crepe 的 onImageLoad 会把图片高度锁定为像素（style.height），宽度 auto——窗口放大后
// 高度不变、宽度随之不变，表现为「图片不跟随窗口变宽」。这里在 CSS 层解锁高度
// （height: auto !important），让宽度成为唯一尺寸维度，并按 ProseMirror attrs 的 ratio
// 恢复用户调整过的尺寸：ratio=1（默认）→ 响应式 min(自然宽, 100%)；ratio<1 → 百分比定宽。
// ratio 沿用 Crepe 的序列化通道（markdown 图片 alt，如 ![0.75](url)），跨会话持久化。

const IMAGE_BLOCK_HANDLE_PX = 26 // 右下角把手命中区边长（与 CSS 视觉一致）

let detachImageListeners: () => void = () => {}

/** 监听图片 load（不冒泡，用捕获）与点击/拖拽把手，随编辑器挂载/销毁配对 */
function attachImageListeners() {
  const root = rootEl.value
  if (!root) return
  root.addEventListener('load', onEditorImgLoad, true)
  root.addEventListener('click', onEditorClick)
  root.addEventListener('pointerdown', onEditorPointerDown, true)
  root.addEventListener('keydown', onCrepeKeydown, true)
  detachImageListeners = () => {
    root.removeEventListener('load', onEditorImgLoad, true)
    root.removeEventListener('click', onEditorClick)
    root.removeEventListener('pointerdown', onEditorPointerDown, true)
    root.removeEventListener('keydown', onCrepeKeydown, true)
    detachImageListeners = () => {}
  }
}

function onEditorImgLoad(e: Event) {
  const t = e.target
  if (!(t instanceof HTMLImageElement) || t.dataset.type !== 'image-block') return
  syncImageWidths()
}

/** 把 doc 里每个 image-block 的 attrs.ratio 应用到对应 DOM 图片（幂等，无强制布局） */
function syncImageWidths() {
  const c = crepe
  if (!c) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    view.state.doc.descendants((node, pos) => {
      if (node.type.name !== 'image-block') return
      const dom = view.nodeDOM(pos)
      const img =
        dom instanceof Element
          ? dom.querySelector<HTMLImageElement>('img[data-type="image-block"]')
          : null
      if (img) applyImageWidth(img, Number(node.attrs.ratio) || 1)
    })
  })
}

function applyImageWidth(img: HTMLImageElement, ratio: number) {
  if (!img.naturalWidth) return // 尚未加载完成，load 后会再来
  if (ratio >= 0.995) {
    if (img.style.width) img.style.removeProperty('width')
    return
  }
  const w = `${(ratio * 100).toFixed(2)}%`
  if (img.style.width !== w) img.style.width = w
}

function findImageBlockAt(view: EditorView, img: HTMLImageElement): { node: ProseNode; pos: number } | null {
  let found: { node: ProseNode; pos: number } | null = null
  view.state.doc.descendants((node, pos) => {
    if (found) return false
    if (node.type.name !== 'image-block') return
    const dom = view.nodeDOM(pos)
    if (dom instanceof Element && dom.contains(img)) {
      found = { node, pos }
      return false
    }
  })
  return found
}

// ---- 拖拽调整图片宽度 ----
interface ResizeCtx {
  img: HTMLImageElement
  startX: number
  startW: number
  base: number
}
let resizeCtx: ResizeCtx | null = null

function isHandleZone(wrapper: Element, e: { clientX: number; clientY: number }): boolean {
  const r = wrapper.getBoundingClientRect()
  return e.clientX >= r.right - IMAGE_BLOCK_HANDLE_PX && e.clientY >= r.bottom - IMAGE_BLOCK_HANDLE_PX
}

function onEditorPointerDown(e: PointerEvent) {
  if (e.button !== 0 || !crepe) return
  const target = e.target
  if (!(target instanceof Element)) return
  const wrapper = target.closest<HTMLElement>('.milkdown-image-block .image-wrapper')
  if (!wrapper || !isHandleZone(wrapper, e)) return
  const img = wrapper.querySelector<HTMLImageElement>('img[data-type="image-block"]')
  if (!img || !img.naturalWidth) return
  // 阻断 PM 的 mousedown 兼容链（取消 pointerdown 即抑制后续 mouse 事件），避免拖拽时选中节点
  e.preventDefault()
  e.stopPropagation()
  const block = img.closest('.milkdown-image-block')
  const startW = img.getBoundingClientRect().width
  const base = block
    ? Math.min(img.naturalWidth, block.getBoundingClientRect().width) || startW
    : startW
  resizeCtx = { img, startX: e.clientX, startW, base }
  window.addEventListener('pointermove', onResizeMove)
  window.addEventListener('pointerup', onResizeUp)
}

function onResizeMove(e: PointerEvent) {
  if (!resizeCtx) return
  // 鼠标移出窗口后松键不会派发 pointerup：buttons 归零即视为拖拽结束
  if (!e.buttons) {
    onResizeUp()
    return
  }
  e.preventDefault()
  const w = Math.max(60, resizeCtx.startW + (e.clientX - resizeCtx.startX))
  // px 定宽拖动；上限由 CSS max-width:100% 兜底（超宽自动停在容器宽）
  resizeCtx.img.style.width = `${Math.round(w)}px`
}

function onResizeUp() {
  window.removeEventListener('pointermove', onResizeMove)
  window.removeEventListener('pointerup', onResizeUp)
  const ctx = resizeCtx
  resizeCtx = null
  if (!ctx || !crepe) return
  const finalW = ctx.img.getBoundingClientRect().width
  if (!ctx.base || !finalW) return
  let ratio = finalW / ctx.base
  if (ratio >= 0.98) ratio = 1 // 拖回默认宽度即恢复响应式
  ratio = Math.max(0.05, Math.round(ratio * 100) / 100)
  crepe.editor.action((actx) => {
    const view = actx.get(editorViewCtx)
    const found = findImageBlockAt(view, ctx.img)
    if (!found) return
    view.dispatch(
      view.state.tr.setNodeMarkup(found.pos, undefined, { ...found.node.attrs, ratio }),
    )
    view.focus()
  })
}

// ---- 点击图片预览（lightbox）----
const previewSrc = ref('')

function onEditorClick(e: MouseEvent) {
  if (previewSrc.value) return
  const target = e.target
  if (!(target instanceof Element)) return
  const block = target.closest<HTMLElement>('.milkdown-image-block')
  if (block) {
    // caption 输入框、右上角操作按钮（说明开关）不触发预览
    if (target.closest('.caption-input, .operation')) return
    const wrapper = target.closest('.image-wrapper')
    if (!wrapper) return
    if (isHandleZone(wrapper, e)) return
    const img = wrapper.querySelector<HTMLImageElement>('img[data-type="image-block"]')
    if (!img) return // 空上传态（无图片本体）
    previewSrc.value = img.currentSrc || img.src
    return
  }
  const inlineImg = target.closest<HTMLImageElement>('img.image-inline')
  if (inlineImg) previewSrc.value = inlineImg.currentSrc || inlineImg.src
}

function onPreviewKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') previewSrc.value = ''
}

watch(previewSrc, (v) => {
  if (v) window.addEventListener('keydown', onPreviewKeydown)
  else window.removeEventListener('keydown', onPreviewKeydown)
})

function scheduleSave() {
  if (!props.note) return
  dirty.value = true
  if (saveTimer) clearTimeout(saveTimer)
  lastNoteId = props.note.id
  saveTimer = setTimeout(() => {
    saveTimer = null
    if (props.note) {
      emit('save', props.note.id, normalizeTitle(localTitle.value), localContent.value)
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
          @input="onTitleInput"
          @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
        />
        <div class="mode-switch" role="radiogroup" aria-label="编辑模式">
          <button
            v-for="item in NOTE_EDITOR_MODES"
            :key="item.id"
            type="button"
            role="radio"
            :aria-checked="mode === item.id"
            :class="{ on: mode === item.id }"
            @click="applyMode(item.id, true)"
          >
            {{ item.label }}
          </button>
        </div>
        <button
          class="icon-btn del"
          title="删除笔记"
          aria-label="删除笔记"
          @click="emit('delete', note.id)"
        >
          <Trash2 :size="14" :stroke-width="1.8" />
        </button>
      </header>

      <div v-if="mode === 'wysiwyg'" ref="rootEl" class="crepe-root" @mousedown.capture="onEditorAreaMouseDown"></div>
      <textarea
        v-else-if="mode === 'source'"
        class="md-source"
        :value="localContent"
        spellcheck="false"
        placeholder="开始记录…"
        aria-label="Markdown 源码"
        @input="onSourceInput"
        @keydown="onSourceKeydown"
      />
      <div v-else class="ed-split">
        <textarea
          class="md-source"
          :value="localContent"
          spellcheck="false"
          placeholder="开始记录…"
          aria-label="Markdown 源码"
          @input="onSourceInput"
        @keydown="onSourceKeydown"
        />
        <div
          v-if="localContent.trim()"
          class="md-preview"
          aria-label="预览"
          v-html="previewHtml"
          @click="onPreviewClick"
        />
        <p v-else class="md-preview md-preview-empty">开始记录…</p>
      </div>

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

    <!-- 图片预览灯箱（瞬态表面，点击遮罩/关闭按钮/Esc 关闭） -->
    <Teleport to="body">
      <div
        v-if="previewSrc"
        class="img-lightbox"
        role="dialog"
        aria-modal="true"
        aria-label="图片预览"
        @click="previewSrc = ''"
      >
        <img :src="previewSrc" alt="图片预览" @click.stop />
        <button class="lb-close" type="button" aria-label="关闭预览" title="关闭 (Esc)" @click="previewSrc = ''">
          <X :size="16" :stroke-width="2" />
        </button>
      </div>
    </Teleport>
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
  container-type: inline-size;
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

.mode-switch {
  display: flex;
  flex-shrink: 0;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--input-bg);
}

.mode-switch button {
  border: none;
  background: transparent;
  color: var(--text-3);
  font: inherit;
  font-size: 0.75em;
  line-height: 1.2;
  padding: 6px 8px;
  cursor: pointer;
}

.mode-switch button.on {
  background: var(--brand-50);
  color: var(--brand-500);
}

.mode-switch button:hover {
  color: var(--text-1);
}

.md-source {
  flex: 1;
  min-height: 0;
  width: 100%;
  resize: none;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  color: var(--text-1);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.875em;
  line-height: 1.6;
  padding: 12px 14px;
  outline: none;
}

.md-source:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}

.ed-split {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 10px;
}

.md-preview {
  min-width: 0;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overflow-wrap: anywhere;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  padding: 12px 14px;
  color: var(--text-1);
  line-height: 1.6;
}

.md-preview-empty {
  margin: 0;
  color: var(--text-4);
}

.md-preview :deep(p),
.md-preview :deep(ul),
.md-preview :deep(ol),
.md-preview :deep(pre),
.md-preview :deep(blockquote) {
  margin: 0 0 0.6em;
}

.md-preview :deep(h1),
.md-preview :deep(h2),
.md-preview :deep(h3) {
  margin: 0.2em 0 0.4em;
  font-weight: 650;
  line-height: 1.3;
}

.md-preview :deep(img) {
  max-width: 100%;
  height: auto;
}

.md-preview :deep(.md-code) {
  margin: 0 0 0.6em;
  border: 1px solid var(--code-border);
  border-radius: 8px;
  background: var(--bg-code);
  overflow: hidden;
}

.md-preview :deep(.md-code-lang) {
  padding: 6px 12px;
  background: var(--bg-code-head);
  border-bottom: 1px solid var(--code-border);
  color: var(--code-text-dim);
  font-size: 0.6875em;
  font-weight: 600;
  letter-spacing: 0.03em;
  text-transform: lowercase;
}

.md-preview :deep(.md-code pre) {
  margin: 0;
  max-width: 100%;
  padding: 12px 14px;
  overflow: hidden;
  background: transparent;
  color: var(--code-text);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.8125em;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.md-preview :deep(.md-code code) {
  font-family: inherit;
  color: inherit;
  background: transparent;
  white-space: inherit;
  overflow-wrap: inherit;
}

.md-preview :deep(table) {
  width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
}

.md-preview :deep(th),
.md-preview :deep(td) {
  overflow-wrap: anywhere;
}

.md-preview :deep(a) {
  color: var(--brand-500);
}

.md-preview :deep(blockquote) {
  padding-left: 12px;
  border-left: 2px solid var(--border-strong);
  color: var(--text-2);
}

@container (max-width: 640px) {
  .ed-split {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
  }
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

/* 图片预览灯箱（Teleport 到 body，瞬态表面允许 backdrop-filter） */
.img-lightbox {
  position: fixed;
  inset: 0;
  z-index: 200;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--scrim);
  backdrop-filter: blur(10px);
  animation: lb-in 0.18s ease-out;
  cursor: zoom-out;
}

.img-lightbox img {
  max-width: 92vw;
  max-height: 90vh;
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
  cursor: default;
}

.lb-close {
  position: absolute;
  top: 20px;
  right: 20px;
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-soft);
  border-radius: 50%;
  background: var(--bg-card);
  color: var(--text-2);
  cursor: pointer;
  box-shadow: var(--shadow-card);
  transition: color 0.12s, border-color 0.12s, transform 0.12s;
}

.lb-close:hover {
  color: var(--text-1);
  border-color: var(--border-strong);
}

.lb-close:active {
  transform: scale(0.96);
}

@keyframes lb-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
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

/* ---- 图片尺寸接管 ----
   Crepe onImageLoad 把图片高度锁成像素（style.height），窗口放大后图片不跟随变宽。
   这里解锁高度让宽度成为唯一尺寸维度：ratio=1（默认）→ max-width:100% 响应式占满内容区；
   ratio<1（用户拖拽调整过，见 NoteEditor 宽度把手）→ 按百分比定宽，同样随窗口缩放 */
.crepe-root .milkdown .milkdown-image-block img {
  height: auto !important;
}

/* Crepe 内置高度把手停用（row-resize 调高度的交互不可见也不直观，改为宽度把手） */
.crepe-root .milkdown .milkdown-image-block .image-resize-handle {
  display: none !important;
}

/* 自定义宽度把手：image-wrapper 右下角 22px 视觉区（命中判定 26px，见 IMAGE_BLOCK_HANDLE_PX），
   悬停浮现斜向三点，pointer 事件由 NoteEditor 在编辑器根上委托处理 */
.crepe-root .milkdown .milkdown-image-block .image-wrapper::after {
  content: '';
  position: absolute;
  right: 0;
  bottom: 0;
  width: 22px;
  height: 22px;
  border-radius: 0 0 6px 0;
  cursor: nwse-resize;
  opacity: 0;
  transition: opacity 0.15s;
  background:
    radial-gradient(2.5px circle at 5px 17px, var(--text-3) 98%, transparent),
    radial-gradient(2.5px circle at 11px 11px, var(--text-3) 98%, transparent),
    radial-gradient(2.5px circle at 17px 5px, var(--text-3) 98%, transparent);
  filter: drop-shadow(0 0 2px rgba(0, 0, 0, 0.35));
}

.crepe-root .milkdown .milkdown-image-block:hover .image-wrapper::after,
.crepe-root .milkdown .milkdown-image-block .image-wrapper:active::after {
  opacity: 0.9;
}
</style>
