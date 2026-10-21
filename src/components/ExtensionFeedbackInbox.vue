<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref, toRef, useId, watch } from 'vue'
import {
  AppWindow,
  CheckCheck,
  Cpu,
  Eye,
  EyeOff,
  Inbox,
  Mail,
  MessageSquare,
  Reply,
  UserX,
  X,
} from 'lucide-vue-next'
import {
  isTauri,
  tauriApi,
  type ExtensionFeedback,
  type FeedbackUnreadExtension,
} from '../api/tauri'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  feedbackErrorText,
  feedbackKindLabel,
  feedbackStatusLabel,
  feedbackTimeText,
} from '../utils/feedback'
import AppSelect from './AppSelect.vue'

/**
 * 开发者收件箱：我发布到市场的扩展收到的反馈。
 *
 * 平台托管的意义就在这里——用户不用去开发者自留的 GitHub/邮箱，开发者也不用守着外部渠道；
 * 反馈与回复都在应用内闭环。入口是扩展中心「我的扩展」tab 上的未读红点。
 *
 * 隐私边界：这里（开发者视角）能看到联系方式与诊断；公开板看不到 —— 同一个后端接口
 * 按归属分流，本组件展示的 `contact` / `diagnostics` 只在开发者接口返回。
 */
const props = defineProps<{ visible: boolean }>()

const emit = defineEmits<{
  (e: 'update:visible', value: boolean): void
  /** 已读水位推进后通知宿主刷新红点 */
  (e: 'read'): void
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const titleId = useId()
const cardRef = ref<HTMLElement | null>(null)

const extensions = ref<FeedbackUnreadExtension[]>([])
const selectedExt = ref('')
const items = ref<ExtensionFeedback[]>([])

const loading = ref(false)
const listLoading = ref(false)
const error = ref('')
const listError = ref('')
const signedIn = ref(true)
const markingAll = ref(false)

const replyDraft = ref<Record<number, string>>({})
const resolveOnReply = ref<Record<number, boolean>>({})
const replyOpenId = ref<number | null>(null)
const hideConfirmId = ref<number | null>(null)
const busyId = ref<number | null>(null)
const diagOpen = ref<Set<number>>(new Set())

// 相对时间（刚刚 / N 分钟前）需要一个会走的时钟：弹窗开着期间每 60s 推进一次，
// 否则「刚刚」会一直停到下次重开（只影响展示，不重新拉网络数据）
const nowTick = ref(Date.now())
let clockTimer: number | undefined

function startClock() {
  stopClock()
  nowTick.value = Date.now()
  clockTimer = window.setInterval(() => {
    nowTick.value = Date.now()
  }, 60_000)
}

function stopClock() {
  if (clockTimer !== undefined) {
    window.clearInterval(clockTimer)
    clockTimer = undefined
  }
}

useFocusTrap(toRef(props, 'visible'), cardRef)

const totalUnread = computed(() => extensions.value.reduce((sum, e) => sum + e.unread, 0))
const selectedExtension = computed(() => extensions.value.find((e) => e.ext_id === selectedExt.value))
const extOptions = computed(() =>
  extensions.value.map((e) => ({
    value: e.ext_id,
    label: e.unread > 0 ? `${e.name} · ${e.unread} 条未读` : e.name,
  })),
)

function close() {
  emit('update:visible', false)
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible) return
  if (e.key === 'Escape') {
    e.preventDefault()
    close()
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  stopClock()
})

/** 打开即重置：清掉上一次的草稿、展开态与选中项，避免「关掉再开还是旧内容」 */
async function init() {
  error.value = ''
  listError.value = ''
  items.value = []
  extensions.value = []
  selectedExt.value = ''
  loading.value = true
  signedIn.value = true
  markingAll.value = false
  replyOpenId.value = null
  hideConfirmId.value = null
  busyId.value = null
  diagOpen.value = new Set()
  replyDraft.value = {}
  resolveOnReply.value = {}

  try {
    if (!isTauri()) {
      return
    }
    const data = await tauriApi.feedbackInboxUnread()
    extensions.value = data.extensions
    // 优先落在有未读的扩展上（多扩展时用户最想看新的）
    const first = data.extensions.find((e) => e.unread > 0) ?? data.extensions[0]
    if (first) {
      selectedExt.value = first.ext_id
      return
    }
    // 没有已发布扩展：区分「未登录」与「登录了但没发布」，空态文案不同
    try {
      signedIn.value = (await tauriApi.accountStatus()).loggedIn
    } catch {
      signedIn.value = true
    }
  } catch (e) {
    error.value = feedbackErrorText(String(e))
  } finally {
    loading.value = false
  }
}

watch(
  () => props.visible,
  (v) => {
    if (v) {
      void init()
      startClock()
    } else {
      stopClock()
    }
  },
)

watch(selectedExt, (id) => {
  if (id && props.visible) void loadList(id)
})

async function loadList(extId: string) {
  if (!extId) return
  listLoading.value = true
  listError.value = ''
  try {
    items.value = await tauriApi.feedbackInboxList(extId)
    // 只有真的读到了才标已读：加载失败（403/断网）不能把红点凭空清掉
    void markRead(extId)
  } catch (e) {
    items.value = []
    listError.value = feedbackErrorText(String(e))
  } finally {
    listLoading.value = false
  }
}

async function markRead(extId: string) {
  try {
    await tauriApi.feedbackMarkRead(extId)
    const e = extensions.value.find((x) => x.ext_id === extId)
    if (e) e.unread = 0
    emit('read')
  } catch {
    /* 静默 */
  }
}

async function markAllRead() {
  if (markingAll.value) return
  const pending = extensions.value.filter((e) => e.unread > 0)
  if (pending.length === 0) return
  markingAll.value = true
  let failed = 0
  try {
    // 逐条推进：成功的立即清零（部分失败时，已成功的扩展不该继续显示未读等下一轮轮询）
    for (const e of pending) {
      try {
        await tauriApi.feedbackMarkRead(e.ext_id)
        e.unread = 0
      } catch {
        failed += 1
      }
    }
    emit('read')
    showToast(failed === 0 ? '已全部标记为已读' : `${failed} 个扩展标记失败，其余已标记`)
  } finally {
    markingAll.value = false
  }
}

function toggleDiag(id: number) {
  const next = new Set(diagOpen.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  diagOpen.value = next
}

function openReply(f: ExtensionFeedback) {
  replyOpenId.value = replyOpenId.value === f.id ? null : f.id
  if (!(f.id in replyDraft.value)) replyDraft.value[f.id] = ''
}

async function sendReply(f: ExtensionFeedback) {
  if (busyId.value !== null) return
  const draft = (replyDraft.value[f.id] ?? '').trim()
  if (!draft) {
    showToast('回复内容不能为空')
    return
  }
  busyId.value = f.id
  try {
    const resolved = !!resolveOnReply.value[f.id]
    await tauriApi.feedbackReply(f.id, draft, resolved)
    f.reply = draft
    f.replied_at = Date.now()
    f.status = resolved ? 'resolved' : 'replied'
    replyOpenId.value = null
    delete replyDraft.value[f.id]
    delete resolveOnReply.value[f.id]
    showToast(resolved ? '回复已发送，并标记为已解决' : '回复已发送，公开可见')
  } catch (e) {
    showToast(`回复失败：${feedbackErrorText(String(e))}`)
  } finally {
    busyId.value = null
  }
}

/** 两段式隐藏：第一次点变成「确认隐藏？」，3 秒内再点才生效（防误触） */
function toggleHide(f: ExtensionFeedback) {
  if (busyId.value !== null) return
  if (!f.hidden && hideConfirmId.value !== f.id) {
    hideConfirmId.value = f.id
    window.setTimeout(() => {
      if (hideConfirmId.value === f.id) hideConfirmId.value = null
    }, 3000)
    return
  }
  void applyHide(f)
}

async function applyHide(f: ExtensionFeedback) {
  busyId.value = f.id
  try {
    const next = !f.hidden
    await tauriApi.feedbackHide(f.id, next)
    f.hidden = next
    hideConfirmId.value = null
    showToast(next ? '已隐藏：公开板不再显示，你与管理员仍可见' : '已恢复公开显示')
  } catch (e) {
    showToast(`操作失败：${feedbackErrorText(String(e))}`)
  } finally {
    busyId.value = null
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" role="presentation">
      <div
        ref="cardRef"
        class="modal-card ifb-card"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="titleId"
      >
        <header class="ifb-head">
          <div class="ifb-ico">
            <Inbox :size="18" :stroke-width="2" aria-hidden="true" />
          </div>
          <div class="ifb-tt">
            <h3 :id="titleId">用户反馈</h3>
            <p>我发布的扩展收到的反馈 · 回复公开可见，联系方式与诊断仅你与管理员可见</p>
          </div>
          <button class="ifb-x" type="button" aria-label="关闭" @click="close">
            <X :size="16" :stroke-width="2" aria-hidden="true" />
          </button>
        </header>

        <div class="ifb-toolbar">
          <AppSelect
            v-if="extensions.length > 0"
            v-model="selectedExt"
            class="ifb-select"
            :options="extOptions"
            aria-label="选择扩展"
          />
          <button
            class="ghost-btn ifb-markall"
            type="button"
            :disabled="markingAll || totalUnread === 0"
            @click="markAllRead"
          >
            <CheckCheck :size="13" :stroke-width="2" aria-hidden="true" />
            全部标记已读
            <span v-if="totalUnread > 0" class="ifb-badge">{{ totalUnread }}</span>
          </button>
          <button class="ghost-btn ifb-refresh" type="button" :disabled="listLoading" @click="selectedExt && loadList(selectedExt)">
            刷新
          </button>
        </div>

        <div class="ifb-body">
          <div v-if="loading" class="ifb-state">正在加载…</div>
          <div v-else-if="error" class="ifb-state">
            {{ error }}
            <button class="ifb-link" type="button" @click="init">重试</button>
          </div>

          <div v-else-if="extensions.length === 0" class="ifb-empty">
            <Inbox :size="36" :stroke-width="1.5" aria-hidden="true" />
            <template v-if="signedIn">
              <h4>你还没有发布到市场的扩展</h4>
              <p>扩展上架后，用户提交的反馈会出现在这里。</p>
            </template>
            <template v-else>
              <h4>登录后可查看扩展反馈</h4>
              <p>反馈收件箱跟账号绑定：在「设置 → 账号」登录后，你发布的扩展收到的反馈会集中在这里。</p>
            </template>
          </div>

          <div v-else-if="listLoading" class="ifb-state">正在加载反馈…</div>
          <div v-else-if="listError" class="ifb-state">
            {{ listError }}
            <button class="ifb-link" type="button" @click="loadList(selectedExt)">重试</button>
          </div>

          <div v-else-if="items.length === 0" class="ifb-empty">
            <MessageSquare :size="36" :stroke-width="1.5" aria-hidden="true" />
            <h4>「{{ selectedExtension?.name || selectedExt }}」还没有反馈</h4>
            <p>用户可以在市场详情里写反馈，提交后会出现在这里。</p>
          </div>

          <div v-else class="ifb-list">
            <article v-for="f in items" :key="f.id" class="ifb-item" :class="{ hidden: f.hidden }">
              <div class="ifb-top">
                <span class="ifb-type" :class="f.kind">{{ feedbackKindLabel(f.kind) }}</span>
                <span class="ifb-author" :class="{ anon: !f.author_name }">
                  {{ f.author_name || '匿名用户' }}
                </span>
                <span class="ifb-time">{{ feedbackTimeText(f.created_at, nowTick) }}</span>
                <span class="ifb-stat" :class="f.status">{{ feedbackStatusLabel(f.status) }}</span>
                <span v-if="f.hidden" class="ifb-hidden-tag">已隐藏</span>
              </div>

              <p class="ifb-body-text">{{ f.body }}</p>

              <div class="ifb-meta">
                <span v-if="f.contact" class="ifb-chip">
                  <Mail :size="10" :stroke-width="2" aria-hidden="true" />{{ f.contact }}
                </span>
                <span v-else class="ifb-chip muted">
                  <UserX :size="10" :stroke-width="2" aria-hidden="true" />未留联系方式
                </span>
                <button
                  v-if="f.diagnostics"
                  class="ifb-chip ifb-chip-btn"
                  type="button"
                  @click="toggleDiag(f.id)"
                >
                  <Cpu :size="10" :stroke-width="2" aria-hidden="true" />
                  诊断信息 · {{ diagOpen.has(f.id) ? '收起' : '查看' }}
                </button>
                <span v-if="f.app_version" class="ifb-chip">
                  <AppWindow :size="10" :stroke-width="2" aria-hidden="true" />x-hub {{ f.app_version }}
                </span>
                <span v-if="f.ext_version" class="ifb-chip">扩展 v{{ f.ext_version }}</span>
              </div>

              <pre v-if="f.diagnostics && diagOpen.has(f.id)" class="ifb-diag">{{ f.diagnostics }}</pre>

              <div v-if="f.reply" class="ifb-reply">
                <span class="ifb-reply-label">
                  <Reply :size="11" :stroke-width="2" aria-hidden="true" />我的回复
                </span>
                <p>{{ f.reply }}</p>
              </div>

              <div class="ifb-actions">
                <button
                  v-if="f.status !== 'resolved'"
                  class="ghost-btn ifb-small"
                  type="button"
                  @click="openReply(f)"
                >
                  <Reply :size="12" :stroke-width="2" aria-hidden="true" />
                  {{ f.reply ? '再次回复' : '回复' }}
                </button>
                <button
                  class="ghost-btn ifb-small"
                  :class="{ danger: hideConfirmId === f.id }"
                  type="button"
                  :disabled="busyId === f.id"
                  @click="toggleHide(f)"
                >
                  <component
                    :is="f.hidden ? Eye : EyeOff"
                    :size="12"
                    :stroke-width="2"
                    aria-hidden="true"
                  />
                  {{ hideConfirmId === f.id ? '确认隐藏？' : f.hidden ? '恢复显示' : '隐藏' }}
                </button>
                <span v-if="f.status === 'resolved'" class="ifb-resolved">
                  <CheckCheck :size="11" :stroke-width="2" aria-hidden="true" />已解决
                </span>
              </div>

              <div v-if="replyOpenId === f.id" class="ifb-reply-box">
                <textarea
                  v-model="replyDraft[f.id]"
                  class="field-input ifb-textarea"
                  maxlength="500"
                  placeholder="回复会公开显示在这条反馈下方…"
                />
                <div class="ifb-reply-foot">
                  <label class="ifb-check">
                    <input v-model="resolveOnReply[f.id]" type="checkbox" />
                    同时标记为已解决
                  </label>
                  <button class="ghost-btn" type="button" @click="replyOpenId = null">取消</button>
                  <button
                    class="pill-btn"
                    type="button"
                    :disabled="busyId === f.id"
                    @click="sendReply(f)"
                  >
                    {{ busyId === f.id ? '发送中…' : '发送回复' }}
                  </button>
                </div>
              </div>
            </article>
          </div>
        </div>

        <footer class="ifb-foot">
          <span>公开板上所有人可见反馈正文与你的回复；联系方式与诊断信息仅你与平台管理员可见。</span>
          <button class="ghost-btn" type="button" @click="close">关闭</button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ifb-card {
  width: min(760px, 92vw);
  max-width: calc(100vw - 40px);
  max-height: min(84vh, 780px);
  padding: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.ifb-head {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 18px 18px 12px;
  flex-shrink: 0;
}
.ifb-ico {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  flex-shrink: 0;
  background: var(--brand-50);
  color: var(--brand-500);
  display: flex;
  align-items: center;
  justify-content: center;
}
.ifb-tt {
  flex: 1;
  min-width: 0;
}
.ifb-tt h3 {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 650;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.ifb-tt p {
  margin: 2px 0 0;
  font-size: 0.6875rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ifb-x {
  width: 26px;
  height: 26px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-3);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  flex-shrink: 0;
}
.ifb-x:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.ifb-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 18px 12px;
  border-bottom: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.ifb-select {
  width: 250px;
}
.ifb-markall {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.ifb-badge {
  font-size: 0.625rem;
  font-weight: 700;
  padding: 0 5px;
  border-radius: var(--radius-pill);
  background: var(--c-red);
  color: #fff;
  line-height: 1.5;
}
.ifb-refresh {
  margin-left: auto;
}
.ifb-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 18px;
}
.ifb-state {
  padding: 28px 0;
  text-align: center;
  font-size: 0.8125rem;
  color: var(--text-3);
}
.ifb-link {
  margin-left: 6px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--brand-500);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
}
.ifb-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 42px 16px;
  text-align: center;
  color: var(--text-4, var(--text-3));
}
.ifb-empty h4 {
  margin: 4px 0 0;
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-2);
}
.ifb-empty p {
  margin: 0;
  max-width: 380px;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-3);
}
.ifb-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ifb-item {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 10px 13px;
  background: var(--bg-card-soft);
  transition: opacity 150ms ease-out;
}
.ifb-item.hidden {
  opacity: 0.6;
}
.ifb-top {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-wrap: wrap;
  margin-bottom: 4px;
}
.ifb-type {
  font-size: 0.625rem;
  font-weight: 650;
  padding: 1px 6px;
  border-radius: 5px;
}
.ifb-type.bug {
  background: color-mix(in srgb, var(--c-red) 12%, transparent);
  color: var(--c-red);
}
.ifb-type.idea {
  background: color-mix(in srgb, var(--c-orange) 15%, transparent);
  color: var(--c-orange);
}
.ifb-type.question {
  background: var(--brand-50);
  color: var(--brand-500);
}
.ifb-author {
  font-size: 0.6875rem;
  font-weight: 650;
  color: var(--text-2);
}
.ifb-author.anon {
  font-weight: 500;
  color: var(--text-3);
}
.ifb-time {
  font-size: 0.625rem;
  color: var(--text-3);
}
.ifb-stat {
  margin-left: auto;
  font-size: 0.625rem;
  font-weight: 650;
  padding: 1px 6px;
  border-radius: var(--radius-pill);
}
.ifb-stat.open {
  background: var(--bg-card-soft);
  color: var(--text-3);
  border: 1px solid var(--border-soft);
}
.ifb-stat.replied,
.ifb-stat.resolved {
  background: color-mix(in srgb, var(--c-green) 14%, transparent);
  color: var(--c-green);
}
.ifb-hidden-tag {
  font-size: 0.625rem;
  font-weight: 650;
  padding: 1px 6px;
  border-radius: var(--radius-pill);
  background: color-mix(in srgb, var(--c-orange) 14%, transparent);
  color: var(--c-orange);
}
.ifb-body-text {
  margin: 0;
  font-size: 0.8125rem;
  line-height: 1.6;
  color: var(--text-2);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.ifb-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 7px;
}
.ifb-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  font-size: 0.625rem;
  color: var(--text-2);
}
.ifb-chip.muted {
  color: var(--text-3);
}
.ifb-chip-btn {
  cursor: pointer;
  font-family: inherit;
}
.ifb-chip-btn:hover {
  color: var(--brand-500);
  border-color: var(--brand-500);
}
.ifb-diag {
  margin: 7px 0 0;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  font-family: Consolas, 'Courier New', monospace;
  font-size: 0.625rem;
  line-height: 1.7;
  color: var(--text-3);
  white-space: pre;
  overflow-x: auto;
}
.ifb-reply {
  margin-top: 8px;
  padding: 6px 9px;
  border-left: 2px solid var(--brand-500);
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  background: var(--brand-50);
}
.ifb-reply-label {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 0.625rem;
  font-weight: 700;
  color: var(--brand-500);
  margin-bottom: 2px;
}
.ifb-reply p {
  margin: 0;
  font-size: 0.75rem;
  line-height: 1.55;
  color: var(--text-2);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.ifb-actions {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-top: 8px;
}
.ifb-small {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px;
  font-size: 0.6875rem;
}
.ifb-small.danger {
  color: var(--c-red);
  border-color: color-mix(in srgb, var(--c-red) 45%, transparent);
}
.ifb-resolved {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 0.625rem;
  font-weight: 650;
  color: var(--c-green);
}
.ifb-reply-box {
  margin-top: 9px;
  padding-top: 9px;
  border-top: 1px dashed var(--border-soft);
}
.ifb-textarea {
  display: block;
  height: 64px;
  resize: none;
  line-height: 1.6;
  margin-bottom: 8px;
}
.ifb-reply-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ifb-check {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.6875rem;
  color: var(--text-3);
  cursor: pointer;
}
.ifb-check input {
  width: 13px;
  height: 13px;
  accent-color: var(--brand-500);
  cursor: pointer;
}
.ifb-reply-foot .ghost-btn,
.ifb-reply-foot .pill-btn {
  margin-left: auto;
}
.ifb-reply-foot .ghost-btn + .pill-btn {
  margin-left: 0;
}
.ifb-foot {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 11px 18px;
  border-top: 1px solid var(--border-soft);
  flex-shrink: 0;
  font-size: 0.625rem;
  line-height: 1.5;
  color: var(--text-3);
}
.ifb-foot span {
  flex: 1;
  min-width: 0;
}
</style>
