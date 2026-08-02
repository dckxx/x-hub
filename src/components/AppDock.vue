<script setup lang="ts">
import { computed } from 'vue'
import { useStore } from '../stores/workbench'
import type { Resource } from '../api/tauri'

const store = useStore()

const appResources = computed(() =>
  store.state.resources.filter((r) => r.kind === 'app').slice(0, 8),
)

const ACCENTS = [
  { soft: 'var(--c-yellow-soft)', text: '#8A6D00' },
  { soft: 'var(--c-red-soft)', text: '#B91C1C' },
  { soft: 'var(--c-blue-soft)', text: '#1D4ED8' },
  { soft: 'var(--c-green-soft)', text: '#15803D' },
  { soft: 'var(--c-pink-soft)', text: '#BE185D' },
  { soft: 'var(--c-orange-soft)', text: '#B45309' },
  { soft: 'var(--c-purple-soft)', text: '#6D28D9' },
  { soft: 'var(--c-gray-soft)', text: '#4B5563' },
]

function accentOf(name: string) {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return ACCENTS[h % ACCENTS.length]
}

function display(r: Resource): string {
  return r.icon ?? r.name.charAt(0).toUpperCase()
}
</script>

<template>
  <Transition name="dock">
    <div v-if="appResources.length > 0" class="app-dock" role="toolbar">
      <button
        v-for="r in appResources"
        :key="r.id"
        class="dock-item"
        :title="r.name"
        @click="store.launchResource(r.id)"
      >
        <span
          class="dock-icon"
          :style="{ background: accentOf(r.name).soft, color: accentOf(r.name).text }"
        >
          {{ display(r) }}
        </span>
      </button>
    </div>
  </Transition>
</template>

<style scoped>
.app-dock {
  position: fixed;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 50;
  display: flex;
  gap: 8px;
  padding: 8px 12px;
  background: var(--bg-card);
  border-radius: var(--radius-pill);
  box-shadow: var(--shadow-dock);
  border: 1px solid var(--border-soft);
}
.dock-item {
  border: none;
  background: transparent;
  padding: 0;
  cursor: pointer;
  transition: transform 0.18s;
}
.dock-item:hover {
  transform: scale(1.12) translateY(-2px);
}
.dock-icon {
  width: 38px;
  height: 38px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
  font-weight: 700;
  box-shadow: var(--shadow-card);
}

.dock-enter-active {
  transition: opacity 0.25s ease-out, transform 0.25s ease-out;
}
.dock-enter-from {
  opacity: 0;
  transform: translateX(-50%) translateY(10px);
}
</style>
