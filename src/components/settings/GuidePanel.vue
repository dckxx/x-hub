<script setup lang="ts">
// 使用说明大类（设置 → 使用说明）
//
// 正文是**仓库里的 docs/user-guide.md 本身**（`?raw` 内联），不在这里另写一份：
// 两处文档必然会漂移，改一处忘一处，用户看到的操作路径就与真实界面不符。
// 左侧章节目录、目录内搜索、正文标题锚点全部由同一份正文推导（`utils/guideToc.ts`），
// 所以加/删章节时只需改 Markdown，界面自动跟随。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { ArrowUp, BookOpen, Search, X } from 'lucide-vue-next'
import guideSource from '../../../docs/user-guide.md?raw'
import { countOccurrences, splitGuideDocument } from '../../utils/guideToc'

const doc = splitGuideDocument(guideSource as string)

// marked + DOMPurify 体积不小（几十 KB），只在真正打开本面板时才取下来
const md = shallowRef<typeof import('../../utils/markdownHtml') | null>(null)
onMounted(() => {
  void import('../../utils/markdownHtml').then((m) => {
    md.value = m
  })
})

/** 每段 → 安全 HTML。渲染口径与速记预览共用 renderNoteMarkdown（marked + DOMPurify） */
const htmlCache = new Map<string, string>()
function render(markdown: string): string {
  if (!md.value || !markdown) return ''
  const hit = htmlCache.get(markdown)
  if (hit != null) return hit
  const html = md.value.renderNoteMarkdown(markdown)
  htmlCache.set(markdown, html)
  return html
}

// ---- 章节目录 + 目录内搜索 ----
const query = ref('')

interface TocRow {
  kind: 'chapter' | 'section'
  id: string
  title: string
  /** 本章/本节正文里的关键词命中数（搜索态显示） */
  hits: number
}

/** 章节 → 该章内所有小节的 id（用于把 activeId（可能是三级标题）反查回所属章） */
const sectionToChapter = new Map<string, string>()
for (const c of doc.chapters) {
  for (const s of c.sections) sectionToChapter.set(s.id, c.id)
}

const toc = computed<TocRow[]>(() => {
  const q = query.value.trim()
  const rows: TocRow[] = []
  for (const c of doc.chapters) {
    const chapterText = [c.title, c.body, ...c.sections.map((s) => `${s.title}\n${s.body}`)].join('\n')
    const chapterHits = countOccurrences(chapterText, q)
    // 搜索态：整章都没命中就整块不显示（章标题命中时保留其全部小节，方便逐条看）
    if (q && chapterHits === 0) continue
    rows.push({ kind: 'chapter', id: c.id, title: c.title, hits: chapterHits })
    for (const s of c.sections) {
      const hits = countOccurrences(`${s.title}\n${s.body}`, q)
      if (q && hits === 0) continue
      rows.push({ kind: 'section', id: s.id, title: s.title, hits })
    }
  }
  return rows
})

const resultCount = computed(() => (query.value.trim() ? toc.value.length : 0))

// ---- 跳转与滚动高亮 ----
// 滚动容器是设置页外壳的 .sv-content（面板自己不套第二个滚动区）：
// 嵌套滚动会让「滚轮/触控板落在目录栏时不动正文」「跳转滚错容器」这类问题接连出现。
const hostRef = ref<HTMLElement | null>(null)
const activeId = ref(doc.chapters[0]?.id ?? '')
const activeChapterId = computed(() => sectionToChapter.get(activeId.value) ?? activeId.value)
const showTop = ref(false)
let scroller: HTMLElement | null = null
// 跳转期间锁定高亮：平滑滚动会连续触发 scroll，中途经过的标题会把 activeId 抢走，
// 表现为「点了第 5 章，目录却高亮第 4 章」——停下后再交还滚动反查
let lockUntil = 0
// 面板刚打开时 Markdown 还没渲染完，正文高度是「逐渐撑开」的：此时启动的平滑滚动会被
// 随后的布局变化冲掉、最终落回顶部。所以跳转一律**瞬时定位**（behavior: 'instant'），
// 再用定时复核兜住「点得比渲染快」的情形。
let lastJumpId = ''

function jumpTo(id: string) {
  lastJumpId = id
  activeId.value = id
  lockUntil = Date.now() + 600
  // 先挂自校正链，再尝试滚动：正文还没渲染时 el 取不到（点得比 marked 快），
  // 若在这里直接 return 就没人再来补这次跳转了——表现为「点了目录停在顶部」
  scheduleRecheck(id)
  scrollToId(id)
}

/** 瞬时定位：平滑滚动在「正文高度还在增长」时会被打断并回弹，跳目录只能瞬时到达 */
function scrollToId(id: string) {
  const el = hostRef.value?.querySelector<HTMLElement>(`[data-guide-id="${CSS.escape(id)}"]`)
  if (!el) return false
  el.scrollIntoView({ behavior: 'instant', block: 'start' })
  return true
}

/**
 * 跳转后的定时自校正：正文是异步渲染的，目标在下面还没撑开时先滚一次只能停在半路，
 * 之后高度撑开、目标跑远，滚动位置却不会自己跟上。这里在几个时间点复核，
 * 只在明显跑偏（>40px）时补位，且目标已被新一次跳转取代就放弃。
 */
function scheduleRecheck(id: string) {
  for (const delay of [120, 400, 900, 1600, 2600]) {
    window.setTimeout(() => {
      if (id !== lastJumpId || !hostRef.value || !scroller) return
      const el = hostRef.value.querySelector<HTMLElement>(`[data-guide-id="${CSS.escape(id)}"]`)
      if (!el) return
      const delta = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top
      if (Math.abs(delta - 12) <= 40) return
      scrollToId(id)
    }, delay)
  }
}

// 正文（marked）就绪就是高度变化最大的那一刻，补一次定位
watch(md, (ready) => {
  if (!ready || !lastJumpId) return
  void nextTick(() => scheduleRecheck(lastJumpId))
})

/**
 * 反查当前读到的章节：最后一个顶到滚动区上沿的标题就是它。
 * 阈值取 64px 而不是 0：scroll-margin-top 与容器内边距会把标题压在离上沿几十像素处，
 * 用 0 会让「刚跳过去的那一章」判成上一章。
 */
function syncActive() {
  const host = hostRef.value
  if (!host || !scroller || Date.now() < lockUntil) return
  const top = scroller.getBoundingClientRect().top
  let current = ''
  for (const el of host.querySelectorAll<HTMLElement>('[data-guide-id]')) {
    if (el.getBoundingClientRect().top - top <= 64) current = el.dataset.guideId ?? ''
    else break
  }
  if (current && current !== activeId.value) activeId.value = current
}

function onScroll() {
  showTop.value = (scroller?.scrollTop ?? 0) > 320
  syncActive()
}

function scrollTop() {
  scroller?.scrollTo({ top: 0, behavior: 'smooth' })
}

onMounted(() => {
  scroller = hostRef.value?.closest<HTMLElement>('.sv-content') ?? null
  scroller?.addEventListener('scroll', onScroll, { passive: true })
})

onBeforeUnmount(() => scroller?.removeEventListener('scroll', onScroll))

function clearQuery() {
  query.value = ''
}
</script>

<template>
  <!-- id 是设置项搜索的落点（SettingsView 的 jumpToSetting 找不到具体设置行时退化到分区顶部） -->
  <section id="sv-sec-guide" class="sv-sec guide-sec" aria-label="使用说明">
    <div class="guide-view">
      <!-- 左：章节树（含目录内搜索） -->
      <nav class="guide-toc" aria-label="使用说明目录">
        <div class="guide-search">
          <Search :size="13" :stroke-width="2" aria-hidden="true" />
          <input
            v-model="query"
            class="guide-search-input"
            type="text"
            placeholder="在说明中搜索…"
            aria-label="在使用说明中搜索"
            @keydown.esc="clearQuery"
          />
          <button v-if="query" class="guide-search-clear" type="button" aria-label="清空搜索" @click="clearQuery">
            <X :size="12" :stroke-width="2" />
          </button>
        </div>
        <p v-if="query.trim()" class="guide-hit-count">命中 {{ resultCount }} 项</p>

        <div class="guide-toc-list">
          <template v-for="row in toc" :key="row.id">
            <button
              v-if="row.kind === 'chapter'"
              type="button"
              class="guide-toc-chapter"
              :class="{ active: activeChapterId === row.id }"
              @click="jumpTo(row.id)"
            >
              <span class="guide-toc-text">{{ row.title }}</span>
              <span v-if="query.trim()" class="guide-toc-hits">{{ row.hits }}</span>
            </button>
            <button
              v-else
              type="button"
              class="guide-toc-section"
              :class="{ active: activeId === row.id }"
              @click="jumpTo(row.id)"
            >
              <span class="guide-toc-text">{{ row.title }}</span>
              <span v-if="query.trim()" class="guide-toc-hits">{{ row.hits }}</span>
            </button>
          </template>
          <p v-if="!toc.length" class="guide-toc-empty">没有匹配的内容，换个关键词试试</p>
        </div>
      </nav>

      <!-- 右：文档正文。滚动条由设置页外壳的 .sv-content 统一承担（面板不套第二个滚动区：
           嵌套滚动会让「滚轮落在目录栏时正文不动」「跳转滚错容器」接连出现） -->
      <div ref="hostRef" class="guide-body">
        <article class="guide-doc">
          <header class="guide-doc-head">
            <h1 class="guide-doc-title">
              <BookOpen :size="18" :stroke-width="2" aria-hidden="true" />
              {{ doc.title }}
            </h1>
            <!-- 前言：适用版本 / 阅读约定（原文里那份链接目录已由左栏承担，解析时整章丢弃） -->
            <div v-if="md" class="guide-md" v-html="render(doc.preamble)" />
          </header>

          <p v-if="!md" class="guide-loading">正在加载说明内容…</p>

          <section v-for="chapter in doc.chapters" :key="chapter.id" class="guide-chapter">
            <h2 :data-guide-id="chapter.id" class="guide-h2">{{ chapter.title }}</h2>
            <div v-if="chapter.body" class="guide-md" v-html="render(chapter.body)" />

            <template v-for="sec in chapter.sections" :key="sec.id">
              <h3 :data-guide-id="sec.id" class="guide-h3">{{ sec.title }}</h3>
              <div v-if="sec.body" class="guide-md" v-html="render(sec.body)" />
            </template>
          </section>

          <footer class="guide-foot">
            本文档随客户端一同发布；界面与文案如有调整，以实际界面为准。文档源文件：仓库
            <code>docs/user-guide.md</code>。
          </footer>

          <!-- 回到顶部：sticky 挂在正文底部，滚过一屏后出现（外层零高占位，不挤压文档末尾） -->
          <div class="guide-top-wrap">
            <button v-if="showTop" class="guide-top" type="button" aria-label="回到顶部" @click="scrollTop">
              <ArrowUp :size="15" :stroke-width="2" />
            </button>
          </div>
        </article>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* 本面板专属样式（通用设置样式在 settings/shared.css，见约定 50）。
   正文排版自成一套（表格/代码块/引用），与 setting-row 体系无关，因此全部写在这里。 */
.guide-view {
  display: flex;
  gap: 18px;
  align-items: flex-start;
}
/* 左侧目录：随设置页滚动区一起滚出视野前保持可见（sticky 比自建滚动区更省事，
   也不会因为「滚轮落在目录里」而出现正文不动的割裂感） */
.guide-toc {
  position: sticky;
  top: calc(-1 * var(--space-4));
  flex-shrink: 0;
  width: 208px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: calc(100vh - 200px);
  padding: 0 0 8px;
}
.guide-search {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-3);
  flex-shrink: 0;
}
.guide-search:focus-within {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.guide-search-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-1);
  font-size: 0.75rem;
}
.guide-search-input::placeholder {
  color: var(--text-3);
}
.guide-search-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
}
.guide-search-clear:hover {
  color: var(--text-1);
}
.guide-hit-count {
  margin: 0;
  padding-left: 2px;
  font-size: 0.6875rem;
  color: var(--text-3);
  flex-shrink: 0;
}
.guide-toc-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding-right: 2px;
}
.guide-toc-chapter,
.guide-toc-section {
  display: flex;
  align-items: baseline;
  gap: 6px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  text-align: left;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  transition: background 150ms ease-out, color 150ms ease-out;
}
.guide-toc-chapter:hover,
.guide-toc-section:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.guide-toc-chapter.active {
  background: var(--brand-50);
  color: var(--brand-500);
}
.guide-toc-section {
  padding-left: 18px;
  font-size: 0.6875rem;
  font-weight: 500;
  color: var(--text-3);
}
.guide-toc-section.active {
  background: var(--brand-50);
  color: var(--brand-500);
  font-weight: 600;
}
.guide-toc-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.guide-toc-hits {
  flex-shrink: 0;
  font-size: 0.625rem;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.guide-toc-empty {
  margin: 6px 8px;
  font-size: 0.7rem;
  color: var(--text-3);
  line-height: 1.6;
}
/* 右侧正文：不设自己的滚动条（统一走设置页外壳的滚动容器），只管排版宽度 */
.guide-body {
  position: relative;
  flex: 1;
  min-width: 0;
  padding-right: 6px;
}
.guide-doc {
  max-width: 880px;
  color: var(--text-1);
  font-size: 0.8125rem;
  line-height: 1.75;
}
.guide-doc-head {
  padding-bottom: 10px;
  border-bottom: 1px solid var(--border-soft);
}
.guide-doc-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 8px;
  font-size: 1.0625rem;
  font-weight: 700;
  color: var(--text-1);
}
.guide-doc-title svg {
  color: var(--brand-500);
}
.guide-loading {
  margin: 12px 0;
  font-size: 0.75rem;
  color: var(--text-3);
}
/* 标题：scroll-margin-top 留出上沿余量，跳过去标题不会被容器边缘切一半 */
.guide-h2 {
  margin: 26px 0 10px;
  padding-left: 10px;
  border-left: 3px solid var(--brand-500);
  font-size: 0.9375rem;
  font-weight: 700;
  color: var(--text-1);
  scroll-margin-top: 8px;
}
.guide-h3 {
  margin: 20px 0 8px;
  font-size: 0.875rem;
  font-weight: 650;
  color: var(--text-1);
  scroll-margin-top: 8px;
}
.guide-foot {
  margin-top: 28px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
  font-size: 0.6875rem;
  color: var(--text-3);
}
.guide-foot code {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 0.95em;
  padding: 1px 4px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--text-3) 16%, transparent);
}
/* 回到顶部：sticky 落在正文滚动区底部，滚过一屏后出现（用外层占位避免挤压文档末尾） */
.guide-top-wrap {
  position: sticky;
  bottom: 0;
  display: flex;
  justify-content: flex-end;
  height: 0;
  pointer-events: none;
  z-index: 2;
}
.guide-top {
  transform: translateY(-38px);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 1px solid var(--border-soft);
  border-radius: 50%;
  background: var(--bg-card-solid);
  color: var(--text-2);
  box-shadow: var(--shadow-item);
  cursor: pointer;
  pointer-events: auto;
}
.guide-top:hover {
  color: var(--brand-500);
  border-color: var(--brand-500);
}

/* ---- 正文排版（v-html 渲染出来的节点不带 data-v，必须 :deep() 穿透） ---- */
.guide-md {
  color: var(--text-1);
  overflow-wrap: anywhere;
}
.guide-md :deep(> *:first-child) {
  margin-top: 0;
}
.guide-md :deep(p),
.guide-md :deep(ul),
.guide-md :deep(ol),
.guide-md :deep(blockquote),
.guide-md :deep(pre),
.guide-md :deep(table) {
  margin: 0 0 10px;
}
.guide-md :deep(ul),
.guide-md :deep(ol) {
  padding-left: 20px;
}
.guide-md :deep(li) {
  margin: 2px 0;
}
.guide-md :deep(li > ul),
.guide-md :deep(li > ol) {
  margin: 2px 0;
}
.guide-md :deep(strong) {
  font-weight: 650;
  color: var(--text-1);
}
.guide-md :deep(a) {
  color: var(--brand-500);
  text-decoration: none;
}
.guide-md :deep(a:hover) {
  text-decoration: underline;
}
.guide-md :deep(code) {
  padding: 1px 5px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--text-3) 16%, transparent);
  color: var(--code-text);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.92em;
  overflow-wrap: anywhere;
}
/* 代码块：与速记预览同款（.md-code 由 renderNoteMarkdown 的 renderer 产出） */
.guide-md :deep(.md-code) {
  margin: 0 0 10px;
  border: 1px solid var(--code-border);
  border-radius: 8px;
  background: var(--bg-code);
  overflow: hidden;
}
.guide-md :deep(.md-code-lang) {
  padding: 5px 12px;
  background: var(--bg-code-head);
  border-bottom: 1px solid var(--code-border);
  color: var(--code-text-dim);
  font-size: 0.6875em;
  font-weight: 600;
  letter-spacing: 0.03em;
}
.guide-md :deep(.md-code pre) {
  margin: 0;
  padding: 10px 12px;
  background: transparent;
  color: var(--code-text);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.8125em;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.guide-md :deep(.md-code code) {
  padding: 0;
  background: transparent;
  color: inherit;
  font-size: inherit;
}
/* 表格：文档里有大量「字段 → 说明」两列表，窄窗口下允许横向滚动而不是挤成一团 */
.guide-md :deep(table) {
  display: block;
  width: max-content;
  max-width: 100%;
  overflow-x: auto;
  border-collapse: collapse;
  font-size: 0.75rem;
}
.guide-md :deep(th),
.guide-md :deep(td) {
  padding: 6px 10px;
  border: 1px solid var(--border-soft);
  text-align: left;
  vertical-align: top;
}
.guide-md :deep(th) {
  background: var(--bg-card-soft);
  font-weight: 650;
  white-space: nowrap;
}
.guide-md :deep(blockquote) {
  padding: 8px 12px;
  border-left: 3px solid var(--border-strong);
  background: var(--bg-card-soft);
  color: var(--text-2);
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
}
.guide-md :deep(blockquote > :last-child) {
  margin-bottom: 0;
}
.guide-md :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-soft);
  margin: 16px 0;
}
.guide-md :deep(img) {
  max-width: 100%;
  height: auto;
}
/* 前言里的链接目录（1. [认识 x-hub](#...)）在面板里是死链——目录已由左栏承担，整块隐藏 */
.guide-doc-head .guide-md :deep(ol) {
  display: none;
}
</style>
