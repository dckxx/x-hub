<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ChevronDown, Download, Keyboard, LocateFixed, Lock, MapPin, Trash2, Upload } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import AppSelect from './AppSelect.vue'
import AiProviders from './AiProviders.vue'
import AboutSection from './AboutSection.vue'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'
import { normalizeShortcutDisplay, useShortcutRecorder } from '../composables/useShortcutRecorder'

const props = defineProps<{ initialSection?: string }>()

const emit = defineEmits<{ (e: 'open-layout-editor'): void }>()

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 分类导航（左侧分类 = 右侧区块锚点，点击平滑滚动定位，不做内容切换） ----
const SECTIONS = [
  { id: 'appearance', label: '外观' },
  { id: 'workbench', label: '工作台' },
  { id: 'ai', label: 'AI 助手' },
  { id: 'shortcut', label: '快捷键' },
  { id: 'clipboard', label: '剪贴板' },
  { id: 'online', label: '联网' },
  { id: 'data', label: '数据' },
  { id: 'about', label: '关于' },
] as const

type SectionId = (typeof SECTIONS)[number]['id']
const activeSection = ref<SectionId>('appearance')
const contentRef = ref<HTMLElement | null>(null)

function goToSection(id: SectionId) {
  activeSection.value = id
  const container = contentRef.value
  const el = container?.querySelector<HTMLElement>(`#sv-sec-${id}`)
  if (!container || !el) return
  // 相对滚动容器计算目标位置（容器为 position: relative，offsetTop 相对它）
  container.scrollTo({ top: Math.max(0, el.offsetTop - 12), behavior: 'smooth' })
}

// 滚动时同步左侧激活态（停留在某区块即高亮对应分类）
function onContentScroll() {
  const container = contentRef.value
  if (!container) return
  let current: SectionId = SECTIONS[0].id
  for (const sec of SECTIONS) {
    const el = container.querySelector<HTMLElement>(`#sv-sec-${sec.id}`)
    if (!el) continue
    if (el.offsetTop - 60 <= container.scrollTop) current = sec.id
  }
  if (current !== activeSection.value) activeSection.value = current
}

onMounted(() => contentRef.value?.addEventListener('scroll', onContentScroll))
onBeforeUnmount(() => contentRef.value?.removeEventListener('scroll', onContentScroll))

// ---- 快捷键录入：全局 / 剪贴板共用一套录制逻辑（见 composables/useShortcutRecorder.ts） ----
const {
  value: shortcut,
  error: shortcutError,
  listening: shortcutListening,
  inputRef: shortcutInputRef,
  commit: commitShortcut,
  startListening: startListeningShortcut,
  onBlur: onShortcutBlur,
  onKeydown: onShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.global_shortcut),
  label: '全局快捷键',
  save: (v) => store.setGlobalShortcut(v),
  showToast,
})

const {
  value: clipShortcut,
  saved: clipSavedShortcut,
  error: clipError,
  listening: clipListening,
  inputRef: clipInputRef,
  commit: commitClipShortcut,
  startListening: startListenClipShortcut,
  onBlur: onClipShortcutBlur,
  onKeydown: onClipShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.clipboard_shortcut ?? 'Ctrl+`'),
  label: '剪贴板快捷键',
  save: (v) => store.setClipboardShortcut(v),
  showToast,
})

// inputRef 仅在模板 ref 绑定中使用（把 DOM 输入框连到 recorder 内部，点击「录入」自动聚焦），
// vue-tsc 不把模板 ref 视为「读取」，这里显式求值一次以通过 noUnusedLocals
void shortcutInputRef
void clipInputRef

onMounted(async () => {
  if (!isTauri()) return
  shortcut.value = normalizeShortcutDisplay(await tauriApi.getGlobalShortcut())
  clipShortcut.value = normalizeShortcutDisplay(store.state.config.clipboard_shortcut ?? 'Ctrl+`')
  clipSavedShortcut.value = clipShortcut.value
  clipMaxItems.value = store.state.config.clipboard_max_items ?? 500
  clipTtlDays.value = store.state.config.clipboard_ttl_days ?? 7
  pasteMethod.value = store.state.config.clipboard_paste_method ?? 'auto'
  // 支持外部定位到指定分类（如 AI 对话面板「去配置」跳转）
  if (props.initialSection) {
    const target = props.initialSection as SectionId
    if (SECTIONS.some((s) => s.id === target)) {
      activeSection.value = target
      void nextTick(() => goToSection(target))
    }
  }
})

function onToggleCountdownSound() {
  void store.setCountdownSound(!store.state.config.countdown_sound)
}

function onToggleSidebar() {
  void store.setSidebarToggle(!store.state.config.sidebar_toggle)
}

function onChatPanelOpacityInput(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  void store.setChatPanelOpacity(v)
}

// ---- 字体大小（全局 + 单模块） ----
const FONT_MODULES = [
  { key: 'sticky', label: '便签', configKey: 'font_sticky' },
  { key: 'notes', label: '速记', configKey: 'font_notes' },
  { key: 'prompt', label: '提示词', configKey: 'font_prompt' },
  { key: 'todo', label: '待办', configKey: 'font_todo' },
  { key: 'usage', label: '用量', configKey: 'font_usage' },
] as const

type FontModuleKey = (typeof FONT_MODULES)[number]['key']

// 字体大小折叠块：默认收起，减少设置页纵向占用
const fontExpanded = ref(false)

function onFontScaleInput(e: Event) {
  void store.setFontScale(Number((e.target as HTMLInputElement).value))
}

function onModuleFontInput(key: FontModuleKey, e: Event) {
  void store.setModuleFontScale(key, Number((e.target as HTMLInputElement).value))
}

// ---- 时钟卡片语录（回车/失焦自动保存，清空则回退默认） ----
const clockQuote = ref(store.state.config.clock_quote ?? '')
const savedClockQuote = ref(clockQuote.value)

function commitClockQuote() {
  const value = clockQuote.value.trim()
  clockQuote.value = value
  if (value === savedClockQuote.value) return
  savedClockQuote.value = value
  void store.setClockQuote(value)
  showToast(value ? '时钟卡片语录已更新' : '时钟卡片语录已改为随机名言金句')
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

const QUOTE_SOURCE_OPTIONS = [
  { value: 'online', label: '在线名言（联网时随机，离线回退本地）' },
  { value: 'local', label: '本地语料（仅内置金句）' },
] as const

const quoteSource = ref(store.state.config.quote_source ?? 'online')

function onQuoteSourceChange(value: string) {
  quoteSource.value = value
  if (!isTauri()) return
  void store.setQuoteSource(value as 'online' | 'local').then(() => {
    showToast(value === 'online' ? '名言来源已设为在线' : '名言来源已设为本地语料')
  })
}

// ---- 数据备份 / 恢复 ----
const confirmRestore = ref(false)
let confirmTimer: ReturnType<typeof setTimeout> | null = null

async function backupData() {
  if (!isTauri()) return
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  try {
    await tauriApi.backupData(dir)
    showToast('备份完成')
  } catch (e) {
    showToast(`备份失败：${String(e)}`)
  }
}

async function restoreData() {
  if (!isTauri()) return
  // 两段式确认：第二次点击才执行
  if (!confirmRestore.value) {
    confirmRestore.value = true
    if (confirmTimer) clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => {
      confirmRestore.value = false
    }, 3000)
    return
  }
  confirmRestore.value = false
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  try {
    await tauriApi.restoreData(dir)
    showToast('恢复已暂存，重启应用后生效')
  } catch (e) {
    showToast(`恢复失败：${String(e)}`)
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

// ---- 主题设置 ----
const COLOR_PRESETS = [
  { id: 'indigo', name: '靛紫', color: '#5b5bf5' },
  { id: 'green', name: '护眼绿', color: '#059669' },
  { id: 'morandi', name: '莫兰迪', color: '#7c7c8a' },
  { id: 'midnight', name: '午夜蓝', color: '#2f54eb' },
  { id: 'rose', name: '玫瑰红', color: '#e11d48' },
  { id: 'amber', name: '琥珀金', color: '#d97706' },
  { id: 'teal', name: '青碧', color: '#0d9488' },
  { id: 'violet', name: '紫罗兰', color: '#7c3aed' },
  { id: 'sky', name: '天蓝', color: '#0284c7' },
  { id: 'slate', name: '墨石', color: '#475569' },
] as const
// 渐变背景预设：色卡显示渐变预览，点击仅覆盖 body 背景（--app-bg），UI 强调色取主色
const GRADIENT_PRESETS = [
  { id: 'grad-star', name: '星夜', color: '#6d5dfc', gradient: 'linear-gradient(150deg, #4f46e5, #a855f7)' },
  { id: 'grad-sunset', name: '落日', color: '#f4572e', gradient: 'linear-gradient(150deg, #f97316, #e11d48)' },
  { id: 'grad-aurora', name: '极光', color: '#0891b2', gradient: 'linear-gradient(150deg, #06b6d4, #8b5cf6)' },
  { id: 'grad-rose', name: '玫瑰', color: '#e11d48', gradient: 'linear-gradient(150deg, #f43f5e, #a855f7)' },
  { id: 'grad-forest', name: '森林', color: '#059669', gradient: 'linear-gradient(150deg, #10b981, #3b82f6)' },
  { id: 'grad-gold', name: '鎏金', color: '#d97706', gradient: 'linear-gradient(150deg, #f59e0b, #ef4444)' },
  { id: 'grad-ocean', name: '深海', color: '#2563eb', gradient: 'linear-gradient(150deg, #3b82f6, #14b8a6)' },
  { id: 'grad-grape', name: '葡萄', color: '#8b5cf6', gradient: 'linear-gradient(150deg, #a855f7, #ec4899)' },
  { id: 'grad-flame', name: '焰火', color: '#ef4444', gradient: 'linear-gradient(150deg, #ef4444, #f59e0b)' },
  { id: 'grad-graphite', name: '石墨', color: '#64748b', gradient: 'linear-gradient(150deg, #64748b, #2563eb)' },
] as const
const PRESET_ACCENT: Record<string, string> = {
  indigo: '#5b5bf5', green: '#059669', morandi: '#7c7c8a', midnight: '#2f54eb', rose: '#e11d48', amber: '#d97706', teal: '#0d9488', violet: '#7c3aed', sky: '#0284c7', slate: '#475569',
  'grad-star': '#6d5dfc', 'grad-sunset': '#f4572e', 'grad-aurora': '#0891b2', 'grad-rose': '#e11d48', 'grad-forest': '#059669', 'grad-gold': '#d97706', 'grad-ocean': '#2563eb', 'grad-grape': '#8b5cf6', 'grad-flame': '#ef4444', 'grad-graphite': '#64748b',
}
const ACCENT_PRESETS = ['#5b5bf5', '#ef4444', '#f59e0b', '#22c55e', '#3b82f6', '#8b5cf6', '#ec4899', '#0ea5e9']
const themeMode = computed(() => store.state.config.theme_mode)
const themePreset = computed(() => store.state.config.theme_preset)
const themeAccent = computed(() => store.state.config.accent_color)
function onAccentInput(e: Event) {
  store.setAccentColor((e.target as HTMLInputElement).value)
}


</script>

<template>
  <div class="settings-view">
    <header class="sv-header">
      <h2 class="sv-title">设置</h2>
    </header>

    <div class="sv-body">
      <!-- 左侧分类导航 -->
      <nav class="sv-nav" aria-label="设置分类">
        <button
          v-for="sec in SECTIONS"
          :key="sec.id"
          type="button"
          class="sv-nav-item"
          :class="{ active: activeSection === sec.id }"
          :aria-current="activeSection === sec.id ? 'true' : undefined"
          @click="goToSection(sec.id)"
        >
          {{ sec.label }}
        </button>
      </nav>

      <!-- 右侧内容：全量渲染，分类仅作滚动锚点 -->
      <div ref="contentRef" class="sv-content">
        <!-- AI 助手 -->
        <section id="sv-sec-ai" class="sv-sec" aria-label="AI 助手">
          <h3 class="sv-sec-title">AI 助手</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">AI 对话面板透明度</span>
              <span class="setting-desc">对话抽屉的整体不透明度（50% – 100%）</span>
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
                @input="onChatPanelOpacityInput"
              />
              <span class="opacity-value">{{ Math.round((store.state.config.chat_panel_opacity ?? 1) * 100) }}%</span>
            </div>
          </div>

          <AiProviders />
        </section>

        <!-- 外观 -->
        <section id="sv-sec-appearance" class="sv-sec" aria-label="外观">
          <h3 class="sv-sec-title">外观</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">侧边栏展开功能</span>
              <span class="setting-desc">开启后侧栏底部显示展开/收起按钮（默认关闭，侧栏默认收起）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.sidebar_toggle"
              :class="{ on: store.state.config.sidebar_toggle }"
              @click="onToggleSidebar"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <!-- 主题设置：标签 + 控件布局 -->
          <div class="setting-group theme-group">
            <!-- ① 主题模式 + 明暗模式 -->
            <div class="theme-row">
              <span class="theme-label">主题模式</span>
              <div class="theme-mode-seg" role="radiogroup" aria-label="明暗模式">
                <button
                  role="radio"
                  :aria-checked="themeMode === 'light'"
                  :class="{ on: themeMode === 'light' }"
                  @click="void store.setThemeMode('light')"
                >
                  亮色
                </button>
                <button
                  role="radio"
                  :aria-checked="themeMode === 'dark'"
                  :class="{ on: themeMode === 'dark' }"
                  @click="void store.setThemeMode('dark')"
                >
                  暗色
                </button>
                <button
                  role="radio"
                  :aria-checked="themeMode === 'system'"
                  :class="{ on: themeMode === 'system' }"
                  @click="void store.setThemeMode('system')"
                >
                  跟随系统
                </button>
              </div>
            </div>

            <!-- ② 主题配色：单色 + 渐变两组，色卡整块放标签正下方 -->
            <div class="theme-row theme-row-stack">
              <span class="theme-label">主题配色</span>
              <div class="theme-presets" role="radiogroup" aria-label="主题配色">
                <span class="theme-sublabel">单色</span>
                <button
                  v-for="p in COLOR_PRESETS"
                  :key="p.id"
                  role="radio"
                  :aria-checked="themePreset === p.id"
                  :class="{ on: themePreset === p.id }"
                  @click="void store.setThemePreset(p.id)"
                >
                  <span class="preset-swatch" :style="{ background: p.color }"></span>
                  <span class="preset-name">{{ p.name }}</span>
                </button>
                <span class="theme-sublabel">渐变</span>
                <button
                  v-for="p in GRADIENT_PRESETS"
                  :key="p.id"
                  role="radio"
                  :aria-checked="themePreset === p.id"
                  :class="{ on: themePreset === p.id }"
                  @click="void store.setThemePreset(p.id)"
                >
                  <span class="preset-swatch" :style="{ background: p.gradient }"></span>
                  <span class="preset-name">{{ p.name }}</span>
                </button>
              </div>
            </div>

            <!-- ③ 强调色：预设档 + 取色器 + 重置 -->
            <div class="theme-row">
              <span class="theme-label">强调色</span>
              <div class="theme-accent">
                <div class="accent-dots" role="radiogroup" aria-label="强调色预设">
                  <button
                    v-for="c in ACCENT_PRESETS"
                    :key="c"
                    role="radio"
                    :aria-checked="themeAccent === c"
                    :class="{ on: themeAccent === c }"
                    :style="{ background: c }"
                    :title="c"
                    @click="void store.setAccentColor(c)"
                  ></button>
                </div>
                <div class="accent-custom">
                  <input
                    type="color"
                    :value="themeAccent ?? PRESET_ACCENT[themePreset]"
                    @input="onAccentInput"
                    aria-label="自定义强调色"
                  />
                  <button
                    v-if="themeAccent"
                    class="ghost-btn accent-reset"
                    @click="void store.setAccentColor(null)"
                  >
                    重置
                  </button>
                </div>
              </div>
            </div>
          </div>

          <!-- ④ 字体大小：全局 + 单模块（模块系数为相对全局的额外缩放，默认 100%）；折叠块默认收起 -->
          <div class="setting-group font-group">
            <button
              class="font-group-head"
              type="button"
              :aria-expanded="fontExpanded"
              @click="fontExpanded = !fontExpanded"
            >
              <h4 class="font-group-title">字体大小</h4>
              <ChevronDown
                :size="14"
                :stroke-width="2"
                class="font-group-chevron"
                :class="{ open: fontExpanded }"
              />
            </button>
            <div v-show="fontExpanded" class="font-group-body">
              <div class="font-row">
                <span class="font-label">全局字体大小</span>
                <div class="font-edit">
                  <input
                    class="opacity-slider"
                    type="range"
                    min="0.85"
                    max="1.3"
                    step="0.01"
                    :value="store.state.config.font_scale"
                    aria-label="全局字体大小"
                    @input="onFontScaleInput"
                  />
                  <span class="opacity-value">{{ Math.round(store.state.config.font_scale * 100) }}%</span>
                </div>
              </div>
              <div v-for="m in FONT_MODULES" :key="m.key" class="font-row">
                <span class="font-label">{{ m.label }}</span>
                <div class="font-edit">
                  <input
                    class="opacity-slider"
                    type="range"
                    min="0.85"
                    max="1.3"
                    step="0.01"
                    :value="store.state.config[m.configKey]"
                    :aria-label="m.label + '字体大小'"
                    @input="onModuleFontInput(m.key, $event)"
                  />
                  <span class="opacity-value">{{ Math.round(store.state.config[m.configKey] * 100) }}%</span>
                </div>
              </div>
            </div>
          </div>
        </section>

        <!-- 工作台 -->
        <section id="sv-sec-workbench" class="sv-sec" aria-label="工作台">
          <h3 class="sv-sec-title">工作台</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">自定义布局</span>
              <span class="setting-desc">拖拽排列主界面的模块位置与显隐（时钟、待办、提示词等），推荐布局为 12×15 棋盘，完成后回到主页面</span>
            </div>
            <button class="ghost-btn data-btn" @click="emit('open-layout-editor')">打开编辑器</button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">倒计时到点提示音</span>
              <span class="setting-desc">到点时额外播放提示音（默认关闭）</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.countdown_sound"
              :class="{ on: store.state.config.countdown_sound }"
              @click="onToggleCountdownSound"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">时钟卡片语录</span>
              <span class="setting-desc">工作台时间卡片下方显示的一句话（留空则显示随机名言金句，点击可换一条）</span>
            </div>
            <div class="quote-edit">
              <input
                v-model="clockQuote"
                class="field-input"
                type="text"
                maxlength="50"
                placeholder="留空显示随机名言金句"
                spellcheck="false"
                @blur="commitClockQuote"
                @keydown.enter="commitClockQuote"
              />
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">名言来源</span>
              <span class="setting-desc">时钟卡片语录：在线随机名言，或仅用本地内置金句（点击语录可随机换一条）</span>
            </div>
            <AppSelect
              :model-value="quoteSource"
              :options="QUOTE_SOURCE_OPTIONS"
              aria-label="名言来源"
              @update:model-value="onQuoteSourceChange"
            />
          </div>
        </section>

        <!-- 快捷键 -->
        <section id="sv-sec-shortcut" class="sv-sec" aria-label="快捷键">
          <h3 class="sv-sec-title">快捷键</h3>
          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">全局快捷键</span>
              <span class="setting-desc">支持手动输入或按键录入，无冲突自动保存</span>
            </div>
            <div class="shortcut-edit">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="shortcutInputRef"
                  v-model="shortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="shortcutListening"
                  placeholder="Ctrl+Shift+Space"
                  @keydown="onShortcutKeydown"
                  @keydown.enter="commitShortcut"
                  @blur="onShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListeningShortcut">
                  {{ shortcutListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
            </div>
          </div>
          <p v-if="shortcutError" class="shortcut-error">{{ shortcutError }}</p>

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">剪贴板呼出快捷键</span>
              <span class="setting-desc">任何应用中一键唤起剪贴板历史浮层</span>
            </div>
            <div class="shortcut-edit">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="clipInputRef"
                  v-model="clipShortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="clipListening"
                  placeholder="Ctrl+`"
                  @keydown="onClipShortcutKeydown"
                  @keydown.enter="commitClipShortcut"
                  @blur="onClipShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListenClipShortcut">
                  {{ clipListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
            </div>
          </div>
          <p v-if="clipError" class="shortcut-error">{{ clipError }}</p>
        </section>

        <!-- 剪贴板 -->
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

        <!-- 联网 -->
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

        <!-- 数据 -->
        <section id="sv-sec-data" class="sv-sec" aria-label="数据">
          <h3 class="sv-sec-title">数据</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据备份</span>
              <span class="setting-desc">数据库与图标，保存到本地任意目录</span>
            </div>
            <button class="ghost-btn data-btn" @click="backupData">
              <Download :size="14" :stroke-width="2" />
              备份
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据恢复</span>
              <span class="setting-desc">从备份目录恢复，重启后生效</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :class="{ confirm: confirmRestore }"
              @click="restoreData"
            >
              <Upload :size="14" :stroke-width="2" />
              {{ confirmRestore ? '确认恢复？' : '恢复' }}
            </button>
          </div>

          <p class="settings-foot">
            <Lock :size="12" :stroke-width="2" class="settings-lock" aria-hidden="true" />
            所有数据默认存储在本地，不会上传云端
          </p>
        </section>

        <!-- 关于 -->
        <section id="sv-sec-about" class="sv-sec" aria-label="关于">
          <h3 class="sv-sec-title">关于</h3>
          <AboutSection />
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-view {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  overflow: hidden;
}
.sv-header {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
}
.sv-title {
  flex: 1;
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-1);
}

/* 双栏主体 */
.sv-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--space-4);
  overflow: hidden;
}
/* 左侧分类导航：纯文字列表 */
.sv-nav {
  flex-shrink: 0;
  width: 132px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
}
.sv-nav-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 8px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.sv-nav-item:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.sv-nav-item.active {
  background: var(--brand-50);
  color: var(--brand-500);
}

/* 右侧内容面板：全量渲染单列，内容放开全宽 */
.sv-content {
  position: relative;
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-6);
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-card);
}
.sv-sec {
  padding-bottom: var(--space-5);
}
.sv-sec + .sv-sec {
  border-top: 1px solid var(--border-soft);
  padding-top: var(--space-5);
  margin-top: var(--space-5);
}
/* 分类标题：品牌色短竖条 + 加粗大字，与下方设置项明确分层 */
.sv-sec-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 var(--space-4);
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
}
.sv-sec-title::before {
  content: '';
  width: 3px;
  height: 14px;
  border-radius: 2px;
  background: var(--brand-500);
  flex-shrink: 0;
}
/* 分类内的小组标题（如剪贴板「保留策略 / 记录行为 / 操作」） */
.sv-subtitle {
  margin: 16px 0 2px;
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.sv-subtitle:first-of-type {
  margin-top: 0;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 0;
}
.setting-row + .setting-row {
  border-top: 1px solid var(--border-soft);
}
.setting-info {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.setting-desc {
  flex-basis: 100%;
}
.setting-name {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-1);
}
.setting-desc {
  font-size: 0.75rem;
  color: var(--text-3);
}

/* 开关 */
.toggle {
  flex-shrink: 0;
  width: 40px;
  height: 22px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--border-strong);
  position: relative;
  cursor: pointer;
  padding: 0;
  transition: background 0.18s;
}
.toggle.on {
  background: var(--brand-500);
}
.toggle-knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  box-shadow: var(--shadow-dock);
  transition: transform 0.18s;
}
.toggle.on .toggle-knob {
  transform: translateX(18px);
}

.data-btn {
  padding: 7px 14px;
}.data-btn.confirm {
  background: var(--c-red);
  color: var(--text-on-accent);
}
.data-btn.confirm:hover {
  background: color-mix(in srgb, var(--c-red) 85%, #000);
  color: var(--text-on-accent);
}

.shortcut-row {
  align-items: flex-start;
}
.opacity-edit {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 240px;
  flex-shrink: 0;
}
.opacity-slider {
  flex: 1;
  min-width: 120px;
  accent-color: var(--brand-500);
  cursor: pointer;
}
.opacity-value {
  min-width: 40px;
  text-align: right;
  font-size: 0.78125rem;
  font-variant-numeric: tabular-nums;
  color: var(--text-2);
}
.font-group {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.font-group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0;
  border: none;
  background: transparent;
  cursor: pointer;
  color: inherit;
}
.font-group-title {
  margin: 0;
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-1);
}
.font-group-chevron {
  color: var(--text-3);
  transition: transform 0.18s ease-out;
}
.font-group-chevron.open {
  transform: rotate(180deg);
}
.font-group-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.font-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.font-label {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  flex-shrink: 0;
}
.font-edit {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 280px;
  flex-shrink: 0;
}
.quote-edit {
  min-width: 240px;
  flex-shrink: 0;
}
.weather-edit {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 340px;
  flex-shrink: 0;
}
.weather-edit .field-input {
  flex: 1;
  min-width: 0;
}
.num-edit {
  min-width: 120px;
  flex-shrink: 0;
}
.num-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  padding: 8px 10px;
  outline: none;
}
.num-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.data-btn.danger {
  color: var(--c-red-ink);
  border-color: color-mix(in srgb, var(--c-red) 35%, transparent);
}
.data-btn.danger:hover {
  background: var(--c-red-soft);
  border-color: transparent;
}
.shortcut-edit {
  min-width: 240px;
}
.shortcut-input-wrap {
  width: 100%;
  position: relative;
}
.shortcut-icon {
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-4);
}
.shortcut-input {
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  padding: 9px 84px 9px 32px;
  outline: none;
}
.shortcut-input:focus {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.shortcut-record-btn {
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  border: none;
  border-radius: var(--radius-sm);
  background: var(--brand-50);
  color: var(--brand-500);
  font-size: 0.75rem;
  padding: 5px 10px;
  cursor: pointer;
}
.shortcut-record-btn:disabled {
  opacity: 0.9;
}
.shortcut-record-btn:hover {
  background: color-mix(in srgb, var(--brand-500) 14%, transparent);
}
.shortcut-error {
  margin-top: -8px;
  font-size: 0.75rem;
  color: var(--c-red);
}

.settings-foot {
  margin-top: 16px;
  font-size: 0.75rem;
  color: var(--text-3);
  text-align: center;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
}
.settings-lock {
  flex-shrink: 0;
}

/* ---- 主题设置 ---- */
.theme-group {
  padding: 4px 0 8px;
}
/* 每行：左侧标签 + 右侧控件两端对齐 */
.theme-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 0;
}
.theme-row + .theme-row {
  border-top: 1px solid var(--border-soft);
}
.theme-label {
  flex-shrink: 0;
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-1);
}
/* 整块内容（如色卡）：标签在上方，内容左对齐铺开 */
.theme-row-stack {
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
}
/* 色卡分组小标签（单色/渐变）：占满整行，换行显示 */
.theme-sublabel {
  flex-basis: 100%;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-3);
}
.theme-sublabel + button {
  margin-top: -2px;
}

/* 明暗模式分段选择 */
.theme-mode-seg {
  display: inline-flex;
  gap: 6px;
  background: var(--bg-card-soft);
  padding: 3px;
  border-radius: var(--radius-pill);
}
.theme-mode-seg button {
  padding: 5px 14px;
  border-radius: inherit;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.theme-mode-seg button.on {
  background: var(--brand-500);
  color: var(--text-on-accent);
}

/* 主题配色色卡：固定尺寸保证长宽一致，支持换行 */
.theme-presets {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.theme-presets button {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  width: 64px;
  height: 64px;
  justify-content: center;
  padding: 0 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-solid);
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.theme-presets button.on {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
}
.preset-swatch {
  width: 22px;
  height: 22px;
  border-radius: 50%;
}
.preset-name {
  font-size: 0.6875rem;
  color: var(--text-3);
}
.theme-presets button.on .preset-name {
  color: var(--text-1);
}

/* 强调色 */
.theme-accent {
  display: flex;
  align-items: center;
  gap: 12px;
}
.accent-dots {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.accent-dots button {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: none;
  cursor: pointer;
  transition: transform 0.12s;
}
.accent-dots button:hover {
  transform: scale(1.1);
}
.accent-dots button.on {
  box-shadow: 0 0 0 2px var(--bg-card-solid), 0 0 0 4px var(--brand-500);
}
.accent-custom {
  display: flex;
  align-items: center;
  gap: 8px;
}
.accent-custom input[type='color'] {
  width: 28px;
  height: 28px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  padding: 2px;
  cursor: pointer;
}
.accent-reset {
  padding: 4px 10px;
  font-size: 0.75rem;
}

</style>
