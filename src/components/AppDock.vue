<script setup lang="ts">
import { computed, ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { isTauri, type Resource } from '../api/tauri'
import { useStore } from '../stores/workbench'

const store = useStore()

// 最近使用：有启动记录的按时间倒序，无记录排最后（兜底仍显示）
const appResources = computed(() =>
  store.state.resources
    .filter((r) => r.kind === 'app')
    .slice()
    .sort((a, b) => {
      const ta = a.last_launched_at ? new Date(a.last_launched_at).getTime() : 0
      const tb = b.last_launched_at ? new Date(b.last_launched_at).getTime() : 0
      return tb - ta
    })
    .slice(0, 8),
)

const ACCENTS = [
  { soft: 'var(--c-yellow-soft)', text: 'var(--c-yellow-ink)' },
  { soft: 'var(--c-red-soft)', text: 'var(--c-red-ink)' },
  { soft: 'var(--c-blue-soft)', text: 'var(--c-blue-ink)' },
  { soft: 'var(--c-green-soft)', text: 'var(--c-green-ink)' },
  { soft: 'var(--c-pink-soft)', text: 'var(--c-pink-ink)' },
  { soft: 'var(--c-orange-soft)', text: 'var(--c-orange-ink)' },
  { soft: 'var(--c-purple-soft)', text: 'var(--c-purple-ink)' },
  { soft: 'var(--c-gray-soft)', text: 'var(--c-gray-ink)' },
]

function accentOf(name: string) {
  let h = 0
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
  return ACCENTS[h % ACCENTS.length]
}

function display(r: Resource): string {
  return r.icon ?? r.name.charAt(0).toUpperCase()
}

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
  <Transition name="dock">
    <div v-if="appResources.length > 0" class="app-dock">
      <div class="dock-pill" role="toolbar">
        <button
          v-for="r in appResources"
          :key="r.id"
          class="dock-item"
          :title="r.name"
          @click="store.launchResource(r.id)"
        >
          <span
            class="dock-icon"
            :style="
              showImageIcon(r)
                ? {}
                : { background: accentOf(r.name).soft, color: accentOf(r.name).text }
            "
          >
            <img
              v-if="showImageIcon(r)"
              class="dock-img"
              :src="iconSrc(r.icon!)"
              alt=""
              @error="onIconError(r)"
            />
            <template v-else>{{ display(r) }}</template>
          </span>
        </button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
/* 布局内底部 Dock：无图标时自动收起不占空间 */
.app-dock {
  display: flex;
  justify-content: center;
  padding: 0 20px 16px;
  flex-shrink: 0;
}
.dock-pill {
  display: flex;
  gap: 10px;
  padding: 10px 24px;
  background: var(--bg-card);
  border-radius: var(--radius-pill);
  box-shadow: var(--shadow-card);
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
.dock-img {
  width: 38px;
  height: 38px;
  border-radius: 50%;
  object-fit: contain;
  background: var(--bg-card);
}

.dock-enter-active {
  transition: opacity 0.25s ease-out, transform 0.25s ease-out;
}
.dock-enter-from {
  opacity: 0;
  transform: translateY(10px);
}
</style>
