<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { convertFileSrc } from '@tauri-apps/api/core'
import {
  Archive,
  File,
  FilePlus,
  FileText,
  Film,
  Folder,
  Image,
  Music,
  Pencil,
  Plus,
  Trash2,
  Wrench,
} from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'
import { useStore } from '../stores/workbench'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import SudaFormDialog from './SudaFormDialog.vue'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})
const rootRef = ref<HTMLElement | null>(null)

// ---- 拖拽导入：exe/lnk 预填为应用，其他文件/文件夹直接建链接 ----
const dropping = ref(false)
const prefill = ref<{ name?: string; target?: string; icon?: string | null; kind?: 'app' | 'web' } | null>(null)
let unlistenDrop: (() => void) | null = null

function isInside(e: { x: number; y: number }): boolean {
  const el = rootRef.value
  if (!el) return false
  const rect = el.getBoundingClientRect()
  const dpr = window.devicePixelRatio || 1
  const x = e.x / dpr
  const y = e.y / dpr
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

onMounted(async () => {
  if (!isTauri()) return
  const webview = getCurrentWebview()
  unlistenDrop = await webview.onDragDropEvent((event) => {
    const ev = event.payload
    if (ev.type === 'enter' || ev.type === 'over') {
      dropping.value = isInside(ev.position)
    } else if (ev.type === 'leave') {
      dropping.value = false
    } else if (ev.type === 'drop') {
      dropping.value = false
      if (!isInside(ev.position)) return
      const file = ev.paths?.[0]
      if (file) void handleDrop(file)
    }
  })
})

onBeforeUnmount(() => {
  unlistenDrop?.()
})

async function handleDrop(file: string) {
  const ext = file.split('.').pop()?.toLowerCase()
  if (ext === 'exe' || ext === 'lnk') {
    try {
      const info = await tauriApi.parseDroppedPath(file)
      prefill.value = { name: info.name, target: info.target, icon: info.icon, kind: 'app' }
      editing.value = null
      formVisible.value = true
    } catch (e) {
      showToast(String(e))
    }
    return
  }
  try {
    const info = await tauriApi.inspectPath(file)
    const category = categorize(file, info.is_dir)
    await store.addResource({ kind: 'file', name: info.name, target: file, category })
    showToast(`已添加「${info.name}」`)
  } catch (e) {
    showToast(String(e))
  }
}

// ---- 分类筛选 ----
type FilterKey = '全部' | '常用' | '应用' | '网页' | (typeof CATEGORIES)[number]

const activeFilter = ref<FilterKey>('全部')

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
  return all.filter((r) => r.kind === 'file' && r.category === activeFilter.value)
})

const FILTER_TABS: FilterKey[] = ['全部', '常用', '应用', '网页', ...CATEGORIES]

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

function onResourceContext(e: MouseEvent, r: Resource) {
  e.preventDefault()
  openMenu(e, [
    {
      label: '打开',
      onClick: () => onOpen(r),
    },
    {
      label: '编辑',
      onClick: () => {
        editing.value = r
        formVisible.value = true
      },
    },
    {
      label: '删除',
      danger: true,
      onClick: () => store.removeResource(r.id),
    },
  ])
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
    store.editResource({ ...payload, id: payload.id })
  } else {
    store.addResource(payload)
  }
  prefill.value = null
}

// ---- 图标渲染 ----
const CATEGORY_ICONS = {
  文件夹: Folder,
  文档: FileText,
  图片: Image,
  视频: Film,
  音频: Music,
  压缩包: Archive,
  其他: File,
} as const

const CATEGORY_ACCENTS = {
  文件夹: { soft: 'var(--c-yellow-soft)', strong: 'var(--c-yellow)', ink: 'var(--c-yellow-ink)' },
  文档: { soft: 'var(--c-purple-soft)', strong: 'var(--c-purple)', ink: 'var(--c-purple-ink)' },
  图片: { soft: 'var(--c-pink-soft)', strong: 'var(--c-pink)', ink: 'var(--c-pink-ink)' },
  视频: { soft: 'var(--c-blue-soft)', strong: 'var(--c-blue)', ink: 'var(--c-blue-ink)' },
  音频: { soft: 'var(--c-green-soft)', strong: 'var(--c-green)', ink: 'var(--c-green-ink)' },
  压缩包: { soft: 'var(--c-orange-soft)', strong: 'var(--c-orange)', ink: 'var(--c-orange-ink)' },
  其他: { soft: 'var(--c-gray-soft)', strong: 'var(--c-gray)', ink: 'var(--c-gray-ink)' },
} as const

const ACCENTS = [
  { strong: 'var(--c-yellow)', soft: 'var(--c-yellow-soft)', text: 'var(--c-yellow-ink)' },
  { strong: 'var(--c-red)', soft: 'var(--c-red-soft)', text: 'var(--c-red-ink)' },
  { strong: 'var(--c-blue)', soft: 'var(--c-blue-soft)', text: 'var(--c-blue-ink)' },
  { strong: 'var(--c-green)', soft: 'var(--c-green-soft)', text: 'var(--c-green-ink)' },
  { strong: 'var(--c-pink)', soft: 'var(--c-pink-soft)', text: 'var(--c-pink-ink)' },
  { strong: 'var(--c-orange)', soft: 'var(--c-orange-soft)', text: 'var(--c-orange-ink)' },
  { strong: 'var(--c-purple)', soft: 'var(--c-purple-soft)', text: 'var(--c-purple-ink)' },
  { strong: 'var(--c-gray)', soft: 'var(--c-gray-soft)', text: 'var(--c-gray-ink)' },
]

function accentOf(name: string) {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return ACCENTS[h % ACCENTS.length]
}

function fileAccentOf(category: string) {
  return CATEGORY_ACCENTS[category as keyof typeof CATEGORY_ACCENTS] ?? CATEGORY_ACCENTS.其他
}

const IMAGE_ICON_RE = /\.(png|jpg|jpeg|ico|gif|webp)$/i

function isImageIcon(icon: string | null): boolean {
  return !!icon && IMAGE_ICON_RE.test(icon)
}

function iconSrc(icon: string): string {
  return isTauri() ? convertFileSrc(icon) : ''
}

const failedIcons = ref(new Set<number>())

function onIconError(r: Resource) {
  failedIcons.value.add(r.id)
}

function showImageIcon(r: Resource): boolean {
  return isImageIcon(r.icon) && !failedIcons.value.has(r.id)
}

function iconText(r: Resource): string {
  return r.name.charAt(0).toUpperCase()
}

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

function fileIconOf(r: Resource) {
  return CATEGORY_ICONS[(r.category ?? '其他') as keyof typeof CATEGORY_ICONS] ?? File
}
</script>

<template>
  <section ref="rootRef" class="card suda">
    <header class="suda-header">
      <h2 class="suda-title">速达</h2>
      <button
        class="icon-btn add"
        title="添加"
        @click="editing = null; prefill = null; formVisible = true"
      >
        <Plus :size="15" :stroke-width="2.2" />
      </button>
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
              @click.stop="editing = r; formVisible = true"
            >
              <Pencil :size="11" :stroke-width="2" />
            </button>
            <button
              class="suda-action del"
              title="删除"
              @click.stop="store.removeResource(r.id)"
            >
              <Trash2 :size="11" :stroke-width="2" />
            </button>
          </div>
          <div
            class="suda-icon"
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
          <span class="suda-name">{{ r.name }}</span>
        </div>
      </div>

      <div v-else class="empty-state">
        <Wrench :size="24" :stroke-width="1.7" aria-hidden="true" />
        <p>{{ activeFilter === '全部' ? '还没有速达资源' : `暂无「${activeFilter}」资源` }}</p>
        <p style="font-size: 12px; color: var(--text-4)">
          添加本地程序、网页书签或文件快捷链接
        </p>
        <button
          class="pill-btn"
          style="padding: 7px 18px; margin-top: 6px"
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

    <!-- 拖拽导入遮罩 -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping" class="drop-overlay">
          <div class="drop-hint">
            <FilePlus :size="34" :stroke-width="1.5" />
            <p>释放以添加</p>
            <span>支持本地程序 / 网页 / 任意文件或文件夹</span>
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
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
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

.suda-tabs {
  margin-bottom: 14px;
}

.suda-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.suda-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 10px;
}
.suda-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 7px;
  padding: 15px 8px 12px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
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
.suda-letter {
  font-size: 20px;
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
  font-size: 12px;
  font-weight: 500;
  color: var(--text-2);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.suda-kind {
  position: absolute;
  top: 6px;
  left: 6px;
  display: inline-flex;
  align-items: center;
  min-height: 18px;
  padding: 2px 7px;
  border-radius: var(--radius-pill);
  font-size: 10px;
  font-weight: 600;
  line-height: 1;
  background: var(--bg-card);
  color: var(--text-3);
}
.suda-kind.app {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.suda-kind.web {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.suda-kind.file {
  background: var(--c-purple-soft);
  color: var(--c-purple-ink);
}

.suda-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.15s;
}
.suda-card:hover .suda-actions {
  opacity: 1;
}
.suda-action {
  width: 20px;
  height: 20px;
  border: none;
  background: var(--bg-card);
  border-radius: 6px;
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
  backdrop-filter: blur(2px);
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
  font-size: 15px;
  font-weight: 600;
}
.drop-hint span {
  font-size: 12px;
  color: var(--text-3);
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
