<script setup lang="ts">
import { computed, inject, onMounted, ref } from 'vue'
import { FolderOpen, MoreHorizontal, PackageOpen, Plus } from 'lucide-vue-next'
import { isTauri, tauriApi, type ExtensionEntry } from '../api/tauri'
import { accentOf, iconSrc } from '../composables/useResourceIcon'

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const emit = defineEmits<{
  open: [ext: ExtensionEntry]
}>()

function onRowClick(e: ExtensionEntry) {
  if (e.invalid) {
    showToast(`「${e.name}」无法打开：${e.error ?? 'manifest 缺失或损坏'}`)
    return
  }
  emit('open', e)
}

const extensions = ref<ExtensionEntry[]>([])
const loading = ref(true)
const failedIcons = ref(new Set<string>())

const visibleCount = computed(() => extensions.value.filter((e) => !e.invalid).length)

function accentFor(e: ExtensionEntry) {
  return accentOf(e.name)
}

function initial(e: ExtensionEntry): string {
  return (e.name || e.id || '?').charAt(0).toUpperCase()
}

function showImg(e: ExtensionEntry): boolean {
  return !!e.icon && !failedIcons.value.has(e.id)
}

function onImgError(e: ExtensionEntry) {
  failedIcons.value.add(e.id)
}

function kindLabel(kind: string): string {
  switch (kind) {
    case 'module':
      return '卡片'
    case 'view':
      return '视图'
    case 'window':
      return '窗口'
    case 'drawer':
      return '抽屉'
    default:
      return kind || '视图'
  }
}

async function load() {
  loading.value = true
  try {
    extensions.value = isTauri() ? await tauriApi.listExtensions() : []
  } catch (e) {
    showToast(`加载扩展列表失败：${String(e)}`)
  } finally {
    loading.value = false
  }
}

onMounted(load)

function onInstall() {
  showToast('安装扩展功能即将上线')
}

function onLocalInstall() {
  showToast('本地安装功能即将上线')
}

function onMore(e: ExtensionEntry) {
  showToast(`「${e.name}」的扩展设置即将上线`)
}
</script>

<template>
  <div class="extension-center">
    <header class="ec-header">
      <div class="ec-title-wrap">
        <h2 class="ec-title">扩展中心</h2>
        <p class="ec-subtitle">
          {{ visibleCount ? `已安装 ${visibleCount} 个扩展` : '管理已安装的扩展' }}
        </p>
      </div>
      <div class="ec-actions">
        <button class="pill-btn" type="button" @click="onInstall">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          安装扩展
        </button>
        <button class="ghost-btn" type="button" @click="onLocalInstall">
          <FolderOpen :size="14" :stroke-width="2" aria-hidden="true" />
          从本地安装
        </button>
      </div>
    </header>

    <div v-if="loading" class="ec-empty">
      <p>正在扫描扩展…</p>
    </div>

    <div v-else-if="extensions.length === 0" class="ec-empty">
      <PackageOpen :size="40" :stroke-width="1.5" aria-hidden="true" />
      <h3>还没有安装任何扩展</h3>
      <p>安装扩展后，工作台就能扩展出你需要的功能</p>
      <button class="pill-btn" type="button" @click="onInstall">安装第一个扩展</button>
    </div>

    <div v-else class="ec-list">
      <div
        v-for="e in extensions"
        :key="e.id"
        class="ec-row"
        :class="{ invalid: e.invalid, clickable: !e.invalid }"
        role="button"
        :tabindex="e.invalid ? undefined : 0"
        @click="onRowClick(e)"
        @keydown.enter="onRowClick(e)"
      >
        <div class="ec-icon" :style="{ background: accentFor(e).soft }">
          <img
            v-if="showImg(e)"
            :src="iconSrc(e.icon!)"
            :alt="e.name"
            draggable="false"
            @error="onImgError(e)"
          />
          <span v-else :style="{ color: accentFor(e).text }">{{ initial(e) }}</span>
        </div>

        <div class="ec-meta">
          <div class="ec-name-line">
            <span class="ec-name">{{ e.name }}</span>
            <span v-if="e.invalid" class="ec-tag ec-tag-invalid">不可用</span>
            <template v-else>
              <span v-if="e.runtime === 'service'" class="ec-tag ec-tag-service">service</span>
              <span class="ec-tag ec-tag-kind">{{ kindLabel(e.kind) }}</span>
            </template>
          </div>
          <p class="ec-desc" :title="e.invalid ? (e.error ?? '') : (e.description || e.id)">
            {{ e.invalid ? (e.error ?? '此扩展无法加载') : (e.description || e.id) }}
          </p>
        </div>

        <div class="ec-right">
          <span class="ec-version">v{{ e.version || '—' }}</span>
          <button
            class="ec-more"
            type="button"
            :aria-label="`${e.name} 设置`"
            :data-tip="`${e.name} 设置`"
            @click.stop="onMore(e)"
          >
            <MoreHorizontal :size="16" :stroke-width="2" aria-hidden="true" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.extension-center {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-5);
  overflow: hidden;
}
.ec-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.ec-title-wrap {
  flex: 1;
  min-width: 0;
}
.ec-title {
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-1);
}
.ec-subtitle {
  margin: 2px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
.ec-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.ec-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-3);
  text-align: center;
}
.ec-empty svg {
  color: var(--text-3);
  opacity: 0.7;
}
.ec-empty h3 {
  margin: 4px 0 0;
  font-size: 0.9375rem;
  font-weight: 650;
  color: var(--text-2);
}
.ec-empty p {
  margin: 0;
  font-size: 0.8125rem;
}
.ec-empty .pill-btn {
  margin-top: 8px;
}

.ec-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 2px;
}
.ec-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  transition: transform 150ms ease-out, box-shadow 150ms ease-out;
}
.ec-row:hover {
  transform: translateY(-1px);
  box-shadow: var(--shadow-card-hover, var(--shadow-card));
}
.ec-row.clickable {
  cursor: pointer;
}
.ec-row.invalid {
  opacity: 0.72;
}
.ec-icon {
  width: 40px;
  height: 40px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  font-size: 1.0625rem;
  font-weight: 700;
  overflow: hidden;
}
.ec-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.ec-meta {
  flex: 1;
  min-width: 0;
}
.ec-name-line {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.ec-name {
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-tag {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: var(--radius-pill);
  font-size: 0.6875rem;
  font-weight: 600;
  line-height: 1.5;
}
.ec-tag-service {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.ec-tag-kind {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ec-tag-invalid {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.ec-desc {
  margin: 2px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ec-version {
  font-size: 0.75rem;
  color: var(--text-3);
}
.ec-more {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.ec-more:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
</style>
