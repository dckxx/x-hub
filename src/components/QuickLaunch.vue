<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  NButton,
  NTag,
  NEmpty,
  NDropdown,
  NIcon,
  useMessage,
} from 'naive-ui'
import {
  AddOutline,
  EllipsisVerticalOutline,
  LinkOutline,
  CodeWorkingOutline,
} from '@vicons/ionicons5'
import { useStore } from '../stores/workbench'
import ResourceForm from './ResourceForm.vue'
import GroupForm from './GroupForm.vue'
import type { Group, Resource } from '../api/tauri'

const store = useStore()
const message = useMessage()

const formOpen = ref(false)
const editing = ref<Resource | null>(null)
const formGroupId = ref<number | null>(null)
const groupFormOpen = ref(false)
const editingGroup = ref<Group | null>(null)

const groups = computed(() => store.state.groups)
const resourcesByGroup = (groupId: number) =>
  store.state.resources
    .filter((r) => r.group_id === groupId)
    .sort((a, b) => a.sort_order - b.sort_order)

function openCreate(groupId: number | null) {
  editing.value = null
  formGroupId.value = groupId
  formOpen.value = true
}

function openEdit(resource: Resource) {
  editing.value = resource
  formOpen.value = true
}

async function handleLaunch(resource: Resource) {
  try {
    await store.launchResource(resource.id)
  } catch (e: unknown) {
    message.error(String(e) || '启动失败')
  }
}

async function handleRemove(resource: Resource) {
  await store.removeResource(resource.id)
  message.success('已删除')
}

async function handleGroupDelete(group: Group) {
  await store.removeGroup(group.id)
  message.success('分组已删除')
}

function openGroupEdit(group: Group | null) {
  editingGroup.value = group
  groupFormOpen.value = true
}

const groupMenuOptions = () => [
  { label: '重命名分组', key: 'rename' },
  { label: '删除分组', key: 'delete' },
]

function onGroupMenu(key: string, group: Group) {
  if (key === 'delete') handleGroupDelete(group)
  if (key === 'rename') openGroupEdit(group)
}
</script>

<template>
  <div class="quick-launch">
    <div class="quick-launch__header">
      <h2 class="quick-launch__title">快捷启动</h2>
      <NButton
        type="primary"
        size="small"
        @click="groupFormOpen = true; editingGroup = null"
      >
        <template #icon><NIcon :component="AddOutline" /></template>
        新建分组
      </NButton>
    </div>

    <div v-if="groups.length" class="quick-launch__groups">
      <div v-for="group in groups" :key="group.id" class="resource-group">
        <div class="resource-group__header">
          <NTag size="small" type="info" round>{{ group.name }}</NTag>
          <NDropdown
            :options="groupMenuOptions()"
            trigger="click"
            @select="(k) => onGroupMenu(k, group)"
          >
            <NButton quaternary circle size="tiny" class="resource-group__more">
              <NIcon :component="EllipsisVerticalOutline" />
            </NButton>
          </NDropdown>
        </div>
        <div v-if="resourcesByGroup(group.id).length" class="resource-group__grid">
          <button
            v-for="resource in resourcesByGroup(group.id)"
            :key="resource.id"
            class="resource-card"
            @click="handleLaunch(resource)"
            @contextmenu.prevent="openEdit(resource)"
          >
            <span class="resource-card__icon">
              <NIcon
                :component="resource.kind === 'web' ? LinkOutline : CodeWorkingOutline"
                size="22"
              />
            </span>
            <span class="resource-card__name">{{ resource.name }}</span>
            <span class="resource-card__actions" @click.stop>
              <NDropdown
                :options="[
                  { label: '编辑', key: 'edit' },
                  { label: '删除', key: 'delete' },
                ]"
                trigger="click"
                @select="(k) => (k === 'edit' ? openEdit(resource) : handleRemove(resource))"
              >
                <NButton quaternary circle size="tiny">
                  <NIcon :component="EllipsisVerticalOutline" />
                </NButton>
              </NDropdown>
            </span>
          </button>
          <button class="resource-card resource-card--add" @click="openCreate(group.id)">
            <NIcon :component="AddOutline" size="20" />
          </button>
        </div>
        <div v-else class="resource-group__empty">
          <NButton dashed size="small" @click="openCreate(group.id)">
            添加快捷资源到「{{ group.name }}」
          </NButton>
        </div>
      </div>
    </div>

    <NEmpty v-else description="暂无分组，请先创建分组">
      <template #extra>
        <NButton type="primary" @click="openGroupEdit(null)">创建分组</NButton>
      </template>
    </NEmpty>

    <ResourceForm
      v-model:open="formOpen"
      :editing="editing"
      :default-group-id="formGroupId"
    />
    <GroupForm v-model:open="groupFormOpen" :editing="editingGroup" />
  </div>
</template>

<style scoped>
.quick-launch {
  padding: 20px 24px;
}
.quick-launch__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
}
.quick-launch__title {
  font-size: 18px;
  font-weight: 600;
}
.quick-launch__groups {
  display: flex;
  flex-direction: column;
  gap: 20px;
}
.resource-group__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.resource-group__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
  gap: 12px;
}
.resource-card {
  position: relative;
  border: 1px solid rgba(127, 127, 127, 0.2);
  border-radius: 10px;
  background: transparent;
  color: inherit;
  padding: 16px 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  transition: border-color 0.15s, background-color 0.15s, transform 0.1s;
}
.resource-card:hover {
  border-color: #2080f0;
  background-color: rgba(32, 128, 240, 0.06);
  transform: translateY(-2px);
}
.resource-card__icon {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: rgba(127, 127, 127, 0.12);
}
.resource-card__name {
  font-size: 12px;
  text-align: center;
  word-break: break-all;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.resource-card__actions {
  position: absolute;
  top: 4px;
  right: 4px;
  display: none;
}
.resource-card:hover .resource-card__actions {
  display: block;
}
.resource-card--add {
  border-style: dashed;
  justify-content: center;
  opacity: 0.5;
}
.resource-card--add:hover {
  opacity: 1;
}
.resource-group__empty {
  padding: 8px 0;
}
</style>
