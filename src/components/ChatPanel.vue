<script setup lang="ts">
import { computed, inject, nextTick, onMounted, ref } from 'vue'
import { marked } from 'marked'
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
const streamError = ref('')
const models = ref<ChatModelConfig[]>([])
const selectedModel = ref('')
const bodyEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLTextAreaElement | null>(null)
// 内容区不在底部时显示「跳到底部」按钮
const showJumpBtn = ref(false)

const modelOptions = computed(() =>
  models.value.map((m) => ({ value: m.name, label: modelLabel(m), group: modelGroup(m) })),
)
const hasModels = computed(() => models.value.length > 0)

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
  if (s) selectedModel.value = s.model_name
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
        scrollToBottom()
      } else if (e.type === 'done') {
        // 用后端落库的权威消息替换占位回复
        const i = messages.value.findIndex((m) => m.id === userMsg.id)
        const next = [...messages.value.slice(0, i + 1), e.message]
        messages.value = next
        streamingContent.value = ''
        sending.value = false
        const s = sessions.value.find((x) => x.id === activeSessionId.value)
        if (s) {
          s.updated_at = new Date().toISOString()
          sessions.value = [...sessions.value].sort((a, b) => b.updated_at.localeCompare(a.updated_at))
        }
        scrollToBottom(true)
      } else if (e.type === 'error') {
        streamError.value = e.message
        if (e.partial) streamingContent.value = e.partial
        sending.value = false
        scrollToBottom(true)
      }
    })
  } catch (err) {
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

// 大模型输出可能是 Markdown，用 marked 渲染为 HTML（含代码块/列表/引用等）
function renderMd(text: string): string {
  if (!text) return ''
  return marked.parse(text, { async: false }) as string
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
  if (!isTauri()) return
  const [w] = await tauriApi.getChatPanel()
  panelWidth.value = w
  await Promise.all([loadSessions(), loadModels()])
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
        <div class="cp-title">
          <MessageSquare :size="15" :stroke-width="2" />
          AI 对话
        </div>
        <div class="cp-spacer"></div>
        <button class="cp-hbtn" title="模型设置" @click="openModelSettings">
          <Settings2 :size="15" />
        </button>
        <button class="cp-hbtn" title="收起面板" @click="emit('toggle')">
          <PanelRightClose :size="15" />
        </button>
      </div>

      <div class="cp-sessions">
        <div
          v-for="s in sessions"
          :key="s.id"
          class="sess-pill"
          :class="{ on: activeSessionId === s.id }"
          @click="openSession(s.id)"
        >
          <span class="sess-title" :title="s.title">{{ s.title }}</span>
          <button class="sess-x" title="删除会话" @click.stop="deleteSession(s.id)">
            <X :size="11" />
          </button>
        </div>
        <button class="sess-add" title="新建会话" @click="createSession">
          <Plus :size="13" />
          新建
        </button>
      </div>

    <div class="cp-body-wrap">
      <div class="cp-body" ref="bodyEl" @scroll="onBodyScroll">
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
              <span v-html="renderMd(streamingContent)"></span><span class="cursor"></span>
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
.cp-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 14px;
  font-weight: 650;
  color: var(--text-1);
}
.cp-title svg {
  color: var(--brand-500);
}
.cp-spacer {
  flex: 1;
}
.model-sel {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
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
  font-size: 12px !important;
  width: 180px !important;
}
.model-app-select:hover {
  background: var(--bg-card-soft) !important;
}
.model-empty {
  font-size: 12px;
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

.cp-sessions {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border-soft);
  overflow-x: auto;
  scrollbar-width: none;
}
.cp-sessions::-webkit-scrollbar {
  display: none;
}
.sess-pill {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--text-2);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  cursor: pointer;
  max-width: 160px;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.sess-pill:hover {
  color: var(--text-1);
}
.sess-pill.on {
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.sess-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sess-x {
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  border-radius: 4px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 150ms;
}
.sess-pill:hover .sess-x {
  opacity: 0.7;
}
.sess-x:hover {
  opacity: 1 !important;
  background: var(--bg-card-solid);
}
.sess-add {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--text-3);
  background: transparent;
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-sm);
  padding: 4px 9px;
  cursor: pointer;
}
.sess-add:hover {
  color: var(--brand-500);
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
  font-size: 13px;
}
.cp-empty svg {
  color: var(--text-3);
  opacity: 0.6;
}
.cp-empty-btn {
  border: none;
  background: var(--brand-500);
  color: var(--text-on-accent);
  font-size: 12.5px;
  font-weight: 600;
  border-radius: var(--radius-pill);
  padding: 7px 18px;
  cursor: pointer;
}
.cp-empty-sub {
  font-size: 12px;
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
  font-size: 12px;
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
  padding: 9px 12px;
  border-radius: 11px;
  font-size: 13px;
  line-height: 1.55;
  color: var(--text-1);
  white-space: pre-wrap;
  word-break: break-word;
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
.bubble.md :deep(h1) { font-size: 18px; }
.bubble.md :deep(h2) { font-size: 16px; }
.bubble.md :deep(h3) { font-size: 14.5px; }
.bubble.md :deep(h4) { font-size: 13.5px; }
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
.bubble.md :deep(table:first-child) {
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
.bubble.md :deep(span:first-child table:first-child) {
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
.bubble.md :deep(code) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  font-size: 12px;
  font-family: var(--font-mono, ui-monospace, 'Cascadia Code', Consolas, monospace);
  word-break: break-all;
}
.bubble.md :deep(pre) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 10px;
  overflow-x: auto;
  margin: 8px 0;
}
.bubble.md :deep(pre code) {
  background: transparent;
  border: none;
  padding: 0;
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
  font-size: 12.5px;
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
  font-size: 13px;
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
  font-size: 11.5px;
  color: var(--text-3);
}
.send {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
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
