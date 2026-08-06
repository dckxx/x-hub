<script setup lang="ts">
import { inject, onBeforeUnmount, onMounted, ref, toRef } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { Download, Lock, Upload, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { useFocusTrap } from '../composables/useFocusTrap'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

const showToast = inject<(msg: string) => void>('showToast', () => {})

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.visible) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

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
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div
          ref="cardRef"
          class="modal-card"
          role="dialog"
          aria-label="设置"
          aria-modal="true"
        >
          <div class="settings-head">
            <h2 class="dialog-title">设置</h2>
            <button class="icon-btn" title="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" />
            </button>
          </div>

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
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.settings-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.dialog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px 0;
  border-bottom: 1px solid var(--border-soft);
}
.setting-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
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

.data-btn {
  padding: 7px 14px;
}
.data-btn.confirm {
  background: var(--c-red);
  color: #fff;
}
.data-btn.confirm:hover {
  background: color-mix(in srgb, var(--c-red) 85%, #000);
  color: #fff;
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

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
