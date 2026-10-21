<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { marked, Renderer } from 'marked'
import DOMPurify from 'dompurify'
import { Check, ChevronDown, CircleOff, Copy, MessageSquare, PanelRightClose, Plus, Send, Settings2, Square, X, Zap } from 'lucide-vue-next'
import { PLATFORM_ENTRY_NAME, isPlatformModel, isTauri, tauriApi, type AccountStatus, type AgentApprovalAsk, type ChatMessage, type ChatModelConfig, type ChatSession, type ChatStreamEvent } from '../api/tauri'
import AppSelect from './AppSelect.vue'

const props = defineProps<{
  side?: 'left' | 'right' | 'top' | 'bottom'
  /** dock = 主窗内嵌抽屉（默认）；window = 独立窗口内嵌（尺寸交给窗口缩放、关闭按钮语义变化） */
  mode?: 'dock' | 'window'
}>()

const emit = defineEmits<{
  (e: 'toggle'): void
  (e: 'open-model-settings'): void
  (e: 'open-account-settings'): void
  (e: 'resized', width: number, height: number): void
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

// 独立窗口形态：不调 getChatPanel/setChatPanel（那两个只管主窗抽屉尺寸），
// 拖拽手柄隐藏（缩放交给窗口边缘），头部收起钮变成关闭钮
const isWindow = computed(() => props.mode === 'window')

// ---- 状态 ----
const sessions = ref<ChatSession[]>([])
const activeSessionId = ref<number | null>(null)
const messages = ref<ChatMessage[]>([])
const input = ref('')
const sending = ref(false)
const streamingContent = ref('')
// 流式渲染节流产物：每次增量不再全量重解析 markdown，按 ~80ms 节流刷新，
// 避免长回复 O(n²) 的 marked 重解析与整块 v-html 替换把主线程卡死、反压流式推送
const streamHtml = ref('')
const streamError = ref('')
const models = ref<ChatModelConfig[]>([])
const selectedModel = ref('')
const bodyEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLTextAreaElement | null>(null)
// 内容区不在底部时显示「跳到底部」按钮
const showJumpBtn = ref(false)
// 新建 / 切换对话下拉菜单
const menuOpen = ref(false)
const menuPos = ref({ x: 0, y: 0, width: 0, openUp: false })
const addBtnRef = ref<HTMLButtonElement | null>(null)
const menuRef = ref<HTMLElement | null>(null)

// ---- Agent 时间线（#t46）：工具卡片 / 内联审批 / 中断 ----
/** 工具卡片：一个工具调用从发起（running/awaiting）到结束（ok/error/denied）的实时状态。
 *  turnId 记录该调用所属回合：审批答复失败时可精准中断那一回合（0.9.0 修复）。 */
interface ToolCard {
  callId: string
  name: string
  args: string
  /** 危险等级；'' = 历史还原卡无从得知（落库消息不带 level），显示中性灰点 */
  level: 'read' | 'write' | 'dangerous' | ''
  status: 'running' | 'awaiting' | 'ok' | 'error' | 'denied'
  turnId: string
  summary: string
  detail: string
  detailOpen: boolean
}
/** 中间过程文本块：模型在发起工具调用前后的叙述（0.9.0 起保留），默认折叠省空间 */
interface TextStep {
  kind: 'text'
  key: string
  /** 已渲染的 Markdown HTML（收进块时渲染一次） */
  html: string
  open: boolean
}
/** 时间线流程项：叙述文本与工具卡片按发生顺序混排（0.9.0 起文本保留为可折叠块） */
type TurnItem = ({ kind: 'card'; key: string } & ToolCard) | TextStep
const turnFlow = ref<TurnItem[]>([])
let stepSeq = 0

function findCard(callId: string): (ToolCard & { kind: 'card'; key: string }) | undefined {
  const it = turnFlow.value.find((i) => i.kind === 'card' && i.callId === callId)
  return it as (ToolCard & { kind: 'card'; key: string }) | undefined
}

/** 追加一张工具卡片（去重以支持历史还原与实时事件重叠） */
function pushCard(c: Omit<ToolCard, 'detailOpen'>) {
  if (findCard(c.callId)) return
  // 危险操作的确认卡默认展开参数：只显示工具名就让用户点「允许」等于盲批（0.9.0 审查修复）
  turnFlow.value.push({
    kind: 'card',
    key: `card_${c.callId}`,
    detailOpen: c.status === 'awaiting' && c.level === 'dangerous',
    ...c,
  })
}

/** 模型产出的参数 JSON 原文 → 展示用（能格式化就缩进，坏 JSON 原样返回） */
function prettyArgs(args: string): string {
  if (!args.trim()) return ''
  try {
    return JSON.stringify(JSON.parse(args), null, 2)
  } catch {
    return args
  }
}

/** 把当前流式缓冲里的叙述文本收进可折叠的中间过程块。
 *  在回合边界（工具调用/审批到达）与出错收尾时调用：文字保留可见但不占地方。 */
function flushStreamToStep() {
  const text = streamingContent.value.trim()
  streamingContent.value = ''
  cancelStreamRender()
  streamHtml.value = ''
  if (text) {
    turnFlow.value.push({ kind: 'text', key: `step_${++stepSeq}`, html: renderMd(text), open: false })
  }
}

/** 进行中回合的 turn_id（中断按钮用） */
const activeTurnId = ref('')
/** 权限档快捷切换（§5：像模型选择器一样随时切）。初始值 = 设置里的持久默认档（loadDefaultPerm） */
const permMode = ref<'readonly' | 'ask' | 'full'>('ask')
const permOptions = [
  { value: 'readonly', label: '仅可查看' },
  { value: 'ask', label: '每次都问' },
  { value: 'full', label: '完全权限' },
] as const
const permLabel = computed(() => permOptions.find((o) => o.value === permMode.value)?.label ?? '')

// ---- 平台积分（#t47）：头部剩余积分 + 429 积分不足分流 ----
/** 账号完整状态（含积分账本）；loadModels 时随登录态一并刷新 */
const account = ref<AccountStatus | null>(null)
const creditRemaining = computed(() =>
  account.value?.loggedIn && !account.value.error && platformEnabled.value ? account.value.creditRemaining : null,
)
/** 平台 429 insufficient_credits 的稳定 token（chat.rs 生成，前端据此渲染分流卡片而非裸报错） */
const INSUFFICIENT_TOKEN = 'insufficient_credits'
const isInsufficientCredits = computed(() => streamError.value.includes(INSUFFICIENT_TOKEN))

function levelClass(l: string): string {
  return l === 'dangerous' ? 'lv-danger' : l === 'write' ? 'lv-write' : l === 'read' ? 'lv-read' : ''
}
function statusText(s: ToolCard['status']): string {
  return s === 'running' ? '执行中' : s === 'awaiting' ? '等待确认' : s === 'ok' ? '完成' : s === 'denied' ? '已拒绝' : '失败'
}
/** 审批三按钮（§5：允许 / 拒绝 / 本会话允许），回答后卡片回到执行中。
 * 0.9.0 修复：答复落空（确认已被处理/已失效，如回合已被停止）时把该卡标记失败
 * 并顺手中断其所属回合——后端等待审批的循环会立刻收尾，不再永久挂起。 */
async function respondApproval(card: ToolCard, answer: 'once' | 'always' | 'deny') {
  card.status = 'running'
  try {
    await tauriApi.respondAgentApproval(card.turnId ?? '', card.callId, answer)
  } catch (err) {
    card.status = 'error'
    showToast(String(err))
    if (card.turnId) {
      tauriApi.cancelAgentTurn(card.turnId).catch(() => {})
    }
  }
}
/** 中断当前回合：已执行的操作不回滚（后端口径），后端收尾后回 TurnDone/Error */
async function stopTurn() {
  if (!activeTurnId.value) return
  try {
    await tauriApi.cancelAgentTurn(activeTurnId.value)
    showToast('已请求中断，正在收尾…')
  } catch (err) {
    showToast(String(err))
  }
}

/** 重开会话时，把最近一个 turn 落库的工具过程还原成卡片（只读态，可展开看结果全文）。
 *  更早 turn 的工具行不还原；各 turn 的中间叙述由消息列表里的折叠块呈现（0.9.0）。 */
function restoreToolCards(msgs: ChatMessage[]) {
  const lastTurn = msgs.length ? msgs[msgs.length - 1].turn_id : null
  if (!lastTurn) return
  for (const m of msgs) {
    if (m.role !== 'tool' || m.turn_id !== lastTurn) continue
    const cancelled = m.tool_status === 'cancelled'
    const status = m.tool_status === 'ok' ? 'ok' : m.tool_status === 'denied' || cancelled ? 'denied' : 'error'
    pushCard({
      callId: m.tool_call_id ?? `restored_${m.id}`,
      name: m.tool_name ?? 'tool',
      args: '',
      // 落库消息不带等级，未知时显示中性灰点——不该把历史里执行过的写/危险操作
      // 一律伪装成绿色只读级（0.9.0 审查优化）
      level: '',
      status,
      turnId: m.turn_id ?? '',
      summary: status === 'ok' ? '已执行（历史回合）' : cancelled ? '已中断（历史回合）' : status === 'denied' ? '已拒绝（历史回合）' : '执行失败（历史回合）',
      detail: m.content,
    })
  }
}

/** 还原仍在等待确认的审批（0.9.0）：挂起审批不落库，面板重开/切回会话后原先
 *  只剩「停止」自救；现在向后端查询该会话的挂起审批，恢复可操作的 awaiting 卡片。
 *  若回合仍在跑，同时恢复 sending/activeTurnId，让流式与停止键接得上。 */
async function restorePendingApprovals(sessionId: number) {
  let pending: AgentApprovalAsk[] = []
  try {
    pending = await tauriApi.listAgentApprovals(sessionId)
  } catch {
    return
  }
  // 慢响应期间用户可能已切到别的会话：旧会话的挂起卡片不能混进新会话的时间线（0.9.0 审查修复）
  if (activeSessionId.value !== sessionId) return
  for (const p of pending) {
    pushCard({
      callId: p.callId,
      name: p.name,
      args: p.args,
      level: (p.level as ToolCard['level']) || 'write',
      status: 'awaiting',
      turnId: p.turnId,
      summary: '',
      detail: '',
    })
  }
  if (pending.length) {
    sending.value = true
    if (!activeTurnId.value) activeTurnId.value = pending[0].turnId
  }
}

const activeSession = computed(() => sessions.value.find((s) => s.id === activeSessionId.value) ?? null)
// 顶部直接显示当前对话标题（未命名时为「新对话」）
const currentTitle = computed(() => activeSession.value?.title || '新对话')

// 平台额度在对话里是**一个入口**：模型下拉只显示「x-hub 平台」，不逐个列出平台的具体模型
// ——具体用哪个由后端在多个平台模型之间负载切换（见 src-tauri commands::pick_chat_model）。
// 自备供应商照旧：按供应商分组逐个列出，用户选哪个就是哪个。
//（入口名与条目判定的单一来源在 api/tauri.ts，与 AiProviders 共用）

// 平台入口的真实凭据是**账号登录态**：开关开了但没登录时同样发不出去，这里提前把状态查出来
// （挂载与每次唤起都会随 loadModels 重查），让绿点/发送提示在发送前就说清原因
const accountLoggedIn = ref(false)

const platformEnabled = computed(() => models.value.some((m) => isPlatformModel(m)))

const modelOptions = computed(() => {
  const opts: { value: string; label: string; group: string }[] = []
  if (platformEnabled.value) {
    // 不设分组名：平台只有一个入口项，再套一层同名组头就成了「x-hub 平台 / x-hub 平台」两行
    opts.push({ value: PLATFORM_ENTRY_NAME, label: PLATFORM_ENTRY_NAME, group: '' })
  }
  for (const m of models.value) {
    if (isPlatformModel(m)) continue
    opts.push({ value: m.name, label: modelLabel(m), group: modelGroup(m) })
  }
  return opts
})
const hasModels = computed(() => modelOptions.value.length > 0)

// 会话里存的模型名 → 下拉显示值：平台的具体模型名（历史会话）统一归到平台入口
function displayModelName(name: string): string {
  const m = models.value.find((x) => x.name === name)
  return m && isPlatformModel(m) ? PLATFORM_ENTRY_NAME : name
}

// 默认选中项：默认模型是平台条目（或只有平台条目）时用平台入口
function defaultModelName(): string {
  const def = models.value.find((m) => m.is_default)
  if (def) return displayModelName(def.name)
  return modelOptions.value[0]?.value ?? ''
}

// 当前选中项是否已具备可用凭据：平台入口的凭据是账号登录态（开关开且已登录才可用）
const selectedReady = computed(() => {
  if (selectedModel.value === PLATFORM_ENTRY_NAME) {
    return platformEnabled.value && accountLoggedIn.value
  }
  return !!models.value.find((m) => m.name === selectedModel.value)?.has_api_key
})

// 绿点不亮时的一句缘由（悬停可见），把「为什么发不了」说在发送之前
const readyHint = computed(() => {
  if (selectedReady.value) return '模型就绪'
  if (selectedModel.value === PLATFORM_ENTRY_NAME) {
    if (!platformEnabled.value) return '平台额度未开启（设置 → AI 助手）'
    return '平台额度需登录账号（设置 → 账号）'
  }
  return '当前模型未配置 API Key（设置 → AI 助手）'
})

// 去掉小数尾部的 ".0"（如 1.0 → 1）
function trimZero(s: string): string {
  return s.endsWith('.0') ? s.slice(0, -2) : s
}

// token 数量格式化：≥1000 用 k，舍入后跨过 1000k（如 999500~999999）自动晋升 M，
// 否则原值（统计行三项共用；速度项单独带 tok/s）
function fmtTokens(n: number): string {
  if (n < 1000) return String(n)
  const k = trimZero((n / 1000).toFixed(1))
  if (parseFloat(k) < 1000) return k + 'k'
  return trimZero((n / 1_000_000).toFixed(1)) + 'M'
}

// 缓存率 = 缓存读取 / 输入
const cacheRate = computed(() => {
  const s = activeSession.value
  const input = s?.tokens_input ?? 0
  const cache = s?.tokens_cache_read ?? 0
  if (!input) return 0
  return Math.round((cache / input) * 100)
})

// TPS = 累计输出 token / 累计生成秒数
const tps = computed(() => {
  const s = activeSession.value
  const out = s?.tokens_output ?? 0
  const ms = s?.elapsed_ms ?? 0
  if (!ms || !out) return 0
  return out / (ms / 1000)
})

// 选中后界面只展示大模型名称，不再展示供应商
function modelLabel(m: ChatModelConfig): string {
  return (m.model ?? '').trim() || m.name
}

// 下拉弹出层按供应商分组展示：上面是供应商标题，下面是该供应商的大模型
function modelGroup(m: ChatModelConfig): string {
  return (m.provider_name ?? '').trim() || (m.base_url ?? '').trim() || '其他'
}

// ---- 会话 ----
async function loadSessions() {
  if (!isTauri()) return
  sessions.value = await tauriApi.listChatSessions()
  if (!activeSessionId.value && sessions.value.length > 0) {
    await openSession(sessions.value[0].id)
  }
}

async function openSession(id: number) {
  activeSessionId.value = id
  streamingContent.value = ''
  streamError.value = ''
  // 切换会话时清掉上一会话的渲染缓存，避免缓存随会话数量无限累积
  mdCache.clear()
  if (!isTauri()) return
  const [msgs, s] = await Promise.all([
    tauriApi.listChatMessages(id),
    tauriApi.listChatSessions().then((l) => l.find((x) => x.id === id) ?? null),
  ])
  messages.value = msgs
  // 新会话的时间线从库里重建（历史卡片 + 挂起审批），不带上一会话的实时状态。
  // activeTurnId 不清：切走的会话若回合仍在跑，回来后停止键要接得上
  turnFlow.value = []
  restoreToolCards(msgs)
  await restorePendingApprovals(id)
  if (s) {
    selectedModel.value = displayModelName(s.model_name)
    // 同步本地会话条目（标题 / token 等可能已变化）
    const i = sessions.value.findIndex((x) => x.id === id)
    if (i >= 0) sessions.value = sessions.value.map((x) => (x.id === id ? s : x))
  }
  // 打开会话强制定位到最新消息（非 force 模式会因 scrollTop=0 误判「不在底部」而停在第一句）
  scrollToBottom(true)
  await commitSessionModel()
}

/// 把界面当前选中的模型写回当前会话。
///
/// **发送时后端只认会话里存的 model_name**（`send_chat_message` 按它解析模型配置），
/// 所以显示归一之后必须落库，否则会出现「界面显示 A、实际按 B 发」的错位：最典型的
/// 就是关掉平台额度后，老会话存的「x-hub 平台」已不可用，界面看着是别的模型、
/// 一发却报「平台额度未开启」。
async function commitSessionModel() {
  if (!isTauri() || !selectedModel.value) return
  const s = sessions.value.find((x) => x.id === activeSessionId.value)
  if (!s || s.model_name === selectedModel.value) return
  await tauriApi.setChatSessionModel(s.id, selectedModel.value)
  s.model_name = selectedModel.value
}

async function createSession() {
  if (!isTauri()) return
  const s = await tauriApi.createChatSession({ modelName: selectedModel.value || undefined })
  sessions.value.unshift(s)
  await openSession(s.id)
}

async function deleteSession(id: number) {
  if (!isTauri()) return
  await tauriApi.deleteChatSession(id)
  sessions.value = sessions.value.filter((x) => x.id !== id)
  if (activeSessionId.value === id) {
    activeSessionId.value = null
    messages.value = []
    streamingContent.value = ''
    if (sessions.value.length > 0) await openSession(sessions.value[0].id)
  }
}

// ---- 新建 / 切换对话下拉菜单 ----
function toggleMenu() {
  if (!menuOpen.value) {
    menuOpen.value = true
    void nextTick().then(positionMenu)
  } else {
    menuOpen.value = false
  }
}

function positionMenu() {
  const trigger = addBtnRef.value
  const menu = menuRef.value
  if (!trigger || !menu) return
  const tr = trigger.getBoundingClientRect()
  const gap = 6
  const width = 140
  // 加号在右侧：菜单右对齐到加号，向左展开，避免超出屏幕右缘
  const x = Math.max(8, Math.min(tr.right - width, window.innerWidth - width - 8))
  const spaceBelow = window.innerHeight - tr.bottom - 8
  const spaceAbove = tr.top - 8
  const menuHeight = menu.offsetHeight
  let y: number
  let openUp = false
  if (spaceBelow >= menuHeight + gap) {
    y = tr.bottom + gap
  } else if (spaceAbove >= menuHeight + gap) {
    y = Math.max(8, tr.top - menuHeight - gap)
    openUp = true
  } else {
    y = Math.max(8, Math.min(tr.bottom + gap, window.innerHeight - menuHeight - 8))
  }
  menuPos.value = { x, y, width, openUp }
}

function onPickSession(id: number) {
  menuOpen.value = false
  void openSession(id)
}

async function onNewSession() {
  menuOpen.value = false
  await createSession()
}

function onWindowClick(e: MouseEvent) {
  if (!menuOpen.value) return
  const t = e.target as HTMLElement
  if (addBtnRef.value?.contains(t) || menuRef.value?.contains(t)) return
  menuOpen.value = false
}

function onWindowKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && menuOpen.value) menuOpen.value = false
}

function onWindowResize() {
  if (menuOpen.value) positionMenu()
}

// ---- 模型 ----
async function loadModels() {
  if (!isTauri()) return
  models.value = await tauriApi.getChatModels()
  // 平台入口的凭据是账号登录态，一并刷新（登录/退出只会发生在设置页，面板下次唤起时自然重查）
  // 完整状态留存（含积分账本），头部剩余积分用（#t47）
  account.value = await tauriApi.accountStatus()
  accountLoggedIn.value = account.value.loggedIn
  await loadDefaultPerm()
  // 归一当前选中值：历史会话存的可能是某个具体平台模型名（平台已折叠成入口）、
  // 也可能是已被移除的自备模型 —— 都回落到默认项
  const cur = displayModelName(selectedModel.value)
  selectedModel.value = modelOptions.value.some((o) => o.value === cur) ? cur : defaultModelName()
  await commitSessionModel()
}

/** 权限档快捷切换的初始值 = 设置里的持久默认档（#t47）。
 *  refreshModels（模型配置保存后）也会重走这里：设置改了默认档，面板即刻对齐 */
async function loadDefaultPerm() {
  if (!isTauri()) return
  try {
    const cfg = await tauriApi.getUiConfig()
    if (cfg.agent_permission_mode === 'readonly' || cfg.agent_permission_mode === 'full') {
      permMode.value = cfg.agent_permission_mode
    } else {
      permMode.value = 'ask'
    }
  } catch {
    // 读不到配置就保持当前值/默认 ask，不影响对话
  }
}

/** 平台回合结束后刷新剩余积分（服务端按 usage 结算，本地不推算）；失败静默 */
function refreshCredits() {
  if (!isTauri() || !platformEnabled.value) return
  void tauriApi
    .accountStatus()
    .then((st) => {
      account.value = st
      accountLoggedIn.value = st.loggedIn
    })
    .catch(() => {})
}

async function switchModel(name: string) {
  selectedModel.value = name
  if (!activeSessionId.value || !isTauri()) return
  await tauriApi.setChatSessionModel(activeSessionId.value, name)
  const s = sessions.value.find((x) => x.id === activeSessionId.value)
  if (s) s.model_name = name
}

function openModelSettings() {
  emit('open-model-settings')
}

// ---- 发送 ----
async function send() {
  const content = input.value.trim()
  if (!content || sending.value || !activeSessionId.value || !isTauri()) return
  if (!hasModels.value) {
    showToast('请先配置大模型（设置 → AI 助手）', {
      label: '去配置',
      onClick: openModelSettings,
    })
    return
  }
  if (!selectedReady.value) {
    // 未登录账号要走设置 → 账号，「去配置」跳的是 AI 助手分区，指错了路——这种情况不给按钮
    const needLogin =
      selectedModel.value === PLATFORM_ENTRY_NAME && platformEnabled.value && !accountLoggedIn.value
    showToast(readyHint.value, needLogin ? undefined : { label: '去配置', onClick: openModelSettings })
    return
  }

  // 追加用户消息 + 创建流式回复占位
  const userMsg: ChatMessage = {
    id: Date.now(),
    session_id: activeSessionId.value,
    role: 'user',
    content,
    created_at: new Date().toISOString(),
  }
  messages.value.push(userMsg)
  input.value = ''
  sending.value = true
  streamingContent.value = ''
  streamError.value = ''
  // 新回合：清上一回合的时间线（中间过程块 + 工具卡片）
  turnFlow.value = []
  activeTurnId.value = ''
  // 用户主动发送：强制定位到最新消息，后续流式输出保持跟随
  scrollToBottom(true)

  // 回合所属会话：期间用户切到其它会话时，消息视图的变更全部跳过
  //（回到本会话时 openSession 会从库里重拉），只保留 sending 等回合级状态收尾，
  // 否则旧回合的 TurnDone 会把当前会话的消息列表整个顶掉（0.9.0 修复）
  const turnSessionId = activeSessionId.value

  try {
    await tauriApi.sendAgentMessage(activeSessionId.value, content, permMode.value, (e: ChatStreamEvent) => {
      const cross = activeSessionId.value !== turnSessionId
      if (e.type === 'chunk') {
        if (cross) return
        streamingContent.value += e.content
        scheduleStreamRender()
        scrollToBottom()
      } else if (e.type === 'turnStart') {
        // 回合开工即拿到 turn_id：停止键立刻亮出（不等首个工具事件）
        if (!cross) activeTurnId.value = e.turnId
      } else if (e.type === 'toolCall') {
        if (cross) return
        // 回合边界：把上一段叙述收进可折叠的中间过程块（0.9.0 起保留）
        flushStreamToStep()
        activeTurnId.value = e.turnId
        pushCard({
          callId: e.callId,
          name: e.name,
          args: e.args,
          level: (e.level as ToolCard['level']) || 'read',
          status: 'running',
          turnId: e.turnId,
          summary: '',
          detail: '',
        })
        scrollToBottom()
      } else if (e.type === 'toolResult') {
        if (cross) return
        const card = findCard(e.callId)
        if (card) {
          card.status = e.ok ? 'ok' : 'error'
          card.summary = e.summary
          card.detail = e.detail
        }
        scrollToBottom()
      } else if (e.type === 'approvalRequest') {
        if (cross) return
        flushStreamToStep()
        activeTurnId.value = e.turnId
        pushCard({
          callId: e.callId,
          name: e.name,
          args: e.args,
          level: (e.level as ToolCard['level']) || 'write',
          status: 'awaiting',
          turnId: e.turnId,
          summary: '',
          detail: '',
        })
        scrollToBottom()
      } else if (e.type === 'turnDone') {
        refreshCredits()
        if (!cross) {
          cancelStreamRender()
          streamHtml.value = ''
          // 中间过程留在时间线（折叠态）；最终回复按库里的权威消息追加
          messages.value = [...messages.value, e.message]
          streamingContent.value = ''
        }
        sending.value = false
        activeTurnId.value = ''
        const idx = sessions.value.findIndex((x) => x.id === e.session.id)
        if (idx >= 0) {
          sessions.value = sessions.value
            .map((x) => (x.id === e.session.id ? e.session : x))
            .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
        } else {
          sessions.value.unshift(e.session)
        }
        if (!cross) scrollToBottom(true)
      } else if (e.type === 'done') {
        if (!cross) {
          cancelStreamRender()
          streamHtml.value = ''
          // 用后端落库的权威消息替换占位回复
          const i = messages.value.findIndex((m) => m.id === userMsg.id)
          messages.value = [...messages.value.slice(0, i + 1), e.message]
          streamingContent.value = ''
        }
        sending.value = false
        // 后端返回的会话含自动生成的标题与累计 token，直接替换本地条目
        const idx = sessions.value.findIndex((x) => x.id === e.session.id)
        if (idx >= 0) {
          sessions.value = sessions.value
            .map((x) => (x.id === e.session.id ? e.session : x))
            .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
        } else {
          sessions.value.unshift(e.session)
        }
        if (!cross) scrollToBottom(true)
      } else if (e.type === 'error') {
        if (!cross) {
          cancelStreamRender()
          streamError.value = e.message
          // 已流出的叙述收进中间过程块保留（可展开），不随气泡一起消失
          flushStreamToStep()
          if (e.partial) streamingContent.value = e.partial
          streamHtml.value = renderMd(streamingContent.value)
          scrollToBottom(true)
        }
        sending.value = false
        activeTurnId.value = ''
      }
    })
  } catch (err) {
    cancelStreamRender()
    streamError.value = String(err)
    sending.value = false
  }
}

function scrollToBottom(force = false) {
  void nextTick(() => {
    const el = bodyEl.value
    if (!el) return
    if (force) {
      el.scrollTop = el.scrollHeight
      showJumpBtn.value = false
    } else {
      // 接近底部时才自动跟随，避免用户上翻查看时被打断
      const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80
      if (nearBottom) {
        el.scrollTop = el.scrollHeight
        showJumpBtn.value = false
      }
    }
  })
}

// 用户手动滚动：不在底部（且内容可滚动）时显示跳转按钮
function onBodyScroll() {
  const el = bodyEl.value
  if (!el) return
  showJumpBtn.value = el.scrollHeight - el.scrollTop - el.clientHeight > 40
}

// ---- Markdown 渲染（代码块深色框 + 语言标签 + 复制按钮）----
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

const mdRenderer = new Renderer()
mdRenderer.code = ({ text, lang }) => {
  const language = (lang ?? '').trim() || 'text'
  return (
    `<div class="code-block">` +
    `<div class="code-block-head"><span class="code-lang">${escapeHtml(language)}</span>` +
    `<button class="code-copy" type="button" data-copy>复制</button></div>` +
    `<pre class="code-pre"><code class="code-code">${escapeHtml(text)}</code></pre>` +
    `</div>`
  )
}

// 大模型输出可能是 Markdown，用 marked 渲染为 HTML（含代码块/列表/引用等）。
// ⚠️ 必须过 DOMPurify：模型输出是**不可信内容**，marked 默认不过滤内联 HTML，
// 而主窗口没有 CSP —— 直接 v-html 时，回复里的 `<img onerror>` / `<svg onload>`
// 就能执行任意脚本并调用应用的本地命令（启动程序 / 写配置 / 开网址）。
// 用户消息走插值渲染，不经这里。
function renderMd(text: string): string {
  if (!text) return ''
  const html = marked.parse(text, { async: false, renderer: mdRenderer }) as string
  return DOMPurify.sanitize(html)
}

// assistant 消息的 Markdown 渲染缓存（按消息 id 惰性缓存）。
// 模板里若直接写 `v-html="renderMd(m.content)"`，每次组件 re-render 都会对**每条历史消息**
// 重新全量 marked.parse：流式期间 streamingContent 每个 chunk 都在变 → 每 ~40ms 触发一次
// 全组件 re-render → 历史消息越长、条数越多，parse 越重，主线程被反复阻塞，表现为
// 对话越用越慢、滚动/输入不跟手。缓存后每条消息只 parse 一次，后续命中缓存零开销。
const mdCache = new Map<number, string>()

function mdHtml(m: ChatMessage): string {
  if (m.role !== 'assistant') return ''
  let html = mdCache.get(m.id)
  if (html === undefined) {
    html = renderMd(m.content)
    mdCache.set(m.id, html)
  }
  return html
}

// ---- 消息复制钮：默认隐藏、悬停对应消息才浮现；点击复制原文，短暂变主题色对勾 ----
const copiedMsgId = ref<number | null>(null)
let copiedTimer: ReturnType<typeof setTimeout> | null = null
async function copyMessage(m: ChatMessage) {
  const text = m.content
  if (!text) return
  let ok = false
  try {
    await navigator.clipboard.writeText(text)
    ok = true
  } catch {
    // 剪贴板 API 不可用时回退 execCommand（与 AiProviders 同款做法）
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    try {
      ok = document.execCommand('copy')
    } catch {
      ok = false
    }
    document.body.removeChild(ta)
  }
  if (!ok) {
    showToast('复制失败')
    return
  }
  copiedMsgId.value = m.id
  if (copiedTimer) clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => {
    copiedMsgId.value = null
  }, 1200)
}

// 历史中间过程块的展开状态（按消息 id，默认折叠）
const stepOpen = ref<Record<number, boolean>>({})
function toggleStep(id: number) {
  stepOpen.value[id] = !stepOpen.value[id]
}

// ---- 流式渲染节流 ----
// 首个 chunk 立即渲染（保住首字延迟手感），后续 ~80ms 内合并多次增量只重渲染一次
let lastRenderAt = 0
let renderTimer: number | undefined
function scheduleStreamRender() {
  if (renderTimer) return
  const now = performance.now()
  const delay = lastRenderAt === 0 ? 0 : Math.max(0, 80 - (now - lastRenderAt))
  renderTimer = window.setTimeout(() => {
    renderTimer = undefined
    lastRenderAt = performance.now()
    streamHtml.value = renderMd(streamingContent.value)
  }, delay)
}
// 结束/出错时清掉挂起的节流定时器，避免卸载后写入
function cancelStreamRender() {
  if (renderTimer) {
    window.clearTimeout(renderTimer)
    renderTimer = undefined
  }
}

async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    /* 降级到 execCommand */
  }
  try {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    const ok = document.execCommand('copy')
    document.body.removeChild(ta)
    return ok
  } catch {
    return false
  }
}

// 代码块复制按钮：v-html 内容走事件委托
async function onBodyClick(e: MouseEvent) {
  const target = e.target as HTMLElement | null
  const btn = target?.closest?.('.code-copy') as HTMLElement | null
  if (!btn) return
  const block = btn.closest('.code-block')
  const code = block?.querySelector('.code-code') as HTMLElement | null
  if (!code) return
  const ok = await copyText(code.textContent ?? '')
  if (ok) {
    btn.textContent = '已复制'
    btn.classList.add('copied')
    window.setTimeout(() => {
      btn.textContent = '复制'
      btn.classList.remove('copied')
    }, 1500)
  }
}

// ---- 面板尺寸拖拽 ----
// 左右方位调整宽度，上下方位调整高度；拖拽后同步父级（index.vue 用 chatDockStyle 定位）并持久化
let dragging = false
let startX = 0
let startY = 0
let startSize = 420

const isVertical = computed(() => props.side === 'top' || props.side === 'bottom')

const panelWidth = ref(450)
const panelHeight = ref(380)

function clampSize(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v))
}

function onResizeDown(e: MouseEvent) {
  dragging = true
  startX = e.clientX
  startY = e.clientY
  startSize = isVertical.value ? panelHeight.value : panelWidth.value
  document.addEventListener('mousemove', onResizeMove)
  document.addEventListener('mouseup', onResizeUp)
  e.preventDefault()
}
function onResizeMove(e: MouseEvent) {
  if (!dragging) return
  if (isVertical.value) {
    let h: number
    const dy = e.clientY - startY
    // 顶部：向下拖变高；底部：向上拖变高
    h = props.side === 'top' ? startSize + dy : startSize - dy
    panelHeight.value = clampSize(h, 200, 640)
  } else {
    let w: number
    const dx = e.clientX - startX
    // 右侧：向左拖变宽；左侧：向右拖变宽
    w = props.side === 'right' ? startSize - dx : startSize + dx
    panelWidth.value = clampSize(w, 320, 640)
  }
  // 实时通知父级刷新停靠区尺寸，保证拖拽过程即时反馈
  emit('resized', panelWidth.value, panelHeight.value)
}
function onResizeUp() {
  dragging = false
  document.removeEventListener('mousemove', onResizeMove)
  document.removeEventListener('mouseup', onResizeUp)
  emit('resized', panelWidth.value, panelHeight.value)
  // 独立窗口形态的尺寸由窗口自身管理，不写回抽屉尺寸配置
  if (isTauri() && !isWindow.value) void tauriApi.setChatPanel(panelWidth.value, panelHeight.value, true)
}

onMounted(async () => {
  window.addEventListener('click', onWindowClick)
  window.addEventListener('keydown', onWindowKeydown)
  window.addEventListener('resize', onWindowResize)
  if (!isTauri()) return
  if (!isWindow.value) {
    const [w, h] = await tauriApi.getChatPanel()
    panelWidth.value = w
    panelHeight.value = h
    emit('resized', w, h)
  }
  await Promise.all([loadSessions(), loadModels()])
  if (isWindow.value) focusInput()
})

onBeforeUnmount(() => {
  cancelStreamRender()
  window.removeEventListener('click', onWindowClick)
  window.removeEventListener('keydown', onWindowKeydown)
  window.removeEventListener('resize', onWindowResize)
})

// 输入框自动增高（最多 6 行）
function autosize() {
  const el = inputEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 132) + 'px'
}

/** 聚焦输入框（独立窗口每次唤起时调用，省一次点击） */
function focusInput() {
  void nextTick(() => inputEl.value?.focus())
}

defineExpose({
  refreshModels: () => {
    void loadModels()
  },
  /** 独立窗口唤起时重新拉会话与模型（期间主窗可能新建会话/改动模型配置）；
   * 隐藏期间 unload() 卸过消息的话，这里把当前会话的消息拉回来 */
  refresh: () => {
    void (async () => {
      await Promise.all([loadSessions(), loadModels()])
      if (activeSessionId.value) await openSession(activeSessionId.value)
    })()
  },
  /** 独立窗口隐藏时卸载会话活堆（内存优化）：Low 只吐缓存吐不掉活堆，而消息
   * DOM + markdown 渲染缓存正是随聊天增长的活数据（隐藏后仍占几十 MB）。
   * 会话与消息都在 SQLite，唤起时 refresh() 重拉只要几十毫秒。
   * 流式进行中跳过——等这轮结束后下次隐藏再卸。 */
  unload: () => {
    if (sending.value) return
    messages.value = []
    streamingContent.value = ''
    streamHtml.value = ''
    streamError.value = ''
    turnFlow.value = []
    mdCache.clear()
  },
  focusInput,
})
</script>

<template>
  <div class="chat-panel" :class="['side-' + (props.side || 'right'), { 'mode-window': isWindow }]">
    <div v-if="!isWindow" class="resize-h" @mousedown="onResizeDown"></div>

    <div class="cp-header">
      <div class="cp-title" :title="currentTitle">{{ currentTitle }}</div>
      <!-- 平台剩余积分（#t47）：只在用平台入口时显示；点按去账号页兑换。
           每回合结束 refreshCredits 随结算刷新，本地不做推算 -->
      <button
        v-if="creditRemaining !== null"
        class="cp-credits"
        :class="{ low: creditRemaining <= 0 }"
        :title="creditRemaining > 0 ? '平台剩余积分（按 token 折算）' : '积分已用完，点击去兑换'"
        @click="emit('open-account-settings')"
      >
        <Zap :size="11" />
        剩余 {{ creditRemaining }} 积分
      </button>
      <div class="cp-spacer"></div>
      <button
        ref="addBtnRef"
        class="cp-hbtn cp-add"
        :class="{ open: menuOpen }"
        title="新建 / 切换对话"
        aria-label="新建或切换对话"
        :aria-expanded="menuOpen"
        aria-haspopup="menu"
        @click="toggleMenu"
      >
        <Plus :size="16" />
      </button>
      <button class="cp-hbtn" title="模型设置" @click="openModelSettings">
        <Settings2 :size="15" />
      </button>
      <!-- 独立窗口形态：关闭由外层自制标题栏承担，这里不再放第二个关闭钮 -->
      <button v-if="!isWindow" class="cp-hbtn" title="收起面板" @click="emit('toggle')">
        <PanelRightClose :size="15" />
      </button>
    </div>

    <div class="cp-body-wrap">
      <div class="cp-body" ref="bodyEl" @scroll="onBodyScroll" @click="onBodyClick">
        <div v-if="sessions.length === 0" class="cp-empty">
          <MessageSquare :size="26" :stroke-width="1.5" />
          <template v-if="hasModels">
            <p>还没有对话</p>
            <button class="cp-empty-btn" @click="createSession">开始新对话</button>
          </template>
          <template v-else>
            <p>还没有配置大模型</p>
            <p class="cp-empty-sub">先添加一个 OpenAI 兼容的模型端点，才能开始对话</p>
            <button class="cp-empty-btn" @click="openModelSettings">去配置大模型</button>
          </template>
        </div>

        <template v-else>
          <template v-for="m in messages" :key="m.id">
            <!-- 中间过程（0.9.0）：带 tool_calls 的 assistant 叙述保留为折叠块，点按展开 -->
            <div v-if="m.role === 'assistant' && m.tool_calls && m.content" class="msg ai">
              <div class="ava">X</div>
              <div class="step-block" :class="{ open: !!stepOpen[m.id] }" @click="toggleStep(m.id)">
                <div class="step-head">
                  <ChevronDown :size="12" class="step-chev" />
                  <span class="step-label">中间过程</span>
                </div>
                <div v-if="stepOpen[m.id]" class="bubble md step-body" v-html="mdHtml(m)"></div>
              </div>
            </div>
            <!-- tool 结果行已由卡片时间线呈现，不重复渲染 -->
            <div v-else-if="m.role !== 'tool' && !m.tool_calls" class="msg" :class="m.role === 'user' ? 'user' : 'ai'">
              <div class="ava">{{ m.role === 'user' ? '你' : 'X' }}</div>
              <div class="bubble-col">
                <div v-if="m.role === 'assistant'" class="bubble md" v-html="mdHtml(m)"></div>
                <div v-else class="bubble">{{ m.content }}</div>
                <button
                  class="msg-copy"
                  :class="{ copied: copiedMsgId === m.id }"
                  type="button"
                  :title="copiedMsgId === m.id ? '已复制' : '复制内容'"
                  :aria-label="copiedMsgId === m.id ? '已复制' : '复制这条消息内容'"
                  @click="copyMessage(m)"
                >
                  <Check v-if="copiedMsgId === m.id" :size="12" :stroke-width="2.6" />
                  <Copy v-else :size="12" :stroke-width="2" />
                </button>
              </div>
            </div>
          </template>

          <!-- 本回合时间线：中间过程块（折叠）与工具卡片按发生顺序混排 + 内联审批 -->
          <div v-if="turnFlow.length" class="msg ai">
            <div class="ava">X</div>
            <div class="tool-timeline">
              <template v-for="item in turnFlow" :key="item.key">
                <div v-if="item.kind === 'text'" class="step-block" :class="{ open: item.open }" @click="item.open = !item.open">
                  <div class="step-head">
                    <ChevronDown :size="12" class="step-chev" />
                    <span class="step-label">中间过程</span>
                  </div>
                  <div v-if="item.open" class="bubble md step-body" v-html="item.html"></div>
                </div>
                <div v-else class="tool-card" :class="levelClass(item.level)">
                  <div class="tc-head" @click="item.detailOpen = !item.detailOpen">
                    <span class="tc-dot"></span>
                    <span class="tc-name">{{ item.name }}</span>
                    <span class="tc-status" :class="item.status">{{ statusText(item.status) }}</span>
                  </div>
                  <div v-if="item.summary" class="tc-summary">{{ item.summary }}</div>
                  <!-- awaiting 态还没有结果：展开时展示参数 JSON——审批前先看清楚将执行什么 -->
                  <div v-if="item.detailOpen && (item.detail || item.args)" class="tc-detail">{{ item.detail || prettyArgs(item.args) }}</div>
                  <div v-if="item.status === 'awaiting'" class="tc-actions">
                    <button class="tc-btn ok" @click.stop="respondApproval(item, 'once')">允许</button>
                    <button class="tc-btn always" @click.stop="respondApproval(item, 'always')">本会话允许</button>
                    <button class="tc-btn no" @click.stop="respondApproval(item, 'deny')">拒绝</button>
                  </div>
                </div>
              </template>
            </div>
          </div>

          <div v-if="sending" class="msg ai">
            <div class="ava">X</div>
            <div v-if="streamingContent" class="bubble streaming md">
              <span v-html="streamHtml"></span><span class="cursor"></span>
            </div>
            <div v-else class="bubble streaming">
              <span class="dots">思考中<span>.</span><span>.</span><span>.</span></span><span class="cursor"></span>
            </div>
          </div>

          <div v-if="streamError" class="msg ai">
            <div class="ava">X</div>
            <!-- 积分不足（429 insufficient_credits）分流卡片：给去处，不只给报错（#t47） -->
            <div v-if="isInsufficientCredits" class="bubble error insufficient">
              <span class="err-title"><CircleOff :size="13" /> 积分不足</span>
              <span class="insufficient-text">平台积分余额不足，这次对话没能开始。可兑换积分后重试，或改用自备 API Key 的模型。</span>
              <div class="insufficient-actions">
                <button class="tc-btn ok" @click="emit('open-account-settings')">去兑换积分</button>
                <button class="tc-btn" @click="openModelSettings">配置自备 Key</button>
              </div>
            </div>
            <div v-else class="bubble error">
              <span class="err-title">出错了</span>
              {{ streamError }}
            </div>
          </div>
        </template>
      </div>

      <Transition name="jump">
        <button
          v-if="showJumpBtn"
          class="cp-jump"
          title="跳到底部"
          aria-label="跳到底部"
          @click="scrollToBottom(true)"
        >
          <ChevronDown :size="14" :stroke-width="2.2" />
        </button>
      </Transition>
    </div>

    <div class="cp-input">
      <!-- 当前会话 token 统计：输入 / 输出 / 缓存 / 缓存率 / TPS -->
      <div v-if="activeSessionId" class="cp-stats">
        <span class="stat"><span class="stat-label">输入</span><b>{{ fmtTokens(activeSession?.tokens_input ?? 0) }}</b></span>
        <span class="stat"><span class="stat-label">输出</span><b>{{ fmtTokens(activeSession?.tokens_output ?? 0) }}</b></span>
        <span class="stat"><span class="stat-label">缓存</span><b>{{ fmtTokens(activeSession?.tokens_cache_read ?? 0) }}</b></span>
        <span class="stat"><span class="stat-label">缓存率</span><b>{{ cacheRate }}%</b></span>
        <span class="stat"><span class="stat-label">速度</span><b>{{ trimZero(tps.toFixed(1)) }} tok/s</b></span>
      </div>
      <textarea
        ref="inputEl"
        v-model="input"
        :disabled="sending || !activeSessionId"
        :placeholder="activeSessionId ? '问我任何事，或粘贴内容进来…（Enter 发送，Shift+Enter 换行）' : '请先新建一个对话'"
        @keydown.enter.exact.prevent="send"
        @input="autosize"
      ></textarea>
      <div class="cp-input-bar">
        <div class="model-sel">
          <span class="dotg" :class="{ off: !selectedReady }" :title="readyHint"></span>
          <AppSelect
            v-if="hasModels"
            class="model-app-select"
            :model-value="selectedModel"
            :options="modelOptions"
            :menu-min-width="260"
            :disabled="sending"
            aria-label="选择模型"
            @update:model-value="switchModel"
          />
          <span v-else class="model-empty" @click="openModelSettings">未配置，去设置</span>
          <!-- 权限档快捷切换（§5）：仅可查看 / 每次都问 / 完全权限 -->
          <AppSelect
            class="perm-app-select"
            :model-value="permMode"
            :options="permOptions.map((o) => ({ value: o.value, label: o.label, group: '' }))"
            :menu-min-width="140"
            :disabled="sending"
            aria-label="AI 权限模式"
            :title="`AI 权限：${permLabel}`"
            @update:model-value="(v: string) => (permMode = v as typeof permMode)"
          />
        </div>
        <button v-if="sending && activeTurnId" class="send stop" title="中断本回合（已执行的操作不回滚）" @click="stopTurn">
          <Square :size="12" />
          停止
        </button>
        <button v-else class="send" :disabled="sending || !input.trim() || !activeSessionId" @click="send">
          <Send :size="13" />
          发送
        </button>
      </div>
    </div>

    <!-- 新建 / 切换对话下拉菜单 -->
    <Teleport to="body">
      <Transition name="cpmenu">
        <div
          v-if="menuOpen"
          ref="menuRef"
          class="cp-menu"
          :class="{ 'open-up': menuPos.openUp }"
          :style="{ left: menuPos.x + 'px', top: menuPos.y + 'px', width: menuPos.width + 'px' }"
          role="menu"
        >
          <button class="cp-menu-item cp-menu-new" role="menuitem" @click="onNewSession">
            <Plus :size="14" :stroke-width="2.2" />
            新建对话
          </button>
          <div class="cp-menu-sep"></div>
          <div v-if="sessions.length === 0" class="cp-menu-empty">暂无对话</div>
          <div v-else class="cp-menu-list">
            <div
              v-for="s in sessions"
              :key="s.id"
              class="cp-menu-sess"
              :class="{ on: s.id === activeSessionId }"
              role="menuitem"
              @click="onPickSession(s.id)"
            >
              <span class="cp-menu-sess-title" :title="s.title">{{ s.title }}</span>
              <button class="cp-menu-del" title="删除对话" @click.stop="deleteSession(s.id)">
                <X :size="12" />
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
.chat-panel {
  position: relative;
  flex: 0 0 auto;
  min-width: 0;
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-chat-panel);
  border-left: 1px solid var(--border-soft);
  border-top-left-radius: var(--radius-lg);
  border-bottom-left-radius: var(--radius-lg);
  box-shadow: -6px 0 24px rgba(38, 35, 29, 0.06);
  overflow: hidden;
}
/* 左侧停靠：镜像圆角与阴影 */
.chat-panel.side-left {
  border-left: none;
  border-right: 1px solid var(--border-soft);
  border-top-left-radius: 0;
  border-bottom-left-radius: 0;
  border-top-right-radius: var(--radius-lg);
  border-bottom-right-radius: var(--radius-lg);
  box-shadow: 6px 0 24px rgba(38, 35, 29, 0.06);
}
/* 顶部停靠：只保留下圆角与下方阴影 */
.chat-panel.side-top {
  border-left: none;
  border-top: none;
  border-bottom: 1px solid var(--border-soft);
  border-top-left-radius: 0;
  border-top-right-radius: 0;
  border-bottom-left-radius: var(--radius-lg);
  border-bottom-right-radius: var(--radius-lg);
  box-shadow: 0 6px 24px rgba(38, 35, 29, 0.06);
}
/* 底部停靠：只保留上圆角与上方阴影 */
.chat-panel.side-bottom {
  border-left: none;
  border-bottom: none;
  border-top: 1px solid var(--border-soft);
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
  border-top-left-radius: var(--radius-lg);
  border-top-right-radius: var(--radius-lg);
  box-shadow: 0 -6px 24px rgba(38, 35, 29, 0.06);
}

/* 独立窗口形态：外层 ChatWindow 负责圆角/描边/落影与玻璃底，这里去掉抽屉自带的
   左侧描边与单侧圆角（避免双重边框）。必须排在四个方位规则之后——同为两类选择器，
   靠后才赢得过 .chat-panel.side-* 的描边与圆角 */
.chat-panel.mode-window,
.chat-panel.mode-window.side-left,
.chat-panel.mode-window.side-top,
.chat-panel.mode-window.side-bottom {
  border: none;
  border-radius: 0;
  box-shadow: none;
  background: transparent;
}

/* 拖拽手柄：水平方位（左右）在侧边，垂直方位（上下）在底边/顶边 */
.resize-h {
  position: absolute;
  z-index: 10;
}
.chat-panel.side-right .resize-h {
  left: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
}
.chat-panel.side-left .resize-h {
  right: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
}
.chat-panel.side-top .resize-h {
  left: 0;
  right: 0;
  bottom: -3px;
  height: 6px;
  cursor: row-resize;
}
.chat-panel.side-bottom .resize-h {
  left: 0;
  right: 0;
  top: -3px;
  height: 6px;
  cursor: row-resize;
}
.resize-h:hover::after {
  content: "";
  position: absolute;
  background: var(--brand-500);
  border-radius: 2px;
  opacity: 0.6;
}
.chat-panel.side-right .resize-h:hover::after {
  left: 2px;
  top: 0;
  bottom: 0;
  width: 2px;
}
.chat-panel.side-left .resize-h:hover::after {
  right: 2px;
  top: 0;
  bottom: 0;
  width: 2px;
}
.chat-panel.side-top .resize-h:hover::after {
  top: 2px;
  left: 0;
  right: 0;
  height: 2px;
}
.chat-panel.side-bottom .resize-h:hover::after {
  bottom: 2px;
  left: 0;
  right: 0;
  height: 2px;
}

.cp-header {
  height: 46px;
  flex: 0 0 46px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border-bottom: 1px solid var(--border-soft);
}
.cp-add {
  flex: 0 0 auto;
  color: var(--text-2);
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
}
.cp-add:hover,
.cp-add.open {
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.cp-title {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cp-spacer {
  flex: 0;
}
.model-sel {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.75rem;
  color: var(--text-2);
  background: var(--input-bg);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  padding: 2px 6px;
  max-width: 320px;
  min-width: 0;
}
/* AppSelect 根是 fragment，父级 scoped class 落不到触发器上，用 :deep() 穿透（DESIGN.md §5）。
   穿透后特异性已高于组件自身的 .app-select-trigger，不需要 !important。 */
.model-sel :deep(.model-app-select) {
  min-height: 24px;
  padding: 2px 6px;
  border: none;
  background: transparent;
  font-size: 0.75rem;
  width: 180px;
}
.model-sel :deep(.model-app-select:hover) {
  background: var(--bg-card-soft);
}
.model-empty {
  font-size: 0.75rem;
  color: var(--brand-500);
  cursor: pointer;
  white-space: nowrap;
}
.dotg {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-green-ink);
  flex-shrink: 0;
}
.dotg.off {
  background: var(--text-3);
}
.cp-hbtn {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
}
.cp-hbtn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}

/* ---- 新建 / 切换对话下拉菜单 ---- */
.cp-menu {
  position: fixed;
  z-index: 320;
  padding: 6px;
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-dock);
  -webkit-backdrop-filter: blur(18px) saturate(160%);
  backdrop-filter: blur(18px) saturate(160%);
}
.cp-menu-new {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 8px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--brand-500);
  font-size: 0.8125rem;
  font-weight: 600;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  transition: background 0.12s;
}
.cp-menu-new:hover {
  background: var(--brand-50);
}
.cp-menu-sep {
  height: 1px;
  margin: 6px 4px;
  background: var(--border-soft);
}
.cp-menu-empty {
  padding: 10px 12px;
  font-size: 0.75rem;
  color: var(--text-4);
  text-align: center;
}
.cp-menu-list {
  max-height: 320px;
  overflow-y: auto;
}
.cp-menu-sess {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px 7px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  color: var(--text-2);
  transition: background 0.12s, color 0.12s;
}
.cp-menu-sess:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.cp-menu-sess.on {
  background: var(--brand-50);
  color: var(--brand-500);
}
.cp-menu-sess-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.8125rem;
}
.cp-menu-del {
  width: 20px;
  height: 20px;
  flex: 0 0 auto;
  border: none;
  background: transparent;
  border-radius: 5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-4);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.12s, color 0.12s, background 0.12s;
}
.cp-menu-sess:hover .cp-menu-del {
  opacity: 1;
}
.cp-menu-del:hover {
  background: var(--bg-card-solid);
  color: var(--c-red-ink);
}
.cpmenu-enter-active,
.cpmenu-leave-active {
  transition: opacity 0.12s ease-out, transform 0.12s ease-out;
}
.cpmenu-enter-from,
.cpmenu-leave-to {
  opacity: 0;
  transform: scale(0.96);
}

.cp-body-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.cp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 14px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  /* 对话内容允许鼠标滑动选中复制（全局 body 默认禁选，这里单独放开） */
  -webkit-user-select: text;
  user-select: text;
}
/* 「跳到底部」悬浮按钮：内容区不在底部时出现 */
.cp-jump {
  position: absolute;
  right: 12px;
  bottom: 10px;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  border: 1px solid var(--border-soft);
  background: var(--frost-surface);
  color: var(--text-2);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  box-shadow: var(--shadow-card);
  z-index: 5;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.cp-jump:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.jump-enter-active,
.jump-leave-active {
  transition: opacity 0.18s ease-out, transform 0.18s ease-out;
}
.jump-enter-from,
.jump-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
.cp-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-3);
  font-size: 0.8125rem;
}
.cp-empty svg {
  color: var(--text-3);
  opacity: 0.6;
}
.cp-empty-btn {
  border: none;
  background: var(--brand-500);
  color: var(--text-on-accent);
  font-size: 0.78125rem;
  font-weight: 600;
  border-radius: var(--radius-pill);
  padding: 7px 18px;
  cursor: pointer;
}
.cp-empty-sub {
  font-size: 0.75rem;
  color: var(--text-4);
  max-width: 240px;
  text-align: center;
  line-height: 1.5;
}
.msg {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  max-width: 92%;
}
.msg.user {
  align-self: flex-end;
  flex-direction: row-reverse;
}
.msg .ava {
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  flex: 0 0 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.75rem;
  font-weight: 700;
}
.msg.ai .ava {
  /* AI 回复：头像顶部与气泡内首行文字顶部对齐（气泡 padding-top 为 9px） */
  margin-top: 9px;
}
.msg.user .ava {
  /* 用户消息：头像贴气泡顶部 */
  margin-top: 0;
}
.msg.user .ava {
  background: var(--brand-50);
  color: var(--brand-500);
}
.msg.ai .ava {
  background: linear-gradient(135deg, var(--brand-500), #a78bfa);
  color: #fff;
}
.bubble {
  min-width: 0;
  padding: 9px 12px;
  border-radius: 11px;
  font-size: 0.8125rem;
  line-height: 1.55;
  color: var(--text-1);
  white-space: pre-wrap;
  word-break: break-word;
  overflow-wrap: anywhere;
}
/* 气泡列：气泡 + 下方复制钮纵向排布（用户消息整体右对齐） */
.bubble-col {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  max-width: 100%;
}
.msg.user .bubble-col {
  align-items: flex-end;
}
.bubble-col .bubble {
  max-width: 100%;
}
/* 复制钮：默认隐藏，悬停对应消息行才浮现；点击后短暂变主题色对勾 */
.msg-copy {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--text-4);
  cursor: pointer;
  opacity: 0;
  transition: opacity 120ms, color 120ms;
}
.msg:hover .msg-copy,
.msg-copy:focus-visible {
  opacity: 1;
}
.msg-copy:hover {
  color: var(--text-2);
  background: var(--bg-card-soft);
}
.msg-copy.copied {
  color: var(--brand-500);
  opacity: 1;
}
.msg.user .bubble {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-top-right-radius: 4px;
}
.msg.ai .bubble {
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-top-left-radius: 4px;
}
/* AI 回复的 Markdown 渲染：区块元素重新换行布局，代码块等独立展示 */
.bubble.md {
  white-space: normal;
}
.bubble.md :deep(h1),
.bubble.md :deep(h2),
.bubble.md :deep(h3),
.bubble.md :deep(h4) {
  color: var(--text-1);
  margin: 12px 0 6px;
  line-height: 1.4;
}
.bubble.md :deep(h1) { font-size: 1.125rem; }
.bubble.md :deep(h2) { font-size: 1rem; }
.bubble.md :deep(h3) { font-size: 0.90625rem; }
.bubble.md :deep(h4) { font-size: 0.84375rem; }
.bubble.md :deep(p) {
  margin: 6px 0;
}
/* 首元素去掉上外边距：让首行文字从气泡内边距处开始，与头像顶部对齐。
   注意：必须用「元素 + :first-child」形式，`:deep(:first-child)` 会被编译器
   编译成无 scoped 前缀的裸全局规则，造成全局污染与两侧不对称 */
.bubble.md :deep(p:first-child),
.bubble.md :deep(h1:first-child),
.bubble.md :deep(h2:first-child),
.bubble.md :deep(h3:first-child),
.bubble.md :deep(h4:first-child),
.bubble.md :deep(ul:first-child),
.bubble.md :deep(ol:first-child),
.bubble.md :deep(pre:first-child),
.bubble.md :deep(blockquote:first-child),
.bubble.md :deep(table:first-child),
.bubble.md :deep(.code-block:first-child) {
  margin-top: 0;
}
/* 流式输出时文字包在 span 内：span 内首段同样去掉上外边距 */
.bubble.md :deep(span:first-child p:first-child),
.bubble.md :deep(span:first-child h1:first-child),
.bubble.md :deep(span:first-child h2:first-child),
.bubble.md :deep(span:first-child h3:first-child),
.bubble.md :deep(span:first-child h4:first-child),
.bubble.md :deep(span:first-child ul:first-child),
.bubble.md :deep(span:first-child ol:first-child),
.bubble.md :deep(span:first-child pre:first-child),
.bubble.md :deep(span:first-child blockquote:first-child),
.bubble.md :deep(span:first-child table:first-child),
.bubble.md :deep(span:first-child .code-block:first-child) {
  margin-top: 0;
}
.bubble.md :deep(ul),
.bubble.md :deep(ol) {
  padding-left: 22px;
  margin: 6px 0;
}
.bubble.md :deep(ul) { list-style: disc; }
.bubble.md :deep(ol) { list-style: decimal; }
.bubble.md :deep(li) {
  margin: 2px 0;
}
/* 行内 code（反引号） */
.bubble.md :deep(code) {
  background: var(--code-inline-bg);
  color: var(--code-inline-fg);
  border-radius: 5px;
  padding: 1px 6px;
  font-size: 0.75rem;
  font-family: var(--font-mono, ui-monospace, 'Cascadia Code', Consolas, monospace);
  word-break: break-all;
}
/* 代码块：深色框 + 头部语言标签 + 复制按钮（贴近主流 AI 对话展示方式） */
.bubble.md :deep(.code-block) {
  margin: 8px 0;
  max-width: 100%;
  border: 1px solid var(--code-border);
  border-radius: 8px;
  background: var(--bg-code);
  overflow: hidden;
}
.bubble.md :deep(.code-block-head) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px 6px 12px;
  background: var(--bg-code-head);
  border-bottom: 1px solid var(--code-border);
}
.bubble.md :deep(.code-lang) {
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  color: var(--code-text-dim);
  text-transform: lowercase;
}
.bubble.md :deep(.code-copy) {
  border: 1px solid var(--code-border);
  background: transparent;
  color: var(--code-text-dim);
  font-size: 0.6875rem;
  font-family: inherit;
  border-radius: 5px;
  padding: 2px 8px;
  cursor: pointer;
  transition: color 0.12s, border-color 0.12s, background 0.12s;
}
.bubble.md :deep(.code-copy:hover) {
  color: var(--code-text);
  border-color: color-mix(in srgb, var(--code-text-dim) 60%, transparent);
}
.bubble.md :deep(.code-copy.copied) {
  color: var(--c-green-ink);
  border-color: color-mix(in srgb, var(--c-green-ink) 50%, transparent);
}
.bubble.md :deep(.code-pre) {
  margin: 0;
  padding: 12px 14px;
  overflow-x: auto;
  background: transparent;
  border: none;
  border-radius: 0;
}
.bubble.md :deep(.code-code) {
  display: block;
  background: transparent;
  border: none;
  padding: 0;
  color: var(--code-text);
  font-size: 0.78125rem;
  line-height: 1.55;
  font-family: var(--font-mono, ui-monospace, 'Cascadia Code', Consolas, monospace);
  white-space: pre;
  word-break: normal;
}
.bubble.md :deep(blockquote) {
  border-left: 3px solid var(--brand-500);
  padding-left: 10px;
  color: var(--text-3);
  margin: 6px 0;
}
.bubble.md :deep(a) {
  color: var(--brand-500);
  text-decoration: underline;
}
.bubble.md :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-soft);
  margin: 12px 0;
}
.bubble.md :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
  display: block;
  overflow-x: auto;
}
.bubble.md :deep(th),
.bubble.md :deep(td) {
  border: 1px solid var(--border-soft);
  padding: 5px 9px;
  font-size: 0.78125rem;
}
.bubble.md :deep(strong) { font-weight: 700; }
.bubble.md :deep(em) { font-style: italic; }
.bubble.md :deep(img) {
  max-width: 100%;
  border-radius: var(--radius-sm);
}
.bubble.streaming {
  color: var(--text-2);
}
.cursor {
  display: inline-block;
  width: 7px;
  height: 14px;
  background: var(--brand-500);
  margin-left: 2px;
  vertical-align: -2px;
  animation: blink 1s steps(1) infinite;
  border-radius: 1px;
}
@keyframes blink {
  50% { opacity: 0; }
}
.dots span {
  animation: dot 1.2s infinite;
}
.dots span:nth-child(2) { animation-delay: 0.2s; }
.dots span:nth-child(3) { animation-delay: 0.4s; }
@keyframes dot {
  0%, 60%, 100% { opacity: 0.2; }
  30% { opacity: 1; }
}
.bubble.error {
  border-color: var(--c-red-soft);
  background: color-mix(in srgb, var(--c-red-soft) 30%, var(--bg-card-solid));
  color: var(--text-1);
}
.err-title {
  display: block;
  font-weight: 600;
  color: var(--c-red-ink);
  margin-bottom: 3px;
}

/* ---- 平台剩余积分徽标（#t47） ---- */
.cp-credits {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
  padding: 2px 9px;
  font-size: 0.71875rem;
  font-weight: 600;
  border-radius: 999px;
  border: 1px solid color-mix(in srgb, var(--brand-500) 30%, transparent);
  background: color-mix(in srgb, var(--brand-500) 10%, var(--bg-card-solid));
  color: var(--text-2);
  cursor: pointer;
  white-space: nowrap;
}
.cp-credits:hover {
  border-color: color-mix(in srgb, var(--brand-500) 55%, transparent);
  color: var(--text-1);
}
.cp-credits.low {
  border-color: var(--c-red-soft);
  background: color-mix(in srgb, var(--c-red-soft) 25%, var(--bg-card-solid));
  color: var(--c-red-ink);
}

/* 积分不足分流卡片（#t47）：标题下给一句去向说明 + 两个按钮 */
.bubble.error.insufficient {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.insufficient .err-title {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  margin-bottom: 0;
}
.insufficient-text {
  font-size: 0.8125rem;
  line-height: 1.55;
  color: var(--text-2);
}
.insufficient-actions {
  display: flex;
  gap: 8px;
}

/* ---- 当前会话 token 统计行（置于输入区内、紧贴输入框上方） ---- */
.cp-stats {
  display: flex;
  align-items: center;
  gap: 6px;
  overflow-x: auto;
  scrollbar-width: none;
}
.cp-stats::-webkit-scrollbar {
  display: none;
}
.cp-stats .stat {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: baseline;
  gap: 4px;
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  font-size: 0.6875rem;
  color: var(--text-4);
}
.cp-stats .stat b {
  font-size: 0.71875rem;
  font-weight: 600;
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}
.cp-stats .stat-label {
  color: var(--text-4);
}

.cp-input {
  flex: 0 0 auto;
  border-top: 1px solid var(--border-soft);
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--bg-card);
}
.cp-input textarea {
  width: 100%;
  height: 52px;
  min-height: 40px;
  max-height: 132px;
  resize: none;
  background: var(--input-bg);
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  padding: 9px 11px;
  font-size: 0.8125rem;
  font-family: inherit;
  color: var(--text-1);
  outline: none;
  transition: border-color 150ms ease-out, box-shadow 150ms ease-out;
}
.cp-input textarea:focus {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px var(--brand-glow);
}
.cp-input-bar {
  display: flex;
  align-items: center;
  gap: 8px;
}
/* 生成中的进行中动效落在停止键上：方块图标脉冲呼吸（进行中一眼可见），
   不再需要「生成中…」文字提示；系统「减少动态效果」时被 style.css 全局降级 */
.send.stop svg {
  animation: stop-pulse 1.1s ease-in-out infinite;
}
@keyframes stop-pulse {
  0%, 100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.35;
    transform: scale(0.8);
  }
}
.send.stop {
  background: var(--c-red);
}
.send {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.78125rem;
  font-weight: 600;
  color: var(--text-on-accent);
  background: var(--brand-500);
  border: none;
  border-radius: var(--radius-sm);
  padding: 6px 14px;
  cursor: pointer;
  transition: opacity 150ms, transform 150ms;
}
.send:active {
  transform: scale(0.96);
}
.send:disabled {
  opacity: 0.5;
  cursor: default;
}

/* ---- Agent 工具时间线（#t46）---- */
.tool-timeline {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  width: 100%;
}
/* 中间过程折叠块（0.9.0）：一行摘要头 + 展开正文，保留多轮叙述又不占空间 */
.step-block {
  border: 1px dashed var(--border-soft);
  border-radius: var(--radius-sm);
  padding: 5px 10px;
  min-width: 0;
  cursor: pointer;
  user-select: none;
}
.step-head {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 0.6875rem;
  color: var(--text-3);
}
.step-chev {
  flex: none;
  transition: transform 0.15s ease;
}
.step-block.open .step-chev {
  transform: rotate(180deg);
}
.step-body {
  margin-top: 6px;
  cursor: auto;
  user-select: text;
}
.tool-card {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  padding: 6px 10px;
  font-size: 0.75rem;
  background: var(--bg-card-soft);
}
.tc-head {
  display: flex;
  align-items: center;
  gap: 7px;
  cursor: pointer;
  min-width: 0;
}
.tc-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex: none;
  background: var(--text-3);
}
.tool-card.lv-read .tc-dot {
  background: var(--c-green);
}
.tool-card.lv-write .tc-dot {
  background: var(--c-yellow);
}
.tool-card.lv-danger .tc-dot {
  background: var(--c-red);
}
.tc-name {
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-status {
  margin-left: auto;
  flex: none;
  font-size: 0.6875rem;
  color: var(--text-3);
}
.tc-status.ok {
  color: var(--c-green-ink);
}
.tc-status.error,
.tc-status.denied {
  color: var(--c-red-ink);
}
.tc-status.awaiting {
  color: var(--c-yellow-ink);
  font-weight: 600;
}
.tc-summary {
  margin-top: 3px;
  color: var(--text-2);
  word-break: break-word;
}
.tc-detail {
  margin-top: 5px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 220px;
  overflow-y: auto;
  color: var(--text-2);
  font-size: 0.71875rem;
}
.tc-actions {
  margin-top: 7px;
  display: flex;
  gap: 8px;
}
.tc-btn {
  font-size: 0.71875rem;
  font-weight: 600;
  padding: 4px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-soft);
  background: transparent;
  color: var(--text-1);
  cursor: pointer;
}
.tc-btn:hover {
  background: var(--bg-card-soft);
}
.tc-btn.ok {
  color: var(--c-green-ink);
  border-color: var(--c-green-ink);
}
.tc-btn.always {
  color: var(--c-yellow-ink);
  border-color: var(--c-yellow-ink);
}
.tc-btn.no {
  color: var(--c-red-ink);
  border-color: var(--c-red-ink);
}
.perm-app-select {
  flex: none;
  font-size: 0.71875rem;
}
</style>
