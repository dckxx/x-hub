<script setup lang="ts">
import { computed } from 'vue'
import { useStore } from '../stores/workbench'

defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const store = useStore()

const isDark = computed(() => store.state.config.theme === 'dark')
const alwaysOnTop = computed(() => store.state.config.window.always_on_top)

function toggleTheme() {
  store.setTheme(isDark.value ? 'light' : 'dark')
}

function toggleAlwaysOnTop() {
  store.setAlwaysOnTop(!alwaysOnTop.value)
}
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="emit('close')">
        <div class="modal-card" role="dialog" aria-label="设置">
          <div class="settings-head">
            <h2 class="dialog-title">设置</h2>
            <button class="icon-btn" title="关闭" @click="emit('close')">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
                <path d="M18 6L6 18M6 6l12 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
              </svg>
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">外观主题</span>
              <span class="setting-desc">切换亮色 / 暗色，自动保存</span>
            </div>
            <div class="theme-switch">
              <button
                class="theme-pill"
                :class="{ active: !isDark }"
                @click="isDark && toggleTheme()"
              >
                ☀️ 亮色
              </button>
              <button
                class="theme-pill"
                :class="{ active: isDark }"
                @click="!isDark && toggleTheme()"
              >
                🌙 暗色
              </button>
            </div>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">窗口置顶</span>
              <span class="setting-desc">保持窗口显示在其他应用之上</span>
            </div>
            <button
              class="toggle"
              :class="{ on: alwaysOnTop }"
              role="switch"
              :aria-checked="alwaysOnTop"
              @click="toggleAlwaysOnTop"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <p class="settings-foot">
            🔒 所有数据默认存储在本地，不会上传云端
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

.theme-switch {
  display: flex;
  gap: 4px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 4px;
}
.theme-pill {
  border: none;
  background: transparent;
  padding: 6px 14px;
  border-radius: var(--radius-pill);
  font-size: 13px;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.theme-pill.active {
  background: var(--bg-card);
  color: var(--brand-500);
  font-weight: 600;
  box-shadow: var(--shadow-card);
}

.toggle {
  width: 44px;
  height: 24px;
  border: none;
  border-radius: var(--radius-pill);
  background: var(--text-4);
  position: relative;
  cursor: pointer;
  transition: background 0.2s;
  flex-shrink: 0;
}
.toggle.on {
  background: var(--brand-500);
}
.toggle-knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  transition: transform 0.2s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
.toggle.on .toggle-knob {
  transform: translateX(20px);
}

.settings-foot {
  margin-top: 16px;
  font-size: 12px;
  color: var(--text-3);
  text-align: center;
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
