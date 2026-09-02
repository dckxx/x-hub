<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watchEffect } from 'vue'
import { X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  EMOJI_CATEGORIES,
  filterEmojis,
  getRecentEmojis,
  pushRecentEmoji,
  type EmojiItem,
} from '../utils/emoji'

/**
 * 表情选择器弹层：分类 tab + 网格 + 搜索 + 最近使用（localStorage）。
 * 由 NoteEditor 斜杠菜单「表情」分组内的「更多表情…」入口打开（受控 visible）。
 * 点击表情不自动关闭（支持连续插入多个表情），Esc / 点击遮罩 / 关闭按钮退出。
 */

const props = defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  (e: 'select', emoji: string): void
  (e: 'close'): void
}>()

const panelRef = ref<HTMLElement | null>(null)
const searchRef = ref<HTMLInputElement | null>(null)
const tabsRef = ref<HTMLElement | null>(null)

const search = ref('')
const activeCat = ref('common')
const recentItems = ref<EmojiItem[]>(getRecentEmojis())

useFocusTrap(
  computed(() => props.visible),
  panelRef,
  searchRef,
)

const visibleActive = computed(() => props.visible)

// 分类 tab：有最近使用时在最前插入「最近使用」
const tabs = computed(() => {
  const cats: { key: string; label: string }[] = []
  if (recentItems.value.length > 0) cats.push({ key: 'recent', label: '最近使用' })
  cats.push(...EMOJI_CATEGORIES.map((c) => ({ key: c.key, label: c.label })))
  return cats
})

const currentCat = computed(() => EMOJI_CATEGORIES.find((c) => c.key === activeCat.value))

const searching = computed(() => search.value.trim().length > 0)

// 全量列表按 emoji 字符去重（同一字符在多个分类复用时只保留首个定义）：
// 搜索结果按此列表渲染且 v-for 以 e 为 key，不去重会撞 key 导致渲染异常
const allEmojis = computed(() => {
  const seen = new Set<string>()
  const out: EmojiItem[] = []
  for (const item of EMOJI_CATEGORIES.flatMap((c) => c.items)) {
    if (seen.has(item.e)) continue
    seen.add(item.e)
    out.push(item)
  }
  return out
})

/** 当前展示列表：搜索时全量过滤，否则按激活分类（最近/常用/…） */
const displayItems = computed(() => {
  if (searching.value) return filterEmojis(allEmojis.value, search.value)
  if (activeCat.value === 'recent') return recentItems.value
  return currentCat.value?.items ?? []
})

/** 网格区顶部的小标题：搜索态显示命中提示，分类态显示分类名 */
const hint = computed(() => {
  if (searching.value) return `匹配「${search.value.trim()}」${displayItems.value.length} 个`
  return activeCat.value === 'recent' ? '最近使用' : currentCat.value?.label ?? ''
})

function onPick(item: EmojiItem) {
  pushRecentEmoji(item.e)
  recentItems.value = getRecentEmojis()
  emit('select', item.e)
}

// ---- 分类 tab 行交互：滚轮横向滑动 + 点击自动滑出隐藏的后续标签 ----

/** 滚轮/触控板在 tab 行上滚动：统一转为横向滚动，不透传给弹层（tab 行本就横向溢出） */
function onTabsWheel(e: WheelEvent) {
  const el = tabsRef.value
  if (!el) return
  const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY
  if (delta === 0) return
  e.preventDefault()
  el.scrollLeft += delta
}

/** 点击分类：切换激活；若点击的是最后一个完整可见的 tab 且后面还有隐藏 tab，则最小滑动把下一个完整露出来 */
function onTabClick(key: string, index: number) {
  activeCat.value = key
  const el = tabsRef.value
  if (!el) return
  const kids = Array.from(el.children) as HTMLElement[]
  const tab = kids[index]
  if (!tab) return
  const next = kids[index + 1]
  const tabRight = tab.offsetLeft + tab.offsetWidth
  if (next && tabRight >= el.scrollLeft + el.clientWidth - 1) {
    // 点击的是可视区末尾且后面还有：滑到让下一个 tab 完整露出
    el.scrollTo({
      left: Math.max(0, next.offsetLeft + next.offsetWidth - el.clientWidth),
      behavior: 'smooth',
    })
  } else if (tab.offsetLeft < el.scrollLeft) {
    // 点击的 tab 左侧被裁了一半：拉回来完整可见
    el.scrollTo({ left: tab.offsetLeft, behavior: 'smooth' })
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
  }
}

watchEffect(() => {
  if (visibleActive.value) window.addEventListener('keydown', onKeydown)
  else window.removeEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @mousedown.self="emit('close')">
        <div ref="panelRef" class="modal-card ep-card" role="dialog" aria-label="表情选择器" aria-modal="true">
          <header class="ep-head">
            <h3 class="ep-title">表情</h3>
            <button class="icon-btn" type="button" title="关闭" aria-label="关闭" @click="emit('close')">
              <X :size="14" :stroke-width="2" />
            </button>
          </header>

          <input
            ref="searchRef"
            v-model="search"
            class="field-input ep-search"
            type="text"
            placeholder="搜索表情（如 笑 / 心 / 火）"
            spellcheck="false"
          />

          <nav
            ref="tabsRef"
            class="filter-tabs ep-tabs"
            role="tablist"
            aria-label="表情分类"
            @wheel="onTabsWheel"
          >
            <button
              v-for="(c, i) in tabs"
              :key="c.key"
              class="filter-tab"
              :class="{ 'filter-tab--primary active': activeCat === c.key }"
              type="button"
              role="tab"
              :aria-selected="activeCat === c.key"
              @click="onTabClick(c.key, i)"
            >
              {{ c.label }}
            </button>
          </nav>

          <div class="ep-body">
            <template v-if="displayItems.length">
              <p class="ep-hint">{{ hint }}</p>
              <div class="ep-grid">
                <button
                  v-for="item in displayItems"
                  :key="item.e"
                  class="ep-emoji"
                  type="button"
                  :title="item.n"
                  :aria-label="item.n"
                  @click="onPick(item)"
                >
                  {{ item.e }}
                </button>
              </div>
            </template>
            <div v-else class="empty-state">没有匹配的表情</div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ep-card {
  width: 384px;
  padding: 14px 16px 16px;
}

.ep-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.ep-title {
  font-size: 1rem;
  font-weight: 650;
  color: var(--text-1);
}

.ep-search {
  margin-bottom: 10px;
}

.ep-tabs {
  margin-bottom: 10px;
}

.ep-body {
  max-height: 300px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.ep-hint {
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--text-3);
  margin: 2px 0 8px;
}

.ep-grid {
  display: grid;
  grid-template-columns: repeat(8, 1fr);
  gap: 2px;
}

.ep-emoji {
  height: 38px;
  border: none;
  background: transparent;
  border-radius: var(--radius-sm);
  font-size: 20px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.12s, transform 0.12s;
}

.ep-emoji:hover {
  background: var(--bg-card-soft);
  transform: translateY(-1px);
}

.ep-emoji:active {
  transform: scale(0.92);
}

/* 全局 .empty-state 默认 padding 40px 16px 过大，弹层内收紧 */
.ep-card :deep(.empty-state) {
  padding: 24px 8px;
  gap: 4px;
}
</style>
