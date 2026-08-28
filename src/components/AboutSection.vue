<script setup lang="ts">
import { inject, onMounted, ref } from 'vue'
import { marked } from 'marked'
import { ChevronDown, RefreshCw } from 'lucide-vue-next'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const version = ref('')
const loading = ref(true)
const changelogHtml = ref('')
// 版本历史折叠：默认收起，避免列表过长占满设置页
const changelogExpanded = ref(false)

// ---- 应用更新：仅保留「检查更新」按钮；发现新版本由全局弹窗（UpdateCheckDialog）接管 ----
const checking = ref(false)

async function onCheckUpdate() {
  if (!isTauri()) {
    showToast('更新功能仅在桌面应用中可用')
    return
  }
  if (checking.value) return
  checking.value = true
  try {
    // manual=true：手动检查忽略「跳过此版本」记录，用户主动查看能再次取到该版本
    const info = await tauriApi.checkForUpdate(true)
    if (info.available) {
      // 后端已广播 update-available → 全局弹窗自动弹出并展示版本/说明
      if (info.ready) showToast(`新版 v${info.version} 已就绪，请在弹窗中点击「立即重启」`)
    } else {
      showToast(`已是最新版本（v${info.current}）`)
    }
  } catch (e) {
    showToast(`检查更新失败：${String(e)}`)
  } finally {
    checking.value = false
  }
}

onMounted(async () => {
  try {
    const info = await tauriApi.getAppInfo()
    version.value = info.version
    changelogHtml.value = marked.parse(info.changelog, { async: false }) as string
  } catch {
    version.value = ''
  } finally {
    loading.value = false
  }
})

function onToggleAutoUpdate() {
  void store.setAutoUpdateEnabled(!store.state.config.auto_update_enabled)
}
</script>

<template>
  <div class="about-section">
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">当前版本</span>
        <span class="setting-desc">版本号以 README 为准，随安装包构建同步</span>
      </div>
      <div class="about-version-wrap">
        <span class="about-version">{{ loading ? '…' : `v${version}` }}</span>
        <button class="ghost-btn upd-check-btn" type="button" :disabled="checking" @click="onCheckUpdate">
          <RefreshCw
            :size="13"
            :stroke-width="2"
            class="upd-check-icon"
            :class="{ spinning: checking }"
          />
          {{ checking ? '检查中…' : '检查更新' }}
        </button>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">自动检查更新</span>
        <span class="setting-desc">启动后静默检查新版本，发现更新时弹窗提示；关闭后不再发起自动检查</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="store.state.config.auto_update_enabled"
        :class="{ on: store.state.config.auto_update_enabled }"
        @click="onToggleAutoUpdate"
      >
        <span class="toggle-knob"></span>
      </button>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">开源许可</span>
        <span class="setting-desc">本项目基于 MIT 许可开源</span>
      </div>
      <a
        class="ghost-btn about-license"
        href="https://github.com/dckxx/x-hub"
        target="_blank"
        rel="noopener noreferrer"
      >
        MIT License
      </a>
    </div>

    <div class="about-changelog">
      <button
        class="about-changelog-head"
        type="button"
        :aria-expanded="changelogExpanded"
        @click="changelogExpanded = !changelogExpanded"
      >
        <h4 class="about-changelog-title">版本历史</h4>
        <ChevronDown
          :size="14"
          :stroke-width="2"
          class="about-changelog-chevron"
          :class="{ open: changelogExpanded }"
        />
      </button>
      <div v-show="changelogExpanded">
        <div v-if="loading" class="about-changelog-empty">加载中…</div>
        <div v-else class="md-body" v-html="changelogHtml"></div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 与 SettingsView 保持一致的设置行布局（scoped 隔离，需在此自绘） */
.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 0;
}
.setting-row + .setting-row {
  border-top: 1px solid var(--border-soft);
}
.setting-info {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.setting-name {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-1);
}
.setting-desc {
  flex-basis: 100%;
  font-size: 0.75rem;
  color: var(--text-3);
}
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

.about-version {
  font-size: 0.875rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--brand-500);
}

/* 应用更新区块 */
.about-version-wrap {
  display: flex;
  align-items: center;
  gap: 10px;
}
.ghost-btn.upd-check-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 0.75rem;
  padding: 4px 10px;
}
.upd-check-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.upd-check-icon {
  color: var(--text-3);
}
.upd-check-icon.spinning {
  animation: upd-spin 1s linear infinite;
  color: var(--brand-500);
}
@keyframes upd-spin {
  to {
    transform: rotate(360deg);
  }
}

.about-license {
  text-decoration: none;
}

.about-changelog {
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border-soft);
}
.about-changelog-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  padding: 0;
  margin-bottom: var(--space-3);
  border: none;
  background: transparent;
  cursor: pointer;
  color: inherit;
}
.about-changelog-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 700;
  color: var(--text-2);
}
.about-changelog-chevron {
  color: var(--text-3);
  transition: transform 0.18s ease-out;
}
.about-changelog-chevron.open {
  transform: rotate(180deg);
}
.about-changelog-empty {
  font-size: 0.8125rem;
  color: var(--text-3);
}

/* 只读静态 Markdown 渲染（复用 marked，样式对齐速记预览） */
.md-body {
  font-size: 0.8125rem;
  line-height: 1.7;
  color: var(--text-2);
}
.md-body :deep(h1) {
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 16px 0 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.md-body :deep(h1:first-child) {
  margin-top: 0;
}
.md-body :deep(h2) {
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 16px 0 8px;
}
.md-body :deep(h3) {
  font-size: 0.84375rem;
  font-weight: 700;
  color: var(--text-1);
  margin: 12px 0 6px;
}
.md-body :deep(p) {
  margin: 6px 0;
}
.md-body :deep(ul),
.md-body :deep(ol) {
  padding-left: 20px;
  margin: 6px 0;
}
.md-body :deep(ol) {
  list-style: decimal;
}
.md-body :deep(ol > li) {
  display: list-item;
}
.md-body :deep(li) {
  margin: 3px 0;
}
.md-body :deep(code) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: 5px;
  padding: 1px 6px;
  font-size: 0.75rem;
  font-family: 'FiraCode', Consolas, monospace;
}
.md-body :deep(pre) {
  background: var(--bg-card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 12px;
  overflow-x: auto;
  margin: 8px 0;
}
.md-body :deep(pre code) {
  background: transparent;
  border: none;
  padding: 0;
}
.md-body :deep(a) {
  color: var(--brand-500);
}
.md-body :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-soft);
  margin: 14px 0;
}
</style>
