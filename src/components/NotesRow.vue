<script setup lang="ts">
const notes = [
  { type: 'yellow', title: '便签', text: '下午 3 点与设计团队评审新版交互方案' },
  { type: 'pink', title: '待办', text: '整理 Q3 产品路线图，更新项目文档' },
]
</script>

<template>
  <div class="notes-row">
    <div
      v-for="note in notes"
      :key="note.title"
      class="note"
      :class="`note-${note.type}`"
    >
      <span class="note-title">{{ note.title }}</span>
      <span class="note-text">{{ note.text }}</span>
    </div>
  </div>
</template>

<style scoped>
.notes-row {
  flex: 1;
  display: flex;
  gap: 16px;
}
.note {
  flex: 1;
  border-radius: var(--radius-md);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-shadow: var(--shadow-sm);
}
.note-yellow { background: #FEF3C7; }
.note-pink { background: #FCE7F3; }
.note-title { font-size: 14px; font-weight: 700; }
.note-yellow .note-title, .note-yellow .note-text { color: #92400E; }
.note-pink .note-title, .note-pink .note-text { color: #9D174D; }
.note-text { font-size: 13px; line-height: 1.5; }
</style>
