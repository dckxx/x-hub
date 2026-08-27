<script setup lang="ts">
import { computed, inject, ref, watch } from 'vue'
import { Trash2, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import { accentOf, iconSrc } from '../composables/useResourceIcon'
import { tauriApi, type ExtensionEntry } from '../api/tauri'
import { useStore } from '../stores/workbench'

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
const perms = ref<Record<string, boolean>>({})
const permsLoading = ref(false)

watch(
  () => props.extension,
  async (e) => {
    confirming.value = false
    uninstalling.value = false
    perms.value = {}
    if (!e || e.invalid) return
    permsLoading.value = true
    try {
      perms.value = await tauriApi.getExtensionPermissions(e.id)
    } catch {
      // 查询失败时按 manifest 声明默认授权
      perms.value = Object.fromEntries(e.permissions.map((p) => [p, true]))
    } finally {
      permsLoading.value = false
    }
  },
)

async function togglePermission(perm: string, granted: boolean) {
  if (!ext.value) return
  perms.value = { ...perms.value, [perm]: granted }
  try {
    await tauriApi.setExtensionPermission(ext.value.id, perm, granted)
  } catch (err) {
    showToast(`权限设置失败：${String(err)}`)
    perms.value = { ...perms.value, [perm]: !granted }
  }
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

function runtimeLabel(runtime: 'web' | 'service'): string {
  return runtime === 'service' ? 'service（含本地后端进程）' : 'web（纯前端）'
}

const store = useStore()

// 固定到侧栏：点击左栏菜单即在主区打开该扩展（view 形态）
const pinnedSidebar = computed(() =>
  ext.value ? (store.state.config.sidebar_extensions ?? []).includes(ext.value.id) : false,
)

function togglePinned() {
  if (!ext.value) return
  store.setSidebarExtension(ext.value.id, !pinnedSidebar.value)
}

// 默认打开方式：视图 / 窗口 / 抽屉（侧栏点击等入口按此打开）
const openMode = computed(() => store.state.config.extension_open_modes?.[ext.value?.id ?? ''] ?? 'view')

const OPEN_MODES = [
  { value: 'view', label: '视图' },
  { value: 'window', label: '窗口' },
  { value: 'drawer', label: '抽屉' },
] as const

function setOpenMode(mode: (typeof OPEN_MODES)[number]['value']) {
  if (!ext.value) return
  store.setExtensionOpenMode(ext.value.id, mode)
  const label = OPEN_MODES.find((m) => m.value === mode)?.label ?? mode
  showToast(`已设为默认在「${label}」打开`)
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
                <p class="es-sub">v{{ ext.version }} · {{ kindLabel(ext.kind) }} · {{ ext.id }}</p>
              </div>
            </div>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" aria-hidden="true" />
            </button>
          </header>

          <div class="es-body">
            <section class="es-block">
              <h3 class="es-block-title">信息</h3>
              <dl class="es-kv">
                <div class="es-kv-row"><dt>运行时</dt><dd>{{ runtimeLabel(ext.runtime) }}</dd></div>
                <div class="es-kv-row"><dt>主形态</dt><dd>{{ kindLabel(ext.kind) }}</dd></div>
                <div v-if="ext.surfaces.length" class="es-kv-row">
                  <dt>支持形态</dt><dd>{{ ext.surfaces.map(kindLabel).join(' / ') }}</dd>
                </div>
              </dl>
              <p v-if="ext.description" class="es-desc">{{ ext.description }}</p>
            </section>

            <section class="es-block">
              <h3 class="es-block-title">权限</h3>
              <div v-if="ext.permissions.length" class="es-perm-list">
                <div v-for="p in ext.permissions" :key="p" class="es-perm-row">
                  <span class="es-perm-name">{{ p }}</span>
                  <button
                    class="toggle"
                    role="switch"
                    type="button"
                    :aria-checked="perms[p] ?? true"
                    :class="{ on: perms[p] ?? true }"
                    :disabled="permsLoading"
                    @click="togglePermission(p, !(perms[p] ?? true))"
                  >
                    <span class="toggle-knob"></span>
                  </button>
                </div>
              </div>
              <p v-else class="es-empty">无权限申请</p>
            </section>

            <section v-if="!ext.invalid" class="es-block">
              <h3 class="es-block-title">侧边栏</h3>
              <div class="es-kv-row es-openmode-row">
                <dt>打开方式</dt>
                <dd>
                  <div class="es-seg" role="group" aria-label="默认打开方式">
                    <button
                      v-for="m in OPEN_MODES"
                      :key="m.value"
                      class="es-seg-btn"
                      :class="{ active: openMode === m.value }"
                      type="button"
                      @click="setOpenMode(m.value)"
                    >
                      {{ m.label }}
                    </button>
                  </div>
                </dd>
              </div>
              <div class="es-perm-row">
                <span class="es-setting-name">在左侧栏固定此扩展</span>
                <button
                  class="toggle"
                  role="switch"
                  type="button"
                  :aria-checked="pinnedSidebar"
                  :class="{ on: pinnedSidebar }"
                  @click="togglePinned"
                >
                  <span class="toggle-knob"></span>
                </button>
              </div>
              <p class="es-empty es-openmode-hint">固定后，点击左栏菜单即按上方「打开方式」打开该扩展。</p>
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
  gap: 12px;
  max-height: 40vh;
  overflow-y: auto;
}
.es-block {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--bg-card-soft);
}
.es-block-title {
  margin: 0;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.es-kv {
  margin: 0;
  display: flex;
  flex-direction: column;
}
.es-kv-row {
  display: flex;
  gap: 12px;
  padding: 6px 0;
  font-size: 0.8125rem;
}
.es-kv-row + .es-kv-row {
  border-top: 1px solid var(--border-soft);
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
  margin: 2px 0 0;
  padding-top: 10px;
  border-top: 1px solid var(--border-soft);
  font-size: 0.8125rem;
  color: var(--text-2);
  line-height: 1.55;
}
.es-perm-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.es-perm-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 7px 0;
}
.es-perm-row + .es-perm-row {
  border-top: 1px solid var(--border-soft);
}
.es-perm-name {
  font-size: 0.8125rem;
  color: var(--text-1);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.toggle {
  flex-shrink: 0;
  width: 40px;
  height: 22px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--border-strong);
  position: relative;
  cursor: pointer;
  padding: 0;
  transition: background 0.18s;
}
.toggle.on {
  background: var(--brand-500);
}
.toggle:disabled {
  opacity: 0.6;
  cursor: default;
}
.toggle-knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  box-shadow: var(--shadow-dock);
  transition: transform 0.18s;
}
.toggle.on .toggle-knob {
  transform: translateX(18px);
}
.es-empty {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--text-3);
}
.es-openmode-hint {
  font-size: 0.75rem;
  color: var(--text-4, var(--text-3));
}
/* 侧边栏区块：普通字体（避免复用权限行的等宽字），打开方式分段选择 */
.es-setting-name {
  font-size: 0.8125rem;
  color: var(--text-1);
}
.es-openmode-row {
  align-items: center;
}
.es-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
}
.es-seg-btn {
  padding: 3px 12px;
  border: 0;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.es-seg-btn:hover {
  color: var(--text-1);
}
.es-seg-btn.active {
  background: var(--brand-500);
  color: #fff;
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
