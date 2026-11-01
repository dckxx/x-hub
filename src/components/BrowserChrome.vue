<script setup lang="ts">
// 独立「应用内浏览器」窗口顶栏（ADR 0011）。每扇池窗口 = Rust 侧预创建的 Window + 两个
// 子 webview：本组件跑在 chrome webview（label suda-web-{i}-chrome，全窗尺寸、渲染顶部条），
// 外站页面在 content webview（suda-web-{i}-content，位于顶栏下方，边界由 Rust 维护）。
// 本页只负责：tab 条（≥2 个才出现）、地址栏、前进/后退/刷新、系统浏览器出口、关闭窗口。
// tab/导航的真源在 Rust（suda_browser.rs 的 SLOTS），经命令 + 定向事件同步。
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { ArrowLeft, ArrowRight, ExternalLink, RotateCw, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { reportClientError } from '../utils/error-report'
import { useShellThemeConfig } from '../composables/useShellThemeConfig'

// 池窗口是正常窗口：进任务栏、可 Alt+Tab 唤回（区别于应用内其他浮窗）
// 主题走轻量自举（chrome.html 不加载 store——原先 useTheme() 会把整份 store
// 拖进这个只有一条顶栏的 webview，见 useShellThemeConfig 注释）
useShellThemeConfig()

const m = getCurrentWebview().label.match(/^suda-web-(\d+)-chrome$/)
const slot = m ? Number(m[1]) : -1

const tabs = ref<string[]>([])
const active = ref(0)
const titles = ref<Record<string, string>>({})
const urlInput = ref('')
const editingUrl = ref(false)
/** 导航失败标记：地址栏短暂变红（轻量窗无 toast 体系，不改变高度） */
const navError = ref(false)
let navErrorTimer = 0

const currentUrl = computed(() => tabs.value[active.value] ?? '')
const showTabs = computed(() => tabs.value.length > 1)

function hostOf(u: string): string {
  try {
    return new URL(u).hostname.replace(/^www\./, '')
  } catch {
    return u
  }
}

function labelOf(u: string): string {
  return titles.value[u] || hostOf(u) || u
}

const rootRef = ref<HTMLElement | null>(null)
let ro: ResizeObserver | null = null
let lastReported = 0
const unlisteners: Array<() => void> = []

function reportHeight() {
  if (!isTauri() || slot < 0) return
  const h = rootRef.value?.offsetHeight ?? 0
  if (!h || Math.abs(h - lastReported) < 1) return
  lastReported = h
  void tauriApi.sudaBrowserChromeHeight(slot, h).catch(() => {})
}

onMounted(async () => {
  if (rootRef.value) {
    ro = new ResizeObserver(reportHeight)
    ro.observe(rootRef.value)
  }
  if (!isTauri() || slot < 0) return
  // 显示前的事件不补发：挂载时拉一次当前状态（与 clipboard-shown 同款约定）
  try {
    const snap = await tauriApi.sudaBrowserState(slot)
    tabs.value = snap.tabs
    active.value = snap.active
    urlInput.value = snap.tabs[snap.active] ?? ''
  } catch {
    /* 窗口槽位未就绪：保持空态 */
  }
  reportHeight()
  // 四个监听是一个整体：任一注册失败（如 capability 缺失导致 ACL 拒绝）页面就收不到
  // 任何同步事件、永远停在空态——必须上报，不能静默吞掉
  try {
    unlisteners.push(
      await listen<{ slot: number; tabs: string[]; active: number }>('suda-browser-tabs', (e) => {
        if (e.payload.slot !== slot) return
        tabs.value = e.payload.tabs
        active.value = e.payload.active
        if (!editingUrl.value) urlInput.value = e.payload.tabs[e.payload.active] ?? ''
      }),
    )
    unlisteners.push(
      await listen<{ slot: number; url: string }>('suda-browser-nav', (e) => {
        if (e.payload.slot !== slot) return
        if (!e.payload.url || e.payload.url === 'about:blank') return
        if (!editingUrl.value) urlInput.value = e.payload.url
      }),
    )
    unlisteners.push(
      await listen<{ slot: number; title: string }>('suda-browser-title', (e) => {
        if (e.payload.slot !== slot) return
        const u = urlInput.value || currentUrl.value
        if (u && e.payload.title) titles.value = { ...titles.value, [u]: e.payload.title }
      }),
    )
    unlisteners.push(
      await listen<{ slot: number; url: string }>('suda-browser-newwindow', (e) => {
        // 宿主接管 target=_blank（ADR 0011：不能指望页面自己弹窗）→ 落成本窗口新 tab
        if (e.payload.slot !== slot || !e.payload.url) return
        void tauriApi.sudaBrowserOpenTab(slot, e.payload.url).catch(() => {})
      }),
    )
  } catch (e) {
    void reportClientError('浏览器顶栏事件监听注册失败（tab/地址栏将无法同步）', e)
  }
})

onBeforeUnmount(() => {
  ro?.disconnect()
  window.clearTimeout(navErrorTimer)
  for (const off of unlisteners) off()
})

async function activateTab(i: number) {
  if (i === active.value) return
  await tauriApi.sudaBrowserActivateTab(slot, i).catch(() => {})
}

async function closeTab(i: number) {
  await tauriApi.sudaBrowserCloseTab(slot, i).catch(() => {})
}

async function goBack() {
  await tauriApi.sudaBrowserBack(slot).catch(() => {})
}
async function goForward() {
  await tauriApi.sudaBrowserForward(slot).catch(() => {})
}
async function reload() {
  await tauriApi.sudaBrowserReload(slot).catch(() => {})
}

async function submitUrl() {
  editingUrl.value = false
  const raw = urlInput.value.trim()
  if (!raw) return
  const u = /^https?:\/\//i.test(raw) ? raw : `https://${raw}`
  try {
    await tauriApi.sudaBrowserNavigate(slot, u)
  } catch (e) {
    // 就地提示：地址栏短暂变红 + title 带原因（如「窗口没有打开的标签」）
    navError.value = true
    window.clearTimeout(navErrorTimer)
    navErrorTimer = window.setTimeout(() => (navError.value = false), 2400)
    void e
  }
}

async function openInSystem() {
  await tauriApi.sudaBrowserOpenSystem(slot).catch(() => {})
}

async function closeWindow() {
  await tauriApi.sudaBrowserClose(slot).catch(() => {})
}
</script>

<template>
  <div ref="rootRef" class="suda-chrome">
    <!-- 轻量 tab 条：本窗口第二个页面出现时才渲染（ADR 0011） -->
    <nav v-if="showTabs" class="sc-tabs" aria-label="标签页">
      <button
        v-for="(t, i) in tabs"
        :key="t + i"
        class="sc-tab"
        :class="{ active: i === active }"
        type="button"
        :title="t"
        @click="activateTab(i)"
        @auxclick.middle.prevent="closeTab(i)"
      >
        <span class="sc-tab-label">{{ labelOf(t) }}</span>
        <span class="sc-tab-close" role="button" aria-label="关闭标签" @click.stop="closeTab(i)">
          <X :size="11" :stroke-width="2.2" />
        </span>
      </button>
    </nav>
    <header class="sc-bar">
      <button class="icon-btn" type="button" title="后退" @click="goBack">
        <ArrowLeft :size="15" :stroke-width="2" />
      </button>
      <button class="icon-btn" type="button" title="前进" @click="goForward">
        <ArrowRight :size="15" :stroke-width="2" />
      </button>
      <button class="icon-btn" type="button" title="重新加载" @click="reload">
        <RotateCw :size="14" :stroke-width="2" />
      </button>
      <input
        v-model="urlInput"
        class="sc-url"
        :class="{ 'nav-error': navError }"
        type="text"
        spellcheck="false"
        :title="navError ? '导航失败：请检查地址后重试' : currentUrl"
        @focus="editingUrl = true"
        @blur="submitUrl"
        @keydown.enter.prevent="submitUrl"
      />
      <button class="icon-btn" type="button" title="用系统浏览器打开" @click="openInSystem">
        <ExternalLink :size="15" :stroke-width="2" />
      </button>
      <button class="icon-btn" type="button" title="关闭窗口" @click="closeWindow">
        <X :size="16" :stroke-width="2" />
      </button>
    </header>
    <div v-if="!tabs.length" class="sc-empty">此窗口当前没有打开的页面，可从「速达」重新打开网页。</div>
  </div>
</template>

<style scoped>
/* ⚠️ 根元素必须 height: auto（按内容自高）：本 webview 的边界由 Rust 收缩到上报的高度，
   根元素若撑满视口（100vh）就变成循环测量——首次上报量到全窗高、clamp 后 webview 收缩、
   100vh 跟着变小、再上报新值，平衡在 clamp 上限（实测踩过：工具栏下多出一段应用底色空白）。
   auto 让 offsetHeight 恒等于工具栏（+tab 条）的真实高度，上报值稳定。 */
.suda-chrome {
  display: flex;
  flex-direction: column;
  background: var(--app-bg);
  color: var(--text-1);
  overflow: hidden;
  user-select: none;
}

.sc-tabs {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  padding: 6px 8px 0;
}

.sc-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 180px;
  min-width: 0;
  height: 30px;
  padding: 0 8px 0 12px;
  border: none;
  border-radius: 8px 8px 0 0;
  background: transparent;
  color: var(--text-3);
  font-size: 11px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.sc-tab:hover {
  background: var(--bg-card-soft);
  color: var(--text-2);
}

.sc-tab.active {
  background: var(--bg-card-soft);
  color: var(--text-1);
}

.sc-tab-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  text-align: left;
}

.sc-tab-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 4px;
  opacity: 0.55;
}

.sc-tab-close:hover {
  opacity: 1;
  background: var(--border-soft);
}

.sc-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 10px;
}

.sc-url {
  flex: 1;
  min-width: 0;
  height: 30px;
  margin: 0 4px;
  padding: 0 12px;
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  background: var(--bg-card-soft);
  color: var(--text-1);
  font-size: 12px;
  outline: none;
  transition: border-color 0.18s, box-shadow 0.18s;
}

.sc-url:focus {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 2px var(--brand-50);
}

.sc-url.nav-error {
  border-color: var(--c-red);
  box-shadow: 0 0 0 2px var(--c-red-soft, var(--c-red));
}

.sc-empty {
  padding: 24px 16px;
  font-size: 12px;
  color: var(--text-3);
  text-align: center;
}
</style>
