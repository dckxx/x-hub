<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { Crepe } from '@milkdown/crepe'
import '@milkdown/crepe/theme/common/style.css'
import '@milkdown/crepe/theme/frame.css'
import { editorViewCtx, parserCtx } from '@milkdown/kit/core'
import type { Ctx } from '@milkdown/kit/ctx'
import { imageBlockSchema } from '@milkdown/kit/component/image-block'
import { codeBlockSchema } from '@milkdown/kit/preset/commonmark'
import { createTable } from '@milkdown/kit/preset/gfm'
import { Fragment, type Node as ProseNode, type Schema } from '@milkdown/kit/prose/model'
import type { EditorView } from '@milkdown/kit/prose/view'
import { NodeSelection, TextSelection, type EditorState } from '@milkdown/kit/prose/state'
import { Code2, Columns2, Link2, PencilLine, Plus, Smile, Sparkles, Tag as TagIcon, WandSparkles, Waypoints, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type Note, type NoteLinks, type Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { normalizeShortcutDisplay } from '../composables/useShortcutRecorder'
import { attachBlockDrag } from '../utils/blockDrag'
import { beautifyNoteMarkdown } from '../utils/markdownBeautify'
import { restoreNoteImageSyntax } from '../utils/noteImageSyntax'
import { expandOnEnter, matchWysiwygLine, type LineShortcut } from '../utils/markdownEnter'
import { loosenHtmlBreaks, renderNoteMarkdown, restoreCrepeMarkdown } from '../utils/markdownHtml'
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
 * |列x行| 或表头行生成表格。实时预览里回车同样把 #、>、-、1.、---、- [ ]、- [x] 收成标题、引用、列表、分隔线和任务。
 * 图片（粘贴/拖拽/点击上传）统一由 Crepe 的上传管线处理：plugin-upload 的 handlePaste/
 * handleDrop + ImageBlock 的 onUpload 配置 → saveNoteImageFile（notes/images + xhub-note 协议）。
 * 注意勿再自建 DOM paste 监听——plugin-upload 已处理粘贴，叠加监听会导致图片重复插入。
 * 块拖拽（六点把手）为指针实现（utils/blockDrag.ts），绕开 Tauri 原生拖放对 HTML5 DnD 的拦截。
 */

const props = defineProps<{
  note: Readonly<Note> | null
  /** 空态落点提示（SpeednoteView 按 createNote 实际落点计算：选中文件夹 > 根目录） */
  newNoteHint?: string
}>()

const emit = defineEmits<{
  (e: 'save', id: number, title: string, content: string): void
  (e: 'delete', id: number): void
  /** 双链跳转：点击正文里的 [[标题]] / 反链面板条目 */
  (e: 'open-note', id: number): void
  /** 空态「新建笔记」按钮：与速记快捷键同一条 createNote 链路 */
  (e: 'create-note'): void
}>()

const store = useStore()
// index.vue provide 的全局轻提示（约定 12）；本组件原本无需 toast，美化无变化/失败时给一句反馈
const showToast = inject<((message: string) => void) | undefined>('showToast', undefined)

/** 空态快捷键提示与设置 → 快捷键同源同格式（键值 + 启用开关）；禁用时不出键提示 */
const notesShortcutHint = computed(() =>
  store.state.config.notes_shortcut_enabled
    ? normalizeShortcutDisplay(store.state.config.notes_shortcut || 'Ctrl+Shift+N')
    : '',
)

const mode = ref<NoteEditorMode>(normalizeNoteEditorMode(store.state.config.note_editor_mode))

const rootEl = ref<HTMLDivElement>()
const splitSourceEl = ref<HTMLTextAreaElement | null>(null)
const sourceEl = ref<HTMLTextAreaElement | null>(null)
const previewEl = ref<HTMLDivElement | null>(null)
let splitResize: ResizeObserver | null = null

let crepe: Crepe | null = null
/** 当前 Crepe 挂在哪一个容器上。容器被换掉（v-if 重建）时必须重挂，不能只看 crepe 是否非空。 */
let mountedOn: HTMLElement | null = null
let mounting = false
/** 挂载期间又切换了笔记：完成后需按最新笔记重挂一次（否则编辑器停留旧内容、防抖保存会跨笔记污染） */
let remountQueued = false
let detachBlockDrag: (() => void) | null = null
/** 图片监听解绑函数（attachImageListeners 内赋值）。声明必须留在下方 immediate watch 之前：
 * 源码/分屏模式下首挂载的 immediate 回调会经 destroyEditor 调到它，后置声明触发 TDZ（约定 38 时序陷阱②） */
let detachImageListeners: () => void = () => {}
/** [[ 双链监听解绑函数与补全浮层状态：同理必须声明在 immediate watch 之前（watch 回调经
 * destroyEditor 触达，后置声明 = ReferenceError: Cannot access ... before initialization，实测踩过） */
let detachWikiListeners: () => void = () => {}
const wikiSuggest = ref<WikiSuggest | null>(null)

interface WikiSuggest {
  /** 「[[」起点（文档位置） */
  from: number
  query: string
  items: Note[]
  index: number
  x: number
  y: number
}

const localTitle = ref('')
const localContent = ref('')
const dirty = ref(false)
const previewHtml = computed(() => renderNoteMarkdown(localContent.value))

// ---- 生命周期 ----
onBeforeUnmount(() => {
  flushPendingSave()
  detachImageListeners()
  detachWikiListeners()
  // 防御：卸载瞬间可能仍在拖拽/预览中
  window.removeEventListener('pointermove', onResizeMove)
  window.removeEventListener('pointerup', onResizeUp)
  resizeCtx = null
  window.removeEventListener('keydown', onPreviewKeydown)
  window.removeEventListener('keydown', onAiKeydown)
  splitResize?.disconnect()
  splitResize = null
  void destroyEditor()
})

async function destroyEditor() {
  detachBlockDrag?.()
  detachBlockDrag = null
  // 切笔记时 rootEl 是同一个 DOM 元素（v-if 分支没变，Vue 复用），必须在这里解绑：
  // 否则每切一次就多挂一套 load/click/pointerdown/keydown，而旧的解绑函数已被覆盖、
  // 再也调不到，表现为「切 N 次笔记后一次回车跑 N 遍」。
  detachImageListeners()
  detachWikiListeners()
  wikiSuggest.value = null
  const c = crepe
  crepe = null
  mountedOn = null
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
      defaultValue: loosenHtmlBreaks(content),
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
                emojiSource.value = 'slash'
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
          text: '输入 / 呼出更多功能…',
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
    mountedOn = rootEl.value
    // [[ 双链监听必须先挂（见 attachWikiListeners 内注释），图片/把手监听后挂
    attachWikiListeners()
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

// 必须同时观察 rootEl：Crepe 容器在 setup 阶段还没渲染，只盯笔记 id 时首次挂载会落空（约定 38 时序陷阱①）。
// flush:'post' 等本次渲染把容器交出来；容器晚一拍出现时，rootEl 变化会再进一次回调。
watch(
  [() => props.note?.id, rootEl],
  async ([id, el], prev) => {
    const prevId = prev?.[0]
    // 回调触发时 props.note 已是新笔记，Crepe 里仍是上一篇。先取出上一篇正文再落盘，避免写到新笔记上。
    if (typeof prevId === 'number' && prevId !== id) {
      if (mode.value === 'wysiwyg') {
        // 切走前同样先填充空段落再序列化，保住上一篇的空行
        normalizeEmptyParagraphs()
        const captured = captureCrepeMarkdown()
        const restored = captured == null ? null : restoreCrepeMarkdown(captured)
        if (restored != null && restored !== localContent.value) {
          localContent.value = restored
          deriveTitleFromContent(restored)
          dirty.value = true
        }
      }
      flushLeavingNote(prevId)
    }
    if (!props.note) {
      await destroyEditor()
      syncLocal()
      noteTags.value = []
      return
    }
    const noteChanged = prevId !== id
    if (noteChanged) syncLocal()
    if (mode.value === 'wysiwyg') {
      // 源码/分屏没有这块容器。el 为空就等下一次 rootEl 赋值，不能当成「笔记没了」去清正文。
      if (el && (noteChanged || !crepe || mountedOn !== el)) void mountEditor(localContent.value)
    } else if (noteChanged) {
      await destroyEditor()
    }
    if (!noteChanged || !props.note || props.note.id !== id) return
    if (isTauri()) {
      // await 期间用户可能已切到第三篇，回来要再校验一次 id，
      // 否则标签行会显示上一篇的标签（点「移除」还会改错关系）
      const tags = await tauriApi.getNoteTags(id)
      if (props.note?.id === id) noteTags.value = tags
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
    emit('save', lastNoteId, normalizeTitle(localTitle.value), collectContentForSave())
  }
}

/** 空标题落库时归一为默认值，与新建笔记的初始标题一致（列表展示不出现空行） */
function normalizeTitle(title: string): string {
  const t = title.trim()
  return t ? t : '无标题笔记'
}

function onEdited(markdown: string) {
  if (!props.note || mode.value !== 'wysiwyg') return
  // Crepe 序列化会加上 \[ \* 并把列表/分隔线改写成星号。收回来再存，切到分屏才和实时预览一致。
  adoptMarkdown(restoreCrepeMarkdown(markdown))
  // ratio 等图片属性可能经撤销/属性事务变化，同步一次宽度（幂等、无强制布局）
  syncImageWidths()
}

/**
 * 空段落规范化：Markdown 规范里空行无语义，序列化/解析往返会把连续空段落折叠掉
 * （用户主动输入或粘贴出的空行在切走再切回后消失，实测反馈）。把顶层空段落填入
 * **零宽空格 U+200B**——普通文本字符、Markdown 原样往返保留、**零显示宽度**：
 * 光标点击空行时贴行首，不会像 NBSP 那样「光标右边空出一格」（NBSP 方案的实测
 * 缺陷，用户反馈「光标闪一下自动空了一格」后迁移）。摘要派生对纯占位行返回空串，
 * 不接管「无标题笔记」默认值。
 *
 * ⚠️ **只能在「取正文落库前」调用**（collectContentForSave），绝不能挂在每次编辑
 * （markdownUpdated）上：① dispatch 事务会打断 IME 组合输入与光标；② Crepe 斜杠
 * 菜单的 shouldShow 要求段落文本以 `/` 开头，占位符前缀会让 `\u200B/` 不匹配——
 * 该场景由 fixSlashAfterNbsp 在输入侧放行。编辑过程中文档保持原样，只在防抖保存/
 * 切卡的取文瞬间填充，用户全程无感。
 */
function normalizeEmptyParagraphs() {
  const c = crepe
  if (!c || mode.value !== 'wysiwyg') return
  // 斜杠菜单打开时绝不填充：dispatch 会让菜单重算过滤词，getContent 读到刚填的
  // 占位符 → filter 被污染 → 菜单项全部被过滤（size=0）→ Crepe 的 watch 直接
  // hide()——「+ 面板一闪而过」的根因（实测抓栈定位）。菜单开着说明用户正在选
  // 命令，本次保存先放弃填充；选完命令后的下一次保存会补上。
  if (document.querySelector('.milkdown-slash-menu[data-show="true"]')) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    // 空段落 → 填零宽空格；存量 NBSP 占位行（上一版方案写入）→ 就地迁移为
    // 零宽空格（NBSP 有显示宽度，光标停在里面会「空一格」）
    const edits: { at: number; len: number; text: string }[] = []
    state.doc.descendants((node, pos, parent) => {
      // 只处理文档顶层的段落（列表项内不动）
      if (node.type.name !== 'paragraph' || !parent || parent.type.name !== 'doc') return
      if (node.content.size === 0) {
        edits.push({ at: pos + 1, len: 0, text: '\u200B' })
      } else if (node.textContent === '\u00A0') {
        edits.push({ at: pos + 1, len: 1, text: '\u200B' })
      }
    })
    if (edits.length > 0) {
      // 从文档尾部往前改：前面的修改不会让后面的 pos 偏移。
      // addToHistory=false：填充不进撤销栈，用户 Ctrl+Z 不会反复删到这些填充符
      let tr = state.tr
      for (const e of edits.slice().reverse()) {
        if (e.len > 0) tr = tr.delete(e.at, e.at + e.len)
        tr = tr.insertText(e.text, e.at)
      }
      tr.setMeta('addToHistory', false)
      view.dispatch(tr)
    }
  })
}

/**
 * 取当前应落盘的正文。wysiwyg 模式：先做空段落零宽空格填充（保住用户输入的空行），
 * 再从 Crepe 序列化；源码/分屏模式：textarea 内容即真相源，空行天然保留。
 */
function collectContentForSave(): string {
  if (mode.value === 'wysiwyg') {
    normalizeEmptyParagraphs()
    const captured = captureCrepeMarkdown()
    if (captured != null) {
      const restored = restoreCrepeMarkdown(captured)
      localContent.value = restored
      return restored
    }
  }
  return localContent.value
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
  emit('save', id, normalizeTitle(localTitle.value), collectContentForSave())
}

function onSourceInput(e: Event) {
  const value = (e.target as HTMLTextAreaElement).value
  adoptMarkdown(value)
  if (mode.value === 'split') void nextTick(syncPreviewScroll)
}

/** 分屏时右边预览按左边源码的滚动比例跟着走。两边高度不同，对齐的是滚动条位置而不是某一行。 */
function syncPreviewScroll() {
  const source = splitSourceEl.value
  const preview = previewEl.value
  if (!source || !preview) return
  const sourceMax = source.scrollHeight - source.clientHeight
  const previewMax = preview.scrollHeight - preview.clientHeight
  preview.scrollTop = sourceMax <= 0 || previewMax <= 0 ? 0 : (source.scrollTop / sourceMax) * previewMax
}

watch([splitSourceEl, previewEl], () => {
  splitResize?.disconnect()
  splitResize = null
  const source = splitSourceEl.value
  const preview = previewEl.value
  if (!source || !preview) return
  splitResize = new ResizeObserver(() => syncPreviewScroll())
  splitResize.observe(source)
  splitResize.observe(preview)
  // 双跳延迟：v-html 刚写入时图片/代码块高度未稳定，立即同步会拿到不准的
  // scrollHeight（表现为首次进分屏两边行不对应，切一次源码再回来才正常）
  requestAnimationFrame(() => requestAnimationFrame(syncPreviewScroll))
})

/** 行尾回车补上代码块、公式、图片、表格。光标留在新结构里。 */
function onSourceKeydown(e: KeyboardEvent) {
  if (e.key !== 'Enter' || e.shiftKey || e.isComposing || e.defaultPrevented) return
  const el = e.target
  if (!(el instanceof HTMLTextAreaElement) || el.selectionStart !== el.selectionEnd) return
  const expanded = expandOnEnter(el.value, el.selectionStart)
  if (!expanded) return
  e.preventDefault()
  adoptMarkdown(expanded.value)
  void nextTick(() => {
    el.setSelectionRange(expanded.cursor, expanded.cursor)
    if (mode.value === 'split') syncPreviewScroll()
  })
}

/**
 * 实时预览里 Milkdown 要在标记后面加空格才变成对应节点，单独回车只是换行。
 * 捕获阶段接住回车，把整行快捷标记换成代码块、公式块、图片块、表格、标题、引用、列表或分隔线。
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
    // 第三个 `-` 会被 Milkdown 的输入规则收成分隔线，并选中这条线。
    // 这时默认回车会在线的上方再插一段（线是父节点第一个子节点时），光标回到占位提示，看起来像没转成。
    if (!empty) {
      const selected = state.selection
      if (!(selected instanceof NodeSelection) || selected.node.type.name !== 'hr') return
      const paragraph = state.schema.nodes.paragraph
      if (!paragraph) return
      const after = selected.to
      let tr = state.tr
      const next = state.doc.resolve(after).nodeAfter
      if (!next || !next.isTextblock) tr = tr.insert(after, paragraph.create())
      const sel = TextSelection.findFrom(tr.doc.resolve(Math.min(after + 1, tr.doc.content.size)), 1, true)
      if (!sel) return
      view.dispatch(tr.setSelection(sel).scrollIntoView())
      view.focus()
      handled = true
      return
    }
    const parent = $from.parent
    if (!parent.isTextblock || parent.type.spec.code) return
    // 光标不在行尾时回车只是拆行，不能把整行快捷标记换掉
    if ($from.parentOffset !== parent.content.size) return
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
    if (shortcut.type === 'heading') {
      const heading = state.schema.nodes.heading
      if (!heading) return
      if (!$from.node(-1).canReplaceWith($from.index(-1), $from.indexAfter(-1), heading)) return
      const from = $from.start()
      const to = Math.min(from + shortcut.prefix, $from.end())
      let tr = state.tr
      if (to > from) tr = tr.delete(from, to)
      const pos = tr.mapping.map(from)
      tr = tr.setBlockType(pos, pos, heading, { level: shortcut.level })
      view.dispatch(tr.scrollIntoView())
      view.focus()
      handled = true
      return
    }
    if (shortcut.type === 'task') {
      const itemDepth = listItemDepth($from)
      if (itemDepth > 0) {
        const item = $from.node(itemDepth)
        if (!item.type.spec.attrs || !('checked' in item.type.spec.attrs)) return
        const from = $from.start()
        const to = Math.min(from + shortcut.prefix, $from.end())
        let tr = state.tr
        if (to > from) tr = tr.delete(from, to)
        const pos = tr.mapping.map($from.before(itemDepth))
        tr = tr.setNodeMarkup(pos, undefined, { ...item.attrs, checked: shortcut.checked })
        view.dispatch(tr.scrollIntoView())
        view.focus()
        handled = true
        return
      }
    }
    if (shortcut.type === 'hr') {
      const hrType = state.schema.nodes.hr ?? state.schema.nodes.horizontal_rule
      const paragraph = state.schema.nodes.paragraph
      if (!hrType || !paragraph) return
      const hr = hrType.create()
      const blank = paragraph.create()
      const fragment = Fragment.from([hr, blank])
      if (!$from.node(-1).canReplace($from.index(-1), $from.indexAfter(-1), fragment)) return
      let tr = state.tr.replaceWith(start, end, fragment)
      const sel = TextSelection.findFrom(tr.doc.resolve(Math.min(start + hr.nodeSize + 1, tr.doc.content.size)), 1, true)
      if (sel) tr = tr.setSelection(sel)
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
  if (shortcut.type === 'blockquote' || shortcut.type === 'bullet' || shortcut.type === 'ordered' || shortcut.type === 'task') {
    return wrappedBlock(schema, shortcut)
  }
  return null
}

function wrappedBlock(
  schema: Schema,
  shortcut: Extract<LineShortcut, { type: 'blockquote' | 'bullet' | 'ordered' | 'task' }>,
): ProseNode | null {
  const paragraph = schema.nodes.paragraph
  if (!paragraph) return null
  const inner = shortcut.text ? paragraph.create(null, schema.text(shortcut.text)) : paragraph.create()
  if (shortcut.type === 'blockquote') {
    const quote = schema.nodes.blockquote
    return quote ? quote.create(null, inner) : null
  }
  const itemType = schema.nodes.list_item
  if (!itemType) return null
  const attrs: Record<string, unknown> = {}
  if (shortcut.type === 'ordered') {
    attrs.listType = 'ordered'
    attrs.label = `${shortcut.order}.`
  }
  if (shortcut.type === 'task') {
    if (!itemType.spec.attrs || !('checked' in itemType.spec.attrs)) return null
    attrs.checked = shortcut.checked
    attrs.listType = 'bullet'
    attrs.label = '•'
  }
  const item = itemType.create(attrs, inner)
  if (shortcut.type === 'ordered') {
    const list = schema.nodes.ordered_list
    return list ? list.create({ order: shortcut.order }, item) : null
  }
  const list = schema.nodes.bullet_list
  return list ? list.create(null, item) : null
}

function listItemDepth($from: EditorState['selection']['$from']): number {
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name === 'list_item') return depth
  }
  return -1
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
    if (captured != null) adoptMarkdown(restoreCrepeMarkdown(captured))
  }
  mode.value = next
  if (persist) void store.setNoteEditorMode(next)
  if (next !== 'wysiwyg') await destroyEditor()
  // 切回实时预览时容器会重新出现，上面的 rootEl watch 负责挂载
}

// ---- 一键美化格式（utils/markdownBeautify.ts，确定性排版整理） ----

const canBeautify = computed(() => !!props.note && localContent.value.trim().length > 0)

function onBeautify() {
  if (!props.note || !canBeautify.value) return
  // 斜杠菜单开着时 dispatch 会污染其过滤词（同 normalizeEmptyParagraphs 的坑，约定 38）
  if (mode.value === 'wysiwyg' && document.querySelector('.milkdown-slash-menu[data-show="true"]')) {
    showToast?.('请先收起斜杠菜单再美化')
    return
  }
  // 取当前真相源：wysiwyg 下同时填充空行占位符，保证所改即所存
  const current = collectContentForSave()
  const beautified = beautifyNoteMarkdown(current)
  if (beautified === current) {
    showToast?.('格式已经很规整了')
    return
  }
  if (mode.value === 'wysiwyg') {
    // 整篇单事务替换（一步撤销）；失败则不落任何改动
    if (!replaceWholeDocWithMarkdown(beautified)) {
      showToast?.('美化失败，请重试')
      return
    }
    // 文档内容未变化（如只压缩了纯空行）时 markdownUpdated 不触发，这里兜底把
    // 美化结果收进保存链路；文档有变化时 onEdited 会用序列化结果再收一次，防抖合并
    adoptMarkdown(beautified)
    return
  }
  // 源码/分屏：textarea 是真相源，替换后按行号迁回光标
  const textarea = mode.value === 'split' ? splitSourceEl.value : sourceEl.value
  const caretLine = textarea
    ? localContent.value.slice(0, textarea.selectionStart).split('\n').length - 1
    : 0
  adoptMarkdown(beautified)
  void nextTick(() => {
    const el = mode.value === 'split' ? splitSourceEl.value : sourceEl.value
    if (!el || el.value !== beautified) return
    const lines = beautified.split('\n')
    const line = Math.min(caretLine, lines.length - 1)
    let pos = 0
    for (let i = 0; i < line; i++) pos += lines[i].length + 1
    el.setSelectionRange(pos, pos)
  })
}

/** 所见即所得模式：把一份 Markdown 解析回文档并整篇替换。单个事务 = Ctrl+Z 一步整体还原。 */
function replaceWholeDocWithMarkdown(markdown: string): boolean {
  const c = crepe
  if (!c) return false
  try {
    let replaced = false
    c.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      const nextDoc = ctx.get(parserCtx)(markdown)
      if (!nextDoc) return
      const { state } = view
      // 选区随事务映射，落点不在文本内时 ProseMirror 自动就近落位
      view.dispatch(state.tr.replaceWith(0, state.doc.content.size, nextDoc))
      replaced = true
    })
    return replaced
  } catch (e) {
    console.warn('一键美化：整篇替换失败', e)
    return false
  }
}

// ---- AI 深度整理（ai_transform_note：无会话、不落库的语义重排，弹层预览后应用） ----

type AiTransformState = 'idle' | 'streaming' | 'done' | 'error'
const aiState = ref<AiTransformState>('idle')
const aiResult = ref('')
const aiError = ref('')
/** 发起整理时的原稿：模型输出会把图片压成裸地址，回收图片语法要按原稿比对（见 utils/noteImageSyntax） */
const aiSource = ref('')
/** 关闭弹层即放弃：迟到的流式增量与返回值一律丢弃（后端那次请求自然跑完，与对话面板同语义）。
 *  用自增请求序号而非共享布尔：关闭后立刻重开时旧请求的闭包还活着，共享布尔被重开
 *  置回 false 会让旧请求的增量与返回值串进新流（旧结果覆盖新结果） */
let aiRequestSeq = 0

/** 弹层展示与应用共用这一份口径：先剥模型自加的外层围栏，再按原稿回收被压扁的图片语法。
 *  少任何一步都会让「预览看着对、应用下去图片没了」或反之 */
const aiMarkdown = computed(() =>
  restoreNoteImageSyntax(aiSource.value, unwrapModelMarkdown(aiResult.value)),
)
/** 回收确实改动了模型输出 = 模型又写坏了图片语法（提示词没拦住），应用时给用户一句交代，
 *  而不是悄悄改掉他要应用的内容 */
const aiImagesRestored = computed(
  () => aiState.value === 'done' && aiMarkdown.value !== unwrapModelMarkdown(aiResult.value),
)
const aiPreviewHtml = computed(() => renderNoteMarkdown(aiMarkdown.value))

/** 模型偶尔无视指令把结果包进 ```markdown 围栏，应用前剥掉。白名单只收
 *  「包装器」类标签（markdown/text/plain 等）——```rust 这类真代码块是笔记内容，
 *  剥了会把代码外壳弄丢 */
function unwrapModelMarkdown(text: string): string {
  const t = text.trim()
  const m = /^```(?:markdown|md|text|plain|plaintext|txt)?[ \t]*\r?\n([\s\S]*?)\r?\n?[ \t]*```$/.exec(t)
  return (m ? m[1] : t).trim()
}

async function openAiTransform() {
  if (!props.note || !canBeautify.value || aiState.value === 'streaming') return
  const current = collectContentForSave()
  const seq = ++aiRequestSeq
  aiSource.value = current
  aiResult.value = ''
  aiError.value = ''
  aiState.value = 'streaming'
  try {
    const full = await tauriApi.aiTransformNote(current, (e) => {
      if (e.type === 'chunk' && seq === aiRequestSeq) aiResult.value += e.content
    })
    if (seq !== aiRequestSeq) return
    if (!full.trim()) {
      aiError.value = '模型未返回内容，请重试或换一个模型'
      aiState.value = 'error'
      return
    }
    aiResult.value = full
    aiState.value = 'done'
  } catch (e) {
    if (seq !== aiRequestSeq) return
    aiError.value = typeof e === 'string' ? e : String(e)
    aiState.value = 'error'
  }
}

function closeAiDialog() {
  // 序号自增即作废在途请求的增量与返回值
  aiRequestSeq++
  aiState.value = 'idle'
  // 原稿可能很长（上限 3000 条笔记里最长的那篇），关闭后没有留存价值——下次发起会重取
  aiSource.value = ''
}

function applyAiResult() {
  const md = aiMarkdown.value
  if (!md) return
  const restored = aiImagesRestored.value
  if (mode.value === 'wysiwyg') {
    // 整篇单事务替换（一步撤销）；失败不落任何改动
    if (!replaceWholeDocWithMarkdown(md)) {
      showToast?.('应用失败，请重试')
      return
    }
    adoptMarkdown(md)
  } else {
    // 内容已重组，行号映射无意义，光标回文档开头
    adoptMarkdown(md)
    void nextTick(() => {
      const el = mode.value === 'split' ? splitSourceEl.value : sourceEl.value
      el?.setSelectionRange(0, 0)
    })
  }
  if (restored) showToast?.('已自动修回被改写掉的图片，应用后请确认图片显示正常')
  closeAiDialog()
}

function onAiKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') closeAiDialog()
}
watch(aiState, (s) => {
  if (s !== 'idle') window.addEventListener('keydown', onAiKeydown)
  else window.removeEventListener('keydown', onAiKeydown)
})

function onPreviewClick(e: MouseEvent) {
  const target = e.target
  if (!(target instanceof Element)) return
  // 预览区链接：默认浏览器打开（编辑区同款口径）
  const anchor = target.closest<HTMLAnchorElement>('a[href]')
  if (anchor) {
    e.preventDefault()
    const href = anchor.getAttribute('href') ?? ''
    if (/^https?:\/\//i.test(href)) void tauriApi.openExternal(href)
    return
  }
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
}/** 把 doc 里每个 image-block 的 attrs.ratio 应用到对应 DOM 图片（幂等，无强制布局） */
function syncImageWidths() {
  const c = crepe
  if (!c) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    view.state.doc.descendants((node, pos) => {
      if (node.type.name !== 'image-block') return
      const dom = view.nodeDOM(pos)
      const block = dom instanceof Element ? dom.closest<HTMLElement>('.milkdown-image-block') : null
      if (!block) return
      const img = block.querySelector<HTMLImageElement>('img[data-type="image-block"]')
      applyImageWidth(block, img, Number(node.attrs.ratio) || 1)
    })
  })
}

/**
 * 图片宽度语义（作用在 image-block 容器上）：
 * ratio=1（默认）→ 清掉内联样式，块级占满内容区；
 * ratio<1（用户拖过宽度把手）→ 容器定宽（百分比）+ inline-block，让同一段内容里的
 * 多张缩窄图**横向并排**（块节点相邻 + inline-block 自然同行排布，放不下自动换行），
 * 用户反馈：缩放后不该仍独占整行，想一行放好几张图看更多信息。
 */
function applyImageWidth(block: HTMLElement, img: HTMLImageElement | null, ratio: number) {
  if (ratio >= 0.995) {
    for (const prop of ['width', 'display', 'vertical-align', 'margin-right']) {
      block.style.removeProperty(prop)
    }
    if (img) img.style.removeProperty('width')
    return
  }
  const w = `${(Math.max(0.05, ratio) * 100).toFixed(2)}%`
  block.style.display = 'inline-block'
  block.style.verticalAlign = 'top'
  block.style.width = w
  block.style.marginRight = '10px'
  if (img) img.style.width = '100%'
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
  // 拖动直接作用在容器宽度上（px 定宽；上限由容器 max-width 兜底），
  // 缩窄后 inline-block 让出右侧空间，可与其他缩窄图并排
  const block = resizeCtx.img.closest<HTMLElement>('.milkdown-image-block')
  if (block) {
    block.style.display = 'inline-block'
    block.style.verticalAlign = 'top'
    block.style.marginRight = '10px'
    block.style.width = `${Math.round(w)}px`
  }
  resizeCtx.img.style.width = '100%'
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
  // 代码块 Copy 按钮（Crepe 内建）只改按钮态、无成功反馈；这里补应用内 toast
  // （剪贴板写入由 Crepe 完成，Linux/Windows 实测可用），与提示词卡片「已复制」同口径
  if (target.closest('.milkdown-code-block .tools-button-group button')) {
    showToast?.('已复制')
  }
  // 链接点击跳系统浏览器（编辑态 a 标签默认无反应，这里委托打开；只放行 http(s)）
  const anchor = target.closest<HTMLAnchorElement>('a[href]')
  if (anchor) {
    e.preventDefault()
    e.stopPropagation()
    const href = anchor.getAttribute('href') ?? ''
    if (/^https?:\/\//i.test(href)) void tauriApi.openExternal(href)
    return
  }
  // [[标题]] 点击跳转（双链轻量版）：命中未解析/无同名笔记时按普通文本处理
  const c0 = crepe
  if (c0 && mode.value === 'wysiwyg' && target.closest('.ProseMirror')) {
    let jumped = false
    c0.editor.action((ctx) => {
      const view = ctx.get(editorViewCtx)
      const pos = view.posAtCoords({ left: e.clientX, top: e.clientY })
      if (pos == null) return
      const title = wikiTitleAt(view.state, pos.pos)
      if (!title) return
      const hit =
        store.state.notes.find((n) => n.title === title && n.id !== props.note?.id) ??
        store.state.notes.find((n) => n.title === title)
      if (hit) {
        jumped = true
        emit('open-note', hit.id)
      }
    })
    if (jumped) return
  }
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
      // 取正文走统一出口：wysiwyg 下先做空段落 NBSP 填充（保空行）再序列化
      emit('save', props.note.id, normalizeTitle(localTitle.value), collectContentForSave())
    }
  }, 600)
}

// 只有「同一篇笔记」的 updated_at 变化才是保存成功后的回写，才可以清 dirty。
// 切笔记时 id 也变了，绝不能在这里清：本 watch 默认 flush:'pre'，会先于下面那个
// flush:'post' 的切笔记 watch 执行，dirty 被清成 false 后 flushLeavingNote 会直接返回，
// 最后一整段 600ms 防抖窗口内的编辑就此静默丢失（切笔记丢字）。
watch(
  () => [props.note?.id, props.note?.updated_at] as const,
  ([id], [prevId]) => {
    if (id === prevId) dirty.value = false
  },
)

// ---- 来源链接（剪藏笔记展示原文出处，ADR 0014 口径：source_url 存在才显示） ----
function openSourceUrl() {
  const url = props.note?.source_url
  if (url) void tauriApi.openExternal(url)
}

// ---- 双链侧面板（双链轻量版，docs/speednote-plan.md §7）----
// outgoing = 本笔记引用的 [[标题]]；incoming = 谁引用了本笔记。派生索引随保存重建，
// 所以 updated_at 变化（保存成功回写）即刷新。头部 Waypoints 图标切换右侧全高面板。
const links = ref<NoteLinks>({ outgoing: [], incoming: [] })
const linksPanelOpen = ref(false)

async function refreshLinks() {
  const id = props.note?.id
  if (!id || !isTauri()) {
    links.value = { outgoing: [], incoming: [] }
    return
  }
  try {
    const l = await tauriApi.getNoteLinks(id)
    if (props.note?.id === id) links.value = l
  } catch {
    links.value = { outgoing: [], incoming: [] }
  }
}

watch(
  () => [props.note?.id, props.note?.updated_at] as const,
  () => void refreshLinks(),
  { immediate: true },
)

// ---- [[ 双链：输入补全 + 点击跳转 ----
// 语法为纯文本（Markdown 原样往返），不引入自定义节点：[[ 触发标题补全浮层，
// 点击已存在的 [[标题]] 跳转到同名笔记（含反链面板条目）。索引重建在保存链路（update_note）。
// ⚠️ wikiSuggest / detachWikiListeners 的声明在文件前部（immediate watch 之前），见彼处注释。

function candidateNotes(query: string): Note[] {
  const q = query.trim().toLowerCase()
  return store.state.notes
    .filter((n) => n.id !== props.note?.id && n.title !== '无标题笔记')
    .filter((n) => (q ? n.title.toLowerCase().includes(q) : true))
    .sort((a, b) => a.title.localeCompare(b.title, 'zh'))
    .slice(0, 8)
}

/** 光标前同一文本块内的「[[query」：返回引用起点，无则 null（口径同 slashQueryStart） */
function wikiQueryStart(state: EditorState, from: number): { start: number; query: string } | null {
  const $from = state.doc.resolve(from)
  if ($from.depth === 0 || !$from.parent.inlineContent) return null
  const blockStart = $from.start()
  const textBefore = state.doc.textBetween(blockStart, from, '\n', '\uFFFC')
  const m = /\[\[([^\[\]\n]*)$/.exec(textBefore)
  if (!m) return null
  return { start: blockStart + m.index, query: m[1] }
}

function onWikiInput(e: Event) {
  const c = crepe
  if (!c || mode.value !== 'wysiwyg') return
  // IME 组合期间绝不 dispatch（会炸掉组合、吞掉用户输入的字符）；组合结束后
  // 浏览器会再派发一次 isComposing=false 的 input，fix 在那时正常执行
  if ((e as InputEvent).isComposing) return
  // ⚠️ 必须 setTimeout 延一拍：PM 对 DOM 变更的回读（DOMObserver）是异步 flush 的，
  // input 事件此刻的 state 还不含刚输入的字符，立即跑 fix 会读到旧文档而错过匹配
  // （实测：execCommand 输 / 后段落仍是占位符+/，fix 空跑）
  setTimeout(fixSlashAfterNbsp, 0)
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const found = wikiQueryStart(state, state.selection.from)
    if (!found) {
      wikiSuggest.value = null
      return
    }
    const items = candidateNotes(found.query)
    if (items.length === 0) {
      wikiSuggest.value = null
      return
    }
    const coords = view.coordsAtPos(state.selection.from)
    wikiSuggest.value = {
      from: found.start,
      query: found.query,
      items,
      index: 0,
      x: coords.left,
      y: coords.bottom,
    }
  })
}

/**
 * 修「输入 / 斜杠菜单不弹」的两类场景，把段落归一成半角 "/" 开头（菜单触发条件）：
 * ① 空行（NBSP 段落）上输入 /：段落 = "\u00A0/"，前缀 NBSP 让 startsWith("/") 失败，
 *    且用户按退格删的其实是 NBSP——表现为「输 / 没反应、退格一下 / 还在但面板出来了」；
 * ② 中文输入法把半角 / 组合成全角 ／（中文标点模式默认行为）：段落 = "／" 或
 *    "\u00A0／"，Crepe 判定的是半角，同样永不弹。
 * 段落文本恰好等于这三种形态之一时整体替换为 "/"（addToHistory=false，不进撤销栈）；
 * 其余输入零开销。只处理段落整体、不碰有内容的行——那与 Crepe 原生行为一致。
 * ⚠️ IME 组合中的调用已被 onWikiInput 的 isComposing 检查挡下，此处只在字符上屏后执行。
 */
function fixSlashAfterNbsp() {
  const c = crepe
  if (!c || mode.value !== 'wysiwyg') return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const { $from } = state.selection
    if ($from.depth === 0 || !$from.parent.inlineContent) return
    if ($from.parent.type.name !== 'paragraph' || $from.parent.childCount === 0) return
    const text = $from.parent.textContent
    // 空行占位（NBSP / 零宽空格）+ 斜杠的三种形态统一归一为半角 "/"
    if (text !== '\u200B/' && text !== '\u00A0/' && text !== '／' && text !== '\u200B／' && text !== '\u00A0／') return
    const start = $from.start()
    let tr = state.tr.delete(start, start + text.length)
    tr = tr.insertText('/', start)
    tr.setMeta('addToHistory', false)
    view.dispatch(tr)
  })
}

function onWikiKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    // 斜杠菜单被 Esc 关闭后，清掉光标所在段落行首残留的孤立 /（块把手「+」改写
    // 与用户手输指令放弃的场景）；延 50ms 等菜单 hide 完成后再判定
    setTimeout(cleanupLoneSlashAtCursor, 50)
    // 不 return：[[ 补全浮层的 Esc 处理继续走
  }
  const s = wikiSuggest.value
  if (!s) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    e.stopPropagation()
    s.index = (s.index + 1) % s.items.length
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopPropagation()
    s.index = (s.index - 1 + s.items.length) % s.items.length
  } else if (e.key === 'Enter' || e.key === 'Tab') {
    e.preventDefault()
    e.stopPropagation()
    commitWikiSuggest(s.items[s.index])
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    wikiSuggest.value = null
  }
}

function commitWikiSuggest(item: Note) {
  const s = wikiSuggest.value
  const c = crepe
  wikiSuggest.value = null
  if (!s || !c) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const to = view.state.selection.from
    view.dispatch(view.state.tr.insertText(`[[${item.title}]]`, s.from, to))
    view.focus()
  })
}

function onWikiClickItem(item: Note) {
  commitWikiSuggest(item)
}

/** 点击位置落在 [[标题]] 内时返回该标题（同一文本块内扫描，索引 1:1 对应） */
function wikiTitleAt(state: EditorState, pos: number): string | null {
  const $pos = state.doc.resolve(pos)
  if ($pos.depth === 0 || !$pos.parent.inlineContent) return null
  const blockStart = $pos.start()
  const blockEnd = $pos.end()
  const text = state.doc.textBetween(blockStart, blockEnd, '\n', '\uFFFC')
  const idx = pos - blockStart
  const open = text.lastIndexOf('[[', Math.max(idx - 1, 0))
  if (open < 0) return null
  const close = text.indexOf(']]', idx)
  if (close < 0 || close <= open + 2) return null
  const inner = text.slice(open + 2, close)
  if (!inner || /[\\[\]\n]/.test(inner)) return null
  // 光标必须在 [[ ... ]] 区间内（含括号本身），落在 ] 之后的不算
  if (idx > close + 1 || idx < open) return null
  return inner.trim()
}

function attachWikiListeners() {
  const root = rootEl.value
  if (!root) return
  // 先于 attachImageListeners 注册（同元素同相位按注册序执行），弹窗打开时按键优先被这里接住。
  // compositionend 必须监听：IME 组合中的 input 事件带 isComposing=true 会被跳过（组合中
  // dispatch 会炸掉组合），「/」经输入法组合上屏后，fix 依赖组合结束的这一次回调执行
  root.addEventListener('input', onWikiInput)
  root.addEventListener('compositionend', onWikiInput)
  root.addEventListener('keydown', onWikiKeydown, true)
  // 光标进入占位行时静默清掉占位符（prunePlaceholderAtCursor）：让「/」走 Crepe 原生
  // 触发路径——占位行被清空后输 / 不再需要 fix 的重建事务，那会与菜单项执行时内部
  // 记录的位置错位（点「一级标题」出现 // 段落、光标跳到下一段，实测）。selectionchange
  // 是 document 级事件，随 detachWikiListeners 一起清理
  document.addEventListener('selectionchange', prunePlaceholderAtCursor)
  detachWikiListeners = () => {
    root.removeEventListener('input', onWikiInput)
    root.removeEventListener('compositionend', onWikiInput)
    root.removeEventListener('keydown', onWikiKeydown, true)
    document.removeEventListener('selectionchange', prunePlaceholderAtCursor)
    detachWikiListeners = () => {}
  }
}

/**
 * 块把手「+」保持 Crepe 原生语义（在下方插入新段落 + 呼出菜单）。
 * 曾尝试改写为「当前行弹菜单」并拦下了 pointerup，最终撤销——Crepe 斜杠菜单的
 * shouldShow 有三个硬条件（block-edit 源码）：段落文本以 / **开头**、光标在**段末**、
 * 编程打开（programmaticallyPos）必须与光标**同节点**。「在含内容的当前行呼出菜单且
 * 不丢内容」与这三个条件互斥（实测：行首/段末插 / 均不弹，menuAPI 未从 @milkdown/crepe
 * 导出、自造 $ctx slice 的 key 与内部注册的不是同一 Symbol，编程打开也走不通）。
 * 勿再试此方向；「不换行」的需求由「/ 在空行输入」路径满足（prune + 原生触发）。
 */

/**
 * 清理光标所在段落的孤立指令字符 "/"：用户手输 / 后按 Esc 放弃指令的场景。
 * 仅当段落文本恰为 "/" 时删除；setTimeout 等菜单 hide 完成后再跑。
 */
function cleanupLoneSlashAtCursor() {
  const c = crepe
  if (!c || mode.value !== 'wysiwyg') return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    const { state } = view
    const { $from } = state.selection
    if ($from.depth === 0 || $from.parent.type.name !== 'paragraph') return
    if ($from.parent.textContent !== '/') return
    const tr = state.tr.delete($from.start(), $from.start() + 1)
    tr.setMeta('addToHistory', false)
    view.dispatch(tr)
  })
}

/**
 * 光标进入「纯占位符」空行时清掉占位符，让该行回到真空段落状态。
 * 时机的意义：占位符（保存时 normalize 填入）必须在用户**开始输入前**离场——
 * 输入后再改文档会与 Crepe 菜单/命令内部记录的位置竞争（实测点菜单项跳行）。
 * dispatch 加 addToHistory=false；仅当光标所在顶层段落文本恰好是单个占位符时触发，
 * 其余选区变化零开销。菜单打开时跳过（同 normalize 的守卫，见彼处注释）。
 */
function prunePlaceholderAtCursor() {
  const c = crepe
  if (!c || mode.value !== 'wysiwyg') return
  if (document.querySelector('.milkdown-slash-menu[data-show="true"]')) return
  c.editor.action((ctx) => {
    const view = ctx.get(editorViewCtx)
    if (view.composing) return
    const { state } = view
    const { $from } = state.selection
    if ($from.depth === 0 || $from.parent.type.name !== 'paragraph') return
    const text = $from.parent.textContent
    if (text !== '\u200B' && text !== '\u00A0') return
    const tr = state.tr.delete($from.start(), $from.start() + text.length)
    tr.setMeta('addToHistory', false)
    view.dispatch(tr)
  })
}

// ---- 标签 ----
async function persistTags() {
  if (!props.note || !isTauri()) return
  await tauriApi.setNoteTags(props.note.id, noteTags.value.map((t) => t.id))
  // set_note_tags 不发事件：就地刷新关联映射，树栏「标签」筛选立即跟上编辑器的增删
  void store.refreshNoteTagRows()
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
  // 来源分流：斜杠菜单「更多表情…」= 插入正文；标题旁的图标按钮 = 设置树图标
  if (emojiSource.value === 'title') {
    void store.setNoteIcon(props.note?.id ?? 0, e)
    return
  }
  insertEmojiText(e)
}

/** 树图标设置（emoji 来自与斜杠表情同一选择器）：icon 显示在标题输入框左侧与树行首 */
const emojiSource = ref<'slash' | 'title'>('slash')

function openTitleEmojiPicker() {
  if (!props.note) return
  emojiSource.value = 'title'
  emojiPickerVisible.value = true
}

function clearNoteIcon() {
  if (!props.note) return
  void store.setNoteIcon(props.note.id, null)
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
    <!-- 空状态：新建按钮 + 与设置同源的快捷键提示（按全局键直接新建，落点见提示行） -->
    <div v-if="!note" class="editor-empty">
      <p>选择或新建笔记</p>
      <button class="editor-empty-btn" type="button" @click="emit('create-note')">
        <Plus :size="14" :stroke-width="2" />
        新建笔记
      </button>
      <p class="editor-empty-kbd">
        <template v-if="notesShortcutHint">按 <kbd>{{ notesShortcutHint }}</kbd> 直接新建<template v-if="newNoteHint">，</template></template>{{ newNoteHint }}
      </p>
    </div>

    <!-- 编辑器内容 -->
    <template v-else>
      <header class="ed-header">
        <!-- 树图标（emoji）：点击选择 / 更换，右键清除；树行与搜索结果行首显示 -->
        <button
          v-if="note"
          class="icon-btn ed-icon-btn"
          :class="{ 'has-icon': !!note.icon }"
          :title="note.icon ? `树图标：${note.icon}（点击更换，右键清除）` : '设置树图标'"
          aria-label="设置树图标"
          @click="openTitleEmojiPicker"
          @contextmenu.prevent="clearNoteIcon"
        >
          <span v-if="note.icon" class="ed-icon-emoji">{{ note.icon }}</span>
          <Smile v-else :size="14" :stroke-width="1.8" />
        </button>
        <input
          v-model="localTitle"
          class="ed-title-input"
          type="text"
          maxlength="80"
          placeholder="笔记标题"
          @input="onTitleInput"
          @keydown.enter.prevent="($event.target as HTMLInputElement).blur()"
        />
        <button
          class="icon-btn ed-links-toggle"
          :class="{ on: linksPanelOpen }"
          :title="linksPanelOpen ? '收起双链面板' : '双链面板'"
          :aria-pressed="linksPanelOpen"
          aria-label="双链面板"
          @click="linksPanelOpen = !linksPanelOpen"
        >
          <Waypoints :size="14" :stroke-width="1.8" />
        </button>
        <button
          class="icon-btn ed-beautify-btn"
          type="button"
          title="一键美化格式"
          aria-label="一键美化格式"
          :disabled="!canBeautify"
          @click="onBeautify"
        >
          <Sparkles :size="14" :stroke-width="1.8" />
        </button>
        <button
          class="icon-btn ed-beautify-btn"
          type="button"
          title="AI 深度整理（语义重排，应用前可预览）"
          aria-label="AI 深度整理"
          :disabled="!canBeautify"
          @click="openAiTransform"
        >
          <WandSparkles :size="14" :stroke-width="1.8" />
        </button>
        <div class="mode-switch" role="radiogroup" aria-label="编辑模式">
          <button
            v-for="item in NOTE_EDITOR_MODES"
            :key="item.id"
            type="button"
            role="radio"
            :aria-checked="mode === item.id"
            :class="{ on: mode === item.id }"
            :title="item.label"
            :aria-label="item.label"
            @click="applyMode(item.id, true)"
          >
            <PencilLine v-if="item.id === 'wysiwyg'" :size="13" :stroke-width="2" />
            <Columns2 v-else-if="item.id === 'split'" :size="13" :stroke-width="2" />
            <Code2 v-else :size="13" :stroke-width="2" />
          </button>
        </div>
      </header>

      <!-- 来源链接（剪藏笔记） -->
      <div v-if="note.source_url" class="ed-source">
        <Link2 :size="12" :stroke-width="1.8" />
        <button
          class="ed-source-link"
          type="button"
          :title="`打开原文：${note.source_url}`"
          @click="openSourceUrl"
        >
          {{ note.source_url }}
        </button>
      </div>

      <!-- 编辑区 + 双链侧面板（全高） -->
      <div class="ed-body">
        <div class="ed-main">
          <div v-if="mode === 'wysiwyg'" ref="rootEl" class="crepe-root" @mousedown.capture="onEditorAreaMouseDown"></div>
          <textarea
            v-else-if="mode === 'source'"
            ref="sourceEl"
            class="md-source"
            :value="localContent"
            spellcheck="false"
            placeholder="输入 / 呼出更多功能…"
            aria-label="Markdown 源码"
            @input="onSourceInput"
            @keydown="onSourceKeydown"
          />
          <div v-else class="ed-split">
            <textarea
              ref="splitSourceEl"
              class="md-source"
              :value="localContent"
              spellcheck="false"
              placeholder="输入 / 呼出更多功能…"
              aria-label="Markdown 源码"
              @input="onSourceInput"
              @keydown="onSourceKeydown"
              @scroll="syncPreviewScroll"
            />
            <div
              v-if="localContent.trim()"
              ref="previewEl"
              class="md-preview"
              aria-label="预览"
              v-html="previewHtml"
              @click="onPreviewClick"
            />
            <p v-else class="md-preview md-preview-empty">输入 / 呼出更多功能…</p>
          </div>
        </div>

        <!-- 双链侧面板：入链/出链点击跳转 -->
        <aside v-if="linksPanelOpen" class="ed-links-panel" aria-label="双链面板">
          <header class="elp-head">
            <Waypoints :size="13" :stroke-width="1.8" class="elp-icon" />
            <span class="elp-title">双链</span>
            <span class="elp-count">{{ links.incoming.length }} 入 · {{ links.outgoing.length }} 出</span>
            <button
              class="icon-btn elp-close"
              type="button"
              title="收起"
              aria-label="收起双链面板"
              @click="linksPanelOpen = false"
            >
              <X :size="13" :stroke-width="2" />
            </button>
          </header>
          <div class="elp-body">
            <p v-if="links.incoming.length === 0 && links.outgoing.length === 0" class="elp-empty">
              正文输入 [[ 可引用其它笔记（按标题），这里会显示互相引用
            </p>
            <template v-else>
              <section v-if="links.incoming.length" class="elp-group">
                <h4 class="elp-label">入链 · 谁引用了本篇</h4>
                <button
                  v-for="l in links.incoming"
                  :key="`in-${l.from_note_id}`"
                  class="elp-chip"
                  type="button"
                  :title="`打开「${l.from_title}」`"
                  @click="emit('open-note', l.from_note_id)"
                >
                  {{ l.from_title }}
                </button>
              </section>
              <section v-if="links.outgoing.length" class="elp-group">
                <h4 class="elp-label">出链 · 本篇引用</h4>
                <template v-for="(l, i) in links.outgoing" :key="`out-${i}`">
                  <button
                    v-if="l.to_note_id != null"
                    class="elp-chip"
                    type="button"
                    :title="`打开「${l.to_title}」`"
                    @click="emit('open-note', l.to_note_id)"
                  >
                    {{ l.to_title }}
                  </button>
                  <span v-else class="elp-chip unresolved" :title="`「${l.to_title}」还不存在，创建同名笔记后自动解析`">
                    {{ l.to_title }}
                  </span>
                </template>
              </section>
            </template>
          </div>
        </aside>
(feat(notes): 代码块 Copy 按钮补「已复制」应用内提示)
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

    <!-- [[ 标题补全浮层（瞬态表面，Teleport 到 body） -->
    <Teleport to="body">
      <ul
        v-if="wikiSuggest"
        class="wiki-suggest"
        :style="{ left: `${wikiSuggest.x}px`, top: `${wikiSuggest.y}px` }"
        role="listbox"
        aria-label="笔记标题补全"
      >
        <li
          v-for="(item, i) in wikiSuggest.items"
          :key="item.id"
          role="option"
          :aria-selected="i === wikiSuggest.index"
          :class="{ on: i === wikiSuggest.index }"
          @mousedown.prevent="onWikiClickItem(item)"
          @mouseenter="wikiSuggest && (wikiSuggest.index = i)"
        >
          {{ item.title }}
        </li>
      </ul>
    </Teleport>

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

    <!-- AI 深度整理：流式预览 + 应用/放弃（瞬态表面，Teleport 到 body；Esc/遮罩关闭=放弃） -->
    <Teleport to="body">
      <div
        v-if="aiState !== 'idle'"
        class="ai-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="AI 深度整理"
        @click.self="closeAiDialog"
      >
        <div class="modal-card ai-card">
          <header class="ai-head">
            <WandSparkles :size="14" :stroke-width="1.8" class="ai-head-icon" />
            <span class="ai-title">AI 深度整理</span>
            <button class="icon-btn ai-close" type="button" title="关闭 (Esc)" aria-label="关闭" @click="closeAiDialog">
              <X :size="14" :stroke-width="2" />
            </button>
          </header>
          <p class="ai-privacy">
            笔记全文将发送给你配置的 AI 模型做语义重排；应用前不会改动笔记，应用后可 Ctrl+Z 一步撤销。
          </p>
          <div class="ai-body" aria-live="polite">
            <p v-if="aiState === 'streaming' && !aiResult" class="ai-status">正在整理…</p>
            <div v-else class="ai-preview md-preview" v-html="aiPreviewHtml"></div>
            <p v-if="aiImagesRestored" class="ai-restored">
              模型把图片改写成了纯网址，上方预览已按原样修回图片。
            </p>
            <p v-if="aiState === 'error'" class="ai-error">整理失败：{{ aiError }}（上面是已生成的部分，可关闭后重试）</p>
          </div>
          <footer class="ai-foot">
            <span v-if="aiState === 'streaming'" class="ai-streaming-hint">整理中…</span>
            <span v-else class="ai-streaming-hint"></span>
            <div class="ai-foot-actions">
              <button class="ai-btn" type="button" @click="closeAiDialog">取消</button>
              <button
                class="ai-btn primary"
                type="button"
                :disabled="aiState !== 'done' || !aiMarkdown"
                @click="applyAiResult"
              >
                应用到笔记
              </button>
            </div>
          </footer>
        </div>
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
/* 新建按钮与 ConfirmDialog 主按钮同款品牌样式 */
.editor-empty-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 16px;
  font-size: 0.8125em;
  font-weight: 600;
  border: 1px solid var(--brand-500);
  border-radius: var(--radius-md);
  background: var(--brand-500);
  color: #fff;
  cursor: pointer;
  transition: filter 0.18s, transform 0.1s;
}
.editor-empty-btn:hover {
  filter: brightness(1.06);
}
.editor-empty-btn:active {
  transform: scale(0.96);
}
.editor-empty-kbd {
  font-size: 0.75em;
  color: var(--text-4);
}
.editor-empty-kbd kbd {
  display: inline-block;
  padding: 1px 6px;
  border: 1px solid var(--border-strong);
  border-bottom-width: 2px;
  border-radius: 5px;
  background: var(--bg-card-soft);
  font-family: inherit;
  font-size: 0.9em;
  color: var(--text-2);
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

/* 来源链接（剪藏笔记） */
.ed-source {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: -6px 0 10px;
  color: var(--text-3);
  min-width: 0;
}
.ed-source-link {
  flex: 1;
  min-width: 0;
  text-align: left;
  border: none;
  background: transparent;
  color: var(--brand-500);
  font-size: 0.6875em;
  font-family: inherit;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding: 0;
  cursor: pointer;
}
.ed-source-link:hover {
  text-decoration: underline;
}

/* [[ 标题补全浮层（瞬态表面允许 backdrop-filter） */
.wiki-suggest {
  position: fixed;
  z-index: 210;
  margin: 0;
  padding: 4px;
  list-style: none;
  min-width: 180px;
  max-width: 300px;
  max-height: 240px;
  overflow-y: auto;
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
  backdrop-filter: blur(10px);
}
.wiki-suggest li {
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  font-size: 0.75em;
  color: var(--text-1);
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wiki-suggest li.on {
  background: var(--brand-50);
  color: var(--brand-500);
}

.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

/* 树图标按钮（emoji）：无图标时显示 Smile 线框，设置后显示 emoji 本体 */
.icon-btn.ed-icon-btn {
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  color: var(--text-3);
}
.icon-btn.ed-icon-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.ed-icon-btn.has-icon {
  font-size: 15px;
}
.ed-icon-emoji {
  line-height: 1;
}

.crepe-root {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
}

.crepe-root :deep(.milkdown) {
  height: 100%;
  min-width: 0;
  overflow-x: hidden;
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
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 26px;
  padding: 0;
  cursor: pointer;
}

.mode-switch button.on {
  background: var(--brand-50);
  color: var(--brand-500);
}

.mode-switch button:hover {
  color: var(--text-1);
}

/* 双链面板开关（头部图标，激活态品牌色） */
.icon-btn.ed-links-toggle {
  width: 28px;
  height: 28px;
  color: var(--text-3);
  flex-shrink: 0;
}
.icon-btn.ed-links-toggle:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.icon-btn.ed-links-toggle.on {
  color: var(--brand-500);
  background: var(--brand-50);
}

.icon-btn.ed-beautify-btn {
  width: 28px;
  height: 28px;
  color: var(--text-3);
  flex-shrink: 0;
}
.icon-btn.ed-beautify-btn:hover:not(:disabled) {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.icon-btn.ed-beautify-btn:disabled {
  color: var(--text-4);
  cursor: default;
  opacity: 0.6;
}

/* 编辑区 + 双链侧面板 */
.ed-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
}
.ed-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* 双链侧面板（全高，右侧） */
.ed-links-panel {
  flex-shrink: 0;
  width: 250px;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--border-soft);
  padding-left: 12px;
}
.elp-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.elp-icon {
  color: var(--brand-500);
  flex-shrink: 0;
}
.elp-title {
  font-size: 0.75em;
  font-weight: 600;
  color: var(--text-1);
}
.elp-count {
  flex: 1;
  min-width: 0;
  font-size: 0.6875em;
  color: var(--text-4);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.icon-btn.elp-close {
  width: 22px;
  height: 22px;
  color: var(--text-3);
}
.icon-btn.elp-close:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.elp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.elp-empty {
  margin: 0;
  color: var(--text-4);
  font-size: 0.6875em;
  line-height: 1.6;
}
.elp-group {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}
.elp-label {
  margin: 0 0 2px;
  font-size: 0.6875em;
  font-weight: 600;
  color: var(--text-4);
}
.elp-chip {
  max-width: 100%;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  color: var(--text-2);
  font-size: 0.6875em;
  font-family: inherit;
  border-radius: var(--radius-pill);
  padding: 3px 10px;
  cursor: pointer;
  transition: color 0.12s, border-color 0.12s;
}
.elp-chip:hover {
  color: var(--brand-500);
  border-color: var(--brand-500);
}
.elp-chip.unresolved {
  cursor: default;
  color: var(--text-4);
  border-style: dashed;
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

.md-preview :deep(input[type='checkbox']) {
  margin: 0 6px 0 0;
  accent-color: var(--brand-500);
  vertical-align: -2px;
}

/* 行内代码（分屏预览侧）：与编辑区同款中性灰薄纱底 + 基线对齐 + 光学抬升（口径见编辑区规则注释） */
.md-preview :deep(p code),
.md-preview :deep(li code) {
  background: color-mix(in srgb, var(--text-3) 16%, transparent);
  color: var(--code-text);
  display: inline-block;
  vertical-align: baseline;
  line-height: 1.2;
  padding: 0 5px;
  border-radius: 4px;
  position: relative;
  top: -0.18em;
}

.md-preview :deep(h1),
.md-preview :deep(h2),
.md-preview :deep(h3),
.md-preview :deep(h4),
.md-preview :deep(h5),
.md-preview :deep(h6) {
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
  /* 引用条走品牌色（用户要求随强调色）：半透明混合保证暗色/壁纸上不刺眼 */
  border-left: 3px solid color-mix(in srgb, var(--brand-500) 55%, transparent);
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

/* AI 深度整理弹层（Teleport 到 body，瞬态表面允许 backdrop-filter；表面复用全局 .modal-card） */
.ai-dialog {
  position: fixed;
  inset: 0;
  z-index: 200;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--scrim);
  backdrop-filter: blur(10px);
  animation: lb-in 0.18s ease-out;
}
.ai-card {
  width: min(720px, calc(100vw - 64px));
  height: min(640px, calc(100vh - 96px));
  display: flex;
  flex-direction: column;
  padding: 16px 20px;
}
.ai-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ai-head-icon {
  color: var(--brand-500);
}
.ai-title {
  font-size: 16px;
  font-weight: 650;
  color: var(--text-1);
  flex: 1;
}
.ai-privacy {
  margin-top: 6px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-3);
  flex-shrink: 0;
}
.ai-body {
  flex: 1;
  min-height: 0;
  margin-top: 10px;
  overflow-y: auto;
  /* 预览面交给 .md-preview 自带的边框与底色，避免双重描边 */
  display: flex;
  flex-direction: column;
}
.ai-status {
  font-size: 13px;
  color: var(--text-3);
}
.ai-error {
  margin-top: 10px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-red);
}
/* 图片语法被模型改写、已自动修回：中性提示（不是错误，用弱化字色 + 警示橙图标语义靠文案表达） */
.ai-restored {
  margin-top: 10px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-orange);
}
.ai-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 12px;
  flex-shrink: 0;
}
.ai-streaming-hint {
  font-size: 12px;
  color: var(--text-3);
}
.ai-foot-actions {
  display: flex;
  gap: 8px;
}
.ai-btn {
  padding: 6px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  cursor: pointer;
  transition: background 0.18s, color 0.18s, transform 0.18s;
}
.ai-btn:hover:not(:disabled) {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.ai-btn:active:not(:disabled) {
  transform: scale(0.96);
}
.ai-btn.primary {
  background: var(--brand-500);
  border-color: transparent;
  color: #fff;
}
.ai-btn.primary:hover:not(:disabled) {
  background: var(--brand-600);
  color: #fff;
}
.ai-btn:disabled {
  opacity: 0.5;
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
  --crepe-color-inline-code: var(--code-text);
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
  min-width: 0;
  overflow-wrap: anywhere;
}

/* 长行在栏宽内折行，不再把整块编辑区撑出横向滚动。
   代码块靠 white-space: break-spaces 让 CodeMirror 自己认出折行（光标、点击仍按视觉行计算）。 */
.crepe-root .milkdown .milkdown-code-block,
.crepe-root .milkdown .milkdown-table-block {
  max-width: 100%;
  min-width: 0;
}

/* 代码块卡片化：对齐分屏预览 .md-code 的观感。surface 默认映射 --input-bg，
   与 .crepe-root 背景同色，亮色下代码块「白底叠白底」整块隐形（只剩行号）；
   这里直接给卡片底/描边/圆角，CodeMirror 主体与行号槽改透出卡片底色。 */
.crepe-root .milkdown .milkdown-code-block {
  background: var(--bg-code);
  border: 1px solid var(--code-border);
  border-radius: 8px;
}

.crepe-root .milkdown .milkdown-code-block .cm-editor,
.crepe-root .milkdown .milkdown-code-block .cm-gutters {
  background: transparent;
}

/* 行号槽与内容并排：WebKitGTK 下 CM6 的 flex 布局会丢失，行号被堆到内容上方 */
.crepe-root .milkdown .milkdown-code-block .cm-scroller {
  display: flex;
  align-items: flex-start;
}
.crepe-root .milkdown .milkdown-code-block .cm-gutters {
  flex-shrink: 0;
}

/* 语言芯片与分屏预览的语言头同口径：常显（Crepe 默认 hover 才出现） */
.crepe-root .milkdown .milkdown-code-block .tools .language-button {
  opacity: 1;
}

/* 透底态：代码块跟随浮层换深玻璃实底（覆盖上面的 --bg-code 卡片底） */
html[data-wallpaper-clear='1'] .crepe-root .milkdown .milkdown-code-block {
  background: var(--crepe-color-surface);
  border-color: transparent;
}

.crepe-root .milkdown .cm-editor,
.crepe-root .milkdown .cm-scroller {
  max-width: 100%;
}

.crepe-root .milkdown .cm-scroller {
  overflow-x: hidden;
}

.crepe-root .milkdown .cm-content {
  flex-shrink: 1;
  max-width: 100%;
  white-space: break-spaces;
  word-break: break-word;
  overflow-wrap: anywhere;
}

.crepe-root .milkdown .milkdown-table-block table {
  width: 100%;
  table-layout: fixed;
}

.crepe-root .milkdown .milkdown-table-block th,
.crepe-root .milkdown .milkdown-table-block td {
  overflow-wrap: anywhere;
  word-break: break-word;
}

.crepe-root .milkdown .katex-display {
  max-width: 100%;
  overflow-x: auto;
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

/* 行内代码：底色用中性灰薄纱（text-3 的 16% 透明），任何主题/壁纸下都柔和可读——
   亮色 --bg-code 接近白，叠在暗色壁纸上刺眼（用户两轮反馈「太白」）；
   字色保持 --code-text 随主题。分屏预览侧同款。
   display/vertical-align/line-height 让内联 code 与正文基线对齐——默认 inline 的
   padding 会把字撑出基线，视觉偏上不居中（用户反馈）。
   top: -0.18em 光学抬升：基线对齐只锁「字」，药丸盒子仍按等宽字体的深 descender
   分布（顶 −11px / 底 +4.75px @正文15px），整颗下沉、底边超中文墨迹 ~3.8px——
   relative top 只做视觉位移不动布局，实测把药丸中心对到中文墨迹中心（偏心
   2.38px → 0.02px），上下缘对称留 ~1.4px 呼吸；em 跟随 code 字号，--fs-notes
   缩放与标题内 code 均自适应（2026-10-05 用户反馈「行内代码不与左右文字垂直居中」） */
.crepe-root .milkdown .ProseMirror p code,
.crepe-root .milkdown .ProseMirror code {
  background: color-mix(in srgb, var(--text-3) 16%, transparent) !important;
  color: var(--code-text) !important;
  display: inline-block;
  vertical-align: baseline;
  line-height: 1.2;
  padding: 0 5px;
  border-radius: 4px;
  position: relative;
  top: -0.18em;
}

/* 引用块：Crepe 默认 padding-left 40px，文字离左侧引用条太远，收紧到贴条显示；
   引用条走品牌色（用户要求随强调色，与分屏预览同款）。Crepe reset.css 另有一个
   ::before 4px 圆角条（--crepe-color-selected）——必须隐藏，否则出现两条色条
   （用户实测反馈「两个色块」），只保留这一条品牌色边框 */
.crepe-root .milkdown .ProseMirror blockquote {
  padding-left: 12px;
  border-left: 3px solid color-mix(in srgb, var(--brand-500) 55%, transparent);
}
.crepe-root .milkdown .ProseMirror blockquote::before {
  content: none;
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

<style>
/* 透底态（壁纸+透明）的行内代码：薄纱底叠暗壁纸对比不足（用户反馈「文字看不清」），
   换深玻璃底 + 白字，与编辑器浮层透底态同款。⚠️ 非 scoped 块写裸选择器——:global()
   只在 scoped 块里有意义，写在这里整条规则会编译成无效选择器（踩过） */
html[data-wallpaper-clear='1'] .crepe-root .milkdown .ProseMirror code {
  background: rgba(28, 29, 41, 0.62) !important;
  color: rgba(255, 255, 255, 0.92) !important;
}
html[data-wallpaper-clear='1'] .md-preview p code,
html[data-wallpaper-clear='1'] .md-preview li code {
  background: rgba(28, 29, 41, 0.62);
  color: rgba(255, 255, 255, 0.92);
}
</style>
