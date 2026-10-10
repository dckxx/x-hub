<script setup lang="ts">
import { computed, inject, onMounted, ref } from 'vue'
import { ChevronDown, Copy, Eye, EyeOff, FlaskConical, ListPlus, Pencil, Plus, Trash2, X } from 'lucide-vue-next'
import { PLATFORM_ENTRY_NAME, isPlatformModel, isTauri, tauriApi, type ChatModelConfig } from '../api/tauri'
import { useStore } from '../stores/workbench'

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 供应商编辑态（一个供应商 = 一个 base_url + 共享 API Key + 多个模型） ----
interface ProviderEdit {
  key: string
  providerName: string
  baseUrl: string
  apiKey: string
  hasApiKey: boolean
  models: ChatModelConfig[]
  expanded: boolean
  fetched: string[]
  selected: Set<string>
  /** 手动添加模型输入框的草稿（与在线拉取并存，见 addManualModel） */
  manualModel: string
  busy: boolean
  msg: string
  savedKey: string
  keyVisible: boolean
  editingKey: boolean
}

const providers = ref<ProviderEdit[]>([])
const loading = ref(false)
const saving = ref(false)

// ---- 平台免费额度（登录后可直接用，不需要自备 API Key） ----
// 形态：设置里只是一个**开启/关闭**开关 —— 它不渲染成供应商卡片（没有供应商名称/Base URL/
// API Key/测试连通/获取模型这些字段，用户也不需要关心平台侧有哪几个模型）。
// 写入的条目仍按 `platform:<模型名>` 存（后端靠这个前缀识别平台额度，见 chat.rs::is_platform_model），
// Key 写占位符「用账号会话换额度」，Rust 侧请求时再换成真实 token（chat.rs::PLATFORM_KEY_SENTINEL）。
// ⚠️ 平台 Key 一律不在界面展示/复制（占位符没有意义，真 token 也不该出 Rust）。
// AI 对话里这批条目折叠成「x-hub 平台」一个入口，具体用哪个模型由后端在多个平台模型之间负载切换。
const PLATFORM_SENTINEL = '__xhub_platform__'
const platformBusy = ref(false)
/// 平台条目：不进 providers（不渲染成卡片），但必须原样留在配置里（丢了等于关掉平台额度）
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
/// ——实测该路径恒 404 `{"error":"not_found"}`（2026-09-17 用户反馈「连接失败：接口返回 404」）；
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
    const existing = collectAll().filter((m) => !isPlatformModel(m))
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
    const saved = await tauriApi.saveChatModels(collectAll())
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

function groupKey(m: ChatModelConfig): string {
  const name = (m.provider_name ?? '').trim()
  const base = (m.base_url ?? '').trim()
  // 一个供应商 = 一个供应商名称 + 多个模型；分组以名称为准：
  // 多账号可能共用同一个 base_url，但供应商名称不同，必须各自成组
  return name || base
}

function toProvider(m: ChatModelConfig): ProviderEdit {
  return {
    key: groupKey(m),
    providerName: (m.provider_name ?? '').trim(),
    baseUrl: (m.base_url ?? '').trim(),
    apiKey: '',
    hasApiKey: m.has_api_key,
    models: [m],
    expanded: false,
    fetched: [],
    selected: new Set(),
    manualModel: '',
    busy: false,
    msg: '',
    savedKey: '',
    keyVisible: false,
    editingKey: false,
  }
}

/// 拉取模型配置并重建供应商卡片列表。
///
/// `silent` = 后台回读（开关切换 / 保存配置之后）：**不进 loading 态**，否则模板会把整块
/// 卡片列表换成「加载中…」再换回来 —— 用户看到的就是开关点下去时界面闪一下。
/// 首次挂载与显式 reload 才走 loading（那时列表本来就还没内容，占位是诚实的）。
async function loadProviders(opts: { silent?: boolean } = {}) {
  if (!isTauri()) return
  if (!opts.silent) loading.value = true
  try {
    const list = await tauriApi.getChatModels()
    // 平台条目单独摘出来（设置里它只是一个开关，不渲染成供应商卡片），其余按自备供应商分组
    platformModels.value = list.filter(isPlatformModel)
    const prev = new Map(providers.value.map((p) => [p.key, p]))
    const map = new Map<string, ProviderEdit>()
    for (const m of list) {
      if (isPlatformModel(m)) continue
      const p = toProvider(m)
      if (map.has(p.key)) {
        map.get(p.key)!.models.push(m)
      } else {
        map.set(p.key, p)
      }
    }
    // 重建卡片时继承已在界面上的展开态/勾选态：开关切换、保存配置都会回读一次，
    // 若每次回读都把卡片收回折叠态，用户会看到自己刚展开的卡片"自己关上了"
    providers.value = [...map.values()].map((p) => {
      const old = prev.get(p.key)
      if (!old) return p
      return {
        ...p,
        expanded: old.expanded,
        fetched: old.fetched,
        selected: old.selected,
        manualModel: old.manualModel,
        msg: old.msg,
        savedKey: old.savedKey,
        keyVisible: old.keyVisible,
      }
    })
  } catch (e) {
    showToast(`加载失败：${String(e)}`)
  } finally {
    if (!opts.silent) loading.value = false
  }
}

function addProvider() {
  providers.value.push({
    key: 'p' + Date.now(),
    providerName: '',
    baseUrl: '',
    apiKey: '',
    hasApiKey: false,
    models: [],
    expanded: true,
    fetched: [],
    selected: new Set(),
    manualModel: '',
    busy: false,
    msg: '',
    savedKey: '',
    keyVisible: false,
    editingKey: false,
  })
}

function removeProvider(index: number) {
  providers.value.splice(index, 1)
}

function expand(p: ProviderEdit) {
  p.expanded = !p.expanded
  if (p.expanded && p.models.length > 0 && !p.hasApiKey) {
    p.hasApiKey = p.models.some((m) => m.has_api_key)
  }
  if (p.expanded && p.hasApiKey && !p.savedKey) {
    void loadSavedKey(p)
  }
}

// 拉取已保存的 API Key（脱敏展示/查看/复制用；真实 Key 存钥匙串，仅本机读取）
async function loadSavedKey(p: ProviderEdit) {
  if (!isTauri()) return
  const id = p.models.find((m) => m.has_api_key)?.id ?? p.models[0]?.id
  if (!id) return
  try {
    p.savedKey = await tauriApi.getChatApiKey(id)
  } catch {
    p.savedKey = ''
  }
}

// 中间脱敏：保留首 4 尾 4 字符，中间以星号掩盖
function maskKey(k: string): string {
  if (!k) return '未读取到 Key'
  if (k.length <= 8) return '•'.repeat(k.length)
  return k.slice(0, 4) + '•'.repeat(k.length - 8) + k.slice(-4)
}

// 复制文本（剪贴板 API 不可用时回退隐藏 textarea + execCommand）
async function copyText(text: string, okMsg: string): Promise<void> {
  if (!text) {
    showToast('当前没有可复制的内容')
    return
  }
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

// 复制当前 Key
function copyKey(p: ProviderEdit) {
  const k = p.apiKey.trim() || p.savedKey
  void copyText(k, 'API Key 已复制')
}

// 复制模型名称（tag 形式展示）
function copyModelName(m: ChatModelConfig) {
  void copyText(m.model || m.name, '模型名称已复制')
}

// 更换 Key 输入失焦时：未输入内容则回到脱敏展示
function finishKeyEdit(p: ProviderEdit) {
  if (p.hasApiKey && !p.apiKey.trim()) {
    p.editingKey = false
  }
}

// 编辑供应商字段时，同步到该供应商下的所有模型（模型共享 base_url / provider_name）
function syncProviderFields(p: ProviderEdit) {
  for (const m of p.models) {
    m.provider_name = p.providerName.trim()
    m.base_url = p.baseUrl.trim()
  }
}

// 供应商上填写的 API Key：保存时写入该供应商所有模型
function applyKey(p: ProviderEdit) {
  const k = p.apiKey.trim()
  if (!k) return
  for (const m of p.models) {
    m.api_key = k
    m.has_api_key = true
  }
}

// 连通性测试：只验证 URL + Key 能否连通
async function testProvider(p: ProviderEdit) {
  if (!isTauri() || p.busy) return
  const baseUrl = p.baseUrl.trim()
  if (!baseUrl) {
    p.msg = '请先填写 Base URL'
    return
  }
  p.busy = true
  p.msg = ''
  try {
    const keyId = p.hasApiKey && !p.apiKey.trim() && p.models.length > 0 ? p.models[0].id : undefined
    await tauriApi.fetchChatProviderModels(baseUrl, p.apiKey.trim(), keyId)
    p.msg = '连通正常'
  } catch (e) {
    p.msg = `连接失败：${String(e)}`
  } finally {
    p.busy = false
  }
}

// 获取模型列表：拉取供应商可用模型，勾选后加入当前配置
async function fetchModels(p: ProviderEdit) {
  if (!isTauri() || p.busy) return
  const baseUrl = p.baseUrl.trim()
  if (!baseUrl) {
    p.msg = '请先填写 Base URL'
    return
  }
  p.busy = true
  p.msg = ''
  try {
    const keyId = p.hasApiKey && !p.apiKey.trim() && p.models.length > 0 ? p.models[0].id : undefined
    const ids = await tauriApi.fetchChatProviderModels(baseUrl, p.apiKey.trim(), keyId)
    p.fetched = ids
    p.selected = new Set()
    if (ids.length === 0) p.msg = '未获取到可用模型'
    else p.msg = `获取到 ${ids.length} 个模型，勾选后点击「添加」`
  } catch (e) {
    p.msg = `获取失败：${String(e)}`
  } finally {
    p.busy = false
  }
}

function toggleFetched(p: ProviderEdit, id: string) {
  if (p.selected.has(id)) p.selected.delete(id)
  else p.selected.add(id)
}

// 把勾选的模型加入当前供应商（供应商名称不能为空，否则无法保存）
function addSelected(p: ProviderEdit) {
  const name = p.providerName.trim()
  if (!name) {
    p.msg = '请先填写供应商名称，再添加模型'
    return
  }
  const ids = p.fetched.filter((id) => p.selected.has(id))
  if (ids.length === 0) return
  for (const id of ids) {
    const already = p.models.some((m) => m.model === id)
    if (already) continue
    p.models.push({
      id: 'm' + Date.now() + '-' + Math.random().toString(36).slice(2, 6),
      name: id,
      provider_name: name,
      base_url: p.baseUrl.trim(),
      model: id,
      api_key: '',
      is_default: false,
      has_api_key: p.hasApiKey,
    })
  }
  if (p.models.length === 1) p.models[0].is_default = true
  p.fetched = []
  p.selected = new Set()
  p.msg = `已添加 ${ids.length} 个模型`
}

// 手动添加模型：不少 OpenAI 兼容中转不实现 /models 接口或返回空列表，在线拉取
// 一个模型都拿不到、原来就没有任何添加入口。这里直接按模型名加入当前供应商，
// 与「获取模型」在线拉取并存（勾选流程照旧可用）。
function addManualModel(p: ProviderEdit) {
  const id = p.manualModel.trim()
  if (!id) {
    p.msg = '请输入模型名称'
    return
  }
  const name = p.providerName.trim()
  if (!name) {
    p.msg = '请先填写供应商名称，再添加模型'
    return
  }
  if (p.models.some((m) => m.model === id)) {
    p.msg = `模型「${id}」已在列表中`
    p.manualModel = ''
    return
  }
  p.models.push({
    id: 'm' + Date.now() + '-' + Math.random().toString(36).slice(2, 6),
    name: id,
    provider_name: name,
    base_url: p.baseUrl.trim(),
    model: id,
    api_key: '',
    is_default: false,
    has_api_key: p.hasApiKey,
  })
  if (p.models.length === 1) p.models[0].is_default = true
  p.manualModel = ''
  p.msg = `已添加模型「${id}」`
}

function removeModel(p: ProviderEdit, id: string) {
  const i = p.models.findIndex((m) => m.id === id)
  if (i >= 0) {
    p.models.splice(i, 1)
    if (p.models.length > 0 && !p.models.some((m) => m.is_default)) p.models[0].is_default = true
  }
}

function collectAll(): ChatModelConfig[] {
  const out: ChatModelConfig[] = []
  for (const p of providers.value) {
    applyKey(p)
    out.push(...p.models)
  }
  // 平台条目不在 providers 里（它只是一个开关，不渲染卡片），但必须原样带回：
  // saveAll / 开关切换都是整体覆盖写配置，漏掉它等于把用户开着的平台额度悄悄关掉
  out.push(...platformModels.value)
  // 全局默认归一
  const hasDefault = out.some((m) => m.is_default)
  if (!hasDefault && out.length > 0) out[0].is_default = true
  return out
}

async function saveAll() {
  if (!isTauri() || saving.value) return
  // ① 每张供应商卡片都必须至少有一个模型。
  // 关键：以前这类「填了名称/地址却忘了点获取模型」的卡片会被**静默丢弃**——
  // 保存提示成功，实际什么都没配上；用户以为配好了，回到对话里却还是没模型可选。
  for (const p of providers.value) {
    if (p.models.length === 0) {
      const label = p.providerName.trim() || p.baseUrl.trim() || '未命名供应商'
      showToast(`供应商「${label}」还没有模型：点「获取模型」在线拉取，或直接输入模型名手动添加；不要这个供应商就点右上角删除`)
      return
    }
  }
  // ② 供应商名称约束：不能为空、不能重复
  const seen = new Set<string>()
  for (const p of providers.value) {
    const n = p.providerName.trim()
    if (!n) {
      showToast('供应商名称不能为空，请先填写')
      return
    }
    if (seen.has(n)) {
      showToast(`供应商名称不能重复：${n}`)
      return
    }
    seen.add(n)
  }
  // ③ 一个模型都没有也不让保存（一张卡片都没有、平台额度也没开）：
  // 空配置会让 AI 对话里没有任何可选模型
  const all = collectAll()
  if (all.length === 0) {
    showToast('还没有可用的模型：点「获取模型」在线拉取，或直接输入模型名手动添加；也可开启上方的「x-hub 平台免费额度」')
    return
  }
  saving.value = true
  try {
    const saved = await tauriApi.saveChatModels(all)
    // 同步进内存快照：后续任意 saveConfig 都带着最新模型，不会被旧快照覆盖
    store.setChatModels(saved)
    showToast('供应商配置已保存')
    // 同样走静默回读：保存后把卡片列表换成「加载中…」再换回来，是一次没必要的整块闪烁
    await loadProviders({ silent: true })
  } catch (e) {
    showToast(`保存失败：${String(e)}`)
  } finally {
    saving.value = false
  }
}

onMounted(() => {
  void loadProviders()
})

defineExpose({ reload: () => void loadProviders() })
</script>

<template>
  <div class="ai-providers">
    <p class="ai-intro">配置 OpenAI 兼容的模型供应商（如 DeepSeek、OpenAI、Ollama）。填好 Base URL 与 API Key 后，可测试连通、拉取可用模型并勾选添加。API Key 仅保存在系统钥匙串。</p>

    <!-- 平台免费额度：登录后可直接用，不需要自备 API Key。它不展开供应商字段（没有 Base URL /
         API Key 要填，平台侧有哪些模型也不必让人关心），只有一个开关；与下面的自备供应商并存。 -->
    <div class="setting-row ap-platform">
      <div class="setting-info">
        <span class="setting-name">使用 x-hub 平台免费额度</span>
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

    <!-- 只有「还没有内容」时才让占位顶掉列表：已有卡片时的刷新一律静默，避免整块闪一下 -->
    <div v-if="loading && providers.length === 0" class="ai-loading">加载中…</div>

    <template v-else>
      <!-- 供应商折叠卡片列表 -->
      <div v-for="(p, pi) in providers" :key="p.key + pi" class="prov-card" :class="{ open: p.expanded }">
        <!-- 折叠头部：列表式展示 -->
        <div class="prov-head" @click="expand(p)">
          <ChevronDown class="prov-chevron" :size="15" :class="{ on: p.expanded }" />
          <div class="prov-title">
            <span class="prov-name" :title="p.providerName || p.baseUrl || '未命名供应商'">{{ p.providerName || p.baseUrl || '未命名供应商' }}</span>
            <span class="prov-meta" :title="p.baseUrl || '未设置地址'">
              {{ p.baseUrl || '未设置地址' }}
              <template v-if="p.models.length"> · {{ p.models.length }} 个模型</template>
              <template v-if="p.models.find((m) => m.is_default)"> · 默认：{{ p.models.find((m) => m.is_default)?.name }}</template>
            </span>
          </div>
          <button class="ghost-btn prov-del" title="删除供应商" @click.stop="removeProvider(pi)">
            <Trash2 :size="13" />
          </button>
        </div>

        <!-- 展开体：编辑 + 获取模型 + 模型列表 -->
        <div v-if="p.expanded" class="prov-body">
          <div class="ai-field">
            <label class="ai-label">供应商名称</label>
            <input v-model="p.providerName" class="field-input" placeholder="如 DeepSeek" @input="syncProviderFields(p)" />
          </div>
          <div class="ai-field">
            <label class="ai-label">Base URL</label>
            <input v-model="p.baseUrl" class="field-input" placeholder="https://api.deepseek.com/v1" @input="syncProviderFields(p)" />
          </div>
          <div class="ai-field">
            <label class="ai-label">API Key</label>
            <div class="ai-key-row">
              <!-- 已保存 Key：脱敏展示 + 眼睛查看全部 + 复制 -->
              <template v-if="p.hasApiKey && !p.editingKey">
                <div class="key-display">
                  <span class="key-text" :title="p.keyVisible ? p.savedKey : '点击眼睛查看全部'">
                    {{ p.keyVisible ? p.savedKey : maskKey(p.savedKey) }}
                  </span>
                  <button
                    class="key-btn"
                    :title="p.keyVisible ? '隐藏' : '查看全部'"
                    @click="p.keyVisible = !p.keyVisible"
                  >
                    <EyeOff v-if="p.keyVisible" :size="13" />
                    <Eye v-else :size="13" />
                  </button>
                  <button class="key-btn" title="复制当前 Key" @click="copyKey(p)">
                    <Copy :size="13" />
                  </button>
                  <button class="key-btn" title="更换 Key" @click="p.editingKey = true">
                    <Pencil :size="13" />
                  </button>
                </div>
              </template>
              <!-- 输入新 Key -->
              <input
                v-else
                v-model="p.apiKey"
                class="field-input"
                :type="p.hasApiKey && !p.apiKey ? 'password' : 'text'"
                :placeholder="p.hasApiKey ? '输入新 Key 覆盖（留空不修改）' : 'sk-…'"
                autocomplete="off"
                @blur="finishKeyEdit(p)"
              />
              <button class="ghost-btn prov-test" :disabled="p.busy" @click="testProvider(p)">
                <FlaskConical :size="13" />
                {{ p.busy ? '测试中…' : '测试连通' }}
              </button>
              <button class="ghost-btn prov-fetch" :disabled="p.busy" @click="fetchModels(p)">
                <ListPlus :size="13" />
                {{ p.busy ? '获取中…' : '获取模型' }}
              </button>
            </div>
          </div>

          <p v-if="p.msg" class="prov-msg" :class="{ ok: p.msg.startsWith('连通正常') || p.msg.startsWith('已添加') || p.msg.startsWith('获取到') }">{{ p.msg }}</p>

          <!-- 获取到的模型列表：勾选添加 -->
          <div v-if="p.fetched.length" class="fetch-list">
            <div class="fetch-head">可用模型（勾选要添加的）</div>
            <label v-for="id in p.fetched" :key="id" class="fetch-item">
              <input type="checkbox" :checked="p.selected.has(id)" @change="toggleFetched(p, id)" />
              <span class="fetch-id" :title="id">{{ id }}</span>
            </label>
            <button class="ghost-btn fetch-add" :disabled="p.selected.size === 0" @click="addSelected(p)">
              添加所选（{{ p.selected.size }}）
            </button>
          </div>

          <!-- 手动添加模型：中转不实现 /models 或返回空时，直接填模型名加入（回车或点按钮） -->
          <div class="manual-model-row">
            <input
              v-model="p.manualModel"
              class="field-input"
              placeholder="手动添加模型名（如 gpt-4o-mini），回车确认"
              aria-label="手动添加模型名"
              @keydown.enter.prevent="addManualModel(p)"
            />
            <button class="ghost-btn manual-add" :disabled="!p.manualModel.trim()" @click="addManualModel(p)">
              <Plus :size="13" />
              添加
            </button>
          </div>

          <!-- 当前供应商下已配置的模型：tag 形式并排展示，可复制名称/删除 -->
          <div class="models-block">
            <div class="models-head">已配置模型</div>
            <div v-if="p.models.length === 0" class="models-empty">暂无模型：点「获取模型」在线拉取，或在上方输入模型名手动添加</div>
            <div v-else class="model-tags">
              <span
                v-for="m in p.models"
                :key="m.id"
                class="model-tag"
                :class="{ on: m.is_default }"
                :title="`${m.name}${m.is_default ? '（默认）' : ''}`"
              >
                <span class="model-tag-name" :title="m.name">{{ m.name }}</span>
                <button class="model-tag-btn" title="复制模型名称" @click="copyModelName(m)">
                  <Copy :size="11" />
                </button>
                <button class="model-tag-btn del" title="移除模型" @click="removeModel(p, m.id)">
                  <X :size="11" />
                </button>
              </span>
            </div>
          </div>
        </div>
      </div>

      <div class="ai-actions">
        <button class="ghost-btn" @click="addProvider">
          <Plus :size="13" /> 添加供应商
        </button>
        <button class="ghost-btn ai-save-btn" :disabled="saving" @click="saveAll">
          {{ saving ? '保存中…' : '保存配置' }}
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* 平台免费额度：一个开关行（.setting-row 通用样式来自设置页 shared.css），
   与下面的自备供应商卡片刻意区分——它不是供应商，没有需要用户填的东西 */
.ap-platform {
  margin: 4px 0 14px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border-soft);
}
/* 说明文字用满整行剩余宽度：不给 max-width（ch 单位对中文只有半个字宽，会把话提前截在半行处） */
.ap-platform .setting-info {
  flex: 1;
  min-width: 0;
}

.ai-intro {
  margin: 0 0 var(--space-4);
  font-size: 0.78125rem;
  line-height: 1.6;
  color: var(--text-3);
}
.ai-loading {
  padding: 18px 0;
  font-size: 0.8125rem;
  color: var(--text-3);
}

/* ---- 供应商卡片 ---- */
.prov-card {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.prov-card + .prov-card {
  margin-top: 12px;
}
.prov-card.open {
  border-color: color-mix(in srgb, var(--brand-500) 30%, transparent);
}
.prov-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  cursor: pointer;
  user-select: none;
}
.prov-chevron {
  flex-shrink: 0;
  color: var(--text-3);
  transition: transform 0.18s ease-out;
}
.prov-chevron.on {
  transform: rotate(180deg);
}
.prov-title {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.prov-name {
  font-size: 0.875rem;
  font-weight: 700;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.prov-meta {
  font-size: 0.75rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.prov-del {
  flex-shrink: 0;
  padding: 5px 10px;
  font-size: 0.75rem;
}
.prov-del:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
  border-color: var(--c-red-soft);
}

.prov-body {
  padding: 0 14px 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  border-top: 1px solid var(--border-soft);
  padding-top: 12px;
}
.ai-field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.ai-label {
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-3);
}
.ai-key-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ai-key-row .field-input {
  flex: 1;
}
.key-display {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  background: var(--input-bg);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
}
.key-text {
  flex: 1;
  min-width: 0;
  font-size: 0.78125rem;
  color: var(--text-2);
  font-family: var(--font-mono, ui-monospace, 'Cascadia Code', Consolas, monospace);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.key-btn {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  border-radius: 5px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
}
.key-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.prov-test {
  flex-shrink: 0;
  padding: 8px 12px;
  font-size: 0.75rem;
}
.prov-fetch {
  flex-shrink: 0;
  padding: 8px 12px;
  font-size: 0.75rem;
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.prov-fetch:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-color: var(--brand-500);
}
/* 手动添加模型行：与 ai-key-row 同款 flex 布局 */
.manual-model-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.manual-model-row .field-input {
  flex: 1;
}
.manual-add {
  flex-shrink: 0;
  padding: 8px 12px;
  font-size: 0.75rem;
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.manual-add:hover:not(:disabled) {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-color: var(--brand-500);
}
.prov-msg {
  margin: 0;
  font-size: 0.75rem;
  color: var(--c-red-ink);
}
.prov-msg.ok {
  color: var(--c-green-ink);
}

/* ---- 获取模型列表 ---- */
.fetch-list {
  border: 1px dashed var(--border-strong);
  border-radius: var(--radius-md);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.fetch-head,
.models-head {
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-2);
}
.fetch-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.8125rem;
  color: var(--text-1);
  cursor: pointer;
}
.fetch-item input[type='checkbox'] {
  accent-color: var(--brand-500);
}
.fetch-id {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fetch-add {
  align-self: flex-start;
  padding: 5px 12px;
  font-size: 0.75rem;
}

/* ---- 已配置模型：tag 并排展示 ---- */
.models-block {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.models-empty {
  font-size: 0.75rem;
  color: var(--text-3);
}
.model-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.model-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  max-width: 100%;
  padding: 3px 4px 3px 10px;
  font-size: 0.75rem;
  color: var(--brand-500);
  background: var(--brand-50);
  border: 1px solid color-mix(in srgb, var(--brand-500) 45%, transparent);
  border-radius: var(--radius-pill);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
}
.model-tag-name {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.model-tag-btn {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  border: none;
  background: transparent;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
}
.model-tag-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.model-tag-btn.del:hover {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}

/* ---- 底部操作 ---- */
.ai-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 16px;
}
.ai-save-btn {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-color: var(--brand-500);
}
.ai-save-btn:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-color: var(--brand-500);
}
.ai-save-btn:disabled {
  opacity: 0.55;
}
</style>
