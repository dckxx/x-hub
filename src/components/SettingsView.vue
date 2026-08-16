<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { Download, Keyboard, Lock, Upload } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import AppSelect from './AppSelect.vue'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 分类导航（左侧分类 = 右侧区块锚点，点击平滑滚动定位，不做内容切换） ----
const SECTIONS = [
  { id: 'general', label: '通用' },
  { id: 'appearance', label: '外观' },
  { id: 'workbench', label: '工作台' },
  { id: 'shortcut', label: '快捷键' },
  { id: 'data', label: '数据' },
] as const

type SectionId = (typeof SECTIONS)[number]['id']
const activeSection = ref<SectionId>('general')
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

const IS_MAC =
  /Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform)

function normalizeShortcutDisplay(s: string): string {
  if (IS_MAC) return s
  return s
    .split('+')
    .map((p) => (p === 'CommandOrControl' ? 'Ctrl' : p))
    .join('+')
}

const shortcut = ref(normalizeShortcutDisplay(store.state.config.global_shortcut))
const savedShortcut = ref(shortcut.value)
const shortcutError = ref('')
const shortcutListening = ref(false)
const shortcutSaving = ref(false)
const previousShortcut = ref('')
const shortcutInputRef = ref<HTMLInputElement | null>(null)
const pressedShortcutKeys = ref(new Set<string>())
const shortcutNormalized = computed(() => shortcut.value.trim())

onMounted(async () => {
  if (!isTauri()) return
  shortcut.value = normalizeShortcutDisplay(await tauriApi.getGlobalShortcut())
})

// ---- 主页面「中上区块」显示内容（Token 统计 / 速记统计 / 待办概览 / 速达数量 / 倒计时） ----
const DASH_MID_OPTIONS = [
  { value: 'token', label: 'Token 统计' },
  { value: 'notes', label: '速记统计' },
  { value: 'todo', label: '待办概览' },
  { value: 'resources', label: '速达数量' },
  { value: 'countdown', label: '倒计时' },
] as const

function onDashboardMidChange(value: string) {
  void store.setDashboardMidContent(value)
  showToast(`主页面中上区块已切换为 ${
    DASH_MID_OPTIONS.find((o) => o.value === value)?.label ?? value
  }`)
}

function onToggleCountdownSound() {
  void store.setCountdownSound(!store.state.config.countdown_sound)
}

function onToggleSidebar() {
  void store.setSidebarToggle(!store.state.config.sidebar_toggle)
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
  showToast(value ? '时钟卡片语录已更新' : '时钟卡片语录已恢复默认')
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

// 自动保存：输入回车/失焦或录入完成后调用，无异常即生效，冲突或非法仅行内提示
async function commitShortcut() {
  if (!isTauri() || shortcutSaving.value) return
  const value = shortcutNormalized.value
  if (!value || value === savedShortcut.value) {
    shortcutError.value = ''
    return
  }
  shortcutSaving.value = true
  shortcutError.value = ''
  try {
    const saved = await store.setGlobalShortcut(value)
    savedShortcut.value = normalizeShortcutDisplay(saved)
    shortcut.value = savedShortcut.value
    showToast(`快捷键已更新为 ${saved}`)
  } catch (e) {
    shortcutError.value = String(e)
    void reportClientError('设置全局快捷键失败', e)
  } finally {
    shortcutSaving.value = false
  }
}

function startListeningShortcut() {
  previousShortcut.value = shortcut.value
  shortcutListening.value = true
  shortcut.value = ''
  pressedShortcutKeys.value = new Set()
  void nextTick(() => shortcutInputRef.value?.focus())
}

function onShortcutBlur() {
  if (shortcutListening.value) {
    // 录制中焦点离开：取消录制并恢复原值
    shortcutListening.value = false
    pressedShortcutKeys.value = new Set()
    shortcut.value = previousShortcut.value
    return
  }
  void commitShortcut()
}

function normalizeShortcutKey(e: KeyboardEvent) {
  // 仅依据 e.key 判断修饰键，切勿使用 e.ctrlKey / e.metaKey 状态判断，
  // 否则组合键中的普通键（如 Ctrl 下的 K）会被误判为修饰键导致主键丢失
  switch (e.key) {
    case 'Control':
      return IS_MAC ? 'CommandOrControl' : 'Ctrl'
    case 'Meta':
      // macOS: Cmd；Windows: Win 键（插件在 Windows 上 Super 才映射 Win）
      return IS_MAC ? 'CommandOrControl' : 'Super'
    case 'Alt':
      return 'Alt'
    case 'Shift':
      return 'Shift'
    case ' ':
      return 'Space' // 插件只认 "SPACE"，不认空格字符
    default:
      return e.key.length === 1 ? e.key.toUpperCase() : e.key
  }
}

const MODIFIER_ORDER = ['CommandOrControl', 'Ctrl', 'Super', 'Alt', 'Shift']

function formatShortcutDisplay(keys: Set<string>) {
  const parts: string[] = []
  for (const mod of MODIFIER_ORDER) {
    if (keys.has(mod)) parts.push(mod)
  }
  for (const key of keys) {
    if (!MODIFIER_ORDER.includes(key)) parts.push(key)
  }
  return parts.join('+')
}

function onShortcutKeydown(e: KeyboardEvent) {
  if (!shortcutListening.value) return
  e.preventDefault()
  e.stopPropagation()
  if (e.key === 'Escape') {
    shortcutListening.value = false
    pressedShortcutKeys.value = new Set()
    shortcut.value = previousShortcut.value
    return
  }
  if (['Control', 'Meta', 'Alt', 'Shift'].includes(e.key)) {
    pressedShortcutKeys.value.add(normalizeShortcutKey(e))
    shortcut.value = formatShortcutDisplay(pressedShortcutKeys.value)
    return
  }
  // 主键按下即完成录制（一个快捷键只有一个主键），避免后续按键污染组合，随后自动保存
  pressedShortcutKeys.value.add(normalizeShortcutKey(e))
  const display = formatShortcutDisplay(pressedShortcutKeys.value)
  if (!display) return
  shortcut.value = display
  shortcutListening.value = false
  pressedShortcutKeys.value = new Set()
  void commitShortcut()
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
        <!-- 通用 -->
        <section id="sv-sec-general" class="sv-sec" aria-label="通用">
          <h3 class="sv-sec-title">通用</h3>
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

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">时钟卡片语录</span>
              <span class="setting-desc">工作台时间卡片下方显示的一句话（默认：日拱一卒，功不唐捐。）</span>
            </div>
            <div class="quote-edit">
              <input
                v-model="clockQuote"
                class="field-input"
                type="text"
                maxlength="50"
                placeholder="日拱一卒，功不唐捐。"
                spellcheck="false"
                @blur="commitClockQuote"
                @keydown.enter="commitClockQuote"
              />
            </div>
          </div>
        </section>

        <!-- 外观 -->
        <section id="sv-sec-appearance" class="sv-sec" aria-label="外观">
          <h3 class="sv-sec-title">外观</h3>
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
        </section>

        <!-- 工作台 -->
        <section id="sv-sec-workbench" class="sv-sec" aria-label="工作台">
          <h3 class="sv-sec-title">工作台</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">工作台中上区块</span>
              <span class="setting-desc">主页面中部展示的卡片内容（默认倒计时）</span>
            </div>
            <AppSelect
              class="setting-select"
              style="min-width: 220px"
              :model-value="store.state.config.dashboard_mid_content"
              :options="DASH_MID_OPTIONS"
              aria-label="首页中部区块展示内容"
              @update:model-value="onDashboardMidChange"
            />
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
  font-size: 18px;
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
  font-size: 13px;
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
  font-size: 15px;
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
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.setting-desc {
  font-size: 12px;
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
.quote-edit {
  min-width: 240px;
  flex-shrink: 0;
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
  font-size: 13px;
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
  font-size: 12px;
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
  font-size: 12px;
  color: var(--c-red);
}

.settings-foot {
  margin-top: 16px;
  font-size: 12px;
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
  font-size: 14px;
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
  font-size: 12px;
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
  font-size: 12px;
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
  font-size: 11px;
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
  font-size: 12px;
}
</style>
