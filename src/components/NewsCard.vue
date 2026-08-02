<script setup lang="ts">
const news = [
  { time: '08:30', title: 'OpenAI 发布 GPT-5 预览版，多模态能力大幅提升' },
  { time: '07:15', title: '苹果 WWDC 2026 公布 visionOS 3 与 AI 功能' },
  { time: '06:50', title: '全球气候峰会达成新协议，2030 碳中和目标提速' },
]
</script>

<template>
  <div class="card news-card">
    <div class="card-header">
      <span class="card-title">新闻简报</span>
      <span class="card-more">更多 ›</span>
    </div>
    <div class="news-list">
      <div v-for="item in news" :key="item.time" class="news-item">
        <span class="news-time">{{ item.time }}</span>
        <span class="news-title">{{ item.title }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card {
  background: var(--surface);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  padding: 20px;
}
.news-card {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 16px;
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
.card-more {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
}
.news-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
}
.news-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.news-time {
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
}
.news-title {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}
</style>
