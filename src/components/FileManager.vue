<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
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
} from 'lucide-vue-next'
import { isTauri, tauriApi, type FileEntry } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'
import { useStore } from '../stores/workbench'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import FileFormDialog from './FileFormDialog.vue'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})
const rootRef = ref<HTMLElement | null>(null)

// ---- 拖拽导入文件/文件夹建链接（仅拖入本区域时响应） ----
const dropping = ref(false)
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
  try {
    const info = await tauriApi.inspectPath(file)
    const category = categorize(file, info.is_dir)
    await store.addFileLink({ name: info.name, path: file, category })
    showToast(`已添加「${info.name}」`)
  } catch (e) {
    showToast(String(e))
  }
}

const CATEGORY_ICONS = {
  文件夹: Folder,
  文档: FileText,
  图片: Image,
  视频: Film,
  音频: Music,
  压缩包: Archive,
  其他: File,
} as const

// ---- 分类选中（默认全部） ----
const activeCategory = ref<string>('全部')

const visibleFiles = computed<readonly FileEntry[]>(() => {
  if (activeCategory.value === '全部') return store.state.files
  return store.state.files.filter((f) => f.category === activeCategory.value)
})

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

function onFileContext(e: MouseEvent, f: FileEntry) {
  e.preventDefault()
  openMenu(e, [
    {
      label: '打开',
      onClick: () => onOpen(f),
    },
    {
      label: '编辑',
      onClick: () => {
        editing.value = f
        formVisible.value = true
      },
    },
    {
      label: '删除',
      danger: true,
      onClick: () => store.removeFileLink(f.id),
    },
  ])
}

// ---- 弹窗 ----
const formVisible = ref(false)
const editing = ref<FileEntry | null>(null)

async function onOpen(f: FileEntry) {
  try {
    await store.openFile(f.path)
  } catch {
    showToast(`无法打开「${f.name}」`)
  }
}

function onFormSubmit(payload: {
  id?: number
  name: string
  path: string
  category: string
}) {
  if (payload.id != null) {
    store.editFileLink(payload.id, payload.name, payload.category)
  } else {
    store.addFileLink({ name: payload.name, path: payload.path, category: payload.category })
  }
}
</script>

<template>
  <section ref="rootRef" class="card file-manager">
    <header class="fm-header">
      <h2 class="fm-title">文件管理</h2>
      <button
        class="icon-btn add"
        title="添加文件链接"
        @click="editing = null; formVisible = true"
      >
        <Plus :size="15" :stroke-width="2.2" />
      </button>
    </header>

    <!-- 分类 tabs（选中为黑色背景块） -->
    <nav class="cat-tabs" aria-label="文件分类">
      <button
        class="cat-tab"
        :class="{ active: activeCategory === '全部' }"
        @click="activeCategory = '全部'"
      >
        全部
      </button>
      <button
        v-for="c in CATEGORIES"
        :key="c"
        class="cat-tab"
        :class="{ active: activeCategory === c }"
        @click="activeCategory = c"
      >
        {{ c }}
      </button>
    </nav>

    <!-- 文件网格 -->
    <div class="fm-body">
      <div v-if="visibleFiles.length > 0" class="file-grid">
        <div
          v-for="f in visibleFiles"
          :key="f.id"
          class="file-card"
          :title="f.path"
          @click="onOpen(f)"
          @contextmenu="onFileContext($event, f)"
        >
          <div class="file-actions">
            <button
              class="file-action"
              title="编辑"
              @click.stop="editing = f; formVisible = true"
            >
              <Pencil :size="11" :stroke-width="2" />
            </button>
            <button
              class="file-action del"
              title="删除"
              @click.stop="store.removeFileLink(f.id)"
            >
              <Trash2 :size="11" :stroke-width="2" />
            </button>
          </div>
          <component
            :is="CATEGORY_ICONS[f.category as keyof typeof CATEGORY_ICONS] ?? File"
            class="file-icon"
            :size="30"
            :stroke-width="1.5"
          />
          <span class="file-name">{{ f.name }}</span>
          <span class="file-cat">{{ f.category }}</span>
        </div>
      </div>

      <div v-else class="empty-state">
        <span style="font-size: 28px">📁</span>
        <p>{{ activeCategory === '全部' ? '还没有文件链接' : `暂无「${activeCategory}」分类文件` }}</p>
        <p style="font-size: 12px; color: var(--text-4)">
          添加文件夹或文件的快捷链接，源文件不会移动
        </p>
        <button
          class="pill-btn"
          style="padding: 7px 18px; margin-top: 6px"
          @click="editing = null; formVisible = true"
        >
          添加文件
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
    <FileFormDialog
      :visible="formVisible"
      :editing="editing"
      @close="formVisible = false"
      @submit="onFormSubmit"
    />

    <!-- 拖拽导入遮罩 -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping" class="drop-overlay">
          <div class="drop-hint">
            <FilePlus :size="34" :stroke-width="1.5" />
            <p>释放以添加文件链接</p>
            <span>支持任意文件或文件夹，源文件不会移动</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.file-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
  min-height: 0;
}
.fm-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.fm-title {
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
  color: #fff;
}

/* 分类 tabs */
.cat-tabs {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 2px;
  margin-bottom: 14px;
  scrollbar-width: none;
}
.cat-tabs::-webkit-scrollbar {
  display: none;
}
.cat-tab {
  flex-shrink: 0;
  border: none;
  background: transparent;
  padding: 5px 12px;
  border-radius: var(--radius-pill);
  font-size: 12px;
  font-weight: 500;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  white-space: nowrap;
}
.cat-tab:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
/* 选中：黑色背景块（spec §4.6 Tab） */
.cat-tab.active {
  background: #1a1a1f;
  color: #fff;
}
[data-theme="dark"] .cat-tab.active {
  background: #f2f2f7;
  color: #1a1a1f;
}

/* 文件网格 */
.fm-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.file-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 10px;
}
.file-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 16px 8px 12px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
}
.file-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-hover);
}
.file-icon {
  color: var(--brand-500);
}
.file-card:hover .file-icon {
  color: var(--brand-600);
}
.file-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-2);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-cat {
  font-size: 10px;
  color: var(--text-4);
  background: var(--bg-card);
  border-radius: var(--radius-pill);
  padding: 1px 8px;
}

.file-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.15s;
}
.file-card:hover .file-actions {
  opacity: 1;
}
.file-action {
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
.file-action:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.file-action.del:hover {
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
