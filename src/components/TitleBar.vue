<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'
import { Copy, MessageSquare, Minus, Music2, Pause, Pin, PinOff, Play, Search, SkipBack, SkipForward, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useTitlebarWidget } from '../composables/useTitlebarWidget'
import type { TitlebarSegment, TitlebarWidget } from '../composables/useTitlebarWidget'
import TitlebarSegments from './TitlebarSegments.vue'

const store = useStore()
const { widgets: extWidgets, sendAction } = useTitlebarWidget()

/** 封面加载失败的 URL 集合（失效/断网/CSP 拦截 http://）：按 URL 记，换封面自然重试 */
const coverFailedUrls = ref(new Set<string>())

type WidgetPos = 'left' | 'center' | 'right'

/** 该扩展的小部件显示位（设置 → 外观「标题栏小部件」调整；缺省居中） */
function slotOf(extId: string): WidgetPos {
  const v = store.state.config.titlebar_widget_positions?.[extId]
  return v === 'left' || v === 'right' ? v : 'center'
}

/** 三个位（左/中/右）各显示一个：按到达顺序、同一位置后到者胜——
 *  同位竞争者不渲染（不叠加），在设置里挪开位置即恢复显示 */
const placedWidgets = computed(() => {
  const byPos = new Map<WidgetPos, { extId: string; widget: TitlebarWidget }>()
  // readonly(ref) 的 DeepReadonly 与段判别联合类型在数组层不兼容；渲染层只读不写，断言一次即可
  for (const e of extWidgets.value) byPos.set(slotOf(e.extId), e as unknown as { extId: string; widget: TitlebarWidget })
  return (['left', 'center', 'right'] as const).flatMap((pos) => {
    const e = byPos.get(pos)
    return e ? [{ pos, extId: e.extId, widget: e.widget }] : []
  })
})

/** media 档进度 0..=1（无时长 = 直播流，隐藏进度线） */
function mediaProgress(w: TitlebarWidget): number {
  if (w.kind !== 'media' || !w.durationMs) return 0
  return Math.min(1, (w.positionMs ?? 0) / w.durationMs)
}

/** status 档进度 0..=1（缺省 null = 不画进度；扁平模式贴底那条用） */
function statusProgress(w: TitlebarWidget): number | null {
  if (w.kind !== 'status' || typeof w.progress !== 'number') return null
  return Math.min(1, Math.max(0, w.progress))
}

/** status 档是否走段式排布（segments 存在且有内容时，完全代替扁平字段渲染） */
function statusSegments(w: TitlebarWidget): TitlebarSegment[] | null {
  if (w.kind !== 'status' || !w.segments?.length) return null
  // readonly(ref) 的 DeepReadonly 与段判别联合类型在数组层不兼容；
  // 渲染层只读不写，这里断言一次即可（运行时不产生拷贝）
  return w.segments as unknown as TitlebarSegment[]
}

function onMediaToggle(e: { extId: string; widget: TitlebarWidget }) {
  sendAction(e.extId, e.widget.playing ? 'pause' : 'play')
}
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

defineEmits<{
  (e: 'search'): void
  (e: 'chat'): void
}>()

const alwaysOnTop = computed(() => store.state.config.window.always_on_top)

function toggleAlwaysOnTop() {
  void store.setAlwaysOnTop(!alwaysOnTop.value)
}

const appWindow = isTauri() ? getCurrentWindow() : null

// ---- 窗口拖动：data-tauri-drag-region 只对 mousedown 的精确目标生效，
// 点击标题栏内的子元素（svg/span）时不触发；改用 startDragging 统一处理
// ---- 双击空白处最大化/还原：第一次按下即进入原生拖动循环，浏览器收不到
// 完整的 click 序列、dblclick 事件不可靠，改按按压时序+位移手动判定 ----
const DBLCLICK_MS = 500
const DBLCLICK_SLOP_PX = 4
let lastTitlePress: { t: number; x: number; y: number } | null = null

function onDragStart(e: MouseEvent) {
  if (!appWindow || e.button !== 0) return
  const target = e.target as HTMLElement
  // 小部件是扩展的交互面（按钮/文字），整块不参与窗口拖动与双击最大化
  if (target.closest('button') || target.closest('.tb-widget')) {
    lastTitlePress = null
    return
  }
  const now = performance.now()
  const prev = lastTitlePress
  lastTitlePress = { t: now, x: e.clientX, y: e.clientY }
  if (
    prev &&
    now - prev.t <= DBLCLICK_MS &&
    Math.abs(e.clientX - prev.x) <= DBLCLICK_SLOP_PX &&
    Math.abs(e.clientY - prev.y) <= DBLCLICK_SLOP_PX
  ) {
    lastTitlePress = null
    toggleMaximize()
    return
  }
  appWindow.startDragging()
}

// ---- 最大化状态（切换图标：最大化 ⇄ 还原） ----
const isMaximized = ref(false)
let unlistenResize: (() => void) | null = null

async function refreshMaximized() {
  if (!appWindow) return
  isMaximized.value = await appWindow.isMaximized()
}

onMounted(async () => {
  if (!appWindow) return
  await refreshMaximized()
  unlistenResize = await appWindow.onResized(() => refreshMaximized())
})

onBeforeUnmount(() => {
  unlistenResize?.()
})

function minimize() {
  if (isTauri()) tauriApi.minimizeWindow()
}

function toggleMaximize() {
  if (isTauri()) tauriApi.toggleMaximize()
}

function close() {
  if (isTauri()) {
    tauriApi.hideToTray()
    // 首次关闭时提示已最小化到托盘，避免用户误以为应用退出了
    if (!localStorage.getItem('tray-hint-shown')) {
      localStorage.setItem('tray-hint-shown', '1')
      showToast('已最小化到系统托盘，右键托盘图标可退出')
    }
  }
}
</script>

<template>
  <div class="title-bar" @mousedown="onDragStart">
    <div class="window-title">
      <span class="title-text">X-Hub</span>
    </div>
    <!-- 扩展标题栏小部件：media（媒体控制档）/ status（通用信息档），由常驻扩展经
         titlebar.set 推上标题栏。三个显示位（左/中/右）最多同显 3 个，位置由用户在
         设置 → 外观调整（缺省居中），同位后到者胜。按钮点击经 titlebar.action 定向
         事件回传给占用扩展（media 固定三键约定 id：previous / play|pause / next）。
         挂载与常驻闸见 useTitlebarWidget -->
    <div
      v-for="w in placedWidgets"
      :key="w.extId"
      class="tb-widget"
      :class="[`tb-${w.widget.kind}`, `tb-pos-${w.pos}`, { 'tb-bordered': store.state.config.titlebar_widget_border !== false, 'tb-solo': placedWidgets.length === 1 }]"
    >
      <template v-if="w.widget.kind === 'media'">
        <img
          v-if="w.widget.cover && !coverFailedUrls.has(w.widget.cover)"
          class="tb-cover"
          :src="w.widget.cover"
          referrerpolicy="no-referrer"
          alt=""
          @error="coverFailedUrls.add(w.widget.cover)"
        />
        <span v-else class="tb-cover tb-cover-fallback" aria-hidden="true">
          <Music2 :size="13" :stroke-width="1.8" />
        </span>
        <span class="tb-text">
          <span class="tb-title">{{ w.widget.title }}</span>
          <span v-if="w.widget.subtitle" class="tb-subtitle">{{ w.widget.subtitle }}</span>
        </span>
        <span class="tb-btns">
          <button class="tb-btn" title="上一曲" @click="sendAction(w.extId, 'previous')">
            <SkipBack :size="13" :stroke-width="2" />
          </button>
          <button
            class="tb-btn tb-btn-primary"
            :title="w.widget.playing ? '暂停' : '播放'"
            @click="onMediaToggle(w)"
          >
            <Pause v-if="w.widget.playing" :size="13" :stroke-width="2" />
            <Play v-else :size="13" :stroke-width="2" />
          </button>
          <button class="tb-btn" title="下一曲" @click="sendAction(w.extId, 'next')">
            <SkipForward :size="13" :stroke-width="2" />
          </button>
        </span>
        <span
          v-if="mediaProgress(w.widget) > 0"
          class="tb-progress"
          :style="{ transform: `scaleX(${mediaProgress(w.widget)})` }"
        ></span>
      </template>
      <template v-else>
        <!-- 段式排布：扩展按 segments 顺序拼条，宿主只认几何原语 -->
        <TitlebarSegments
          v-if="statusSegments(w.widget)"
          :segments="statusSegments(w.widget)!"
          @action="(id: string) => sendAction(w.extId, id)"
        />
        <!-- 扁平排布（未传 segments 的老扩展）：图标 + 两行文本 + 按钮 + 贴底进度 -->
        <template v-else>
          <span v-if="w.widget.icon" class="tb-icon" aria-hidden="true">{{ w.widget.icon }}</span>
          <span class="tb-text">
            <span class="tb-title">{{ w.widget.title }}</span>
            <span v-if="w.widget.subtitle" class="tb-subtitle">{{ w.widget.subtitle }}</span>
          </span>
          <span v-if="w.widget.actions?.length" class="tb-btns">
            <button
              v-for="a in w.widget.actions"
              :key="a.id"
              class="tb-act"
              @click="sendAction(w.extId, a.id)"
            >{{ a.label }}</button>
          </span>
          <span
            v-if="statusProgress(w.widget) !== null"
            class="tb-progress"
            :style="{ transform: `scaleX(${statusProgress(w.widget)})` }"
          ></span>
        </template>
      </template>
    </div>
    <div class="window-controls">
      <button class="tool-btn" :title="`全局搜索 (${store.state.config.search_shortcut || 'Ctrl+K'}，可在设置中自定义)`" @click="$emit('search')">
        <Search :size="15" :stroke-width="1.8" />
      </button>
      <button class="tool-btn" data-chat-opener :title="`AI 对话 (${store.state.config.chat_shortcut || 'Ctrl+Shift+K'}，可在设置中自定义)`" @click="$emit('chat')">
        <MessageSquare :size="15" :stroke-width="1.8" />
      </button>
      <div class="tool-divider"></div>
      <button
        class="win-btn top-btn"
        :class="{ active: alwaysOnTop }"
        :title="alwaysOnTop ? '取消窗口置顶' : '窗口置顶'"
        @click="toggleAlwaysOnTop"
      >
        <Pin v-if="!alwaysOnTop" :size="14" :stroke-width="1.8" />
        <PinOff v-else :size="14" :stroke-width="1.8" />
      </button>
      <button class="win-btn minimize" title="最小化" @click="minimize">
        <Minus :size="15" :stroke-width="1.8" />
      </button>
      <button
        class="win-btn maximize"
        :title="isMaximized ? '还原' : '最大化'"
        @click="toggleMaximize"
      >
        <Square v-if="!isMaximized" :size="14" :stroke-width="1.8" />
        <Copy v-else :size="14" :stroke-width="1.8" />
      </button>
      <button class="win-btn close" title="关闭（最小化至托盘）" @click="close">
        <X :size="15" :stroke-width="1.8" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.title-bar {
  height: 48px;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-left: 18px;
  flex-shrink: 0;
  position: relative;
}
/* 铬件（标题栏）始终全透明：与背景（渐变/壁纸）构成同一个连续平面，表面只属于卡片（ADR 0003） */
.window-title {
  display: flex;
  align-items: center;
  gap: 10px;
}
.title-text {
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  letter-spacing: -0.2px;
}
/* ---- 扩展标题栏小部件：三个显示位（左/中/右）最多同显 3 个，位置由设置调整（缺省居中）。
     文字两端省略；720px 以下直接让位（太窄放不下）。
     边框可选（设置 → 外观，titlebar_widget_border，默认开）：tb-bordered 才垫
     玻璃底 + 描边（整块底座），关掉后内容直接浮在标题栏上（可读性由铬件文字光晕兜底） ---- */
.tb-widget {
  position: absolute;
  top: 50%;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 6px 0 5px;
  border-radius: 8px;
  overflow: hidden;
}
.tb-pos-center {
  left: 50%;
  transform: translate(-50%, -50%);
  max-width: clamp(120px, calc(100vw - 560px), 520px);
}
/* 左位：紧挨 X-Hub 标题（18px 内边距 + ~48px 文字）右侧；
   右位：窗口按钮区（38×2 + 分隔线 9 + 46×4 = 269px）左侧，各留 8px 间隙。
   多部件同显时侧位收窄给中间让空间（媒体内容约 330px 起，塞不下由文字段省略号兜底） */
.tb-pos-left {
  left: 82px;
  transform: translateY(-50%);
  max-width: clamp(120px, 24vw, 340px);
}
.tb-pos-right {
  right: 277px;
  transform: translateY(-50%);
  max-width: clamp(96px, 14vw, 200px);
}
/* 只有一个小部件时给全额宽度预算（与居中位同款）：单部件放左/右位也要显示得全 */
.tb-widget.tb-solo {
  max-width: clamp(120px, calc(100vw - 560px), 520px);
}
.tb-widget.tb-bordered {
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
}
@media (max-width: 719px) {
  .tb-widget {
    display: none;
  }
}
.tb-cover {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  object-fit: cover;
  flex-shrink: 0;
}
.tb-cover-fallback {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--brand-50);
  color: var(--brand-500);
}
.tb-icon {
  font-size: 14px;
  line-height: 1;
  flex-shrink: 0;
}
.tb-text {
  display: flex;
  flex-direction: column;
  justify-content: center;
  flex: 1;
  min-width: 0;
  line-height: 1.25;
}
.tb-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tb-subtitle {
  font-size: 11px;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* 段式排布的样式在 TitlebarSegments.vue（自包含，便于递归复用） */
.tb-btns {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.tb-btn {
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 6px;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.tb-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.tb-btn-primary {
  background: var(--brand-500);
  color: var(--text-on-accent);
}
.tb-btn-primary:hover {
  background: var(--brand-600);
  color: var(--text-on-accent);
}
.tb-act {
  height: 22px;
  padding: 0 8px;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  background: transparent;
  font-size: 11px;
  color: var(--text-2);
  cursor: pointer;
  white-space: nowrap;
  transition: border-color 0.15s, color 0.15s;
}
.tb-act:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
/* 进度线贴部件底边，scaleX 驱动（周期更新走 transform，不触发布局重排，见约定 19） */
.tb-progress {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 2px;
  background: var(--brand-500);
  transform-origin: left;
  pointer-events: none;
}
.window-controls {
  display: flex;
  align-items: center;
  height: 100%;
}
.tool-btn {
  width: 38px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.tool-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
[data-theme="dark"] .tool-btn:hover {
  background: var(--brand-50);
  color: var(--text-1);
}
.tool-divider {
  width: 1px;
  height: 18px;
  background: var(--border-soft);
  margin: 0 4px;
}
.win-btn {
  width: 46px;
  height: 100%;
  border: none;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.win-btn:hover {
  background: var(--bg-card-soft);
}
.win-btn.top-btn.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.top-btn.active:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.win-btn.close:hover {
  background: var(--window-close);
  color: var(--text-on-accent);
}
</style>
