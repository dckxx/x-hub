import { computed, nextTick, ref } from 'vue'
import { isTauri } from '../api/tauri'
import { reportClientError } from '../utils/error-report'
import {
  formatShortcutTokens,
  modifierTokenFromEvent,
  shortcutKeyFromEvent,
  validateShortcutTokens,
} from '../utils/shortcutKeys'

/**
 * 快捷键录入（录制）共用逻辑：设置页的 5 个可自定义快捷键（主窗显隐 / 剪贴板 /
 * 全局搜索 / AI 对话 / 速记）共用，避免五套几乎相同的状态/函数重复维护。
 * 支持手动输入 + 按键录制，失焦/回车自动保存。
 *
 * 按键归一与合法性校验抽在 utils/shortcutKeys.ts（纯函数、带单测），这里只管状态机：
 * 修饰键按下只累积显示，主键按下即定稿并保存；录到不合法组合时给出可读提示并还原，
 * 绝不把坏值落盘（旧实现会把裸空格存成全局快捷键，见该文件的实测说明）。
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
    // 手工输入的旁路同样过一遍校验：单独一个空格/字母存进去就是全局劫持该按键
    const invalid = validateShortcutTokens(v.split('+').map((p) => p.trim()).filter(Boolean))
    if (invalid) {
      error.value = invalid
      value.value = saved.value
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
    error.value = ''
    pressedKeys.value = new Set()
    void nextTick(() => inputRef.value?.focus())
  }

  function cancelListening() {
    listening.value = false
    pressedKeys.value = new Set()
    value.value = previous.value
  }

  function onBlur() {
    if (listening.value) {
      // 录制中焦点离开：取消录制并恢复原值
      cancelListening()
      return
    }
    void commit()
  }

  function onKeydown(e: KeyboardEvent) {
    if (!listening.value) return
    e.preventDefault()
    e.stopPropagation()
    if (e.key === 'Escape') {
      cancelListening()
      return
    }
    // 修饰键先判：它们不该走「物理键位 → 主键」那条归一，否则 Control/Shift 会被
    // shortcutKeyFromEvent 当成无法识别的主键丢掉，Ctrl+空格 就退化成「裸空格」被拒
    const modifier = modifierTokenFromEvent(e, IS_MAC)
    if (modifier) {
      pressedKeys.value.add(modifier)
      value.value = formatShortcutTokens(pressedKeys.value)
      return
    }
    // 输入法组合态（中文选词中）的按键不是用户想录的快捷键，直接忽略：
    // 硬录下来会得到 "Ctrl+Process"，后端解析必然失败
    const token = shortcutKeyFromEvent(e)
    if (!token) return
    // 主键按下即完成录制（一个快捷键只有一个主键），避免后续按键污染组合
    const keys = new Set(pressedKeys.value)
    keys.add(token)
    const display = formatShortcutTokens(keys)
    const invalid = validateShortcutTokens(keys)
    if (invalid) {
      // 不保存、不留在录入态空等：还原成原值并说明原因（如「空格不能单独作快捷键」）
      error.value = invalid
      cancelListening()
      return
    }
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
