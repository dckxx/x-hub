<script setup lang="ts">
import { computed, inject } from 'vue'
import { Boxes, File as FileIcon, Globe, Settings2 } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import {
  accentFor,
  CATEGORY_ICONS,
  iconSrc,
  useResourceIcon,
} from '../composables/useResourceIcon'
import {
  sudaCustomConfigured,
  sudaCustomItems,
} from '../utils/sudaCustom'
import type { Resource } from '../api/tauri'

/**
 * 工作台「自定义速达」卡片（槽位 suda1..suda4，模块库各自独立条目）：
 * 一格可点击的快捷启动网格——内容来源在配置弹窗里选（手动挑选 / 整个大类 / 指定小类），
 * 点击条目走 store.launchResource（与速达页同一链路：最近使用、网页打开方式分流全生效）。
 * 内容过滤/排序唯一实现在 utils/sudaCustom.ts，与编辑器缩印共用（口径铁律）。
 */
const props = defineProps<{
  slotId: string
  title?: string
  hideTitle?: boolean
}>()

const emit = defineEmits<{ (e: 'configure'): void }>()

const store = useStore()

const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

const cfg = computed(() => store.sudaCustomConfigOf(props.slotId))
const items = computed(() => sudaCustomItems(cfg.value, store.state.resources))
const configured = computed(() => sudaCustomConfigured(cfg.value))

const { showImageIcon, showWebFallbackIcon, iconText, onIconError } =
  useResourceIcon()

function itemStyle(r: Resource) {
  const a = accentFor(r)
  return {
    '--sc-accent': a.strong,
    '--sc-accent-soft': a.soft,
    '--sc-accent-ink': a.ink,
  }
}

function onOpen(r: Resource) {
  store.launchResource(r.id).catch((e) => showToast(`无法打开「${r.name}」：${String(e)}`))
}
</script>

<template>
  <section class="card suda-custom" :aria-label="title ?? '自定义速达'">
    <header class="sc-header" :class="{ 'hd-float': hideTitle }">
      <h3 v-if="!hideTitle" class="sc-title">
        <Boxes :size="14" :stroke-width="2" aria-hidden="true" />
        <span>{{ title ?? '自定义速达' }}</span>
      </h3>
      <button
        class="sc-setting"
        type="button"
        title="配置内容"
        aria-label="配置内容"
        @click="emit('configure')"
      >
        <Settings2 :size="14" :stroke-width="2" aria-hidden="true" />
      </button>
    </header>

    <div v-if="configured && items.length" class="sc-grid">
      <button
        v-for="r in items"
        :key="r.id"
        class="sc-item"
        type="button"
        :title="`${r.name}\n${r.target}`"
        :style="itemStyle(r)"
        @click="onOpen(r)"
      >
        <span class="sc-icon" :class="{ 'has-img': showImageIcon(r) }">
          <img
            v-if="showImageIcon(r)"
            class="sc-img"
            :src="iconSrc(r.icon!)"
            alt=""
            draggable="false"
            @error="onIconError(r)"
          />
          <Globe
            v-else-if="showWebFallbackIcon(r)"
            :size="18"
            :stroke-width="1.7"
            :style="{ color: 'var(--c-green-ink)' }"
            aria-hidden="true"
          />
          <component
            v-else-if="r.kind === 'file'"
            :is="CATEGORY_ICONS[(r.category ?? '其他') as keyof typeof CATEGORY_ICONS] ?? FileIcon"
            :size="18"
            :stroke-width="1.7"
            :style="{ color: 'var(--sc-accent)' }"
            aria-hidden="true"
          />
          <span v-else class="sc-letter" :style="{ color: 'var(--sc-accent-ink)' }">{{
            iconText(r)
          }}</span>
        </span>
        <span class="sc-name">{{ r.name }}</span>
      </button>
    </div>

    <div v-else-if="configured" class="sc-empty">
      <p class="sc-empty-title">这个来源还没有资源</p>
      <p class="sc-empty-sub">先在速达里添加，或点右上角换个来源</p>
    </div>

    <div v-else class="sc-empty">
      <p class="sc-empty-title">还没配置内容</p>
      <p class="sc-empty-sub">点右上角设置，挑选应用 / 网页 / 文件放进来</p>
    </div>
  </section>
</template>

<style scoped>
.suda-custom {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 12px;
  min-height: 0;
}
.sc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
  min-height: 26px;
}
/* 关闭标题：表头整条不占位，动作按钮由全局 .hd-float 悬浮在卡片右上角（见 style.css） */
.sc-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
  letter-spacing: -0.01em;
  margin: 0;
  min-width: 0;
}
.sc-title span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sc-title :deep(svg) {
  color: var(--brand-500);
  flex: none;
}
.sc-setting {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  flex: none;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.18s, color 0.18s;
}
.sc-setting:hover {
  background: var(--bg-card-soft);
  color: var(--brand-500);
}
.sc-grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(56px, 1fr));
  gap: 6px;
  align-content: start;
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: contain;
  /* 滚动条（12px，透明轨道）叠在 6px 内缩上，不挤占网格列宽（同 CountdownCard 口径） */
  padding-right: 6px;
  margin-right: -6px;
}
.sc-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 6px 2px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  cursor: pointer;
  min-width: 0;
  transition: background 0.15s, transform 0.15s;
}
.sc-item:hover {
  background: var(--bg-card-soft);
}
.sc-item:active {
  transform: scale(0.94);
}
.sc-icon {
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  background: var(--sc-accent-soft);
  overflow: hidden;
  flex: none;
}
/* 图片图标不铺强调色底：程序图标四周常带透明留白、圆形图标四角透明，
 * 铺底会透出一圈色边。与最近使用（RecentBar）同口径：底色只给首字母/内置图标 */
.sc-icon.has-img {
  background: transparent;
}
.sc-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.sc-letter {
  font-size: 0.9375rem;
  font-weight: 700;
  line-height: 1;
}
.sc-name {
  max-width: 100%;
  font-size: 0.6875rem;
  line-height: 1.2;
  color: var(--text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sc-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  text-align: center;
}
.sc-empty-title {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-2);
}
.sc-empty-sub {
  margin: 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}
</style>
