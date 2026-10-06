<script setup lang="ts">
// 常规大类（常规 / 悬浮球 / 快捷键）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { Keyboard } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { AutostartStatus } from '../../api/tauri';
import { useStore } from '../../stores/workbench';
import { normalizeShortcutDisplay, useShortcutRecorder } from '../../composables/useShortcutRecorder';
import { FLOATING_BALL_BUTTONS, FLOATING_BALL_MAX_BUTTONS } from '../../composables/floatingBallButtons';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 桌面悬浮球（ADR 0004）：启用 / 贴边自动隐藏 / 与主窗同显 / 静止转动 / 环形按钮增删排序 ----
const ballButtons = computed(() => store.state.config.floating_ball_buttons ?? [])

function onToggleFloatingBall() {
  void store.setFloatingBallEnabled(!store.state.config.floating_ball_enabled)
}

function onToggleFloatingBallAutoHide() {
  void store.setFloatingBallAutoHide(!store.state.config.floating_ball_auto_hide)
}

function onToggleFloatingBallWithMain() {
  void store.setFloatingBallWithMain(!store.state.config.floating_ball_with_main)
}

function onToggleFloatingBallIdleSpin() {
  void store.setFloatingBallIdleSpin(!store.state.config.floating_ball_idle_spin)
}

async function addBallButton(id: string) {
  if (ballButtons.value.includes(id) || ballButtons.value.length >= FLOATING_BALL_MAX_BUTTONS) return
  await store.setFloatingBallButtons([...ballButtons.value, id])
}

async function removeBallButton(id: string) {
  await store.setFloatingBallButtons(ballButtons.value.filter((b) => b !== id))
}

async function moveBallButton(index: number, delta: number) {
  const target = index + delta
  if (target < 0 || target >= ballButtons.value.length) return
  const arr = [...ballButtons.value]
  ;[arr[index], arr[target]] = [arr[target], arr[index]]
  await store.setFloatingBallButtons(arr)
}

// ---- 快捷键录入：4 个可自定义快捷键（主窗/剪贴板/搜索/AI 对话）共用一套录制逻辑
//（见 composables/useShortcutRecorder.ts） ----
const {
  value: shortcut,
  error: shortcutError,
  listening: shortcutListening,
  inputRef: shortcutInputRef,
  commit: commitShortcut,
  startListening: startListeningShortcut,
  onBlur: onShortcutBlur,
  onKeydown: onShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.global_shortcut),
  label: '全局快捷键',
  save: (v) => store.setGlobalShortcut(v),
  showToast,
})

const {
  value: clipShortcut,
  saved: clipSavedShortcut,
  error: clipError,
  listening: clipListening,
  inputRef: clipInputRef,
  commit: commitClipShortcut,
  startListening: startListenClipShortcut,
  onBlur: onClipShortcutBlur,
  onKeydown: onClipShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.clipboard_shortcut ?? 'Ctrl+`'),
  label: '剪贴板快捷键',
  save: (v) => store.setClipboardShortcut(v),
  showToast,
})

const {
  value: searchShortcut,
  error: searchError,
  listening: searchListening,
  inputRef: searchInputRef,
  commit: commitSearchShortcut,
  startListening: startListenSearchShortcut,
  onBlur: onSearchShortcutBlur,
  onKeydown: onSearchShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.search_shortcut ?? 'Ctrl+K'),
  label: '搜索快捷键',
  save: (v) => store.setSearchShortcut(v),
  showToast,
})

const {
  value: chatShortcut,
  error: chatError,
  listening: chatListening,
  inputRef: chatInputRef,
  commit: commitChatShortcut,
  startListening: startListenChatShortcut,
  onBlur: onChatShortcutBlur,
  onKeydown: onChatShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.chat_shortcut ?? 'Ctrl+Shift+K'),
  label: 'AI 对话快捷键',
  save: (v) => store.setChatShortcut(v),
  showToast,
})

const {
  value: notesShortcut,
  error: notesError,
  listening: notesListening,
  inputRef: notesInputRef,
  commit: commitNotesShortcut,
  startListening: startListenNotesShortcut,
  onBlur: onNotesShortcutBlur,
  onKeydown: onNotesShortcutKeydown,
} = useShortcutRecorder({
  initial: normalizeShortcutDisplay(store.state.config.notes_shortcut ?? 'Ctrl+Shift+N'),
  label: '速记快捷键',
  save: (v) => store.setNotesShortcut(v),
  showToast,
})

// inputRef 仅在模板 ref 绑定中使用（把 DOM 输入框连到 recorder 内部，点击「录入」自动聚焦），
// vue-tsc 不把模板 ref 视为「读取」，这里显式求值一次以通过 noUnusedLocals
void shortcutInputRef
void clipInputRef
void searchInputRef
void chatInputRef
void notesInputRef

// ---- 快捷键启用/禁用：关掉 = 注销热键但保留键值（重开即恢复），比「清空」更明确、不会误失效 ----
type ShortcutKind = 'main' | 'clipboard' | 'search' | 'chat' | 'notes'

function shortcutEnabled(kind: ShortcutKind): boolean {
  const c = store.state.config
  switch (kind) {
    case 'main':
      return c.global_shortcut_enabled
    case 'clipboard':
      return c.clipboard_shortcut_enabled
    case 'search':
      return c.search_shortcut_enabled
    case 'chat':
      return c.chat_shortcut_enabled
    case 'notes':
      return c.notes_shortcut_enabled
  }
}

const shortcutToggleBusy = ref(false)

async function toggleShortcutEnabled(kind: ShortcutKind) {
  if (shortcutToggleBusy.value) return
  const next = !shortcutEnabled(kind)
  shortcutToggleBusy.value = true
  try {
    await store.setShortcutEnabled(kind, next)
    const label = { main: '全局', clipboard: '剪贴板', search: '搜索', chat: 'AI 对话', notes: '速记' }[kind]
    showToast(next ? `${label}快捷键已启用` : `${label}快捷键已禁用（键值保留，随时可重开）`)
  } catch (e) {
    showToast(`设置失败：${String(e)}`)
  } finally {
    shortcutToggleBusy.value = false
  }
}

// ---- 右下角通知驻留时长（秒；后端每条通知都带当前值下发，改完立即生效） ----
const noticeSeconds = ref(5)
const noticeSaving = ref(false)

function commitNoticeDuration() {
  const sec = Math.min(60, Math.max(1, Math.round(Number(noticeSeconds.value) || 0)))
  noticeSeconds.value = sec
  if (!isTauri() || noticeSaving.value) return
  const ms = sec * 1000
  if (ms === (store.state.config.notice_duration_ms ?? 5000)) return
  noticeSaving.value = true
  void store
    .setNoticeDuration(ms)
    .then(() => showToast(`通知驻留时长已设为 ${sec} 秒`))
    .catch((e) => showToast(`设置失败：${String(e)}`))
    .finally(() => {
      noticeSaving.value = false
    })
}

// ---- 开机自启动 ----
const autostartBusy = ref(false)
// 系统真实状态探测：区分「用户开了开关」与「登录时是否真的会拉起」
const autostartStatus = ref<AutostartStatus | null>(null)
// 意图为开、但实际不会生效 → 判定失效，提示修复
const autostartFailed = computed(
  () => !!autostartStatus.value && autostartStatus.value.configured && !autostartStatus.value.enabled,
)
const autostartFailReason = computed(() => {
  const s = autostartStatus.value
  if (!s) return ''
  if (s.os_disabled) return '已被系统或安全软件在「启动项」中禁用'
  if (!s.registered) return '注册信息丢失或程序路径已变更'
  return '当前不会开机自启'
})

async function refreshAutostartStatus() {
  if (!isTauri()) return
  try {
    autostartStatus.value = await tauriApi.getRunAtStartup()
  } catch {
    // 探测失败不影响开关本身，保持上次值
  }
}

// 一键修复：重新按当前 exe 写入 Run 键并清掉系统的「禁用启动项」标记
async function repairAutostart() {
  if (autostartBusy.value) return
  autostartBusy.value = true
  try {
    await store.setRunAtStartup(true)
    await refreshAutostartStatus()
    showToast(autostartFailed.value ? '修复未完全生效，请检查安全软件启动项设置' : '开机自启动已修复')
  } catch (e) {
    showToast(`修复失败：${String(e)}`)
  } finally {
    autostartBusy.value = false
  }
}

async function onToggleAutostart() {
  if (autostartBusy.value) return
  autostartBusy.value = true
  const next = !store.state.config.run_at_startup
  try {
    await store.setRunAtStartup(next)
    await refreshAutostartStatus()
    showToast(next ? '已开启开机自启动' : '已关闭开机自启动')
  } catch (e) {
    showToast(`设置失败：${String(e)}`)
  } finally {
    autostartBusy.value = false
  }
}

onMounted(async () => {

  if (!isTauri()) return

  shortcut.value = normalizeShortcutDisplay(await tauriApi.getGlobalShortcut())

  clipShortcut.value = normalizeShortcutDisplay(store.state.config.clipboard_shortcut ?? 'Ctrl+`')

  clipSavedShortcut.value = clipShortcut.value

  searchShortcut.value = normalizeShortcutDisplay(store.state.config.search_shortcut ?? 'Ctrl+K')

  chatShortcut.value = normalizeShortcutDisplay(store.state.config.chat_shortcut ?? 'Ctrl+Shift+K')

  notesShortcut.value = normalizeShortcutDisplay(store.state.config.notes_shortcut ?? 'Ctrl+Shift+N')

  noticeSeconds.value = Math.round((store.state.config.notice_duration_ms ?? 5000) / 1000)

  void refreshAutostartStatus()

})
</script>

<template>
        <section id="sv-sec-general" class="sv-sec" aria-label="常规">
          <h3 class="sv-sec-title">常规</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">开机自动启动</span>
              <span class="setting-desc">登录 Windows 后自动在后台运行并驻留托盘（不弹出主窗口，点托盘图标可随时唤出）</span>
              <span v-if="autostartFailed" class="autostart-warn">
                ⚠ 开机自启动已失效：{{ autostartFailReason }}
                <button type="button" class="autostart-repair" :disabled="autostartBusy" @click="repairAutostart">
                  重新启用
                </button>
              </span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.run_at_startup"
              :class="{ on: store.state.config.run_at_startup }"
              :disabled="autostartBusy"
              @click="onToggleAutostart"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">通知驻留时长</span>
              <span class="setting-desc">右下角提醒弹窗停留多少秒后自动消失（1–60 秒，默认 5 秒）；鼠标点一下可立即关掉</span>
            </div>
            <div class="num-edit with-unit">
              <input
                v-model.number="noticeSeconds"
                class="num-input"
                type="number"
                min="1"
                max="60"
                step="1"
                aria-label="通知驻留时长（秒）"
                @change="commitNoticeDuration"
              />
              <span class="num-unit">秒</span>
            </div>
          </div>
        </section>

        <section id="sv-sec-ball" class="sv-sec" aria-label="悬浮球">
          <h3 class="sv-sec-title">悬浮球</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">桌面悬浮球</span>
              <span class="setting-desc">主窗口隐藏/最小化时在桌面显示悬浮球（可开启下方「与主窗口同时显示」常驻）：单击展开环形快捷菜单，双击显示主窗口，右键快捷菜单，可拖拽，贴边自动隐藏一半</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.floating_ball_enabled"
              :class="{ on: store.state.config.floating_ball_enabled }"
              @click="onToggleFloatingBall"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">悬浮球贴边自动隐藏</span>
              <span class="setting-desc">拖到屏幕边缘附近松手时自动半隐：球体贴边只露出一半，鼠标悬停时完整滑出，移开再隐回</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.floating_ball_auto_hide"
              :class="{ on: store.state.config.floating_ball_auto_hide }"
              :disabled="!store.state.config.floating_ball_enabled"
              @click="onToggleFloatingBallAutoHide"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">与主窗口同时显示</span>
              <span class="setting-desc">开启后悬浮球常驻桌面：主窗口显示时也不隐藏，单击球仍可展开菜单</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.floating_ball_with_main"
              :class="{ on: store.state.config.floating_ball_with_main }"
              :disabled="!store.state.config.floating_ball_enabled"
              @click="onToggleFloatingBallWithMain"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">静止时保持转动</span>
              <span class="setting-desc">开启后悬浮球静止时陀螺环持续旋转、粒子满帧渲染（观感更炫酷，笔记本更耗电发热）；关闭则静止时暂停自旋并降到 24fps 省电，交互时立即恢复</span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.floating_ball_idle_spin"
              :class="{ on: store.state.config.floating_ball_idle_spin }"
              :disabled="!store.state.config.floating_ball_enabled"
              @click="onToggleFloatingBallIdleSpin"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <div class="setting-row fb-btn-row">
            <div class="setting-info">
              <span class="setting-name">环形菜单按钮</span>
              <span class="setting-desc">单击悬浮球展开的按钮集合（最多 {{ FLOATING_BALL_MAX_BUTTONS }} 个）：下方按钮点击添加，已选中的可上移/下移/移除。「AI 对话」按「AI 助手 → 以独立窗口打开」的设置决定唤起独立小窗还是主窗抽屉</span>
            </div>
            <div class="fb-btn-cfg">
              <div class="fb-btn-list">
                <span v-for="(id, i) in ballButtons" :key="id" class="fb-chip">
                  {{ FLOATING_BALL_BUTTONS[id]?.label ?? id }}
                  <button type="button" class="fb-chip-btn" :disabled="i === 0" aria-label="上移" @click="moveBallButton(i, -1)">↑</button>
                  <button type="button" class="fb-chip-btn" :disabled="i === ballButtons.length - 1" aria-label="下移" @click="moveBallButton(i, 1)">↓</button>
                  <button type="button" class="fb-chip-btn fb-chip-remove" aria-label="移除" @click="removeBallButton(id)">×</button>
                </span>
                <span v-if="ballButtons.length === 0" class="fb-chip-empty">未配置（悬浮球仅支持拖拽/双击）</span>
              </div>
              <div class="fb-btn-add">
                <button
                  v-for="(meta, id) in FLOATING_BALL_BUTTONS"
                  :key="id"
                  type="button"
                  class="fb-chip-add"
                  :disabled="ballButtons.includes(id) || ballButtons.length >= FLOATING_BALL_MAX_BUTTONS"
                  @click="addBallButton(id)"
                >
                  + {{ meta.label }}
                </button>
              </div>
            </div>
          </div>
        </section>

        <section id="sv-sec-shortcut" class="sv-sec" aria-label="快捷键">
          <h3 class="sv-sec-title">快捷键</h3>
          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">全局快捷键</span>
              <span class="setting-desc">支持手动输入或按键录入，无冲突自动保存；右侧开关可临时禁用（保留键值）</span>
            </div>
            <div class="shortcut-edit" :class="{ off: !store.state.config.global_shortcut_enabled }">
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
              <button
                class="toggle shortcut-toggle"
                role="switch"
                type="button"
                :aria-checked="store.state.config.global_shortcut_enabled"
                :class="{ on: store.state.config.global_shortcut_enabled }"
                :disabled="shortcutToggleBusy"
                :title="store.state.config.global_shortcut_enabled ? '点击禁用该快捷键' : '点击启用该快捷键'"
                @click="toggleShortcutEnabled('main')"
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          </div>
          <p v-if="shortcutError" class="shortcut-error">{{ shortcutError }}</p>

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">剪贴板呼出快捷键</span>
              <span class="setting-desc">任何应用中一键唤起剪贴板历史浮层；右侧开关可临时禁用（保留键值）</span>
            </div>
            <div class="shortcut-edit" :class="{ off: !store.state.config.clipboard_shortcut_enabled }">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="clipInputRef"
                  v-model="clipShortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="clipListening"
                  placeholder="Ctrl+`"
                  @keydown="onClipShortcutKeydown"
                  @keydown.enter="commitClipShortcut"
                  @blur="onClipShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListenClipShortcut">
                  {{ clipListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
              <button
                class="toggle shortcut-toggle"
                role="switch"
                type="button"
                :aria-checked="store.state.config.clipboard_shortcut_enabled"
                :class="{ on: store.state.config.clipboard_shortcut_enabled }"
                :disabled="shortcutToggleBusy"
                :title="store.state.config.clipboard_shortcut_enabled ? '点击禁用该快捷键' : '点击启用该快捷键'"
                @click="toggleShortcutEnabled('clipboard')"
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          </div>
          <p v-if="clipError" class="shortcut-error">{{ clipError }}</p>

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">搜索呼出快捷键</span>
              <span class="setting-desc">任何应用中一键唤起全局搜索；弹窗内的快捷键提示也会随之更新；右侧开关可临时禁用（保留键值）</span>
            </div>
            <div class="shortcut-edit" :class="{ off: !store.state.config.search_shortcut_enabled }">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="searchInputRef"
                  v-model="searchShortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="searchListening"
                  placeholder="Ctrl+K"
                  @keydown="onSearchShortcutKeydown"
                  @keydown.enter="commitSearchShortcut"
                  @blur="onSearchShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListenSearchShortcut">
                  {{ searchListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
              <button
                class="toggle shortcut-toggle"
                role="switch"
                type="button"
                :aria-checked="store.state.config.search_shortcut_enabled"
                :class="{ on: store.state.config.search_shortcut_enabled }"
                :disabled="shortcutToggleBusy"
                :title="store.state.config.search_shortcut_enabled ? '点击禁用该快捷键' : '点击启用该快捷键'"
                @click="toggleShortcutEnabled('search')"
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          </div>
          <p v-if="searchError" class="shortcut-error">{{ searchError }}</p>

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">AI 对话呼出快捷键</span>
              <span class="setting-desc">任何应用中一键唤起 AI 对话（形态由「AI 助手」里的开关决定）；右侧开关可临时禁用（保留键值）</span>
            </div>
            <div class="shortcut-edit" :class="{ off: !store.state.config.chat_shortcut_enabled }">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="chatInputRef"
                  v-model="chatShortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="chatListening"
                  placeholder="Ctrl+Shift+K"
                  @keydown="onChatShortcutKeydown"
                  @keydown.enter="commitChatShortcut"
                  @blur="onChatShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListenChatShortcut">
                  {{ chatListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
              <button
                class="toggle shortcut-toggle"
                role="switch"
                type="button"
                :aria-checked="store.state.config.chat_shortcut_enabled"
                :class="{ on: store.state.config.chat_shortcut_enabled }"
                :disabled="shortcutToggleBusy"
                :title="store.state.config.chat_shortcut_enabled ? '点击禁用该快捷键' : '点击启用该快捷键'"
                @click="toggleShortcutEnabled('chat')"
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          </div>
          <p v-if="chatError" class="shortcut-error">{{ chatError }}</p>

          <div class="setting-row shortcut-row">
            <div class="setting-info">
              <span class="setting-name">速记呼出快捷键</span>
              <span class="setting-desc">任何应用中一键呼出主窗并新建速记；右侧开关可临时禁用（保留键值）</span>
            </div>
            <div class="shortcut-edit" :class="{ off: !store.state.config.notes_shortcut_enabled }">
              <div class="shortcut-input-wrap">
                <Keyboard :size="14" :stroke-width="2" class="shortcut-icon" />
                <input
                  ref="notesInputRef"
                  v-model="notesShortcut"
                  class="shortcut-input"
                  type="text"
                  spellcheck="false"
                  :readonly="notesListening"
                  placeholder="Ctrl+Shift+N"
                  @keydown="onNotesShortcutKeydown"
                  @keydown.enter="commitNotesShortcut"
                  @blur="onNotesShortcutBlur"
                />
                <button class="shortcut-record-btn" type="button" @click="startListenNotesShortcut">
                  {{ notesListening ? '按下组合键…' : '录入' }}
                </button>
              </div>
              <button
                class="toggle shortcut-toggle"
                role="switch"
                type="button"
                :aria-checked="store.state.config.notes_shortcut_enabled"
                :class="{ on: store.state.config.notes_shortcut_enabled }"
                :disabled="shortcutToggleBusy"
                :title="store.state.config.notes_shortcut_enabled ? '点击禁用该快捷键' : '点击启用该快捷键'"
                @click="toggleShortcutEnabled('notes')"
              >
                <span class="toggle-knob"></span>
              </button>
            </div>
          </div>
          <p v-if="notesError" class="shortcut-error">{{ notesError }}</p>
        </section>
</template>
