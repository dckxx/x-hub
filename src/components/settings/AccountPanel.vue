<script setup lang="ts">
// 账号大类（登录 / 额度 / 开发者申请 / 我的设备）
//
// 平台服务端地址是内置常量（`config::DEFAULT_SERVER_URL`），设置里**不提供地址入口**：
// 正式域名启用后用户无需也无法改地址（改地址只会让账号功能指向失效的服务端，见约定 52）。
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { inject, onBeforeUnmount, onMounted, ref } from 'vue';
import { Copy } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { AccountDevice, AccountStatus, GithubDeviceStart } from '../../api/tauri';
import { reportClientError } from '../../utils/error-report';

const showToast = inject<(msg: string) => void>('showToast', () => {})

// ---- 平台账号（登录 / 额度 / 开发者申请） ----
// 登录只服务「申请开发者 + 发布扩展」：装扩展、用自带 API Key 的 AI 对话都不需要账号。
const account = ref<AccountStatus | null>(null)
const accountBusy = ref(false)
const ghDevice = ref<GithubDeviceStart | null>(null)
/** 发起登录的进行中态：服务端要代调 GitHub（自带 15s 超时），期间按钮必须立刻给出可见反馈 */
const ghStarting = ref(false)

/**
 * 邮箱验证码登录入口**已开启**（2026-09-18）：服务端 SMTP 就绪，`POST /api/v1/auth/email/send`
 * 正常发码（6 位数字、10 分钟有效、同一邮箱每小时最多 5 封）。
 *
 * 它是国内网络下的**兜底登录方式**：GitHub 链路要服务端出网到 github.com / api.github.com，
 * 国内机房会间歇性失败，邮箱收码不依赖任何境外链路。
 *
 * 曾以 `EMAIL_LOGIN_ENABLED: boolean = false` + 模板里的 settings-index skip 标记
 * 临时隐藏（服务端没配发信时，入口只会让用户点一次撞一次 503）；现在两者一并移除 ——
 * 若哪天服务端又没有发信，界面上会就地显示「服务器尚未配置发信」，不再是静默失效。
 */

/**
 * 登录区的就地反馈（busy = 正在联系服务端；warn = 需要用户自己动手；error = 失败原因）。
 *
 * 为什么不用 toast 报错：服务端要**代调 GitHub**（它自己有 15s 超时），发起失败实测要 10s 上下才返回，
 * 而 toast 只显示 2.2s —— 用户点完「开始登录」干等十几秒，期间界面只有按钮文案变化，
 * 体感就是「点了没反应」（实测：服务端连不上 GitHub 时稳定 ~10.5s 返回 502，
 * toast 一闪即过，用户完全没看见）。所以登录结果就地在按钮下方常驻，直到下一次操作。
 */
const loginNotice = ref<{ kind: 'busy' | 'warn' | 'error'; text: string; raw: string } | null>(null)

/**
 * 后端错误是 `CODE: 说明` 形态（account.rs::api_error）：界面只展示说明部分，编码留在 title 里备查。
 * 少数几个码额外补一句「该找谁处理」——尤其 GITHUB_UNAVAILABLE 是**服务端出网问题**，
 * 不加说明用户只会以为是自己网络的问题，反复重试。
 */
function authErrorText(e: unknown): { text: string; raw: string } {
  const raw = String(e)
  const body = raw.replace(/^[A-Z_]+:\s*/, '')
  if (raw.startsWith('GITHUB_UNAVAILABLE')) {
    return { text: '登录服务暂时连不上 GitHub，请稍后再试（服务端网络问题，与你的网络无关）', raw }
  }
  if (raw.startsWith('NETWORK_ERROR')) {
    return { text: '连不上登录服务器，请检查网络后重试', raw }
  }
  // 轮询阶段的两个常见结果：用户在 GitHub 页点了「取消」，或等待窗口过期
  if (raw.includes('access_denied')) return { text: '你取消了 GitHub 授权', raw }
  if (raw.includes('expired_token') || raw.includes('session_expired')) {
    return { text: '这次登录等待已超时，请重新发起', raw }
  }
  // 服务端建会话失败（500 login_failed）：拿到的 message 是「登录失败，请稍后再试」，
  // 从这句话看不出是服务端出的问题 —— 补上归因与线索位置，否则用户只会以为是自己操作错了。
  if (raw.startsWith('LOGIN_FAILED')) {
    return { text: '服务端建立登录会话时出错（500），请稍后再试（服务端日志里有 [auth] 开头的详情）', raw }
  }
  return { text: body || raw, raw }
}

/**
 * 邮箱链路的错误文案：服务端已经把 message 写成可读句子（`ERROR_CODE: 说明`），
 * 这里只补几条「该怎么办」，否则用户对同一句话只会反复点重试。
 *
 * ⚠️ `RATE_LIMITED`（同一邮箱每小时 5 封）是最容易踩的一条：文案只说「发送过于频繁」时，
 * 用户的本能是立刻再点一次 —— 越点越久，而且这一小时内都没法登录。
 */
function emailErrorText(e: unknown): { text: string; raw: string } {
  const raw = String(e)
  if (raw.startsWith('RATE_LIMITED')) {
    return { text: '这个邮箱一小时内发得太多了（上限 5 封），请稍后再试，或换一个邮箱', raw }
  }
  if (raw.startsWith('EMAIL_SEND_FAILED')) {
    return { text: '邮件发送失败，请稍后再试（服务端发信出错，与你的邮箱无关）', raw }
  }
  if (raw.startsWith('EMAIL_NOT_CONFIGURED')) {
    return { text: '服务器尚未配置发信，暂时无法用邮箱登录', raw }
  }
  return authErrorText(e)
}

/**
 * 邀请码兑换的错误文案：错误码（INVITE_CODE_EXHAUSTED 等）对用户不可读，直接塞进 toast
 * 就是「兑换失败：INVITE_CODE_EXHAUSTED:服务端返回400」。服务端已补可读 message
 * （api_error 拼成 `CODE: 说明`，前缀在这里剥掉）；同时保留本地映射兜底——
 * 服务端发版前，老响应不带 message 也照样给可读句子。
 */
function redeemErrorText(e: unknown): { text: string; raw: string } {
  const raw = String(e)
  if (raw.startsWith('INVITE_CODE_EXHAUSTED')) {
    return { text: '该邀请码已被领完，请换一个邀请码', raw }
  }
  if (raw.startsWith('INVITE_CODE_REVOKED')) {
    return { text: '该邀请码已失效', raw }
  }
  if (raw.startsWith('INVITE_CODE_EXPIRED')) {
    return { text: '该邀请码已过期', raw }
  }
  if (raw.startsWith('INVITE_CODE_INVALID')) {
    return { text: '邀请码不存在，请检查是否输入有误', raw }
  }
  if (raw.startsWith('ALREADY_REDEEMED')) {
    return { text: '这个账号已经兑换过邀请码，无需重复兑换', raw }
  }
  if (raw.startsWith('INVITE_CODE_REQUIRED')) {
    return { text: '请填写邀请码', raw }
  }
  if (raw.startsWith('NETWORK_ERROR')) {
    return { text: '连不上服务器，请检查网络后重试', raw }
  }
  return authErrorText(e)
}

/** 发送成功后的重发冷却：连点不但会撞服务端限流，还会把「最新一封」的验证码换掉 */
function startEmailCooldown(sec: number) {
  emailCooldown.value = sec
  if (emailTimer !== null) clearInterval(emailTimer)
  emailTimer = window.setInterval(() => {
    emailCooldown.value -= 1
    if (emailCooldown.value <= 0) {
      emailCooldown.value = 0
      if (emailTimer !== null) {
        clearInterval(emailTimer)
        emailTimer = null
      }
    }
  }, 1000)
}

const emailInput = ref('')
const emailCode = ref('')
const emailSent = ref(false)
/** 发信请求在途（accountBusy 与 GitHub 链路共用，按钮文案要更精确的这一个） */
const emailSending = ref(false)
/** 重发冷却秒数：服务端按邮箱限流（每小时 5 封），连点会把配额一次耗光 */
const emailCooldown = ref(0)
let emailTimer: number | null = null
/** 邮箱链路的就地反馈（同 loginNotice：发信要走 SMTP，可能等数秒，只挂 toast 等于没有反馈） */
const emailNotice = ref<{ kind: 'busy' | 'warn' | 'error'; text: string; raw: string } | null>(null)
const redeemInput = ref('')
const showApply = ref(false)
const applyReason = ref('')
let ghTimer: number | null = null
/** GitHub 要求降低轮询频率（slow_down）时置位：界面据此换一句提示，别让用户以为卡死了 */
const ghSlow = ref(false)

/** 复制文本（剪贴板 API 不可用时回退 execCommand，与 AiProviders 同款做法） */
async function copyText(text: string, okMsg: string): Promise<void> {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    showToast(okMsg)
    return
  } catch {
    // 继续尝试 execCommand 回退
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } catch {
    ok = false
  }
  document.body.removeChild(ta)
  showToast(ok ? okMsg : '复制失败')
}

async function loadAccount() {
  if (!isTauri()) return
  try {
    account.value = await tauriApi.accountStatus()
    if (account.value.loggedIn) void loadDevices()
  } catch {
    // 后端不可用（浏览器预览）时静默
  }
}

// ---- 在线设备（多设备登录：换机不被顶下线，丢失时可单独撤销） ----
const devices = ref<AccountDevice[]>([])
const devicesMax = ref(5)
const devicesBusy = ref(false)

async function loadDevices() {
  if (!isTauri()) return
  try {
    const r = await tauriApi.accountListDevices()
    devices.value = r.devices ?? []
    devicesMax.value = r.max ?? 5
  } catch {
    devices.value = []
  }
}

/**
 * 会话已失效（401）时的统一收尾：清过的 token 由 Rust 侧清掉，这里只需刷新界面。
 * 场景：在别处撤销了本机 / 服务端重置了 token —— 不刷新的话界面会一直显示「已登录」。
 */
async function handleAuthError(e: unknown): Promise<boolean> {
  if (String(e).includes('UNAUTHORIZED')) {
    await loadAccount()
    showToast('登录已失效，请重新登录')
    return true
  }
  return false
}

async function revokeDevice(id: number) {
  devicesBusy.value = true
  try {
    await tauriApi.accountRevokeDevice(id)
    await loadDevices()
    showToast('已撤销该设备')
  } catch (e) {
    if (await handleAuthError(e)) return
    showToast(`撤销失败：${e}`)
  } finally {
    devicesBusy.value = false
  }
}

function fmtDeviceTime(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getMonth() + 1}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

function stopGithubPolling() {
  if (ghTimer !== null) {
    clearTimeout(ghTimer)
    ghTimer = null
  }
}

/**
 * 轮询 GitHub 授权结果（按服务端给的 interval，最少 3 秒一次）。
 *
 * ⚠️ `slow_down` 必须被尊重：GitHub 收到过快轮询后会回它，并要求「之后所有请求间隔 +5s」。
 * 早期实现由服务端把它并进 pending 吞掉，于是客户端永远按原速轮询、永远不会减速——
 * 用户看到的现象是「浏览器里已经授权成功，客户端却一直等待授权」，而且服务端日志一片正常。
 */
function startGithubPolling(dev: GithubDeviceStart) {
  stopGithubPolling()
  let intervalMs = Math.max(3, dev.interval) * 1000
  const tick = async () => {
    try {
      const r = await tauriApi.accountLoginGithubPoll(dev.pollId)
      if (r.status === 'pending') {
        ghTimer = window.setTimeout(() => void tick(), intervalMs)
        return
      }
      if (r.status === 'slow_down') {
        intervalMs += 5000
        ghSlow.value = true
        ghTimer = window.setTimeout(() => void tick(), intervalMs)
        return
      }
      stopGithubPolling()
      ghDevice.value = null
      if (r.status === 'ok') {
        loginNotice.value = null
        showToast('登录成功')
        await loadAccount()
      } else {
        const { text, raw } = authErrorText(r.message ?? '未知原因')
        loginNotice.value = { kind: 'error', text: `登录失败：${text}`, raw }
        showToast(`登录失败：${text}`)
      }
    } catch (e) {
      stopGithubPolling()
      ghDevice.value = null
      const { text, raw } = authErrorText(e)
      loginNotice.value = { kind: 'error', text: `登录失败：${text}`, raw }
      showToast(`登录失败：${text}`)
      // 同发起失败：轮询请求本身报错（如服务端 500）要在客户端留痕
      void reportClientError('账号登录轮询失败', { error: raw })
    }
  }
  ghTimer = window.setTimeout(() => void tick(), intervalMs)
}

async function startGithubLogin() {
  if (ghStarting.value) return
  ghStarting.value = true
  accountBusy.value = true
  // 先落一条「正在联系」，否则从点击到服务端回话这段（最长 20s）界面没有任何变化
  loginNotice.value = { kind: 'busy', text: '正在联系登录服务…（最长约 20 秒）', raw: '' }
  const startedAt = Date.now()
  try {
    const dev = await tauriApi.accountLoginGithubStart()
    ghSlow.value = false
    ghDevice.value = dev
    loginNotice.value = null
    // 顺手把浏览器打开（用户仍可手动复制地址）；打不开就明说 —— 验证码在界面上、按钮也在，
    // 静默失败会让人以为「点了没反应」（和发起失败同一类体感问题）
    void tauriApi.openExternal(dev.verificationUri).catch(() => {
      if (ghDevice.value === dev) {
        loginNotice.value = { kind: 'warn', text: '没能自动打开浏览器，请点下面的「打开浏览器」继续', raw: '' }
      }
    })
    startGithubPolling(dev)
  } catch (e) {
    // 服务端代调 GitHub 失败时给的是可读文案（如「GITHUB_UNAVAILABLE: GitHub 暂时不可用，请稍后再试」）
    const { text, raw } = authErrorText(e)
    loginNotice.value = { kind: 'error', text: `发起登录失败：${text}`, raw }
    showToast(`发起登录失败：${text}`)
    // 落本地日志（含耗时）：账号链路横跨客户端 / 服务端 / GitHub 三方，
    // 没有这条记录时「服务端返回 500」在客户端侧不留任何痕迹，只能靠服务端日志——
    // 那台机器不一定够得着。耗时还能区分「超时」与「立刻报错」两种完全不同的故障。
    void reportClientError('账号登录发起失败', { error: raw, elapsedMs: Date.now() - startedAt })
  } finally {
    accountBusy.value = false
    ghStarting.value = false
  }
}

/** 取消 GitHub 登录等待：停掉轮询并收起验证码（否则点错了只能干等 15 分钟） */
function cancelGithubLogin() {
  stopGithubPolling()
  ghDevice.value = null
  ghSlow.value = false
  loginNotice.value = null
}

async function sendEmailCode() {
  const email = emailInput.value.trim()
  if (!email) {
    emailNotice.value = { kind: 'warn', text: '请先填写邮箱', raw: '' }
    return
  }
  if (emailSending.value || emailCooldown.value > 0) return
  emailSending.value = true
  accountBusy.value = true
  // 先落一条「正在发送」：发信要过 SMTP，弱网/对方服务器慢时要数秒，期间界面必须有变化
  emailNotice.value = { kind: 'busy', text: '正在发送验证码…', raw: '' }
  const startedAt = Date.now()
  try {
    const r = await tauriApi.accountLoginEmailSend(email)
    if (!r.ok) {
      // 服务端未配置发信（Rust 侧把 503 转成 ok=false + 说明）：显示出来而不是假装成功
      emailNotice.value = { kind: 'error', text: r.message ?? '发送失败', raw: '' }
      return
    }
    emailSent.value = true
    emailNotice.value = null
    showToast('验证码已发送，请查收邮件')
    startEmailCooldown(60)
  } catch (e) {
    const { text, raw } = emailErrorText(e)
    emailNotice.value = { kind: 'error', text, raw }
    showToast(`发送失败：${text}`)
    // 与 GitHub 链路同款留痕：发信失败横跨客户端 / 服务端 / 邮件服务商三方，客户端不留痕就无从排查
    void reportClientError('邮箱验证码发送失败', { error: raw, elapsedMs: Date.now() - startedAt })
  } finally {
    emailSending.value = false
    accountBusy.value = false
  }
}

async function verifyEmailCode() {
  const email = emailInput.value.trim()
  const code = emailCode.value.trim()
  if (!code) {
    emailNotice.value = { kind: 'warn', text: '请填写邮件里的 6 位验证码', raw: '' }
    return
  }
  accountBusy.value = true
  emailNotice.value = { kind: 'busy', text: '正在校验验证码…', raw: '' }
  try {
    account.value = await tauriApi.accountLoginEmailVerify(email, code)
    emailSent.value = false
    emailCode.value = ''
    emailNotice.value = null
    showToast('登录成功')
  } catch (e) {
    const { text, raw } = emailErrorText(e)
    emailNotice.value = { kind: 'error', text, raw }
    showToast(`登录失败：${text}`)
  } finally {
    accountBusy.value = false
  }
}

async function doLogout() {
  accountBusy.value = true
  try {
    stopGithubPolling()
    ghDevice.value = null
    account.value = await tauriApi.accountLogout()
    showToast('已退出登录')
  } catch (e) {
    showToast(`退出失败：${authErrorText(e).text}`)
  } finally {
    accountBusy.value = false
  }
}

async function doRedeem() {
  if (!redeemInput.value.trim()) {
    showToast('请填写邀请码')
    return
  }
  accountBusy.value = true
  try {
    account.value = await tauriApi.accountRedeem(redeemInput.value.trim())
    redeemInput.value = ''
    showToast('兑换成功，权益已到账')
  } catch (e) {
    const { text } = redeemErrorText(e)
    showToast(`兑换失败：${text}`)
  } finally {
    accountBusy.value = false
  }
}

async function doApply() {
  if (applyReason.value.trim().length < 10) {
    showToast('请至少写 10 个字说明你想做什么扩展')
    return
  }
  accountBusy.value = true
  try {
    await tauriApi.devApply(applyReason.value.trim())
    showApply.value = false
    applyReason.value = ''
    showToast('申请已提交，等待审核')
    await loadAccount()
  } catch (e) {
    if (await handleAuthError(e)) return
    showToast(`提交失败：${authErrorText(e).text}`)
  } finally {
    accountBusy.value = false
  }
}

function accountSummary(a: AccountStatus): string {
  if (a.error) return `暂时连不上服务器：${a.error}`
  if (a.developerStatus === 'approved') return '扩展开发者'
  if (a.developerStatus === 'pending') return '开发者申请审核中'
  if (a.developerStatus === 'rejected') return '开发者申请未通过'
  return a.inviteRedeemed ? '已兑换邀请码' : '尚未兑换邀请码'
}

onMounted(() => {
  void loadAccount()
})
onBeforeUnmount(() => {
  stopGithubPolling()
  if (emailTimer !== null) clearInterval(emailTimer)
})
</script>

<template>
        <section id="sv-sec-account" class="sv-sec" aria-label="账号">
          <h3 class="sv-sec-title">账号</h3>
          <p class="account-intro">
            登录仅用于「申请成为扩展开发者」和「发布扩展」。安装扩展、用自己的 API Key 对话都不需要账号。
          </p>

          <template v-if="account && !account.loggedIn">
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">用 GitHub 登录</span>
                <span class="setting-desc">在浏览器里输入验证码即可，不用记密码</span>
              </div>
              <button
                class="ghost-btn data-btn"
                type="button"
                :disabled="accountBusy || !!ghDevice"
                @click="startGithubLogin"
              >
                {{ ghStarting ? '正在发起…' : ghDevice ? '等待授权…' : '开始登录' }}
              </button>
            </div>
            <!-- 登录结果就地常驻（服务端代调 GitHub 要 10s 上下，只靠 2.2s 的 toast 等于没有反馈，见 loginNotice 注释） -->
            <p
              v-if="loginNotice"
              class="account-notice"
              :class="loginNotice.kind"
              :title="loginNotice.raw"
              role="status"
            >
              {{ loginNotice.text }}
            </p>
            <div v-if="ghDevice" class="account-device">
              <span>浏览器里输入验证码</span>
              <b class="account-code">{{ ghDevice.userCode }}</b>
              <button
                class="account-copy"
                type="button"
                title="复制验证码"
                aria-label="复制验证码"
                @click="copyText(ghDevice.userCode, '验证码已复制')"
              >
                <Copy :size="13" :stroke-width="2" />
              </button>
              <button
                class="ghost-btn"
                type="button"
                @click="tauriApi.openExternal(ghDevice.verificationUri)"
              >
                打开浏览器
              </button>
              <button
                class="ghost-btn"
                type="button"
                @click="cancelGithubLogin"
              >
                取消
              </button>
              <span class="dev-dir-warn">
                {{ ghSlow ? 'GitHub 要求降低频率，已自动放慢…' : '等待授权…' }}
              </span>
            </div>

            <!-- 邮箱验证码登录：国内网络下的兜底方式（GitHub 链路要服务端出网到境外，会间歇性失败）。
                 服务端发信已就绪；万一哪天又没配，Rust 侧会把 503 转成下面那条就地提示，不静默失败。 -->
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">用邮箱验证码登录</span>
                <span class="setting-desc">验证码 10 分钟内有效，收不到时先看看垃圾邮件</span>
              </div>
              <div class="account-inline">
                <input v-model="emailInput" class="account-input" placeholder="you@example.com" />
                <button
                  class="ghost-btn data-btn"
                  type="button"
                  :disabled="accountBusy || emailCooldown > 0"
                  @click="sendEmailCode"
                >
                  {{ emailCooldown > 0 ? `${emailCooldown}s 后可重发` : emailSending ? '正在发送…' : '发验证码' }}
                </button>
              </div>
            </div>
            <div v-if="emailSent" class="account-inline account-inline-end">
              <input
                v-model="emailCode"
                class="account-input account-input-sm"
                placeholder="6 位验证码"
                maxlength="6"
              />
              <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="verifyEmailCode">
                登录
              </button>
            </div>
            <!-- 邮箱链路的结果就地常驻（发信要过 SMTP，且服务端按邮箱限流每小时 5 封，只靠 toast 会「点了没反应」） -->
            <p
              v-if="emailNotice"
              class="account-notice"
              :class="emailNotice.kind"
              :title="emailNotice.raw"
              role="status"
            >
              {{ emailNotice.text }}
            </p>
          </template>

          <template v-else-if="account?.loggedIn">
            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">{{ account.username }}</span>
                <span class="setting-desc">{{ accountSummary(account) }}</span>
              </div>
              <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doLogout">
                退出登录
              </button>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">AI 额度</span>
                <span class="setting-desc">
                  {{
                    account.inviteRedeemed
                      ? `剩余 ${account.quotaRemaining} 次（共 ${account.quotaTotal} 次）`
                      : '尚未兑换邀请码，当前没有额度'
                  }}
                </span>
              </div>
            </div>

            <div v-if="!account.inviteRedeemed" class="setting-row">
              <div class="setting-info">
                <span class="setting-name">兑换邀请码</span>
                <span class="setting-desc">兑换后才发放额度，并开放「申请成为开发者」</span>
              </div>
              <div class="account-inline">
                <input v-model="redeemInput" class="account-input" placeholder="邀请码" />
                <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doRedeem">
                  兑换
                </button>
              </div>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">扩展开发者</span>
                <span class="setting-desc">
                  {{
                    account.developerStatus === 'approved'
                      ? '已通过：可在扩展中心发布扩展'
                      : account.developerStatus === 'pending'
                        ? '申请审核中'
                        : account.developerStatus === 'rejected'
                          ? '申请未通过（可在扩展中心重新提交）'
                          : account.canApplyDeveloper
                            ? '还没有申请'
                            : '先兑换邀请码才能申请'
                  }}
                </span>
              </div>
              <button
                v-if="account.canApplyDeveloper"
                class="ghost-btn data-btn"
                type="button"
                :disabled="accountBusy"
                @click="showApply = !showApply"
              >
                申请成为开发者
              </button>
            </div>

            <div v-if="showApply" class="account-apply">
              <textarea
                v-model="applyReason"
                class="account-textarea"
                rows="3"
                placeholder="说说你想做什么扩展、为什么需要相应权限（审核时看这段）"
              ></textarea>
              <div class="account-inline account-inline-end">
                <button class="ghost-btn" type="button" @click="showApply = false">取消</button>
                <button class="ghost-btn data-btn" type="button" :disabled="accountBusy" @click="doApply">
                  提交申请
                </button>
              </div>
            </div>

            <div class="setting-row">
              <div class="setting-info">
                <span class="setting-name">我的设备</span>
                <span class="setting-desc">
                  同一账号最多 {{ devicesMax }} 台同时在线（换机不会被顶下线）；设备丢失时可单独撤销
                </span>
              </div>
              <button class="ghost-btn" type="button" :disabled="devicesBusy" @click="loadDevices">
                刷新
              </button>
            </div>
            <p v-if="!devices.length" class="account-device-empty">暂无设备记录</p>
            <div v-for="d in devices" :key="d.id" class="account-device-item">
              <span class="account-device-name">
                {{ d.label || '未命名设备' }}
                <b v-if="d.current" class="account-device-self">本机</b>
              </span>
              <span class="account-device-meta">最近使用 {{ fmtDeviceTime(d.last_seen_at) }}</span>
              <button
                v-if="!d.current"
                class="ghost-btn"
                type="button"
                :disabled="devicesBusy"
                @click="revokeDevice(d.id)"
              >
                撤销
              </button>
              <span v-else class="account-device-meta">下线请用「退出登录」</span>
            </div>
          </template>
        </section>
</template>
