<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ImagePlus, Info, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  isTauri,
  tauriApi,
  type DevSubmissionRow,
  type ExtensionEntry,
  type PrecheckResult,
  type SubmitResult,
} from '../api/tauri'

// 发布扩展：打包(客户端) → 上传(平台) → 展示服务端返回的关卡逐项结论。
// 客户端不内置任何审核规则，只负责"把包送上去 + 把结论如实展示出来"（红线：客户端只问不判）。
const props = defineProps<{ extension: ExtensionEntry | null }>()
const emit = defineEmits<{ close: [] }>()

const visible = computed(() => !!props.extension)
const cardRef = ref<HTMLElement | null>(null)
const showToast = inject<(msg: string) => void>('showToast', () => {})
useFocusTrap(visible, cardRef)

const changelog = ref('')
const homepage = ref('')
const minAppVersion = ref('')
/** 作者署名：预填 manifest.author，非空且与当前署名不同时提交会写回 manifest.json（市场卡片与详情页展示它） */
const author = ref('')
/** 「发布版本」输入框：非空 = 提交时先把该版本写回扩展的 manifest.json 再打包；留空 = 按 manifest 当前版本发布（重提同一版的路径） */
const newVersion = ref('')
/** 本会话跟踪的当前 manifest 版本：props.extension 在提交后是旧值（列表要等 5s stamp 轮询才刷新），提交成功后用 result.version 就地更新 */
const currentVersion = ref('')
const submitting = ref(false)
const result = ref<SubmitResult | null>(null)
const errorText = ref('')

const submissions = ref<DevSubmissionRow[]>([])
/**
 * 展示用的分页：**全量始终在内存里**（判定与配额明细要用全量），列表只限制渲染条数。
 * 提交记录可能上百条，一次性渲染既慢又没法定位，所以显示层做「每页 20 条 + 显示更多」。
 */
const PAGE_SIZE = 20
const visibleCount = ref(PAGE_SIZE)
const allLoading = ref(false)
/** 全量是否已拉到（阻塞判定与配额明细必须基于全量，只看第 1 页会出现「有待审却放行」「数字对不上明细」） */
const allLoaded = ref(false)
const quota = ref<{ drafts_remaining?: number; published_remaining?: number; daily_submits_remaining?: number } | null>(null)

/** 当前扩展的提交记录（列表只显示这一份：看版本历史不该被别的扩展刷屏） */
const extSubmissions = computed(() => submissions.value.filter((s) => s.ext_id === props.extension?.id))

/** 列表里要显示的记录（超过一页时按当前页截断） */
const visibleSubmissions = computed(() => extSubmissions.value.slice(0, visibleCount.value))
/** 还有未展示的记录 */
const hasMore = computed(() => extSubmissions.value.length > visibleCount.value)

/** 展开配额明细（「待处理 N」到底是哪几条） */
const showDraftDetail = ref(false)

/** 市场清单里的撤销列表（`id@version`）：判断某条已上架的提交是否已被下架 */
const revokedKeys = ref<string[]>([])

/** 某条提交对应的版本是否已被平台下架（下架只写 `revoked` 表、**不改 submissions.status**） */
function isDelisted(s: DevSubmissionRow): boolean {
  return s.status === 'published' && revokedKeys.value.includes(`${s.ext_id}@${s.version}`)
}

/** 列表里要显示的记录状态：已下架的不再算「已上架」 */
function statusKey(s: DevSubmissionRow): string {
  return isDelisted(s) ? 'delisted' : s.status
}

/** 能否撤回（与服务端 `not_withdrawable` 的白名单一致；`approved` 不可撤回） */
function isWithdrawable(s: DevSubmissionRow): boolean {
  return (WITHDRAWABLE as readonly string[]).includes(s.status)
}

// ---- 提交阻塞判定：拦在打包之前 ----
// 服务端只有在**整包上传完成之后**才校验，撞上限制的代价是白等几分钟的打包上传（`publisher.rs::dev_submit`
// 先 pack_to_temp + upload，服务端 `checkDevQuota` 才回 429）。所以这里前置判定、就地说明原因。
//
// 口径：**同一扩展同时只允许一个待审版本**——有一条未走完流程的提交时必须先撤回或等结果。
// ⚠️ 两个集合刻意分开，别合并：
//   · `WITHDRAWABLE`（阻塞集合）：能撤回的才拦——拦住也一定能靠自己解开，不会「被拦又无从下手」；
//   · `DRAFT_STATUSES`（配额明细集合）：服务端 `quotaView` 的 drafts 口径，比上面**多一个 `approved`**
//     （审核通过、但还没真正写进市场清单的短暂过渡态）。它占名额，却**不可撤回**（服务端只允许
//     uploaded / gate_failed / pending_review 撤回），所以只用于算明细、不用于拦提交。
const WITHDRAWABLE = ['uploaded', 'pending_review', 'gate_failed'] as const
const DRAFT_STATUSES = ['uploaded', 'pending_review', 'gate_failed', 'approved'] as const

/** 占用「待处理」名额的提交（账号级、含所有扩展，与配额视图数字同源） */
const draftBlockers = computed(() =>
  submissions.value.filter((s) => (DRAFT_STATUSES as readonly string[]).includes(s.status)),
)

/** 该扩展当前未走完流程、且**可撤回**的提交（倒序取最新一条） */
const blockingSubmission = computed(() => {
  const extId = props.extension?.id
  if (!extId) return null
  return submissions.value.find((s) => s.ext_id === extId && (WITHDRAWABLE as readonly string[]).includes(s.status)) ?? null
})

/** 阻塞原因（null = 可以提交） */
const blockReason = computed<{ kind: 'reviewing' | 'gate_failed' | 'quota'; text: string; hint: string } | null>(
  () => {
    const s = blockingSubmission.value
    if (s) {
      if (s.status === 'gate_failed') {
        return {
          kind: 'gate_failed',
          text: `v${s.version} 上次提交未过关卡，还没有走完流程`,
          hint: '改完源码后重新提交即可；若不想再交这一版，先在下方「我的提交」里撤回它。',
        }
      }
      return {
        kind: 'reviewing',
        text: `v${s.version} 正在审核中`,
        hint: '同一扩展同时只允许一个待审版本：等审核通过或驳回后再提交新版本；若要现在就改，先在下方「我的提交」里撤回它。',
      }
    }
    // 配额：只判「剩余为 0」——服务端只回剩余次数、不回上限（PRD 附录 A），且官方账号不受配额约束，
    // 所以这里只做能确定的拦截，其余情形交给服务端报错兜底。
    if (quota.value?.drafts_remaining === 0) {
      return {
        kind: 'quota',
        text: '账号的「待处理」额度已用完',
        hint: '先在下方撤回不需要的提交，或等审核结果出来后再提交。',
      }
    }
    if (quota.value?.daily_submits_remaining === 0) {
      return { kind: 'quota', text: '今日提交次数已用完', hint: '明天再来，或先在下方撤回不需要的提交。' }
    }
    return null
  },
)

/** 提交前的最后一道闸：校验刚拉到的配额，避免信息过期后仍然白等一轮打包上传 */
function checkSubmittable(): boolean {
  if (blockReason.value) {
    const r = blockReason.value
    errorText.value = `${r.text}。${r.hint}`
    return false
  }
  return true
}

// ---- 发布版本：写回 manifest 的本地校验（服务端关卡才是最终结论，这里只挡明显的误填） ----

/** x.y.z 三段纯数字（与服务端关卡同口径，1.0.0-beta / 1.2 不合法）；返回 null = 不合法 */
function parseXyz(v: string): [number, number, number] | null {
  if (!/^\d+\.\d+\.\d+$/.test(v)) return null
  const parts = v.split('.')
  // 拒绝前导零（01.2.3），与 Rust 端 semver 解析口径一致
  if (parts.some((p) => p.length > 1 && p.startsWith('0'))) return null
  return [Number(parts[0]), Number(parts[1]), Number(parts[2])]
}

/** 建议的下一版本（补丁号 +1）；当前版本不是规范 x.y.z 时返回空（留空按当前版本发布） */
function suggestNextVersion(v: string): string {
  const p = parseXyz(v)
  return p ? `${p[0]}.${p[1]}.${p[2] + 1}` : ''
}

/**
 * 某版本是否正挂在市场上（该扩展有这条提交：已上架且未被撤销）。
 * 决定「发布版本」的默认建议与校验口径：在架上 → 必须升号（服务端递增关卡）；不在架上 → 允许原号重发。
 */
function isVersionLive(v: string): boolean {
  const extId = props.extension?.id
  if (!extId) return false
  return extSubmissions.value.some(
    (s) => s.status === 'published' && s.version === v && !revokedKeys.value.includes(`${extId}@${v}`),
  )
}

/** 错误文案；null = 通过（含留空：留空是合法路径，按 manifest 当前版本发布） */
function validateNewVersion(): string | null {
  const v = newVersion.value.trim()
  if (!v) return null
  const p = parseXyz(v)
  if (!p) return `版本号必须是 x.y.z 三段纯数字（如 0.2.1），当前填的是「${v}」`
  const c = parseXyz(currentVersion.value)
  if (c) {
    if (p[0] === c[0] && p[1] === c[1] && p[2] === c[2]) {
      // 与 manifest 当前版本相同：只有「正在架上」才必须升号（服务端递增关卡会拒）；
      // 未上架的（首发 / 审核中撤回 / 被驳回 / 关卡未过 / 已撤销）允许原号重发——市场没有（或已收回）这个版本
      if (isVersionLive(currentVersion.value)) {
        return `v${v} 正在市场上架，新版本号必须高于它（留空则按当前版本原样重新发布）`
      }
      return null
    }
    const greater =
      p[0] > c[0] ||
      (p[0] === c[0] && p[1] > c[1]) ||
      (p[0] === c[0] && p[1] === c[1] && p[2] > c[2])
    if (!greater) return `新版本 ${v} 必须大于当前 manifest 版本 ${currentVersion.value}`
  }
  return null
}

/** 用户是否手动改过「发布版本」：预填只跟随默认值到用户动手为止，别覆盖输入 */
const versionTouched = ref(false)

/**
 * 「发布版本」默认值：当前版本在架上 → 升一版（补丁号 +1，服务端要求递增）；
 * 不在架上（首发 / 撤回 / 驳回 / 关卡未过 / 已撤销）→ 原号——市场没有这个版本号，没必要强迫叠加。
 * 提交记录是异步拉取的，这里做成 computed：记录到位后（未动手时）自动校正预填。
 */
const defaultNewVersion = computed(() => {
  const cur = currentVersion.value
  if (!parseXyz(cur)) return ''
  return isVersionLive(cur) ? suggestNextVersion(cur) : cur
})

watch(defaultNewVersion, (v) => {
  if (!versionTouched.value) newVersion.value = v
})

// 「我的提交」里被指引的那一行：短暂高亮（配合说明里的可点行动）
const highlightedId = ref<number | null>(null)
let highlightTimer: number | null = null

/** 说明里的行动指引：滚到「我的提交」里那条阻塞本次提交的记录并高亮它（撤回入口就在那一行） */
function focusBlockingSubmission() {
  const s = blockingSubmission.value
  if (!s) {
    document.getElementById('pub-list-head')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    return
  }
  focusSubmission(s.id)
}

/** 滚到列表里指定那条提交并短暂高亮（配额明细 / 阻塞说明共用） */
function focusSubmission(id: number) {
  // 目标可能在「显示更多」之后才渲染：先确保它落在当前渲染条数内
  const idx = submissions.value.findIndex((s) => s.id === id)
  if (idx >= visibleCount.value) visibleCount.value = idx + 1
  highlightedId.value = id
  if (highlightTimer !== null) window.clearTimeout(highlightTimer)
  highlightTimer = window.setTimeout(() => (highlightedId.value = null), 2600)
  void nextTick(() => {
    const el = document.getElementById(`pub-sub-${id}`)
    if (el) {
      el.scrollIntoView({ behavior: 'smooth', block: 'center' })
      return
    }
    // 兜底：目标行确实不在列表里时，至少落到列表头部并给个交代，别静默无反应
    document.getElementById('pub-list-head')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    errorText.value = '这条提交不在当前列表里（可能是列表未完整加载），点「刷新」重试。'
  })
}

/** 展开/收起配额明细（展开后把面板滚进视野，避免它长在视线之外） */
function toggleDraftDetail() {
  showDraftDetail.value = !showDraftDetail.value
  if (!showDraftDetail.value) return
  void nextTick(() => {
    document.getElementById('pub-draft-detail')?.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
  })
}

const STATUS_TEXT: Record<string, string> = {
  uploaded: '已上传',
  gate_failed: '关卡未过',
  pending_review: '待审核',
  approved: '已通过',
  rejected: '已驳回',
  published: '已上架',
  withdrawn: '已撤回',
  /** 派生状态（非服务端 status）：平台已下架该版本，见 `isDelisted` */
  delisted: '已下架',
}

function fmtTime(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/** 「显示更多」：只放大渲染条数，数据早已全量在手 */
function showMore() {
  visibleCount.value += PAGE_SIZE
}

/**
 * 拉全量提交记录（翻页到底）。
 *
 * 为什么必须全量：阻塞判定与「待处理 N 条」的明细都要看**所有**未走完流程的提交，
 * 而列表首屏只渲染 20 条——只看第 1 页会出现「明明有待审版本却放行」和「数字对不上明细」。
 * 失败时降级为「用已加载的部分」并置 `allLoaded=false`，界面会标注数据不完整。
 */
async function loadAllSubmissions() {
  if (!isTauri()) return
  allLoading.value = true
  allLoaded.value = false
  try {
    const first = await tauriApi.devListSubmissions(1, 100)
    let rows = first.submissions ?? []
    const t = typeof first.total === 'number' ? first.total : rows.length
    quota.value = first.quota ?? quota.value
    let p = 1
    while (rows.length < t && p < 20) {
      p += 1
      const r = await tauriApi.devListSubmissions(p, 100)
      const batch = r.submissions ?? []
      if (!batch.length) break
      rows = [...rows, ...batch]
    }
    submissions.value = rows
    visibleCount.value = PAGE_SIZE
    allLoaded.value = true
  } catch {
    // 全量拉取失败：保留已有数据，`allLoaded=false` 让界面别把明细说得太满
    allLoaded.value = false
  } finally {
    allLoading.value = false
  }
}

/**
 * 同步「已下架」状态：下架动作只写服务端 `revoked` 表 + 重签市场清单，
 * **不会**把 `submissions.status` 从 `published` 改掉（见 x-hub-server `routes.ts::/admin/market/:id/revoke`），
 * 所以作者侧那条记录会一直显示「已上架」——客户端只能拿清单的 `revoked`（`id@version`）自己对照。
 */
async function loadRevokedKeys() {
  if (!isTauri()) return
  const extId = props.extension?.id
  try {
    // 优先拉最新清单；联网失败时回退读本地缓存（Rust 端把验签过的清单缓存到数据根）
    const status = await tauriApi.refreshMarketRegistry().catch(() => tauriApi.getMarketRegistry())
    revokedKeys.value = status?.revoked ?? []
    // 顺手取该扩展当前已上架版本的截图 URL ——「引用上一版截图」的数据源（未上架 = 空，按钮不出现）。
    // 响应回来时可能已切到别的扩展：只在本扩展仍在场时才落位，防串台
    if (props.extension?.id === extId) {
      prevShots.value = status?.extensions?.find((e) => e.id === extId)?.screenshots?.slice() ?? []
    }
  } catch {
    revokedKeys.value = []
  }
}

// 本地预检：让作者在点发布之前就发现问题（口径与服务端关卡一致，但不作为放行依据）
const precheck = ref<PrecheckResult | null>(null)
const prechecking = ref(false)

async function runPrecheck() {
  const ext = props.extension
  if (!ext || !isTauri()) return
  prechecking.value = true
  try {
    precheck.value = await tauriApi.precheckExtension(ext.id)
  } catch {
    precheck.value = null
  } finally {
    prechecking.value = false
  }
}

// ---- 截图（展示物料）：让用户一眼看出扩展是干嘛的 ----
// 数量在这里限制（最多 5 张）；**大小与真实类型由服务端把关**（按文件头判，不信扩展名）。
const MAX_SHOTS = 5
const screenshots = ref<string[]>([])
/** path → data URL（作者选的图不在资产白名单目录，只能读成 base64 预览） */
const shotPreviews = ref<Record<string, string>>({})
/** 该扩展当前已上架版本（= 发布新版本时的「上一版」）在市场清单里的截图 URL */
const prevShots = ref<string[]>([])
/** 正在下载上一版截图（按钮 busy 态） */
const fetchingShots = ref(false)

async function pickScreenshots() {
  if (!isTauri()) return
  try {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
    })
    const paths = Array.isArray(picked) ? picked : typeof picked === 'string' ? [picked] : []
    if (!paths.length) return
    const room = MAX_SHOTS - screenshots.value.length
    if (room <= 0) {
      showToast(`最多 ${MAX_SHOTS} 张截图`)
      return
    }
    if (paths.length > room) showToast(`最多 ${MAX_SHOTS} 张，只取前 ${room} 张`)
    for (const p of paths.slice(0, room)) {
      if (screenshots.value.includes(p)) continue
      screenshots.value.push(p)
      try {
        shotPreviews.value[p] = await tauriApi.readImageDataUrl(p)
      } catch (e) {
        showToast(`该图无法预览：${e}`)
      }
    }
  } catch (e) {
    showToast(String(e))
  }
}

/**
 * 「引用上一版截图」：把市场清单里已上架版本的截图下载成本地临时文件（Rust 按文件头嗅探类型、
 * 限 2MB、内容哈希命名），之后与手选图片走同一条上传链路——服务端截图本就按内容哈希存储，
 * 同图重传不占新空间。接口未变时发布新版本可整组复用，不必重新选图。
 */
async function reusePrevShots() {
  if (!isTauri() || fetchingShots.value) return
  const room = MAX_SHOTS - screenshots.value.length
  if (room <= 0) {
    showToast(`最多 ${MAX_SHOTS} 张截图`)
    return
  }
  fetchingShots.value = true
  try {
    // 空位不够时只取前 room 张（与手选多处同口径）；单张失败由 Rust 跳过并落日志
    const paths = await tauriApi.fetchRemoteScreenshots(prevShots.value.slice(0, room))
    let added = 0
    for (const p of paths) {
      if (screenshots.value.length >= MAX_SHOTS) break
      if (screenshots.value.includes(p)) continue
      screenshots.value.push(p)
      added += 1
      try {
        shotPreviews.value[p] = await tauriApi.readImageDataUrl(p)
      } catch {
        /* 下载侧已验过文件头，预览失败极罕见；模板会给「无法预览」占位 */
      }
    }
    const failed = Math.max(0, Math.min(prevShots.value.length, room) - added)
    showToast(failed ? `已引用 ${added} 张，${failed} 张下载失败` : `已引用上一版 ${added} 张截图`)
  } catch (e) {
    showToast(String(e))
  } finally {
    fetchingShots.value = false
  }
}

function removeShot(path: string) {
  screenshots.value = screenshots.value.filter((p) => p !== path)
  delete shotPreviews.value[path]
}

async function submit() {
  const ext = props.extension
  if (!ext) return
  if (!isTauri()) return
  // 阻塞判定可能基于几分钟前拉到的列表/配额，提交前再跟服务端核一次（失败不拦，交给服务端兜底）
  await loadAllSubmissions()
  if (!checkSubmittable()) return
  // 版本号的本地校验放在配额之后：错误就地显示，别等上传完了才被 Rust 打回
  const versionError = validateNewVersion()
  if (versionError) {
    errorText.value = versionError
    return
  }
  submitting.value = true
  result.value = null
  errorText.value = ''
  try {
    result.value = await tauriApi.devSubmit(
      ext.id,
      changelog.value.trim(),
      minAppVersion.value.trim(),
      homepage.value.trim(),
      screenshots.value,
      newVersion.value.trim() || undefined,
      author.value.trim() || undefined,
    )
    quota.value = result.value.quota ?? quota.value
    // 版本已随提交落盘：就地跟进当前版本（props.extension 是旧值，列表要等 stamp 轮询刷新），
    // 并预填同版本号——刚提交的版本还没上架，「关卡挂了 → 撤回 → 改完源码 → 原号重发」一击直达，不用手填
    currentVersion.value = result.value.version
    versionTouched.value = false
    newVersion.value = result.value.version
    // 提交后刷新：新记录要立刻出现在列表里（它现在也是一条「待处理」）
    await loadAllSubmissions()
  } catch (e) {
    errorText.value = String(e)
  } finally {
    submitting.value = false
  }
}

async function withdraw(row: DevSubmissionRow) {
  try {
    await tauriApi.devWithdrawSubmission(row.id)
    // 撤回后刷新列表与配额：按钮的置灰状态要立刻跟着放开
    await loadAllSubmissions()
  } catch (e) {
    errorText.value = String(e)
  }
}

// 展开某条历史提交的关卡逐项结论（服务端已返回，之前界面没接）
const detailId = ref<number | null>(null)
const detailItems = ref<{ id: string; label: string; ok: boolean; detail?: string | null }[]>([])
const detailLoading = ref(false)

async function toggleDetail(row: DevSubmissionRow) {
  if (detailId.value === row.id) {
    detailId.value = null
    return
  }
  detailId.value = row.id
  detailItems.value = []
  detailLoading.value = true
  try {
    const r = await tauriApi.devGetSubmission(row.id)
    detailItems.value = r.submission.gate_report ?? []
  } catch (e) {
    errorText.value = String(e)
  } finally {
    detailLoading.value = false
  }
}

/** 弹窗打开时的初始化：这些数据（提交记录 / 配额 / 预检 / 已下架清单）是判定与展示的输入，每次打开都刷新 */
function initDialog() {
  result.value = null
  errorText.value = ''
  changelog.value = ''
  homepage.value = ''
  minAppVersion.value = ''
  author.value = props.extension?.author ?? ''
  currentVersion.value = props.extension?.version ?? ''
  // 预填「发布版本」默认值（在架上 → 升一版；不在架上 → 原号重发，见 defaultNewVersion）；
  // 提交记录还在异步拉取，到位后由 watcher 校正（用户已动手则不覆盖）
  versionTouched.value = false
  newVersion.value = defaultNewVersion.value
  precheck.value = null
  showDraftDetail.value = false
  // 截图属于上一个扩展的会话状态：不清空会带到下一个扩展的弹窗里，
  // 且此时提交会把 A 扩展挑的截图挂到 B 扩展名下（组件常驻挂载、只靠 v-if 隐藏）
  screenshots.value = []
  shotPreviews.value = {}
  // 上一版截图同样跟着扩展走；清单在异步拉取，到位后由 loadRevokedKeys 落位
  prevShots.value = []
  detailId.value = null
  submissions.value = []
  visibleCount.value = PAGE_SIZE
  allLoaded.value = false
  void runPrecheck()
  void loadAllSubmissions()
  void loadRevokedKeys()
}

watch(visible, (v) => {
  if (v) {
    initDialog()
  } else {
    // 关窗即收尾：不残留上一次的错误与高亮
    errorText.value = ''
    highlightedId.value = null
    if (highlightTimer !== null) {
      window.clearTimeout(highlightTimer)
      highlightTimer = null
    }
  }
  // ⚠️ 必须 immediate：弹窗组件常在「已经打开」的状态下首次挂载（例如刚打开扩展中心就点发布、
  // 或父组件把它与别的弹窗一起渲染），此时 watch 不会触发 → 提交记录/配额永远为空，
  // 「打包并发布」也就永远不会进入置灰态，用户必须关掉再开一次才正常。
}, { immediate: true })

onBeforeUnmount(() => {
  if (highlightTimer !== null) window.clearTimeout(highlightTimer)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" @click.self="emit('close')">
      <div ref="cardRef" class="modal-card pub-card" role="dialog" aria-label="发布扩展" aria-modal="true">
        <div class="pub-head">
          <div>
            <h2 class="dialog-title">发布「{{ extension?.name }}」</h2>
            <p class="pub-sub">
              {{ extension?.id }} · v{{ currentVersion || extension?.version }} ·
              {{ extension?.source === 'dev' ? '开发中（源码直挂）' : '已安装' }}
            </p>
          </div>
          <button class="icon-btn" type="button" title="关闭" aria-label="关闭" @click="emit('close')">
            ✕
          </button>
        </div>

        <div class="pub-body">
          <p class="pub-hint">
            打包在本机完成（不含 <code>node_modules</code> 与隐藏文件），上传后由平台跑关卡与人工审核。
            「发布版本」「作者署名」会写回扩展的 manifest.json 再打包：正在市场上架的版本必须填更大的号；
            还没上架的（首发、审核中撤回、被驳回、未过关卡）可以填原版本号直接重发，留空则按 manifest 当前版本发布。
          </p>

          <!-- 本地预检：只列需要修的问题（ok 项不刷屏），服务端关卡才是最终结论 -->
          <div v-if="prechecking" class="pub-precheck-loading">正在本地预检…</div>
          <div v-else-if="precheck" class="pub-precheck" :class="{ clean: precheck.clean }">
            <div class="pub-precheck-head">
              {{ precheck.clean ? '本地预检通过（服务端关卡仍是最终结论）' : '本地预检发现需要修的问题' }}
            </div>
            <ul>
              <li
                v-for="i in precheck.items.filter((x) => x.level !== 'ok')"
                :key="i.label"
                :class="i.level"
              >
                <span>{{ i.label }}</span>
                <span v-if="i.detail" class="pub-precheck-detail">{{ i.detail }}</span>
              </li>
              <li v-if="precheck.clean" class="ok">没有发现问题</li>
            </ul>
          </div>

          <label class="pub-field">
            <span>更新说明</span>
            <textarea v-model="changelog" rows="3" placeholder="这一版改了什么（会展示给用户）"></textarea>
          </label>

          <div class="pub-row">
            <label class="pub-field pub-field-sm">
              <span>发布版本</span>
              <input v-model="newVersion" placeholder="留空 = 按当前版本发布" @input="versionTouched = true" />
            </label>
            <label class="pub-field pub-field-sm">
              <span>宿主最低版本</span>
              <input v-model="minAppVersion" placeholder="如 0.5.5" />
            </label>
          </div>
          <div class="pub-row">
            <label class="pub-field pub-field-sm">
              <span>作者署名</span>
              <input
                v-model="author"
                maxlength="60"
                placeholder="显示在市场卡片与详情页"
                title="会写回扩展的 manifest.json（author 字段）"
              />
            </label>
            <label class="pub-field pub-field-sm">
              <span>项目主页</span>
              <input v-model="homepage" placeholder="https://…" />
            </label>
          </div>

          <div class="pub-field">
            <div class="pub-shots-head">
              <span>截图（可选，最多 {{ MAX_SHOTS }} 张 · 单张 ≤ 2MB）</span>
              <button
                v-if="prevShots.length"
                class="pub-shot-reuse"
                type="button"
                :disabled="fetchingShots || screenshots.length >= MAX_SHOTS"
                title="下载市场清单里当前已上架版本的截图，直接作为本版截图；界面没变时不必重新选图"
                @click="reusePrevShots"
              >
                {{ fetchingShots ? '正在引用…' : `引用上一版截图（${prevShots.length} 张）` }}
              </button>
            </div>
            <div class="pub-shots">
              <div v-for="p in screenshots" :key="p" class="pub-shot">
                <img v-if="shotPreviews[p]" :src="shotPreviews[p]" :alt="p" />
                <span v-else class="pub-shot-pending">无法预览</span>
                <button class="pub-shot-del" type="button" title="移除这张" @click="removeShot(p)">
                  <X :size="12" :stroke-width="2.2" />
                </button>
              </div>
              <button
                v-if="screenshots.length < MAX_SHOTS"
                class="pub-shot-add"
                type="button"
                @click="pickScreenshots"
              >
                <ImagePlus :size="18" :stroke-width="1.8" />
                <span>添加图片</span>
              </button>
            </div>
            <p class="pub-hint">
              截图会展示在扩展详情页 —— 建议 1~3 张：主界面 + 典型用法，用户一眼就知道这扩展能干什么
            </p>
          </div>

          <div v-if="errorText" class="pub-error">{{ errorText }}</div>

          <div v-if="result" class="pub-result" :class="{ ok: result.gatePassed }">
            <div class="pub-result-head">
              {{
                result.gatePassed
                  ? '已提交，等待人工审核'
                  : '机器关卡未通过，请按下面提示修改后重新发布'
              }}
            </div>
            <ul class="pub-gate">
              <li v-for="g in result.gateItems" :key="g.id" :class="{ bad: !g.ok }">
                <span class="pub-gate-label">{{ g.label }}</span>
                <span v-if="g.detail" class="pub-gate-detail">{{ g.detail }}</span>
              </li>
            </ul>
          </div>

          <div class="pub-list">
            <div id="pub-list-head" class="pub-list-head">
              <span>
                提交记录
                <span v-if="submissions.length" class="pub-list-count">
                  （账号共 {{ submissions.length }} 条{{ hasMore ? `，显示 ${visibleSubmissions.length} 条` : '' }}）
                </span>
              </span>
              <button class="ghost-btn" type="button" :disabled="allLoading" @click="loadAllSubmissions">
                {{ allLoading ? '刷新中…' : '刷新' }}
              </button>
            </div>
            <p v-if="allLoading && !submissions.length" class="pub-empty">正在加载提交记录…</p>
            <p v-else-if="!submissions.length" class="pub-empty">账号下还没有提交记录</p>
            <p v-else-if="!extSubmissions.length" class="pub-empty">这个扩展还没有提交记录</p>
            <div
              v-for="s in visibleSubmissions"
              :id="`pub-sub-${s.id}`"
              :key="s.id"
              class="pub-item"
              :class="{ 'pub-item-highlight': highlightedId === s.id }"
            >
              <div class="pub-item-main">
                <!-- 别的扩展的记录也列在这里（账号级列表，撤回入口要能点到，见下方配额明细），
                     但用标签明确区分，避免看起来像本扩展的版本 -->
                <span v-if="s.ext_id !== extension?.id" class="pub-item-other">
                  其他扩展 · {{ s.ext_id }}
                </span>
                <span class="pub-item-title"><b>v{{ s.version }}</b></span>
                <span class="pub-item-meta">
                  <span class="pub-tag" :class="`st-${statusKey(s)}`">{{ STATUS_TEXT[statusKey(s)] ?? s.status }}</span>
                  {{ fmtTime(s.created_at) }}
                </span>
                <span v-if="s.review_note" class="pub-item-note">驳回原因：{{ s.review_note }}</span>
                <ul v-if="detailId === s.id" class="pub-item-gate">
                  <li v-if="detailLoading">加载中…</li>
                  <li
                    v-for="g in detailItems"
                    :key="g.id"
                    :class="{ bad: !g.ok }"
                  >
                    <span>{{ g.label }}</span>
                    <span v-if="g.detail" class="pub-gate-detail">{{ g.detail }}</span>
                  </li>
                </ul>
              </div>
              <div class="pub-item-actions">
                <button class="ghost-btn" type="button" @click="toggleDetail(s)">
                  {{ detailId === s.id ? '收起' : '详情' }}
                </button>
                <button
                  v-if="isWithdrawable(s)"
                  class="ghost-btn"
                  type="button"
                  @click="withdraw(s)"
                >
                  撤回
                </button>
              </div>
              <p v-if="isDelisted(s)" class="pub-item-delisted">
                该版本已被平台下架，暂不提供下载与更新。修好问题后可直接用原版本号重新发布（留空「发布版本」重新提审，审核通过即恢复上架），也可以提交新的版本号。
              </p>
            </div>
            <button v-if="hasMore" class="ghost-btn pub-more" type="button" @click="showMore">
              显示更多（还有 {{ extSubmissions.length - visibleSubmissions.length }} 条）
            </button>

            <!-- 提交记录（账号级，含所有扩展）：既占用账号的「待处理」额度，也是撤回入口所在。
                 ⚠️ 文案别只写「待处理 N 条」——那是**占用**数；扩展中心里的「待处理还可 N 条」是**剩余**额度，
                 两个 N 方向相反，必须各自写明「占用 … 额度」/「还可 …」（见约定 61）。 -->
            <p class="pub-draft-hint">
              <span v-if="draftBlockers.length">
                提交记录共 <b>{{ submissions.length }}</b> 条（账号级，含其他扩展）：其中
                <b>{{ draftBlockers.length }}</b> 条未走完流程，占用账号的「待处理」额度
              </span>
              <span v-else>提交记录共 <b>{{ submissions.length }}</b> 条（账号级，含其他扩展）：当前没有未走完流程的</span>
              <button
                v-if="draftBlockers.length"
                class="pub-quota-detail"
                type="button"
                :aria-expanded="showDraftDetail"
                @click="toggleDraftDetail"
              >
                {{ showDraftDetail ? '收起' : '看是哪几条' }}
              </button>
            </p>
            <div v-if="showDraftDetail" id="pub-draft-detail" class="pub-detail">
              <ul class="pub-detail-list">
                <li v-for="b in draftBlockers" :key="b.id">
                  <!-- 本扩展的直接跳过去（撤回入口就在那一行）；别的扩展没有对应行，用展开的「本扩展的提交」过滤 -->
                  <button
                    v-if="b.ext_id === extension?.id"
                    class="pub-detail-item"
                    type="button"
                    @click="focusSubmission(b.id)"
                  >
                    <span class="pub-detail-id">{{ extension?.name || extension?.id }}</span>
                    <span class="pub-detail-ver">v{{ b.version }}</span>
                    <span class="pub-tag" :class="`st-${b.status}`">{{ STATUS_TEXT[b.status] ?? b.status }}</span>
                    <span class="pub-detail-time">{{ fmtTime(b.created_at) }}</span>
                  </button>
                  <span v-else class="pub-detail-item pub-detail-item-static">
                    <span class="pub-detail-id">{{ b.ext_id }}</span>
                    <span class="pub-detail-ver">v{{ b.version }}</span>
                    <span class="pub-tag" :class="`st-${b.status}`">{{ STATUS_TEXT[b.status] ?? b.status }}</span>
                    <span class="pub-detail-time">{{ fmtTime(b.created_at) }}</span>
                  </span>
                </li>
              </ul>
              <p v-if="!allLoaded" class="pub-detail-warn">
                提交记录未完整加载（网络或登录异常），上面的明细可能不全，请点「刷新」重试。
              </p>
            </div>
          </div>
        </div>

        <div class="pub-foot">
          <!-- 提交被拦时就地写明原因 + 去哪解（按钮置灰不能是哑的；见 `blockReason`） -->
          <p v-if="blockReason" class="pub-block" :class="`pub-block-${blockReason.kind}`">
            <Info :size="13" :stroke-width="2" aria-hidden="true" />
            <span>
              <b>{{ blockReason.text }}</b> —— {{ blockReason.hint }}
              <button
                v-if="blockingSubmission"
                class="pub-block-link"
                type="button"
                @click="focusBlockingSubmission"
              >
                定位到这条记录
              </button>
            </span>
          </p>
          <div class="pub-foot-actions">
            <button class="ghost-btn" type="button" @click="emit('close')">关闭</button>
            <button
              class="pill-btn"
              type="button"
              :disabled="submitting || !!blockReason"
              :title="blockReason ? `${blockReason.text}：${blockReason.hint}` : ''"
              @click="submit"
            >
              {{ submitting ? '正在打包上传…' : '打包并发布' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.pub-card {
  width: min(620px, 92vw);
  max-height: 86vh;
  display: flex;
  flex-direction: column;
  /* 弹窗外壳自带 24px padding；本卡片由头/体/脚各自排内边距，清零后与扩展设置弹窗、
     市场详情弹窗的 18px 对齐（同一个扩展的四处弹窗内边距必须一致） */
  padding: 0;
}
.pub-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px 0;
}
.pub-sub {
  margin: 3px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
.pub-body {
  padding: 14px 18px 0;
  overflow: auto;
  flex: 1;
}
.pub-hint {
  margin: 0 0 12px;
  font-size: 0.75rem;
  line-height: 1.7;
  color: var(--text-3);
}
.pub-hint code {
  font-size: 0.72rem;
  padding: 1px 4px;
  border-radius: 4px;
  background: var(--bg-card-soft);
}
.pub-field {
  display: block;
  margin-bottom: 10px;
}
.pub-field > span {
  display: block;
  margin-bottom: 4px;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
}
.pub-field textarea,
.pub-field input {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 9px;
  font-size: 0.78rem;
  line-height: 1.6;
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-family: inherit;
  resize: vertical;
}
.pub-row {
  display: flex;
  gap: 10px;
}
.pub-field-sm {
  flex: 1;
  min-width: 0;
}
.pub-quota-detail {
  padding: 1px 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-size: 0.69rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out, border-color 150ms ease-out;
}
.pub-quota-detail:hover {
  background: var(--brand-50);
  border-color: var(--brand-500);
  color: var(--brand-500);
}
/* 配额明细：占用「待处理」名额的提交逐条列出，点一条即跳到「我的提交」里那一行 */
.pub-detail {
  margin: 0 0 10px;
  padding: 9px 11px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--bg-card-soft);
}
.pub-detail-head {
  margin: 0 0 6px;
  font-size: 0.71rem;
  line-height: 1.6;
  color: var(--text-3);
}
.pub-detail-list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 168px;
  overflow-y: auto;
}
.pub-detail-item {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 4px 6px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-1);
  font-size: 0.72rem;
  text-align: left;
  cursor: pointer;
  transition: background 150ms ease-out;
}
.pub-detail-item:hover {
  background: var(--brand-50);
}
.pub-detail-id {
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.69rem;
  color: var(--text-2);
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  max-width: 46%;
}
.pub-detail-ver {
  font-weight: 650;
  flex-shrink: 0;
}
.pub-detail-time {
  margin-left: auto;
  flex-shrink: 0;
  font-size: 0.67rem;
  color: var(--text-3);
}
.pub-detail-warn {
  margin: 6px 0 0;
  font-size: 0.69rem;
  line-height: 1.5;
  color: var(--c-orange-ink);
}
.pub-list-count {
  font-weight: 400;
  color: var(--text-3);
}
/* 别的扩展的记录：淡色标签，避免被误认成本扩展的版本 */
.pub-item-other {
  display: inline-block;
  margin-bottom: 2px;
  padding: 0 6px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-3);
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.65rem;
}
/* 待处理明细入口（配额行搬到扩展管理页后，明细留在提交列表下方） */
.pub-draft-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin: 10px 0 0;
  font-size: 0.71rem;
  line-height: 1.6;
  color: var(--text-3);
}
.pub-draft-hint b {
  color: var(--text-1);
  font-weight: 650;
}
.pub-detail-item-static {
  cursor: default;
}
.pub-detail-item-static:hover {
  background: transparent;
}
.pub-more {
  width: 100%;
  margin-top: 8px;
  justify-content: center;
}
/* 已下架：服务端 status 仍是 published，客户端按清单 revoked 派生（见 isDelisted） */
.pub-item-delisted {
  flex-basis: 100%;
  margin: 4px 0 0;
  font-size: 0.69rem;
  line-height: 1.5;
  color: var(--c-orange-ink);
}
.pub-tag.st-delisted {
  background: var(--c-gray-soft, var(--bg-card-soft));
  color: var(--text-3);
}
.pub-precheck-loading {
  margin: 4px 0 10px;
  font-size: 0.74rem;
  color: var(--text-3);
}
.pub-precheck {
  margin: 4px 0 12px;
  padding: 9px 11px;
  border-radius: 10px;
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.pub-precheck.clean {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
.pub-precheck-head {
  font-size: 0.76rem;
  font-weight: 600;
  margin-bottom: 4px;
}
.pub-precheck ul {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.pub-precheck li {
  font-size: 0.73rem;
  line-height: 1.5;
}
.pub-precheck li.error {
  color: var(--c-red-ink);
}
.pub-precheck-detail {
  display: block;
  font-size: 0.7rem;
  opacity: 0.85;
}
.pub-error {
  margin: 6px 0 10px;
  padding: 8px 10px;
  border-radius: 8px;
  font-size: 0.75rem;
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.pub-result {
  margin: 6px 0 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: var(--bg-card-soft);
}
.pub-result.ok {
  background: var(--c-green-soft);
}
.pub-result-head {
  font-size: 0.78rem;
  font-weight: 600;
  margin-bottom: 6px;
}
.pub-gate {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.pub-gate li {
  position: relative;
  padding-left: 16px;
  font-size: 0.74rem;
  color: var(--text-2);
}
.pub-gate li::before {
  content: '✓';
  position: absolute;
  left: 0;
  color: var(--c-green-ink);
}
.pub-gate li.bad {
  color: var(--c-red-ink);
}
.pub-gate li.bad::before {
  content: '✕';
  color: var(--c-red-ink);
}
.pub-gate-detail {
  display: block;
  font-size: 0.7rem;
  color: var(--text-3);
}
.pub-list {
  margin: 8px 0 4px;
}
.pub-list-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--text-2);
  margin-bottom: 6px;
}
.pub-empty {
  margin: 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
.pub-item {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding: 7px 0;
  border-top: 1px solid var(--border-soft);
}
.pub-item-main {
  flex: 1;
  min-width: 0;
}
.pub-item-title {
  display: block;
  font-size: 0.76rem;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pub-item-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.7rem;
  color: var(--text-3);
}
.pub-item-note {
  display: block;
  margin-top: 2px;
  font-size: 0.72rem;
  color: var(--c-red-ink);
}
.pub-item-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.pub-item-gate {
  margin: 6px 0 2px;
  padding: 8px 10px;
  list-style: none;
  border-radius: 8px;
  background: var(--bg-card-soft);
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.pub-item-gate li {
  position: relative;
  padding-left: 15px;
  font-size: 0.72rem;
  line-height: 1.5;
  color: var(--text-2);
}
.pub-item-gate li::before {
  content: '✓';
  position: absolute;
  left: 0;
  color: var(--c-green-ink);
}
.pub-item-gate li.bad {
  color: var(--c-red-ink);
}
.pub-item-gate li.bad::before {
  content: '✕';
  color: var(--c-red-ink);
}
.pub-tag {
  padding: 0 6px;
  border-radius: 999px;
  background: var(--bg-card-soft);
  font-weight: 600;
}
.pub-tag.st-published {
  background: var(--c-green-soft);
  color: var(--c-green-ink);
}
/* 截图：标签行（含「引用上一版截图」入口）+ 缩略图条 + 虚线「添加图片」块 */
.pub-shots-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 4px;
}
.pub-shots-head > span {
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
}
/* 引用上一版截图：小药丸钮（样式口径同 .pub-quota-detail） */
.pub-shot-reuse {
  flex-shrink: 0;
  padding: 2px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-size: 0.69rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out, border-color 150ms ease-out;
}
.pub-shot-reuse:hover:not(:disabled) {
  background: var(--brand-50);
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.pub-shot-reuse:disabled {
  opacity: 0.55;
  cursor: default;
}
.pub-shots {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.pub-shot {
  position: relative;
  width: 128px;
  height: 76px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-card-soft);
}
.pub-shot img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.pub-shot-pending {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 0.68rem;
  color: var(--text-3);
}
.pub-shot-del {
  position: absolute;
  top: 4px;
  right: 4px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: var(--scrim);
  color: #fff;
  cursor: pointer;
}
.pub-shot-del:hover {
  background: var(--c-red-ink);
}
.pub-shot-add {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  width: 128px;
  height: 76px;
  border: 1px dashed var(--border-soft);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  font-size: 0.7rem;
  cursor: pointer;
  transition: color 150ms ease-out, border-color 150ms ease-out;
}
.pub-shot-add:hover {
  color: var(--brand-500);
  border-color: var(--brand-500);
}
.pub-hint {
  margin: 6px 0 0;
  font-size: 0.68rem;
  line-height: 1.5;
  color: var(--text-3);
}
.pub-tag.st-pending_review {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.pub-tag.st-gate_failed,
.pub-tag.st-rejected {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.pub-foot {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 18px;
  border-top: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.pub-foot-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
/* 提交被拦的原因说明：常驻在按钮旁（不能只靠置灰或点击后的 toast） */
.pub-block {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin: 0;
  padding: 8px 10px;
  border-radius: 8px;
  font-size: 0.74rem;
  line-height: 1.6;
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.pub-block svg {
  flex-shrink: 0;
  margin-top: 2px;
}
.pub-block b {
  font-weight: 650;
}
.pub-block-quota {
  background: var(--bg-card-soft);
  color: var(--text-2);
}
.pub-block-link {
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font-size: inherit;
  font-weight: 650;
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}
/* 被「定位到这条记录」指到的行：短暂高亮，撤回入口就在这一行 */
.pub-item-highlight {
  border-radius: 8px;
  background: var(--brand-50);
  box-shadow: inset 0 0 0 1px var(--brand-500);
}
</style>

