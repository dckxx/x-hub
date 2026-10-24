<script setup lang="ts">
// 外观大类（主题 / 壁纸 / 字体 / 侧边栏）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { convertFileSrc } from '@tauri-apps/api/core';
import { ChevronDown } from 'lucide-vue-next';
import { isTauri, tauriApi, type ExtensionEntry } from '../../api/tauri';
import { useStore } from '../../stores/workbench';
import AppSelect from '../AppSelect.vue';
import { useTitlebarWidget } from '../../composables/useTitlebarWidget';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 应用壁纸与卡片玻璃透明度（见 docs/adr/0002：模糊作用于壁纸层整体） ----
// 壁纸单一套，所有主题模式共用同一张壁纸与蒙版
const wallpaperSrc = computed(() => {
  const p = store.state.config.wallpaper_path
  return p && isTauri() ? convertFileSrc(p) : ''
})

async function pickWallpaper() {
  try {
    const file = await open({
      multiple: false,
      directory: false,
      filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp'] }],
    })
    if (typeof file !== 'string') return
    const stored = await tauriApi.importWallpaper(file)
    await store.setWallpaper(stored)
    showToast('壁纸已更新')
  } catch (e) {
    showToast(`壁纸导入失败：${String(e)}`)
  }
}

async function clearWallpaper() {
  await store.setWallpaper('')
  try {
    // 先更新配置再清理目录：回收不再被引用的壁纸文件
    await tauriApi.cleanupWallpapers()
  } catch {
    // 目录已不存在等场景不阻塞：配置置空即达成清除
  }
  showToast('已清除壁纸')
}

function onToggleWallpaperBlur() {
  void store.setWallpaperBlur(!store.state.config.wallpaper_blur)
}

function onToggleWallpaperImmersive() {
  void store.setWallpaperImmersive(!store.state.config.wallpaper_immersive)
}

function onWallpaperVeilInput(e: Event) {
  void store.setWallpaperVeil(Number((e.target as HTMLInputElement).value))
}

function onGlassOpacityInput(e: Event) {
  void store.setGlassOpacity(Number((e.target as HTMLInputElement).value))
}

function onToggleSidebar() {
  void store.setSidebarToggle(!store.state.config.sidebar_toggle)
}

function onToggleTitlebarWidgetBorder() {
  void store.setTitlebarWidgetBorder(!store.state.config.titlebar_widget_border)
}

// ---- 标题栏小部件位置：三个显示位（左/中/右）最多同显 3 个，缺省居中。
//      行来自当前在显示的小部件（useTitlebarWidget 模块级状态与标题栏共享同一份）；
//      扩展名单独拉一次用于行名展示，拿不到就显示扩展 id ----
const { widgets: titlebarWidgets } = useTitlebarWidget()
const extNames = ref<Record<string, string>>({})
const WIDGET_POS_OPTIONS = [
  { value: 'left', label: '左侧' },
  { value: 'center', label: '居中' },
  { value: 'right', label: '右侧' },
] as const

const widgetRows = computed(() =>
  titlebarWidgets.value.map((w) => ({
    extId: w.extId,
    name: extNames.value[w.extId] ?? w.extId,
    pos: store.state.config.titlebar_widget_positions?.[w.extId] ?? 'center',
  })),
)

onMounted(async () => {
  if (!isTauri()) return
  try {
    const list: ExtensionEntry[] = await tauriApi.listExtensions()
    extNames.value = Object.fromEntries(list.map((e) => [e.id, e.name]))
  } catch {
    /* 名字拿不到时行名回退显示扩展 id */
  }
})

function onWidgetPos(extId: string, pos: string) {
  void store.setTitlebarWidgetSlot(extId, pos)
}

// ---- 字体大小（全局 + 单模块） ----
const FONT_MODULES = [
  { key: 'sticky', label: '便签', configKey: 'font_sticky' },
  { key: 'notes', label: '速记', configKey: 'font_notes' },
  { key: 'prompt', label: '提示词', configKey: 'font_prompt' },
  { key: 'todo', label: '待办', configKey: 'font_todo' },
] as const

type FontModuleKey = (typeof FONT_MODULES)[number]['key']

// 字体大小折叠块：默认展开（用户要求 —— 它是外观区最常改的一项，收起等于多一次点击）
const fontExpanded = ref(true)

function onFontScaleInput(e: Event) {
  void store.setFontScale(Number((e.target as HTMLInputElement).value))
}

function onModuleFontInput(key: FontModuleKey, e: Event) {
  void store.setModuleFontScale(key, Number((e.target as HTMLInputElement).value))
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

          <!-- 扩展推上标题栏的小部件（媒体控制/状态条）的外观：描边可选 -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">标题栏小部件边框</span>
              <span class="setting-desc">扩展展示在标题栏上的小部件（媒体控制/状态条）带玻璃底座与描边（默认开启）；关闭后去掉整个底座，内容直接浮在标题栏上</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.titlebar_widget_border !== false"
              :class="{ on: store.state.config.titlebar_widget_border !== false }"
              @click="onToggleTitlebarWidgetBorder"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <!-- 小部件位置：有扩展正在显示小部件时出现。左/中/右各显一个，缺省居中 -->
          <div v-for="row in widgetRows" :key="row.extId" class="setting-row">
            <div class="setting-info">
              <span class="setting-name">{{ row.name }}</span>
              <span class="setting-desc">这个小部件在标题栏上的显示位置（左 / 中 / 右各显示一个；同一位置被后推的小部件占用时会暂时让位，挪开即恢复）</span>
            </div>
            <AppSelect
              style="width: 108px"
              :model-value="row.pos"
              :options="WIDGET_POS_OPTIONS"
              :aria-label="`调整 ${row.name} 小部件的显示位置`"
              compact
              @update:model-value="onWidgetPos(row.extId, $event)"
            />
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

            <!-- ④ 应用壁纸：主窗口所有视图共用，浮窗不跟随；模糊为整屏静态层（ADR 0002）。
                 壁纸单一套，所有主题模式共用同一张图片与蒙版 -->
            <div class="theme-row theme-row-stack">
              <span class="theme-label">应用壁纸</span>
              <div class="wallpaper-box">
                <div class="wallpaper-preview" :class="{ empty: !wallpaperSrc }">
                  <img v-if="wallpaperSrc" :src="wallpaperSrc" alt="壁纸预览" />
                  <span v-else class="wallpaper-empty">未设置壁纸（当前使用主题渐变背景）</span>
                </div>
                <div class="wallpaper-actions">
                  <button class="ghost-btn" @click="pickWallpaper">
                    {{ wallpaperSrc ? '更换图片' : '选择图片' }}
                  </button>
                  <button v-if="wallpaperSrc" class="ghost-btn" @click="clearWallpaper">
                    清除壁纸
                  </button>
                </div>
                <div v-if="wallpaperSrc" class="setting-row wallpaper-blur-row">
                  <div class="setting-info">
                    <span class="setting-name">壁纸蒙版</span>
                    <span class="setting-desc">叠一层主题底色，改善文字与图标对比度；0% 壁纸最鲜亮，越高越接近原背景</span>
                  </div>
                  <div class="font-edit">
                    <input
                      class="opacity-slider"
                      type="range"
                      min="0"
                      max="0.85"
                      step="0.01"
                      :value="store.state.config.wallpaper_veil"
                      aria-label="壁纸蒙版"
                      @input="onWallpaperVeilInput"
                    />
                    <span class="opacity-value">{{ Math.round(store.state.config.wallpaper_veil * 100) }}%</span>
                  </div>
                </div>
                <div v-if="wallpaperSrc" class="setting-row wallpaper-blur-row">
                  <div class="setting-info">
                    <span class="setting-name">沉浸模式</span>
                    <span class="setting-desc">卡片改用真毛玻璃：壁纸在卡片间隙完整清晰展示，卡内文字自动可读（低端核显滚动可能掉帧）</span>
                  </div>
                  <button
                    class="toggle"
                    role="switch"
                    type="button"
                    :aria-checked="store.state.config.wallpaper_immersive"
                    :class="{ on: store.state.config.wallpaper_immersive }"
                    @click="onToggleWallpaperImmersive"
                  >
                    <span class="toggle-knob"></span>
                  </button>
                </div>
                <div v-if="wallpaperSrc && !store.state.config.wallpaper_immersive" class="setting-row wallpaper-blur-row">
                  <div class="setting-info">
                    <span class="setting-name">背景模糊</span>
                    <span class="setting-desc">柔化整张壁纸（含卡片间空隙透出的部分），文字更易读；沉浸模式下不生效</span>
                  </div>
                  <button
                    class="toggle"
                    role="switch"
                    type="button"
                    :aria-checked="store.state.config.wallpaper_blur"
                    :class="{ on: store.state.config.wallpaper_blur }"
                    @click="onToggleWallpaperBlur"
                  >
                    <span class="toggle-knob"></span>
                  </button>
                </div>
              </div>
            </div>

            <!-- ⑤ 卡片玻璃透明度：全局卡片透底程度，无壁纸时对渐变背景同样生效 -->
            <div class="theme-row">
              <span class="theme-label">卡片玻璃透明度</span>
              <div class="font-edit">
                <input
                  class="opacity-slider"
                  type="range"
                  min="0.4"
                  max="1"
                  step="0.01"
                  :value="store.state.config.glass_opacity"
                  aria-label="卡片玻璃透明度"
                  @input="onGlassOpacityInput"
                />
                <span class="opacity-value">{{ Math.round(store.state.config.glass_opacity * 100) }}%</span>
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
</template>
