<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { marked } from 'marked'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

const store = useStore()

const version = ref('')
const loading = ref(true)
const changelogHtml = ref('')

onMounted(async () => {
  if (!isTauri()) {
    version.value = '预览模式'
    loading.value = false
    return
  }
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

function onToggleWhatsNew() {
  void store.setWhatsNewEnabled(!store.state.config.whats_new_enabled)
}
</script>

<template>
  <div class="about-section">
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">当前版本</span>
        <span class="setting-desc">版本号以 README 为准，随安装包构建同步</span>
      </div>
      <span class="about-version">{{ loading ? '…' : `v${version}` }}</span>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-name">升级后显示更新说明</span>
        <span class="setting-desc">检测到新版本时弹一次 What's New（默认关闭）</span>
      </div>
      <button
        class="toggle"
        role="switch"
        type="button"
        :aria-checked="store.state.config.whats_new_enabled"
        :class="{ on: store.state.config.whats_new_enabled }"
        @click="onToggleWhatsNew"
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
      <h4 class="about-changelog-title">版本历史</h4>
      <div v-if="loading" class="about-changelog-empty">加载中…</div>
      <div v-else class="md-body" v-html="changelogHtml"></div>
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
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.setting-desc {
  flex-basis: 100%;
  font-size: 12px;
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
  font-size: 14px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--brand-500);
}
.about-license {
  text-decoration: none;
}

.about-changelog {
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border-soft);
}
.about-changelog-title {
  margin: 0 0 var(--space-3);
  font-size: 13px;
  font-weight: 700;
  color: var(--text-2);
}
.about-changelog-empty {
  font-size: 13px;
  color: var(--text-3);
}

/* 只读静态 Markdown 渲染（复用 marked，样式对齐速记预览） */
.md-body {
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-2);
}
.md-body :deep(h1) {
  font-size: 18px;
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
  font-size: 15px;
  font-weight: 700;
  color: var(--text-1);
  margin: 16px 0 8px;
}
.md-body :deep(h3) {
  font-size: 13.5px;
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
  font-size: 12px;
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
