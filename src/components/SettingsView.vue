<script setup lang="ts">
// 设置页外壳：只负责「两级导航 + 设置项搜索 + 面板路由」，具体设置项都在 ./settings/*.vue 面板里。
//
// 为什么拆：以前是单文件（模板 + 脚本 + 样式共 3000 余行），点开设置要下载/解析整份代码，
// 于是先出现一段空白再显示内容。现在外壳 + 当前大类按需加载，切大类才取对应面板。
// 样式统一在 ./settings/shared.css（规则带 .settings-view 前缀，避免外移后污染其它视图）。
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { ChevronRight, Search, X } from 'lucide-vue-next';
import { SETTINGS_INDEX } from './settingsIndex.generated';
import {
  GROUPS,
  SECTIONS,
  SECTION_GROUP,
  SECTION_LABEL,
  type GroupId,
  type SectionId,
} from '../utils/settingsSections';
import PanelLoading from './settings/PanelLoading.vue';

const props = defineProps<{ initialSection?: string }>()

const emit = defineEmits<{
  (e: 'open-layout-editor'): void
  /** 面板里的「去扩展中心」入口（本地扩展目录的增删已挪到扩展中心「我的扩展」标签页） */
  (e: 'open-extensions'): void
}>()

// ---- 两级分类导航 ----
// 左栏列 10 个大类，当前大类的子项在它下方缩进展开；右侧**只挂载当前大类**的分区。
// 为什么不再「全量渲染 + 滚动锚点」：12 个分区一次性挂载（模板近 2000 行）会让点开设置时
// 先空白一段再出现内容。分区元数据（SECTIONS/GROUPS/SECTION_GROUP/各 label）已抽到
// utils/settingsSections.ts 单一数据源 —— GlobalSearch 的「设置」检索结果共用同一份。
const activeGroup = ref<GroupId>('general')
const activeSection = ref<SectionId>('general')
const contentRef = ref<HTMLElement | null>(null)

/**
 * 某个大类下的分区列表：**按 SECTIONS 的顺序推导**，不另写一份。
 * 这样左栏子项顺序天然等于右侧渲染顺序（写成两份迟早会有一份出错，
 * 表现为点子项时页面向反方向跳）。
 */
function sectionsOf(id: GroupId): readonly SectionId[] {
  return SECTIONS.filter((s) => SECTION_GROUP[s.id] === id).map((s) => s.id)
}

/** 该大类是否显示子项行：只挂一个分区（必与自身同名，如 外观/账号/使用说明/关于）时就是「没有子项」，点了即达，不再缩进重复一行 */
function hasKids(id: GroupId): boolean {
  return sectionsOf(id).length > 1
}

// ---- 设置项搜索 ----
// 索引是**构建期从模板生成**的（scripts/gen-settings-index.mjs）：设置页只挂载当前大类，
// 其它大类的设置项不在 DOM 里，所以搜索不能靠遍历页面。索引绑在 prebuild 上，不会漂移。
const query = ref('')

function groupLabel(id: GroupId): string {
  return GROUPS.find((g) => g.id === id)?.label ?? ''
}

const searchResults = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return [] as { section: SectionId; title: string; where: string; rank: number }[]
  const hits: { section: SectionId; title: string; where: string; rank: number }[] = []
  for (const it of SETTINGS_INDEX) {
    const section = it.section as SectionId
    if (!(section in SECTION_GROUP)) continue
    const where = `${groupLabel(SECTION_GROUP[section])} / ${SECTION_LABEL[section]}`
    const idx = it.title.toLowerCase().indexOf(q)
    const whereHit = where.toLowerCase().includes(q)
    if (idx < 0 && !whereHit) continue
    // 排序：标题开头命中 > 标题中间命中 > 只命中「所属位置」
    hits.push({ section, title: it.title, where, rank: idx === 0 ? 0 : idx > 0 ? 1 : 2 })
  }
  return hits.sort((a, b) => a.rank - b.rank || a.title.length - b.title.length).slice(0, 30)
})

/** 跳到某个设置项：切大类 → 滚到那一行 → 短暂高亮（找不到行就退化为滚到该分区） */
function jumpToSetting(section: SectionId, title: string, attempt = 0) {
  activeGroup.value = SECTION_GROUP[section]
  activeSection.value = section
  void nextTick(() => {
    const container = contentRef.value
    if (!container) return
    const hit = Array.from(
      container.querySelectorAll<HTMLElement>('.setting-name, .theme-label, .sv-subtitle, .guide-h2, .guide-h3')
    ).find((el) => el.textContent?.trim() === title)
    // 面板可能还在加载：这一轮找不到就等下一轮（保留搜索词，用户能看到进度）
    if (!hit && attempt < 20) {
      window.setTimeout(() => jumpToSetting(section, title, attempt + 1), 50)
      return
    }
    // 命中设置项 → 高亮它所在那一行；命中小组标题（sv-subtitle）/使用说明正文标题（guide-h2/h3）
    // → 就用标题本身（它们没有「行」可高亮，闪一下标题即可）
    const row = hit?.closest<HTMLElement>('.setting-row, .theme-row') ?? hit
    const target = row ?? container.querySelector<HTMLElement>(`#sv-sec-${section}`)
    if (target) {
      // 用矩形差而不是 offsetTop：使用说明的标题在 .guide-body（position: relative）里，
      // offsetTop 相对的是那个内层容器，按它滚动会落到分区顶部附近而不是目标章节
      const scrollToTarget = () => {
        const delta = target.getBoundingClientRect().top - container.getBoundingClientRect().top
        container.scrollTo({ top: Math.max(0, container.scrollTop + delta - 12), behavior: 'smooth' })
      }
      scrollToTarget()
      // 使用说明面板的正文是异步渲染的：跳转那一刻高度还没撑开，落点会算小（停在中途/顶部）。
      // 渲染完成后再校一次，只在目标确实跑偏时才补滚，避免把用户的正常阅读位置抖一下
      if (section === 'guide') {
        window.setTimeout(() => {
          const delta = target.getBoundingClientRect().top - container.getBoundingClientRect().top
          if (Math.abs(delta - 12) > 40) scrollToTarget()
        }, 400)
      }
    }
    if (row) {
      row.classList.add('setting-flash')
      window.setTimeout(() => row.classList.remove('setting-flash'), 1600)
    }
    query.value = '' // 回到导航（否则看不到定位后的上下文）
  })
}

/** 切大类：内容整体换掉 → 直接归零滚动；再点当前大类则回到该类顶部 */
function selectGroup(id: GroupId) {
  const first = sectionsOf(id)[0]
  if (activeGroup.value === id) {
    if (first) activeSection.value = first
    contentRef.value?.scrollTo({ top: 0, behavior: 'smooth' })
    return
  }
  activeGroup.value = id
  if (first) activeSection.value = first
  void nextTick(() => contentRef.value?.scrollTo({ top: 0 }))
}

/**
 * 滚到某个分区。
 * 面板按需加载：目标 DOM 可能还没挂上，所以取不到就重试几次（最多约 400ms）。
 */
function goToSection(id: SectionId, attempt = 0) {
  activeSection.value = id
  const container = contentRef.value
  if (!container) return
  const el = container.querySelector<HTMLElement>(`#sv-sec-${id}`)
  if (!el) {
    if (attempt < 20) window.setTimeout(() => goToSection(id, attempt + 1), 50)
    return
  }
  // 相对滚动容器计算目标位置（容器为 position: relative，offsetTop 相对它）
  container.scrollTo({ top: Math.max(0, el.offsetTop - 12), behavior: 'smooth' })
}

// 类内滚动时同步子项激活态（只遍历当前大类已挂载的分区，未挂载的自然取不到）
function onContentScroll() {
  const container = contentRef.value
  if (!container) return
  let current: SectionId | null = null
  for (const id of sectionsOf(activeGroup.value)) {
    const el = container.querySelector<HTMLElement>(`#sv-sec-${id}`)
    if (!el) continue
    if (el.offsetTop - 60 <= container.scrollTop) current = id
  }
  if (current && current !== activeSection.value) activeSection.value = current
}

onMounted(() => contentRef.value?.addEventListener('scroll', onContentScroll))
onBeforeUnmount(() => contentRef.value?.removeEventListener('scroll', onContentScroll))

// ---- 大类面板：按需加载 ----
// 首次打开设置只加载「外壳 + 当前大类」的代码，切到大类时才去取对应面板。
// 面板在 ./settings/ 下，样式统一在 ./settings/shared.css（规则带 .settings-view 前缀）。
const PANEL_LOADERS = {
  general: () => import('./settings/GeneralPanel.vue'),
  appearance: () => import('./settings/AppearancePanel.vue'),
  workbench: () => import('./settings/WorkbenchPanel.vue'),
  ai: () => import('./settings/AiPanel.vue'),
  features: () => import('./settings/FeaturesPanel.vue'),
  extensions: () => import('./settings/ExtensionsPanel.vue'),
  account: () => import('./settings/AccountPanel.vue'),
  data: () => import('./settings/DataPanel.vue'),
  guide: () => import('./settings/GuidePanel.vue'),
  about: () => import('./settings/AboutPanel.vue'),
} as const

/** 面板加载占位：delay 0 —— 宁可闪一下占位，也不要出现空白（见 index.vue 的同类说明） */
const withPanelPlaceholder = (loader: () => Promise<unknown>) =>
  defineAsyncComponent({
    loader: loader as () => Promise<never>,
    loadingComponent: PanelLoading,
    delay: 0,
  })

const PANELS: Record<GroupId, ReturnType<typeof defineAsyncComponent>> = {
  general: withPanelPlaceholder(PANEL_LOADERS.general),
  appearance: withPanelPlaceholder(PANEL_LOADERS.appearance),
  workbench: withPanelPlaceholder(PANEL_LOADERS.workbench),
  ai: withPanelPlaceholder(PANEL_LOADERS.ai),
  features: withPanelPlaceholder(PANEL_LOADERS.features),
  extensions: withPanelPlaceholder(PANEL_LOADERS.extensions),
  account: withPanelPlaceholder(PANEL_LOADERS.account),
  data: withPanelPlaceholder(PANEL_LOADERS.data),
  guide: withPanelPlaceholder(PANEL_LOADERS.guide),
  about: withPanelPlaceholder(PANEL_LOADERS.about),
}

/** 鼠标移到大类就先把它那份代码取下来：点下去时通常已在内存里，切大类不再等 */
function preloadPanel(id: GroupId) {
  void PANEL_LOADERS[id]()
}

const currentPanel = computed(() => PANELS[activeGroup.value])


// 深链（如 AI 对话面板的「去配置」）：先切到目标分区所属大类，再滚动定位。
// 面板是异步加载的，DOM 不一定已挂上 —— goToSection 自带重试。
onMounted(() => {
  if (!props.initialSection) return
  const target = props.initialSection as SectionId
  if (!SECTIONS.some((s) => s.id === target)) return
  activeGroup.value = SECTION_GROUP[target]
  activeSection.value = target
  void nextTick(() => goToSection(target))
})

</script>

<template>
  <div class="settings-view">
    <header class="sv-header">
      <h2 class="sv-title">设置</h2>
    </header>

    <div class="sv-body">
      <!-- 左侧分类：顶部搜索框 + 大类/子项两级导航（搜索时用结果列表替换导航） -->
      <nav class="sv-nav" aria-label="设置分类">
        <div class="sv-search">
          <Search :size="13" :stroke-width="2" />
          <input
            v-model="query"
            class="sv-search-input"
            type="text"
            placeholder="搜索设置项"
            aria-label="搜索设置项"
            @keydown.esc="query = ''"
          />
          <button
            v-if="query"
            class="sv-search-clear"
            type="button"
            aria-label="清空搜索"
            @click="query = ''"
          >
            <X :size="12" :stroke-width="2" />
          </button>
        </div>

        <template v-if="query.trim()">
          <button
            v-for="r in searchResults"
            :key="`${r.section}:${r.title}`"
            type="button"
            class="sv-nav-item sv-nav-hit"
            @click="jumpToSetting(r.section, r.title)"
          >
            <span class="sv-hit-title">{{ r.title }}</span>
            <span class="sv-hit-where">{{ r.where }}</span>
          </button>
          <p v-if="!searchResults.length" class="sv-hit-empty">没有匹配的设置项</p>
        </template>

        <template v-else>
          <template v-for="g in GROUPS" :key="g.id">
            <button
              type="button"
              class="sv-nav-item sv-nav-group"
              :class="{ active: activeGroup === g.id }"
              :aria-expanded="hasKids(g.id) ? activeGroup === g.id : undefined"
              @click="selectGroup(g.id)"
              @mouseenter="preloadPanel(g.id)"
            >
              <component :is="g.icon" :size="14" :stroke-width="2" />
              <span>{{ g.label }}</span>
              <ChevronRight
                v-if="hasKids(g.id)"
                class="sv-nav-caret"
                :size="13"
                :stroke-width="2.5"
                aria-hidden="true"
              />
            </button>
            <div v-if="activeGroup === g.id && hasKids(g.id)" class="sv-nav-kids">
              <button
                v-for="sid in sectionsOf(g.id)"
                :key="sid"
                type="button"
                class="sv-nav-item sv-nav-sub"
                :class="{ active: activeSection === sid }"
                :aria-current="activeSection === sid ? 'true' : undefined"
                @click="goToSection(sid)"
              >
                {{ SECTION_LABEL[sid] }}
              </button>
            </div>
          </template>
        </template>
      </nav>

      <!-- 右侧内容：只挂载当前大类的分区（其余不渲染 —— 这是点设置不再先空白的关键） -->
      <div ref="contentRef" class="sv-content">
        <!-- 当前大类面板（按需加载）：具体设置项都在 ./settings/*.vue 里 -->
        <component
          :is="currentPanel"
          @open-layout-editor="emit('open-layout-editor')"
          @open-extensions="emit('open-extensions')"
        />
      </div>
    </div>
  </div>
</template>
