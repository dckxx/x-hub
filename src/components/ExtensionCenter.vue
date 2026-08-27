<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import { FolderOpen, MoreHorizontal, PackageOpen, Plus, RefreshCw } from 'lucide-vue-next'
import {
  isTauri,
  tauriApi,
  type MarketDownloadProgress,
  type MarketExtension,
  type MarketStatus,
  type ExtensionEntry,
} from '../api/tauri'
import { accentOf, iconSrc } from '../composables/useResourceIcon'
import { loadExtensionModules } from '../composables/useDashboardLayout'
import ExtensionSettingsDialog from './ExtensionSettingsDialog.vue'
import MarketDetailDialog from './MarketDetailDialog.vue'

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const emit = defineEmits<{
  open: [ext: ExtensionEntry]
  openSurface: [ext: ExtensionEntry, surface: string]
}>()

function onAction(e: ExtensionEntry, surface: string) {
  emit('openSurface', e, surface)
}

function onRowClick(e: ExtensionEntry) {
  if (e.invalid) {
    showToast(`「${e.name}」无法打开：${e.error ?? 'manifest 缺失或损坏'}`)
    return
  }
  if (e.disabled) {
    showToast(`「${e.name}」已在当前环境被禁用（manifest.disabled 条件命中）`)
    return
  }
  if (e.missing_dependencies.length > 0) {
    showToast(`「${e.name}」缺少依赖扩展：${e.missing_dependencies.join('、')}`)
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

/** 汇总缺失的能力 / 依赖，供描述行提示 */
function issuesText(e: ExtensionEntry): string {
  const parts: string[] = []
  if (e.missing_capabilities.length > 0) parts.push(`缺少宿主能力：${e.missing_capabilities.join('、')}`)
  if (e.missing_dependencies.length > 0) parts.push(`缺少依赖扩展：${e.missing_dependencies.join('、')}`)
  return parts.join('；')
}

function descText(e: ExtensionEntry): string {
  if (e.invalid) return e.error ?? '此扩展无法加载'
  const issues = issuesText(e)
  if (issues) return issues
  return e.description || e.id
}

async function load() {
  loading.value = true
  try {
    extensions.value = isTauri()
      ? (await tauriApi.listExtensions()).map((e) => ({
          ...e,
          // 兼容旧后端：新字段可能在旧二进制里缺失，运行时补默认值避免白屏
          disabled: (e as any).disabled ?? false,
          missing_capabilities: (e as any).missing_capabilities ?? [],
          missing_dependencies: (e as any).missing_dependencies ?? [],
          depends_on: (e as any).depends_on ?? [],
          expose: (e as any).expose ?? [],
          actions: (e as any).actions ?? [],
        }))
      : []
    // 安装/卸载后同步刷新工作台模块库，让新扩展的 module 形态立即出现在自定义布局中
    await loadExtensionModules()
  } catch (e) {
    showToast(`加载扩展列表失败：${String(e)}`)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void load()
  // 已安装 tab 也需要市场数据来判断「可更新」，故启动即拉取一次市场清单
  void loadMarket()
  // 运行时热更新：轮询扩展目录内容戳，变化（新装/卸载/改 manifest）即刷新列表，无需重启
  stampTimer = window.setInterval(() => void pollStamp(), 5000)
})

onBeforeUnmount(() => {
  if (stampTimer !== null) window.clearInterval(stampTimer)
})

/** 扩展目录内容戳：首次记录基准，之后变化则刷新列表 */
let stamp = 0
let stampTimer: number | null = null
async function pollStamp() {
  if (!isTauri()) return
  try {
    const s = await tauriApi.extensionsStamp()
    if (stamp !== 0 && s !== stamp) {
      await load()
    }
    stamp = s
  } catch {
    /* 忽略轮询失败 */
  }
}

function onInstall() {
  switchTab('market')
}

const tab = ref<'installed' | 'market'>('installed')
const marketStatus = ref<MarketStatus | null>(null)
const marketLoading = ref(false)
const installingId = ref<string | null>(null)
const installingProgress = ref<MarketDownloadProgress | null>(null)
let unlistenProgress: (() => void) | null = null
const marketFailedIcons = ref(new Set<string>())

/** 市场列表（来自市场状态，远端清单；失败时 Rust 端回退缓存仍能列出） */
const market = computed<MarketExtension[]>(() => marketStatus.value?.extensions ?? [])

/** 已安装列表 → id 索引 / 市场列表 → id 索引（用于版本对比判断更新） */
const installedById = computed(() => new Map(extensions.value.map((e) => [e.id, e])))
const marketById = computed(() => new Map(market.value.map((m) => [m.id, m])))

/** 更新进行中的状态（复用 market-download-progress 事件） */
const updatingId = ref<string | null>(null)
const updatingProgress = ref<MarketDownloadProgress | null>(null)
let unlistenUpdate: (() => void) | null = null

/** 当前查看详情的市场扩展（详情弹窗） */
const detailExt = ref<MarketExtension | null>(null)

function openDetail(m: MarketExtension) {
  detailExt.value = m
}

/** 宿主版本（minAppVersion 门槛判断用） */
const appVersion = ref('')

/** 是否已发起过一次市场加载（成功或失败都置位；避免每次切换 tab 重复拉远端清单） */
let marketRequested = false

function switchTab(t: 'installed' | 'market') {
  tab.value = t
  // 仅首次切到市场才拉取；之后切换不重载（数据缓存于 marketStatus，手动点刷新按钮才重新拉）
  if (t === 'market' && !marketRequested) void loadMarket()
}

async function loadMarket() {
  if (marketLoading.value) return // 防重入（并行触发 / 加载中）
  marketRequested = true
  marketLoading.value = true
  try {
    if (!isTauri()) {
      marketStatus.value = null
      return
    }
    // 并行：刷新市场清单（远端拉取 + 验签 + 落缓存，失败自动回退本地缓存）+ 取宿主版本
    const [status, info] = await Promise.all([tauriApi.refreshMarketRegistry(), tauriApi.getAppInfo()])
    marketStatus.value = status
    appVersion.value = info.version
  } catch (e) {
    showToast(`加载市场失败：${String(e)}`)
  } finally {
    marketLoading.value = false
  }
}

/** 简单语义化版本比较：a < b（分节数字比较，x.y.z 足够） */
function versionLessThan(a: string, b: string): boolean {
  const pa = (a || '').split('.').map(Number)
  const pb = (b || '').split('.').map(Number)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = pa[i] ?? 0
    const y = pb[i] ?? 0
    if (x !== y) return x < y
  }
  return false
}

/** 宿主版本低于扩展要求的 minAppVersion → 不可安装 */
function hostTooOld(m: MarketExtension): boolean {
  return !!m.minAppVersion && appVersion.value !== '' && versionLessThan(appVersion.value, m.minAppVersion)
}

function marketInitial(m: MarketExtension): string {
  return (m.name || m.id || '?').charAt(0).toUpperCase()
}

/** 「上次更新」展示文案（ISO → 本地可读；无则占位） */
function marketUpdatedText(): string {
  const s = marketStatus.value?.last_updated
  if (!s) return '—'
  const d = new Date(s)
  return isNaN(d.getTime()) ? s : d.toLocaleString()
}

function onMarketImgError(m: MarketExtension) {
  marketFailedIcons.value.add(m.id)
}

function progressPercent(p: MarketDownloadProgress | null): number {
  if (!p || !p.total || p.total <= 0) return 0
  return Math.min(100, Math.round((p.received / p.total) * 100))
}

async function installFromMarket(m: MarketExtension) {
  if (!isTauri()) {
    showToast('市场安装需在桌面应用中操作')
    return
  }
  installingId.value = m.id
  installingProgress.value = null
  try {
    unlistenProgress = await listen<MarketDownloadProgress>('market-download-progress', (e) => {
      if (e.payload.id === m.id) installingProgress.value = e.payload
    })
    const id = await tauriApi.installFromMarket(m)
    showToast(`已安装「${id}」`)
    await load()
  } catch (e) {
    showToast(`安装失败：${String(e)}`)
  } finally {
    unlistenProgress?.()
    unlistenProgress = null
    installingId.value = null
    installingProgress.value = null
  }
}

/** 该已装扩展在市场是否有更高版本可更新；无则返回 null */
function updateFor(e: ExtensionEntry): MarketExtension | null {
  const m = marketById.value.get(e.id)
  if (m && versionLessThan(e.version, m.version)) return m
  return null
}

/** 从市场更新扩展（校验 + 版本比较 + 备份 + 保留用户数据 + 原子替换，失败自动回滚） */
async function updateFromMarket(m: MarketExtension) {
  if (!isTauri()) {
    showToast('市场更新需在桌面应用中操作')
    return
  }
  updatingId.value = m.id
  updatingProgress.value = null
  try {
    unlistenUpdate = await listen<MarketDownloadProgress>('market-download-progress', (e) => {
      if (e.payload.id === m.id) updatingProgress.value = e.payload
    })
    const id = await tauriApi.updateFromMarket(m)
    showToast(`已更新「${id}」至 v${m.version}`)
    await load()
  } catch (e) {
    showToast(`更新失败：${String(e)}`)
  } finally {
    unlistenUpdate?.()
    unlistenUpdate = null
    updatingId.value = null
    updatingProgress.value = null
  }
}

/** 市场卡片主按钮动作：已装同版 → 提示已最新；已装旧版 → 更新；未装 → 安装 */
function onMarketAction(m: MarketExtension) {
  const inst = installedById.value.get(m.id)
  if (inst) {
    if (versionLessThan(inst.version, m.version)) {
      void updateFromMarket(m)
      return
    }
    showToast(`「${m.name}」已是最新版本`)
    return
  }
  void installFromMarket(m)
}

/** 市场卡片主按钮文案 */
function marketActionLabel(m: MarketExtension): string {
  const inst = installedById.value.get(m.id)
  if (inst) {
    if (versionLessThan(inst.version, m.version)) {
      if (updatingId.value === m.id) {
        const p = updatingProgress.value
        return p && progressPercent(p) > 0 ? `更新中 ${progressPercent(p)}%` : '更新中…'
      }
      return '更新'
    }
    return '已安装'
  }
  if (installingId.value === m.id) {
    const p = installingProgress.value
    return p && progressPercent(p) > 0 ? `下载中 ${progressPercent(p)}%` : '安装中…'
  }
  return '安装'
}

/** 已装行「更新」按钮文案 */
function updateBtnText(e: ExtensionEntry): string {
  if (updatingId.value !== e.id) return `更新 v${updateFor(e)!.version}`
  const p = updatingProgress.value
  return p && progressPercent(p) > 0 ? `更新中 ${progressPercent(p)}%` : '更新中…'
}

/** 已装扩展是否可更新（供行内按钮 / 市场卡片复用） */
function installedOutdated(m: MarketExtension): boolean {
  const inst = installedById.value.get(m.id)
  return !!inst && versionLessThan(inst.version, m.version)
}

/** 已装且已是最新版本（按钮置灰不可点） */
function installedUpToDate(m: MarketExtension): boolean {
  const inst = installedById.value.get(m.id)
  return !!inst && !versionLessThan(inst.version, m.version)
}

async function onLocalFileInstall() {
  if (!isTauri()) {
    showToast('本地安装需在桌面应用中操作')
    return
  }
  try {
    const file = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'x-hub 扩展包', extensions: ['xhpack'] }],
    })
    if (typeof file !== 'string') return // 取消
    const id = await tauriApi.installLocalArchive(file)
    showToast(`已安装「${id}」`)
    await load()
  } catch (e) {
    showToast(`安装失败：${String(e)}`)
  }
}

async function onLocalFolderInstall() {
  if (!isTauri()) {
    showToast('本地安装需在桌面应用中操作')
    return
  }
  try {
    const dir = await open({ multiple: false, directory: true })
    if (typeof dir !== 'string') return // 取消
    const id = await tauriApi.installExtension(dir)
    showToast(`已安装「${id}」`)
    await load()
  } catch (e) {
    showToast(`安装失败：${String(e)}`)
  }
}

const settingsExt = ref<ExtensionEntry | null>(null)

function onMore(e: ExtensionEntry) {
  settingsExt.value = e
}
</script>

<template>
  <div class="extension-center">
    <header class="ec-header">
      <div class="ec-title-wrap">
        <div class="ec-tabs">
          <button
            class="ec-tab"
            :class="{ active: tab === 'installed' }"
            type="button"
            @click="switchTab('installed')"
          >
            已安装
          </button>
          <button
            class="ec-tab"
            :class="{ active: tab === 'market' }"
            type="button"
            @click="switchTab('market')"
          >
            市场
          </button>
        </div>
        <p class="ec-subtitle">
          {{ tab === 'installed' ? (visibleCount ? `已安装 ${visibleCount} 个扩展` : '管理已安装的扩展') : '发现并安装新扩展' }}
        </p>
      </div>
      <div class="ec-actions">
        <button class="pill-btn" type="button" @click="onInstall">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          安装扩展
        </button>
        <button class="ghost-btn" type="button" @click="onLocalFileInstall">
          <PackageOpen :size="14" :stroke-width="2" aria-hidden="true" />
          导入扩展包
        </button>
        <button class="ghost-btn" type="button" @click="onLocalFolderInstall">
          <FolderOpen :size="14" :stroke-width="2" aria-hidden="true" />
          从文件夹
        </button>
      </div>
    </header>

    <template v-if="tab === 'installed'">
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
          :class="{ invalid: e.invalid, disabled: e.disabled, clickable: !e.invalid && !e.disabled }"
          role="button"
          :tabindex="e.invalid || e.disabled ? undefined : 0"
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
                <span v-if="e.disabled" class="ec-tag ec-tag-disabled">已禁用</span>
                <span v-if="e.missing_capabilities.length" class="ec-tag ec-tag-warn">缺能力</span>
                <span v-if="e.missing_dependencies.length" class="ec-tag ec-tag-warn">缺依赖</span>
                <span v-if="e.runtime === 'service'" class="ec-tag ec-tag-service">service</span>
                <span class="ec-tag ec-tag-kind">{{ kindLabel(e.kind) }}</span>
              </template>
            </div>
            <p class="ec-desc" :title="descText(e)">
              {{ descText(e) }}
            </p>
            <div v-if="e.actions.length" class="ec-actions-row">
              <button
                v-for="a in e.actions"
                :key="a.id"
                class="ec-action-btn"
                type="button"
                :title="`${a.title}（打开 ${kindLabel(a.surface)}）`"
                @click.stop="onAction(e, a.surface)"
              >
                {{ a.title }}
              </button>
            </div>
          </div>

          <div class="ec-right">
            <button
              v-if="updateFor(e)"
              class="ec-update-btn"
              type="button"
              :disabled="updatingId === e.id"
              @click.stop="updatingId === e.id ? undefined : updateFromMarket(updateFor(e)!)"
            >
              {{ updateBtnText(e) }}
            </button>
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
    </template>

    <div v-else class="ec-market">
      <div class="ec-market-toolbar">
        <span class="ec-market-updated" :title="marketStatus?.last_updated || '尚未刷新'">
          上次更新：{{ marketUpdatedText() }}
        </span>
        <button class="ghost-btn" type="button" :disabled="marketLoading" @click="loadMarket">
          <RefreshCw
            :size="12"
            :stroke-width="2"
            aria-hidden="true"
            :class="{ spin: marketLoading }"
          />
          {{ marketLoading ? '刷新中…' : '刷新' }}
        </button>
      </div>
      <div v-if="marketStatus?.error" class="ec-market-warn">
        <span>市场源异常：{{ marketStatus.error }}</span>
        <button class="ghost-btn" type="button" :disabled="marketLoading" @click="loadMarket">
          <RefreshCw :size="12" :stroke-width="2" aria-hidden="true" />
          重试
        </button>
      </div>
      <div v-if="marketLoading" class="ec-empty">
        <p>正在刷新市场…</p>
      </div>
      <template v-else>
        <div v-if="market.length === 0" class="ec-empty">
          <PackageOpen :size="40" :stroke-width="1.5" aria-hidden="true" />
          <h3>市场暂无内容</h3>
          <p>
            扩展市场由远端清单驱动，请确认已发布扩展（或用发布脚本上传 registry.json）
            <span v-if="marketStatus?.last_updated">（上次更新：{{ marketStatus.last_updated }}）</span>
          </p>
          <button class="pill-btn" type="button" @click="loadMarket">刷新市场</button>
        </div>
        <div v-else class="ec-market-list">
          <div v-for="m in market" :key="m.id" class="ec-mcard">
            <div class="ec-mcard-head">
              <div class="ec-mcard-title">
                <span class="ec-mcard-icon" :style="{ background: accentOf(m.name).soft }">
                  <img
                    v-if="m.icon && !marketFailedIcons.has(m.id)"
                    :src="m.icon"
                    :alt="m.name"
                    draggable="false"
                    @error="onMarketImgError(m)"
                  />
                  <span v-else :style="{ color: accentOf(m.name).text }">{{ marketInitial(m) }}</span>
                </span>
                <span class="ec-mcard-name" :title="m.name">{{ m.name }}</span>
              </div>
              <span class="ec-version">v{{ m.version }}</span>
            </div>
            <p class="ec-mcard-desc" :title="m.description || m.id">{{ m.description || m.id }}</p>
            <p v-if="m.changelog" class="ec-mcard-changelog" :title="m.changelog">更新：{{ m.changelog }}</p>
            <div class="ec-mcard-foot">
              <span class="ec-mcard-author">{{ m.author || '—' }}</span>
              <div class="ec-mcard-btns">
                <button class="ghost-btn" type="button" @click="openDetail(m)">详情</button>
                <button
                  class="ghost-btn"
                  :class="{ 'ec-btn-update': installedOutdated(m) }"
                  type="button"
                  :disabled="installingId === m.id || updatingId === m.id || hostTooOld(m) || installedUpToDate(m)"
                  :title="
                    hostTooOld(m)
                      ? `该扩展要求宿主 v${m.minAppVersion}+，当前为 v${appVersion}`
                      : installedOutdated(m)
                        ? `升级到 v${m.version}`
                        : ''
                  "
                  @click="onMarketAction(m)"
                >
                  {{ hostTooOld(m) ? `需 v${m.minAppVersion}+` : marketActionLabel(m) }}
                </button>
              </div>
            </div>
            <div
              v-if="
                (installingId === m.id && installingProgress) ||
                (updatingId === m.id && updatingProgress)
              "
              class="ec-mcard-progress"
            >
              <div
                class="ec-mcard-progress-inner"
                :style="{
                  transform: `scaleX(${
                    (installingId === m.id
                      ? progressPercent(installingProgress)
                      : progressPercent(updatingProgress)) / 100
                  })`,
                }"
              ></div>
            </div>
          </div>
        </div>
      </template>
    </div>

    <ExtensionSettingsDialog
      :extension="settingsExt"
      @close="settingsExt = null"
      @uninstalled="load"
    />

    <MarketDetailDialog
      :extension="detailExt"
      :action-label="detailExt ? marketActionLabel(detailExt) : ''"
      :action-disabled="
        detailExt
          ? installingId === detailExt.id ||
            updatingId === detailExt.id ||
            hostTooOld(detailExt) ||
            installedUpToDate(detailExt)
          : false
      "
      :action-title="
        detailExt
          ? hostTooOld(detailExt)
            ? `该扩展要求宿主 v${detailExt.minAppVersion}+，当前为 v${appVersion}`
            : installedOutdated(detailExt)
              ? `升级到 v${detailExt.version}`
              : ''
          : ''
      "
      @action="detailExt && onMarketAction(detailExt)"
      @close="detailExt = null"
    />
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
.ec-tabs {
  display: flex;
  align-items: center;
  gap: 4px;
}
.ec-tab {
  padding: 4px 12px;
  border: 0;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-3);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.ec-tab:hover {
  color: var(--text-1);
}
.ec-tab.active {
  background: var(--brand-50);
  color: var(--brand-500);
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
.ec-row.disabled {
  opacity: 0.6;
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
.ec-tag-disabled {
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.ec-tag-warn {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.ec-desc {
  margin: 2px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-actions-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 5px;
}
.ec-action-btn {
  padding: 2px 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-size: 0.6875rem;
  font-weight: 600;
  line-height: 1.5;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out, border-color 150ms ease-out;
}
.ec-action-btn:hover {
  background: var(--brand-50);
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.ec-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ec-update-btn {
  padding: 3px 10px;
  border: 1px solid var(--brand-500);
  border-radius: var(--radius-pill);
  background: var(--brand-50);
  color: var(--brand-500);
  font-size: 0.6875rem;
  font-weight: 650;
  line-height: 1.5;
  white-space: nowrap;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out, transform 150ms ease-out;
}
.ec-update-btn:hover {
  background: var(--brand-500);
  color: #fff;
  transform: translateY(-1px);
}
.ec-update-btn:disabled {
  opacity: 0.6;
  cursor: default;
  transform: none;
}
.ec-btn-update {
  border-color: var(--brand-500);
  color: var(--brand-500);
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

/* 市场 */
.ec-market {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px;
}
.ec-market-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}
.ec-market-updated {
  font-size: 0.75rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-market-toolbar .ghost-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
}
.spin {
  animation: ec-spin 0.8s linear infinite;
}
@keyframes ec-spin {
  to {
    transform: rotate(360deg);
  }
}
.ec-market-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 10px;
}
.ec-mcard {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
  transition: transform 150ms ease-out;
}
.ec-mcard:hover {
  transform: translateY(-1px);
}
.ec-mcard-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.ec-mcard-name {
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-mcard-desc {
  flex: 1;
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-3);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.ec-mcard-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.ec-mcard-author {
  font-size: 0.75rem;
  color: var(--text-4);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-mcard code {
  font-size: 0.72rem;
  background: var(--bg-card-soft);
  padding: 1px 5px;
  border-radius: 4px;
}
.ec-market-warn {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--c-yellow-soft, rgba(240, 180, 40, 0.14));
  color: var(--c-yellow-ink, #b5850a);
  font-size: 0.75rem;
  flex-shrink: 0;
}
.ec-market-warn .ghost-btn {
  flex-shrink: 0;
}
.ec-mcard-title {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ec-mcard-icon {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  font-size: 0.8125rem;
  font-weight: 700;
  overflow: hidden;
}
.ec-mcard-icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.ec-mcard-changelog {
  margin: 0;
  font-size: 0.6875rem;
  color: var(--text-4, var(--text-3));
  line-height: 1.5;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ec-mcard-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ec-mcard-foot .ec-mcard-author {
  flex: 1;
}
.ec-mcard-btns {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.ec-mcard-progress {
  height: 3px;
  border-radius: 2px;
  background: var(--bg-card-soft);
  overflow: hidden;
}
.ec-mcard-progress-inner {
  height: 100%;
  border-radius: 2px;
  background: var(--brand-500);
  transform-origin: left center;
  transform: scaleX(0);
  transition: transform 150ms ease-out;
}
.ec-mcard-foot .ghost-btn {
  min-width: 64px;
}
</style>
