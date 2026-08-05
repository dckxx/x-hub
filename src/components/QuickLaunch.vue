<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { convertFileSrc } from '@tauri-apps/api/core'
import { FilePlus, FolderPlus, Pencil, Plus, Trash2, Wrench } from 'lucide-vue-next'
import { isTauri, tauriApi, type Group, type Resource } from '../api/tauri'
import { useStore } from '../stores/workbench'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import GroupFormDialog from './GroupFormDialog.vue'
import ResourceFormDialog from './ResourceFormDialog.vue'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})
const rootRef = ref<HTMLElement | null>(null)

// 判断拖拽位置是否落在本组件区域内（物理坐标 → CSS 坐标）
function isInside(e: { x: number; y: number }): boolean {
  const el = rootRef.value
  if (!el) return false
  const rect = el.getBoundingClientRect()
  const dpr = window.devicePixelRatio || 1
  const x = e.x / dpr
  const y = e.y / dpr
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

// ---- 拖拽导入本地应用（exe / lnk，仅拖入本区域时响应） ----
const dropping = ref(false)
const prefill = ref<{ name?: string; target?: string } | null>(null)
let unlistenDrop: (() => void) | null = null

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
    const info = await tauriApi.parseDroppedPath(file)
    prefill.value = { name: info.name, target: info.target }
    editing.value = null
    resourceDialogVisible.value = true
  } catch (e) {
    showToast(String(e))
  }
}

// ---- 分组选中 ----
const activeGroupId = ref<number | 'all'>('all')

const visibleResources = computed(() => {
  if (activeGroupId.value === 'all') return store.state.resources
  return store.state.resources.filter((r) => r.group_id === activeGroupId.value)
})

// ---- 拖拽排序（分组 tabs，视觉占位模式） ----
const dragGroupId = ref<number | null>(null)
const dragGroupIndex = ref<number | null>(null)

const displayGroups = computed(() => {
  const base = store.state.groups
  if (dragGroupId.value === null) return base
  const list = base.filter((g) => g.id !== dragGroupId.value)
  const idx = dragGroupIndex.value ?? list.length
  const result: (Group | null)[] = [...list.slice(0, idx), null, ...list.slice(idx)]
  return result
})

function onGroupDragStart(gid: number) {
  dragGroupId.value = gid
  dragGroupIndex.value = null
}

function onGroupDragOver(e: DragEvent, gid: number) {
  if (dragGroupId.value === null) return
  const list = store.state.groups.filter((g) => g.id !== dragGroupId.value)
  let idx = list.findIndex((g) => g.id === gid)
  const el = e.currentTarget as HTMLElement
  if (e.offsetY > el.offsetHeight / 2) idx += 1
  dragGroupIndex.value = idx
}

async function onGroupDragEnd() {
  if (dragGroupId.value !== null) {
    const order = displayGroups.value
      .filter((g): g is Group => g !== null)
      .map((g) => g.id)
    await store.moveGroups(order)
  }
  dragGroupId.value = null
  dragGroupIndex.value = null
}

// ---- 拖拽排序（资源卡片，视觉占位模式） ----
const dragResId = ref<number | null>(null)
const dragResIndex = ref<number | null>(null)

const displayResources = computed(() => {
  const base = visibleResources.value
  if (dragResId.value === null) return base
  const list = base.filter((r) => r.id !== dragResId.value)
  const idx = dragResIndex.value ?? list.length
  const result: (Resource | null)[] = [...list.slice(0, idx), null, ...list.slice(idx)]
  return result
})

function onResDragStart(rid: number) {
  dragResId.value = rid
  dragResIndex.value = null
}

function onResDragOver(e: DragEvent, rid: number) {
  if (dragResId.value === null) return
  const list = visibleResources.value.filter((r) => r.id !== dragResId.value)
  let idx = list.findIndex((r) => r.id === rid)
  const el = e.currentTarget as HTMLElement
  if (e.offsetY > el.offsetHeight / 2) idx += 1
  dragResIndex.value = idx
}

async function onResDragEnd() {
  if (dragResId.value !== null) {
    const order = displayResources.value
      .filter((r): r is Resource => r !== null)
      .map((r) => r.id)
    const groupId = activeGroupId.value === 'all' ? null : activeGroupId.value
    if (groupId !== null) {
      await store.moveResources(groupId, order)
    }
  }
  dragResId.value = null
  dragResIndex.value = null
}

// ---- 右键菜单 ----
const menu = ref({ visible: false, x: 0, y: 0, items: [] as ContextMenuItem[] })

function openMenu(e: MouseEvent, items: ContextMenuItem[]) {
  menu.value = { visible: true, x: e.clientX, y: e.clientY, items }
}

function onResourceContext(e: MouseEvent, r: Resource) {
  e.preventDefault()
  openMenu(e, [
    {
      label: '启动',
      onClick: () => store.launchResource(r.id),
    },
    {
      label: '编辑',
      onClick: () => {
        editing.value = r
        resourceDialogVisible.value = true
      },
    },
    {
      label: '删除',
      danger: true,
      onClick: () => store.removeResource(r.id),
    },
  ])
}

function onGroupContext(e: MouseEvent, gid: number) {
  e.preventDefault()
  const group = store.state.groups.find((g) => g.id === gid)
  if (!group) return
  openMenu(e, [
    {
      label: '重命名',
      onClick: () => {
        groupDialog.value = { visible: true, title: '重命名分组', group }
      },
    },
    {
      label: '删除分组',
      danger: true,
      onClick: () => {
        store.removeGroup(gid)
        if (activeGroupId.value === gid) activeGroupId.value = 'all'
      },
    },
  ])
}

async function onLaunch(r: Resource) {
  try {
    await store.launchResource(r.id)
  } catch (e) {
    showToast(`无法启动「${r.name}」：${String(e)}`)
  }
}

// ---- 弹窗 ----
const resourceDialogVisible = ref(false)
const editing = ref<Resource | null>(null)
const groupDialog = ref<{
  visible: boolean
  title: string
  group: { id: number; name: string } | null
}>({ visible: false, title: '新建分组', group: null })

function openAddResource() {
  editing.value = null
  resourceDialogVisible.value = true
}

function onEditResource(r: Resource) {
  editing.value = r
  resourceDialogVisible.value = true
}

function onResourceSubmit(payload: {
  id?: number
  groupId: number
  kind: 'app' | 'web'
  name: string
  target: string
  icon?: string | null
  args?: string | null
}) {
  if (payload.id != null) {
    store.editResource({
      id: payload.id,
      groupId: payload.groupId,
      kind: payload.kind,
      name: payload.name,
      target: payload.target,
      icon: payload.icon ?? null,
      args: payload.args ?? null,
    })
  } else {
    store.addResource({
      groupId: payload.groupId,
      kind: payload.kind,
      name: payload.name,
      target: payload.target,
      icon: payload.icon ?? null,
      args: payload.args ?? null,
    })
  }
}

function onGroupSubmit(name: string) {
  const g = groupDialog.value.group
  if (g) {
    store.renameGroup(g.id, name)
  } else {
    store.addGroup(name).then((created) => {
      if (activeGroupId.value === 'all') activeGroupId.value = created.id
    })
  }
}

// ---- 图标配色（按名称 hash 取强调色） ----
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

function iconText(r: Resource): string {
  return r.name.charAt(0).toUpperCase()
}

// ---- 图标渲染：文件路径（提取的 PNG）用 asset 协议显示，否则 emoji/首字母 ----
const IMAGE_ICON_RE = /\.(png|jpg|jpeg|ico|gif|webp)$/i

function isImageIcon(icon: string | null): boolean {
  return !!icon && IMAGE_ICON_RE.test(icon)
}

function iconSrc(icon: string): string {
  return isTauri() ? convertFileSrc(icon) : ''
}

// 图片加载失败的图标回退到首字母（避免破图）
const failedIcons = ref(new Set<number>())

function onIconError(r: Resource) {
  failedIcons.value.add(r.id)
}

function showImageIcon(r: Resource): boolean {
  return isImageIcon(r.icon) && !failedIcons.value.has(r.id)
}
</script>

<template>
  <section ref="rootRef" class="card quicklaunch">
    <header class="ql-header">
      <h2 class="ql-title">快捷启动</h2>
      <div class="ql-actions">
        <button
          class="icon-btn"
          title="新建分组"
          @click="groupDialog = { visible: true, title: '新建分组', group: null }"
        >
          <FolderPlus :size="15" :stroke-width="1.8" />
        </button>
        <button class="icon-btn add" title="添加资源" @click="openAddResource">
          <Plus :size="15" :stroke-width="2.2" />
        </button>
      </div>
    </header>

    <!-- 分组 tabs（可拖拽排序，占位模式） -->
    <nav class="filter-tabs group-tabs" aria-label="资源分组">
      <button
        class="filter-tab filter-tab--primary"
        :class="{ active: activeGroupId === 'all' }"
        @click="activeGroupId = 'all'"
      >
        全部
      </button>
      <template v-for="(g, i) in displayGroups" :key="g ? g.id : 'group-ph-' + i">
        <span
          v-if="g === null"
          class="group-placeholder"
          aria-hidden="true"
        ></span>
        <button
          v-else
          class="filter-tab filter-tab--primary group-tab"
          :class="{
            active: activeGroupId === g.id,
            dragging: dragGroupId === g.id,
          }"
          draggable="true"
          @click="activeGroupId = g.id"
          @contextmenu="onGroupContext($event, g.id)"
          @dragstart="onGroupDragStart(g.id)"
          @dragover.prevent="onGroupDragOver($event, g.id)"
          @dragend="onGroupDragEnd"
        >
          {{ g.name }}
        </button>
      </template>
    </nav>

    <!-- 资源网格（可拖拽排序，占位模式） -->
    <div class="ql-body">
      <div v-if="visibleResources.length > 0" class="resource-grid">
        <template v-for="(r, i) in displayResources" :key="r ? r.id : 'res-ph-' + i">
          <div
            v-if="r === null"
            class="res-card placeholder"
            aria-hidden="true"
          ></div>
          <div
            v-else
            class="res-card"
            :class="{ dragging: dragResId === r.id }"
            :title="r.target"
            role="button"
            tabindex="0"
            draggable="true"
            @click="onLaunch(r)"
            @keydown.enter="onLaunch(r)"
            @keydown.space.prevent="onLaunch(r)"
            @contextmenu="onResourceContext($event, r)"
            @dragstart="onResDragStart(r.id)"
            @dragover.prevent="onResDragOver($event, r.id)"
            @dragend="onResDragEnd"
          >
          <span class="res-kind" :class="r.kind">{{ r.kind === 'app' ? '应用' : '网页' }}</span>
          <div class="res-actions">
            <button
              class="res-action"
              title="编辑"
              @click.stop="onEditResource(r)"
            >
              <Pencil :size="11" :stroke-width="2" />
            </button>
            <button
              class="res-action del"
              title="删除"
              @click.stop="store.removeResource(r.id)"
            >
              <Trash2 :size="11" :stroke-width="2" />
            </button>
          </div>
          <div
            class="res-icon"
            :style="
              showImageIcon(r)
                ? {}
                : { background: accentOf(r.name).soft }
            "
          >
            <img
              v-if="showImageIcon(r)"
              class="res-img"
              :src="iconSrc(r.icon!)"
              alt=""
              @error="onIconError(r)"
            />
            <span
              v-else-if="!r.icon"
              class="res-letter"
              :style="{ color: accentOf(r.name).text }"
            >
              {{ iconText(r) }}
            </span>
             <span
               v-else
               class="res-letter"
               :style="{ color: accentOf(r.name).text }"
             >
               {{ iconText(r) }}
             </span>
          </div>
          <span class="res-name">{{ r.name }}</span>
          </div>
        </template>
      </div>

      <div v-else class="empty-state">
        <Wrench :size="24" :stroke-width="1.7" aria-hidden="true" />
        <p>还没有快捷资源</p>
        <p style="font-size: 12px; color: var(--text-4)">
          添加本地程序或网页书签，一键启动
        </p>
        <button class="pill-btn" style="padding: 7px 18px; margin-top: 6px" @click="openAddResource">
          添加资源
        </button>
      </div>
    </div>

    <!-- 右键菜单 -->
    <ContextMenu
      :visible="menu.visible"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu.visible = false"
    />

    <!-- 弹窗 -->
    <ResourceFormDialog
      :visible="resourceDialogVisible"
      :groups="store.state.groups"
      :editing="editing"
      :default-group-id="
        activeGroupId === 'all' ? null : (activeGroupId as number)
      "
      :prefill="prefill"
      @close="resourceDialogVisible = false"
      @submit="onResourceSubmit"
    />
    <GroupFormDialog
      :visible="groupDialog.visible"
      :title="groupDialog.title"
      :initial-value="groupDialog.group?.name"
      @close="groupDialog.visible = false"
      @submit="onGroupSubmit"
    />

    <!-- 拖拽导入遮罩 -->
    <Teleport to="body">
      <Transition name="drop">
        <div v-if="dropping" class="drop-overlay">
          <div class="drop-hint">
            <FilePlus :size="34" :stroke-width="1.5" />
            <p>释放以添加本地应用</p>
            <span>支持 .exe 文件或 .lnk 快捷方式</span>
          </div>
        </div>
      </Transition>
    </Teleport>
  </section>
</template>

<style scoped>
.quicklaunch {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  margin-top: 4px;
  padding: 8px 0 0;
  background: transparent;
  border: 0;
  border-radius: 0;
  box-shadow: none;
}
.ql-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}
.ql-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.ql-actions {
  display: flex;
  gap: 2px;
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

/* 分组 tabs */
.group-tabs {
  margin-bottom: 8px;
  flex-wrap: nowrap;
  overflow-x: auto;
  scrollbar-width: none;
}
.group-tabs::-webkit-scrollbar { display: none; }
.group-tab.dragging {
  opacity: 0.4;
}
.group-tab {
  cursor: grab;
}
.group-tab:active {
  cursor: grabbing;
}
.group-placeholder {
  flex-shrink: 0;
  width: 48px;
  height: 26px;
  border: 2px dashed var(--brand-500);
  border-radius: var(--radius-pill);
  opacity: 0.6;
}

/* 资源网格 */
.ql-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.resource-grid {
  display: grid;
  grid-template-columns: 1fr;
  gap: 4px;
}
.res-card {
  position: relative;
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  min-height: 36px;
  padding: 6px 8px;
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s, border-color 0.18s;
}
.res-card:hover {
  background: var(--bg-card-solid);
  transform: translateX(2px);
  box-shadow: none;
  border-color: color-mix(in srgb, var(--brand-500) 32%, var(--border-soft));
}
.res-card.dragging {
  opacity: 0.4;
  transform: scale(0.96);
}
.res-card.placeholder {
  border: 2px dashed var(--brand-500);
  background: var(--brand-50);
  opacity: 0.6;
  min-height: 36px;
  cursor: default;
}
.res-actions {
  position: absolute;
  top: 5px;
  right: 5px;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.15s;
}
.res-card:hover .res-actions {
  opacity: 1;
}
.res-action {
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
.res-action:hover {
  color: var(--brand-500);
  background: var(--brand-50);
}
.res-action.del:hover {
  color: var(--c-red);
  background: color-mix(in srgb, var(--c-red) 10%, transparent);
}
.res-icon {
  position: relative;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
}
.res-letter {
  font-size: 18px;
  font-weight: 700;
}
.res-img {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  object-fit: contain;
  background: var(--bg-card);
}
.res-kind {
  position: static;
  order: 3;
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  min-height: 18px;
  padding: 2px 7px;
  border-radius: var(--radius-pill);
  font-size: 10px;
  font-weight: 600;
  line-height: 1;
}
.res-kind.app {
  background: var(--c-blue-soft);
  color: var(--c-blue-ink);
}
.res-kind.web {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.res-name {
  min-width: 0;
  flex: 1;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-2);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.quicklaunch :deep(.empty-state) { padding: 16px 8px; }
.quicklaunch :deep(.empty-state > span) { display: none; }

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
