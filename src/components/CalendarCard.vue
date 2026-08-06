<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronLeft, ChevronRight } from 'lucide-vue-next'

const today = new Date()
const viewYear = ref(today.getFullYear())
const viewMonth = ref(today.getMonth()) // 0-based

const weekLabels = ['日', '一', '二', '三', '四', '五', '六']

const daysInMonth = computed(() => new Date(viewYear.value, viewMonth.value + 1, 0).getDate())
// 当月 1 号是周几（0 = 周日），用于前置空格
const leadingEmpty = computed(() => new Date(viewYear.value, viewMonth.value, 1).getDay())

function isToday(day: number): boolean {
  return (
    viewYear.value === today.getFullYear() &&
    viewMonth.value === today.getMonth() &&
    day === today.getDate()
  )
}

function prevMonth() {
  viewMonth.value -= 1
  if (viewMonth.value < 0) {
    viewMonth.value = 11
    viewYear.value -= 1
  }
}

function nextMonth() {
  viewMonth.value += 1
  if (viewMonth.value > 11) {
    viewMonth.value = 0
    viewYear.value += 1
  }
}

function goToday() {
  viewYear.value = today.getFullYear()
  viewMonth.value = today.getMonth()
}
</script>

<template>
  <section class="card calendar" aria-label="日历">
    <h3 class="cal-title">{{ viewYear }}年{{ viewMonth + 1 }}月</h3>
    <div class="cal-controls">
      <button class="cal-nav-btn today-btn" title="回到今天" @click="goToday">今</button>
      <div class="cal-week" aria-hidden="true">
        <span v-for="w in weekLabels" :key="w" class="cal-week-label">{{ w }}</span>
      </div>
      <div class="cal-nav">
        <button class="cal-nav-btn" title="上个月" @click="prevMonth">
          <ChevronLeft :size="13" :stroke-width="2.2" />
        </button>
        <button class="cal-nav-btn" title="下个月" @click="nextMonth">
          <ChevronRight :size="13" :stroke-width="2.2" />
        </button>
      </div>
    </div>

    <div class="cal-grid" role="grid">
      <span
        v-for="i in leadingEmpty"
        :key="'e' + i"
        class="cal-cell empty"
        aria-hidden="true"
      ></span>
      <div
        v-for="d in daysInMonth"
        :key="d"
        class="cal-cell"
        :class="{ today: isToday(d) }"
        role="gridcell"
      >
        {{ d }}
      </div>
    </div>
  </section>
</template>

<style scoped>
.calendar {
  padding: 16px 16px 12px;
  flex-shrink: 0;
}
.cal-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-1);
  letter-spacing: -0.01em;
  white-space: nowrap;
  margin-bottom: 8px;
}
.cal-controls {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 4px;
}
.cal-nav {
  display: flex;
  gap: 2px;
}
.cal-nav-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: var(--bg-card-soft);
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.today-btn {
  width: auto;
  padding: 0 7px;
  margin-left: 2px;
  font-size: 11px;
  font-weight: 600;
}
.cal-nav-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.cal-nav-btn:active {
  transform: scale(0.92);
}

.cal-week {
  flex: 1;
  display: grid;
  grid-template-columns: repeat(7, 1fr);
}
.cal-week-label {
  text-align: center;
  font-size: 10px;
  color: var(--text-3);
}

.cal-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}
.cal-cell {
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  color: var(--text-2);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, transform 0.15s;
}
.cal-cell:not(.empty):hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.cal-cell.today {
  background: var(--brand-500);
  color: #fff;
  font-weight: 700;
}
.cal-cell.empty {
  cursor: default;
}
</style>
