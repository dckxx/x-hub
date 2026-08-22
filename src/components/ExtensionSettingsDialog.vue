<script setup lang="ts">
import { computed, inject, ref, watch } from 'vue'
import { Trash2, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import { accentOf, iconSrc } from '../composables/useResourceIcon'
import { tauriApi, type ExtensionEntry } from '../api/tauri'

const props = defineProps<{ extension: ExtensionEntry | null }>()
const emit = defineEmits<{
  close: []
  uninstalled: []
}>()

const cardRef = ref<HTMLElement | null>(null)
const visible = computed(() => props.extension !== null)
useFocusTrap(visible, cardRef)

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const ext = computed(() => props.extension)
const accent = computed(() => (ext.value ? accentOf(ext.value.name) : null))
const initial = computed(() => ((ext.value?.name ?? '?').charAt(0) || '?').toUpperCase())

const confirming = ref(false)
const uninstalling = ref(false)

watch(
  () => props.extension,
  () => {
    confirming.value = false
    uninstalling.value = false
  },
)

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

function runtimeLabel(runtime: 'web' | 'service'): string {
  return runtime === 'service' ? 'service（含本地后端进程）' : 'web（纯前端）'
}

async function confirmUninstall() {
  if (!ext.value) return
  uninstalling.value = true
  try {
    await tauriApi.uninstallExtension(ext.value.id)
    showToast(`已卸载「${ext.value.name}」`)
    emit('uninstalled')
    emit('close')
  } catch (e) {
    showToast(`卸载失败：${String(e)}`)
  } finally {
    uninstalling.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="ext" class="modal-mask">
        <div
          ref="cardRef"
          class="modal-card es-card"
          role="dialog"
          aria-label="扩展设置"
          aria-modal="true"
        >
          <header class="es-head">
            <div class="es-head-left">
              <div class="es-icon" :style="{ background: accent?.soft }">
                <img
                  v-if="ext.icon"
                  :src="iconSrc(ext.icon)"
                  :alt="ext.name"
                  draggable="false"
                />
                <span v-else :style="{ color: accent?.text }">{{ initial }}</span>
              </div>
              <div class="es-head-meta">
                <h2 class="dialog-title">{{ ext.name }}</h2>
                <p class="es-sub">v{{ ext.version }} · {{ ext.id }}</p>
              </div>
            </div>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" aria-hidden="true" />
            </button>
          </header>

          <div class="es-body">
            <section class="es-section">
              <h3 class="es-section-title">信息</h3>
              <dl class="es-kv">
                <div class="es-kv-row"><dt>运行时</dt><dd>{{ runtimeLabel(ext.runtime) }}</dd></div>
                <div class="es-kv-row"><dt>主形态</dt><dd>{{ kindLabel(ext.kind) }}</dd></div>
                <div v-if="ext.surfaces.length" class="es-kv-row">
                  <dt>支持形态</dt><dd>{{ ext.surfaces.map(kindLabel).join(' / ') }}</dd>
                </div>
              </dl>
              <p v-if="ext.description" class="es-desc">{{ ext.description }}</p>
            </section>

            <section class="es-section">
              <h3 class="es-section-title">权限</h3>
              <div v-if="ext.permissions.length" class="es-perms">
                <span v-for="p in ext.permissions" :key="p" class="es-perm">{{ p }}</span>
              </div>
              <p v-else class="es-empty">无权限申请</p>
            </section>
          </div>

          <footer class="es-foot">
            <template v-if="!confirming">
              <button class="es-danger-btn" type="button" @click="confirming = true">
                <Trash2 :size="14" :stroke-width="2" aria-hidden="true" />
                卸载扩展
              </button>
            </template>
            <template v-else>
              <p class="es-confirm-tip">
                {{ ext.runtime === 'service' ? '将卸载扩展并停止其本地后端进程。' : '将卸载扩展。' }}
                此操作不可撤销。
              </p>
              <div class="es-confirm-actions">
                <button
                  class="ghost-btn"
                  type="button"
                  :disabled="uninstalling"
                  @click="confirming = false"
                >
                  取消
                </button>
                <button
                  class="es-danger-btn"
                  type="button"
                  :disabled="uninstalling"
                  @click="confirmUninstall"
                >
                  {{ uninstalling ? '卸载中…' : '确认卸载' }}
                </button>
              </div>
            </template>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.es-card {
  width: 480px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.es-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.es-head-left {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}
.es-icon {
  width: 44px;
  height: 44px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  font-size: 1.125rem;
  font-weight: 700;
  overflow: hidden;
}
.es-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.es-head-meta {
  min-width: 0;
}
.es-head-meta .dialog-title {
  margin: 0;
}
.es-sub {
  margin: 2px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.es-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-height: 40vh;
  overflow-y: auto;
}
.es-section-title {
  margin: 0 0 8px;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.es-kv {
  margin: 0;
}
.es-kv-row {
  display: flex;
  gap: 12px;
  padding: 4px 0;
  font-size: 0.8125rem;
}
.es-kv-row dt {
  flex-shrink: 0;
  width: 72px;
  color: var(--text-3);
}
.es-kv-row dd {
  margin: 0;
  color: var(--text-1);
}
.es-desc {
  margin: 8px 0 0;
  font-size: 0.8125rem;
  color: var(--text-2);
  line-height: 1.5;
}
.es-perms {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.es-perm {
  padding: 2px 9px;
  border-radius: var(--radius-pill);
  background: var(--brand-50);
  color: var(--brand-500);
  font-size: 0.75rem;
  font-weight: 600;
}
.es-empty {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--text-3);
}
.es-foot {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-top: 4px;
}
.es-confirm-tip {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--c-red);
}
.es-confirm-actions {
  display: flex;
  gap: 8px;
}
.es-danger-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 7px 16px;
  border: 1px solid color-mix(in srgb, var(--c-red) 45%, transparent);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--c-red);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.18s, color 0.18s, transform 0.18s;
}
.es-danger-btn:hover:not(:disabled) {
  background: var(--c-red);
  color: #fff;
  transform: translateY(-1px);
}
.es-danger-btn:active:not(:disabled) {
  transform: scale(0.96);
}
.es-danger-btn:disabled {
  opacity: 0.55;
  cursor: default;
}
</style>
