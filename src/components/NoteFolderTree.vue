<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from 'vue'
import {
  Archive,
  ArchiveRestore,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Eraser,
  FilePlus2,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  Plus,
  Search,
  Trash2,
  Undo2,
  X,
} from 'lucide-vue-next'
import type { Note, NoteFolder, Tag } from '../api/tauri'
import { useStore } from '../stores/workbench'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'

/**
 * 速记视图左栏 = **唯一导航面**（两栏布局，2026-10-04 用户反馈砍掉中栏列表）：文件夹树 +
 * 树根笔记（docs/speednote-plan.md §5.1，ADR 0015），并承载搜索 / 标签筛选 / 回收站。
 * - 搜索框默认收起，树头搜索 icon 切换；输入 `#` 进入标签筛选（弹出标签列表选择），
 *   标签行 UI 已移除（2026-10-04 用户反馈：标签并入搜索）。
 * - 头部「新建目录」跟随选中文件夹（未选中 = 树根）；树空白处右键可新建笔记/目录。
 * - 回收站态带「一键清空」（SpeednoteView 层确认后执行，不可恢复）。
 * - 拖拽为**指针实现**（约定 14/38），渲染为扁平化可见行列表（无递归组件）。
 */

const props = defineProps<{
  folders: readonly NoteFolder[]
  notes: readonly Note[]
  /** 回收站列表（SpeednoteView 在进入回收站态时拉取） */
  trashedNotes: readonly Note[]
  selectedFolderId: number | null
  activeNoteId: number | null
}>()

const emit = defineEmits<{
  (e: 'select-folder', id: number | null): void
  (e: 'select-note', id: number): void
  (e: 'create-note'): void
  (e: 'create-note-in', folderId: number): void
  (e: 'create-folder', parentId: number | null, name: string): void
  (e: 'rename-folder', id: number, name: string): void
  (e: 'delete-folder', id: number): void
  (e: 'trash-note', id: number): void
  (e: 'restore-note', id: number): void
  (e: 'purge-note', id: number): void
  (e: 'purge-all'): void
  (e: 'move-note', noteId: number, folderId: number | null): void
  (e: 'reorder-folders', moves: { id: number; parent_id: number | null; sort_order: number }[]): void
}>()

/** 搜索词：'#xxx' = 标签筛选（x 匹配标签名），其余 = 全文搜索 */
const query = defineModel<string>('query', { default: '' })
const trashMode = defineModel<boolean>('trashMode', { default: false })

const store = useStore()

// ---- 搜索框显隐（默认收起，树头 icon 切换；有筛选词时保持展开） ----
const searchOpen = ref(false)
const searchInput = ref<HTMLInputElement | null>(null)
const searching = computed(() => !trashMode.value && query.value.trim().length > 0)

function toggleSearch() {
  searchOpen.value = !searchOpen.value
  if (searchOpen.value) {
    void nextTick(() => searchInput.value?.focus())
  } else {
    query.value = ''
    tagPickOpen.value = false
    hintOpen.value = false
  }
}

/** 聚焦且为空时显示用法提示（参考 macOS 备忘录式的前缀提示） */
const hintOpen = ref(false)
function onSearchFocus() {
  hintOpen.value = query.value.length === 0
}
function onSearchBlur() {
  // 延迟关闭：给下拉/提示里的 mousedown（先于 blur）留出处理窗口。
  // 搜索词为空时连搜索框一起收起（点击其他地方 = 收起，用户反馈）
  setTimeout(() => {
    hintOpen.value = false
    tagPickOpen.value = false
    if (!query.value) searchOpen.value = false
  }, 150)
}

// ---- 标签筛选（# 语法；标签行 UI 已移除） ----
const tagPickOpen = ref(false)
const tagPickIndex = ref(0)
const activeTagId = ref<number | null>(null)

const tagMap = computed(() => {
  const map = new Map<number, number[]>()
  for (const row of store.state.noteTagRows) {
    const list = map.get(row.note_id) ?? []
    list.push(row.tag_id)
    map.set(row.note_id, list)
  }
  return map
})

function tagCount(tagId: number): number {
  const ids = tagMap.value.get(tagId)
  if (!ids) return 0
  return props.notes.filter((n) => ids.includes(n.id)).length
}

/** # 后的过滤词 */
const tagQueryText = computed(() =>
  query.value.startsWith('#') ? query.value.slice(1).trim() : '',
)

const tagCandidates = computed<Tag[]>(() => {
  const q = tagQueryText.value.toLowerCase()
  return store.state.tags.filter((t) => (q ? t.name.toLowerCase().includes(q) : true))
})

watch(query, (q) => {
  if (trashMode.value) return
  if (q.startsWith('#')) {
    const exact = store.state.tags.find((t) => t.name === q.slice(1).trim())
    if (exact && activeTagId.value !== exact.id) {
      // 精确命中标签名：进入该标签的筛选态，下拉收起
      activeTagId.value = exact.id
      tagPickOpen.value = false
      hintOpen.value = false
    } else if (!exact) {
      // 非精确：仍在挑选中（下拉列出过滤结果）
      activeTagId.value = null
      tagPickOpen.value = true
      tagPickIndex.value = 0
      hintOpen.value = false
    }
  } else {
    activeTagId.value = null
    tagPickOpen.value = false
    hintOpen.value = q.length === 0
  }
})

function pickTag(t: Tag) {
  activeTagId.value = t.id
  query.value = `#${t.name}`
  tagPickOpen.value = false
  hintOpen.value = false
  void nextTick(() => searchInput.value?.focus())
}

function onSearchKeydown(e: KeyboardEvent) {
  if (tagPickOpen.value && tagCandidates.value.length > 0) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      tagPickIndex.value = (tagPickIndex.value + 1) % tagCandidates.value.length
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      tagPickIndex.value =
        (tagPickIndex.value - 1 + tagCandidates.value.length) % tagCandidates.value.length
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      pickTag(tagCandidates.value[tagPickIndex.value])
      return
    }
  }
  if (e.key === 'Escape') {
    e.preventDefault()
    query.value = ''
    searchInput.value?.blur()
  }
}

const filterActive = computed(() => searching.value || activeTagId.value != null)

/** 全文匹配防抖（约定 10 口径 300ms）：输入框与标签下拉即时响应，重扫描
 *  （全部笔记 title+content）延迟跟进，大树击键不卡顿 */
const searchDebounced = ref(query.value)
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null
watch(query, (q) => {
  if (searchDebounceTimer !== null) clearTimeout(searchDebounceTimer)
  searchDebounceTimer = setTimeout(() => {
    searchDebounceTimer = null
    searchDebounced.value = q
  }, 300)
})

/** 筛选结果：标签态按标签映射，否则全文（均跨全部文件夹，创建时间正序） */
const filterResults = computed<Note[]>(() => {
  const sorted = [...props.notes].sort(
    (a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id,
  )
  if (activeTagId.value != null) {
    const ids = tagMap.value.get(activeTagId.value)
    if (!ids) return []
    return sorted.filter((n) => ids.includes(n.id))
  }
  const q = searchDebounced.value.trim().toLowerCase()
  if (!q) return []
  return sorted.filter(
    (n) => n.title.toLowerCase().includes(q) || n.content.toLowerCase().includes(q),
  )
})

/** 笔记所在文件夹路径（结果行标注；树根返回空串由模板兜底显示「根目录」）。
 *  id → 完整路径预建一次：每行渲染逐级 find 爬父链在结果多时是 O(行数 × 深度) */
const folderPathById = computed(() => {
  const byId = new Map(props.folders.map((f) => [f.id, f]))
  const paths = new Map<number, string>()
  const resolve = (f: NoteFolder): string => {
    const cached = paths.get(f.id)
    if (cached !== undefined) return cached
    const parent = f.parent_id != null ? byId.get(f.parent_id) : undefined
    const p = parent ? `${resolve(parent)} / ${f.name}` : f.name
    paths.set(f.id, p)
    return p
  }
  for (const f of props.folders) resolve(f)
  return paths
})

function folderPathOf(n: Note): string {
  return n.folder_id != null ? (folderPathById.value.get(n.folder_id) ?? '') : ''
}

// ---- 树构建 + 扁平化 ----
interface TreeNode {
  folder: NoteFolder
  children: TreeNode[]
  notes: Note[]
}

const childrenMap = computed(() => {
  const map = new Map<number | null, NoteFolder[]>()
  for (const f of props.folders) {
    const list = map.get(f.parent_id) ?? []
    list.push(f)
    map.set(f.parent_id, list)
  }
  for (const list of map.values()) {
    list.sort(
      (a, b) => a.sort_order - b.sort_order || a.created_at.localeCompare(b.created_at) || a.id - b.id,
    )
  }
  return map
})

const tree = computed<TreeNode[]>(() => {
  const build = (parentId: number | null): TreeNode[] => {
    return (childrenMap.value.get(parentId) ?? []).map((f) => ({
      folder: f,
      children: build(f.id),
      notes: props.notes
        .filter((n) => n.folder_id === f.id)
        .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id),
    }))
  }
  return build(null)
})

const rootNotes = computed(() =>
  props.notes
    .filter((n) => n.folder_id === null)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id - b.id),
)

type Row =
  | { kind: 'folder'; folder: NoteFolder; depth: number }
  | { kind: 'note'; note: Note; depth: number }
  /** 行内新建文件夹输入框：紧跟在目标父文件夹行之后（parentId=-1 = 最后一个顶层文件夹之后） */
  | { kind: 'create'; parentId: number; depth: number }

const expanded = reactive(new Set<number>())
function isExpanded(id: number): boolean {
  return expanded.has(id)
}
function toggleExpand(id: number) {
  if (expanded.has(id)) expanded.delete(id)
  else expanded.add(id)
}

/** 全部展开 / 全部收起（树头按钮）：收起保留选中文件夹自身展开态无必要——
 *  全收起后树只剩顶层，选中的深层文件夹会不可见，故收起时仅当选中文件夹在
 *  深层时保留其祖先链展开（光标锚点不丢失） */
function expandAll() {
  props.folders.forEach((f) => expanded.add(f.id))
}

/** 是否存在「可收起」的内容：任一文件夹处于展开态且其下有子文件夹/子笔记。
 *  决定树头展开/收起切换按钮的图标与动作（合成一个按钮，用户反馈） */
const hasCollapsible = computed(() => {
  const childCount = (parentId: number | null): number =>
    (childrenMap.value.get(parentId)?.length ?? 0) +
    props.notes.filter((n) => n.folder_id === parentId).length
  return props.folders.some((f) => isExpanded(f.id) && childCount(f.id) > 0)
})

function collapseAll() {
  const keep = new Set<number>(chainsToKeep())
  expanded.clear()
  keep.forEach((id) => expanded.add(id))
}

/** 点击文件夹行：选中并切换展开/收起（文件树惯例，VS Code 同款） */
function onFolderClick(f: NoteFolder) {
  toggleExpand(f.id)
  emit('select-folder', f.id)
}

const rows = computed<Row[]>(() => {
  const out: Row[] = []
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const node of nodes) {
      out.push({ kind: 'folder', folder: node.folder, depth })
      // 「+」新建输入框就地插在该文件夹正下方（无论展开与否都能看到）
      if (creatingIn.value === node.folder.id) {
        out.push({ kind: 'create', parentId: node.folder.id, depth: depth + 1 })
      }
      if (isExpanded(node.folder.id)) {
        walk(node.children, depth + 1)
        for (const n of node.notes) out.push({ kind: 'note', note: n, depth: depth + 1 })
      }
    }
  }
  walk(tree.value, 0)
  // 树根层新建：插在最后一个顶层文件夹子树之后（树根笔记之前），不沉底
  if (creatingIn.value === -1) out.push({ kind: 'create', parentId: -1, depth: 0 })
  // 树根层：文件夹在前（rows 已含），笔记在后
  for (const n of rootNotes.value) out.push({ kind: 'note', note: n, depth: 0 })
  return out
})

// 树默认**收起**（用户反馈）：仅展开「选中文件夹 / 打开笔记」的祖先链，
// 避免选中项被藏进收起子树。collapseAll 复用同一链计算。
const everInitialized = ref(false)
function ancestorChainOf(id: number): number[] {
  const chain: number[] = []
  let cursor: number | null = id
  let hops = 0
  while (cursor != null && hops <= props.folders.length) {
    chain.push(cursor)
    cursor = props.folders.find((f) => f.id === cursor)?.parent_id ?? null
    hops += 1
  }
  return chain
}
function chainsToKeep(): number[] {
  const ids = new Set<number>()
  if (props.selectedFolderId != null) ancestorChainOf(props.selectedFolderId).forEach((x) => ids.add(x))
  const active = props.notes.find((x) => x.id === props.activeNoteId)
  if (active?.folder_id != null) ancestorChainOf(active.folder_id).forEach((x) => ids.add(x))
  return [...ids]
}
watch(
  () => props.folders.length,
  (n) => {
    if (everInitialized.value || n === 0) return
    everInitialized.value = true
    chainsToKeep().forEach((id) => expanded.add(id))
  },
  { immediate: true },
)

// ---- 行内新建 / 重命名 ----
const creatingIn = ref<number | null>(null) // null=不在新建；数字=父文件夹 id；-1=树根
const createName = ref('')
const renamingId = ref<number | null>(null)
const renameValue = ref('')

function startCreate(parentId: number | null) {
  creatingIn.value = parentId === null ? -1 : parentId
  createName.value = ''
  // 输入框渲染在 rows 流里（v-for 内不能用模板 ref——会聚成数组），就地处聚焦
  void nextTick(() => document.querySelector<HTMLInputElement>('.nft-new-input')?.focus())
}

/** 头部「新建目录」：点了文件夹就建在它下面，没点 = 树根 */
function startCreateAtSelection() {
  startCreate(props.selectedFolderId)
}

function commitCreate() {
  const name = createName.value.trim()
  const parent = creatingIn.value
  creatingIn.value = null
  if (!name || parent == null) return
  const parentId = parent === -1 ? null : parent
  if (parentId != null) expanded.add(parentId)
  emit('create-folder', parentId, name)
}

function startRename(f: NoteFolder) {
  if (f.builtin) return
  renamingId.value = f.id
  renameValue.value = f.name
  void nextTick(() => document.querySelector<HTMLInputElement>('.nft-rename-input')?.focus())
}

function commitRename() {
  const id = renamingId.value
  const name = renameValue.value.trim()
  renamingId.value = null
  if (!id || !name) return
  emit('rename-folder', id, name)
}

function folderName(id: number | null): string {
  if (id === null || id === -1) return '根目录'
  return props.folders.find((f) => f.id === id)?.name ?? '根目录'
}

// ---- 右键菜单 ----
const ctx = ref<{ visible: boolean; x: number; y: number; items: ContextMenuItem[] }>({
  visible: false,
  x: 0,
  y: 0,
  items: [],
})

function openFolderMenu(f: NoteFolder, e: MouseEvent) {
  e.preventDefault()
  e.stopPropagation() // 不冒泡到树体的空白菜单，避免覆盖
  emit('select-folder', f.id)
  const items: ContextMenuItem[] = [
    { label: '新建笔记', onClick: () => emit('create-note-in', f.id) },
    { label: '新建子文件夹', onClick: () => startCreate(f.id) },
  ]
  if (!f.builtin) {
    items.push(
      { label: '重命名', onClick: () => startRename(f) },
      {
        label: '删除文件夹',
        danger: true,
        dividerBefore: true,
        onClick: () => emit('delete-folder', f.id),
      },
    )
  }
  ctx.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

function openNoteMenu(n: Note, e: MouseEvent) {
  e.preventDefault()
  e.stopPropagation()
  emit('select-note', n.id)
  ctx.value = {
    visible: true,
    x: e.clientX,
    y: e.clientY,
    items: [{ label: '移入回收站', danger: true, onClick: () => emit('trash-note', n.id) }],
  }
}

/** 点击树空白区域 = 取消文件夹选中（回树根）：选中文件夹后头部「新建目录」跟随
 *  选中建子目录，用户需要一条明确的退路回树根（树头「速记」点击同义，实测反馈） */
function onBodyClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  if (target.closest('.nft-row, input, .nft-new-input, .nft-search, .nft-search-pop')) return
  emit('select-folder', null)
}

/** 树空白/树头右键：在当前选中文件夹（默认树根）下新建。
 *  行上的右键已由各自菜单处理并 stopPropagation，这里只兜真正的空白处 */
function openBodyMenu(e: MouseEvent) {  e.preventDefault()
  const target = e.target as HTMLElement | null
  if (target?.closest('.nft-row, input, button, .nft-search')) return
  const targetFolder = props.selectedFolderId
  const items: ContextMenuItem[] = []
  if (targetFolder != null) {
    const name = folderName(targetFolder)
    items.push(
      { label: `新建笔记（在「${name}」）`, onClick: () => emit('create-note-in', targetFolder) },
      { label: `新建子文件夹（在「${name}」）`, onClick: () => startCreate(targetFolder) },
    )
  } else {
    items.push(
      { label: '新建笔记', onClick: () => emit('create-note') },
      { label: '新建目录', onClick: () => startCreate(null) },
    )
  }
  ctx.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

// ---- 标签管理（右键 chip；内置标签后端拒绝）——入口已并入 # 搜索下拉的行内右键 ----
const renamingTagId = ref<number | null>(null)
const tagNameValue = ref('')

function openTagMenu(tag: Tag, e: MouseEvent) {
  e.preventDefault()
  e.stopPropagation()
  if (tag.builtin) return
  const items: ContextMenuItem[] = [
    {
      label: '重命名',
      onClick: () => {
        renamingTagId.value = tag.id
        tagNameValue.value = tag.name
        void nextTick(() => document.querySelector<HTMLInputElement>('.nft-tag-input')?.focus())
      },
    },
    {
      label: '删除标签',
      danger: true,
      dividerBefore: true,
      onClick: () => {
        if (activeTagId.value === tag.id) {
          activeTagId.value = null
          query.value = ''
        }
        void store.deleteTag(tag.id)
      },
    },
  ]
  ctx.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

function commitTagRename() {
  const id = renamingTagId.value
  const name = tagNameValue.value.trim()
  renamingTagId.value = null
  if (!id || !name) return
  void store.renameTag(id, name)
  // 正在以旧名筛选时同步搜索框显示
  if (activeTagId.value === id) query.value = `#${name}`
}

// ---- 键盘（F2/Delete 作用于选中文件夹；Delete 作用于打开中的笔记） ----
function onTreeKeydown(e: KeyboardEvent) {
  if (renamingId.value != null || creatingIn.value != null) return
  if (e.key === 'Delete' && !trashMode.value && props.activeNoteId != null) {
    // 焦点在树容器时，Delete 优先作用于选中的文件夹；没有则作用于打开中的笔记
    const selected = props.selectedFolderId
    if (selected != null) {
      const f = props.folders.find((x) => x.id === selected)
      if (f) {
        if (f.builtin) return
        e.preventDefault()
        emit('delete-folder', f.id)
        return
      }
    }
    e.preventDefault()
    emit('trash-note', props.activeNoteId)
    return
  }
  if (props.selectedFolderId == null) return
  const f = props.folders.find((x) => x.id === props.selectedFolderId)
  if (!f) return
  if (e.key === 'F2') {
    e.preventDefault()
    startRename(f)
  }
}

// ---- 指针拖拽（非 HTML5 DnD，见组件头注释） ----
type DragItem = { kind: 'note'; id: number } | { kind: 'folder'; id: number }
type DropHint = { rowKey: string; mode: 'into' | 'before' | 'after' | 'root' }
interface DropPlan {
  kind: 'into' | 'before' | 'after' | 'root'
  targetFolderId: number | null
  targetId: number | null
  /** 落点提示的渲染目标行：必须跟着光标命中的行走——提示打在别处
   *  （如父文件夹行/树头）时目标行一滚出可视区，落点指示就整体消失（实测反馈） */
  hint: DropHint
}
let dragItem: DragItem | null = null
let dragStart: { x: number; y: number } | null = null
let dragPlan: DropPlan | null = null
const dragActive = ref(false)
const dragPos = ref({ x: 0, y: 0 })
const dragLabel = ref('')
const dropHint = ref<DropHint | null>(null)
/** ghost 浮签上的落点名（「→ 个人」）：唯一保证任何滚动位置都可见的落点反馈 */
const dragTargetName = ref('')

function folderRowKey(id: number) {
  return `folder:${id}`
}
function noteRowKey(id: number) {
  return `note:${id}`
}

function isDescendant(folderId: number, maybeAncestorId: number): boolean {
  // folderId 的祖先链里是否含 maybeAncestorId（把 folderId 拖进 its 后代必成环）
  let cursor: number | null = folderId
  let hops = 0
  while (cursor != null && hops <= props.folders.length + 1) {
    if (cursor === maybeAncestorId) return true
    cursor = props.folders.find((f) => f.id === cursor)?.parent_id ?? null
    hops += 1
  }
  return false
}

function onRowPointerDown(e: PointerEvent, item: DragItem, label: string) {
  if (e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('input, button')) return
  if (trashMode.value || filterActive.value) return // 回收站/搜索态没有文件夹落点
  dragItem = item
  dragLabel.value = label
  dragStart = { x: e.clientX, y: e.clientY }
  rowTopCache = null
  window.addEventListener('scroll', invalidateRowTopCache, true)
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragUp)
}

function onDragMove(e: PointerEvent) {
  if (!dragItem || !dragStart) return
  // 鼠标移出窗口松键不会派发 pointerup：buttons 归零即视为结束（同 blockDrag 口径）
  if (!e.buttons) {
    finishDrag()
    return
  }
  if (!dragActive.value) {
    if (Math.hypot(e.clientX - dragStart.x, e.clientY - dragStart.y) < 6) return
    dragActive.value = true
  }
  dragPos.value = { x: e.clientX, y: e.clientY }
  dragPlan = hitTest(e)
  dropHint.value = dragPlan?.hint ?? null
  dragTargetName.value = planTargetName(dragPlan)
}

/** ghost 浮签的落点名：into/before/after = 目标文件夹名，root = 根目录 */
function planTargetName(plan: DropPlan | null): string {
  if (!plan) return ''
  if (plan.kind === 'root') return '根目录'
  return props.folders.find((f) => f.id === plan.targetId)?.name ?? ''
}

/** 拖拽会话内的行顶距缓存：hitTest 行间隙命中时按缓存取「上方最近行」，
 *  不再每帧对全部行做 getBoundingClientRect；滚动即失效（capture 捕获树体内滚动） */
let rowTopCache: { el: HTMLElement; top: number }[] | null = null
function invalidateRowTopCache() {
  rowTopCache = null
}
function collectRowTops(body: Element): { el: HTMLElement; top: number }[] {
  if (rowTopCache) return rowTopCache
  rowTopCache = Array.from(body.querySelectorAll<HTMLElement>('.nft-row'))
    .map((el) => ({ el, top: el.getBoundingClientRect().top }))
    .sort((a, b) => a.top - b.top)
  return rowTopCache
}

function hitTest(e: PointerEvent): DropPlan | null {
  const el = document.elementFromPoint(e.clientX, e.clientY)
  if (!el) return null
  // 树头（速记标题区）= 树根
  if (el.closest('.nft-header')) {
    return { kind: 'root', targetFolderId: null, targetId: null, hint: { rowKey: 'root', mode: 'root' } }
  }
  const row = el.closest<HTMLElement>('[data-drop-folder]')
  if (row) {
    const fid = Number(row.dataset.dropFolder)
    const rect = row.getBoundingClientRect()
    const rel = (e.clientY - rect.top) / rect.height
    if (dragItem?.kind === 'folder') {
      if (fid === dragItem.id) return null
      // 文件夹拖文件夹：整行默认「移入子目录」，贴近行上/下边缘（15%）才是同层排序
      if (rel < 0.15) return { kind: 'before', targetFolderId: null, targetId: fid, hint: { rowKey: folderRowKey(fid), mode: 'before' } }
      if (rel > 0.85) return { kind: 'after', targetFolderId: null, targetId: fid, hint: { rowKey: folderRowKey(fid), mode: 'after' } }
    }
    return { kind: 'into', targetFolderId: fid, targetId: fid, hint: { rowKey: folderRowKey(fid), mode: 'into' } }
  }
  // 行外（行间隙/树体空白）：取「鼠标上方最近的行」判落点——
  //  - 文件夹行 = 移入该文件夹（「拖到目录下面就应该进目录」，用户口径，实测反馈）
  //  - 笔记行 = 移入该笔记所在文件夹；根目录笔记行即**树根落点**——树渲染为
  //    「文件夹在前、根笔记在后」，底部笔记区/空白区此前被「上方最近文件夹」
  //    吞成 into，加上笔记分支不处理 root，形同「目录/笔记拖不回根目录」（实测反馈）
  const body = el.closest('.nft-body')
  if (!body || dragItem == null) return null
  // 行几何在拖拽会话内不变（滚动即失效重采）：缓存顶距避免每帧全量 getBoundingClientRect
  let nearest: HTMLElement | null = null
  let nearestTop = Number.NEGATIVE_INFINITY
  for (const { el: r, top } of collectRowTops(body)) {
    if (dragItem.kind === 'folder' && r.dataset.dropFolder === String(dragItem.id)) continue // 自己不算
    if (dragItem.kind === 'note' && r.dataset.noteId === String(dragItem.id)) continue
    if (top <= e.clientY && top >= nearestTop) {
      nearest = r
      nearestTop = top
    }
  }
  if (nearest?.hasAttribute('data-drop-folder')) {
    const fid = Number(nearest.dataset.dropFolder)
    return { kind: 'into', targetFolderId: fid, targetId: fid, hint: { rowKey: folderRowKey(fid), mode: 'into' } }
  }
  if (nearest?.hasAttribute('data-note-id')) {
    // 笔记行的 data-note-folder 在树根笔记上为 null → Vue 移除该属性，故以 data-note-id
    // 认定笔记行、再读 folder 值：'' / 缺值 = 树根落点
    const raw = nearest.dataset.noteFolder
    const nid = Number(nearest.dataset.noteId)
    const fid = raw == null || raw === '' ? null : Number(raw)
    const hint = { rowKey: noteRowKey(nid), mode: 'into' as const }
    // 根目录笔记行 = 树根落点：提示打在该笔记行上。树头（另一处 root 提示）在树体之外、
    // 长树滚到底时不在光标附近，光靠它等于「底部拖拽看不到落点」（实测反馈）
    if (fid == null || Number.isNaN(fid)) return { kind: 'root', targetFolderId: null, targetId: null, hint }
    return { kind: 'into', targetFolderId: fid, targetId: fid, hint }
  }
  // 无邻近行（树体最顶部/底部空白）= 树根：提示打树头（此时树头就在光标上方不远处）
  return { kind: 'root', targetFolderId: null, targetId: null, hint: { rowKey: 'root', mode: 'root' } }
}

function onDragUp() {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  window.removeEventListener('scroll', invalidateRowTopCache, true)
  rowTopCache = null
  const item = dragItem
  const plan = dragPlan
  const wasActive = dragActive.value
  finishDrag()
  if (!wasActive || !item || !plan) return

  if (item.kind === 'note') {
    if (plan.kind === 'into') {
      emit('move-note', item.id, plan.targetFolderId ?? null)
      // 目标目录多为收起态：不展开的话笔记落进去就「凭空消失」，观感即「没生效」
      if (plan.targetFolderId != null) expanded.add(plan.targetFolderId)
    } else if (plan.kind === 'root') {
      emit('move-note', item.id, null)
    }
    return
  }

  // ---- 文件夹移动 ----
  const moving = props.folders.find((f) => f.id === item.id)
  if (!moving) return
  if (plan.kind === 'into' || plan.kind === 'root') {
    const parentId = plan.kind === 'root' ? null : plan.targetFolderId
    if (parentId != null && (parentId === item.id || isDescendant(parentId, item.id))) return // 环，忽略
    if (moving.parent_id === parentId && plan.kind === 'root') return
    const siblings = (childrenMap.value.get(parentId) ?? []).filter((f) => f.id !== item.id)
    const moves = [
      ...siblings.map((f, i) => ({ id: f.id, parent_id: parentId, sort_order: i })),
      { id: item.id, parent_id: parentId, sort_order: siblings.length },
    ]
    emit('reorder-folders', moves)
    // 同上：拖进目标目录后展开它，落点结果当场可见（同 commitCreate 的先例）
    if (plan.kind === 'into' && plan.targetFolderId != null) expanded.add(plan.targetFolderId)
    return
  }
  // 同层 before / after
  const targetId = plan.targetId
  if (targetId == null || targetId === item.id) return
  const target = props.folders.find((f) => f.id === targetId)
  if (!target) return
  if (isDescendant(targetId, item.id)) return // 拖到自己后代前后 = 成环，忽略
  const parentId = target.parent_id
  const siblings = (childrenMap.value.get(parentId) ?? []).filter((f) => f.id !== item.id)
  const idx = siblings.findIndex((f) => f.id === targetId)
  if (idx < 0) return
  const ordered = siblings.map((f) => ({ id: f.id, parent_id: parentId, sort_order: 0 }))
  ordered.splice(plan.kind === 'before' ? idx : idx + 1, 0, {
    id: item.id,
    parent_id: parentId,
    sort_order: 0,
  })
  emit(
    'reorder-folders',
    ordered.map((m, i) => ({ ...m, sort_order: i })),
  )
}

function finishDrag() {
  dragItem = null
  dragStart = null
  dragPlan = null
  dragActive.value = false
  dropHint.value = null
  dragTargetName.value = ''
}

function hintClass(row: Row): string {
  const hint = dropHint.value
  if (!hint || row.kind === 'create') return ''
  const key = row.kind === 'folder' ? folderRowKey(row.folder.id) : noteRowKey(row.note.id)
  if (hint.rowKey !== key) return ''
  // root 落点（回根目录）落在笔记行上时用「移入」同款高亮反馈：树头虚线框在树体之外，
  // 长树滚到底时光标附近什么都没有，等于「看不到落点」（实测反馈）
  return `nft-drop-${hint.mode === 'root' ? 'into' : hint.mode}`
}

function onRowKeydownRow(e: KeyboardEvent, row: Row) {
  if (row.kind === 'note' && e.key === 'Enter') {
    e.preventDefault()
    emit('select-note', row.note.id)
  }
}

// 拖拽窗口级监听随组件卸载清理：拖到一半切出速记视图时不能把监听泄漏到松键
onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragUp)
  window.removeEventListener('scroll', invalidateRowTopCache, true)
  if (searchDebounceTimer !== null) {
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = null
  }
})
</script>

<script lang="ts">
// create-note-in 事件带目标文件夹 id，SpeednoteView 据此直接新建（树右键「新建笔记」用）
export default { name: 'NoteFolderTree' }
</script>

<template>
  <section
    class="card nft-root"
    :class="{ 'nft-dragging': dragActive }"
    aria-label="笔记文件夹"
    @keydown="onTreeKeydown"
  >
    <header
      class="nft-header"
      :class="{ 'nft-drop-root': dropHint?.mode === 'root' && dropHint.rowKey === 'root' && !trashMode && !filterActive }"
      title="根目录：未归入文件夹的笔记都在这里"
      @click="emit('select-folder', null)"
      @contextmenu="openBodyMenu"
    >
      <h2 class="nft-title">速记</h2>
      <div class="nft-header-actions">
        <template v-if="trashMode">
          <button
            class="icon-btn nft-add nft-danger"
            title="一键清空回收站（不可恢复）"
            aria-label="一键清空回收站"
            @click.stop="emit('purge-all')"
          >
            <Eraser :size="15" :stroke-width="2" />
          </button>
          <button
            class="icon-btn nft-add"
            :class="{ 'nft-trash-on': trashMode }"
            title="返回笔记"
            aria-label="返回笔记"
            @click.stop="trashMode = false"
          >
            <ArchiveRestore :size="15" :stroke-width="2" />
          </button>
        </template>
        <template v-else>
          <button
            class="icon-btn nft-add"
            :class="{ on: searchOpen }"
            title="搜索"
            aria-label="搜索笔记"
            @click.stop="toggleSearch"
          >
            <Search :size="15" :stroke-width="2" />
          </button>
          <button
            class="icon-btn nft-add"
            :title="hasCollapsible ? '全部收起' : '全部展开'"
            :aria-label="hasCollapsible ? '全部收起' : '全部展开'"
            @click.stop="hasCollapsible ? collapseAll() : expandAll()"
          >
            <ChevronsDownUp v-if="hasCollapsible" :size="15" :stroke-width="2" />
            <ChevronsUpDown v-else :size="15" :stroke-width="2" />
          </button>
          <button
            class="icon-btn nft-add"
            title="新建笔记"
            aria-label="新建笔记"
            @click.stop="emit('create-note')"
          >
            <FilePlus2 :size="15" :stroke-width="2" />
          </button>
          <button
            class="icon-btn nft-add"
            :title="selectedFolderId != null ? `在「${folderName(selectedFolderId)}」下新建目录` : '新建目录（根目录）'"
            aria-label="新建目录"
            @click.stop="startCreateAtSelection"
          >
            <FolderPlus :size="15" :stroke-width="2" />
          </button>
          <button
            class="icon-btn nft-add"
            title="回收站"
            aria-label="打开回收站"
            @click.stop="trashMode = true"
          >
            <Archive :size="15" :stroke-width="2" />
          </button>
        </template>
      </div>
    </header>

    <!-- 搜索框（默认收起，树头搜索 icon 切换；筛选词存在时保持显示） -->
    <div v-if="searchOpen && !trashMode" class="nft-search">
      <input
        ref="searchInput"
        v-model="query"
        type="text"
        placeholder="搜索… 输入 # 按标签筛选"
        aria-label="搜索笔记"
        spellcheck="false"
        @focus="onSearchFocus"
        @blur="onSearchBlur"
        @keydown="onSearchKeydown"
      />
      <button v-if="query" class="icon-btn nft-clear" title="清空" @mousedown.prevent @click="query = ''; searchInput?.focus()">
        <X :size="12" :stroke-width="2" />
      </button>

      <!-- 空词聚焦：用法提示（参考系统搜索的前缀提示面板）。
           行用 mousedown.prevent 防止输入框失焦收起 -->
      <div v-if="hintOpen && !tagPickOpen" class="nft-search-pop">
        <p class="nft-sp-head">搜索提示</p>
        <button
          class="nft-sp-row nft-sp-act"
          type="button"
          @mousedown.prevent
          @click="query = '#'; searchInput?.focus()"
        >
          <code class="nft-sp-key">#标签</code>
          <span class="nft-sp-desc">按标签筛选（点击直接输入 #）</span>
        </button>
        <div class="nft-sp-row">
          <code class="nft-sp-key">直接输入</code>
          <span class="nft-sp-desc">全文搜索标题与正文</span>
        </div>
      </div>


      <!-- # 标签挑选下拉 -->
      <ul v-if="tagPickOpen" class="nft-search-pop nft-tag-pick" role="listbox" aria-label="选择标签">
        <li
          v-for="(t, i) in tagCandidates"
          :key="t.id"
          role="option"
          :aria-selected="i === tagPickIndex"
          :class="{ on: i === tagPickIndex }"
          @mousedown.prevent="pickTag(t)"
          @mouseenter="tagPickIndex = i"
          @contextmenu="openTagMenu(t, $event)"
        >
          <span class="nft-tp-name">#{{ t.name }}</span>
          <span class="nft-tp-count">{{ tagCount(t.id) }}</span>
        </li>
        <li v-if="tagCandidates.length === 0" class="nft-tp-empty">没有匹配的标签</li>
      </ul>
    </div>

    <!-- 行内改标签名（# 下拉行右键触发） -->
    <input
      v-if="renamingTagId != null"
      v-model="tagNameValue"
      class="nft-tag-input"
      maxlength="20"
      @keydown.enter.prevent="commitTagRename"
      @keydown.esc="renamingTagId = null"
      @blur="commitTagRename"
    />

    <div class="nft-body" @click="onBodyClick" @contextmenu="openBodyMenu">
      <!-- 回收站态：平铺已删笔记 -->
      <template v-if="trashMode">
        <div
          v-for="n in trashedNotes"
          :key="`t${n.id}`"
          class="nft-row nft-note-row"
          :class="{ active: n.id === activeNoteId }"
          role="button"
          tabindex="0"
          @click="emit('select-note', n.id)"
        >
          <FileText :size="13" :stroke-width="1.8" class="nft-note-icon" />
          <span class="nft-row-name" :class="{ 'is-untitled': n.title === '无标题笔记' }" :title="n.title">
            {{ n.title }}
          </span>
          <span class="nft-row-actions">
            <button class="icon-btn nft-act" title="还原" @click.stop="emit('restore-note', n.id)">
              <Undo2 :size="12" :stroke-width="2" />
            </button>
            <button
              class="icon-btn nft-act nft-act-del"
              title="永久删除"
              @click.stop="emit('purge-note', n.id)"
            >
              <Trash2 :size="12" :stroke-width="2" />
            </button>
          </span>
        </div>
        <div v-if="trashedNotes.length === 0" class="nft-empty">
          <p>回收站是空的</p>
        </div>
      </template>

      <!-- 搜索 / 标签态：跨全部文件夹的平铺结果，标注所在路径 -->
      <template v-else-if="filterActive">
        <div
          v-for="n in filterResults"
          :key="`s${n.id}`"
          class="nft-row nft-note-row nft-result-row"
          :class="{ active: n.id === activeNoteId }"
          role="button"
          tabindex="0"
          @click="emit('select-note', n.id)"
          @keydown.enter.prevent="emit('select-note', n.id)"
        >
          <span v-if="n.icon" class="nft-note-emoji">{{ n.icon }}</span>
          <FileText v-else :size="13" :stroke-width="1.8" class="nft-note-icon" />
          <span class="nft-result-main">
            <span class="nft-row-name" :class="{ 'is-untitled': n.title === '无标题笔记' }" :title="n.title">
              {{ n.title }}
            </span>
            <span class="nft-result-path" :title="folderPathOf(n) || '根目录'">
              {{ folderPathOf(n) || '根目录' }}
            </span>
          </span>
        </div>
        <div v-if="filterResults.length === 0" class="nft-empty">
          <p>{{ activeTagId != null ? '该标签下还没有笔记' : '没有匹配的笔记' }}</p>
        </div>
      </template>

      <!-- 正常树：文件夹 + 笔记 -->
      <template v-else>
        <template v-for="row in rows" :key="row.kind === 'folder' ? `f${row.folder.id}` : row.kind === 'note' ? `n${row.note.id}` : `c${row.parentId}`">
          <!-- 行内新建文件夹输入框：就地渲染在目标父文件夹正下方 -->
          <input
            v-if="row.kind === 'create'"
            v-model="createName"
            class="nft-new-input"
            :style="{ marginLeft: `${row.depth * 14 + 6}px`, width: `calc(100% - ${row.depth * 14 + 18}px)` }"
            :placeholder="`在「${folderName(row.parentId)}」下新建，回车确认`"
            maxlength="30"
            @keydown.enter.prevent="commitCreate"
            @keydown.esc="creatingIn = null"
            @blur="commitCreate"
          />

          <!-- 文件夹行（点击 = 选中并切换展开/收起） -->
          <div
            v-else-if="row.kind === 'folder'"
            class="nft-row"
            :class="[
              {
                active: row.folder.id === selectedFolderId,
                builtin: row.folder.builtin,
                'nft-expanded': isExpanded(row.folder.id),
              },
              hintClass(row),
            ]"
            :data-drop-folder="row.folder.id"
            :style="{ paddingLeft: `${row.depth * 14 + 6}px` }"
            role="button"
            tabindex="0"
            @click="onFolderClick(row.folder)"
            @keydown.enter.prevent="onFolderClick(row.folder)"
            @pointerdown="onRowPointerDown($event, { kind: 'folder', id: row.folder.id }, row.folder.name)"
            @contextmenu="openFolderMenu(row.folder, $event)"
          >
            <button
              class="nft-caret"
              :title="isExpanded(row.folder.id) ? '收起' : '展开'"
              :aria-label="isExpanded(row.folder.id) ? '收起' : '展开'"
              @click.stop="toggleExpand(row.folder.id)"
              @pointerdown.stop
            >
              <ChevronDown v-if="isExpanded(row.folder.id)" :size="12" :stroke-width="2" />
              <ChevronRight v-else :size="12" :stroke-width="2" />
            </button>
            <component
              :is="isExpanded(row.folder.id) ? FolderOpen : Folder"
              :size="14"
              :stroke-width="1.8"
              class="nft-folder-icon"
            />
            <input
              v-if="renamingId === row.folder.id"
              v-model="renameValue"
              class="nft-rename-input"
              maxlength="30"
              @click.stop
              @pointerdown.stop
              @keydown.enter.prevent="commitRename"
              @keydown.esc="renamingId = null"
              @blur="commitRename"
            />
            <span v-else class="nft-row-name" :title="row.folder.name">{{ row.folder.name }}</span>
            <span class="nft-row-actions">
              <button class="icon-btn nft-act" title="新建子文件夹" @click.stop="startCreate(row.folder.id)">
                <Plus :size="12" :stroke-width="2" />
              </button>
              <template v-if="!row.folder.builtin">
                <button class="icon-btn nft-act" title="重命名 (F2)" @click.stop="startRename(row.folder)">
                  <Pencil :size="12" :stroke-width="2" />
                </button>
                <button
                  class="icon-btn nft-act nft-act-del"
                  title="删除文件夹"
                  @click.stop="emit('delete-folder', row.folder.id)"
                >
                  <Trash2 :size="12" :stroke-width="2" />
                </button>
              </template>
            </span>
          </div>

          <!-- 笔记行（icon = 用户设置的 emoji，无则默认文件图标）。
               data-note-id / data-note-folder 供拖拽 hitTest 判落点：
               笔记行（根目录笔记行=树根落点）与行间隙的命中依据，见 hitTest -->
          <div
            v-else
            class="nft-row nft-note-row"
            :class="[{ active: row.note.id === activeNoteId }, hintClass(row)]"
            :style="{ paddingLeft: `${row.depth * 14 + 22}px` }"
            :data-note-id="row.note.id"
            :data-note-folder="row.note.folder_id"
            role="button"
            tabindex="0"
            @click="emit('select-note', row.note.id)"
            @keydown="onRowKeydownRow($event, row)"
            @pointerdown="onRowPointerDown($event, { kind: 'note', id: row.note.id }, row.note.title)"
            @contextmenu="openNoteMenu(row.note, $event)"
          >
            <span v-if="row.note.icon" class="nft-note-emoji">{{ row.note.icon }}</span>
            <FileText v-else :size="13" :stroke-width="1.8" class="nft-note-icon" />
            <span class="nft-row-name" :class="{ 'is-untitled': row.note.title === '无标题笔记' }" :title="row.note.title">
              {{ row.note.title }}
            </span>
          </div>
        </template>

        <div v-if="rows.length === 0 && creatingIn == null" class="nft-empty">
          <p>还没有文件夹</p>
          <p class="nft-empty-sub">笔记新建后落在这里（根目录）</p>
        </div>
      </template>
    </div>

    <!-- 拖拽幽灵：落点名跟随显示（长树滚到底时行内高亮可能不在光标附近，浮签是兜底反馈） -->
    <Teleport to="body">
      <div
        v-if="dragActive"
        class="nft-drag-ghost"
        :style="{ left: `${dragPos.x + 10}px`, top: `${dragPos.y + 8}px` }"
      >
        {{ dragLabel }}
        <span v-if="dragTargetName" class="nft-ghost-target">→ {{ dragTargetName }}</span>
      </div>
    </Teleport>

    <ContextMenu :visible="ctx.visible" :x="ctx.x" :y="ctx.y" :items="ctx.items" @close="ctx.visible = false" />
  </section>
</template>

<style scoped>
.nft-root {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 20px 10px 12px;
  font-size: calc(1rem * var(--fs-notes, 1));
  user-select: none;
}
.nft-root.nft-dragging {
  cursor: grabbing;
}
.nft-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  padding: 0 6px 10px;
  margin-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.nft-header.nft-drop-root {
  outline: 2px dashed var(--brand-500);
  outline-offset: -2px;
}
.nft-title {
  font-size: 1em;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.nft-header-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.icon-btn.nft-add {
  width: 26px;
  height: 26px;
  color: var(--text-3);
}
.icon-btn.nft-add:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.nft-add.on,
.icon-btn.nft-add.nft-trash-on {
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.nft-add.nft-danger:hover {
  background: color-mix(in srgb, var(--c-red) 12%, transparent);
  color: var(--c-red);
}

/* 搜索框 + 弹出提示/标签下拉 */
.nft-search {
  position: relative;
  margin: 0 4px 8px;
}
.nft-search input {
  width: 100%;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  font-size: 0.75em;
  font-family: inherit;
  color: var(--text-1);
  padding: 6px 26px 6px 10px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.nft-search input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.nft-clear {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 20px;
  height: 20px;
  color: var(--text-3);
}
.nft-search-pop {
  position: absolute;
  left: 0;
  right: 0;
  top: calc(100% + 4px);
  z-index: 30;
  margin: 0;
  padding: 6px;
  list-style: none;
  /* 实底（--bg-card-solid）而非半透明卡：提示文字叠在壁纸/树行上必须清晰可读 */
  background: var(--bg-card-solid);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-card);
}
.nft-sp-head {
  margin: 2px 6px 6px;
  font-size: 0.625em;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: var(--text-3);
}
.nft-sp-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  width: 100%;
  padding: 4px 6px;
  border-radius: var(--radius-sm);
}
button.nft-sp-act {
  border: none;
  background: transparent;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
button.nft-sp-act:hover {
  background: var(--bg-card-soft);
}
.nft-sp-key {
  flex-shrink: 0;
  font-family: inherit;
  font-size: 0.6875em;
  font-weight: 700;
  color: var(--brand-500);
}
.nft-sp-desc {
  font-size: 0.6875em;
  color: var(--text-2);
}
.nft-tag-pick {
  max-height: 220px;
  overflow-y: auto;
}
.nft-tag-pick li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  font-size: 0.75em;
  color: var(--text-1);
  cursor: pointer;
}
.nft-tag-pick li.on {
  background: var(--brand-50);
  color: var(--brand-500);
}
.nft-tp-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nft-tp-count {
  flex-shrink: 0;
  font-size: 0.85em;
  color: var(--text-4);
}
.nft-tag-pick li.on .nft-tp-count {
  color: var(--brand-500);
}
.nft-tp-empty {
  color: var(--text-4);
  cursor: default;
}
.nft-tag-input {
  margin: 0 4px 8px;
  border: 1px solid var(--brand-500);
  background: var(--input-bg);
  border-radius: var(--radius-pill);
  font-size: 0.75em;
  font-family: inherit;
  color: var(--text-1);
  padding: 4px 10px;
  outline: none;
  width: calc(100% - 8px);
}

.nft-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding-right: 2px;
}
.nft-row {
  display: flex;
  align-items: center;
  gap: 5px;
  min-height: 28px;
  padding-right: 6px;
  padding-top: 3px;
  padding-bottom: 3px;
  border-radius: var(--radius-md);
  cursor: pointer;
  position: relative;
  transition: background 0.12s;
}
.nft-row:hover {
  background: var(--bg-card-soft);
}
.nft-row.active {
  background: var(--brand-50);
}
.nft-row.active .nft-row-name {
  color: var(--brand-500);
}
.nft-row.builtin .nft-folder-icon {
  color: var(--c-pink);
}
.nft-folder-icon {
  flex-shrink: 0;
  color: var(--text-3);
}
.nft-note-icon {
  flex-shrink: 0;
  color: var(--text-4);
}
.nft-note-emoji {
  flex-shrink: 0;
  width: 15px;
  font-size: 12px;
  line-height: 1;
  text-align: center;
}
.nft-caret {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-4);
  border-radius: 4px;
  padding: 0;
  cursor: pointer;
}
.nft-caret:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.nft-row-name {
  flex: 1;
  min-width: 0;
  font-size: 0.8125em;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nft-row-name.is-untitled {
  color: var(--text-4);
  font-style: italic;
}
/* 搜索/标签结果行：标题 + 所在文件夹路径两行 */
.nft-result-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nft-result-path {
  font-size: 0.6875em;
  color: var(--text-4);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nft-row-actions {
  display: none;
  align-items: center;
  gap: 1px;
  flex-shrink: 0;
}
.nft-row:hover .nft-row-actions,
.nft-row:focus-within .nft-row-actions {
  display: inline-flex;
}
.icon-btn.nft-act {
  width: 22px;
  height: 22px;
  color: var(--text-3);
}
.icon-btn.nft-act:hover {
  color: var(--text-1);
  background: var(--bg-card-soft);
}
.icon-btn.nft-act-del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}
.nft-rename-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--brand-500);
  background: var(--input-bg);
  border-radius: var(--radius-sm);
  font-size: 0.8125em;
  font-family: inherit;
  color: var(--text-1);
  padding: 2px 6px;
  outline: none;
}
.nft-new-input {
  margin: 4px 6px;
  border: 1px solid var(--brand-500);
  background: var(--input-bg);
  border-radius: var(--radius-sm);
  font-size: 0.8125em;
  font-family: inherit;
  color: var(--text-1);
  padding: 4px 8px;
  outline: none;
}
.nft-empty {
  padding: 20px 8px;
  text-align: center;
  color: var(--text-4);
  font-size: 0.75em;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.nft-empty-sub {
  font-size: 0.92em;
}
/* 拖拽落点提示：into = 目标行高亮框；before/after = 顶部/底部插入线 */
.nft-row.nft-drop-into {
  outline: 2px solid var(--brand-500);
  outline-offset: -2px;
  background: var(--brand-50);
}
.nft-row.nft-drop-before::before,
.nft-row.nft-drop-after::after {
  content: '';
  position: absolute;
  left: 4px;
  right: 4px;
  height: 3px;
  border-radius: 2px;
  background: var(--brand-500);
  z-index: 1;
}
.nft-row.nft-drop-before::before {
  top: -2px;
}
.nft-row.nft-drop-after::after {
  bottom: -2px;
}
.nft-drag-ghost {
  position: fixed;
  z-index: 300;
  pointer-events: none;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding: 4px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-card);
  color: var(--text-1);
  font-size: 12px;
  border: 1px solid var(--brand-500);
  box-shadow: var(--shadow-card);
}
.nft-ghost-target {
  margin-left: 6px;
  color: var(--brand-500);
  font-weight: 600;
}
</style>

<style>
/* 透底态（壁纸+透明）的搜索弹出面板：--bg-card-solid 在透底态被压到 0.2 白，
   面板文字与后面的笔记内容重叠看不清（实测反馈）。换深玻璃实底 + 白墨描边，
   与编辑器浮层透底态的既有处理同款。⚠️ 本块是非 scoped 样式，直接写裸选择器；
   :global() 只在 scoped 块里有意义，写在这里整条规则会被编译成无效选择器（踩过） */
html[data-wallpaper-clear='1'] .nft-search-pop {
  background: rgba(28, 29, 41, 0.94);
  border-color: rgba(255, 255, 255, 0.16);
}
html[data-wallpaper-clear='1'] .nft-search-pop .nft-sp-desc {
  color: rgba(255, 255, 255, 0.78);
}
html[data-wallpaper-clear='1'] .nft-search-pop .nft-sp-head {
  color: rgba(255, 255, 255, 0.5);
}
html[data-wallpaper-clear='1'] .nft-search-pop .nft-tag-pick li {
  color: rgba(255, 255, 255, 0.88);
}
html[data-wallpaper-clear='1'] .nft-search-pop .nft-tp-count {
  color: rgba(255, 255, 255, 0.5);
}
html[data-wallpaper-clear='1'] .nft-search-pop .nft-tag-pick li.on {
  background: rgba(255, 255, 255, 0.12);
}
html[data-wallpaper-clear='1'] .nft-search-pop button.nft-sp-act:hover {
  background: rgba(255, 255, 255, 0.12);
}
</style>
