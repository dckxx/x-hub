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
// 本次录制的时长上限（秒）：到达自动触发停止（旧实现只停采集不收尾，计时一直走）
const maxMs = ref(0)
/** 已保存态 5 秒无操作自动隐藏（鼠标悬停在控制条上时暂停倒计时） */
let hideTimer: ReturnType<typeof setTimeout> | null = null
const hovering = ref(false)
let timer: ReturnType<typeof setInterval> | null = null
let startedAt = Date.now()
let autoStopped = false
let unlistenProgress: (() => void) | null = null
let unlistenStart: (() => void) | null = null
let unlistenDone: (() => void) | null = null

function fmt(ms: number): string {
  const s = Math.floor(ms / 1000)
  const m = Math.floor(s / 60)
  const sec = s % 60
  return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`
}

async function stop() {
  if (done.value || stopping.value) return
  stopping.value = true
  // 立即冻结计时，避免编码期间"时间还在跑"
  stopTimer()
  try {
    done.value = await tauriApi.recorderStop()
    scheduleAutoHide()
  } catch (e) {
    error.value = String(e)
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

async function cancel() {
  if (stopping.value) return
  stopTimer()
  clearAutoHide()
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
      elapsed.value = 0
      maxMs.value = Math.max(0, (p.max_seconds || 0) * 1000)
      autoStopped = false
      clearAutoHide()
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
    // 线程内到限自停（recorder.rs finish_from_capture_thread）也会发 recorder-done：
    // 控制条自己没调 stop，靠这条事件收尾置已保存态（手动 stop 路径收到时值相同，幂等）
    unlistenDone = await listen('recorder-done', (e) => {
      const r = e.payload as RecorderResult
      if (!r || done.value) return
      stopTimer()
      autoStopped = true
      done.value = r
      scheduleAutoHide()
    })
  } catch {
    // 事件监听失败不阻塞手动停止
  }
})

onBeforeUnmount(() => {
  stopTimer()
  clearAutoHide()
  unlistenProgress?.()
  unlistenStart?.()
  unlistenDone?.()
})
</script>

<template>
  <div
    class="rc-root"
    @mouseenter="hovering = true"
    @mouseleave="hovering = false"
  >
    <!-- 进行中：计时 + 停止 -->
    <template v-if="!done">
      <span class="rc-dot"></span>
      <span class="rc-time">{{ fmt(elapsed) }}</span>
      <button class="rc-stop" type="button" :disabled="stopping" @click="stop">
        <Square v-if="!stopping" :size="12" :fill="'currentColor'" :stroke-width="2" />
        {{ stopping ? '处理中…' : '停止' }}
      </button>
      <button class="rc-x" type="button" :disabled="stopping" title="取消并丢弃" @click="cancel">
        <X :size="14" :stroke-width="2" />
      </button>
      <p v-if="error" class="rc-error">{{ error }}</p>
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
