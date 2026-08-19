<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { marked, Renderer } from 'marked'
import { ChevronDown, MessageSquare, PanelRightClose, Plus, Send, Settings2, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type ChatMessage, type ChatModelConfig, type ChatSession, type ChatStreamEvent } from '../api/tauri'
import AppSelect from './AppSelect.vue'

const emit = defineEmits<{
  (e: 'toggle'): void
  (e: 'open-model-settings'): void
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

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

const activeSession = computed(() => sessions.value.find((s) => s.id === activeSessionId.value) ?? null)
// 顶部直接显示当前对话标题（未命名时为「新对话」）
const currentTitle = computed(() => activeSession.value?.title || '新对话')

const modelOptions = computed(() =>
  models.value.map((m) => ({ value: m.name, label: modelLabel(m), group: modelGroup(m) })),
)
const hasModels = computed(() => models.value.length > 0)

// 去掉小数尾部的 ".0"（如 1.0 → 1）
function trimZero(s: string): string {
  return s.endsWith('.0') ? s.slice(0, -2) : s
}

// token 数量格式化：≥100万用 M，≥1000 用 K，否则原值
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return trimZero((n / 1_000_000).toFixed(1)) + 'M'
  if (n >= 1000) return trimZero((n / 1000).toFixed(1)) + 'K'
  return String(n)
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
  if (!isTauri()) return
  const [msgs, s] = await Promise.all([
    tauriApi.listChatMessages(id),
    tauriApi.listChatSessions().then((l) => l.find((x) => x.id === id) ?? null),
  ])
  messages.value = msgs
  if (s) {
    selectedModel.value = s.model_name
    // 同步本地会话条目（标题 / token 等可能已变化）
    const i = sessions.value.findIndex((x) => x.id === id)
    if (i >= 0) sessions.value = sessions.value.map((x) => (x.id === id ? s : x))
  }
  // 打开会话强制定位到最新消息（非 force 模式会因 scrollTop=0 误判「不在底部」而停在第一句）
  scrollToBottom(true)
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
  if (!selectedModel.value) {
    const def = models.value.find((m) => m.is_default) ?? models.value[0]
    if (def) selectedModel.value = def.name
  }
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
  const cfg = models.value.find((m) => m.name === selectedModel.value)
  if (!cfg || !cfg.has_api_key) {
    showToast('当前模型未配置 API Key（设置 → AI 助手）', {
      label: '去配置',
      onClick: openModelSettings,
    })
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
  // 用户主动发送：强制定位到最新消息，后续流式输出保持跟随
  scrollToBottom(true)

  try {
    await tauriApi.sendChatMessage(activeSessionId.value, content, (e: ChatStreamEvent) => {
      if (e.type === 'chunk') {
        streamingContent.value += e.content
        scheduleStreamRender()
        scrollToBottom()
      } else if (e.type === 'done') {
        cancelStreamRender()
        streamHtml.value = ''
        // 用后端落库的权威消息替换占位回复
        const i = messages.value.findIndex((m) => m.id === userMsg.id)
        messages.value = [...messages.value.slice(0, i + 1), e.message]
        streamingContent.value = ''
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
        scrollToBottom(true)
      } else if (e.type === 'error') {
        cancelStreamRender()
        streamError.value = e.message
        if (e.partial) streamingContent.value = e.partial
        streamHtml.value = renderMd(streamingContent.value)
        sending.value = false
        scrollToBottom(true)
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

// 大模型输出可能是 Markdown，用 marked 渲染为 HTML（含代码块/列表/引用等）
function renderMd(text: string): string {
  if (!text) return ''
  return marked.parse(text, { async: false, renderer: mdRenderer }) as string
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

// ---- 面板宽度拖拽 ----
let dragging = false
let startX = 0
let startWidth = 420

function onResizeDown(e: MouseEvent) {
  dragging = true
  startX = e.clientX
  startWidth = panelWidth.value
  document.addEventListener('mousemove', onResizeMove)
  document.addEventListener('mouseup', onResizeUp)
  e.preventDefault()
}
function onResizeMove(e: MouseEvent) {
  if (!dragging) return
  const w = Math.max(320, Math.min(640, startWidth - (e.clientX - startX)))
  panelWidth.value = w
}
function onResizeUp() {
  dragging = false
  document.removeEventListener('mousemove', onResizeMove)
  document.removeEventListener('mouseup', onResizeUp)
  if (isTauri()) void tauriApi.setChatPanel(panelWidth.value, true)
}

const panelWidth = ref(420)

onMounted(async () => {
  window.addEventListener('click', onWindowClick)
  window.addEventListener('keydown', onWindowKeydown)
  window.addEventListener('resize', onWindowResize)
  if (!isTauri()) return
  const [w] = await tauriApi.getChatPanel()
  panelWidth.value = w
  await Promise.all([loadSessions(), loadModels()])
})

onBeforeUnmount(() => {
  cancelStreamRender()
  window.removeEventListener('click', onWindowClick)
  window.removeEventListener('keydown', onWindowKeydown)
  window.removeEventListener('resize', onWindowResize)
})

defineExpose({ refreshModels: () => { void loadModels() } })

// 输入框自动增高（最多 6 行）
function autosize() {
  const el = inputEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 132) + 'px'
}
</script>

<template>
  <div class="chat-panel" :style="{ width: panelWidth + 'px' }">
    <div class="resize-h" @mousedown="onResizeDown"></div>

    <div class="cp-header">
      <div class="cp-title" :title="currentTitle">{{ currentTitle }}</div>
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
      <button class="cp-hbtn" title="收起面板" @click="emit('toggle')">
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
          <div
            v-for="m in messages"
            :key="m.id"
            class="msg"
            :class="m.role === 'user' ? 'user' : 'ai'"
          >
            <div class="ava">{{ m.role === 'user' ? '你' : 'A' }}</div>
            <div v-if="m.role === 'assistant'" class="bubble md" v-html="renderMd(m.content)"></div>
            <div v-else class="bubble">{{ m.content }}</div>
          </div>

          <div v-if="sending" class="msg ai">
            <div class="ava">A</div>
            <div v-if="streamingContent" class="bubble streaming md">
              <span v-html="streamHtml"></span><span class="cursor"></span>
            </div>
            <div v-else class="bubble streaming">
              <span class="dots">思考中<span>.</span><span>.</span><span>.</span></span><span class="cursor"></span>
            </div>
          </div>

          <div v-if="streamError" class="msg ai">
            <div class="ava">A</div>
            <div class="bubble error">
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
        <span class="stat"><span class="stat-label">TPS</span><b>{{ trimZero(tps.toFixed(1)) }}</b></span>
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
        <span v-if="sending" class="cp-hint">生成中…</span>
        <div class="model-sel">
          <span class="dotg" :class="{ off: !models.find((m) => m.name === selectedModel)?.has_api_key }"></span>
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
        </div>
        <button class="send" :disabled="sending || !input.trim() || !activeSessionId" @click="send">
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
  display: flex;
  flex-direction: column;
  background: var(--bg-chat-panel);
  border-left: 1px solid var(--border-soft);
  border-top-left-radius: var(--radius-lg);
  border-bottom-left-radius: var(--radius-lg);
  box-shadow: -6px 0 24px rgba(38, 35, 29, 0.06);
  overflow: hidden;
}
.resize-h {
  position: absolute;
  left: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 10;
}
.resize-h:hover::after {
  content: "";
  position: absolute;
  left: 2px;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--brand-500);
  border-radius: 2px;
  opacity: 0.6;
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
.model-app-select {
  min-height: 24px !important;
  padding: 2px 6px !important;
  border: none !important;
  background: transparent !important;
  font-size: 0.75rem !important;
  width: 180px !important;
}
.model-app-select:hover {
  background: var(--bg-card-soft) !important;
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
.cp-hint {
  font-size: 0.71875rem;
  color: var(--text-3);
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
</style>
