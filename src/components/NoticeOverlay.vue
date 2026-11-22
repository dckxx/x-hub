<script setup lang="ts">
// 右下角自绘通知窗（label=notice）：跨 Win10/11 一致，替代系统 WinRT Toast。
// 后端每来一条通知 emit `notice-new`；本窗维护卡片堆叠、逐条定时淡出，
// 堆叠高度变化后回调后端 notice_layout 把窗口锚定到工作区右下角，队列空了 notice_dismiss 收起。
// 入/出场动画交给 <TransitionGroup>，用其 after-enter / after-leave 事件再量高（离场卡片仍占位到动画结束）。
import { nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { Bell, CheckCircle2, Clock, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'

// 标记为通知浮窗：窗口透明，body 透明，只显示卡片堆叠本体
document.documentElement.dataset.noticeWindow = ''

interface NoticeItem {
  uid: number
  kind: string
  title: string
  body: string
}

// 兜底驻留时长（后端每条通知都会带 durationMs，见 notify.rs；这里只防 payload 缺字段）
const FALLBACK_DISMISS_MS = 5200
const MAX_VISIBLE = 4 // 超出立即挤掉最旧，限制窗口高度

const items = ref<NoticeItem[]>([])
const stackEl = ref<HTMLElement | null>(null)
let uidSeq = 1
const timers = new Map<number, ReturnType<typeof setTimeout>>()

// ---------- 主题：独立窗口自取初始值 + 跟随主窗推送 ----------
function applyTheme(t: { mode?: string; preset?: string; accent?: string | null }) {
  const el = document.documentElement
  const systemDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
  const dark = t.mode === 'dark' || (t.mode === 'system' && systemDark)
  el.dataset.theme = dark ? 'dark' : ''
  if (t.preset) el.dataset.preset = t.preset
  if (t.accent) el.style.setProperty('--accent', t.accent)
  else el.style.removeProperty('--accent')
}

// ---------- 布局回调：把内容实测高度交给后端锚定 ----------
async function syncLayout() {
  if (!isTauri()) return
  await nextTick()
  if (items.value.length === 0) {
    void tauriApi.noticeDismiss().catch(() => {})
    return
  }
  const h = stackEl.value ? Math.ceil(stackEl.value.getBoundingClientRect().height) : 0
  if (h > 0) void tauriApi.noticeLayout(h).catch(() => {})
}

function startTimer(uid: number, ms: number) {
  timers.set(uid, setTimeout(() => dismiss(uid), ms))
}

function clearTimer(uid: number) {
  const t = timers.get(uid)
  if (t) {
    clearTimeout(t)
    timers.delete(uid)
  }
}

function dismiss(uid: number) {
  clearTimer(uid)
  // 直接从数组移除，离场动画由 TransitionGroup 播放，结束触发 after-leave → syncLayout
  items.value = items.value.filter((n) => n.uid !== uid)
}

function push(item: Omit<NoticeItem, 'uid'>, durationMs?: number) {
  const it: NoticeItem = { uid: uidSeq++, ...item }
  items.value.push(it)
  // 超量：立即挤掉最旧（TransitionGroup 会为其播放离场）
  while (items.value.length > MAX_VISIBLE) {
    const old = items.value.shift()
    if (old) clearTimer(old.uid)
  }
  startTimer(it.uid, durationMs && durationMs > 0 ? durationMs : FALLBACK_DISMISS_MS)
  // 新卡片入场后立即量高（入场动画只横向滑入，不影响布局高度）
  void syncLayout()
}

let unlistenNew: (() => void) | null = null
let unlistenTheme: (() => void) | null = null

onMounted(async () => {
  if (!isTauri()) return
  try {
    applyTheme(await tauriApi.getThemeConfig())
  } catch {
    /* 无后端时保持默认 */
  }
  unlistenNew = await listen<{ kind?: string; title?: string; body?: string; durationMs?: number }>('notice-new', (e) => {
    const p = e.payload ?? {}
    push({ kind: p.kind || 'info', title: p.title || '通知', body: p.body || '' }, p.durationMs)
  })
  unlistenTheme = await listen<{ mode?: string; preset?: string; accent?: string | null }>(
    'notice-theme',
    (e) => applyTheme(e.payload ?? {}),
  )
  // 监听就绪后告知后端：启动初期（本窗口 webview 尚未加载完）到达的通知会被
  // 后端暂存，见 notify.rs 的 pending 队列——不 ready 重放就会静默漏提醒
  // （倒计时/待办提醒触发后 remind_fired 已置位，丢了不会补发）
  void tauriApi.noticeReady().catch(() => {})
})

onBeforeUnmount(() => {
  unlistenNew?.()
  unlistenTheme?.()
  timers.forEach((t) => clearTimeout(t))
  timers.clear()
})

function iconFor(kind: string) {
  if (kind === 'countdown') return Clock
  if (kind === 'todo') return CheckCircle2
  return Bell
}
</script>

<template>
  <div class="notice-host">
    <!-- 量高 ref 必须挂在真实元素上：挂在 TransitionGroup 上拿到的是组件实例
         （无 defineExpose，没有 getBoundingClientRect），syncLayout 会抛
         TypeError，notice_layout 高度校正链路整体失效、窗口高度永远停在默认值 -->
    <div ref="stackEl" class="notice-stack-host">
      <TransitionGroup
        tag="div"
        name="notice"
        class="notice-stack"
        @after-enter="syncLayout"
        @after-leave="syncLayout"
      >
        <div
          v-for="n in items"
          :key="n.uid"
          class="notice-card"
          :class="`k-${n.kind}`"
          @click="dismiss(n.uid)"
        >
          <span class="nc-icon" :class="`k-${n.kind}`">
            <component :is="iconFor(n.kind)" :size="18" :stroke-width="2" />
          </span>
          <div class="nc-text">
            <div class="nc-title">{{ n.title }}</div>
            <div v-if="n.body" class="nc-body">{{ n.body }}</div>
          </div>
          <button class="nc-close" type="button" aria-label="关闭" @click.stop="dismiss(n.uid)">
            <X :size="14" :stroke-width="2" />
          </button>
        </div>
      </TransitionGroup>
    </div>
  </div>
</template>

<style scoped>
.notice-host {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  background: transparent;
  user-select: none;
}
/* 量高层：包裹栈体、高度随内容收缩（syncLayout 量它的 rect） */
.notice-stack-host {
  flex: none;
}
.notice-stack {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 6px 8px 8px;
}
.notice-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px 12px 12px 14px;
  border-radius: 14px;
  /* 通知窗是透明小窗，背后是桌面而非应用底色/壁纸蒙版：--bg-card-solid 暗色只有 20% 白，
     落在桌面壁纸上几乎隐形（实测「背景透明看不清」）。改用浮窗专用不透明面板底
     （口径同 GlobalSearch 透明小窗分支）；白描边在浅色桌面上呈粉笔框，一并去掉，
     卡片层次靠阴影 + 左侧强调条。玻璃透明度/沉浸模式是主窗壁纸语境参数，通知窗不读，
     任何主题下都保持这层实底 */
  background: var(--bg-float-panel);
  border: none;
  box-shadow: var(--shadow-card);
  cursor: pointer;
  position: relative;
  overflow: hidden;
}
/* 左侧一条按类型着色的强调条 */
.notice-card::before {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 4px;
  background: var(--brand-500);
}
.notice-card.k-todo::before {
  background: var(--c-green-ink, var(--brand-500));
}
.notice-card.k-info::before {
  background: var(--accent, var(--brand-500));
}
.nc-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 9px;
  flex: none;
  color: #fff;
  background: var(--brand-500);
}
.nc-icon.k-todo {
  background: var(--c-green-ink, var(--brand-500));
}
.nc-icon.k-info {
  background: var(--accent, var(--brand-500));
}
.nc-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.nc-title {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nc-body {
  font-size: 0.78rem;
  line-height: 1.4;
  color: var(--text-2);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.nc-close {
  flex: none;
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.nc-close:hover {
  background: var(--bg-hover, rgba(127, 127, 127, 0.14));
  color: var(--text-1);
}

/* 进出场：从右侧滑入、向左滑出并淡出 */
.notice-enter-from {
  opacity: 0;
  transform: translateX(110%);
}
.notice-enter-to {
  opacity: 1;
  transform: translateX(0);
}
.notice-leave-from {
  opacity: 1;
  transform: translateX(0);
}
.notice-leave-to {
  opacity: 0;
  transform: translateX(110%);
}
.notice-enter-active {
  transition: opacity 0.22s ease-out, transform 0.26s cubic-bezier(0.22, 1, 0.36, 1);
}
.notice-leave-active {
  transition: opacity 0.24s ease-in, transform 0.24s ease-in;
}
/* move 让堆叠上下位移平滑 */
.notice-move {
  transition: transform 0.24s ease;
}
</style>
