import { computed, nextTick, ref } from 'vue'
import { isTauri } from '../api/tauri'
import { reportClientError } from '../utils/error-report'

/**
 * 快捷键录入（录制）共用逻辑：设置页的「全局快捷键」与「剪贴板呼出快捷键」共用，
 * 避免两套几乎相同的状态/函数重复维护。支持手动输入 + 按键录制，失焦/回车自动保存。
 */

const IS_MAC =
  /Mac|iPhone|iPad/.test(navigator.userAgent) || /Mac|iPhone|iPad/.test(navigator.platform)

/** 把 CommandOrControl 按平台显示为 Ctrl（macOS 保持不变） */
export function normalizeShortcutDisplay(s: string): string {
  if (IS_MAC) return s
  return s
    .split('+')
    .map((p) => (p === 'CommandOrControl' ? 'Ctrl' : p))
    .join('+')
}

function normalizeShortcutKey(e: KeyboardEvent): string {
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

function formatShortcutDisplay(keys: Set<string>): string {
  const parts: string[] = []
  for (const mod of MODIFIER_ORDER) {
    if (keys.has(mod)) parts.push(mod)
  }
  for (const key of keys) {
    if (!MODIFIER_ORDER.includes(key)) parts.push(key)
  }
  return parts.join('+')
}

export interface ShortcutRecorderOptions {
  /** 初始显示值 */
  initial: string
  /** 用于 toast / 错误上报文案，如「全局快捷键」 */
  label: string
  /** 保存回调：返回后端归一化后的最终值 */
  save: (value: string) => Promise<string>
  showToast: (msg: string) => void
}

export function useShortcutRecorder(options: ShortcutRecorderOptions) {
  const { initial, label, save, showToast } = options
  const value = ref(initial)
  const saved = ref(initial)
  const error = ref('')
  const listening = ref(false)
  const saving = ref(false)
  const previous = ref('')
  const inputRef = ref<HTMLInputElement | null>(null)
  const pressedKeys = ref(new Set<string>())
  const normalized = computed(() => value.value.trim())

  async function commit() {
    if (!isTauri() || saving.value) return
    const v = normalized.value
    if (!v || v === saved.value) {
      error.value = ''
      return
    }
    saving.value = true
    error.value = ''
    try {
      const savedValue = await save(v)
      saved.value = normalizeShortcutDisplay(savedValue)
      value.value = saved.value
      showToast(`${label}已更新为 ${saved.value}`)
    } catch (e) {
      error.value = String(e)
      void reportClientError(`设置${label}失败`, e)
    } finally {
      saving.value = false
    }
  }

  function startListening() {
    previous.value = value.value
    listening.value = true
    value.value = ''
    pressedKeys.value = new Set()
    void nextTick(() => inputRef.value?.focus())
  }

  function onBlur() {
    if (listening.value) {
      // 录制中焦点离开：取消录制并恢复原值
      listening.value = false
      pressedKeys.value = new Set()
      value.value = previous.value
      return
    }
    void commit()
  }

  function onKeydown(e: KeyboardEvent) {
    if (!listening.value) return
    e.preventDefault()
    e.stopPropagation()
    if (e.key === 'Escape') {
      listening.value = false
      pressedKeys.value = new Set()
      value.value = previous.value
      return
    }
    if (['Control', 'Meta', 'Alt', 'Shift'].includes(e.key)) {
      pressedKeys.value.add(normalizeShortcutKey(e))
      value.value = formatShortcutDisplay(pressedKeys.value)
      return
    }
    // 主键按下即完成录制（一个快捷键只有一个主键），避免后续按键污染组合，随后自动保存
    pressedKeys.value.add(normalizeShortcutKey(e))
    const display = formatShortcutDisplay(pressedKeys.value)
    if (!display) return
    value.value = display
    listening.value = false
    pressedKeys.value = new Set()
    void commit()
  }

  return {
    value,
    saved,
    error,
    listening,
    saving,
    inputRef,
    commit,
    startListening,
    onBlur,
    onKeydown,
  }
}
