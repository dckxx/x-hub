<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  AlarmClock,
  AlertTriangle,
  ArrowRight,
  Boxes,
  CalendarDays,
  Clock,
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudRain,
  CloudSnow,
  CloudSun,
  Cpu,
  File as FileIcon,
  FileText,
  Flame,
  FolderOpen,
  Globe,
  ListTodo,
  MemoryStick,
  PanelTopClose,
  Pin,
  Plus,
  Puzzle,
  Quote,
  Repeat,
  Settings2,
  StickyNote,
  Sun,
  Timer,
} from 'lucide-vue-next'
import { accentFor, accentOf, iconSrc, isImageIcon, CATEGORY_ICONS } from '../composables/useResourceIcon'
import { dashModuleTitle } from '../composables/useDashboardLayout'
import { dashPreviewData } from '../composables/useDashPreviewData'
import { SUDA_CUSTOM_IDS } from '../utils/sudaCustom'
import { calendarGrid } from '../utils/todoSchedule'
import type { Resource } from '../api/tauri'

/**
 * 布局编辑器 / 形态浮层里的「模块预览」：非 live 模块（clock / weather 在画布内挂真实卡片）
 * 的结构化缩印，DOM 与类结构逐一对齐各张真实卡片（SysMonitorCard / StickyCard /
 * NotesOverviewCard / TodoOverviewCard / ResourcesOverviewCard / CountdownCard /
 * PromptBoxCard / TodoCard / RecentBar / WeatherCard / ClockCard），视觉参数照抄真实值，
 * 统一乘以一个缩放系数 `--dp-k`（= 编辑器每列像素 ÷ 真实工作台每列像素），
 * 于是预览就是「真实工作台那张卡的等比缩印」，而不是重新设计的一套迷你样式。
 *
 * - 数据全部来自 useDashPreviewData 的共享 computed：派生（过滤 + 排序）只做一次，
 *   九个格子共用，且口径逐张对齐真实卡片——预览数字/顺序与真卡不符会被当成新 bug。
 * - 无数据时按真实卡片空态呈现（真实卡片本来就是那样，缩印不该粉饰）。
 * - 列表按软上限渲染 + 容器 overflow hidden：格子装得下几行就显几行，装不下即裁切——
 *   与真实卡片同样的溢出行为，正是「这块格子够不够用」最诚实的表达。
 * - 纯展示：外层 .le-pv / .le-pop-thumb-in 已 pointer-events:none。
 */

const props = defineProps<{
  modId: string
  variant?: string
  /** 自定义标题（缺省 = 模块内置标题），缩印要跟真实卡片一致 */
  title?: string
  /** 关闭标题行：缩印里同样不渲染标题 */
  hideTitle?: boolean
}>()

const {
  previewDate,
  timeText,
  timeHM,
  secText,
  dateText,
  lunar,
  quoteText,
  relTime,
  fmtRemain,
  weather,
  weatherDesc,
  tempText,
  weatherLabel,
  cityText,
  weatherSubText,
  cpuPct,
  memPct,
  memLabel,
  stickyText,
  notesCount,
  tagCount,
  latestNote,
  latestTitle,
  todoTotal,
  todoDone,
  todoRate,
  todoTodayAdded,
  resCount,
  countdownList,
  snippetList,
  todoGroups,
  todoDayMarks,
  pendingCount,
  doneCount,
  recentList,
  sudaCustomOf,
} = dashPreviewData

// ---- 天气图标映射（与 ClockCard / WeatherCard 同一分支表）----
const weatherIconComp = computed(() => {
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

// ---- 各卡的静态映射表（组件私有，无计算成本）----
const MODE_ICON: Record<string, unknown> = { once: Timer, daily: AlarmClock, interval: Repeat }
const MODE_LABEL: Record<string, string> = { once: '一次性', daily: '每天', interval: '间隔' }
const BADGE_ICON: Record<string, unknown> = {
  over: AlertTriangle,
  today: Sun,
  tmr: CalendarDays,
  date: CalendarDays,
}
/** 优先级圆点底色与 TodoRow 同源（default 灰 / yellow-soft 重要 / red-soft 紧急） */
const PRI_BG = ['var(--todo-pri-default)', 'var(--c-yellow-soft)', 'var(--c-red-soft)']
function priBg(p: number): string {
  return PRI_BG[p] ?? PRI_BG[0]
}

// ---- 最近使用：图标可用性（文件被外部删除时回退色块首字母，不留破图）----
const failedIcons = ref(new Set<number>())
function recentImg(r: Resource): string {
  return r.icon && isImageIcon(r.icon) && !failedIcons.value.has(r.id) ? iconSrc(r.icon) : ''
}
function recentAccent(r: Resource) {
  return accentOf(r.name)
}
function recentInitial(r: Resource): string {
  return r.name.charAt(0).toUpperCase()
}
function fileIconOf(r: Resource) {
  return CATEGORY_ICONS[(r.category ?? '其他') as keyof typeof CATEGORY_ICONS] ?? FileIcon
}

// ---- 自定义速达槽位（suda1..4）：取数走 dashPreviewData.sudaCustomOf（过滤/排序与真卡共用
// utils/sudaCustom 单一实现）；缩印软上限 24 项防超大 DOM——真卡列表可滚动，缩印保持
// overflow hidden 裁切（裁切本身即「格子不够大」的诚实信号，约定 27）----
const sudaCustom = computed(() => {
  const { cfg, items, configured } = sudaCustomOf(props.modId)
  return { cfg, items: items.slice(0, 24), total: items.length, configured }
})
function sudaAccent(r: Resource) {
  return accentFor(r)
}
function sudaImg(r: Resource): string {
  return r.icon && isImageIcon(r.icon) && !failedIcons.value.has(r.id) ? iconSrc(r.icon) : ''
}

// ---- 模块分发 ----
const stickySlot = computed(() => (props.modId === 'sticky2' ? 2 : 1))
// 缩印里的模块名：一律取模块注册表标题（扩展 = manifest.name），不要用 ext: 后面的 id
const extName = computed(() => dashModuleTitle(props.modId))
/** 日历模块缩印：当月 6×7 网格 + 有截止的待办分布。
 *  网格与标记都走与真卡同源的派生数据（previewDate 由分钟 tick 推进、
 *  todoDayMarks 是全量口径），缩印里不另算一套。 */
const calCells = computed(() => calendarGrid(previewDate.value, previewDate.value))
const calMarked = todoDayMarks
const kind = computed(() => {
  const id = props.modId
  if (id.startsWith('ext:')) return 'ext'
  if (id === 'sticky1' || id === 'sticky2') return 'sticky'
  if ((SUDA_CUSTOM_IDS as readonly string[]).includes(id)) return 'sudaCustom'
  return id
})
</script>

<template>
  <div class="dpv">
    <!-- ===== 时钟（静态缩印，画布内用的是真实 ClockCard） ===== -->
    <template v-if="kind === 'clock'">
      <div v-if="variant === 'lunar'" class="clock-lunar">
        <div class="lunar-time">{{ timeHM }}</div>
        <div class="lunar-solar">{{ dateText }}</div>
        <div v-if="lunar" class="lunar-date">{{ lunar.monthName }}{{ lunar.dayName }}</div>
        <hr v-if="lunar" class="lunar-div" />
        <div v-if="lunar" class="lunar-extra">{{ lunar.ganZhiYear }}年 · 属{{ lunar.zodiac }}</div>
      </div>
      <div v-else-if="variant === 'minimal'" class="clock-mini">
        <div class="mini-time">{{ timeHM }}<span class="mini-sec">{{ secText }}</span></div>
      </div>
      <div v-else class="clock-big">
        <div class="clock-top">
          <div class="clock-main">
            <div class="clock-time">{{ timeText }}</div>
            <div class="clock-date">{{ dateText }}</div>
          </div>
          <div v-if="weather" class="clock-weather">
            <component :is="weatherIconComp" class="cw-icon" />
            <div class="cw-temp">{{ tempText }}</div>
            <div class="cw-label">{{ weatherSubText }}</div>
          </div>
        </div>
        <div class="clock-quote">
          <Quote class="cq-icon" />
          <span>{{ quoteText }}</span>
        </div>
      </div>
    </template>

    <!-- ===== 天气（静态缩印，画布内用的是真实 WeatherCard） ===== -->
    <template v-else-if="kind === 'weather'">
      <div v-if="variant === 'detail'" class="w-detail">
        <div class="wd-head">
          <component :is="weatherIconComp" class="wd-ic" />
          <div class="wd-title">
            <span class="wd-temp">{{ tempText }}</span>
            <span class="wd-desc">{{ weatherLabel }}</span>
          </div>
          <span v-if="cityText" class="wd-city">{{ cityText }}</span>
        </div>
        <div class="wd-grid">
          <div class="wd-item">
            <span class="k">体感温度</span>
            <span class="v">{{ weather ? Math.round(weather.apparent_temperature) + '°' : '--' }}</span>
          </div>
          <div class="wd-item">
            <span class="k">相对湿度</span>
            <span class="v">{{ weather ? weather.relative_humidity + '%' : '--' }}</span>
          </div>
          <div class="wd-item">
            <span class="k">风速</span>
            <span class="v">{{ weather ? weather.wind_speed + ' km/h' : '--' }}</span>
          </div>
          <div class="wd-item">
            <span class="k">天气状况</span>
            <span class="v">{{ weatherLabel }}</span>
          </div>
        </div>
      </div>
      <div v-else class="w-now">
        <component :is="weatherIconComp" class="w-ic" />
        <span class="w-temp">{{ tempText }}</span>
        <div class="w-meta">
          <span class="w-desc">{{ weatherLabel }}</span>
          <span v-if="cityText" class="w-city">{{ cityText }}</span>
        </div>
      </div>
    </template>

    <!-- ===== 系统资源 ===== -->
    <template v-else-if="kind === 'sysmon'">
      <header class="hd" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Cpu class="ic" /><span>{{ title ?? '系统资源' }}</span></h3>
        <span class="sm-live-dot"></span>
      </header>
      <div class="sm-body">
        <div class="sm-item">
          <div class="sm-item-top">
            <span class="sm-item-name"><Cpu class="ic-sm" />CPU</span>
            <span class="sm-item-value">{{ cpuPct }}<em>%</em></span>
          </div>
          <div class="sm-bar"><i :style="{ transform: `scaleX(${cpuPct / 100})` }"></i></div>
        </div>
        <div class="sm-item">
          <div class="sm-item-top">
            <span class="sm-item-name"><MemoryStick class="ic-sm" />内存</span>
            <span class="sm-item-value">{{ memPct }}<em>%</em></span>
          </div>
          <div class="sm-bar"><i :style="{ transform: `scaleX(${memPct / 100})` }"></i></div>
          <p class="sm-mem-label">{{ memLabel }}</p>
        </div>
      </div>
    </template>

    <!-- ===== 便签 ===== -->
    <template v-else-if="kind === 'sticky'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><StickyNote class="ic" /><span>{{ title ?? '便签' }}</span></h3>
        <span class="hd-btn"><PanelTopClose class="ic" /></span>
      </header>
      <div class="sticky-input">
        <span v-if="stickyText[stickySlot]" class="sticky-text">{{ stickyText[stickySlot] }}</span>
        <span v-else class="sticky-ph">随手记…</span>
      </div>
    </template>

    <!-- ===== 速记概览 ===== -->
    <template v-else-if="kind === 'notes'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><FileText class="ic" /><span>{{ title ?? '速记统计' }}</span></h3>
        <span class="hd-btn"><ArrowRight class="ic" /></span>
      </header>
      <template v-if="notesCount > 0">
        <div class="ov-metrics cols-2">
          <div class="ov-metric">
            <span class="ov-value">{{ notesCount }}</span>
            <span class="ov-label">笔记</span>
          </div>
          <div class="ov-metric">
            <span class="ov-value">{{ tagCount }}</span>
            <span class="ov-label">标签</span>
          </div>
        </div>
        <div class="no-latest">
          <span class="no-latest-label">最近编辑</span>
          <p class="no-latest-title">{{ latestTitle }}</p>
          <span v-if="latestNote" class="no-latest-time">{{ relTime(latestNote.updated_at) }}</span>
        </div>
      </template>
      <div v-else class="empty">
        <p class="empty-title">还没有速记</p>
        <p class="empty-sub">新建速记后会在这里展示统计</p>
      </div>
    </template>

    <!-- ===== 待办概览 ===== -->
    <template v-else-if="kind === 'todo_overview'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><ListTodo class="ic" /><span>{{ title ?? '待办概览' }}</span></h3>
        <span class="hd-btn"><ArrowRight class="ic" /></span>
      </header>
      <template v-if="todoTotal > 0">
        <div class="to-rate">
          <div class="to-ring" :style="{ '--rate': todoRate * 3.6 + 'deg' }">
            <div class="to-ring-inner">
              <span class="to-rate-value">{{ todoRate }}%</span>
              <span class="to-rate-label">完成率</span>
            </div>
          </div>
          <p class="to-rate-note">{{ todoTotal - todoDone > 0 ? `还有 ${todoTotal - todoDone} 项待完成` : '今日待办已清空' }}</p>
        </div>
        <div class="ov-metrics cols-3">
          <div class="ov-metric"><span class="ov-value2">{{ todoTotal }}</span><span class="ov-label">总待办</span></div>
          <div class="ov-metric"><span class="ov-value2">{{ todoTodayAdded }}</span><span class="ov-label">今日新增</span></div>
          <div class="ov-metric"><span class="ov-value2">{{ todoDone }}</span><span class="ov-label">已完成</span></div>
        </div>
      </template>
      <div v-else class="empty">
        <p class="empty-title">还没有待办</p>
        <p class="empty-sub">添加待办后会在这里展示概览</p>
      </div>
    </template>

    <!-- ===== 速达数量 ===== -->
    <template v-else-if="kind === 'resources'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><FolderOpen class="ic" /><span>{{ title ?? '速达' }}</span></h3>
        <span class="hd-btn"><ArrowRight class="ic" /></span>
      </header>
      <template v-if="resCount.total > 0">
        <div class="ro-total">
          <span class="ro-total-value">{{ resCount.total }}</span>
          <span class="ro-total-suffix">个资源</span>
        </div>
        <div class="ov-metrics cols-3">
          <div class="ov-metric"><span class="ov-value2">{{ resCount.app }}</span><span class="ov-label">应用</span></div>
          <div class="ov-metric"><span class="ov-value2">{{ resCount.web }}</span><span class="ov-label">网页</span></div>
          <div class="ov-metric"><span class="ov-value2">{{ resCount.file }}</span><span class="ov-label">文件</span></div>
        </div>
        <div class="ro-split">
          <i :style="{ flex: resCount.app || 0 }"></i>
          <i :style="{ flex: resCount.web || 0 }"></i>
          <i :style="{ flex: resCount.file || 0 }"></i>
        </div>
      </template>
      <div v-else class="empty">
        <p class="empty-title">还没有速达资源</p>
        <p class="empty-sub">添加应用 / 网页 / 文件后会在这里展示数量</p>
      </div>
    </template>

    <!-- ===== 倒计时 ===== -->
    <template v-else-if="kind === 'countdown'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Timer class="ic" /><span>{{ title ?? '倒计时' }}</span></h3>
        <span class="hd-btn"><Plus class="ic" /></span>
      </header>
      <div v-if="countdownList.length" class="cc-list">
        <div
          v-for="c in countdownList"
          :key="c.id"
          class="cc-item"
          :class="{ paused: c.paused && !c.finished, finished: c.finished }"
        >
          <div class="cc-mode" :class="c.repeat_mode">
            <component :is="MODE_ICON[c.repeat_mode] || Timer" class="ic-md" />
          </div>
          <div class="cc-main">
            <div class="cc-top">
              <span class="cc-name">{{ c.name }}</span>
              <span class="cc-badge" :class="c.repeat_mode">{{ c.finished ? '已结束' : (MODE_LABEL[c.repeat_mode] ?? '一次性') }}</span>
            </div>
            <div class="cc-meta">
              <span class="cc-remaining">{{ c.finished ? '00:00' : fmtRemain(c) }}</span>
              <span class="cc-due">{{ c.repeat_mode === 'daily' ? '每天' : c.repeat_mode === 'interval' ? `每 ${c.interval_minutes ?? 0} 分钟` : '' }}</span>
            </div>
          </div>
        </div>
      </div>
      <div v-else class="empty">
        <Clock class="empty-ic" />
        <p class="empty-title">还没有倒计时</p>
        <p class="empty-sub">点「新建」添加一个，支持时长 / 定时 / 每天 / 间隔</p>
      </div>
    </template>

    <!-- ===== 提示词 ===== -->
    <template v-else-if="kind === 'prompts'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Boxes class="ic" /><span>{{ title ?? '提示词' }}</span></h3>
        <div class="hd-actions">
          <span class="hd-btn"><Settings2 class="ic" /></span>
          <span class="hd-btn"><PanelTopClose class="ic" /></span>
        </div>
      </header>
      <div v-if="snippetList.length" class="pb-body">
        <div v-for="s in snippetList" :key="s.id" class="pb-row">
          <span class="pb-row-title">
            <span class="pb-row-text">{{ s.title }}</span>
            <Pin v-if="s.is_pinned" class="pb-pin" />
          </span>
          <span class="pb-row-preview">{{ s.content }}</span>
        </div>
      </div>
      <div v-else class="empty">
        <p class="empty-title">暂无提示词</p>
        <p class="empty-sub">常用的提示词片段都在这里</p>
      </div>
    </template>

    <!-- ===== 待办 ===== -->
    <template v-else-if="kind === 'todo'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><ListTodo class="ic" /><span>{{ title ?? '待办' }}</span></h3>
        <div class="hd-actions">
          <span class="hd-btn"><PanelTopClose class="ic" /></span>
          <span class="seg on">待办 {{ pendingCount }}</span>
          <span class="seg">已完成 {{ doneCount }}</span>
        </div>
      </header>
      <div class="todo-add">
        <span class="todo-ph">添加待办，回车确认</span>
      </div>
      <div v-if="todoGroups.length" class="todo-body">
        <div v-for="g in todoGroups" :key="g.label" class="todo-group">
          <div class="todo-group-head">
            <span class="glabel">{{ g.label }}</span>
            <span class="gline"></span>
            <span class="gcount">{{ g.total }}</span>
          </div>
          <div v-for="t in g.items" :key="t.id" class="todo-row">
            <span class="todo-check"></span>
            <span class="todo-pri" :style="{ background: priBg(t.priority) }"></span>
            <span class="todo-label">{{ t.title }}</span>
            <span v-if="t.badge" class="todo-badge" :class="t.badge.kind">
              <component :is="BADGE_ICON[t.badge.kind]" class="ic-xs" />{{ t.badge.text }}
            </span>
          </div>
        </div>
      </div>
      <div v-else class="empty">
        <p class="empty-title">今天要做什么？</p>
        <p class="empty-sub">按回车快速添加</p>
      </div>
    </template>

    <!-- ===== 日历（待办分布） ===== -->
    <template v-else-if="kind === 'calendar'">
      <h3 v-if="!hideTitle" class="hd-title">
        <CalendarDays class="ic" /><span>{{ title ?? '日历' }}</span>
      </h3>
      <div class="cal-mini">
        <div v-for="d in ['一', '二', '三', '四', '五', '六', '日']" :key="d" class="cal-dow">{{ d }}</div>
        <div v-for="c in calCells" :key="c.key" class="cal-cell" :class="{ out: c.out, today: c.today }">
          <span class="cal-day">{{ c.day }}</span>
          <i v-if="calMarked.get(c.key)" class="cal-dot" :title="`${calMarked.get(c.key)} 条待办`"></i>
        </div>
      </div>
    </template>

    <!-- ===== 最近使用 ===== -->
    <template v-else-if="kind === 'recent'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Flame class="ic" /><span>{{ title ?? '最近使用' }}</span></h3>
        <span class="hd-btn"><ArrowRight class="ic" /></span>
      </header>
      <div v-if="recentList.length" class="rb-body">
        <div v-for="r in recentList" :key="r.id" class="rb-card">
          <span class="rb-icon" :style="r.icon && isImageIcon(r.icon) ? undefined : { background: recentAccent(r).soft, color: recentAccent(r).text }">
            <img v-if="recentImg(r)" :src="recentImg(r)" class="rb-img" alt="" @error="failedIcons.add(r.id)" />
            <Globe v-else-if="r.kind === 'web'" class="ic-lg" style="color: var(--c-green-ink)" />
            <component :is="fileIconOf(r)" v-else-if="r.kind === 'file'" class="ic-lg" style="color: var(--text-2)" />
            <span v-else class="rb-initial">{{ recentInitial(r) }}</span>
          </span>
          <span class="rb-name">{{ r.name }}</span>
        </div>
      </div>
      <div v-else class="empty">
        <p class="empty-title">暂无最近使用</p>
        <p class="empty-sub">启动过的项目会出现在这里</p>
      </div>
    </template>

    <!-- ===== 自定义速达（suda1..4）：快捷启动网格，结构/尺寸照抄真卡 SudaCustomCard ===== -->
    <template v-else-if="kind === 'sudaCustom'">
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Boxes class="ic" /><span>{{ title ?? '自定义速达' }}</span></h3>
        <span class="hd-btn"><Settings2 class="ic" /></span>
      </header>
      <div v-if="sudaCustom.configured && sudaCustom.items.length" class="scc-grid">
        <div v-for="r in sudaCustom.items" :key="r.id" class="scc-item">
          <span class="scc-icon" :style="sudaImg(r) ? {} : { background: sudaAccent(r).soft }">
            <img v-if="sudaImg(r)" :src="sudaImg(r)" class="scc-img" alt="" @error="failedIcons.add(r.id)" />
            <Globe v-else-if="r.kind === 'web'" class="scc-lg" style="color: var(--c-green-ink)" />
            <component
              :is="fileIconOf(r)"
              v-else-if="r.kind === 'file'"
              class="scc-lg"
              :style="{ color: sudaAccent(r).strong }"
            />
            <span v-else class="scc-letter" :style="{ color: sudaAccent(r).ink }">{{
              r.name.charAt(0).toUpperCase()
            }}</span>
          </span>
          <span class="scc-name">{{ r.name }}</span>
        </div>
      </div>
      <div v-else-if="sudaCustom.configured" class="empty">
        <p class="empty-title">这个来源还没有资源</p>
        <p class="empty-sub">先在速达里添加，或点设置换个来源</p>
      </div>
      <div v-else class="empty">
        <p class="empty-title">还没配置内容</p>
        <p class="empty-sub">点卡片右上角设置，挑选内容放进来</p>
      </div>
    </template>

    <!-- ===== 扩展模块 / 未知模块：骨架示意 ===== -->
    <template v-else>
      <header class="hd hd-split" :class="{ 'hd-float': hideTitle }">
        <h3 v-if="!hideTitle" class="hd-title"><Puzzle class="ic" /><span>{{ title ?? extName }}</span></h3>
      </header>
      <div class="sk-lines">
        <i></i><i style="width: 74%"></i><i style="width: 52%"></i>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* 缩放系数：--dp-k 由调用方（布局编辑器）写入 = 编辑器每列像素 ÷ 真实工作台每列像素。
 * 所有尺寸一律取真实卡片的原值 × --u（1 真实 px 的缩印长度），因此预览就是真实卡片的缩印。
 * 兜底 0.78：编辑器脱离画布单独渲染（形态浮层）时也有合理比例。 */
.dpv {
  --k: var(--dp-k, 0.78);
  --u: calc(1px * var(--k));
  --todo-pri-default: #c6cad4;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: calc(12 * var(--u));
  border-radius: var(--radius-lg);
  overflow: hidden;
  font-size: calc(13 * var(--u));
  line-height: 1.45;
  color: var(--text-1);
}
/* 日历模块缩印：月历网格 + 有待办的日子点一个小点 */
.cal-mini {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  grid-auto-rows: minmax(0, 1fr);
  gap: calc(2 * var(--u));
}
.cal-dow {
  font-size: calc(9 * var(--u));
  font-weight: 700;
  color: var(--text-4);
  text-align: center;
}
.cal-cell {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-start;
  gap: calc(1 * var(--u));
  /* 尺寸照抄真卡 .tc-cell（padding 2px 3px / 圆角 5px），只把 px 换成 var(--u) */
  padding: calc(2 * var(--u)) calc(3 * var(--u));
  border: 1px solid var(--border-soft);
  border-radius: calc(5 * var(--u));
  background: var(--bg-card-soft);
  overflow: hidden;
}
.cal-cell.out {
  opacity: 0.4;
}
.cal-cell.today {
  border-color: var(--brand-500);
}
.cal-day {
  font-size: calc(9 * var(--u));
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.cal-dot {
  width: calc(4 * var(--u));
  height: calc(4 * var(--u));
  border-radius: 50%;
  background: var(--brand-500);
}
html[data-theme='dark'] .dpv {
  --todo-pri-default: #52525f;
}

/* ---- 卡头（真实卡片统一模式：13px/600 + 14px 品牌色图标 + 右侧 26~28px 钮） ---- */
.hd {
  display: flex;
  align-items: center;
  gap: calc(8 * var(--u));
  margin-bottom: calc(8 * var(--u));
  flex-shrink: 0;
}
.hd-split {
  justify-content: space-between;
}
/* 关闭标题：表头整条不占位，动作按钮悬浮右上角（与真实卡片同款 .hd-float 规则） */
.hd-title {
  display: flex;
  align-items: center;
  gap: calc(6 * var(--u));
  margin: 0;
  font-size: calc(13 * var(--u));
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-1);
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.hd-actions {
  display: flex;
  align-items: center;
  gap: calc(6 * var(--u));
  flex-shrink: 0;
}
.hd-btn {
  width: calc(26 * var(--u));
  height: calc(26 * var(--u));
  border-radius: var(--radius-sm);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  flex-shrink: 0;
}
.ic {
  width: calc(14 * var(--u));
  height: calc(14 * var(--u));
  stroke-width: 2;
  color: var(--brand-500);
  flex-shrink: 0;
}
.ic-sm {
  width: calc(12 * var(--u));
  height: calc(12 * var(--u));
  stroke-width: 2;
}
.ic-xs {
  width: calc(10 * var(--u));
  height: calc(10 * var(--u));
  stroke-width: 2;
}
.ic-md {
  width: calc(15 * var(--u));
  height: calc(15 * var(--u));
  stroke-width: 2;
}
.ic-lg {
  width: calc(24 * var(--u));
  height: calc(24 * var(--u));
  stroke-width: 1.7;
}

/* ---- 空态（与真实卡片同文案样式） ---- */
.empty {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: calc(6 * var(--u));
  text-align: center;
}
.empty-ic {
  width: calc(16 * var(--u));
  height: calc(16 * var(--u));
  color: var(--text-4);
}
.empty-title {
  margin: 0;
  font-size: calc(13 * var(--u));
  font-weight: 600;
  color: var(--text-2);
}
.empty-sub {
  margin: 0;
  font-size: calc(11 * var(--u));
  color: var(--text-4);
}

/* ---- 时钟：big ---- */
.clock-big,
.clock-lunar {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: calc(6 * var(--u));
}
.clock-top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: calc(10 * var(--u));
}
.clock-main {
  display: flex;
  flex-direction: column;
  gap: calc(4 * var(--u));
  min-width: 0;
  flex: 1;
}
.clock-time {
  font-size: calc(30 * var(--u));
  font-weight: 700;
  line-height: 1.05;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
}
.clock-date {
  font-size: calc(13 * var(--u));
  font-weight: 500;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
}
.clock-weather {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: calc(2 * var(--u));
  flex-shrink: 0;
}
.cw-icon {
  width: calc(22 * var(--u));
  height: calc(22 * var(--u));
  color: var(--brand-500);
}
.cw-temp {
  font-size: calc(20 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.cw-label {
  font-size: calc(11 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
  max-width: calc(110 * var(--u));
  overflow: hidden;
  text-overflow: ellipsis;
}
.clock-quote {
  display: flex;
  align-items: flex-start;
  gap: calc(5 * var(--u));
  margin-top: auto;
  padding-top: calc(6 * var(--u));
  border-top: 1px solid var(--border-soft);
  font-size: calc(14 * var(--u));
  line-height: 1.5;
  font-weight: 600;
}
.cq-icon {
  width: calc(13 * var(--u));
  height: calc(13 * var(--u));
  margin-top: calc(2 * var(--u));
  flex-shrink: 0;
  color: var(--brand-500);
}
.clock-quote span {
  background: linear-gradient(100deg, var(--brand-500), #f472b6 45%, #38bdf8 80%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  color: transparent;
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

/* ---- 时钟：阴阳历 / 月历 / 极简 ---- */
.lunar-time {
  font-size: calc(30 * var(--u));
  font-weight: 700;
  line-height: 1.05;
  letter-spacing: -0.02em;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.lunar-solar {
  font-size: calc(13 * var(--u));
  font-weight: 500;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
}
.lunar-date {
  margin-top: auto;
  font-size: calc(19 * var(--u));
  font-weight: 700;
  color: var(--brand-500);
  white-space: nowrap;
}
.lunar-div {
  border: none;
  border-top: 1px solid var(--border-soft);
  margin: calc(4 * var(--u)) 0 0;
}
.lunar-extra {
  font-size: calc(12 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
}
.clock-mini {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.mini-time {
  display: flex;
  align-items: baseline;
  gap: calc(4 * var(--u));
  font-size: calc(34 * var(--u));
  font-weight: 700;
  letter-spacing: -0.03em;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.mini-sec {
  font-size: calc(18 * var(--u));
  font-weight: 600;
  color: var(--brand-500);
}

/* ---- 天气 ---- */
.w-now {
  height: 100%;
  display: flex;
  align-items: center;
  gap: calc(10 * var(--u));
  min-width: 0;
}
.w-ic {
  width: calc(26 * var(--u));
  height: calc(26 * var(--u));
  flex-shrink: 0;
  color: var(--brand-500);
}
.w-temp {
  font-size: calc(24 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.w-meta {
  display: flex;
  flex-direction: column;
  gap: calc(2 * var(--u));
  min-width: 0;
}
.w-desc {
  font-size: calc(13 * var(--u));
  font-weight: 600;
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.w-city {
  font-size: calc(11 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.w-detail {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: calc(8 * var(--u));
}
.wd-head {
  display: flex;
  align-items: center;
  gap: calc(8 * var(--u));
  min-width: 0;
}
.wd-ic {
  width: calc(22 * var(--u));
  height: calc(22 * var(--u));
  color: var(--brand-500);
  flex-shrink: 0;
}
.wd-title {
  display: flex;
  align-items: baseline;
  gap: calc(6 * var(--u));
  min-width: 0;
}
.wd-temp {
  font-size: calc(26 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.wd-desc {
  font-size: calc(12 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
}
.wd-city {
  margin-left: auto;
  font-size: calc(11 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
}
.wd-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 1fr 1fr;
  grid-auto-rows: 1fr;
  gap: calc(6 * var(--u));
}
.wd-item {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: calc(2 * var(--u));
  padding: calc(8 * var(--u));
  background: var(--bg-card-soft);
  border-radius: calc(8 * var(--u));
  min-width: 0;
  overflow: hidden;
}
.wd-item .k {
  font-size: calc(10 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
}
.wd-item .v {
  font-size: calc(15 * var(--u));
  font-weight: 700;
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---- 系统资源 ---- */
.sm-live-dot {
  width: calc(6 * var(--u));
  height: calc(6 * var(--u));
  border-radius: var(--radius-pill);
  background: var(--c-green);
  flex-shrink: 0;
}
.sm-body {
  display: flex;
  flex-direction: column;
  gap: calc(8 * var(--u));
}
.sm-item {
  display: flex;
  flex-direction: column;
  gap: calc(5 * var(--u));
}
.sm-item-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: calc(6 * var(--u));
}
.sm-item-name {
  display: inline-flex;
  align-items: center;
  gap: calc(5 * var(--u));
  font-size: calc(12 * var(--u));
  font-weight: 600;
  color: var(--text-2);
}
.sm-item-name .ic-sm {
  color: var(--text-3);
}
.sm-item-value {
  font-size: calc(15 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}
.sm-item-value em {
  font-size: calc(10 * var(--u));
  font-weight: 500;
  font-style: normal;
  color: var(--text-3);
  margin-left: calc(2 * var(--u));
}
.sm-bar {
  height: calc(8 * var(--u));
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  overflow: hidden;
}
.sm-bar i {
  display: block;
  height: 100%;
  width: 100%;
  border-radius: var(--radius-pill);
  background: linear-gradient(90deg, var(--brand-600), var(--brand-500));
  transform-origin: left center;
}
.sm-mem-label {
  margin: 0;
  font-size: calc(10 * var(--u));
  line-height: 1.2;
  color: var(--text-3);
}

/* ---- 便签 ---- */
.sticky-input {
  flex: 1;
  min-height: 0;
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  padding: calc(8 * var(--u));
  font-size: calc(12 * var(--u));
  line-height: 1.6;
  color: var(--text-2);
  overflow: hidden;
  white-space: pre-wrap;
  word-break: break-word;
}
.sticky-ph {
  color: var(--text-4);
}

/* ---- 概览三兄弟（速记 / 待办 / 速达）共用统计块 ---- */
.ov-metrics {
  display: grid;
  gap: calc(8 * var(--u));
  margin-bottom: calc(12 * var(--u));
}
.ov-metrics.cols-2 {
  grid-template-columns: repeat(2, 1fr);
}
.ov-metrics.cols-3 {
  grid-template-columns: repeat(3, 1fr);
  margin-bottom: 0;
}
.ov-metric {
  display: flex;
  flex-direction: column;
  gap: calc(2 * var(--u));
  padding: calc(10 * var(--u)) calc(8 * var(--u));
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  min-width: 0;
}
.ov-value {
  font-size: calc(22 * var(--u));
  font-weight: 700;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}
.ov-value2 {
  font-size: calc(18 * var(--u));
  font-weight: 700;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}
.ov-label {
  font-size: calc(11 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
}
.no-latest {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: calc(4 * var(--u));
  padding: calc(10 * var(--u));
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.no-latest-label {
  font-size: calc(11 * var(--u));
  color: var(--text-4);
}
.no-latest-title {
  margin: 0;
  font-size: calc(13 * var(--u));
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.no-latest-time {
  font-size: calc(11 * var(--u));
  color: var(--text-4);
}
.to-rate {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: calc(10 * var(--u));
  padding: calc(4 * var(--u)) 0 calc(10 * var(--u));
}
.to-ring {
  width: calc(84 * var(--u));
  height: calc(84 * var(--u));
  border-radius: 50%;
  background: conic-gradient(var(--brand-500) var(--rate, 0deg), var(--bg-card-soft) 0);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.to-ring-inner {
  width: calc(62 * var(--u));
  height: calc(62 * var(--u));
  border-radius: 50%;
  background: var(--bg-card-solid);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: calc(1 * var(--u));
}
.to-rate-value {
  font-size: calc(18 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}
.to-rate-label {
  font-size: calc(10 * var(--u));
  color: var(--text-4);
}
.to-rate-note {
  margin: 0;
  font-size: calc(12 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
}
.ro-total {
  display: flex;
  align-items: baseline;
  gap: calc(6 * var(--u));
  margin-bottom: calc(10 * var(--u));
}
.ro-total-value {
  font-size: calc(30 * var(--u));
  font-weight: 700;
  line-height: 1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.03em;
}
.ro-total-suffix {
  font-size: calc(12 * var(--u));
  color: var(--text-4);
}
.ro-split {
  display: flex;
  gap: calc(3 * var(--u));
  height: calc(6 * var(--u));
  margin-top: calc(12 * var(--u));
  border-radius: var(--radius-pill);
  overflow: hidden;
}
.ro-split i {
  border-radius: var(--radius-pill);
  background: var(--brand-500);
  opacity: 0.35;
}
.ro-split i:nth-child(2) {
  opacity: 0.7;
}
.ro-split i:nth-child(3) {
  opacity: 1;
}

/* ---- 倒计时 ---- */
.cc-list {
  /* 与真卡 CountdownCard 同口径：撑满 + 行等分（1fr），格子高度变化时条目跟着变 */
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-auto-rows: minmax(calc(48 * var(--u)), 1fr);
  gap: calc(8 * var(--u));
  align-content: stretch;
  overflow: hidden;
}
.cc-item {
  display: flex;
  align-items: center;
  gap: calc(10 * var(--u));
  min-height: calc(48 * var(--u));
  padding: calc(8 * var(--u)) calc(10 * var(--u));
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  box-shadow: var(--shadow-item);
  min-width: 0;
  overflow: hidden;
}
.cc-item.paused {
  opacity: 0.62;
}
.cc-item.finished {
  opacity: 0.7;
}
.cc-mode {
  width: calc(32 * var(--u));
  height: calc(32 * var(--u));
  border-radius: 50%;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--accent) 14%, var(--bg-card-solid));
  color: var(--brand-600);
  border: 1px solid var(--border-soft);
}
.cc-mode.interval {
  background: color-mix(in srgb, var(--c-green) 14%, var(--bg-card-solid));
  color: var(--c-green-ink);
}
.cc-main {
  min-width: 0;
  flex: 1;
}
.cc-top {
  display: flex;
  align-items: center;
  gap: calc(6 * var(--u));
  min-width: 0;
}
.cc-name {
  font-size: calc(13 * var(--u));
  font-weight: 600;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.cc-badge {
  font-size: calc(10 * var(--u));
  line-height: calc(14 * var(--u));
  padding: 0 calc(7 * var(--u));
  border-radius: var(--radius-pill);
  background: var(--bg-card-solid);
  color: var(--text-3);
  border: 1px solid var(--border-soft);
  flex-shrink: 0;
}
.cc-badge.daily {
  background: color-mix(in srgb, var(--accent) 14%, var(--bg-card-solid));
  color: var(--brand-600);
  border-color: transparent;
}
.cc-badge.interval {
  background: color-mix(in srgb, var(--c-green) 14%, var(--bg-card-solid));
  color: var(--c-green-ink);
  border-color: transparent;
}
.cc-meta {
  display: flex;
  align-items: baseline;
  gap: calc(8 * var(--u));
  margin-top: calc(2 * var(--u));
  white-space: nowrap;
  overflow: hidden;
}
.cc-remaining {
  font-size: calc(13 * var(--u));
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.cc-due {
  font-size: calc(11 * var(--u));
  color: var(--text-4);
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---- 提示词 ---- */
.pb-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: calc(6 * var(--u));
  overflow: hidden;
}
.pb-row {
  display: flex;
  flex-direction: column;
  gap: calc(2 * var(--u));
  padding: calc(8 * var(--u)) calc(10 * var(--u));
  border-radius: var(--radius-sm);
  min-width: 0;
}
.pb-row-title {
  display: flex;
  align-items: center;
  gap: calc(4 * var(--u));
  min-width: 0;
}
.pb-row-text {
  font-size: calc(13 * var(--u));
  font-weight: 600;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.pb-pin {
  width: calc(12 * var(--u));
  height: calc(12 * var(--u));
  color: var(--brand-500);
  flex-shrink: 0;
}
.pb-row-preview {
  font-size: calc(12 * var(--u));
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---- 待办 ---- */
.seg {
  font-size: calc(11 * var(--u));
  font-weight: 500;
  color: var(--text-3);
  background: var(--bg-card-solid);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  padding: calc(3 * var(--u)) calc(8 * var(--u));
  white-space: nowrap;
}
.seg.on {
  background: var(--brand-500);
  border-color: transparent;
  color: var(--text-on-accent);
  font-weight: 600;
}
.todo-add {
  border: 1px solid var(--border-soft);
  background: var(--input-bg);
  border-radius: var(--radius-md);
  padding: calc(7 * var(--u)) calc(10 * var(--u));
  font-size: calc(13 * var(--u));
  line-height: 1.45;
  flex-shrink: 0;
}
.todo-ph {
  color: var(--text-4);
}
.todo-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
.todo-group {
  margin-top: calc(12 * var(--u));
}
.todo-group:first-child {
  margin-top: 0;
}
.todo-group-head {
  display: flex;
  align-items: center;
  gap: calc(8 * var(--u));
  padding: 0 calc(6 * var(--u)) calc(4 * var(--u));
}
.glabel {
  font-size: calc(10.5 * var(--u));
  font-weight: 600;
  color: var(--text-3);
  letter-spacing: 0.02em;
  white-space: nowrap;
}
.gline {
  flex: 1;
  height: 1px;
  background: var(--border-soft);
}
.gcount {
  font-size: calc(10 * var(--u));
  color: var(--text-4);
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 0 calc(7 * var(--u));
  line-height: calc(15 * var(--u));
}
.todo-row {
  display: flex;
  align-items: flex-start;
  gap: calc(8 * var(--u));
  padding: calc(6 * var(--u));
  border-radius: var(--radius-sm);
  min-width: 0;
}
.todo-check {
  width: calc(18 * var(--u));
  height: calc(18 * var(--u));
  border: 1.5px solid var(--border-strong);
  border-radius: var(--radius-pill);
  flex-shrink: 0;
  margin-top: calc(1 * var(--u));
}
.todo-pri {
  width: calc(10 * var(--u));
  height: calc(10 * var(--u));
  border-radius: var(--radius-pill);
  flex-shrink: 0;
  margin-top: calc(5 * var(--u));
}
.todo-label {
  flex: 1;
  min-width: 0;
  font-size: calc(13 * var(--u));
  line-height: 1.45;
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.todo-badge {
  display: inline-flex;
  align-items: center;
  gap: calc(3 * var(--u));
  font-size: calc(10 * var(--u));
  font-weight: 600;
  line-height: calc(14 * var(--u));
  padding: 0 calc(8 * var(--u));
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-3);
  flex-shrink: 0;
  white-space: nowrap;
}
.todo-badge.over {
  background: var(--c-red-soft);
  color: var(--c-red-ink);
}
.todo-badge.today {
  background: var(--c-orange-soft);
  color: var(--c-orange-ink);
}
.todo-badge.tmr {
  background: var(--brand-50);
  color: var(--brand-500);
}

/* ---- 最近使用 ---- */
.rb-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-wrap: wrap;
  align-content: flex-start;
  gap: calc(10 * var(--u));
  overflow: hidden;
  align-items: flex-start;
}
.rb-card {
  width: calc(72 * var(--u));
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: calc(6 * var(--u));
  padding: calc(8 * var(--u)) calc(4 * var(--u));
  border-radius: var(--radius-md);
  min-width: 0;
}
.rb-icon {
  width: calc(42 * var(--u));
  height: calc(42 * var(--u));
  border-radius: calc(12 * var(--u));
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  flex-shrink: 0;
}
.rb-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  background: var(--bg-card);
}
.rb-initial {
  font-size: calc(17 * var(--u));
  font-weight: 700;
}
.rb-name {
  font-size: calc(11 * var(--u));
  font-weight: 500;
  color: var(--text-2);
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---- 扩展 / 未知骨架 ---- */
.sk-lines {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: calc(8 * var(--u));
}
.sk-lines i {
  height: calc(8 * var(--u));
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  display: block;
}

/* ---- 自定义速达缩印：结构/尺寸照抄真卡 SudaCustomCard（56px 列宽、34px 图标、11px 名称、6px 间距）---- */
.scc-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(calc(56 * var(--u)), 1fr));
  gap: calc(6 * var(--u));
  align-content: start;
  overflow: hidden;
}
.scc-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: calc(4 * var(--u));
  padding: calc(6 * var(--u)) calc(2 * var(--u));
  border-radius: calc(8 * var(--u));
  min-width: 0;
}
.scc-icon {
  width: calc(34 * var(--u));
  height: calc(34 * var(--u));
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: calc(8 * var(--u));
  overflow: hidden;
  flex: none;
}
.scc-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.scc-lg {
  width: calc(18 * var(--u));
  height: calc(18 * var(--u));
}
.scc-letter {
  font-size: calc(15 * var(--u));
  font-weight: 700;
  line-height: 1;
}
.scc-name {
  max-width: 100%;
  font-size: calc(11 * var(--u));
  line-height: 1.2;
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
