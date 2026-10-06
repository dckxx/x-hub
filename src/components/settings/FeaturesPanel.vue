<script setup lang="ts">
// 功能大类（AI 助手 / 剪贴板 / 联网）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { LocateFixed, MapPin, Trash2 } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import AppSelect from '../AppSelect.vue';
import AiProviders from '../AiProviders.vue';
import SudaSubcategoryManager from './SudaSubcategoryManager.vue';
import { useStore } from '../../stores/workbench';
import { reportClientError } from '../../utils/error-report';

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

async function onToggleChatWindowMode() {
  const next = !chatWindowMode.value
  try {
    await store.setChatWindowMode(next)
    showToast(next ? 'AI 对话已改为独立窗口打开' : 'AI 对话已改回主窗内嵌面板')
  } catch {
    showToast('切换失败，请重试')
  }
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
        <section id="sv-sec-ai" class="sv-sec" aria-label="AI 助手">
          <h3 class="sv-sec-title">AI 助手</h3>

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

          <AiProviders />
        </section>

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
