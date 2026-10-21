<script setup lang="ts">
import { computed, inject, onBeforeUnmount, onMounted, ref, toRef, useId, watch } from 'vue'
import { Bug, CircleHelp, Globe, Info, Lightbulb, MessageSquare, Send, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type ExtensionFeedbackKind } from '../api/tauri'
import { useFocusTrap } from '../composables/useFocusTrap'
import { buildFeedbackDiagnostics, currentAppVersion } from '../utils/feedbackDiagnostics'
import { feedbackErrorText } from '../utils/feedback'

/**
 * 提交扩展反馈（平台托管 · 公开可见）。
 *
 * 三处入口共用同一个弹窗：市场详情、已安装扩展设置、扩展出错态。提交后反馈落在
 * x-hub-server，公开可见；开发者在「我的扩展 → 用户反馈」里看完整信息并回复。
 *
 * 两个隐私口径写死在界面上：
 *   · 正文与用户名会**公开**（顶部提示，用户必须在知情下发布）；
 *   · 联系方式与诊断信息**仅开发者与平台管理员可见**（公开接口不返回这两列）。
 */
const props = defineProps<{
  visible: boolean
  extId: string
  /** 扩展名（出错态只拿得到 extId 时留空，打开时现查） */
  extName?: string | null
  extVersion?: string | null
  /** web | service（诊断信息用） */
  runtime?: string | null
  /** 诊断信息默认勾选（仅出错态传 true；普通入口默认不勾，用户自己决定带不带） */
  defaultDiagnostics?: boolean
}>()

const emit = defineEmits<{
  (e: 'update:visible', value: boolean): void
  (e: 'submitted', id: number): void
}>()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const titleId = useId()

const TYPES: { value: ExtensionFeedbackKind; label: string; icon: typeof Bug }[] = [
  { value: 'bug', label: '缺陷', icon: Bug },
  { value: 'idea', label: '建议', icon: Lightbulb },
  { value: 'question', label: '提问', icon: CircleHelp },
]

const cardRef = ref<HTMLElement | null>(null)
const bodyRef = ref<HTMLTextAreaElement | null>(null)

const kind = ref<ExtensionFeedbackKind>('bug')
const body = ref('')
const contact = ref('')
const includeDiag = ref(true)
const diagOpen = ref(false)
const submitting = ref(false)
const errorMsg = ref('')

// 扩展信息：props 有就用，没有就按 extId 现查（出错态只有 extId）
const resolvedName = ref('')
const resolvedVersion = ref('')
const resolvedRuntime = ref('')
/** 本机开发扩展（「我的扩展」源码直挂）没有市场身份：服务端会判未知扩展，这里直接拦住 */
const isDevExtension = ref(false)
const appVersion = ref('')
const diagPreview = ref('')

const subtitle = computed(() => {
  const parts = [resolvedName.value || props.extId]
  if (resolvedVersion.value) parts.push(`v${resolvedVersion.value}`)
  if (resolvedRuntime.value) parts.push(resolvedRuntime.value)
  return parts.join(' · ')
})

const canSubmit = computed(
  () => !isDevExtension.value && body.value.trim().length > 0 && !submitting.value,
)

useFocusTrap(toRef(props, 'visible'), cardRef, bodyRef)

function close() {
  if (submitting.value) return
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
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

function diagnosticsText(): string {
  return buildFeedbackDiagnostics({
    appVersion: appVersion.value,
    extId: props.extId,
    extVersion: resolvedVersion.value,
    runtime: resolvedRuntime.value,
  })
}

/**
 * 打开即重置表单，并按需补齐扩展信息与诊断预览。
 * 每次打开递增令牌：快速关闭再打开时，上一轮在途的异步查询（扩展清单 / 版本号）
 * 回来后会发现自己已过期，不再往新一轮的表单里写。
 */
let initToken = 0

async function init() {
  const token = ++initToken
  kind.value = 'bug'
  body.value = ''
  contact.value = ''
  errorMsg.value = ''
  submitting.value = false
  diagOpen.value = false
  includeDiag.value = props.defaultDiagnostics === true
  resolvedName.value = props.extName ?? ''
  resolvedVersion.value = props.extVersion ?? ''
  resolvedRuntime.value = props.runtime ?? ''
  isDevExtension.value = false
  appVersion.value = ''
  diagPreview.value = ''

  const versionTask = currentAppVersion().then((v) => {
    if (token === initToken) appVersion.value = v
  })

  if (isTauri() && (!resolvedName.value || !resolvedVersion.value || !resolvedRuntime.value)) {
    try {
      const list = await tauriApi.listExtensions()
      if (token !== initToken) return
      const e = list.find((x) => x.id === props.extId)
      if (e) {
        if (!resolvedName.value) resolvedName.value = e.name
        if (!resolvedVersion.value) resolvedVersion.value = e.version
        if (!resolvedRuntime.value) resolvedRuntime.value = e.runtime
        isDevExtension.value = e.source === 'dev'
      }
    } catch {
      // 查不到（扩展已卸载）就只显示 extId：不影响提交，服务端会再校验
    }
  }

  await versionTask
  if (token !== initToken) return
  diagPreview.value = diagnosticsText()
}

watch(
  () => props.visible,
  (v) => {
    if (v) void init()
    // 关闭时让在途初始化作废（下一次打开会拿到新令牌）
    else initToken += 1
  },
)

/** 服务端错误是 `CODE: message`；网络类换成人话，业务文案直接透出（口径与收件箱共用） */
function friendlyError(raw: string): string {
  return feedbackErrorText(raw)
}

async function submit() {
  if (submitting.value) return
  const text = body.value.trim()
  if (!text) {
    errorMsg.value = '请先填写详细描述'
    return
  }
  if (!isTauri()) {
    errorMsg.value = '提交反馈需在桌面应用中使用'
    return
  }
  if (isDevExtension.value) {
    errorMsg.value = '本机开发扩展没有市场反馈渠道'
    return
  }
  submitting.value = true
  errorMsg.value = ''
  try {
    if (!appVersion.value) appVersion.value = await currentAppVersion()
    const id = await tauriApi.submitExtensionFeedback({
      extId: props.extId,
      kind: kind.value,
      body: text,
      contact: contact.value.trim() || undefined,
      diagnostics: includeDiag.value ? diagnosticsText() : undefined,
      appVersion: appVersion.value,
      extVersion: resolvedVersion.value || undefined,
    })
    showToast('反馈已提交，开发者可在收件箱看到')
    emit('submitted', id)
    emit('update:visible', false)
  } catch (e) {
    errorMsg.value = friendlyError(String(e))
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" role="presentation">
      <div
        ref="cardRef"
        class="modal-card fb-card"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="titleId"
      >
        <header class="fb-head">
          <div class="fb-ico">
            <MessageSquare :size="18" :stroke-width="2" aria-hidden="true" />
          </div>
          <div class="fb-tt">
            <h3 :id="titleId">反馈问题</h3>
            <p :title="subtitle">{{ subtitle }}</p>
          </div>
          <button class="fb-x" type="button" aria-label="关闭" @click="close">
            <X :size="16" :stroke-width="2" aria-hidden="true" />
          </button>
        </header>

        <div class="fb-scroll">
          <div class="fb-public">
            <Globe :size="14" :stroke-width="2" aria-hidden="true" />
            <span>
              这条反馈会<b>公开发布</b>在该扩展的反馈板，所有人可见（登录状态下会显示你的用户名）；开发者可在收件箱看到并回复。
            </span>
          </div>

          <div v-if="isDevExtension" class="fb-public fb-dev">
            <Info :size="14" :stroke-width="2" aria-hidden="true" />
            <span>
              这是「我的扩展」里的<b>本机开发扩展</b>，没有市场反馈渠道（服务端只接受已上架扩展的反馈）。
            </span>
          </div>

          <div class="field-label">反馈类型</div>
          <div class="fb-seg" role="radiogroup" aria-label="反馈类型">
            <button
              v-for="t in TYPES"
              :key="t.value"
              class="fb-seg-btn"
              :class="{ active: kind === t.value }"
              type="button"
              role="radio"
              :aria-checked="kind === t.value"
              @click="kind = t.value"
            >
              <component :is="t.icon" :size="13" :stroke-width="2" aria-hidden="true" />
              {{ t.label }}
            </button>
          </div>

          <div class="field-label fb-label-row">
            详细描述
            <span class="fb-counter" :class="{ warn: body.length > 450 }">{{ body.length }} / 500</span>
          </div>
          <textarea
            ref="bodyRef"
            v-model="body"
            class="field-input fb-body"
            maxlength="500"
            placeholder="发生了什么？复现步骤、期望结果…"
            aria-label="详细描述"
          />

          <div class="field-label fb-label-row">
            联系方式
            <span class="fb-hint">选填 · 仅开发者与管理员可见</span>
          </div>
          <input
            v-model="contact"
            class="field-input fb-contact"
            type="text"
            maxlength="100"
            placeholder="邮箱 / QQ / 其他，方便开发者回访"
            aria-label="联系方式（选填）"
          />

          <div class="fb-diag">
            <div class="fb-diag-head">
              <label class="fb-check">
                <input v-model="includeDiag" type="checkbox" />
                <span>附带诊断信息</span>
                <span class="fb-hint">仅开发者与管理员可见</span>
              </label>
              <button class="fb-link" type="button" @click="diagOpen = !diagOpen">
                {{ diagOpen ? '收起' : '查看' }}
              </button>
            </div>
            <pre v-if="diagOpen" class="fb-diag-pre">{{ diagPreview }}</pre>
          </div>

          <p v-if="errorMsg" class="fb-error" role="alert">{{ errorMsg }}</p>
        </div>

        <footer class="fb-foot">
          <button class="ghost-btn" type="button" :disabled="submitting" @click="close">取消</button>
          <button class="pill-btn fb-submit" type="button" :disabled="!canSubmit" @click="submit">
            <Send :size="13" :stroke-width="2" aria-hidden="true" />
            {{ submitting ? '提交中…' : '提交反馈' }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.fb-card {
  width: 470px;
  max-width: calc(100vw - 40px);
  max-height: min(86vh, 720px);
  padding: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.fb-head {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 18px 18px 12px;
  flex-shrink: 0;
}
.fb-ico {
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
.fb-tt {
  flex: 1;
  min-width: 0;
}
.fb-tt h3 {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 650;
  color: var(--text-1);
  letter-spacing: -0.01em;
}
.fb-tt p {
  margin: 2px 0 0;
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.fb-x {
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
.fb-x:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.fb-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0 18px 2px;
}
.fb-public {
  display: flex;
  gap: 7px;
  align-items: flex-start;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--brand-50);
  color: var(--brand-500);
  font-size: 0.6875rem;
  line-height: 1.55;
  margin-bottom: 14px;
}
.fb-public b {
  font-weight: 700;
}
.fb-dev {
  margin-top: -8px;
  background: color-mix(in srgb, var(--c-orange) 13%, transparent);
  color: var(--c-orange);
}
.fb-label-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.fb-counter {
  margin-left: auto;
  font-size: 0.6875rem;
  font-weight: 500;
  color: var(--text-3);
}
.fb-counter.warn {
  color: var(--c-orange);
}
.fb-hint {
  font-size: 0.625rem;
  font-weight: 500;
  color: var(--text-3);
}
.fb-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  margin-bottom: 13px;
}
.fb-seg-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 12px;
  border: 0;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.fb-seg-btn:hover {
  color: var(--text-1);
}
.fb-seg-btn.active {
  background: var(--brand-500);
  color: #fff;
}
.fb-body {
  display: block;
  height: 96px;
  resize: none;
  line-height: 1.6;
  margin-bottom: 13px;
}
.fb-contact {
  display: block;
  margin-bottom: 13px;
}
.fb-diag {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  padding: 9px 11px;
}
.fb-diag-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.fb-check {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-2);
  cursor: pointer;
}
.fb-check input {
  width: 14px;
  height: 14px;
  accent-color: var(--brand-500);
  cursor: pointer;
}
.fb-link {
  margin-left: auto;
  padding: 0;
  border: 0;
  background: none;
  color: var(--brand-500);
  font-size: 0.6875rem;
  font-weight: 600;
  cursor: pointer;
}
.fb-link:hover {
  text-decoration: underline;
}
.fb-diag-pre {
  margin: 8px 0 0;
  padding-top: 8px;
  border-top: 1px dashed var(--border-soft);
  font-family: Consolas, 'Courier New', monospace;
  font-size: 0.625rem;
  line-height: 1.75;
  color: var(--text-3);
  white-space: pre;
  overflow-x: auto;
}
.fb-error {
  margin: 10px 0 0;
  font-size: 0.75rem;
  line-height: 1.5;
  color: var(--c-red);
}
.fb-foot {
  display: flex;
  justify-content: flex-end;
  gap: 9px;
  padding: 14px 18px 16px;
  border-top: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.fb-submit {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
