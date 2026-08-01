<script setup lang="ts">
import { NIcon, NText } from 'naive-ui'
import { RocketOutline, CreateOutline, SettingsOutline } from '@vicons/ionicons5'

defineProps<{
  view: 'quick' | 'notes' | 'settings'
}>()

const emit = defineEmits<{
  (e: 'update:view', view: 'quick' | 'notes' | 'settings'): void
}>()

const items = [
  { key: 'quick' as const, label: '快捷启动', icon: RocketOutline },
  { key: 'notes' as const, label: '速记笔记', icon: CreateOutline },
  { key: 'settings' as const, label: '系统设置', icon: SettingsOutline },
]
</script>

<template>
  <aside class="side-nav">
    <div class="side-nav__list">
      <button
        v-for="item in items"
        :key="item.key"
        class="side-nav__item"
        :class="{ 'side-nav__item--active': view === item.key }"
        @click="emit('update:view', item.key)"
      >
        <NIcon :component="item.icon" size="18" />
        <NText class="side-nav__label">{{ item.label }}</NText>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.side-nav {
  width: 64px;
  border-right: 1px solid rgba(127, 127, 127, 0.15);
  padding: 12px 0;
  display: flex;
  flex-direction: column;
}
.side-nav__list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.side-nav__item {
  width: 100%;
  padding: 12px 0;
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  opacity: 0.65;
  transition: opacity 0.15s, background-color 0.15s;
}
.side-nav__item:hover {
  opacity: 1;
  background-color: rgba(127, 127, 127, 0.1);
}
.side-nav__item--active {
  opacity: 1;
  color: #2080f0;
}
.side-nav__label {
  font-size: 11px;
}
</style>
