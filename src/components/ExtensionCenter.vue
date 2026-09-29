<script setup lang="ts">
import { computed, inject, onMounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import { FolderCog, FolderOpen, PackageOpen, Play, Plus, RefreshCw, Trash2 } from 'lucide-vue-next'
import {
  isTauri,
  tauriApi,
  type DevExtensionInfo,
  type DevModeStatus,
  type MarketDownloadProgress,
  type MarketExtension,
  type MarketStatus,
  type ExtensionEntry,
} from '../api/tauri'
import { accentOf, iconSrc } from '../composables/useResourceIcon'
import { loadExtensionModules } from '../composables/useDashboardLayout'
import { useAdaptivePolling } from '../composables/useAdaptivePolling'
import ExtensionSettingsDialog from './ExtensionSettingsDialog.vue'
import MarketDetailDialog from './MarketDetailDialog.vue'
import ExtensionPublishDialog from './ExtensionPublishDialog.vue'

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const emit = defineEmits<{
  open: [ext: ExtensionEntry]
  openSurface: [ext: ExtensionEntry, surface: string]
  changed: []
  /** 「去安装扩展开发 Skill」：跳设置页的 Skills 分区（宿主里那条安装入口） */
  openSkills: []
}>()

// 外部请求打开某扩展的设置/授权弹窗（扩展页 xhub.openPermissions「去授权」跳转）。
// 本组件可能尚未挂载或列表未加载完：按 nonce 去重，列表就绪前留待 watch(extensions) 补开
const props = defineProps<{
  /** { 扩展 id, 递增序号 }：nonce 变化即一次新请求；null = 无待处理请求 */
  jumpSettings?: { id: string; nonce: number } | null
}>()

let handledJumpNonce = -1

function tryOpenJumpedSettings() {
  const j = props.jumpSettings
  if (!j || j.nonce === handledJumpNonce) return
  const entry = extensions.value.find((e) => e.id === j.id)
  if (!entry) return // 列表未就绪/扩展不存在：等列表加载后再试一次
  handledJumpNonce = j.nonce
  settingsExt.value = entry
}

watch(() => props.jumpSettings, tryOpenJumpedSettings)

function onAction(e: ExtensionEntry, surface: string) {
  emit('openSurface', e, surface)
}

/** 行点击守卫：不可用/禁用/缺依赖时给出原因提示，可打开时执行传入动作 */
function withRowGuard(e: ExtensionEntry, action: () => void) {
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
  action()
}

/** 「已安装」行点击：默认打开**扩展详情**（信息 / 权限 / 打开方式 / 卸载），
 *  直接打开扩展走右侧 ▶ 按钮 */
function onRowClick(e: ExtensionEntry) {
  withRowGuard(e, () => {
    settingsExt.value = e
  })
}

/** 「已安装」行 ▶ 按钮：直接打开扩展（按该扩展设置里的「打开方式」） */
function openInstalledExtension(e: ExtensionEntry) {
  withRowGuard(e, () => emit('open', e))
}

const extensions = ref<ExtensionEntry[]>([])
const loading = ref(true)
const failedIcons = ref(new Set<string>())

/** 已安装清单：「我的扩展」直挂的源码目录不算「已安装」，它们有自己的标签页。
 *  按安装时间（目录创建时间）升序：先装在上、最后安装的在最下面；取不到时间的排最前 */
const installedExtensions = computed(() =>
  extensions.value
    .filter((e) => e.source !== 'dev')
    .slice()
    .sort((a, b) => (a.installed_at ?? '').localeCompare(b.installed_at ?? '')),
)

const visibleCount = computed(() => installedExtensions.value.filter((e) => !e.invalid).length)

/** 标签页下的一句话说明：让用户不问「这一页是干嘛的」 */
const subtitle = computed(() => {
  if (tab.value === 'installed') {
    return visibleCount.value ? `已安装 ${visibleCount.value} 个扩展` : '管理已安装的扩展：点开使用，右侧可更新或卸载'
  }
  if (tab.value === 'market') return '发现并安装新扩展'
  return devMode.value.extensions.length
    ? `本机源码目录 ${devMode.value.extensions.length} 个，改代码即自动重载`
    : '添加本机扩展源码目录，边写边看效果'
})

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

/** 在系统文件管理器中打开扩展目录（开发调试：改完代码一眼找到源码） */
async function openDir(e: ExtensionEntry) {
  if (!isTauri()) {
    showToast('打开扩展目录需在桌面应用中使用')
    return
  }
  try {
    await tauriApi.openExtensionDir(e.id)
  } catch (err) {
    showToast(`打开目录失败：${String(err)}`)
  }
}

/** 发布配额（账号级）：展示在扩展管理页，供发布前心里有数；取不到就不显示，不打扰 */
const quota = ref<{ drafts_remaining?: number; published_remaining?: number; daily_submits_remaining?: number } | null>(
  null,
)

async function loadQuota() {
  if (!isTauri()) return
  try {
    // 列表与配额同一个响应（服务端 dev/submissions 返回 quota）；这里只要配额，列表忽略
    const r = await tauriApi.devListSubmissions(1, 1)
    quota.value = r.quota ?? null
  } catch {
    // 未登录 / 服务不可达：静默（配额只是参考信息，不挡任何操作）
  }
}

async function load() {
  loading.value = true
  try {
    extensions.value = isTauri()
      ? (await tauriApi.listExtensions()).map((e) => ({
          ...e,
          // 兼容旧后端：新字段可能在旧二进制里缺失，运行时补默认值避免白屏
          source: ((e as any).source ?? 'installed') as 'installed' | 'dev',
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
    // 发布配额跟着刷新（装/卸/发布都会改变「在架可新增」「待处理」的余量）
    void loadQuota()
    // 扩展列表变化（装/卸/更新）时通知宿主刷新侧栏固定扩展，让已卸载的图标立即消失
    emit('changed')
  } catch (e) {
    showToast(`加载扩展列表失败：${String(e)}`)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void load()
  // 发布配额（账号级）——发布弹窗里已不再展示，改由本页承载
  void loadQuota()
  // 已安装 tab 也需要市场数据来判断「可更新」，故启动即拉取一次市场清单
  void loadMarket()
  // 「我的扩展」标签页的目录清单（列表真源；发布入口是否出现也看它）
  void loadDevMode()
})

/** 扩展目录内容戳：首次记录基准，之后变化则刷新列表 */
let stamp = 0
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

// 运行时热更新：轮询扩展目录内容戳，变化（新装/卸载/改 manifest）即刷新列表，无需重启。
// 目录扫描是重量级 FS + IPC：可见且聚焦 5s，失焦/隐藏完全停——扩展文件只会在用户
// 于本窗口操作时变化，失焦期间不可能变；从停止恢复时立即补扫一次（useAdaptivePolling）。
useAdaptivePolling(pollStamp, { activeMs: 5000 })

function onInstall() {
  switchTab('market')
}

const tab = ref<'installed' | 'market' | 'dev'>('installed')

// ---- 我的扩展（「我的扩展」直挂的本机源码目录）----
// 列表来源是 get_dev_mode_status（注册了哪些目录），**不是**已加载的扩展清单：
// 目录无效 / 冲突时不会被加载，但用户仍然要能看到并移除它们（见 ADR 0005）。
// enabled 是「开发者模式开关」的遗留字段，恒为 true（约定 55）；初值必须与 ExtensionsPanel 一致
const devMode = ref<DevModeStatus>({ enabled: true, extensions: [] })
const devBusy = ref(false)

/** 路径比较：统一分隔符与大小写（同一目录的两种写法要能对上） */
function normalizePath(p: string): string {
  return p.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase()
}

/** 该直挂目录对应到已加载的扩展（目录有效且 manifest 可解析时才有） */
function devEntryFor(d: DevExtensionInfo): ExtensionEntry | undefined {
  const key = normalizePath(d.path)
  return extensions.value.find((e) => e.source === 'dev' && normalizePath(e.dir) === key)
}

/** 可以打开 / 发布：目录有效且已加载（未加载 = 目录无效或与已装扩展同 id） */
function devReady(d: DevExtensionInfo): boolean {
  return d.exists && d.valid && !d.conflict
}

/** 发布入口：源码已加载出来才能打包上传；能否真正发布由发布弹窗的开发者认证把关 */
function canPublish(d: DevExtensionInfo): boolean {
  return devReady(d) && !!devEntryFor(d)
}

function devAccent(d: DevExtensionInfo) {
  return accentOf(d.name || d.id || d.path)
}

function devInitial(d: DevExtensionInfo): string {
  return (d.name || d.id || '?').charAt(0).toUpperCase()
}

function devDesc(d: DevExtensionInfo): string {
  if (!d.exists) return '目录不存在（可能已被移动或删除）'
  if (!d.valid) return d.error ?? 'manifest.json 无法解析'
  if (d.conflict) return '与已装扩展同 id：已装优先，此目录不会被加载（点右侧「卸载已装版」即可生效）'
  return d.path
}

async function loadDevMode() {
  if (!isTauri()) return
  try {
    devMode.value = await tauriApi.getDevModeStatus()
  } catch {
    // 后端不可用（浏览器预览）时静默：该标签页只在桌面端有意义
  }
}

/** 添加本机扩展源码目录（须含 manifest.json） */
async function pickDevDir() {
  if (!isTauri()) {
    showToast('添加本地扩展需在桌面应用中操作')
    return
  }
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      title: '选择扩展源码目录（须含 manifest.json）',
    })
    const path = typeof picked === 'string' ? picked : null
    if (!path) return
    devBusy.value = true
    devMode.value = await tauriApi.addDevExtension(path)
    showToast('已添加到「我的扩展」并立即加载，改代码即自动重载')
    await load()
  } catch (e) {
    showToast(String(e))
  } finally {
    devBusy.value = false
  }
}

/** 移除直挂目录（只解除挂载，不动磁盘上的源码） */
async function removeDevDir(path: string) {
  try {
    devMode.value = await tauriApi.removeDevExtension(path)
    showToast('已从「我的扩展」移除')
    await load()
  } catch (e) {
    showToast(String(e))
  }
}

/**
 * 卸载与直挂目录同 id 的已装扩展，好让直挂目录生效。
 *
 * 背景：同 id 冲突时**已装优先**、开发目录被静默跳过（见 extension.rs 的说明与
 * docs/adr/0005）。宿主选择这个优先级是为了避免「打开的到底是哪一份」——
 * 存储、权限、发布提交都以扩展 id 为键，同 id 两份会互相串味。
 * 但对「自己发布、自己又装」的作者来说，这一步很常做，所以在这里给一键入口，
 * 省得去「已安装」页找。**只卸载已装版本，磁盘上的源码目录不动。**
 */
async function uninstallConflicting(d: DevExtensionInfo) {
  devBusy.value = true
  try {
    await tauriApi.uninstallExtension(d.id)
    showToast(`已卸载已装版本「${d.name || d.id}」，源码目录现在生效`)
    await load()
    devMode.value = await tauriApi.getDevModeStatus()
  } catch (e) {
    showToast(`卸载失败：${String(e)}`)
  } finally {
    devBusy.value = false
  }
}

/** 「我的扩展」行点击：默认打开**扩展详情**（信息 / 权限 / 打开方式），
 *  不直接跑扩展——打开走右侧「打开扩展」按钮，避免想看信息时把扩展跑起来 */
function onDevRowClick(d: DevExtensionInfo) {
  if (!devReady(d)) {
    showToast(devDesc(d))
    return
  }
  const e = devEntryFor(d)
  if (!e) {
    showToast('该目录尚未加载：请确认 manifest.json 合法且未与已装扩展同 id')
    return
  }
  onMore(e)
}

/** 「打开扩展」按钮：与已装标签页点行同口径（emit open，按该扩展的「打开方式」打开） */
function openDevExtension(e: ExtensionEntry) {
  emit('open', e)
}
const marketStatus = ref<MarketStatus | null>(null)
const marketLoading = ref(false)
const installingId = ref<string | null>(null)
const installingProgress = ref<MarketDownloadProgress | null>(null)
let unlistenProgress: (() => void) | null = null
const marketFailedIcons = ref(new Set<string>())

/** 市场列表（来自市场状态，远端清单；失败时 Rust 端回退缓存仍能列出） */
const market = computed<MarketExtension[]>(() => marketStatus.value?.extensions ?? [])

/**
 * 已安装列表 → id 索引 / 市场列表 → id 索引（用于版本对比判断更新）。
 *
 * ⚠️ 必须与「已安装」标签页同口径，只收 `source !== 'dev'`：
 * 全量 `extensions` 里也含「我的扩展」直挂的本机源码目录（source='dev'），
 * 而这类扩展**不复制进已安装、也不参与市场更新与卸载**（见 dev 标签页的说明）。
 * 用全量建索引会让市场卡片把直挂扩展误判成「已安装」——实机表现是
 * 「已卸载已装版本、只留直挂目录，市场仍显示『已安装』且版本停在旧号」。
 */
const installedById = computed(() => new Map(installedExtensions.value.map((e) => [e.id, e])))
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

function switchTab(t: 'installed' | 'market' | 'dev') {
  tab.value = t
  // 仅首次切到市场才拉取；之后切换不重载（数据缓存于 marketStatus，手动点刷新按钮才重新拉）
  if (t === 'market' && !marketRequested) void loadMarket()
  // 我的扩展：每次切过去都重新取一遍，避免磁盘上的目录被外部改动后状态是旧的
  if (t === 'dev') void loadDevMode()
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

/** 市场错误提示：隐藏具体 URL（reqwest 错误串可能带 endpoint），对用户只显示原因类别 */
function marketErrorText(): string {
  const err = marketStatus.value?.error ?? ''
  // 去掉 http(s)://... 形式的地址，避免把市场源 URL 暴露给用户
  return err.replace(/https?:\/\/[^\s，。、\)）]+/g, '(网络地址)')
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

/** 清单条目是否命中撤销列表（`id@version`，大小写不敏感、容忍条目空白） */
function isRevokedEntry(m: MarketExtension): boolean {
  const list = marketStatus.value?.revoked ?? []
  const key = `${m.id}@${m.version}`.toLowerCase()
  return list.some((r) => r.trim().toLowerCase() === key)
}

/** 已装扩展的当前版本是否已被平台下架：只警示 + 停止推送该版本，绝不静默卸载或禁用 */
function isInstalledRevoked(e: ExtensionEntry): boolean {
  const list = marketStatus.value?.revoked ?? []
  const key = `${e.id}@${e.version}`.toLowerCase()
  return list.some((r) => r.trim().toLowerCase() === key)
}

/** 该已装扩展在市场是否有更高版本可更新；无则返回 null。
 *  目标版本若已被撤销，一律不提供更新入口——绝不把用户推向被撤回的版本。 */
function updateFor(e: ExtensionEntry): MarketExtension | null {
  const m = marketById.value.get(e.id)
  if (m && versionLessThan(e.version, m.version) && !isRevokedEntry(m)) return m
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

const settingsExt = ref<ExtensionEntry | null>(null)
// 跳转补开：扩展列表就绪（或变化）后若仍有未处理的 jumpSettings 请求，再试一次
watch(extensions, tryOpenJumpedSettings)
/** 发布弹窗的目标扩展（开发中的扩展可用；已装扩展也可重发新版本） */
const publishTarget = ref<ExtensionEntry | null>(null)

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
          <button
            class="ec-tab"
            :class="{ active: tab === 'dev' }"
            type="button"
            @click="switchTab('dev')"
          >
            我的扩展
          </button>
        </div>
        <p class="ec-subtitle">{{ subtitle }}</p>
      </div>
      <div class="ec-actions">
        <template v-if="tab === 'dev'">
          <button class="pill-btn" type="button" :disabled="devBusy" @click="pickDevDir">
            <FolderCog :size="14" :stroke-width="2" aria-hidden="true" />
            选择目录
          </button>
        </template>
        <template v-else>
          <button class="pill-btn" type="button" @click="onInstall">
            <Plus :size="14" :stroke-width="2" aria-hidden="true" />
            安装扩展
          </button>
          <button class="ghost-btn" type="button" @click="onLocalFileInstall">
            <PackageOpen :size="14" :stroke-width="2" aria-hidden="true" />
            导入扩展包
          </button>
        </template>
        <!-- 发布配额：账号级的（不限当前扩展、不限来源），放在扩展管理页比塞进发布弹窗更好找。
             ⚠️ 三个数都是**剩余**额度（服务端只回剩余、不回上限，见 PRD），文案必须写成「还可…」——
             写成「待处理 5」会被读成「已经有 5 条待处理」（实际含义是「还能再提交 5 条」）。 -->
        <p v-if="quota" class="ec-quota" title="发布扩展的平台配额（全账号共用；数字是还能用的额度）">
          发布配额：待处理还可
          <b :class="{ full: quota.drafts_remaining === 0 }">{{ quota.drafts_remaining ?? '—' }}</b> 条 · 今日还可提交
          <b :class="{ full: quota.daily_submits_remaining === 0 }">{{ quota.daily_submits_remaining ?? '—' }}</b> 次 · 在架还可新增
          <b :class="{ full: quota.published_remaining === 0 }">{{ quota.published_remaining ?? '—' }}</b> 个
        </p>
      </div>
    </header>

    <template v-if="tab === 'installed'">
      <div v-if="loading" class="ec-empty">
        <p>正在扫描扩展…</p>
      </div>

      <div v-else-if="installedExtensions.length === 0" class="ec-empty">
        <PackageOpen :size="40" :stroke-width="1.5" aria-hidden="true" />
        <h3>还没有安装任何扩展</h3>
        <p>到「市场」挑一个装上，工作台就能扩展出你需要的功能</p>
        <button class="pill-btn" type="button" @click="onInstall">去市场看看</button>
      </div>

      <div v-else class="ec-list">
        <div
          v-for="e in installedExtensions"
          :key="e.id"
          class="ec-row"
          :class="{ invalid: e.invalid, disabled: e.disabled, clickable: !e.invalid && !e.disabled }"
          role="button"
          :tabindex="e.invalid || e.disabled ? undefined : 0"
          @click="onRowClick(e)"
          @keydown.enter="onRowClick(e)"
        >
          <div class="ec-icon" :style="showImg(e) ? {} : { background: accentFor(e).soft }">
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
              <span v-if="isInstalledRevoked(e)" class="ec-tag ec-tag-revoked">已下架</span>
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
            <p v-if="isInstalledRevoked(e)" class="ec-revoked-note">
              该版本已被平台下架，建议尽快卸载。平台不会自动卸载或禁用你本机已装的扩展。
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
              :disabled="e.invalid"
              :aria-label="`打开 ${e.name} 所在目录`"
              :title="e.invalid ? '此扩展目录不可用' : `打开所在目录：${e.dir}`"
              @click.stop="e.invalid ? undefined : openDir(e)"
            >
              <FolderOpen :size="16" :stroke-width="2" aria-hidden="true" />
            </button>
            <button
              class="ec-more"
              type="button"
              :aria-label="`打开 ${e.name}`"
              :data-tip="`打开 ${e.name}`"
              :title="`打开 ${e.name}（按扩展设置里的「打开方式」打开，默认软件内）`"
              @click.stop="openInstalledExtension(e)"
            >
              <Play :size="16" :stroke-width="2" aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- 我的扩展：「我的扩展」直挂的本机源码目录（增删 + 开启后可发布） -->
    <div v-else-if="tab === 'dev'" class="ec-dev">
      <div class="ec-hint">
        <p class="ec-hint-line">
          添加本机扩展源码目录（须含 <code>manifest.json</code>）后<b>立即加载</b>，<b>目录即真源</b>：改代码保存约 1.5 秒自动重载，可用真实数据与 service 后端；先在本机把效果调好，觉得可以了再走发布（需要开发者认证）。
        </p>
        <p class="ec-hint-line">
          这类扩展<b>不复制进「已安装」</b>，也不参与市场更新与卸载；移除目录即撤销，磁盘上的源码不动。
          点扩展行打开<b>详情</b>（信息 / 权限 / 打开方式），右侧 <b>▶</b> 直接打开扩展。
        </p>
        <p class="ec-hint-line">
          想<b>快速开发自己的扩展</b>？先去「<b>设置 → 扩展 → Skills</b>」安装<b>扩展开发 Skill</b>，让 AI 助手陪你从零把它做出来。
          <button class="ec-hint-jump" type="button" @click="emit('openSkills')">点击跳转&gt;&gt;</button>
        </p>
      </div>

      <div v-if="!isTauri()" class="ec-empty">
        <p>本地扩展调试需在桌面应用中使用</p>
      </div>

      <div v-else-if="devMode.extensions.length === 0" class="ec-empty">
        <FolderCog :size="40" :stroke-width="1.5" aria-hidden="true" />
        <h3>还没有添加本机扩展</h3>
        <p>选一个含 manifest.json 的源码目录，改完保存就能在宿主里看到效果</p>
        <button class="pill-btn" type="button" :disabled="devBusy" @click="pickDevDir">
          选择源码目录
        </button>
      </div>

      <div v-else class="ec-list">
        <div
          v-for="d in devMode.extensions"
          :key="d.path"
          class="ec-row"
          :class="{ invalid: !d.exists || !d.valid || d.conflict, clickable: devReady(d) }"
          role="button"
          :tabindex="devReady(d) ? 0 : undefined"
          @click="onDevRowClick(d)"
          @keydown.enter="onDevRowClick(d)"
        >
          <div class="ec-icon" :style="devEntryFor(d)?.icon ? {} : { background: devAccent(d).soft }">
            <img
              v-if="devEntryFor(d)?.icon"
              :src="iconSrc(devEntryFor(d)!.icon!)"
              :alt="d.name"
              draggable="false"
            />
            <span v-else :style="{ color: devAccent(d).text }">{{ devInitial(d) }}</span>
          </div>

          <div class="ec-meta">
            <div class="ec-name-line">
              <span class="ec-name">{{ d.name || d.id || '（manifest 无法解析）' }}</span>
              <span class="ec-tag ec-tag-dev">源码直挂</span>
              <span v-if="d.version" class="ec-tag ec-tag-kind">v{{ d.version }}</span>
            </div>
            <p class="ec-desc" :title="d.path">{{ devDesc(d) }}</p>
          </div>

          <div class="ec-right">
            <button
              v-if="canPublish(d)"
              class="ec-update-btn"
              type="button"
              :title="`把「${d.name || d.id}」打包发布到扩展市场`"
              @click.stop="publishTarget = devEntryFor(d)!"
            >
              发布
            </button>
            <button
              v-if="devEntryFor(d)"
              class="ec-more"
              type="button"
              :aria-label="`打开 ${d.name || d.id} 源码目录`"
              :title="`打开源码目录：${d.path}`"
              @click.stop="openDir(devEntryFor(d)!)"
            >
              <FolderOpen :size="16" :stroke-width="2" aria-hidden="true" />
            </button>
            <button
              v-if="devEntryFor(d)"
              class="ec-more"
              type="button"
              :aria-label="`打开 ${d.name || d.id}`"
              :data-tip="`打开 ${d.name || d.id}`"
              :title="`打开 ${d.name || d.id}（按扩展设置里的「打开方式」打开，默认软件内）`"
              @click.stop="openDevExtension(devEntryFor(d)!)"
            >
              <Play :size="16" :stroke-width="2" aria-hidden="true" />
            </button>
            <button
              v-if="d.conflict"
              class="ec-remove-btn"
              type="button"
              :disabled="devBusy"
              :title="`卸载已装版本「${d.name || d.id}」，让本机源码目录生效（不动磁盘上的源码）`"
              @click.stop="uninstallConflicting(d)"
            >
              <Trash2 :size="13" :stroke-width="2" aria-hidden="true" />
              卸载已装版
            </button>
            <button
              class="ec-remove-btn"
              type="button"
              :disabled="devBusy"
              :title="`从「我的扩展」移除（不删除磁盘上的源码）`"
              @click.stop="removeDevDir(d.path)"
            >
              <Trash2 :size="13" :stroke-width="2" aria-hidden="true" />
              移除
            </button>
          </div>
        </div>
      </div>
    </div>

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
        <span>市场源异常：{{ marketErrorText() }}</span>
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
          <button class="pill-btn" type="button" @click="loadMarket">刷新市场</button>
        </div>
        <div v-else class="ec-market-list">
          <div v-for="m in market" :key="m.id" class="ec-mcard">
            <div class="ec-mcard-head">
              <div class="ec-mcard-title">
                <span class="ec-mcard-icon" :style="m.icon && !marketFailedIcons.has(m.id) ? {} : { background: accentOf(m.name).soft }">
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

    <!-- 发布扩展：本机打包 → 上传平台 → 展示服务端返回的关卡逐项结论（客户端只问不判） -->
    <ExtensionPublishDialog :extension="publishTarget" @close="publishTarget = null" />
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
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
  flex-shrink: 0;
}
/* 发布配额：账号级余量（不限当前扩展），占满一行右对齐；归零的那项标红加粗，一眼看出被什么卡住 */
.ec-quota {
  flex-basis: 100%;
  margin: 0;
  text-align: right;
  font-size: 0.6875rem;
  line-height: 1.5;
  color: var(--text-3);
}
.ec-quota b {
  font-weight: 650;
  color: var(--text-1);
}
.ec-quota b.full {
  color: var(--c-red-ink);
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
/* 开发扩展（源码目录直挂）：与已装扩展区分，提示它不参与市场更新与卸载 */
.ec-tag-dev {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
/* 已被平台下架的版本（清单 revoked 命中）：警示但不自动处置用户本机的扩展 */
.ec-tag-revoked {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.ec-revoked-note {
  margin: 4px 0 0;
  font-size: 0.72rem;
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
.ec-more:hover:not(:disabled) {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ec-more:disabled {
  opacity: 0.45;
  cursor: default;
}

/* 我的扩展（源码直挂）：顶部说明 + 目录清单 */
.ec-dev {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: hidden;
}
.ec-hint {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: var(--radius-lg);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  box-shadow: var(--shadow-card);
}
.ec-hint-line {
  margin: 0;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-3);
}
.ec-hint-line b {
  color: var(--text-1);
}
.ec-hint-line code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--bg-card-soft);
  font-size: 0.72rem;
}
/* 「点击跳转 >>」：品牌色文字按钮，跟着说明走一行（不是块级按钮，别抢视线） */
.ec-hint-jump {
  margin-left: 4px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--brand-500);
  font-size: 0.75rem;
  font-weight: 700;
  font-family: inherit;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
  text-decoration-thickness: 1px;
}
.ec-hint-jump:hover {
  color: var(--brand-600, var(--brand-500));
  text-decoration-thickness: 2px;
}
.ec-remove-btn {
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
.ec-remove-btn:hover:not(:disabled) {
  background: var(--c-red-soft);
  border-color: var(--c-red-ink);
  color: var(--c-red-ink);
}
.ec-remove-btn:disabled {
  opacity: 0.6;
  cursor: default;
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
