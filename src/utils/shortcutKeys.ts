/**
 * 快捷键按键归一 + 合法性校验（录制器与设置页共用，纯函数便于单测）。
 * tests/shortcutKeys.test.mjs 直接加载本文件（Node ESM 不解析省略的扩展名，
 * 故这里**不 import 任何东西**，保持模块自足）。
 *
 * 两条实测结论决定这里的写法：
 * ① 中文输入法开着时，Chromium 在组合链路里上报的 key 可能是 "Process"（keyCode 229），
 *    物理键位只能用 `e.code` 取——旧实现只看 `e.key`，会把 Ctrl+空格录成 `Ctrl+Process`，
 *    后端解析失败、用户只看到一句英文报错。
 * ② 无修饰键的单键快捷键会全局劫持该按键：空格最惨，RegisterHotKey 成功后全系统输入框
 *    都打不出空格（连回设置里改回来都费劲），故单键只放行 F1–F24。
 */

/** 键盘事件里本模块用到的字段（KeyboardEvent 的结构子集，便于测试构造） */
export interface KeyLikeEvent {
  key: string
  code?: string
  keyCode?: number
  isComposing?: boolean
}

/** 修饰键的规范写法（与 tauri-plugin-global-shortcut 的解析词对齐） */
const MODIFIER_CANONICAL: Record<string, string> = {
  control: 'Ctrl',
  ctrl: 'Ctrl',
  commandorcontrol: 'CommandOrControl',
  cmdorctrl: 'CommandOrControl',
  cmdorcontrol: 'CommandOrControl',
  commandorctrl: 'CommandOrControl',
  command: 'CommandOrControl',
  cmd: 'CommandOrControl',
  meta: 'CommandOrControl',
  super: 'Super',
  win: 'Super',
  alt: 'Alt',
  option: 'Alt',
  shift: 'Shift',
}

/** 修饰键固定排在前面：插件解析要求「修饰键在前、最后单个主键」 */
const MODIFIER_ORDER = ['CommandOrControl', 'Ctrl', 'Super', 'Alt', 'Shift']

/** 把任意写法的修饰键归一成规范 token；不是修饰键返回 null */
export function canonicalModifier(token: string): string | null {
  return MODIFIER_CANONICAL[token.trim().toLowerCase()] ?? null
}

export function isModifierToken(token: string): boolean {
  return canonicalModifier(token) !== null
}

/** 一个 keydown 的 e.key → 修饰键 token（mac 上 Ctrl/Cmd 都记成 CommandOrControl） */
export function modifierTokenFromEvent(e: KeyLikeEvent, mac: boolean): string | null {
  switch (e.key) {
    case 'Control':
      return mac ? 'CommandOrControl' : 'Ctrl'
    case 'Meta':
      // macOS: Cmd；Windows: Win 键（插件在 Windows 上只认 Super 才是 Win）
      return mac ? 'CommandOrControl' : 'Super'
    case 'Alt':
    case 'AltGraph':
      return 'Alt'
    case 'Shift':
      return 'Shift'
    default:
      return null
  }
}

/** 主键写法归一：单个字母一律大写（插件 parse_key 大小写不敏感，但配置里统一更好读） */
function normalizeMainToken(token: string): string {
  if (token.length === 1) return token.toUpperCase()
  return token
}

export function formatShortcutTokens(tokens: Iterable<string>): string {
  const normalized = new Set<string>()
  for (const raw of tokens) {
    const token = raw.trim()
    if (!token) continue
    normalized.add(canonicalModifier(token) ?? normalizeMainToken(token))
  }
  const parts: string[] = []
  for (const mod of MODIFIER_ORDER) if (normalized.has(mod)) parts.push(mod)
  for (const token of normalized) if (!MODIFIER_ORDER.includes(token)) parts.push(token)
  return parts.join('+')
}

/**
 * `e.code`（物理键位）→ 插件认的主键写法。优先用它而不是 `e.key`：
 * IME 组合态下 `e.key` 会变成 "Process"，物理键位却始终可靠。
 */
const CODE_TO_TOKEN: Record<string, string> = {
  Space: 'Space',
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Enter: 'Enter',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Escape: 'Escape',
  Delete: 'Delete',
  Insert: 'Insert',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  CapsLock: 'CapsLock',
  NumLock: 'NumLock',
  ScrollLock: 'ScrollLock',
  Pause: 'Pause',
  PrintScreen: 'PrintScreen',
  NumpadAdd: 'NumpadAdd',
  NumpadSubtract: 'NumpadSubtract',
  NumpadMultiply: 'NumpadMultiply',
  NumpadDivide: 'NumpadDivide',
  NumpadDecimal: 'NumpadDecimal',
  NumpadEnter: 'NumpadEnter',
  NumpadEqual: 'NumpadEqual',
}

const FUNCTION_KEY_RE = /^F([1-9]|1[0-9]|2[0-4])$/

function tokenFromCode(code: string): string | null {
  const direct = CODE_TO_TOKEN[code]
  if (direct) return direct
  if (/^Key[A-Z]$/.test(code)) return code.slice(3) // KeyA..KeyZ → A..Z
  if (/^Digit[0-9]$/.test(code)) return code.slice(5) // Digit0..Digit9 → 0..9
  if (/^Numpad[0-9]$/.test(code)) return code
  if (FUNCTION_KEY_RE.test(code)) return code
  return null
}

/** `e.key` 兜底（老 WebView / 合成事件没有 code 时） */
function tokenFromKey(key: string): string | null {
  if (key === ' ' || key === 'Spacebar') return 'Space'
  if (key.length === 1) return /[\x21-\x7e]/.test(key) ? key.toUpperCase() : null
  // 具名键只放行插件 parse_key 列出的那些；Process / Unidentified / Dead 等一律不认
  return CODE_TO_TOKEN[key] ?? (FUNCTION_KEY_RE.test(key) ? key : null)
}

/**
 * 把一个 keydown 归一成插件认的主键写法；返回 null 表示这一下不该被录制
 * （输入法组合态、无法识别的键）。
 *
 * 修饰键也会原样返回（`Ctrl`/`Shift`/…）：调用方据此区分「还在攒前缀」和「主键到了」。
 * 录制器里的修饰键走 `modifierTokenFromEvent`（要按平台区分 Ctrl/Cmd），两者结果一致。
 */
export function shortcutKeyFromEvent(e: KeyLikeEvent): string | null {
  if (e.isComposing || e.key === 'Process' || e.keyCode === 229) return null
  const mod = modifierTokenFromEvent(e, false)
  if (mod) return mod
  const code = e.code ?? ''
  return (code ? tokenFromCode(code) : null) ?? tokenFromKey(e.key)
}

/** 无修饰键时只有功能键可单独作快捷键（空格/字母/数字单键会劫持全局输入） */
export function isSingleKeyAllowed(key: string): boolean {
  return FUNCTION_KEY_RE.test(key)
}

/**
 * 校验一组按键能否保存：null = 通过，否则返回给用户看的中文说明。
 * 后端 shortcut::validate_custom_shortcut 有同口径校验（手工输入、老配置同样拦得住），
 * 这里是为了「按下的瞬间就给出可读提示」，不必等 invoke 回来。
 */
export function validateShortcutTokens(tokens: Iterable<string>): string | null {
  const list = [...tokens].map((t) => t.trim()).filter(Boolean)
  if (list.length === 0) return '请按下组合键'
  // 顺序校验：插件要求修饰键全部在主键之前（"Ctrl+K+Shift" 这种写法它直接判 InvalidFormat）
  const firstMain = list.findIndex((t) => !isModifierToken(t))
  if (firstMain !== -1 && list.slice(firstMain + 1).some(isModifierToken)) {
    return '修饰键要写在前面，如 Ctrl+Shift+K'
  }
  const modifiers = list.filter(isModifierToken)
  const mainKeys = list.filter((t) => !isModifierToken(t))
  if (mainKeys.length === 0) return '请再按一个主键，如 Ctrl+Shift+K'
  if (mainKeys.length > 1) return '一个快捷键只能有一个主键'
  const main = mainKeys[0]
  if (modifiers.length > 0) return null
  if (isSingleKeyAllowed(main)) return null
  if (main === 'Space' || main === ' ') {
    return '空格不能单独作快捷键（会让所有程序都打不出空格）；请加修饰键，如 Ctrl+Shift+Space'
  }
  return `「${main}」不能单独作快捷键（会劫持所有程序的这个键）；请加修饰键，如 Ctrl+Shift+${main}`
}

/** 校验整串写法（手工输入/回填旧配置用）：把 "Ctrl+K" 拆开后走同一套规则 */
export function validateShortcutString(value: string): string | null {
  const s = value.trim()
  if (!s) return '快捷键不能为空'
  return validateShortcutTokens(s.split('+'))
}
