<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { Quote } from 'lucide-vue-next'

const now = ref(new Date())
let timer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  timer = setInterval(() => {
    now.value = new Date()
  }, 30_000)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
})

const WEEKDAYS = ['日', '一', '二', '三', '四', '五', '六'] as const

const timeText = computed(() => {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(now.value.getHours())}:${pad(now.value.getMinutes())}`
})

const dateText = computed(() => {
  const d = now.value
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${WEEKDAYS[d.getDay()]}`
})

const QUOTE = '日拱一卒，功不唐捐。'
</script>

<template>
  <section class="card clock-card" aria-label="时钟">
    <div class="clock-time">{{ timeText }}</div>
    <div class="clock-date">{{ dateText }}</div>
    <div class="clock-quote">
      <Quote :size="12" :stroke-width="2" aria-hidden="true" />
      <span>{{ QUOTE }}</span>
    </div>
  </section>
</template>

<style scoped>
.clock-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px;
}
.clock-time {
  font-size: 30px;
  font-weight: 700;
  line-height: 1.1;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
}
.clock-date {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-3);
}
.clock-quote {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  padding-top: 10px;
  border-top: 1px solid var(--border-soft);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-3);
}
.clock-quote :deep(svg) {
  flex-shrink: 0;
  color: var(--brand-500);
}
.clock-quote span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
