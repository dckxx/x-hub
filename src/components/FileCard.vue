<script setup lang="ts">
import { ref } from 'vue'

const tabs = ['文件夹', '文本/Office', '压缩包', '图片', '其他', '自定义']
const activeTab = ref('文件夹')

const files = [
  { name: '工作文档', icon: 'folder', color: '#FCD34D', bg: '#FCD34D' },
  { name: '个人资料', icon: 'folder', color: '#F87171', bg: '#F87171' },
  { name: '设计资源', icon: 'folder', color: '#60A5FA', bg: '#60A5FA' },
  { name: '项目素材', icon: 'folder', color: '#34D399', bg: '#34D399' },
  { name: '2026 年报', icon: 'doc', color: '#9CA3AF', bg: '#E5E7EB' },
  { name: '会议纪要', icon: 'doc', color: '#3B82F6', bg: '#DBEAFE' },
  { name: '产品原型', icon: 'doc', color: '#EC4899', bg: '#FCE7F3' },
  { name: '品牌规范', icon: 'doc', color: '#10B981', bg: '#D1FAE5' },
  { name: '代码仓库', icon: 'folder', color: '#A78BFA', bg: '#A78BFA' },
  { name: '产品需求', icon: 'doc', color: '#D97706', bg: '#FEF3C7' },
  { name: '用户访谈', icon: 'doc', color: '#7C3AED', bg: '#DDD6FE' },
  { name: '周报合集', icon: 'doc', color: '#DC2626', bg: '#FECACA' },
]
</script>

<template>
  <div class="card file-card">
    <div class="card-header">
      <span class="card-title">文件管理</span>
      <div class="file-actions">
        <button class="icon-btn" title="新建">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
            <path d="M12 5v14M5 12h14" stroke="#4B5563" stroke-width="2" stroke-linecap="round"/>
          </svg>
        </button>
        <button class="icon-btn" title="更多">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="6" r="2" fill="#4B5563"/>
            <circle cx="12" cy="12" r="2" fill="#4B5563"/>
            <circle cx="12" cy="18" r="2" fill="#4B5563"/>
          </svg>
        </button>
      </div>
    </div>
    <div class="tabs-row">
      <button
        v-for="tab in tabs"
        :key="tab"
        class="tab"
        :class="{ 'tab--active': activeTab === tab }"
        @click="activeTab = tab"
      >{{ tab }}</button>
    </div>
    <div class="file-grid">
      <div v-for="file in files" :key="file.name" class="file-item">
        <svg v-if="file.icon === 'folder'" width="48" height="48" viewBox="0 0 48 48" fill="none">
          <path d="M6 14c0-2.21 1.79-4 4-4h8l4 4h16c2.21 0 4 1.79 4 4v18c0 2.21-1.79 4-4 4H10c-2.21 0-4-1.79-4-4V14z" :fill="file.bg"/>
        </svg>
        <svg v-else width="48" height="48" viewBox="0 0 48 48" fill="none">
          <rect x="8" y="6" width="32" height="38" rx="4" :fill="file.bg"/>
          <path d="M14 16h20M14 24h20M14 32h14" :stroke="file.color" stroke-width="2" stroke-linecap="round"/>
        </svg>
        <span class="file-name">{{ file.name }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card {
  background: var(--surface);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-md);
  padding: 20px;
}
.file-card {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 20px;
  overflow: hidden;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.card-title {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
}
.file-actions {
  display: flex;
  gap: 12px;
}
.icon-btn {
  width: 36px;
  height: 36px;
  border-radius: var(--radius-sm);
  background: #F3F4F6;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: background 0.15s;
}
.icon-btn:hover { background: #E5E7EB; }
.tabs-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.tab {
  padding: 8px 16px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  background: #F3F4F6;
  cursor: pointer;
  border: none;
  font-family: inherit;
  transition: background 0.15s, color 0.15s;
}
.tab:hover { background: #E5E7EB; }
.tab--active {
  background: var(--text-primary);
  color: #fff;
}
.file-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 16px;
  flex: 1;
  overflow-y: auto;
}
.file-item {
  background: #F9FAFB;
  border-radius: var(--radius-md);
  padding: 16px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  cursor: pointer;
  transition: transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1), box-shadow 0.18s, background 0.18s;
}
.file-item:hover {
  transform: translateY(-3px);
  background: #fff;
  box-shadow: var(--shadow-md);
}
.file-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-secondary);
  text-align: center;
}
</style>
