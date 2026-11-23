<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Search } from 'lucide-vue-next'
import {
  isTauri,
  tauriApi,
  type ClipboardItem,
  type ExtensionEntry,
  type Note,
  type Resource,
  type Snippet,
  type Todo,
} from '../api/tauri'
import { SETTINGS_INDEX, type SettingsIndexEntry } from './settingsIndex.generated'
import { GROUP_LABEL, SECTION_GROUP, SECTION_LABEL, type SectionId } from '../utils/settingsSections'
import { useFocusTrap } from '../composables/useFocusTrap'
import { markdownPlainText } from '../utils/markdown'
import { IS_MAC } from '../utils/platform'

const props = defineProps<{
  visible: boolean
  /**
   * 独立浮窗形态（search 窗口）：不再套 Teleport + `.modal-mask` 遮罩，
   * 只渲染搜索卡片本体——窗口本身已经是透明无边框的，遮罩会把整窗涂黑。
   */
  standalone?: boolean
  /** 空态提示区展示的呼出快捷键文案。宿主传（浮窗读 getUiConfig、主窗读 store），默认 Ctrl+K */
  shortcutLabel?: string
  /** 已安装扩展列表（宿主传入，本组件不 import store——轻量入口要减活堆）。
   *  扩展不走后端检索：list_extensions 是磁盘扫描，逐键扫 manifest 太重，
   *  主窗用已加载的列表、浮窗每次唤起拉一次，前端按名/描述过滤。 */
  extensions?: ExtensionEntry[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'openResource', r: Resource): void
  (e: 'openNote', n: Note): void
  (e: 'openTodo', t: Todo): void
  /** 设置项结果：把分区 id 交给宿主，亮主窗深链到设置对应分区 */
  (e: 'openSettings', section: string): void
  /** 扩展结果：把扩展 id 交给宿主，按该扩展的「打开方式」打开 */
  (e: 'openExtension', id: string): void
}>()

// ⚠️ 这里绝不能 import useStore：本组件被做进 search.html 的轻量入口
// （src/light/search.ts），拖进全量 store 就等于白做内存优化（约定 41）。
// 检索直调命令，快捷键文案由宿主经 shortcutLabel 传入。
/** 弹窗内展示当前生效的呼出快捷键 */
const searchShortcutLabel = computed(() => props.shortcutLabel || (IS_MAC ? 'Cmd+K' : 'Ctrl+K'))

const keyword = ref('')
const results = ref<{
  resources: Resource[]
  notes: Note[]
  todos: Todo[]
  snippets: Snippet[]
  clipboard: ClipboardItem[]
}>({
  resources: [],
  notes: [],
  todos: [],
  snippets: [],
  clipboard: [],
})
/** 设置项检索结果（纯前端：走构建期生成的设置项索引，无需后端） */
const settingHits = ref<SettingsIndexEntry[]>([])
/** 扩展检索结果（纯前端：过滤宿主传入的已安装扩展列表） */
const extHits = ref<ExtensionEntry[]>([])
const searched = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)
const cardRef = ref<HTMLElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, inputRef)

interface FlatItem {
  type: 'resource' | 'note' | 'todo' | 'snippet' | 'clipboard' | 'extension' | 'setting'
  key: string
  resource?: Resource
  note?: Note
  todo?: Todo
  snippet?: Snippet
  clipboard?: ClipboardItem
  extension?: ExtensionEntry
  setting?: SettingsIndexEntry
}

/** 组标题与组间固定次序（相关度同分时保持这个顺序） */
const GROUP_TITLES: Record<FlatItem['type'], string> = {
  resource: '速达',
  note: '速记',
  todo: '待办',
  snippet: '提示词',
  clipboard: '剪贴板',
  extension: '扩展',
  setting: '设置',
}
const GROUP_ORDER: FlatItem['type'][] = [
  'resource',
  'note',
  'todo',
  'snippet',
  'clipboard',
  'extension',
  'setting',
]

/** 原始扁平结果（按域固定顺序聚合）；展示顺序由 orderedGroups 按相关度重排 */
const rawResults = computed<FlatItem[]>(() => [
  ...results.value.resources.map((r) => ({ type: 'resource' as const, key: 'r' + r.id, resource: r })),
  ...results.value.notes.map((n) => ({ type: 'note' as const, key: 'n' + n.id, note: n })),
  ...results.value.todos.map((t) => ({ type: 'todo' as const, key: 't' + t.id, todo: t })),
  ...results.value.snippets.map((s) => ({ type: 'snippet' as const, key: 's' + s.id, snippet: s })),
  ...results.value.clipboard.map((c) => ({
    type: 'clipboard' as const,
    key: 'c' + c.id,
    clipboard: c,
  })),
  ...extHits.value.map((e) => ({ type: 'extension' as const, key: 'e' + e.id, extension: e })),
  ...settingHits.value.map((st) => ({
    type: 'setting' as const,
    key: 'k' + st.section + '-' + st.title,
    setting: st,
  })),
])

/** 单条结果的相关度：0 = 名称/标题以关键词开头（最符合），1 = 名称/标题包含，
 *  2+ = 仅内容/归属命中（数值越大越弱） */
function rankOf(item: FlatItem): number {
  const q = keyword.value.trim().toLowerCase()
  const starts = (s: string) => s.toLowerCase().startsWith(q)
  const has = (s: string) => s.toLowerCase().includes(q)
  switch (item.type) {
    case 'resource':
      return starts(item.resource!.name) ? 0 : has(item.resource!.name) ? 1 : 2
    case 'note':
      // 后端笔记检索标题/内容都可能命中，标题命中更相关
      return starts(item.note!.title) ? 0 : has(item.note!.title) ? 1 : 2
    case 'todo':
      return starts(item.todo!.title) ? 0 : has(item.todo!.title) ? 1 : 2
    case 'snippet':
      return starts(item.snippet!.title) ? 0 : has(item.snippet!.title) ? 1 : 2
    case 'clipboard':
      return starts(clipName(item.clipboard!))
        ? 0
        : has(item.clipboard!.content) || has(item.clipboard!.source_app ?? '')
          ? 1
          : 2
    case 'extension':
      return starts(item.extension!.name)
        ? 0
        : has(item.extension!.name)
          ? 1
          : has(item.extension!.description)
            ? 2
            : 3
    case 'setting': {
      const idx = item.setting!.title.toLowerCase().indexOf(q)
      if (idx === 0) return 0
      if (idx > 0) return 1
      return settingWhere(item.setting!.section).toLowerCase().includes(q) ? 2 : 3
    }
  }
}

/** 组内最符合者的相关度（组间排序依据） */
function minRank(items: FlatItem[]): number {
  return items.length > 0 ? Math.min(...items.map((it) => rankOf(it))) : 99
}

interface ResultGroup {
  key: FlatItem['type']
  title: string
  items: (FlatItem & { index: number })[]
}

/** 展示分组：组间按「组内最符合者」的相关度排——搜「主题」时标题开头命中的设置组排最前，
 *  仅内容命中的速记组靠后；同分保持 GROUP_ORDER 固定次序（Array.sort 稳定）。组内同理按
 *  相关度稳定排序，同级不打散后端返回的原顺序。items 附全局下标：activeIndex / 键盘
 *  导航 / 滚动定位都按展示序的绝对位置来。 */
const orderedGroups = computed<ResultGroup[]>(() => {
  const byType = new Map<FlatItem['type'], FlatItem[]>()
  for (const item of rawResults.value) {
    const list = byType.get(item.type) ?? []
    list.push(item)
    byType.set(item.type, list)
  }
  const groups = GROUP_ORDER.map((t) => ({
    type: t,
    items: (byType.get(t) ?? []).slice().sort((a, b) => rankOf(a) - rankOf(b)),
  })).filter((g) => g.items.length > 0)
  groups.sort((a, b) => minRank(a.items) - minRank(b.items))
  let start = 0
  return groups.map((g) => {
    const items = g.items.map((it, i) => ({ ...it, index: start + i }))
    start += items.length
    return { key: g.type, title: GROUP_TITLES[g.type], items }
  })
})

/** 展示序扁平列表（= orderedGroups 展平）：键盘导航/滚动定位/回车打开都按它取绝对下标 */
const flatResults = computed<FlatItem[]>(() => orderedGroups.value.flatMap((g) => g.items))

/** 是否有任何命中：七类结果（后端五域 + 前端扩展/设置两域）全空才算未找到 */
const hasAnyResult = computed(
  () =>
    results.value.resources.length > 0 ||
    results.value.notes.length > 0 ||
    results.value.todos.length > 0 ||
    results.value.snippets.length > 0 ||
    results.value.clipboard.length > 0 ||
    extHits.value.length > 0 ||
    settingHits.value.length > 0,
)

/** 扩展检索：名称 / 描述包含关键词，上限 8 条 */
function searchExtensions(list: ExtensionEntry[] | undefined, kw: string): ExtensionEntry[] {
  const q = kw.toLowerCase()
  if (!q) return []
  return (list ?? [])
    .filter(
      (e) => e.name.toLowerCase().includes(q) || e.description.toLowerCase().includes(q),
    )
    .slice(0, 8)
}

/** 剪贴板条目的行名：文本取首个非空行；文件取首个文件名（多个带计数）；图片给占位 */
function clipName(c: ClipboardItem): string {
  if (c.kind === 'image') return '图片'
  const lines = (c.content || '').split('\n').filter((l) => l.trim())
  const first = lines[0] ?? ''
  if (c.kind === 'file') {
    const seg = first.split(/[\\/]/).pop() ?? first
    const extra = (c.file_paths?.length ?? 0) - 1
    return extra > 0 ? `${seg} 等 ${extra + 1} 个文件` : seg || '文件'
  }
  return first || '（空）'
}

/** 剪贴板条目的副行：文本给后续内容预览，文件给路径清单，图片给来源应用 */
function clipSub(c: ClipboardItem): string {
  if (c.kind === 'text') return markdownPlainText(c.content, 60)
  if (c.kind === 'file') return (c.file_paths ?? []).join('、')
  return c.source_app || ''
}

/** 选中剪贴板历史条目：写回系统剪贴板后收起（口径同剪贴板浮层的「复制」） */
async function copyClipboardItem(c: ClipboardItem) {
  if (isTauri()) {
    try {
      await tauriApi.clipboardCopy(c.id)
    } catch {
      return // 写回失败不收起，留在原地可重试
    }
  }
  emit('close')
}

/** 设置项结果的归属路径（显示用，口径同设置页内搜索的 where） */
function settingWhere(section: string): string {
  const group = SECTION_GROUP[section as SectionId]
  return `${GROUP_LABEL[group] ?? ''} / ${SECTION_LABEL[section] ?? ''}`
}

/** 设置项检索（纯前端）：口径对齐设置页内搜索——标题命中 > 「所属位置」命中，上限 8 条 */
function searchSettings(kw: string): SettingsIndexEntry[] {
  const q = kw.toLowerCase()
  const hits: { entry: SettingsIndexEntry; rank: number }[] = []
  for (const entry of SETTINGS_INDEX) {
    if (!(entry.section in SECTION_GROUP)) continue
    const where = settingWhere(entry.section).toLowerCase()
    const idx = entry.title.toLowerCase().indexOf(q)
    if (idx < 0 && !where.includes(q)) continue
    hits.push({ entry, rank: idx === 0 ? 0 : idx > 0 ? 1 : 2 })
  }
  return hits
    .sort((a, b) => a.rank - b.rank || a.entry.title.length - b.entry.title.length)
    .slice(0, 8)
    .map((h) => h.entry)
}

const activeIndex = ref(-1)

watch(flatResults, (list) => {
  activeIndex.value = list.length > 0 ? 0 : -1
})

function scrollActiveIntoView() {
  const item = flatResults.value[activeIndex.value]
  if (!item) return
  document
    .querySelector<HTMLElement>(`[data-search-key="${item.key}"]`)
    ?.scrollIntoView({ block: 'nearest' })
}

/** 统一的结果打开分流：点击行与键盘 Enter 共用 */
function openItem(item: FlatItem | undefined) {
  if (!item) return
  if (item.type === 'resource' && item.resource) emit('openResource', item.resource)
  else if (item.type === 'note' && item.note) emit('openNote', item.note)
  else if (item.type === 'todo' && item.todo) emit('openTodo', item.todo)
  else if (item.type === 'snippet' && item.snippet) void copySnippet(item.snippet)
  else if (item.type === 'clipboard' && item.clipboard) void copyClipboardItem(item.clipboard)
  else if (item.type === 'extension' && item.extension) emit('openExtension', item.extension.id)
  else if (item.type === 'setting' && item.setting) emit('openSettings', item.setting.section)
}

function openActive() {
  openItem(flatResults.value[activeIndex.value])
}

/** 行徽标样式 / 文本 / 名称 / 副行：七种结果类型共用一套通用行渲染 */
function badgeClass(item: FlatItem): string {
  switch (item.type) {
    case 'resource':
      return item.resource!.kind
    case 'note':
      return 'note-badge'
    case 'todo':
      return 'todo-badge'
    case 'snippet':
      return 'snippet-badge'
    case 'clipboard':
      return 'clipboard-badge'
    case 'extension':
      return 'ext-badge'
    case 'setting':
      return 'setting-badge'
  }
}

function badgeText(item: FlatItem): string {
  switch (item.type) {
    case 'resource':
      return kindText(item.resource!.kind)
    case 'note':
      return '笔记'
    case 'todo':
      return item.todo!.done ? '已完成' : '待完成'
    case 'snippet':
      return '提示词'
    case 'clipboard':
      return '剪贴板'
    case 'extension':
      return '扩展'
    case 'setting':
      return '设置'
  }
}

function itemLabel(item: FlatItem): string {
  switch (item.type) {
    case 'resource':
      return item.resource!.name
    case 'note':
      return item.note!.title
    case 'todo':
      return item.todo!.title
    case 'snippet':
      return item.snippet!.title
    case 'clipboard':
      return clipName(item.clipboard!)
    case 'extension':
      return item.extension!.name
    case 'setting':
      return item.setting!.title
  }
}

function nameTitle(item: FlatItem): string {
  // 剪贴板文本条的悬浮全文用原文，其余用名称本身
  if (item.type === 'clipboard') {
    return item.clipboard!.kind === 'text' ? item.clipboard!.content : clipName(item.clipboard!)
  }
  return itemLabel(item)
}

function subLabel(item: FlatItem): string {
  switch (item.type) {
    case 'resource':
      return item.resource!.target
    case 'note':
      return markdownPlainText(item.note!.content, 60)
    case 'todo':
      return `${['普通', '重要', '紧急'][item.todo!.priority] ?? '普通'}优先级`
    case 'snippet':
      return markdownPlainText(item.snippet!.content, 60)
    case 'clipboard':
      return clipSub(item.clipboard!)
    case 'extension':
      return item.extension!.description
    case 'setting':
      return settingWhere(item.setting!.section)
  }
}

/** 选中提示词：复制正文后收起（顺带记复制次数，口径同 PromptBoxCard 的复制链路） */
async function copySnippet(s: Snippet) {
  let ok = false
  try {
    await navigator.clipboard.writeText(s.content)
    ok = true
  } catch {
    // 剪贴板 API 不可用时回退隐藏 textarea + execCommand（同 PromptBoxCard）
    const ta = document.createElement('textarea')
    ta.value = s.content
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    try {
      ok = document.execCommand('copy')
    } catch {
      ok = false
    }
    document.body.removeChild(ta)
  }
  if (!ok) return // 复制失败不收起，留在原地可重试
  if (isTauri()) void tauriApi.recordSnippetCopy(s.id).catch(() => {})
  emit('close')
}

/** 独立浮窗：按住卡片空白（输入框、结果行除外）拖动移动窗口。位置只影响当次展示，
 *  收起后下次唤起重新全局居中，不做跨唤起记忆。右键 / 中键不触发。 */
function onDragStart(e: MouseEvent) {
  if (!props.standalone || !isTauri() || e.button !== 0) return
  if ((e.target as HTMLElement | null)?.closest('input, textarea, .result-item')) return
  void getCurrentWindow().startDragging()
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
    return
  }
  const list = flatResults.value
  if (list.length === 0) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value + 1) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    activeIndex.value = (activeIndex.value - 1 + list.length) % list.length
    scrollActiveIntoView()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    openActive()
  }
}

let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.visible,
  (v) => {
    if (v) {
      keyword.value = ''
      results.value = { resources: [], notes: [], todos: [], snippets: [], clipboard: [] }
      settingHits.value = []
      extHits.value = []
      searched.value = false
      activeIndex.value = -1
    }
  },
)

watch(keyword, (kw) => {
  if (searchTimer) clearTimeout(searchTimer)
  const trimmed = kw.trim()
  if (!trimmed) {
    results.value = { resources: [], notes: [], todos: [], snippets: [], clipboard: [] }
    settingHits.value = []
    extHits.value = []
    searched.value = false
    return
  }
  searchTimer = setTimeout(async () => {
    // 浏览器预览无后端（isTauri() 为假）给空结果，避免 invoke 抛错
    results.value = isTauri()
      ? await tauriApi.searchAll(trimmed)
      : { resources: [], notes: [], todos: [], snippets: [], clipboard: [] }
    settingHits.value = searchSettings(trimmed)
    extHits.value = searchExtensions(props.extensions, trimmed)
    searched.value = true
  }, 300)
})

// 扩展列表是宿主传入的响应式数据（浮窗每次唤起重拉）：变化时按当前关键词就地重算
watch(
  () => props.extensions,
  (list) => {
    const trimmed = keyword.value.trim()
    if (trimmed) extHits.value = searchExtensions(list, trimmed)
  },
)

function kindText(kind: Resource['kind']): string {
  if (kind === 'app') return '程序'
  if (kind === 'web') return '网页'
  return '文件'
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <!-- standalone（search 独立浮窗）不 Teleport、不套遮罩：窗口本身就是容器，
       遮罩会把透明小窗涂成整块深色（本组件被 search.html 轻量入口复用） -->
  <Teleport to="body" :disabled="standalone">
    <Transition :name="standalone ? 'search-none' : 'mask'">
      <!-- 点击卡片外的空白（独立窗的透明留边 / 主窗的遮罩）即收起，口径同应用内弹窗遮罩 -->
      <div v-if="visible" :class="standalone ? 'search-layer' : 'modal-mask'" @click.self="emit('close')">
        <!-- 独立浮窗时整卡即拖拽把手（standalone 才可拖，主窗内嵌形态拖了会挪主窗）：
             输入框与结果行除外——前者要光标定位/选词，后者是点击目标；
             卡片顶部留白、结果区空白都能拖 -->
        <div
          class="search-card"
          :class="{ draggable: standalone }"
          ref="cardRef"
          role="dialog"
          aria-label="全局搜索"
          aria-modal="true"
          @mousedown="onDragStart"
        >
          <div class="search-input-row">
            <Search class="search-icon" :size="17" :stroke-width="1.8" />
            <input
              ref="inputRef"
              v-model="keyword"
              class="search-input"
              type="text"
              placeholder="搜索速达、速记、待办、提示词、剪贴板、扩展与设置…"
            />
          </div>

          <div class="search-results" role="listbox" aria-label="搜索结果">
            <!-- 结果分组：组间按相关度动态排（标题/名称开头命中的组最前，见 orderedGroups），
                 组内同理；七种类型共用一套行渲染，点击统一走 openItem 分流 -->
            <template v-for="g in orderedGroups" :key="g.key">
              <p class="result-group-title" role="presentation">{{ g.title }}</p>
              <div
                v-for="item in g.items"
                :key="item.key"
                class="result-item"
                :class="{ active: item.index === activeIndex }"
                :data-search-key="item.key"
                role="option"
                :aria-selected="item.index === activeIndex"
                @click="openItem(item)"
                @mouseenter="activeIndex = item.index"
              >
                <span class="result-badge" :class="badgeClass(item)">{{ badgeText(item) }}</span>
                <span class="result-name" :title="nameTitle(item)">{{ itemLabel(item) }}</span>
                <span class="result-sub">{{ subLabel(item) }}</span>
              </div>
            </template>

            <!-- 状态 -->
            <div v-if="searched && !hasAnyResult" class="empty-state">
              <span style="font-size: 1.625rem">🔍</span>
              <p>未找到「{{ keyword.trim() }}」相关内容</p>
            </div>
            <div
              v-else-if="!keyword.trim()"
              class="empty-state"
              style="padding: 24px 16px"
            >
              <p style="color: var(--text-4)">输入关键词检索速达、速记、待办、提示词、剪贴板、扩展与设置</p>
              <div class="shortcut-hints">
                <span><kbd>{{ searchShortcutLabel }}</kbd> 唤起搜索</span>
                <span><kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Space</kbd> 显示/隐藏窗口</span>
                <span><kbd>Esc</kbd> 关闭</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.search-card {
  width: 560px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 120px);
  display: flex;
  flex-direction: column;
  background: var(--bg-card-solid);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  overflow: hidden;
  animation: card-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
}
@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(10px) scale(0.97);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.search-input-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-soft);
}
/* 独立浮窗：整卡即拖拽把手，光标提示可抓取（结果行保持 pointer、输入框保持文本光标） */
.search-card.draggable {
  cursor: grab;
}
.search-card.draggable input {
  cursor: text;
}
.search-icon {
  color: var(--text-3);
  flex-shrink: 0;
}
.search-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: 0.9375rem;
  font-family: inherit;
  color: var(--text-1);
}
.search-input::placeholder {
  color: var(--text-4);
}
/* 输入框常驻聚焦（弹窗打开即自动聚焦），全局 input:focus-visible 的品牌色光环
   （--shadow-focus，随强调色）在这里是噪声——强调色偏暖时呈一圈浅黄框。
   弹窗本身就是搜索 UI、键盘导航走 ↑↓/Enter，不需要光环指示，单独关掉。 */
.search-input:focus,
.search-input:focus-visible {
  outline: none;
  box-shadow: none;
}

.search-results {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 12px 16px;
}
.result-group-title {
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-3);
  padding: 8px 10px 4px;
}
.result-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.12s;
}
.result-item:hover,
.result-item.active {
  background: var(--brand-50);
}
.result-badge {
  flex-shrink: 0;
  width: 34px;
  text-align: center;
  font-size: 0.625rem;
  font-weight: 600;
  border-radius: 6px;
  padding: 3px 0;
}
.result-badge.app {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.result-badge.web {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.result-badge.file {
  background: var(--c-purple-soft);
  color: var(--c-purple-ink);
}
.note-badge {
  background: var(--c-yellow-soft);
  color: var(--c-yellow-ink);
}
.todo-badge {
  background: var(--c-gray-soft);
  color: var(--c-gray-ink);
}
.snippet-badge {
  background: var(--c-pink-soft);
  color: var(--c-pink-ink);
}
.clipboard-badge {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.ext-badge {
  background: var(--c-purple-soft);
  color: var(--c-purple-ink);
}
.setting-badge {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.shortcut-hints {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 12px;
  font-size: 0.75rem;
  color: var(--text-3);
}
.shortcut-hints kbd {
  font-family: inherit;
  font-size: 0.6875rem;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  color: var(--text-2);
}
.result-name {
  flex-shrink: 0;
  max-width: 160px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.result-sub {
  flex: 1;
  min-width: 0;
  font-size: 0.75rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}

/* ---- 独立浮窗形态（standalone）----
   窗口本身已经是透明无边框的 600×520 小窗，这里只做「铺满可用区域 + 卡片自适应」：
   `search-layer` 对标 `.modal-mask` 的水平垂直双居中布局，但不带遮罩底色。 */
.search-layer {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 18px;
}
.search-layer .search-card {
  width: 100%;
  max-width: none;
  max-height: calc(100vh - 36px);
  /* 透明小窗背后是桌面而非应用底色：改用浮窗专用不透明面板底（--bg-float-panel），
     否则暗色 20% 白的卡片底在桌面上几乎隐形；透明窗里 backdrop-filter 取景不到
     桌面内容，blur 纯属白费 GPU，一并关掉（GPU 约束同约定 19）。
     描边去掉：输入药丸与面板的深浅对比已经足够分层，白描边在浅色桌面上呈粉笔框 */
  background: var(--bg-float-panel);
  border: none;
  -webkit-backdrop-filter: none;
  backdrop-filter: none;
}
/* 输入行整体做成内嵌药丸（icon + 输入框都在里面）：比面板深一档，替代边框分层 */
.search-layer .search-input-row {
  border-bottom: none;
  margin: 14px 14px 12px;
  padding: 9px 12px;
  background: var(--bg-float-input);
  border-radius: var(--radius-md);
}
/* 浮窗出场动画沿用卡片的 card-in（遮罩淡入在透明窗里没有意义） */
.search-none-enter-active,
.search-none-leave-active {
  transition: none;
}
</style>
