<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { convertFileSrc } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { emit, listen } from '@tauri-apps/api/event'
import { Boxes, Copy, Download, FileText, Pin, PinOff, Search, Trash2, X, ZoomIn } from 'lucide-vue-next'
import { isTauri, tauriApi, type ClipboardInfo, type ClipboardItem } from '../api/tauri'
import { useShellThemeConfig } from '../composables/useShellThemeConfig'
import { useScrollbarHover } from '../composables/useScrollbarHover'
import { parseTimestamp } from '../utils/time'

// 轻量壳窗口（clipboard.html）：不加载全量 store。
// 主题自举：原 useTheme 对本窗口真正起作用的只有 config 的主题三轴/玻璃/沉浸/字号，
// 见 useShellThemeConfig（口径与 useTheme 逐条对齐）。
// 「存为速记 / 加入提示词」原先经 store.addNote/saveNote/addSnippet——那三个方法
// 除了调后端命令就只往 store.state 数组里 push，而本窗口从不读那些数组，故直接调
// 命令；跨窗口刷新照旧靠既有的 notes-changed / snippets-changed 广播（消费方在主窗）。
useShellThemeConfig()
useScrollbarHover()

const appWindow = isTauri() ? getCurrentWindow() : null
const keyword = ref('')
const items = ref<ClipboardItem[]>([])
const selected = ref(0)
const loading = ref(false)
const info = ref<ClipboardInfo>({ paused: false, max_items: 500, ttl_days: 7, total: 0, shortcut: 'Ctrl+`' })
const listRef = ref<HTMLElement | null>(null)
const toasts = ref<{ id: number; text: string }[]>([])

// 滚动分页：徽标显示的是 DB 总条数（info.total），列表按页续拉
const PAGE_SIZE = 50
const loadingMore = ref(false)
const pageFull = ref(true) // 搜索态没有总数可判，用「上一页是否拉满」推断还有更多

// 右键菜单状态
const ctx = ref<{ x: number; y: number; item: ClipboardItem } | null>(null)

// 图片预览状态
const preview = ref<ClipboardItem | null>(null)

let searchTimer: ReturnType<typeof setTimeout> | null = null
let toastId = 0

// 浮层根标记：透明窗口只渲染卡片本体
document.documentElement.dataset.clipboardWindow = ''

const countText = computed(() => {
  if (!keyword.value.trim()) return `${info.value.total} 条`
  return pageFull.value ? `${items.value.length}+ 条` : `${items.value.length} 条`
})

const hasMore = computed(() =>
  keyword.value.trim() ? pageFull.value : items.value.length < info.value.total,
)

const hasPinned = computed(() => items.value.some((i) => i.is_pinned))

function toast(text: string) {
  const id = ++toastId
  toasts.value.push({ id, text })
  setTimeout(() => {
    toasts.value = toasts.value.filter((t) => t.id !== id)
  }, 1600)
}

function firstLine(text: string): string {
  const line = text.split('\n')[0]?.trim() ?? ''
  return line.length > 40 ? line.slice(0, 40) + '…' : line
}

// 请求序号：loadList 递增使所有在途请求（含旧 loadMore）作废——浮层隐藏再唤起 /
// 搜索词变化时，旧 invoke 若在重拉之后才返回，其整页数据会 append 进新列表
// （按 id 去重拦不住：新列表 id 集合不含旧页 id），列表混入不匹配/重复条目
let requestEpoch = 0

async function loadList() {
  if (!isTauri()) return
  const ep = ++requestEpoch
  loading.value = true
  try {
    const kw = keyword.value.trim()
    const list = await tauriApi.clipboardList(kw || undefined, PAGE_SIZE, 0)
    if (ep !== requestEpoch) return // 已被更新的请求作废
    items.value = list
    pageFull.value = items.value.length >= PAGE_SIZE
    selected.value = 0
    await nextTick()
    if (listRef.value) listRef.value.scrollTop = 0
  } catch {
    // 拉取失败：清掉首拉满页标记，避免搜索态残留「N+ 条」假徽标
    pageFull.value = false
  } finally {
    loading.value = false
  }
}

// 滚动到底拉下一页；offset=已加载条数（后端 LIMIT/OFFSET）
async function loadMore() {
  if (!isTauri() || loading.value || loadingMore.value || !hasMore.value) return
  const ep = requestEpoch
  loadingMore.value = true
  try {
    const kw = keyword.value.trim()
    const page = await tauriApi.clipboardList(kw || undefined, PAGE_SIZE, items.value.length)
    if (ep !== requestEpoch || kw !== keyword.value.trim()) return // 请求已被作废
    pageFull.value = page.length >= PAGE_SIZE
    if (page.length > 0) {
      // 分页期间若来了新复制记录，offset 窗口会整体平移导致跨页重复，按 id 去重
      const seen = new Set(items.value.map((i) => i.id))
      items.value = [...items.value, ...page.filter((p) => !seen.has(p.id))]
    }
  } catch {
    pageFull.value = false // 拉取失败即止损，避免滚动事件反复重试打后端
  } finally {
    loadingMore.value = false
  }
}

function onListScroll() {
  const el = listRef.value
  if (!el) return
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 140) void loadMore()
}

async function loadInfo() {
  if (!isTauri()) return
  try {
    info.value = await tauriApi.clipboardGetInfo()
  } catch {
    // 命令未就绪时使用默认值
  }
}

function reset() {
  keyword.value = ''
  ctx.value = null
  void loadList()
  void loadInfo()
}

// 浮层以「无激活」方式显示（不抢前台焦点，原输入框保持焦点可直接粘贴）。
// Rust 侧显示后派发 clipboard-shown 通知刷新列表；聚焦搜索框触发激活后
// 焦点事件同样会走到这里刷新。
function onShown() {
  reset()
}

// 窗口获得焦点 = 已激活（用户点击过搜索框），重置列表。
// 失焦关闭由 Rust 侧 dismiss 监视（前台切到其他窗口）与 Esc 全局热键负责，
// 不再在这里直接 hide——直接隐藏不会注销 Esc 热键，还会跳过焦点归还。
function onFocusChanged({ payload }: { payload: boolean }) {
  if (payload) {
    reset()
  }
}

// 点击搜索框开始键盘操作时激活浮层（清除 WS_EX_NOACTIVATE 并拿到前台焦点），
// 之后 ↑↓/Enter/Esc 与输入才能落到本窗口；纯鼠标点条目粘贴无需激活。
function onSearchFocus() {
  if (isTauri()) void tauriApi.clipboardActivate()
}

onMounted(async () => {
  // 不再 loadInitialData()：这里原先只是为了让 store 有数据可供 addNote/saveNote
  // 之类的方法内部使用，而本窗口只读剪贴板自身数据（loadList/loadInfo 各自拉）。
  // 那份全量数据（资源/笔记/待办/便签/提示词…）是浮层 renderer 活堆的大头。
  reset()
  appWindow?.onFocusChanged(onFocusChanged)
  if (isTauri()) await listen('clipboard-shown', onShown)
  document.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('keydown', onKeydown)
  if (searchTimer) clearTimeout(searchTimer)
})

// ---- 交互 ----

async function pasteItem(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    // Rust 侧：写入剪贴板 → 条目挪到最前；若唤起前是本应用主窗口，
    // 由主窗口 JS 直接插回原输入框（不依赖焦点时序的 Ctrl+V 注入）
    await tauriApi.clipboardPaste(item.id)
  } catch (e) {
    toast(`粘贴失败：${String(e)}`)
  }
}

// 浮层以无激活方式显示时 WebView 不持有焦点，click（mouseup 合成）在部分场景不可靠；
// 改在 mousedown 即触发粘贴，并 preventDefault 阻止 WebView 抢走输入焦点
// （否则源应用的输入框会失焦，注入的粘贴键找不到目标）。按钮等控件走 @click.stop 不受影响。
function onItemMouseDown(e: MouseEvent, item: ClipboardItem) {
  if (e.button !== 0) return
  const t = e.target as HTMLElement
  if (t.closest('button')) return
  e.preventDefault()
  void pasteItem(item)
}

async function onCopy(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    await tauriApi.clipboardCopy(item.id)
    toast('已复制')
  } catch (e) {
    toast(`复制失败：${String(e)}`)
  }
}

async function onSaveNote(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    const title = firstLine(item.content) || '无标题速记'
    // 直接调命令（原 store.addNote + saveNote 两个包装）：createNoteIn 的 content 参数
    // 已是完整正文，无需再 updateNote 补一次；folderId=null 落树根，与 addNote 默认一致
    await tauriApi.createNoteIn(title, item.content, null, '')
    await emit('notes-changed')
    toast('已存为速记')
  } catch (e) {
    toast(`保存失败：${String(e)}`)
  }
}

async function onSavePrompt(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    const title = firstLine(item.content) || '未命名提示词'
    await tauriApi.createSnippet(title, item.content)
    await emit('snippets-changed')
    toast('已加入提示词库')
  } catch (e) {
    toast(`保存失败：${String(e)}`)
  }
}

async function onTogglePin(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    const updated = await tauriApi.clipboardTogglePin(item.id)
    const idx = items.value.findIndex((i) => i.id === item.id)
    if (idx >= 0) items.value[idx] = updated
    items.value = [...items.value].sort((a, b) => {
      if (a.is_pinned !== b.is_pinned) return a.is_pinned ? -1 : 1
      return b.updated_at.localeCompare(a.updated_at)
    })
    toast(updated.is_pinned ? '已置顶（豁免自动清理）' : '已取消置顶')
  } catch (e) {
    toast(`操作失败：${String(e)}`)
  }
}

async function onDelete(item: ClipboardItem) {
  if (!isTauri()) return
  try {
    await tauriApi.clipboardDelete(item.id)
    items.value = items.value.filter((i) => i.id !== item.id)
    info.value.total = Math.max(0, info.value.total - 1)
    ctx.value = null
    toast('已删除')
  } catch (e) {
    toast(`删除失败：${String(e)}`)
  }
}

function previewImage(item: ClipboardItem) {
  if (!item.image_path) return
  ctx.value = null
  preview.value = item
}

function closePreview() {
  preview.value = null
}

async function onSaveImage(item: ClipboardItem) {
  if (!isTauri()) return
  const src = item.image_path
  if (!src) return
  // 快照可能是 .bmp（截图类应用只往剪贴板写 DIB 位图），默认名统一给 .png，由后端按扩展名转码
  const name = (src.split(/[\\/]/).pop() ?? 'image.png').replace(/\.bmp$/i, '.png')
  try {
    const dest = await save({
      defaultPath: name,
      filters: [{ name: '图片', extensions: ['png', 'bmp'] }],
    })
    if (!dest) return
    await tauriApi.clipboardExportImage(item.id, dest)
    toast('图片已保存')
  } catch (e) {
    toast(`保存失败：${String(e)}`)
  }
}

async function onClear() {
  if (!isTauri()) return
  try {
    await tauriApi.clipboardClear()
    items.value = []
    pageFull.value = false
    info.value.total = 0
    toast('历史已清空')
  } catch (e) {
    toast(`清空失败：${String(e)}`)
  }
}

async function onTogglePause() {
  if (!isTauri()) return
  const next = !info.value.paused
  try {
    await tauriApi.clipboardSetPaused(next)
    info.value.paused = next
    toast(next ? '已暂停记录' : '已恢复记录')
  } catch (e) {
    toast(`操作失败：${String(e)}`)
  }
}

function onSearchInput() {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(() => void loadList(), 300)
}

// 窗口拖动：整个顶部搜索栏（含输入框空白处，仍避开按钮）按下后指针发生实际位移才启动拖动，
// 避免单纯点击误触发（点击输入框打字不受影响）。
// Windows 模态拖动循环中鼠标松开事件会被 WebView 吞掉，参考 DetachedStickyWindow 实现。
const DRAG_THRESHOLD = 4
let dragPending: { x: number; y: number } | null = null

function onHeaderMouseDown(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  if (target.closest('button')) return
  dragPending = { x: e.screenX, y: e.screenY }
}

function onHeaderMouseMove(e: MouseEvent) {
  if (!dragPending || !appWindow) return
  const dx = e.screenX - dragPending.x
  const dy = e.screenY - dragPending.y
  if (dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD) {
    dragPending = null
    void appWindow.startDragging()
  }
}

function onHeaderMouseUp() {
  dragPending = null
}

function scrollSelectedIntoView() {
  listRef.value
    ?.querySelector(`[data-cb-idx="${selected.value}"]`)
    ?.scrollIntoView({ block: 'nearest' })
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    // 图片预览打开时先关预览，否则收起浮层
    if (preview.value) {
      preview.value = null
      return
    }
    // 收起并恢复唤起前窗口焦点（Esc 时前台仍是浮层，需主动归还焦点）
    if (isTauri()) void tauriApi.clipboardHide()
    else void appWindow?.hide()
    return
  }
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (items.value.length === 0) return
    if (selected.value === items.value.length - 1) void loadMore()
    selected.value = (selected.value + 1) % items.value.length
    scrollSelectedIntoView()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    if (items.value.length === 0) return
    selected.value = (selected.value - 1 + items.value.length) % items.value.length
    scrollSelectedIntoView()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const item = items.value[selected.value]
    if (item) void pasteItem(item)
  }
}

function onContextMenu(e: MouseEvent, item: ClipboardItem) {
  e.preventDefault()
  const x = Math.min(e.clientX, window.innerWidth - 168)
  const y = Math.min(e.clientY, window.innerHeight - 210)
  ctx.value = { x, y, item }
}

function onCtxAction(fn: () => Promise<void>) {
  void fn().finally(() => {
    ctx.value = null
  })
}

function relTime(ts: string): string {
  const diff = Date.now() - parseTimestamp(ts)
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  return `${Math.floor(diff / 86_400_000)} 天前`
}

function typeLabel(item: ClipboardItem): string {
  if (item.kind === 'image') return '图片'
  if (item.kind === 'file') return '文件'
  if (/^https?:\/\//.test(item.content.trim())) return '链接'
  if (/[{}[\]]|=>|fn\s*\(|function|class\s+\w/.test(item.content)) return '代码'
  return '文本'
}

function imageSrc(item: ClipboardItem | null): string {
  if (!item?.image_path) return ''
  return isTauri() ? convertFileSrc(item.image_path) : ''
}

function fileName(item: ClipboardItem): string {
  const first = item.file_paths[0] ?? ''
  const name = first.split(/[\\/]/).pop() ?? ''
  return name || first
}
</script>

<template>
  <div class="clipboard-overlay">
    <div class="cb-panel">
      <!-- 搜索框（顶部：按住可拖动窗口） -->
      <div
        class="cb-search"
        @mousedown="onHeaderMouseDown"
        @mousemove="onHeaderMouseMove"
        @mouseup="onHeaderMouseUp"
        @mouseleave="onHeaderMouseUp"
      >
        <Search :size="15" :stroke-width="2.2" class="cb-search-icon" />
        <input
          v-model="keyword"
          class="cb-input"
          type="text"
          placeholder="搜索剪贴板历史…"
          autocomplete="off"
          spellcheck="false"
          @focus="onSearchFocus"
          @input="onSearchInput"
        />
        <span class="cb-count">{{ countText }}</span>
      </div>

      <!-- 列表 -->
      <div ref="listRef" class="cb-list" @scroll="onListScroll">
        <template v-if="items.length">
          <div v-if="hasPinned" class="cb-section">置顶</div>
          <div
            v-for="(item, idx) in items"
            :key="item.id"
            :data-cb-idx="idx"
            class="cb-item"
            :class="{ selected: idx === selected }"
            @mousedown="onItemMouseDown($event, item)"
            @contextmenu="onContextMenu($event, item)"
          >
            <Pin v-if="item.is_pinned" :size="12" :stroke-width="2.2" class="cb-pin" />
            <img
              v-else-if="item.kind === 'image' && imageSrc(item)"
              :src="imageSrc(item)"
              class="cb-thumb"
              loading="lazy"
              alt=""
              title="点击放大预览"
              @mousedown.stop="previewImage(item)"
            />
            <span v-else class="cb-type">{{ typeLabel(item) }}</span>
            <div class="cb-body">
              <template v-if="item.kind === 'image'">
                <div class="cb-content">图片</div>
              </template>
              <template v-else-if="item.kind === 'file'">
                <div class="cb-content" :title="item.file_paths.join('\n')">{{ fileName(item) }}</div>
                <div v-if="item.file_paths.length > 1" class="cb-file-count">
                  {{ item.file_paths.length }} 个文件
                </div>
              </template>
              <div v-else class="cb-content" :title="item.content">{{ item.content }}</div>
              <div class="cb-meta">
                <span v-if="item.source_app" class="cb-src">{{ item.source_app }}</span>
                <span v-if="item.source_app" class="cb-dot">·</span>
                <span>{{ relTime(item.updated_at) }}</span>
              </div>
            </div>
            <div class="cb-actions">
              <button class="cb-a" title="复制" @click.stop="onCopy(item)">
                <Copy :size="14" :stroke-width="2" />
              </button>
              <button v-if="item.kind === 'text'" class="cb-a" title="存为速记" @click.stop="onSaveNote(item)">
                <FileText :size="14" :stroke-width="2" />
              </button>
              <button v-if="item.kind === 'text'" class="cb-a" title="加入提示词库" @click.stop="onSavePrompt(item)">
                <Boxes :size="14" :stroke-width="2" />
              </button>
              <button class="cb-a danger" title="删除" @click.stop="onDelete(item)">
                <Trash2 :size="14" :stroke-width="2" />
              </button>
            </div>
          </div>
        </template>
        <div v-else-if="loading" class="cb-empty">加载中…</div>
        <div v-else class="cb-empty">{{ keyword.trim() ? '没有匹配的记录' : '暂无剪贴板记录' }}</div>
      </div>

      <!-- 底部栏 -->
      <div class="cb-footer">
        <div class="cb-hints">
          <span class="cb-kbd">单击 / Enter</span> 粘贴
          <span class="cb-kbd">↑↓</span> 选择
          <span class="cb-kbd">Esc</span> 关闭
        </div>
        <div class="cb-tools">
          <button class="cb-toggle" :class="{ on: info.paused }" @click="onTogglePause">
            <span class="cb-switch"></span>
            {{ info.paused ? '已暂停' : '暂停记录' }}
          </button>
          <button class="cb-clear" title="清空历史" @click="onClear">
            <Trash2 :size="13" :stroke-width="2" />
          </button>
        </div>
      </div>
    </div>

    <!-- 右键菜单 -->
    <Teleport to="body">
      <Transition name="cb-fade">
        <div
          v-if="ctx"
          class="cb-ctx"
          :style="{ left: ctx.x + 'px', top: ctx.y + 'px' }"
          @click.stop
        >
          <div class="cb-ctx-item" @click="onCtxAction(() => onCopy(ctx!.item))">
            <Copy :size="13" :stroke-width="2" /> 复制
          </div>
          <div v-if="ctx.item.kind === 'image'" class="cb-ctx-item" @click="previewImage(ctx.item)">
            <ZoomIn :size="13" :stroke-width="2" /> 查看大图
          </div>
          <div v-if="ctx.item.kind === 'image'" class="cb-ctx-item" @click="onCtxAction(() => onSaveImage(ctx!.item))">
            <Download :size="13" :stroke-width="2" /> 保存图片
          </div>
          <div v-if="ctx.item.kind === 'text'" class="cb-ctx-item" @click="onCtxAction(() => onSaveNote(ctx!.item))">
            <FileText :size="13" :stroke-width="2" /> 存为速记
          </div>
          <div v-if="ctx.item.kind === 'text'" class="cb-ctx-item" @click="onCtxAction(() => onSavePrompt(ctx!.item))">
            <Boxes :size="13" :stroke-width="2" /> 加入提示词库
          </div>
          <div class="cb-ctx-sep"></div>
          <div class="cb-ctx-item" @click="onCtxAction(() => onTogglePin(ctx!.item))">
            <Pin v-if="ctx.item.is_pinned" :size="13" :stroke-width="2" />
            <PinOff v-else :size="13" :stroke-width="2" />
            <span>{{ ctx.item.is_pinned ? '取消置顶' : '置顶' }}</span>
          </div>
          <div class="cb-ctx-item danger" @click="onCtxAction(() => onDelete(ctx!.item))">
            <Trash2 :size="13" :stroke-width="2" /> 删除
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- 图片预览 -->
    <Teleport to="body">
      <Transition name="cb-fade">
        <div v-if="preview" class="cb-preview" @click="closePreview">
          <div class="cb-preview-card" @click.stop>
            <img :src="imageSrc(preview)" class="cb-preview-img" alt="" />
            <button class="cb-preview-close" title="关闭" @click="closePreview">
              <X :size="16" :stroke-width="2" />
            </button>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Toast -->
    <div class="cb-toasts">
      <TransitionGroup name="cb-toast">
        <div v-for="t in toasts" :key="t.id" class="cb-toast">{{ t.text }}</div>
      </TransitionGroup>
    </div>
  </div>
</template>

<style scoped>
.clipboard-overlay {
  position: relative;
  width: 100vw;
  height: 100vh;
  display: flex;
  padding: 0;
  box-sizing: border-box;
  -webkit-app-region: no-drag;
}

.cb-panel {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: 12px;
  box-shadow: var(--shadow-dock);
  backdrop-filter: blur(18px) saturate(140%);
  -webkit-backdrop-filter: blur(18px) saturate(140%);
  overflow: hidden;
  animation: cb-pop 0.16s ease-out;
}
/* 整条选择器包进一个 :global()：「:global(前缀) 后代」写法会被 lightningcss 管线丢弃 */
:global([data-theme="dark"] .cb-panel) {
  background: rgba(30, 31, 44, 0.92);
}
@keyframes cb-pop {
  from {
    opacity: 0;
    transform: scale(0.97) translateY(-4px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

/* 搜索框 */
.cb-search {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
  cursor: grab;
  user-select: none;
}
.cb-search-icon {
  color: var(--text-3);
  flex: none;
}
.cb-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  font: inherit;
  font-size: 0.875rem;
  color: var(--text-1);
}
.cb-input::placeholder {
  color: var(--text-3);
}
.cb-count {
  font-size: 0.6875rem;
  color: var(--text-3);
  flex: none;
  font-variant-numeric: tabular-nums;
}

/* 列表 */
.cb-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 6px;
  scroll-behavior: smooth;
}
.cb-list::-webkit-scrollbar {
  width: 8px;
}
.cb-list::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border-radius: 4px;
  border: 2px solid transparent;
  background-clip: content-box;
}
.cb-section {
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-3);
  padding: 10px 12px 4px;
  letter-spacing: 0.04em;
}
.cb-item {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 9px 10px;
  border-radius: 8px;
  cursor: default;
  transition: background 0.12s ease-out;
}
.cb-item:hover {
  background: var(--bg-card-soft);
}
.cb-item.selected {
  background: var(--brand-50);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent);
}
.cb-pin {
  flex: none;
  color: var(--c-yellow-ink);
  margin-top: 1px;
}
.cb-type {
  flex: none;
  font-size: 0.625rem;
  color: var(--text-3);
  background: var(--bg-card-soft);
  border-radius: 4px;
  padding: 1px 5px;
  margin-top: 1px;
  letter-spacing: 0.02em;
}
.cb-thumb {
  flex: none;
  width: 42px;
  height: 42px;
  border-radius: 6px;
  object-fit: cover;
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  margin-top: 1px;
}
.cb-body {
  flex: 1;
  min-width: 0;
}
.cb-content {
  font-size: 0.8125rem;
  line-height: 1.5;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cb-file-count {
  margin-top: 3px;
  font-size: 0.6875rem;
  color: var(--text-3);
}
.cb-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 3px;
  font-size: 0.6875rem;
  color: var(--text-3);
}
.cb-src {
  max-width: 140px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cb-dot {
  opacity: 0.5;
}
.cb-actions {
  display: flex;
  gap: 2px;
  flex: none;
  opacity: 0;
  transition: opacity 0.12s ease-out;
  align-items: center;
}
.cb-item:hover .cb-actions,
.cb-item.selected .cb-actions {
  opacity: 1;
}
.cb-a {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 6px;
  color: var(--text-3);
  cursor: pointer;
  transition: all 0.12s;
}
.cb-a:hover {
  background: var(--bg-card-solid);
  color: var(--text-1);
  box-shadow: var(--shadow-card);
}
.cb-a.danger:hover {
  color: var(--c-red-ink);
  background: var(--c-red-soft);
}
.cb-empty {
  padding: 44px 0;
  text-align: center;
  color: var(--text-3);
  font-size: 0.75rem;
}

/* 底部 */
.cb-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-top: 1px solid var(--border-soft);
  font-size: 0.6875rem;
  color: var(--text-3);
  flex-shrink: 0;
}
.cb-hints {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.cb-kbd {
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.625rem;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: 4px;
  padding: 1px 5px;
}
.cb-tools {
  display: flex;
  align-items: center;
  gap: 8px;
}
.cb-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: none;
  color: var(--text-3);
  font: inherit;
  font-size: 0.6875rem;
  cursor: pointer;
  border-radius: 4px;
  padding: 2px 4px;
  transition: color 0.12s;
}
.cb-toggle:hover {
  color: var(--text-2);
}
.cb-toggle.on {
  color: var(--c-yellow-ink);
}
.cb-switch {
  width: 26px;
  height: 15px;
  border-radius: 999px;
  background: var(--border-strong);
  position: relative;
  transition: background 0.15s;
  flex: none;
}
.cb-switch::after {
  content: '';
  position: absolute;
  top: 2px;
  left: 2px;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: #fff;
  transition: transform 0.15s ease-out;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
}
.cb-toggle.on .cb-switch {
  background: var(--c-yellow-ink);
}
.cb-toggle.on .cb-switch::after {
  transform: translateX(11px);
}
.cb-clear {
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: none;
  color: var(--text-3);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.12s;
}
.cb-clear:hover {
  color: var(--c-red-ink);
  background: var(--c-red-soft);
}

/* 右键菜单 */
.cb-ctx {
  position: fixed;
  z-index: 100;
  min-width: 160px;
  padding: 5px;
  background: var(--bg-card-solid);
  border-radius: 10px;
  box-shadow: var(--shadow-dock);
  border: 1px solid var(--border-soft);
  backdrop-filter: blur(18px);
  -webkit-backdrop-filter: blur(18px);
}
:global([data-theme="dark"] .cb-ctx) {
  background: rgba(30, 31, 44, 0.96);
}
.cb-ctx-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 6px;
  font-size: 0.75rem;
  color: var(--text-2);
  cursor: pointer;
}
.cb-ctx-item:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.cb-ctx-item.danger {
  color: var(--c-red-ink);
}
.cb-ctx-item.danger:hover {
  background: var(--c-red-soft);
}
.cb-ctx-sep {
  height: 1px;
  background: var(--border-soft);
  margin: 4px 6px;
}

/* 图片预览 */
.cb-preview {
  position: fixed;
  inset: 0;
  z-index: 300;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--scrim);
  padding: 16px;
}
.cb-preview-card {
  position: relative;
  max-width: 100%;
  max-height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.cb-preview-img {
  max-width: 100%;
  max-height: calc(100vh - 32px);
  border-radius: 10px;
  box-shadow: var(--shadow-dock);
  object-fit: contain;
}
.cb-preview-close {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  cursor: pointer;
  transition: background 0.12s;
}
.cb-preview-close:hover {
  background: rgba(0, 0, 0, 0.65);
}

/* Toast */
.cb-toasts {
  position: fixed;
  bottom: 14px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  z-index: 200;
  pointer-events: none;
}
.cb-toast {
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-1);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  padding: 7px 13px;
  box-shadow: var(--shadow-dock);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}
:global([data-theme="dark"] .cb-toast) {
  background: rgba(30, 31, 44, 0.95);
}

.cb-fade-enter-active,
.cb-fade-leave-active {
  transition: opacity 0.12s ease-out;
}
.cb-fade-enter-from,
.cb-fade-leave-to {
  opacity: 0;
}
.cb-toast-enter-active,
.cb-toast-leave-active {
  transition: opacity 0.2s ease-out, transform 0.2s ease-out;
}
.cb-toast-enter-from,
.cb-toast-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
