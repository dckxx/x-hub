<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'
import { FolderOpen, Square, X } from 'lucide-vue-next'
import { isTauri, tauriApi, type RecorderResult } from '../api/tauri'

// 录制控制条窗口：悬浮计时 + 停止。
// 窗口启动期预创建隐藏常驻、跨会话复用（recorder.rs::init），每次开录由
// recorder-control-start 事件重置状态；运行期只显隐，绝不 destroy。
document.documentElement.dataset.recorderControl = ''

const appWindow = isTauri() ? getCurrentWindow() : null

const elapsed = ref(0)
const done = ref<RecorderResult | null>(null)
const error = ref('')
const stopping = ref(false)
// 编码阶段（stop 后到 GIF 落盘完成）：大录制编码可达几十秒，必须让状态可见地流动，
// 不能把「停止」按钮继续挂在窗口上（用户会以为卡死）
const encoding = ref(false)
const encodeDone = ref(0)
const encodeTotal = ref(0)
// 本次录制的时长上限（秒）：到达自动触发停止（旧实现只停采集不收尾，计时一直走）
const maxMs = ref(0)
/** 已保存态 5 秒无操作自动隐藏（鼠标悬停在控制条上时暂停倒计时） */
let hideTimer: ReturnType<typeof setTimeout> | null = null
/** 失败态停留几秒后自动收窗 */
let errorTimer: ReturnType<typeof setTimeout> | null = null
const hovering = ref(false)
let timer: ReturnType<typeof setInterval> | null = null
let startedAt = Date.now()
let autoStopped = false
let unlistenProgress: (() => void) | null = null
let unlistenStart: (() => void) | null = null
let unlistenDone: (() => void) | null = null
let unlistenEncoding: (() => void) | null = null
let unlistenEncodeProgress: (() => void) | null = null
let unlistenError: (() => void) | null = null

function fmt(ms: number): string {
  const s = Math.floor(ms / 1000)
  const m = Math.floor(s / 60)
  const sec = s % 60
  return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`
}

async function stop() {
  if (done.value || stopping.value) return
  stopping.value = true
  // 立即切「编码中」并冻结计时：停止按钮就此消失，编码再久也不会看着像卡在录制态
  encoding.value = true
  encodeDone.value = 0
  encodeTotal.value = 0
  error.value = ''
  clearErrorHide()
  stopTimer()
  try {
    const r = await tauriApi.recorderStop()
    if (r) {
      done.value = r
      encoding.value = false
      scheduleAutoHide()
    }
  } catch (e) {
    const msg = String(e)
    // 到限自停已由采集线程接管收尾：本命令拿不到会话属正常，等 encoding/done 事件
    if (msg.includes('没有正在进行')) {
      encoding.value = true
    } else if (!done.value) {
      encoding.value = false
      error.value = msg
      scheduleErrorHide()
    }
  } finally {
    stopping.value = false
  }
}

/** 已保存态 5 秒无操作自动隐藏：悬停暂停、移开重新计时；新会话/出错时取消 */
function scheduleAutoHide() {
  clearAutoHide()
  if (!done.value) return
  hideTimer = setTimeout(() => {
    if (hovering.value) {
      scheduleAutoHide()
      return
    }
    void hide()
  }, 5000)
}

function clearAutoHide() {
  if (hideTimer) {
    clearTimeout(hideTimer)
    hideTimer = null
  }
}

/** 失败态 5 秒后自动收窗（悬停暂停，同已保存态） */
function scheduleErrorHide() {
  clearErrorHide()
  errorTimer = setTimeout(() => {
    if (hovering.value) {
      scheduleErrorHide()
      return
    }
    void hide()
  }, 5000)
}

function clearErrorHide() {
  if (errorTimer) {
    clearTimeout(errorTimer)
    errorTimer = null
  }
}

async function cancel() {
  if (stopping.value) return
  stopTimer()
  clearAutoHide()
  clearErrorHide()
  try {
    await tauriApi.recorderCancel()
  } catch {
    // 忽略
  }
  // Rust 侧 cancel 已收控制条，这里兜底（无会话时 cancel 提前返回不收窗）
  await hide()
}

async function reveal() {
  if (done.value) await tauriApi.recorderReveal(done.value.path)
}

/** 已保存态点 X：只隐藏（窗口复用），下次开录由 recorder-control-start 重置 */
async function hide() {
  if (isTauri()) await appWindow?.hide()
}

function stopTimer() {
  if (timer) {
    clearInterval(timer)
    timer = null
  }
}

function startTimer() {
  stopTimer()
  startedAt = Date.now()
  timer = setInterval(() => {
    elapsed.value = Date.now() - startedAt
    if (maxMs.value > 0 && !autoStopped && elapsed.value >= maxMs.value) {
      autoStopped = true
      void stop()
    }
  }, 250)
}

onMounted(async () => {
  if (!isTauri()) return
  try {
    // 新会话重置（每次 recorder_start 下发；窗口复用所以不能依赖挂载清态）
    unlistenStart = await listen('recorder-control-start', (e) => {
      const p = e.payload as { max_seconds: number }
      done.value = null
      error.value = ''
      stopping.value = false
      encoding.value = false
      encodeDone.value = 0
      encodeTotal.value = 0
      elapsed.value = 0
      maxMs.value = Math.max(0, (p.max_seconds || 0) * 1000)
      autoStopped = false
      clearAutoHide()
      clearErrorHide()
      startTimer()
    })
    unlistenProgress = await listen('recorder-progress', (e) => {
      const p = e.payload as { elapsed_ms: number; frames: number; fps: number; max_seconds?: number }
      elapsed.value = p.elapsed_ms
      if (!maxMs.value && p.max_seconds) maxMs.value = p.max_seconds * 1000
      if (maxMs.value > 0 && !autoStopped && p.elapsed_ms >= maxMs.value) {
        autoStopped = true
        void stop()
      }
    })
    // 编码开始（stop 命令或采集线程到限自停都会发）：切「编码中」并清掉可能的竞态报错
    unlistenEncoding = await listen('recorder-encoding', (e) => {
      const p = e.payload as { frames?: number }
      if (done.value) return
      encoding.value = true
      stopping.value = true
      encodeDone.value = 0
      encodeTotal.value = p.frames || 0
      error.value = ''
      clearErrorHide()
      stopTimer()
    })
    unlistenEncodeProgress = await listen('recorder-encoding-progress', (e) => {
      const p = e.payload as { done: number; total: number }
      encodeDone.value = p.done || 0
      encodeTotal.value = p.total || 0
    })
    // 线程内到限自停（recorder.rs finish_from_capture_thread）也会发 recorder-done：
    // 控制条自己没调 stop 也能靠事件收尾置已保存态（手动 stop 路径收到时值相同，幂等）
    unlistenDone = await listen('recorder-done', (e) => {
      const r = e.payload as RecorderResult
      if (!r || done.value) return
      stopTimer()
      autoStopped = true
      encoding.value = false
      stopping.value = false
      error.value = ''
      clearErrorHide()
      done.value = r
      scheduleAutoHide()
    })
    unlistenError = await listen('recorder-error', (e) => {
      if (done.value) return
      const p = e.payload as string | { message?: string }
      const msg = typeof p === 'string' ? p : p?.message || 'GIF 保存失败'
      stopTimer()
      encoding.value = false
      stopping.value = false
      error.value = msg
      scheduleErrorHide()
    })
  } catch {
    // 事件监听失败不阻塞手动停止
  }
})

onBeforeUnmount(() => {
  stopTimer()
  clearAutoHide()
  clearErrorHide()
  unlistenProgress?.()
  unlistenStart?.()
  unlistenDone?.()
  unlistenEncoding?.()
  unlistenEncodeProgress?.()
  unlistenError?.()
})
</script>

<template>
  <div
    class="rc-root"
    @mouseenter="hovering = true"
    @mouseleave="hovering = false"
  >
    <!-- 编码中：停止按钮立即消失，进度可见（大文件编码可能几十秒） -->
    <template v-if="!done && encoding">
      <span class="rc-dot rc-dot-encoding"></span>
      <span class="rc-time">{{ fmt(elapsed) }}</span>
      <span class="rc-encoding">
        编码中…
        <template v-if="encodeTotal > 0">{{ Math.min(encodeDone, encodeTotal) }}/{{ encodeTotal }}</template>
      </span>
    </template>

    <!-- 保存失败：短暂展示原因后自动收窗 -->
    <template v-else-if="!done && error">
      <span class="rc-time rc-fail">保存失败</span>
      <button class="rc-x" type="button" title="关闭" @click="hide">
        <X :size="14" :stroke-width="2" />
      </button>
      <p class="rc-error">{{ error }}</p>
    </template>

    <!-- 进行中：计时 + 停止 -->
    <template v-else-if="!done">
      <span class="rc-dot"></span>
      <span class="rc-time">{{ fmt(elapsed) }}</span>
      <button class="rc-stop" type="button" :disabled="stopping" @click="stop">
        <Square :size="12" :fill="'currentColor'" :stroke-width="2" />
        停止
      </button>
      <button class="rc-x" type="button" :disabled="stopping" title="取消并丢弃" @click="cancel">
        <X :size="14" :stroke-width="2" />
      </button>
    </template>

    <!-- 完成：结果展示（5 秒无操作自动隐藏） -->
    <template v-else>
      <span class="rc-done">
        <FolderOpen :size="14" :stroke-width="2" />
        GIF 已保存
      </span>
      <button class="rc-stop" type="button" @click="reveal">打开</button>
      <button class="rc-x" type="button" title="关闭" @click="hide">
        <X :size="14" :stroke-width="2" />
      </button>
      <p class="rc-meta">
        {{ done.frames }} 帧 · {{ (done.size_bytes / 1024).toFixed(1) }} KB · {{ done.width }}×{{ done.height }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.rc-root {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border-radius: 10px;
  background: rgba(16, 18, 30, 0.92);
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  color: #e8eaf2;
  user-select: none;
  border: 1px solid rgba(255, 255, 255, 0.12);
}
.rc-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #ff5a5a;
  box-shadow: 0 0 0 0 rgba(255, 90, 90, 0.6);
  animation: rc-pulse 1.2s infinite;
}
@keyframes rc-pulse {
  0% { box-shadow: 0 0 0 0 rgba(255, 90, 90, 0.5); }
  70% { box-shadow: 0 0 0 6px rgba(255, 90, 90, 0); }
  100% { box-shadow: 0 0 0 0 rgba(255, 90, 90, 0); }
}
.rc-time {
  font-size: 0.875rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
}
.rc-time.rc-fail {
  color: #ff8b8b;
  white-space: nowrap;
}
.rc-dot-encoding {
  background: #f5b942;
  animation: rc-pulse-encoding 1.2s infinite;
}
@keyframes rc-pulse-encoding {
  0% { box-shadow: 0 0 0 0 rgba(245, 185, 66, 0.5); }
  70% { box-shadow: 0 0 0 6px rgba(245, 185, 66, 0); }
  100% { box-shadow: 0 0 0 0 rgba(245, 185, 66, 0); }
}
.rc-encoding {
  font-size: 0.75rem;
  font-weight: 600;
  color: #f5b942;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.rc-stop {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 12px;
  border-radius: 7px;
  border: none;
  background: #ff5a5a;
  color: #fff;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.rc-stop:hover {
  filter: brightness(1.1);
}
.rc-stop:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.rc-x:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.rc-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 6px;
  background: rgba(232, 234, 242, 0.12);
  color: #e8eaf2;
  cursor: pointer;
}
.rc-x:hover {
  background: rgba(232, 234, 242, 0.22);
}
.rc-done {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: #7ce38b;
  white-space: nowrap;
}
.rc-meta {
  position: absolute;
  top: 100%;
  left: 10px;
  margin: 4px 0 0;
  font-size: 0.6875rem;
  color: rgba(232, 234, 242, 0.6);
  white-space: nowrap;
}
.rc-error {
  position: absolute;
  top: 100%;
  left: 10px;
  margin: 4px 0 0;
  font-size: 0.6875rem;
  color: #ff8b8b;
  white-space: nowrap;
}
</style>
