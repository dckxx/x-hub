<script setup lang="ts">
import { computed } from 'vue'

const now = new Date()
const year = now.getFullYear()
const month = now.getMonth()
const today = now.getDate()

const monthLabel = computed(() => `${year}年${month + 1}月`)

const daysInMonth = computed(() => new Date(year, month + 1, 0).getDate())

const weekdays = ['一', '二', '三', '四', '五', '六', '日']

const blankCount = computed(() => {
  const firstDay = new Date(year, month, 1).getDay()
  return firstDay === 0 ? 6 : firstDay - 1
})

const days = computed(() => Array.from({ length: daysInMonth.value }, (_, i) => i + 1))
const blanks = computed(() => Array.from({ length: blankCount.value }, () => 0))
</script>

<template>
  <div class="card calendar-card">
    <div class="card-header">
      <span class="card-title">{{ monthLabel }}</span>
      <div class="calendar-nav">
        <button class="calendar-nav-btn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
            <path d="M16 9l-5 5 5 5" stroke="#4B5563" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
        </button>
        <button class="calendar-nav-btn">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
            <path d="M12 9l5 5-5 5" stroke="#4B5563" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
          </svg>
        </button>
      </div>
    </div>
    <div class="weekdays">
      <div v-for="wd in weekdays" :key="wd" class="weekday">{{ wd }}</div>
    </div>
    <div class="calendar-grid">
      <div v-for="(_, i) in blanks" :key="'b' + i" class="day day--blank"></div>
      <div
        v-for="d in days"
        :key="d"
        class="day"
        :class="{ 'day--active': d === today }"
      >{{ d }}</div>
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
.calendar-card {
  height: 420px;
  display: flex;
  flex-direction: column;
  gap: 20px;
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
.calendar-nav {
  display: flex;
  gap: 8px;
}
.calendar-nav-btn {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  background: #F3F4F6;
  border: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.weekdays {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 8px;
}
.weekday {
  text-align: center;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-muted);
  height: 24px;
  line-height: 24px;
}
.calendar-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 8px;
  flex: 1;
}
.day {
  aspect-ratio: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  font-size: 14px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background 0.2s;
}
.day:hover { background: #F3F4F6; }
.day--active {
  background: var(--accent);
  color: #fff;
  font-weight: 700;
}
.day--blank { cursor: default; }
.day--blank:hover { background: transparent; }
</style>
