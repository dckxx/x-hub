<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudRain,
  CloudSnow,
  CloudSun,
  Quote,
  Sun,
} from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import { randomLocalQuote } from '../utils/quotes'
import { describeWeather } from '../utils/weather'

const store = useStore()

const now = ref(new Date())
let timer: ReturnType<typeof setTimeout> | null = null

onMounted(() => {
  const tick = () => {
    now.value = new Date()
    // 对齐下一秒边界触发，避免相位漂移导致的跳秒；每次读钟不自增，节流后回前台也能立即正确
    timer = setTimeout(tick, 1005 - (Date.now() % 1000))
  }
  tick()
})
onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})

const WEEKDAYS = ['日', '一', '二', '三', '四', '五', '六'] as const

const timeText = computed(() => {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(now.value.getHours())}:${pad(now.value.getMinutes())}:${pad(now.value.getSeconds())}`
})

const dateText = computed(() => {
  const d = now.value
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${WEEKDAYS[d.getDay()]}`
})

// ---- 语录（炫彩 + 点击随机） ----
const currentQuote = ref(randomLocalQuote())

// 在线金句拉取成功后同步到当前显示
watch(
  () => store.state.quote,
  (q) => {
    if (q && store.state.config.quote_source === 'online' && store.state.online) {
      currentQuote.value = { content: q.content, from: q.from }
    }
  },
)

const displayQuote = computed(() => {
  const custom = store.state.config.clock_quote?.trim()
  if (custom) return { content: custom, from: '' }
  return currentQuote.value
})

async function nextQuote() {
  const custom = store.state.config.clock_quote?.trim()
  if (custom) return
  const before = currentQuote.value.content
  if (store.state.config.quote_source === 'online' && store.state.online) {
    await store.refreshQuote()
    // 在线拉取未带来新内容（失败或重复）时，本地语料兜底换一条，保证点击总有反馈
    if (currentQuote.value.content === before) {
      currentQuote.value = randomLocalQuote()
    }
  } else {
    currentQuote.value = randomLocalQuote()
  }
}

// ---- 天气（右上角） ----
const weather = computed(() => store.state.weather)
const weatherDesc = computed(() =>
  weather.value ? describeWeather(weather.value.weather_code) : null,
)

const weatherIcon = computed(() => {
  switch (weatherDesc.value?.icon) {
    case 'sun':
      return Sun
    case 'cloud-sun':
      return CloudSun
    case 'cloud-fog':
      return CloudFog
    case 'cloud-drizzle':
      return CloudDrizzle
    case 'cloud-rain':
      return CloudRain
    case 'cloud-snow':
      return CloudSnow
    case 'cloud-lightning':
      return CloudLightning
    default:
      return Cloud
  }
})

const tempText = computed(() =>
  weather.value ? `${Math.round(weather.value.temperature)}°` : '',
)

const weatherSubText = computed(() => {
  if (!weather.value || !weatherDesc.value) return ''
  const city = weather.value.city ? ` · ${weather.value.city}` : ''
  return `${weatherDesc.value.label}${city}`
})
</script>

<template>
  <section class="card clock-card" aria-label="时钟">
    <div class="clock-top">
      <div class="clock-main">
        <div class="clock-time">{{ timeText }}</div>
        <div class="clock-date">{{ dateText }}</div>
      </div>

      <!-- 天气（右上角） -->
      <div
        v-if="weather"
        class="clock-weather"
        :title="`${tempText} ${weatherSubText}`"
      >
        <component
          :is="weatherIcon"
          class="cw-icon"
          :size="22"
          :stroke-width="1.8"
          aria-hidden="true"
        />
        <div class="cw-temp">{{ tempText }}</div>
        <div class="cw-label">{{ weatherSubText }}</div>
      </div>
    </div>

    <!-- 金句：占满整行（可延伸到天气区下方） -->
    <button
      class="clock-quote"
      type="button"
      :title="displayQuote.content"
      aria-label="点击换一条语录"
      @click="nextQuote"
    >
      <Quote :size="13" :stroke-width="2" aria-hidden="true" />
      <span>{{ displayQuote.content }}</span>
    </button>
  </section>
</template>

<style scoped>
.clock-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px;
}
.clock-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
.clock-main {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  flex: 1;
}
.clock-time {
  font-size: 1.875rem;
  font-weight: 700;
  line-height: 1.1;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  color: var(--text-1);
}
.clock-date {
  font-size: 0.8125rem;
  font-weight: 500;
  color: var(--text-3);
}
.clock-quote {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  width: 100%;
  padding: 8px 0 0;
  border: 0;
  border-top: 1px solid var(--border-soft);
  background: none;
  cursor: pointer;
  text-align: left;
  font: inherit;
  color: inherit;
}
.clock-quote :deep(svg) {
  flex-shrink: 0;
  margin-top: 4px;
  color: var(--brand-500);
}
.clock-quote span {
  font-size: 0.875rem;
  line-height: 1.5;
  font-weight: 600;
  white-space: normal;
  overflow-wrap: break-word;
  /* 炫彩渐变文字：品牌色 → 粉 → 蓝 */
  background: linear-gradient(100deg, var(--brand-500), #f472b6 45%, #38bdf8 80%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  color: transparent;
  transition: opacity 0.2s ease;
}
.clock-quote:hover span {
  opacity: 0.75;
}
.clock-quote:active span {
  opacity: 0.55;
}

/* 天气 */
/* 无独立底色：直接融进时钟卡片表面 */
.clock-weather {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
}
.cw-icon {
  color: var(--brand-500);
}
.cw-temp {
  font-size: 1.25rem;
  font-weight: 700;
  line-height: 1;
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
}
.cw-label {
  font-size: 0.6875rem;
  color: var(--text-3);
  white-space: nowrap;
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
