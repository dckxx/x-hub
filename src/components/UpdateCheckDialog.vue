<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Download, RotateCcw, X } from 'lucide-vue-next'
import { isTauri, type MarketDownloadProgress, type UpdateInfo, tauriApi } from '../api/tauri'

const visible = ref(false)
const info = ref<UpdateInfo | null>(null)
const phase = ref<'available' | 'downloading' | 'ready'>('available')
const progress = ref<{ received: number; total: number | null } | null>(null)
const busy = ref(false)
const error = ref('')

let unlisteners: UnlistenFn[] = []

function fmtProgress(received: number, total: number | null): string {
  if (!total) return `${Math.round(received / 1048576)} MB`
  const pct = Math.min(100, Math.round((received / total) * 100))
  return `${pct}%（${(received / 1048576).toFixed(1)} / ${(total / 1048576).toFixed(1)} MB）`
}

function fmtMB(bytes: number): string {
  if (!bytes) return ''
  return `${(bytes / 1048576).toFixed(1)} MB`
}

function showAvailable(payload: UpdateInfo) {
  info.value = payload
  progress.value = null
  error.value = ''
  if (payload.ready) {
    phase.value = 'ready'
  } else {
    phase.value = 'available'
  }
  visible.value = true
}

function close() {
  visible.value = false
}

async function onUpdateNow() {
  if (!info.value || busy.value || phase.value !== 'available') return
  busy.value = true
  error.value = ''
  phase.value = 'downloading'
  progress.value = { received: 0, total: info.value.size || null }
  try {
    const result = await tauriApi.downloadUpdate(info.value.version)
    info.value = result
    if (result.ready) {
      progress.value = { received: result.size, total: result.size }
      phase.value = 'ready'
    }
  } catch (e) {
    error.value = String(e)
    phase.value = 'available'
  } finally {
    busy.value = false
  }
}

async function onSkipVersion() {
  if (!info.value) return
  try {
    await tauriApi.skipUpdateVersion(info.value.version)
  } catch {
    // 记录失败不阻塞关闭
  }
  close()
}

function onRestart() {
  if (!isTauri()) return
  void tauriApi.restartApp()
}

onMounted(async () => {
  if (!isTauri()) return
  unlisteners.push(
    await listen<UpdateInfo>('update-available', (e) => showAvailable(e.payload)),
    await listen<MarketDownloadProgress>('update-download-progress', (e) => {
      progress.value = { received: e.payload.received, total: e.payload.total }
    }),
    await listen<UpdateInfo>('update-ready', (e) => {
      info.value = e.payload
      progress.value = { received: e.payload.size, total: e.payload.size }
      phase.value = 'ready'
    }),
  )
  // 启动时若已有待应用更新，直接弹「立即重启」
  try {
    const st = await tauriApi.getUpdateStatus()
    if (st.ready) showAvailable(st)
  } catch {
    // 预览/无命令时静默
  }
})

onBeforeUnmount(() => unlisteners.forEach((u) => u()))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask ud-mask" role="presentation" @click.self="close">
        <div class="modal-card ud-card" role="dialog" aria-modal="true" aria-label="发现新版本">
          <header class="ud-header">
            <h3 class="ud-title">发现新版本</h3>
            <button class="icon-btn" title="关闭" aria-label="关闭" @click="close">
              <X :size="15" :stroke-width="2" />
            </button>
          </header>

          <template v-if="phase === 'available'">
            <div class="ud-version">
              <span class="ud-ver-badge">v{{ info?.version }}</span>
              <span v-if="info?.portable" class="ud-portable">便携版</span>
              <span v-if="info?.size" class="ud-size">{{ fmtMB(info.size) }}</span>
            </div>
            <div class="ud-notes">{{ info?.notes || '暂无更新说明' }}</div>
            <div v-if="error" class="ud-error">{{ error }}</div>
            <footer class="ud-footer">
              <button class="ghost-btn" type="button" @click="onSkipVersion">跳过此版本</button>
              <button class="ghost-btn" type="button" @click="close">取消</button>
              <button class="pill-btn" type="button" :disabled="busy" @click="onUpdateNow">
                <Download :size="14" :stroke-width="2" />
                立即更新
              </button>
            </footer>
          </template>

          <template v-else-if="phase === 'downloading'">
            <div class="ud-version">
              <span class="ud-ver-badge">v{{ info?.version }}</span>
              <span class="ud-downloading-label">正在下载…</span>
            </div>
            <div class="ud-bar">
              <div
                class="ud-bar-fill"
                :style="{
                  width: progress && progress.total
                    ? `${Math.min(100, (progress.received / progress.total) * 100)}%`
                    : '8%',
                }"
              ></div>
            </div>
            <div v-if="progress" class="ud-progress">
              {{ fmtProgress(progress.received, progress.total) }}
            </div>
            <div v-if="error" class="ud-error">{{ error }}</div>
          </template>

          <template v-else>
            <div class="ud-version">
              <span class="ud-ver-badge">v{{ info?.version }}</span>
              <span class="ud-ready-label">更新已就绪</span>
            </div>
            <div class="ud-notes">点击「立即重启」完成升级（当前 v{{ info?.current }}）</div>
            <footer class="ud-footer">
              <button class="ghost-btn" type="button" @click="close">稍后</button>
              <button class="pill-btn" type="button" @click="onRestart">
                <RotateCcw :size="14" :stroke-width="2" />
                立即重启
              </button>
            </footer>
          </template>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ud-mask {
  z-index: 200;
}
.ud-card {
  width: 460px;
  max-width: calc(100vw - 48px);
  display: flex;
  flex-direction: column;
  padding: 20px 24px;
}
.ud-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.ud-title {
  margin: 0;
  font-size: 1rem;
  font-weight: 700;
  color: var(--text-1);
}
.ud-version {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.ud-ver-badge {
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--brand-500);
  font-variant-numeric: tabular-nums;
}
.ud-portable {
  font-size: 0.6875rem;
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  background: var(--brand-50);
  color: var(--brand-500);
  font-weight: 600;
}
.ud-size {
  font-size: 0.75rem;
  color: var(--text-3);
}
.ud-notes {
  font-size: 0.8125rem;
  line-height: 1.7;
  color: var(--text-2);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 180px;
  overflow-y: auto;
  padding: 10px 12px;
  background: var(--frost-surface);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  margin-bottom: 12px;
}
.ud-error {
  font-size: 0.75rem;
  color: var(--c-red);
  margin-bottom: 10px;
  word-break: break-word;
}
.ud-footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}
.ud-downloading-label,
.ud-ready-label {
  font-size: 0.75rem;
  color: var(--text-3);
}
.ud-ready-label {
  color: var(--ok-green);
}
.ud-bar {
  height: 6px;
  border-radius: var(--radius-pill);
  background: var(--border-soft);
  overflow: hidden;
  margin-bottom: 10px;
}
.ud-bar-fill {
  height: 100%;
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  transition: width 0.2s ease-out;
}
.ud-progress {
  font-size: 0.75rem;
  color: var(--text-3);
  text-align: right;
  margin-bottom: 12px;
  font-variant-numeric: tabular-nums;
}
</style>