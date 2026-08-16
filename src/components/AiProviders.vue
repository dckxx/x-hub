<script setup lang="ts">
import { inject, onMounted, ref } from 'vue'
import { ChevronDown, Copy, Eye, EyeOff, FlaskConical, ListPlus, Pencil, Plus, Trash2, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type ChatModelConfig } from '../api/tauri'

const showToast = inject<(msg: string) => void>('showToast', () => {})

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
  busy: boolean
  msg: string
  savedKey: string
  keyVisible: boolean
  editingKey: boolean
}

const providers = ref<ProviderEdit[]>([])
const loading = ref(false)
const saving = ref(false)

function groupKey(m: ChatModelConfig): string {
  const name = (m.provider_name ?? '').trim()
  const base = (m.base_url ?? '').trim()
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
    busy: false,
    msg: '',
    savedKey: '',
    keyVisible: false,
    editingKey: false,
  }
}

async function loadProviders() {
  if (!isTauri()) return
  loading.value = true
  try {
    const list = await tauriApi.getChatModels()
    const map = new Map<string, ProviderEdit>()
    for (const m of list) {
      const p = toProvider(m)
      if (map.has(p.key)) {
        map.get(p.key)!.models.push(m)
      } else {
        map.set(p.key, p)
      }
    }
    providers.value = [...map.values()]
  } catch (e) {
    showToast(`加载失败：${String(e)}`)
  } finally {
    loading.value = false
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

// 把勾选的模型加入当前供应商
function addSelected(p: ProviderEdit) {
  const ids = p.fetched.filter((id) => p.selected.has(id))
  if (ids.length === 0) return
  for (const id of ids) {
    const already = p.models.some((m) => m.model === id)
    if (already) continue
    p.models.push({
      id: 'm' + Date.now() + '-' + Math.random().toString(36).slice(2, 6),
      name: id,
      provider_name: p.providerName.trim() || id,
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
  // 全局默认归一
  const hasDefault = out.some((m) => m.is_default)
  if (!hasDefault && out.length > 0) out[0].is_default = true
  return out
}

async function saveAll() {
  if (!isTauri() || saving.value) return
  saving.value = true
  try {
    await tauriApi.saveChatModels(collectAll())
    showToast('供应商配置已保存')
    await loadProviders()
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

    <div v-if="loading" class="ai-loading">加载中…</div>

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

          <p v-if="p.msg" class="prov-msg" :class="{ ok: p.msg === '连通正常' || p.msg.startsWith('已添加') || p.msg.startsWith('获取到') }">{{ p.msg }}</p>

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

          <!-- 当前供应商下已配置的模型：tag 形式并排展示，可复制名称/删除 -->
          <div class="models-block">
            <div class="models-head">已配置模型</div>
            <div v-if="p.models.length === 0" class="models-empty">暂无模型，点击「获取模型」拉取后添加</div>
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
.ai-intro {
  margin: 0 0 var(--space-4);
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-3);
}
.ai-loading {
  padding: 18px 0;
  font-size: 13px;
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
  font-size: 14px;
  font-weight: 700;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.prov-meta {
  font-size: 12px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.prov-del {
  flex-shrink: 0;
  padding: 5px 10px;
  font-size: 12px;
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
  font-size: 12px;
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
  font-size: 12.5px;
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
  font-size: 12px;
}
.prov-fetch {
  flex-shrink: 0;
  padding: 8px 12px;
  font-size: 12px;
  background: var(--brand-50);
  color: var(--brand-500);
  border-color: color-mix(in srgb, var(--brand-500) 35%, transparent);
}
.prov-fetch:hover {
  background: var(--brand-500);
  color: var(--text-on-accent);
  border-color: var(--brand-500);
}
.prov-msg {
  margin: 0;
  font-size: 12px;
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
  font-size: 12px;
  font-weight: 700;
  color: var(--text-2);
}
.fetch-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
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
  font-size: 12px;
}

/* ---- 已配置模型：tag 并排展示 ---- */
.models-block {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.models-empty {
  font-size: 12px;
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
  font-size: 12px;
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
