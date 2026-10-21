<script setup lang="ts">
// AI 助手大类（2026-10-16 自「功能」大类整块拉出为一级设置项），分四个子项：
//   助手 —— 权限档与个性化（2026-10-17 自 AiProviders 迁入）
//   面板 —— 对话形态与面板行为
//   额度 —— 平台额度开关（2026-10-17 自 AiProviders 迁入）+ 积分余额（自「账号」并入）
//   供应商 —— 自备供应商与模型管理（AiProviders，平台条目经 prop 回传参与整体保存）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { isPlatformModel, isTauri, tauriApi, PLATFORM_ENTRY_NAME, type AccountStatus, type ChatModelConfig } from '../../api/tauri';
import AppSelect from '../AppSelect.vue';
import AiProviders from '../AiProviders.vue';
import { useStore } from '../../stores/workbench';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

function onChatPanelOpacityInput(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  void store.setChatPanelOpacity(v)
}

const CHAT_PANEL_SIDE_OPTIONS = [
  { value: 'right', label: '右侧' },
  { value: 'left', label: '左侧' },
  { value: 'top', label: '顶部' },
  { value: 'bottom', label: '底部' },
] as const

async function onChatPanelSideChange(value: string) {
  const side = value as 'left' | 'right' | 'top' | 'bottom'
  await store.setChatPanelSide(side)
  showToast(`AI 对话面板已改为从${['右侧', '左侧', '顶部', '底部'][['right', 'left', 'top', 'bottom'].indexOf(side)]}滑出`)
}

// AI 对话形态：独立小窗 / 主窗内嵌抽屉（互斥）。开关经后端专用命令落地建窗/隐窗
const chatWindowMode = computed(() => !!store.state.config.chat_window_mode)

async function onToggleChatWindowMode() {
  const next = !chatWindowMode.value
  try {
    await store.setChatWindowMode(next)
    showToast(next ? 'AI 对话已改为独立窗口打开' : 'AI 对话已改回主窗内嵌面板')
  } catch {
    showToast('切换失败，请重试')
  }
}

// ---- 平台额度开关（原 AiProviders 的 .ap-platform 行，2026-10-17 迁入「额度」子项） ----
// 形态：只是一个**开启/关闭**开关 —— 它不是供应商卡片（没有供应商名称/Base URL/
// API Key/测试连通/获取模型这些字段，用户也不需要关心平台侧有哪几个模型）。
// 写入的条目仍按 `platform:<模型名>` 存（后端靠这个前缀识别平台额度，见 chat.rs::is_platform_model），
// Key 写占位符「用账号会话换额度」，Rust 侧请求时再换成真实 token（chat.rs::PLATFORM_KEY_SENTINEL）。
// ⚠️ 平台 Key 一律不在界面展示/复制（占位符没有意义，真 token 也不该出 Rust）。
// AI 对话里这批条目折叠成「x-hub 平台」一个入口，具体用哪个模型由后端在多个平台模型之间负载切换。
// ⚠️ platformModels 会经 prop 传给 AiProviders 的 collectAll——供应商保存是整体覆盖写配置，
//    漏掉平台条目等于把用户开着的平台额度悄悄关掉。
const PLATFORM_SENTINEL = '__xhub_platform__'
const platformBusy = ref(false)
/// 平台条目：不渲染成卡片，但必须原样留在配置里（丢了等于关掉平台额度）
const platformModels = ref<ChatModelConfig[]>([])

const platformEnabled = computed(() => platformModels.value.length > 0)

/// 开关下方的状态说明：只说「开没开、平台现在有几个模型、对话里显示成什么」，
/// 具体是哪些模型不在这里列（用户不需要选，后端会负载切换）
const platformStatusText = computed(() => {
  if (!platformEnabled.value) {
    return '登录账号后即可使用平台提供的模型（走平台额度，不需要填 API Key）。开启后 AI 对话的模型列表里显示为「x-hub 平台」。'
  }
  const n = platformModels.value.length
  return n > 1
    ? `已开启 · 平台当前提供 ${n} 个模型，AI 对话里显示为「x-hub 平台」，多个模型之间自动负载切换`
    : `已开启 · 平台当前提供 ${n} 个模型，AI 对话里显示为「x-hub 平台」`
})

/// 平台模型的列表与地址只能从平台账号接口取。
///
/// 平台中转只实现 `POST /v1/chat/completions`，**没有** OpenAI 的 `GET {base}/models`
/// ——实测该路径恒 404（2026-09-17 用户反馈「连接失败：接口返回 404」）；
/// 平台自己的列表接口是 `/api/v1/ai/models`，鉴权走**账号登录态**而不是 API Key。
/// 所以平台额度一律走这里，绝不能套用自备供应商那条 `GET {base_url}/models` 的通用探测。
async function loadPlatformModels(): Promise<{ ids: string[]; baseUrl: string }> {
  const account = await tauriApi.accountStatus()
  if (!account.loggedIn) throw new Error('请先在「设置 → 账号」登录')
  // 服务端地址是内置常量（设置里没有地址入口），后端正常都会回非空值；
  // 这里只留防御性兜底，不再提示用户「去填地址」——那个入口已经不存在了
  const server = (account.serverUrl || '').replace(/\/+$/, '')
  if (!server) throw new Error('账号服务地址暂不可用，请稍后重试')
  const names = await tauriApi.platformModels()
  return { ids: names.map((n) => `platform:${n}`), baseUrl: `${server}/v1` }
}

/// 平台条目的统一构造：`platform:` 前缀是后端认出平台模型的依据（chat.rs::resolve_api_key），
/// Key 写占位符（请求时换成账号 token），base_url 后端也会现算（这里只存一份便于展示）。
function platformModel(id: string, baseUrl: string): ChatModelConfig {
  const name = id.replace(/^platform:/, '')
  return {
    id,
    name: `${name}（平台额度）`,
    base_url: baseUrl,
    model: name,
    api_key: PLATFORM_SENTINEL,
    is_default: false,
    has_api_key: true,
    provider_name: PLATFORM_ENTRY_NAME,
  }
}

/// 开启平台额度：拉平台当前可用模型并整体写入配置（平台加/减模型时重新开关一次即同步）
async function enablePlatform() {
  if (!isTauri() || platformBusy.value) return
  platformBusy.value = true
  try {
    const { ids, baseUrl } = await loadPlatformModels()
    if (!ids.length) {
      showToast('平台暂未开放任何模型')
      return
    }
    const existing = (await tauriApi.getChatModels()).filter((m) => !isPlatformModel(m))
    const saved = await tauriApi.saveChatModels([...existing, ...ids.map((id) => platformModel(id, baseUrl))])
    store.setChatModels(saved)
    // 只更新平台条目本身，**不回读整个列表**：开关与自备供应商列表毫无关系，
    // 回读会把卡片全部重建（展开态要重继承），还可能把用户正在填的新供应商卡片冲掉
    platformModels.value = saved.filter(isPlatformModel)
    showToast(
      ids.length > 1
        ? `平台额度已开启（${ids.length} 个模型，对话里按负载自动切换）`
        : '平台额度已开启',
    )
  } catch (e) {
    showToast(`开启失败：${e}`)
  } finally {
    platformBusy.value = false
  }
}

/// 关闭平台额度：把平台条目从配置里整体移除（对话里的「x-hub 平台」入口随之消失）
async function disablePlatform() {
  if (!isTauri() || platformBusy.value) return
  platformBusy.value = true
  const kept = platformModels.value
  platformModels.value = []
  try {
    const saved = await tauriApi.saveChatModels(await tauriApi.getChatModels().then((list) => list.filter((m) => !isPlatformModel(m))))
    store.setChatModels(saved)
    // 同上：平台条目已在前面就地清空（platformModels.value = []），无需回读整表
    showToast('平台额度已关闭')
  } catch (e) {
    // 保存失败就把条目放回去，别让界面显示成「已关闭」而配置里其实还在
    platformModels.value = kept
    showToast(`关闭失败：${e}`)
  } finally {
    platformBusy.value = false
  }
}

// ---- 助手：权限档与个性化（#t47：Rust send_agent_message 缺省回落这套持久化值） ----
// 2026-10-17 自 AiProviders 迁入「助手」子项
const AGENT_PERM_OPTIONS = [
  { value: 'readonly', label: '仅可查看' },
  { value: 'ask', label: '每次都问' },
  { value: 'full', label: '完全权限' },
] as const
type AgentPermMode = (typeof AGENT_PERM_OPTIONS)[number]['value']

const agentPermMode = ref<AgentPermMode>('ask')
const agentPromptOnDangerous = ref(false)
const agentAboutMe = ref('')
const agentExtTools = ref(true)
const agentSaving = ref(false)

function normalizePermMode(v: string | undefined): AgentPermMode {
  return v === 'readonly' || v === 'full' ? v : 'ask'
}

/** 权限档下拉换值：归一后即时落盘（约定 30：下拉一律 AppSelect，禁原生 select） */
function onPermSelect(v: string) {
  agentPermMode.value = normalizePermMode(v)
  void saveAgentSettings({ agent_permission_mode: agentPermMode.value })
}

/// 即时落盘（select/toggle 点下就存）；textarea 走失焦保存，避免每个按键都写盘
async function saveAgentSettings(patch: Parameters<typeof store.setAgentSettings>[0]) {
  agentSaving.value = true
  try {
    await store.setAgentSettings(patch)
  } catch (e) {
    showToast(`保存失败：${String(e)}`)
  } finally {
    agentSaving.value = false
  }
}

// ---- 积分余额（自「账号」大类并入）：只读展示；兑换邀请码的入口仍在「账号」大类 ----
const account = ref<AccountStatus | null>(null)
const accountFailed = ref(false)

onMounted(async () => {
  if (!isTauri()) return
  // 权限与个性化：从持久化配置读初始值（#t47）
  agentPermMode.value = normalizePermMode(store.state.config.agent_permission_mode)
  agentPromptOnDangerous.value = !!store.state.config.agent_prompt_on_dangerous
  agentAboutMe.value = store.state.config.agent_about_me ?? ''
  agentExtTools.value = store.state.config.agent_ext_tools_enabled !== false
  try {
    // 平台开关的初始状态与积分余额同源（getChatModels / accountStatus），挂载时一并拉取
    platformModels.value = (await tauriApi.getChatModels()).filter(isPlatformModel)
  } catch {
    // 后端不可用（浏览器预览）时保持关闭态
  }
  try {
    account.value = await tauriApi.accountStatus()
  } catch {
    accountFailed.value = true
  }
})
</script>

<template>
  <section id="sv-sec-assistant" class="sv-sec" aria-label="助手">
    <h3 class="sv-sec-title">助手</h3>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">AI 助手默认权限档</span>
        <span class="setting-desc">对话框里随时可切；这里决定每次打开时的初始档位</span>
      </div>
      <AppSelect
        class="ap-perm-select"
        :model-value="agentPermMode"
        :options="AGENT_PERM_OPTIONS.map((o) => ({ value: o.value, label: o.label, group: '' }))"
        :menu-min-width="132"
        aria-label="AI 助手默认权限档"
        :title="`AI 助手默认权限档：${AGENT_PERM_OPTIONS.find((o) => o.value === agentPermMode)?.label ?? ''}`"
        @update:model-value="onPermSelect"
      />
    </div>
    <div v-if="agentPermMode === 'full'" class="setting-row">
      <div class="setting-info">
        <span class="setting-name">危险操作仍需确认</span>
        <span class="setting-desc">开启后完全权限档下删除类、启动类操作仍会弹确认（默认关：完全权限 = 风险自担）</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="agentPromptOnDangerous"
        :class="{ on: agentPromptOnDangerous }"
        :disabled="agentSaving"
        @click="agentPromptOnDangerous = !agentPromptOnDangerous; saveAgentSettings({ agent_prompt_on_dangerous: agentPromptOnDangerous })"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">允许调用扩展提供的工具</span>
        <span class="setting-desc">service 扩展可在 manifest 里声明 AI 工具（数据查询等），AI 按其声明的档位受同一套审批约束；每个扩展还可单独在扩展详情的设置弹窗里关闭</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="agentExtTools"
        :class="{ on: agentExtTools }"
        :disabled="agentSaving"
        @click="agentExtTools = !agentExtTools; saveAgentSettings({ agent_ext_tools_enabled: agentExtTools })"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>
    <div class="setting-row">
      <div class="setting-info ap-about">
        <span class="setting-name">关于我</span>
        <span class="setting-desc">给 AI 的自画像（职业、偏好、常用上下文），对话时作为系统提示注入；留空不注入</span>
        <textarea
          v-model="agentAboutMe"
          class="field-input ap-about-input"
          rows="3"
          maxlength="1000"
          placeholder="如：我是后端工程师，常用 Go 和 TypeScript，回答偏简洁，给命令时用 PowerShell…"
          @blur="saveAgentSettings({ agent_about_me: agentAboutMe })"
        ></textarea>
      </div>
    </div>
  </section>

  <section id="sv-sec-ai" class="sv-sec" aria-label="面板">
    <h3 class="sv-sec-title">面板</h3>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">以独立窗口打开 AI 对话</span>
        <span class="setting-desc">开启后对话变为可缩放、可置顶的独立小窗：标题栏按钮、AI 对话呼出快捷键（设置 → 快捷键）与悬浮球「AI 对话」入口都唤起它，主窗内嵌抽屉随之停用（两种形态互斥）</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="chatWindowMode"
        :class="{ on: chatWindowMode }"
        @click="onToggleChatWindowMode"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">AI 对话面板透明度</span>
        <span class="setting-desc">{{ chatWindowMode ? '仅内嵌面板生效（当前为独立窗口形态）' : '对话抽屉的整体不透明度（50% – 100%）' }}</span>
      </div>
      <div class="opacity-edit">
        <input
          class="opacity-slider"
          type="range"
          min="0.5"
          max="1"
          step="0.05"
          :value="store.state.config.chat_panel_opacity ?? 1"
          :aria-label="'AI 对话面板透明度'"
          :disabled="chatWindowMode"
          @input="onChatPanelOpacityInput"
        />
        <span class="opacity-value">{{ Math.round((store.state.config.chat_panel_opacity ?? 1) * 100) }}%</span>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">AI 对话面板位置</span>
        <span class="setting-desc">{{ chatWindowMode ? '仅内嵌面板生效（独立窗口可自由拖动摆放）' : '对话抽屉从上下左右哪个方位滑出（左右方位可拖拽调宽，上下方位可拖拽调高）' }}</span>
      </div>
      <AppSelect
        :model-value="store.state.config.chat_panel_side ?? 'right'"
        :options="CHAT_PANEL_SIDE_OPTIONS"
        aria-label="AI 对话面板位置"
        class="chat-panel-side"
        :disabled="chatWindowMode"
        @update:model-value="onChatPanelSideChange"
      />
    </div>
  </section>

  <section id="sv-sec-credit" class="sv-sec" aria-label="额度">
    <h3 class="sv-sec-title">额度</h3>

    <!-- 平台额度开关（自 AiProviders 迁入，原「使用 x-hub 平台免费额度」去掉「免费」二字）：
         与自备供应商刻意区分——它不是供应商，没有需要用户填的东西 -->
    <div class="setting-row ap-platform">
      <div class="setting-info">
        <span class="setting-name">使用 x-hub 平台额度</span>
        <span class="setting-desc">{{ platformStatusText }}</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="platformEnabled"
        :class="{ on: platformEnabled }"
        :disabled="platformBusy"
        :title="platformEnabled ? '关闭后平台额度条目会从配置中移除' : '开启后自动拉取平台当前可用的模型'"
        @click="platformEnabled ? disablePlatform() : enablePlatform()"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">AI 额度（积分）</span>
        <span class="setting-desc">
          {{
            account
              ? account.inviteRedeemed
                ? `剩余 ${account.creditRemaining} 积分（共 ${account.creditTotal} 积分）· 平台模型按 token 折算扣积分`
                : '尚未兑换邀请码，当前没有额度；兑换入口在「账号」分类'
              : accountFailed
                ? '账号状态获取失败，请稍后重试'
                : '账号状态获取中…'
          }}
        </span>
      </div>
    </div>
  </section>

  <section id="sv-sec-providers" class="sv-sec" aria-label="供应商">
    <h3 class="sv-sec-title">供应商</h3>

    <!-- platformModels 经 prop 回传：供应商保存是整体覆盖写，平台条目必须原样带回 -->
    <AiProviders :platform-models="platformModels" />
  </section>
</template>

<style scoped>
/* 权限档下拉固定宽（自 AiProviders 迁入） */
.ap-perm-select {
  width: 132px;
  flex-shrink: 0;
}
/* 关于我：说明与输入框纵向排布 */
.ap-about {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ap-about-input {
  width: 100%;
  resize: vertical;
  min-height: 64px;
  font-size: 0.8125rem;
  line-height: 1.55;
}
/* 平台额度开关行（自 AiProviders 迁入）：与供应商卡片刻意区分——它不是供应商 */
.ap-platform {
  margin: 4px 0 14px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-soft);
}
/* 说明文字用满整行剩余宽度：不给 max-width（ch 单位对中文只有半个字宽，会把话提前截在半行处） */
.ap-platform .setting-info {
  flex: 1;
}
</style>
