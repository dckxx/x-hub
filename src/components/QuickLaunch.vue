<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import { useStore } from '../stores/workbench'
import type { Resource } from '../api/tauri'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import GroupFormDialog from './GroupFormDialog.vue'
import ResourceFormDialog from './ResourceFormDialog.vue'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 分组选中 ----
const activeGroupId = ref<number | 'all'>('all')

const visibleResources = computed(() => {
  if (activeGroupId.value === 'all') return store.state.resources
  return store.state.resources.filter((r) => r.group_id === activeGroupId.value)
})

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
  } catch {
    showToast(`无法启动「${r.name}」，请检查路径`)
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
  { strong: 'var(--c-yellow)', soft: 'var(--c-yellow-soft)', text: '#8A6D00' },
  { strong: 'var(--c-red)', soft: 'var(--c-red-soft)', text: '#B91C1C' },
  { strong: 'var(--c-blue)', soft: 'var(--c-blue-soft)', text: '#1D4ED8' },
  { strong: 'var(--c-green)', soft: 'var(--c-green-soft)', text: '#15803D' },
  { strong: 'var(--c-pink)', soft: 'var(--c-pink-soft)', text: '#BE185D' },
  { strong: 'var(--c-orange)', soft: 'var(--c-orange-soft)', text: '#B45309' },
  { strong: 'var(--c-purple)', soft: 'var(--c-purple-soft)', text: '#6D28D9' },
  { strong: 'var(--c-gray)', soft: 'var(--c-gray-soft)', text: '#4B5563' },
]

function accentOf(name: string) {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return ACCENTS[h % ACCENTS.length]
}

function iconText(r: Resource): string {
  if (r.icon) return r.icon
  return r.name.charAt(0).toUpperCase()
}
</script>

<template>
  <section class="card quicklaunch">
    <header class="ql-header">
      <h2 class="ql-title">快捷启动</h2>
      <div class="ql-actions">
        <button
          class="icon-btn"
          title="新建分组"
          @click="groupDialog = { visible: true, title: '新建分组', group: null }"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
            <path d="M4 7h11l2 2h3v9H4z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" />
            <path d="M4 7V5h7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </button>
        <button class="icon-btn add" title="添加资源" @click="openAddResource">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none">
            <path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
          </svg>
        </button>
      </div>
    </header>

    <!-- 分组 tabs -->
    <nav class="group-tabs" aria-label="资源分组">
      <button
        class="group-tab"
        :class="{ active: activeGroupId === 'all' }"
        @click="activeGroupId = 'all'"
      >
        全部
      </button>
      <button
        v-for="g in store.state.groups"
        :key="g.id"
        class="group-tab"
        :class="{ active: activeGroupId === g.id }"
        @click="activeGroupId = g.id"
        @contextmenu="onGroupContext($event, g.id)"
      >
        {{ g.name }}
      </button>
    </nav>

    <!-- 资源网格 -->
    <div class="ql-body">
      <div v-if="visibleResources.length > 0" class="resource-grid">
        <div
          v-for="r in visibleResources"
          :key="r.id"
          class="res-card"
          :title="r.target"
          @click="onLaunch(r)"
          @contextmenu="onResourceContext($event, r)"
        >
          <div
            class="res-icon"
            :style="{ background: accentOf(r.name).soft }"
          >
            <span
              v-if="!r.icon"
              class="res-letter"
              :style="{ color: accentOf(r.name).text }"
            >
              {{ iconText(r) }}
            </span>
            <span v-else class="res-emoji">{{ r.icon }}</span>
            <span class="res-kind" :class="r.kind">·</span>
          </div>
          <span class="res-name">{{ r.name }}</span>
        </div>
      </div>

      <div v-else class="empty-state">
        <span style="font-size: 28px">🧰</span>
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
  </section>
</template>

<style scoped>
.quicklaunch {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
  min-height: 0;
}
.ql-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.ql-title {
  font-size: 16px;
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
  color: #fff;
}

/* 分组 tabs */
.group-tabs {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 2px;
  margin-bottom: 14px;
  scrollbar-width: none;
}
.group-tabs::-webkit-scrollbar {
  display: none;
}
.group-tab {
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
.group-tab:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.group-tab.active {
  background: #1a1a1f;
  color: #fff;
}
[data-theme="dark"] .group-tab.active {
  background: #f2f2f7;
  color: #1a1a1f;
}

/* 资源网格 */
.ql-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
.resource-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 10px;
}
.res-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 14px 8px 12px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform 0.18s, box-shadow 0.18s;
}
.res-card:hover {
  transform: translateY(-2px);
  box-shadow: var(--shadow-hover);
}
.res-icon {
  position: relative;
  width: 46px;
  height: 46px;
  border-radius: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
}
.res-letter {
  font-size: 18px;
  font-weight: 700;
}
.res-kind {
  position: absolute;
  right: -2px;
  bottom: -2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--bg-card);
  box-shadow: var(--shadow-card);
  font-size: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.res-kind.app {
  background: var(--c-blue);
}
.res-kind.web {
  background: var(--c-green);
}
.res-kind::before {
  content: '';
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #fff;
}
.res-name {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-2);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
