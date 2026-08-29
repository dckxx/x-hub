<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  FilePlus,
  Globe,
  Loader2,
  Pencil,
  Plus,
  ScanSearch,
  Trash2,
  Wrench,
} from 'lucide-vue-next'
import { isTauri, tauriApi, type InstalledAppInfo, type InstalledBrowser, type Resource } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'
import { accentOf, fileAccentOf, iconSrc, useResourceIcon } from '../composables/useResourceIcon'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import SudaFormDialog from './SudaFormDialog.vue'
import SudaScanDialog from './SudaScanDialog.vue'

const store = useStore()
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)
const rootRef = ref<HTMLElement | null>(null)
void rootRef
const hasOverlayModal = computed(() => formVisible.value || menu.value.visible || scanVisible.value)
const { onIconError, showImageIcon, showWebFallbackIcon, iconText, fileIconOf } =
  useResourceIcon()

// ---- 拖拽导入：只预填弹窗，用户确认后才真正添加 ----
const dropping = ref(false)
// 程序解析期间的文件名（exe/lnk 解析需启动 PowerShell，期间保持遮罩提示正在识别）
const parsing = ref<string | null>(null)
const prefill = ref<{
  name?: string
  target?: string
  icon?: string | null
  kind?: 'app' | 'web' | 'file'
  category?: string | null
  isDir?: boolean
} | null>(null)
let unlistenDrop: (() => void) | null = null

onMounted(async () => {
  void installedBrowsers() // 预热浏览器列表，右键菜单即开即用
  if (!isTauri()) return
  const webview = getCurrentWebview()
  unlistenDrop = await webview.onDragDropEvent((event) => {
    const ev = event.payload
    if (hasOverlayModal.value || parsing.value) return
    if (ev.type === 'enter' || ev.type === 'over') {
      if (ev.type === 'over') return
      if (!ev.paths.length) return
      // 整个窗口均可拖拽导入：enter 即亮起全屏遮罩，
      // 释放时不再校验位置（遮罩提示居中，用户常移到提示处释放，二次校验会误丢 drop）
      dropping.value = true
    } else if (ev.type === 'leave') {
      dropping.value = false
    } else if (ev.type === 'drop') {
      dropping.value = false
      const file = ev.paths?.[0]
      if (file) {
        // 程序解析耗时较长：立刻切到“正在识别”遮罩，避免无反馈空白期
        const ext = file.split('.').pop()?.toLowerCase()
        if (ext === 'exe' || ext === 'lnk') {
          parsing.value = file.split(/[\\/]/).pop() ?? file
        }
        void handleDrop(file)
      }
    }
  })
})

onBeforeUnmount(() => {
  unlistenDrop?.()
  if (runningTimer) {
    clearInterval(runningTimer)
    runningTimer = null
  }
})

// ---- 拖拽去重：目标路径与现有资源一致即视为重复（统一分隔符/大小写后比较） ----
function normalizeTarget(p: string): string {
  return p.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase()
}
function findDuplicateTarget(target: string): Resource | undefined {
  const key = normalizeTarget(target)
  return store.state.resources.find((r) => r.target && normalizeTarget(r.target) === key)
}

async function handleDrop(file: string) {
  // 命中已有资源的路径直接提示跳过：exe/lnk 还可省去 PowerShell 解析
  const direct = findDuplicateTarget(file)
  if (direct) {
    showToast(`「${direct.name}」已在速达中，已跳过重复添加`)
    return
  }
  const ext = file.split('.').pop()?.toLowerCase()
  if (ext === 'exe' || ext === 'lnk') {
    parsing.value ||= file.split(/[\\/]/).pop() ?? file
    try {
      const info = await tauriApi.parseDroppedPath(file)
      // .lnk 解析出的目标 exe 可能已用别的方式添加过（如另一个指向同一程序的快捷方式）
      const dup = findDuplicateTarget(info.target)
      if (dup) {
        showToast(`「${dup.name}」已在速达中，已跳过重复添加`)
        return
      }
      prefill.value = { name: info.name, target: info.target, icon: info.icon, kind: 'app' }
      editing.value = null
      formVisible.value = true
      showToast(`已识别「${info.name}」，请点击添加确认`)
    } catch (e) {
      showToast(String(e))
    } finally {
      parsing.value = null
    }
    return
  }
  try {
    const info = await tauriApi.inspectPath(file)
    const category = categorize(file, info.is_dir)
    prefill.value = { name: info.name, target: file, kind: 'file', category, isDir: info.is_dir }
    editing.value = null
    formVisible.value = true
    showToast(`已识别「${info.name}」，请点击添加确认`)
  } catch (e) {
    void reportClientError('速达拖拽解析失败', e)
    showToast(String(e))
  }
}

// ---- 运行状态检测：每 3s 轮询进程名集合，应用已启动时名称左侧显示小绿点 ----
const runningNames = ref<Set<string>>(new Set())
let runningTimer: ReturnType<typeof setInterval> | null = null

function isRunning(r: Resource): boolean {
  if (r.kind !== 'app' || !r.target) return false
  const file = r.target.split(/[\\/]/).pop()?.toLowerCase()
  return !!file && runningNames.value.has(file)
}

async function refreshRunning() {
  if (!isTauri()) return
  try {
    const names = await tauriApi.getRunningProcesses()
    runningNames.value = new Set(names)
  } catch {
    // 静默失败，下一轮重试
  }
}

onMounted(() => {
  if (!isTauri()) return
  void refreshRunning()
  runningTimer = setInterval(() => void refreshRunning(), 3000)
})

// ---- 分类筛选 ----
type FilterKey = '全部' | '常用' | '应用' | '网页' | '文件'

const activeFilter = ref<FilterKey>('全部')
const activeCategory = ref<string | null>(null)

const visibleResources = computed<Resource[]>(() => {
  const all = store.state.resources
  if (activeFilter.value === '全部') return [...all]
  if (activeFilter.value === '常用') {
    return all
      .filter((r) => r.last_launched_at)
      .slice()
      .sort(
        (a, b) =>
          new Date(b.last_launched_at!).getTime() - new Date(a.last_launched_at!).getTime(),
      )
  }
  if (activeFilter.value === '应用') return all.filter((r) => r.kind === 'app')
  if (activeFilter.value === '网页') return all.filter((r) => r.kind === 'web')
  return all.filter(
    (r) =>
      r.kind === 'file' &&
      (activeCategory.value === null || r.category === activeCategory.value),
  )
})

const FILTER_TABS: FilterKey[] = ['全部', '常用', '应用', '网页', '文件']

const emptyTitle = computed(() => {
  if (activeFilter.value === '全部') return '还没有速达资源'
  if (activeFilter.value === '文件' && activeCategory.value) {
    return `暂无「${activeCategory.value}」分类资源`
  }
  return `暂无「${activeFilter.value}」资源`
})

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  // 必须延迟到当前事件派发结束后再置位：ContextMenu 在 window 上监听 contextmenu/click
  // 用于点击别处关闭菜单，若在同一事件派发内同步置位，紧跟的全局关闭监听会在
  // props 更新后立即把菜单关掉（表现为右键无反应）；已开时右键另一资源也无法重定位
  setTimeout(() => {
    menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
  }, 0)
}

async function onDeleteResource(r: Resource) {
  await store.removeResource(r.id)
  showToast(`已删除「${r.name}」`, {
    label: '撤销',
    onClick: async () => {
      await store.addResource({
        kind: r.kind,
        name: r.name,
        target: r.target,
        category: r.category,
        icon: r.icon,
        args: r.args,
      })
      showToast('已恢复')
    },
  })
}

// ---- 指定浏览器打开（网页资源）：列表来自本机已安装浏览器（Rust 注册表枚举） ----
let browserCache: InstalledBrowser[] | null = null

async function installedBrowsers(): Promise<InstalledBrowser[]> {
  if (browserCache === null) {
    try {
      browserCache = isTauri() ? await tauriApi.listInstalledBrowsers() : []
    } catch {
      browserCache = []
    }
  }
  return browserCache
}

async function onOpenWithBrowser(r: Resource, b: InstalledBrowser) {
  try {
    await store.openResourceInBrowser(r.id, b.exe)
  } catch (e) {
    showToast(`无法用「${b.name}」打开：${String(e)}`)
  }
}

async function onResourceContext(e: MouseEvent, r: Resource) {
  e.preventDefault()
  const items: ContextMenuItem[] = [{ label: '打开', onClick: () => onOpen(r) }]
  let hasBrowsers = false
  if (r.kind === 'web') {
    const browsers = await installedBrowsers()
    hasBrowsers = browsers.length > 0
    for (const [i, b] of browsers.entries()) {
      items.push({
        label: `用 ${b.name} 打开`,
        dividerBefore: i === 0,
        onClick: () => void onOpenWithBrowser(r, b),
      })
    }
  }
  items.push({
    label: '编辑',
    dividerBefore: hasBrowsers,
    onClick: () => {
      editing.value = r
      formVisible.value = true
    },
  })
  items.push({
    label: '删除',
    danger: true,
    onClick: () => void onDeleteResource(r),
  })
  openMenu(e, items)
}

// ---- 弹窗 ----
const formVisible = ref(false)
const editing = ref<Resource | null>(null)

async function onOpen(r: Resource) {
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法打开「${r.name}」：${String(e)}`)
  }
}

function onFormSubmit(payload: {
  id?: number
  kind: 'app' | 'web' | 'file'
  name: string
  target: string
  category?: string | null
  icon?: string | null
  args?: string | null
}) {
  if (payload.id != null) {
    void store.editResource({ ...payload, id: payload.id })
    showToast(`已更新「${payload.name}」`)
  } else {
    void store.addResource(payload)
    showToast(`已添加「${payload.name}」`)
  }
  prefill.value = null
}

// ---- 扫描已安装应用导入 ----
const scanVisible = ref(false)
const importing = ref(false)

async function onScanImported(apps: InstalledAppInfo[]) {
  if (importing.value) return
  importing.value = true
  let added = 0
  let skipped = 0
  for (const a of apps) {
    // 二次去重保护：目标路径已存在则跳过（弹窗中已禁用，这里兜底）
    if (
      store.state.resources.some(
        (r) => r.kind === 'app' && r.target.toLowerCase() === a.target.toLowerCase(),
      )
    ) {
      skipped++
      continue
    }
    try {
      await store.addResource({
        kind: 'app',
        name: a.name,
        target: a.target,
        category: null,
        icon: a.icon,
        args: null,
      })
      added++
    } catch (e) {
      void reportClientError('速达扫描导入失败', e)
    }
  }
  importing.value = false
  showToast(skipped > 0 ? `已添加 ${added} 个应用，跳过 ${skipped} 个已存在` : `已添加 ${added} 个应用`)
}

// ---- 图标渲染（统一在 useResourceIcon composable） ----

function kindLabel(r: Resource): string {
  if (r.kind === 'file') return r.category ?? '文件'
  return r.kind === 'app' ? '应用' : '网页'
}

function cardAccentStyle(r: Resource) {
  if (r.kind === 'file') {
    const a = fileAccentOf(r.category ?? '其他')
    return {
      '--suda-accent-soft': a.soft,
      '--suda-accent': a.strong,
      '--suda-accent-ink': a.ink,
    }
  }
  const a = accentOf(r.name)
  return {
    '--suda-accent-soft': a.soft,
    '--suda-accent': a.strong,
    '--suda-accent-ink': a.text,
  }
}
</script>

<template>
  <section ref="rootRef" class="card suda">
    <header class="suda-header">
      <h2 class="suda-title">速达</h2>
      <div class="suda-header-actions">
        <button
          v-if="isTauri()"
          class="icon-btn scan"
          title="扫描已安装应用"
          aria-label="扫描已安装应用"
          @click="scanVisible = true"
        >
          <ScanSearch :size="15" :stroke-width="2.2" />
        </button>
        <button
          class="icon-btn add"
          title="添加"
          @click="editing = null; prefill = null; formVisible = true"
        >
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <!-- 分类 tabs -->
    <nav class="filter-tabs suda-tabs" aria-label="速达分类">
      <button
        v-for="f in FILTER_TABS"
        :key="f"
        class="filter-tab filter-tab--primary"
        :class="{ active: activeFilter === f }"
        @click="activeFilter = f"
      >
        {{ f }}
      </button>
    </nav>

    <!-- 文件二级分类（仅文件视图） -->
    <nav v-if="activeFilter === '文件'" class="filter-tabs suda-cat-tabs" aria-label="文件分类">
      <button
        class="filter-tab filter-tab--tag"
        :class="{ active: activeCategory === null }"
        @click="activeCategory = null"
      >
        全部文件
      </button>
      <button
        v-for="c in CATEGORIES"
        :key="c"
        class="filter-tab filter-tab--tag"
        :class="{ active: activeCategory === c }"
        @click="activeCategory = c"
      >
        {{ c }}
      </button>
    </nav>

    <!-- 资源网格（5 列） -->
    <div class="suda-body">
      <div v-if="visibleResources.length > 0" class="suda-grid">
        <div
          v-for="r in visibleResources"
          :key="r.id"
          class="suda-card"
          :title="r.target"
          role="button"
          tabindex="0"
          :style="cardAccentStyle(r)"
          @click="onOpen(r)"
          @keydown.enter="onOpen(r)"
          @keydown.space.prevent="onOpen(r)"
          @contextmenu="onResourceContext($event, r)"
        >
          <span class="suda-kind" :class="r.kind">{{ kindLabel(r) }}</span>
          <div class="suda-actions">
            <button
              class="suda-action"
              title="编辑"
              aria-label="编辑"
              @click.stop="editing = r; formVisible = true"
            >
              <Pencil :size="11" :stroke-width="2" />
            </button>
            <button
              class="suda-action del"
              title="删除"
              aria-label="删除"
              @click.stop="onDeleteResource(r)"
            >
              <Trash2 :size="11" :stroke-width="2" />
            </button>
          </div>
            <div
              class="suda-icon"
              :class="{ 'web-default': showWebFallbackIcon(r) }"
              :style="
                showImageIcon(r)
                  ? {}
                  : { background: 'var(--suda-accent-soft)' }
            "
          >
            <img
              v-if="showImageIcon(r)"
              class="suda-img"
              :src="iconSrc(r.icon!)"
              alt=""
              @error="onIconError(r)"
            />
            <Globe
              v-else-if="showWebFallbackIcon(r)"
              class="suda-file-icon"
              :size="25"
              :stroke-width="1.7"
              :style="{ color: 'var(--c-green-ink)' }"
            />
            <component
              v-else-if="r.kind === 'file'"
              :is="fileIconOf(r)"
              class="suda-file-icon"
              :size="25"
              :stroke-width="1.7"
              :style="{ color: 'var(--suda-accent)' }"
            />
            <span
              v-else
              class="suda-letter"
              :style="{ color: 'var(--suda-accent-ink)' }"
            >
              {{ iconText(r) }}
            </span>
          </div>
          <span class="suda-name">
            <span v-if="isRunning(r)" class="suda-dot" title="运行中" />
                <span class="suda-name-text" :title="r.name">{{ r.name }}</span>
          </span>
        </div>
      </div>

      <div v-else class="empty-state">
        <Wrench :size="24" :stroke-width="1.7" aria-hidden="true" />
        <p>{{ emptyTitle }}</p>
        <p style="font-size: 0.75rem; color: var(--text-4)">
          拖拽本地文件/程序到窗口，或手动添加快捷链接
        </p>
        <button
          class="pill-btn"
          style="margin-top: 6px"
          @click="editing = null; prefill = null; formVisible = true"
        >
          添加
        </button>
      </div>
    </div>

    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />
    <SudaFormDialog
      :visible="formVisible"
      :editing="editing"
      :prefill="prefill"
      @close="formVisible = false"
      @submit="onFormSubmit"
    />
    <SudaScanDialog
      :visible="scanVisible"
      @close="scanVisible = false"
      @imported="onScanImported"
    />

    <!-- 拖拽导入遮罩（dropping = 拖拽中；parsing = 正在识别程序） -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping || parsing" class="drop-overlay">
          <div class="drop-hint">
            <Loader2 v-if="parsing" :size="34" :stroke-width="1.5" class="spin" />
            <FilePlus v-else :size="34" :stroke-width="1.5" />
            <p v-if="parsing">正在识别…</p>
            <p v-else>释放以添加</p>
            <span v-if="parsing" :title="parsing">{{ parsing }}</span>
            <span v-else>支持本地程序 / 网页 / 任意文件或文件夹</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.suda {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
  min-height: 0;
}
.suda-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.suda-title {
  font-size: 1rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.suda-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.icon-btn.add {
  width: 30px;
  height: 30px;
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-btn.add:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.icon-btn.scan {
  width: 30px;
  height: 30px;
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.icon-btn.scan:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

.suda-tabs {
  margin-bottom: 10px;
}
.suda-cat-tabs {
  margin-bottom: 14px;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--border-soft);
}

.suda-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.suda-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, 124px);
  justify-content: space-between;
  gap: 10px;
}
.suda-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 7px;
  width: 124px;
  min-width: 0;
  padding: 15px 8px 12px;
  /* 比外层玻璃面板更实的底 + 描边 + 投影，与面板拉开层次 */
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
}
.suda-card:hover .suda-kind,
.suda-card:focus-within .suda-kind {
  color: var(--text-1);
}
.suda-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-hover);
}
.suda-icon {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.18s ease-out, background 0.18s ease-out;
}
.suda-card:hover .suda-icon {
  transform: scale(1.06);
}
.suda-file-icon {
  background: transparent;
}
.suda-icon.web-default {
  background: var(--c-green-soft);
}
.suda-letter {
  font-size: 1.25rem;
  font-weight: 700;
}
.suda-img {
  width: 46px;
  height: 46px;
  border-radius: 14px;
  object-fit: contain;
  background: var(--bg-card);
}
.suda-name {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  font-size: 0.75rem;
  font-weight: 500;
  color: var(--text-2);
}
.suda-name-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.suda-dot {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--c-green);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--c-green) 22%, transparent);
}
.suda-kind {
  position: absolute;
  top: 6px;
  left: 6px;
  display: inline-flex;
  align-items: center;
  min-height: 0;
  padding: 0;
  font-size: 0.625rem;
  font-weight: 600;
  line-height: 1;
  color: var(--text-3);
}
.suda-kind.app {
  color: var(--c-blue-ink);
}
.suda-kind.web {
  color: var(--c-green-ink);
}
.suda-kind.file {
  color: var(--c-purple-ink);
}

.suda-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.15s;
}
.suda-card:hover .suda-actions,
.suda-card:focus-within .suda-actions {
  opacity: 1;
}
.suda-action {
  width: 28px;
  height: 28px;
  border: none;
  background: var(--bg-card);
  border-radius: 7px;
  box-shadow: var(--shadow-card);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.12s, color 0.12s;
}
.suda-action:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.suda-action.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}

/* 拖拽导入遮罩 */
.drop-overlay {
  position: fixed;
  inset: 0;
  z-index: 250;
  background: color-mix(in srgb, var(--brand-500) 10%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drop-hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 32px 48px;
  background: var(--bg-card);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-dock);
  border: 2px dashed var(--brand-500);
  color: var(--brand-500);
}
.drop-hint p {
  font-size: 0.9375rem;
  font-weight: 600;
}
.drop-hint span {
  font-size: 0.75rem;
  color: var(--text-3);
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.spin {
  animation: drop-spin 0.9s linear infinite;
}
@keyframes drop-spin {
  to {
    transform: rotate(360deg);
  }
}

.drop-enter-active,
.drop-leave-active {
  transition: opacity 0.15s ease-out;
}
.drop-enter-from,
.drop-leave-to {
  opacity: 0;
}
</style>
