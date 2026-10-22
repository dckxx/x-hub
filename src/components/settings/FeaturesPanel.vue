<script setup lang="ts">
// 功能大类（速达 / 剪贴板 / 联网 / 性能）
// （AI 助手 2026-10-16 自本大类拉出为独立设置项 AiPanel.vue：对话形态 + 供应商 + AI 额度）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { LocateFixed, MapPin, Trash2 } from 'lucide-vue-next';
import { isPlatformModel, isTauri, tauriApi, PLATFORM_ENTRY_NAME, type ChatModelConfig } from '../../api/tauri';
import AppSelect from '../AppSelect.vue';
import SudaSubcategoryManager from './SudaSubcategoryManager.vue';
import { useStore } from '../../stores/workbench';
import { reportClientError } from '../../utils/error-report';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 速达（网页打开方式 + 小类管理，ADR 0011 / 0012）----
const SUDA_OPEN_MODE_OPTIONS = [
  { value: 'panel', label: '内嵌面板（主窗口内）' },
  { value: 'window', label: '独立浏览器窗口' },
  { value: 'system', label: '系统默认浏览器' },
]
const sudaWebOpenMode = computed(() => {
  const mode = store.state.config.suda_web_open_mode
  return mode === 'window' || mode === 'system' ? mode : 'panel'
})
function onSudaOpenModeChange(v: string | number) {
  void store.setSudaWebOpenMode(v === 'window' || v === 'system' ? v : 'panel')
}

async function onToggleSudaPanelToolbar() {
  await store.setSudaPanelToolbar(!store.state.config.suda_panel_toolbar)
}

// ---- 联网 / 在线服务 ----
const weatherCityInput = ref(store.state.config.weather_city ?? '')
const weatherSaving = ref(false)

async function onToggleOnline() {
  if (!isTauri()) return
  const next = !store.state.config.online_enabled
  await store.setOnlineEnabled(next)
  showToast(next ? '已开启联网功能' : '已关闭联网功能')
}

// ---- 性能 / 内存 ----
async function onToggleWebviewMem() {
  if (!isTauri()) return
  const next = !store.state.config.webview_mem_low_on_hide
  await store.setWebviewMemLowOnHide(next)
  showToast(next ? '已开启：隐藏窗口时释放内存' : '已关闭：隐藏窗口保持全量内存')
}

// ---- 速记（AI 深度整理模型，约定 38）----
// note_ai_model 取值：''=自动 / 平台入口名=负载切换 / 'platform:<名>'=指定平台模型 / 其余=自备模型名。
// 来源与模型是两级 UI 筛选，落盘只存模型值一个字段；来源「自动」时模型下拉置灰。
const noteModels = ref<ChatModelConfig[]>([])
const platformModelNames = ref<string[]>([])
const platformModelsFailed = ref(false)

function providerLabel(m: ChatModelConfig): string {
  return m.provider_name?.trim() || '未命名供应商'
}

const noteSourceOptions = computed(() => {
  const providers: string[] = []
  for (const m of noteModels.value) {
    if (isPlatformModel(m)) continue
    const p = providerLabel(m)
    if (!providers.includes(p)) providers.push(p)
  }
  return [
    { value: 'auto', label: '自动（推荐）' },
    { value: 'platform', label: 'x-hub 平台（平台内置）' },
    ...providers.map((p) => ({ value: p, label: p })),
  ]
})

const noteModelSource = computed(() => {
  const v = store.state.config.note_ai_model ?? ''
  if (v === PLATFORM_ENTRY_NAME || v.startsWith('platform:')) return 'platform'
  if (!v) return 'auto'
  const m = noteModels.value.find((x) => !isPlatformModel(x) && x.name === v)
  // 存的自备模型已被删除：按「自动」渲染（不悄悄改写配置，用户下次保存时自然覆盖）
  return m ? providerLabel(m) : 'auto'
})

const noteModelOptions = computed(() => {
  const src = noteModelSource.value
  if (src === 'platform') {
    const opts = [{ value: PLATFORM_ENTRY_NAME, label: '自动（负载切换）' }]
    for (const n of platformModelNames.value) opts.push({ value: `platform:${n}`, label: n })
    return opts
  }
  if (src === 'auto') return []
  return noteModels.value
    .filter((m) => !isPlatformModel(m) && providerLabel(m) === src)
    .map((m) => ({ value: m.name, label: m.name }))
})

const noteModelValue = computed(() => {
  const v = store.state.config.note_ai_model ?? ''
  if (noteModelSource.value !== 'platform') {
    return noteModelOptions.value.some((o) => o.value === v) ? v : ''
  }
  // 指定的平台模型已下架 → 显示「自动（负载切换）」
  return noteModelOptions.value.some((o) => o.value === v) ? v : PLATFORM_ENTRY_NAME
})

function onNoteModelSourceChange(v: string | number) {
  const src = String(v)
  let next = ''
  if (src === 'platform') {
    next = PLATFORM_ENTRY_NAME
  } else if (src !== 'auto') {
    const first = noteModels.value.find((m) => !isPlatformModel(m) && providerLabel(m) === src)
    if (!first) return
    next = first.name
  }
  void store.setNoteAiModel(next)
}

function onNoteModelChange(v: string | number) {
  void store.setNoteAiModel(String(v))
}

async function loadNoteAiModels() {
  if (!isTauri()) return
  try {
    noteModels.value = await tauriApi.getChatModels()
  } catch {
    // 浏览器预览无后端
  }
  try {
    platformModelNames.value = await tauriApi.platformModels()
    platformModelsFailed.value = false
  } catch {
    // 未登录或网络失败：模型下拉只剩「自动（负载切换）」，desc 里给一句交代
    platformModelNames.value = []
    platformModelsFailed.value = true
  }
}

let unlistenNoteModels: UnlistenFn | null = null
onMounted(async () => {
  void loadNoteAiModels()
  if (!isTauri()) return
  // 平台清单后台对账（chat-models-changed）后重拉：设置开着时下拉跟随最新清单
  unlistenNoteModels = await listen('chat-models-changed', () => {
    void loadNoteAiModels()
  })
})
onBeforeUnmount(() => {
  unlistenNoteModels?.()
})

async function applyWeatherCity() {
  const city = weatherCityInput.value.trim()
  if (!city) {
    showToast('请输入城市名')
    return
  }
  weatherSaving.value = true
  try {
    const loc = await store.setWeatherCity(city)
    weatherCityInput.value = loc.name
    showToast(`天气已设为 ${loc.name}`)
  } catch (e) {
    showToast(`设置城市失败：${String(e)}`)
  } finally {
    weatherSaving.value = false
  }
}

async function onLocateByIp() {
  weatherSaving.value = true
  try {
    const loc = await store.locateWeatherByIp()
    weatherCityInput.value = loc.name
    showToast(`已定位到 ${loc.name}`)
  } catch (e) {
    showToast(`自动定位失败：${String(e)}`)
  } finally {
    weatherSaving.value = false
  }
}

// ---- 剪贴板保留策略 ----
const clipMaxItems = ref(500)
const clipTtlDays = ref(7)
const clipRetentionSaving = ref(false)

function commitClipRetention() {
  const maxItems = Math.round(clipMaxItems.value)
  const ttlDays = Math.round(clipTtlDays.value)
  if (!isTauri() || clipRetentionSaving.value) return
  if (maxItems === store.state.config.clipboard_max_items && ttlDays === store.state.config.clipboard_ttl_days) return
  clipRetentionSaving.value = true
  void store
    .setClipboardRetention(maxItems, ttlDays)
    .then(() => showToast(`保留策略已更新：最多 ${maxItems} 条 / ${ttlDays} 天`))
    .catch((e) => {
      void reportClientError('更新剪贴板保留策略失败', e)
    })
    .finally(() => {
      clipRetentionSaving.value = false
    })
}

async function onToggleClipboardPause() {
  if (!isTauri()) return
  const next = !store.state.config.clipboard_paused
  await store.setClipboardPaused(next)
  showToast(next ? '剪贴板已暂停记录' : '剪贴板已恢复记录')
}

// ---- 粘贴快捷键方式 ----
const PASTE_METHOD_OPTIONS = [
  { value: 'auto', label: '自动（终端用 Ctrl+Shift+V，其他用 Ctrl+V）' },
  { value: 'ctrl_v', label: 'Ctrl+V' },
  { value: 'ctrl_shift_v', label: 'Ctrl+Shift+V' },
  { value: 'shift_insert', label: 'Shift+Insert' },
] as const

const pasteMethod = ref(store.state.config.clipboard_paste_method ?? 'auto')

function onPasteMethodChange(value: string) {
  pasteMethod.value = value
  if (!isTauri()) return
  void tauriApi
    .setClipboardPasteMethod(value)
    .then(() => showToast(`粘贴方式已更新为 ${PASTE_METHOD_OPTIONS.find((o) => o.value === value)?.label ?? value}`))
    .catch((e) => {
      void reportClientError('更新粘贴方式失败', e)
    })
}

async function onClearClipboard() {
  if (!isTauri()) return
  try {
    await tauriApi.clipboardClear()
    showToast('剪贴板历史已清空')
  } catch (e) {
    showToast(`清空失败：${String(e)}`)
  }
}

onMounted(() => {

  clipMaxItems.value = store.state.config.clipboard_max_items ?? 500

  clipTtlDays.value = store.state.config.clipboard_ttl_days ?? 7

  pasteMethod.value = store.state.config.clipboard_paste_method ?? 'auto'

})
</script>

<template>
        <section id="sv-sec-suda" class="sv-sec" aria-label="速达">
          <h3 class="sv-sec-title">速达</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">网页默认打开方式</span>
              <span class="setting-desc">点击网页条目时的打开位置：内嵌面板在主窗口右侧视图打开（轻量、单页），独立浏览器窗口支持多标签与同地址复用，系统默认浏览器交由 Windows 按系统设置打开；右键菜单可临时换其它方式</span>
            </div>
            <AppSelect
              :model-value="sudaWebOpenMode"
              :options="SUDA_OPEN_MODE_OPTIONS"
              aria-label="网页默认打开方式"
              @update:model-value="onSudaOpenModeChange"
            />
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">内嵌面板显示工具栏</span>
              <span class="setting-desc">开启后内嵌面板顶部显示地址栏与前进/后退/刷新按钮；默认关闭，整个面板区域只显示网页（返回速达点左侧导航即可）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.suda_panel_toolbar"
              :class="{ on: store.state.config.suda_panel_toolbar }"
              @click="onToggleSudaPanelToolbar"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">小类管理</span>
              <span class="setting-desc">大类（应用/网页/文件）下的二级归属，每条资源归入一个小类；行内改名、拖拽排序、点星标设默认，名称可用 / 分层级（如 开发/前端）；删除需点两下——第一下垃圾桶变 √ 勾，再点一下才真删（3 秒后自动取消），删除后条目自动改挂默认小类；右上「批量删除」可勾选多个一并删除</span>
            </div>
          </div>
          <SudaSubcategoryManager />
        </section>

        <section id="sv-sec-notes" class="sv-sec" aria-label="速记">
          <h3 class="sv-sec-title">速记</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">AI 深度整理模型</span>
              <span class="setting-desc">编辑器「深度整理」按钮使用的模型。自动：平台额度可用时优先走平台（多个平台模型自动负载切换），否则用对话的默认模型；也可固定用平台某个模型或自备供应商的某个模型。所选模型被删除或下架时自动回到「自动」</span>
            </div>
            <div class="notes-ai-model">
              <AppSelect
                :model-value="noteModelSource"
                :options="noteSourceOptions"
                aria-label="整理模型来源"
                @update:model-value="onNoteModelSourceChange"
              />
              <AppSelect
                :model-value="noteModelValue"
                :options="noteModelOptions"
                :disabled="noteModelSource === 'auto'"
                aria-label="AI 深度整理模型"
                @update:model-value="onNoteModelChange"
              />
              <span v-if="noteModelSource === 'platform' && platformModelsFailed" class="notes-ai-hint">
                平台模型列表获取失败（未登录或网络问题），仅可自动负载切换
              </span>
            </div>
          </div>
        </section>

        <section id="sv-sec-clipboard" class="sv-sec" aria-label="剪贴板">
          <h3 class="sv-sec-title">剪贴板</h3>

          <h4 class="sv-subtitle">保留策略</h4>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">保留条数上限</span>
              <span class="setting-desc">总记录数上限（含置顶项，默认 500）</span>
            </div>
            <div class="num-edit">
              <input
                v-model.number="clipMaxItems"
                class="num-input"
                type="number"
                min="20"
                max="5000"
                step="50"
                @change="commitClipRetention"
              />
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">保留天数</span>
              <span class="setting-desc">非置顶记录超过 N 天自动清理（默认 7 天）</span>
            </div>
            <div class="num-edit">
              <input
                v-model.number="clipTtlDays"
                class="num-input"
                type="number"
                min="1"
                max="365"
                step="1"
                @change="commitClipRetention"
              />
            </div>
          </div>

          <h4 class="sv-subtitle">记录行为</h4>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">粘贴方式</span>
              <span class="setting-desc">自动模式下：终端/命令行（不支持 Ctrl+V）用 Ctrl+Shift+V，其他应用用 Ctrl+V</span>
            </div>
            <AppSelect
              :model-value="pasteMethod"
              :options="PASTE_METHOD_OPTIONS"
              aria-label="粘贴方式"
              @update:model-value="onPasteMethodChange"
            />
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">暂停记录</span>
              <span class="setting-desc">暂停期间复制的内容不会写入历史（已保存的记录保留）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.clipboard_paused"
              :class="{ on: store.state.config.clipboard_paused }"
              @click="onToggleClipboardPause"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <h4 class="sv-subtitle">操作</h4>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">清空历史</span>
              <span class="setting-desc">立即删除所有剪贴板记录（含置顶项），不可恢复</span>
            </div>
            <button class="ghost-btn data-btn danger" @click="onClearClipboard">
              <Trash2 :size="14" :stroke-width="2" />
              清空
            </button>
          </div>
        </section>

        <section id="sv-sec-online" class="sv-sec" aria-label="联网">
          <h3 class="sv-sec-title">联网</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">联网功能</span>
              <span class="setting-desc">开启后：有网时显示天气与在线名言；无网时自动隐藏在线内容（不影响本地功能）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.online_enabled"
              :class="{ on: store.state.config.online_enabled }"
              @click="onToggleOnline"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">天气城市</span>
              <span class="setting-desc">手动输入城市名（已内置全国主要城市，精确匹配）；IP 自动定位仅供参考、可能不准</span>
            </div>
            <div class="weather-edit">
              <input
                v-model="weatherCityInput"
                class="field-input"
                type="text"
                maxlength="30"
                placeholder="输入城市名，如：北京"
                spellcheck="false"
                @keydown.enter="applyWeatherCity"
              />
              <button
                class="ghost-btn data-btn"
                type="button"
                :disabled="weatherSaving"
                @click="applyWeatherCity"
              >
                <MapPin :size="14" :stroke-width="2" />
                设置
              </button>
              <button
                class="ghost-btn data-btn"
                type="button"
                :disabled="weatherSaving"
                @click="onLocateByIp"
              >
                <LocateFixed :size="14" :stroke-width="2" />
                自动定位
              </button>
            </div>
          </div>
        </section>

        <section id="sv-sec-mem" class="sv-sec" aria-label="性能">
          <h3 class="sv-sec-title">性能</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">隐藏窗口时降低内存占用</span>
              <span class="setting-desc">对话窗、剪贴板、通知、速达浏览器等隐藏窗口常驻后台，开启后它们隐藏时把渲染进程内存尽量交还给系统、显示前自动恢复（脚本与消息照常运行）；关闭后窗口隐藏也保持全量内存</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.webview_mem_low_on_hide"
              :class="{ on: store.state.config.webview_mem_low_on_hide }"
              @click="onToggleWebviewMem"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>
        </section>
</template>
