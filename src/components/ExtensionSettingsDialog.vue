<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Trash2, X, FolderOpen } from 'lucide-vue-next'
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

// Esc 关闭：弹窗可见时才响应（同屏多个 Esc 监听互不干扰，各自只管自己的弹窗）
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && visible.value) emit('close')
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

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
      showToast('权限状态读取失败，请重新打开权限设置')
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

/** 权限标识 → 用户可读说明（未登记的权限原样展示） */
const PERMISSION_LABELS: Record<string, string> = {
  'data:read': '读取宿主数据（笔记 / 待办 / 速达）',
  'data:write': '写入宿主数据',
  fs: '保存文件（写入系统下载目录）',
  clipboard: '访问剪贴板',
  network: '访问网络',
  'open-url': '用系统默认浏览器打开网页',
  'service:execute': '运行本地后端（信任当前版本）',
  system: '系统级操作（宿主暂无该能力）',
  notify: '发送系统通知',
  events: '广播扩展事件',
  'shared-storage': '读写跨扩展共享存储',
}

function permissionLabel(perm: string): string {
  return PERMISSION_LABELS[perm] ?? perm
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

// 默认打开方式：软件内（主区视图）/ 独立窗口 / 抽屉——
// 扩展中心点开、左栏固定菜单等入口都按此打开（默认「软件内」）
const openMode = computed(() => store.state.config.extension_open_modes?.[ext.value?.id ?? ''] ?? 'view')

const OPEN_MODES = [
  { value: 'view', label: '软件内' },
  { value: 'window', label: '独立窗口' },
  { value: 'drawer', label: '抽屉' },
] as const

function setOpenMode(mode: (typeof OPEN_MODES)[number]['value']) {
  if (!ext.value) return
  store.setExtensionOpenMode(ext.value.id, mode)
  const label = OPEN_MODES.find((m) => m.value === mode)?.label ?? mode
  showToast(`已设为默认在「${label}」打开`)
}

// 链接打开方式：扩展页里的外链（xhub.openExternal）怎么打开——
// 软件内 = 应用内置浏览器窗口（默认）；浏览器 = 系统默认浏览器
const linkMode = computed(() => store.state.config.extension_link_modes?.[ext.value?.id ?? ''] ?? 'inapp')

const LINK_MODES = [
  { value: 'inapp', label: '软件内' },
  { value: 'browser', label: '浏览器' },
] as const

function setLinkMode(mode: (typeof LINK_MODES)[number]['value']) {
  if (!ext.value) return
  store.setExtensionLinkMode(ext.value.id, mode)
  const label = LINK_MODES.find((m) => m.value === mode)?.label ?? mode
  showToast(`扩展内链接已改为用「${label}」打开`)
}

/** 在系统文件管理器中打开扩展目录（开发调试用） */
const openingDir = ref(false)

async function openDir() {
  if (!ext.value) return
  openingDir.value = true
  try {
    await tauriApi.openExtensionDir(ext.value.id)
  } catch (e) {
    showToast(`打开目录失败：${String(e)}`)
  } finally {
    openingDir.value = false
  }
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
      <div v-if="ext" class="modal-mask" @click.self="emit('close')">
        <div
          ref="cardRef"
          class="modal-card es-card"
          role="dialog"
          aria-label="扩展设置"
          aria-modal="true"
        >
          <header class="es-head">
            <div class="es-head-left">
              <div class="es-icon" :style="ext.icon ? {} : { background: accent?.soft }">
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
                <div v-if="ext.dir" class="es-kv-row es-dir-row">
                  <dt>目录</dt>
                  <dd>
                    <span class="es-dir-path" :title="ext.dir">{{ ext.dir }}</span>
                    <button
                      class="es-dir-btn"
                      type="button"
                      :disabled="openingDir"
                      :title="`在文件管理器中打开：${ext.dir}`"
                      @click="openDir"
                    >
                      <FolderOpen :size="13" :stroke-width="2" aria-hidden="true" />
                      打开目录
                    </button>
                  </dd>
                </div>
              </dl>
              <p v-if="ext.description" class="es-desc">{{ ext.description }}</p>
            </section>

            <section class="es-block">
              <h3 class="es-block-title">权限</h3>
              <p v-if="ext.runtime === 'service'" class="es-empty">本地后端可读取你有权限访问的文件并联网，未受系统沙箱隔离。仅在信任作者与当前版本时开启；更新版本后需重新授权。关闭网络权限也会停止后端。</p>
              <div v-if="Object.keys(perms).length" class="es-perm-list">
                <div v-for="p in Object.keys(perms)" :key="p" class="es-perm-row" :title="p">
                  <span class="es-perm-name">{{ permissionLabel(p) }}</span>
                  <button
                    class="toggle"
                    role="switch"
                    type="button"
                    :aria-checked="perms[p] ?? false"
                    :class="{ on: perms[p] ?? false }"
                    :disabled="permsLoading"
                    @click="togglePermission(p, !(perms[p] ?? false))"
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
              <div class="es-kv-row es-openmode-row">
                <dt>链接打开方式</dt>
                <dd>
                  <div class="es-seg" role="group" aria-label="链接打开方式">
                    <button
                      v-for="m in LINK_MODES"
                      :key="m.value"
                      class="es-seg-btn"
                      :class="{ active: linkMode === m.value }"
                      type="button"
                      @click="setLinkMode(m.value)"
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
              <p class="es-empty es-openmode-hint">从扩展中心点开、或点击左栏固定菜单时，都按上方「打开方式」打开该扩展（默认软件内打开）；扩展内的外部链接按「链接打开方式」打开（默认软件内的应用内浏览器）。</p>
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
  /* 弹窗外壳自带 24px padding，本卡片自排内边距：清零避免与头/体/脚叠加成双份 */
  padding: 0;
}
.es-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  /* 卡片 padding 清零后头/体/脚各自排 18px 内边距；这一条原先漏了，导致图标与关闭按钮
     贴死在卡片左上/右上角（体/脚都有 18px，表头没有） */
  padding: 18px 18px 0;
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
  /* 卡片 padding 清零后由头/体/脚各自排 18px 内边距，与市场详情、发布弹窗全一致 */
  padding: 0 18px;
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
/* 目录行：路径等宽省略 + 「打开目录」按钮（开发调试快速定位扩展所在目录） */
.es-dir-row dd {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  width: 100%;
}
.es-dir-path {
  flex: 1;
  min-width: 0;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.75rem;
  line-height: 1.5;
  color: var(--text-2);
  /* 最多两行：路径尾部（目录名本身）比开头更需要看得见，故不做单行省略号截断 */
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  word-break: break-all;
  overflow: hidden;
}
.es-dir-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  padding: 3px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-size: 0.6875rem;
  font-weight: 600;
  line-height: 1.5;
  white-space: nowrap;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out, border-color 150ms ease-out;
}
.es-dir-btn:hover:not(:disabled) {
  background: var(--brand-50);
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.es-dir-btn:disabled {
  opacity: 0.6;
  cursor: default;
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
  padding: 4px 18px 18px;
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
