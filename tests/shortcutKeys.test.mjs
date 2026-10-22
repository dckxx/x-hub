/**
 * 快捷键按键归一与校验（src/utils/shortcutKeys.ts）单测。
 * 直接以 node:test 加载 TS 源码（模块零依赖，Node 24 原生类型剥离）：
 *   node --test tests/shortcutKeys.test.mjs
 *
 * 回归背景：中文输入法开着时按 Ctrl+空格，Chromium 在组合链路里上报 key="Process"、
 * keyCode=229；旧实现只看 e.key，把组合录成 "Ctrl+Process"，保存时后端解析失败，
 * 用户只看到一句英文报错——「两个快捷键里有一个按空格没用」就是这么来的。
 */
import test from 'node:test'
import assert from 'node:assert/strict'
import {
  formatShortcutTokens,
  isModifierToken,
  shortcutKeyFromEvent,
  validateShortcutString,
  validateShortcutTokens,
} from '../src/utils/shortcutKeys.ts'

// ---- 主键归一：物理键位优先，IME 组合态不录制 ----

test('空格通过 code=Space 归一为 Space（不依赖 e.key）', () => {
  assert.equal(shortcutKeyFromEvent({ key: ' ', code: 'Space', keyCode: 32 }), 'Space')
})

test('IME 组合态（key=Process / keyCode 229）返回 null，绝不录成 Process', () => {
  assert.equal(shortcutKeyFromEvent({ key: 'Process', code: 'Space', keyCode: 229 }), null)
  assert.equal(shortcutKeyFromEvent({ key: 'Process', keyCode: 229, isComposing: true }), null)
  // 组合中即使 keyCode 正常也不录（选词时的按键不是快捷键意图）
  assert.equal(shortcutKeyFromEvent({ key: 'k', code: 'KeyK', keyCode: 75, isComposing: true }), null)
})

test('没有 code 的老环境回退 e.key，空格/字母仍正确', () => {
  assert.equal(shortcutKeyFromEvent({ key: ' ' }), 'Space')
  assert.equal(shortcutKeyFromEvent({ key: 'k', keyCode: 75 }), 'K')
  assert.equal(shortcutKeyFromEvent({ key: 'F5', keyCode: 116 }), 'F5')
})

test('物理键位映射覆盖字母/数字/符号/功能键', () => {
  const cases = [
    [{ key: 'k', code: 'KeyK' }, 'K'],
    [{ key: '5', code: 'Digit5' }, '5'],
    [{ key: '`', code: 'Backquote' }, '`'],
    [{ key: '\\', code: 'Backslash' }, '\\'],
    [{ key: ' ', code: 'Numpad0' }, 'Numpad0'],
    [{ key: 'F12', code: 'F12' }, 'F12'],
    [{ key: 'ArrowUp', code: 'ArrowUp' }, 'Up'],
    [{ key: 'Enter', code: 'Enter' }, 'Enter'],
  ]
  for (const [ev, want] of cases) {
    assert.equal(shortcutKeyFromEvent(ev), want, JSON.stringify(ev))
  }
})

test('无法识别的键返回 null（不把怪异值存进配置）', () => {
  assert.equal(shortcutKeyFromEvent({ key: 'Unidentified', code: '' }), null)
  assert.equal(shortcutKeyFromEvent({ key: 'Dead', code: 'IntlRo' }), null)
})

test('修饰键原样返回（录制器靠它区分「攒前缀」与「主键到了」）', () => {
  // 回归：修饰键若被判成 null，Ctrl+空格 里的 Ctrl 会丢，整个组合退化成「裸空格」被拒
  assert.equal(shortcutKeyFromEvent({ key: 'Control', code: 'ControlLeft' }), 'Ctrl')
  assert.equal(shortcutKeyFromEvent({ key: 'Shift', code: 'ShiftLeft' }), 'Shift')
  assert.equal(shortcutKeyFromEvent({ key: 'Alt', code: 'AltLeft' }), 'Alt')
  assert.equal(shortcutKeyFromEvent({ key: 'Meta', code: 'MetaLeft' }), 'Super')
})

// ---- 组合串拼装：修饰键在前，写法归一 ----

test('修饰键固定在前，且写法归一（大小写/别名）', () => {
  assert.equal(formatShortcutTokens(['Space', 'Ctrl', 'Shift']), 'Ctrl+Shift+Space')
  assert.equal(formatShortcutTokens(['k', 'control']), 'Ctrl+K')
  assert.equal(formatShortcutTokens(['CommandOrControl', 'K']), 'CommandOrControl+K')
})

test('isModifierToken 认各种别名', () => {
  for (const m of ['Ctrl', 'Control', 'CommandOrControl', 'Super', 'Alt', 'Shift', 'cmd']) {
    assert.equal(isModifierToken(m), true, m)
  }
  assert.equal(isModifierToken('Space'), false)
})

// ---- 合法性校验：单键劫持 ----

test('空格单独作快捷键被拒绝，并给出可照抄的组合', () => {
  const msg = validateShortcutTokens(['Space'])
  assert.ok(msg, '应当返回错误文案')
  assert.match(msg, /空格/)
  assert.match(msg, /Ctrl\+Shift\+Space/)
})

test('字母/数字单独作快捷键被拒绝', () => {
  assert.match(validateShortcutTokens(['K']), /K/)
  assert.match(validateShortcutTokens(['5']), /5/)
})

test('功能键可单独使用（F1–F24）', () => {
  for (const k of ['F1', 'F5', 'F12', 'F24']) {
    assert.equal(validateShortcutTokens([k]), null, k)
  }
  assert.ok(validateShortcutTokens(['F25']), 'F25 超出插件支持范围')
})

test('带修饰键的空格组合合法（这正是用户要配的那种）', () => {
  assert.equal(validateShortcutTokens(['Ctrl', 'Space']), null)
  assert.equal(validateShortcutTokens(['Ctrl', 'Shift', 'Space']), null)
  assert.equal(validateShortcutTokens(['Alt', 'Space']), null)
})

test('只有修饰键 / 多个主键 / 空 都不合法', () => {
  assert.match(validateShortcutTokens(['Ctrl', 'Shift']), /主键/)
  assert.match(validateShortcutTokens(['Ctrl', 'K', 'B']), /一个主键/)
  assert.match(validateShortcutTokens([]), /按下组合键/)
})

test('validateShortcutString 走同一套规则（手工输入旁路）', () => {
  assert.equal(validateShortcutString('Ctrl+Shift+Space'), null)
  assert.match(validateShortcutString('Space'), /空格/)
  assert.match(validateShortcutString(''), /不能为空/)
  // 修饰键写在主键之后：插件判 InvalidFormat，这里给更具体的提示
  assert.match(validateShortcutString('Ctrl+K+Shift'), /修饰键要写在前面/)
})
