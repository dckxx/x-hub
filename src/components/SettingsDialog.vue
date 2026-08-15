<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref, toRef } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { Download, Keyboard, Lock, Upload, X } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import AppSelect from './AppSelect.vue'
import { useFocusTrap } from '../composables/useFocusTrap'
import { useStore } from '../stores/workbench'
import { reportClientError } from '../utils/error-report'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

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

function onKeydown(e: KeyboardEvent) {
  // 录制快捷键时按 Escape 是"取消录制"，不应关闭弹窗
  if (e.key === 'Escape' && props.visible && !shortcutListening.value) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

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

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">全局快捷键</span>
              <span class="setting-desc">支持手动输入或按键录入，无冲突自动保存</span>
            </div>            <div class="shortcut-edit">
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
/* 设置项较多，覆盖全局 modal-card 的 400px，给行内容更舒展的空间 */
.modal-card {
  width: 520px;
  padding: 28px;
}

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
  color: #fff;
}
.data-btn.confirm:hover {
  background: color-mix(in srgb, var(--c-red) 85%, #000);
  color: #fff;
}

.shortcut-row {
  align-items: flex-start;
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

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
