<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ExternalLink, Package, Shield, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import { isTauri, tauriApi, type MarketExtension } from '../api/tauri'
import { accentOf } from '../composables/useResourceIcon'

const props = defineProps<{
  extension: MarketExtension | null
  /** 底部主按钮文案（安装 / 更新 / 已安装） */
  actionLabel: string
  /** 底部主按钮是否禁用 */
  actionDisabled?: boolean
  /** 底部主按钮标题提示（如宿主要求过高） */
  actionTitle?: string
}>()

const emit = defineEmits<{
  action: []
  close: []
}>()

const visible = computed(() => !!props.extension)
const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(visible, cardRef)

/** 图标是否可显示（https URL 加载失败则回退首字母） */
const iconFailed = ref(false)
const m = computed(() => props.extension)

function initial(): string {
  const e = m.value
  return (e?.name || e?.id || '?').charAt(0).toUpperCase()
}

function formatSize(bytes: number): string {
  if (!bytes) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function openHomepage() {
  const url = m.value?.homepage
  if (url && isTauri()) void tauriApi.openExternal(url)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && visible.value) emit('close')
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div ref="cardRef" class="modal-card md-card" role="dialog" aria-label="扩展详情" aria-modal="true">
          <div class="md-head">
            <div class="md-title">
              <span class="md-icon" :style="{ background: accentOf(m!.name).soft }">
                <img
                  v-if="m!.icon && !iconFailed"
                  :src="m!.icon"
                  :alt="m!.name"
                  draggable="false"
                  @error="iconFailed = true"
                />
                <span v-else :style="{ color: accentOf(m!.name).text }">{{ initial() }}</span>
              </span>
              <div class="md-title-text">
                <h2 class="dialog-title">{{ m!.name }}</h2>
                <span class="md-version">v{{ m!.version }}</span>
              </div>
            </div>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" />
            </button>
          </div>

          <div class="md-body">
            <p class="md-desc">{{ m!.description || '暂无描述' }}</p>

            <div class="md-meta">
              <div class="md-meta-item">
                <span class="md-meta-label">类型</span>
                <span class="md-meta-value">{{ m!.runtime === 'service' ? '服务' : 'Web' }}</span>
              </div>
              <div class="md-meta-item">
                <span class="md-meta-label">大小</span>
                <span class="md-meta-value">{{ formatSize(m!.size) }}</span>
              </div>
              <div v-if="m!.minAppVersion" class="md-meta-item">
                <span class="md-meta-label">要求宿主</span>
                <span class="md-meta-value">v{{ m!.minAppVersion }}+</span>
              </div>
              <div v-if="m!.author" class="md-meta-item">
                <span class="md-meta-label">作者</span>
                <span class="md-meta-value">{{ m!.author }}</span>
              </div>
            </div>

            <button
              v-if="m!.homepage"
              class="md-homepage"
              type="button"
              :disabled="!isTauri()"
              @click="openHomepage"
            >
              <span class="md-homepage-text">查看主页与文档</span>
              <span class="md-homepage-url" :title="m!.homepage">{{ m!.homepage }}</span>
              <ExternalLink :size="13" :stroke-width="2" aria-hidden="true" />
            </button>

            <template v-if="m!.changelog">
              <div class="md-sep" />
              <div class="md-section">
                <div class="md-section-title">
                  <Package :size="13" :stroke-width="2" aria-hidden="true" />更新日志
                </div>
                <p class="md-section-text">{{ m!.changelog }}</p>
              </div>
            </template>

            <div v-if="m!.sha256" class="md-hash">
              <span class="md-hash-head">
                <Shield :size="13" :stroke-width="2" aria-hidden="true" />完整性校验 sha256
              </span>
              <code class="md-hash-code" :title="m!.sha256">{{ m!.sha256.slice(0, 32) }}…</code>
            </div>
          </div>

          <div class="md-foot">
            <button class="ghost-btn" type="button" @click="emit('close')">关闭</button>
            <button
              class="pill-btn"
              type="button"
              :disabled="actionDisabled"
              :title="actionTitle || ''"
              @click="emit('action')"
            >
              {{ actionLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.md-card {
  width: min(520px, 92vw);
  max-height: min(640px, 88vh);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.md-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 16px 18px 0;
  flex-shrink: 0;
}
.md-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.md-icon {
  width: 42px;
  height: 42px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  font-size: 1.125rem;
  font-weight: 700;
  overflow: hidden;
}
.md-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.md-title-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.md-title-text .dialog-title {
  margin: 0;
  font-size: 1.0625rem;
  font-weight: 700;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.md-version {
  font-size: 0.75rem;
  color: var(--text-3);
}
.md-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 13px;
  padding: 14px 18px 16px;
}
.md-desc {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--text-2);
  line-height: 1.7;
  white-space: pre-wrap;
}
.md-meta {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
  gap: 8px;
}
.md-meta-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
}
.md-meta-label {
  font-size: 0.6875rem;
  color: var(--text-4, var(--text-3));
}
.md-meta-value {
  font-size: 0.8125rem;
  font-weight: 650;
  color: var(--text-1);
  word-break: break-all;
}
.md-homepage {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--brand-500);
  font-size: 0.75rem;
  text-align: left;
  cursor: pointer;
  transition: background 150ms ease-out, border-color 150ms ease-out;
}
.md-homepage-text {
  font-weight: 600;
  white-space: nowrap;
}
.md-homepage-url {
  color: var(--text-3);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.md-homepage:hover {
  background: var(--brand-50);
  border-color: var(--brand-500);
}
.md-homepage:disabled {
  opacity: 0.6;
  cursor: default;
}
.md-sep {
  height: 1px;
  background: var(--border-soft);
  margin: 2px 0;
}
.md-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.md-section-title {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 0.75rem;
  font-weight: 650;
  color: var(--text-2);
}
.md-section-text {
  margin: 0;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  font-size: 0.75rem;
  color: var(--text-3);
  line-height: 1.7;
  white-space: pre-wrap;
}
.md-hash {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
}
.md-hash-head {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 0.6875rem;
  color: var(--text-4, var(--text-3));
}
.md-hash-code {
  font-size: 0.6875rem;
  font-family: var(--font-mono, ui-monospace, monospace);
  color: var(--text-3);
  word-break: break-all;
}
.md-foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 18px;
  border-top: 1px solid var(--border-soft);
  flex-shrink: 0;
}
</style>
