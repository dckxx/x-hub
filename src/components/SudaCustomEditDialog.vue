<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { inject } from 'vue'
import { Check, Search } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import { SUDA_CUSTOM_SOURCE_OPTIONS } from '../utils/sudaCustom'
import type { SudaCustomModuleConfig } from '../api/tauri'

/**
 * 工作台「自定义速达」槽位内容配置弹窗（suda1..suda4 各一张卡片，右上角设置钮唤起）：
 * 选内容来源——手动挑选（勾选即入列、顺序即展示顺序）/ 整个大类（应用/网页/文件）/
 * 指定小类 / 指定分区（成员增删自动跟随）。
 * 保存走 store.saveSudaCustomModule（config 整体落盘）；关闭不保存。
 */
const props = defineProps<{
  slotId: string
  visible: boolean
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast')

const cardRef = ref<HTMLElement | null>(null)
const searchRef = ref<HTMLInputElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef, searchRef)

const KIND_LABELS: Record<string, string> = { app: '应用', web: '网页', file: '文件' }

const slotTitle = computed(() => {
  const m = /^suda(\d+)$/.exec(props.slotId)
  return m ? `自定义速达 ${m[1]}` : props.slotId
})

// ---- 草稿态：打开时从现有配置快照（immediate：弹窗挂载时可能已是打开态，约定 58） ----
const source = ref<SudaCustomModuleConfig['source']>('pinned')
const subcategory = ref('')
const zoneId = ref<number | null>(null)
const selectedIds = ref<number[]>([])
const search = ref('')

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    const cfg = store.sudaCustomConfigOf(props.slotId)
    source.value = cfg?.source ?? 'pinned'
    subcategory.value = cfg?.subcategory ?? ''
    zoneId.value = cfg?.zone_id ?? null
    selectedIds.value = [...(cfg?.resource_ids ?? [])]
    search.value = ''
  },
  { immediate: true },
)

function setSource(s: SudaCustomModuleConfig['source']) {
  source.value = s
}

function toggleResource(id: number) {
  const i = selectedIds.value.indexOf(id)
  if (i >= 0) selectedIds.value.splice(i, 1)
  else selectedIds.value.push(id)
}

// ---- 手动挑选列表：全部资源 + 搜索过滤（store 顺序，勾选序号 = selectedIds 中的位置） ----
const pool = computed(() => {
  const q = search.value.trim().toLowerCase()
  return store.state.resources.filter(
    (r) => !q || r.name.toLowerCase().includes(q) || r.target.toLowerCase().includes(q),
  )
})

function orderOf(id: number): number {
  return selectedIds.value.indexOf(id)
}

// ---- 小类单选：按大类分组（小类名 = resources.category 口径，与速达页小类筛选一致） ----
const subGroups = computed(() =>
  (['app', 'web', 'file'] as const).map((kind) => ({
    kind,
    label: KIND_LABELS[kind],
    subs: store.subcategoriesOf(kind),
  })),
)

function pickSub(name: string) {
  subcategory.value = subcategory.value === name ? '' : name
}

// ---- 分区单选：速达「全部」tab 的分区（store 序）；再点一次取消选择 ----
function pickZone(id: number) {
  zoneId.value = zoneId.value === id ? null : id
}

function save() {
  store
    .saveSudaCustomModule({
      id: props.slotId,
      source: source.value,
      subcategory: source.value === 'subcategory' ? subcategory.value : '',
      zone_id: source.value === 'zone' ? zoneId.value : null,
      resource_ids: source.value === 'pinned' ? [...selectedIds.value] : [],
    })
    .then(() => {
      showToast?.('已保存')
      emit('close')
    })
    .catch((e) => {
      // 落盘失败保持弹窗开着，就地提示（可重试）
      showToast?.(`保存失败：${String(e)}`)
    })
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" @click.self="emit('close')">
      <div
        ref="cardRef"
        class="modal-card sc-dialog"
        role="dialog"
        :aria-label="`配置${slotTitle}`"
        aria-modal="true"
      >
        <h2 class="dialog-title">配置「{{ slotTitle }}」</h2>
        <p class="sc-sub">选择这张卡片展示哪些速达内容，点击卡片即可直接打开</p>

        <!-- 来源切换 -->
        <div class="sc-src-switch" role="radiogroup" aria-label="内容来源">
          <button
            v-for="o in SUDA_CUSTOM_SOURCE_OPTIONS"
            :key="o.value"
            class="sc-src-pill"
            :class="{ active: source === o.value }"
            type="button"
            role="radio"
            :aria-checked="source === o.value"
            @click="setSource(o.value)"
          >
            {{ o.label }}
          </button>
        </div>

        <!-- 手动挑选 -->
        <template v-if="source === 'pinned'">
          <div class="sc-search-row">
            <Search :size="14" :stroke-width="2" aria-hidden="true" />
            <input
              ref="searchRef"
              v-model="search"
              class="sc-search"
              type="text"
              placeholder="搜索资源名称或地址"
            />
          </div>
          <div class="sc-pick-list" role="listbox" aria-label="资源列表" aria-multiselectable="true">
            <button
              v-for="r in pool"
              :key="r.id"
              class="sc-pick-item"
              :class="{ checked: orderOf(r.id) >= 0 }"
              type="button"
              @click="toggleResource(r.id)"
            >
              <span class="sc-pick-check">
                <Check v-if="orderOf(r.id) >= 0" :size="12" :stroke-width="2.4" />
                <span v-else class="sc-pick-order">{{ orderOf(r.id) + 1 }}</span>
              </span>
              <span class="sc-pick-name" :title="r.target">{{ r.name }}</span>
              <span class="sc-pick-kind" :class="r.kind">{{ { app: '应用', web: '网页', file: '文件' }[r.kind] }}</span>
            </button>
            <p v-if="pool.length === 0" class="sc-pick-empty">没有匹配的资源</p>
          </div>
          <p class="sc-hint">已选 {{ selectedIds.length }} 项 · 勾选顺序即卡片展示顺序</p>
        </template>

        <!-- 整个大类 -->
        <template v-else-if="source === 'app' || source === 'web' || source === 'file'">
          <p class="sc-mode-hint">
            卡片将展示速达里的全部「{{ { app: '应用', web: '网页', file: '文件' }[source] }}」，
            在速达页增删条目会自动跟随。
          </p>
        </template>

        <!-- 小类 -->
        <template v-else-if="source === 'subcategory'">
          <div class="sc-sub-list">
            <div v-for="g in subGroups" :key="g.kind" class="sc-sub-group">
              <span class="sc-sub-group-label">{{ g.label }}</span>
              <div class="sc-sub-opts">
                <button
                  v-for="s in g.subs"
                  :key="s.id"
                  class="sc-src-pill sm"
                  :class="{ active: subcategory === s.name }"
                  type="button"
                  @click="pickSub(s.name)"
                >
                  {{ s.name }}
                </button>
                <span v-if="g.subs.length === 0" class="sc-sub-none">还没有小类</span>
              </div>
            </div>
          </div>
          <p class="sc-hint">展示所属小类为所选值的全部资源（与速达页小类筛选同口径）</p>
        </template>

        <!-- 分区 -->
        <template v-else-if="source === 'zone'">
          <div class="sc-sub-list">
            <div class="sc-sub-group">
              <span class="sc-sub-group-label">速达分区</span>
              <div class="sc-sub-opts">
                <button
                  v-for="z in store.state.zones"
                  :key="z.id"
                  class="sc-src-pill sm"
                  :class="{ active: zoneId === z.id }"
                  type="button"
                  @click="pickZone(z.id)"
                >
                  {{ z.name }}
                </button>
                <span v-if="store.state.zones.length === 0" class="sc-sub-none">还没有分区，先在速达「全部」页新建</span>
              </div>
            </div>
          </div>
          <p class="sc-hint">展示该分区内的全部资源（顺序与速达页一致）；往分区新增或拖入资源，卡片自动跟随</p>
        </template>

        <div class="sc-footer">
          <button class="sc-btn" type="button" @click="emit('close')">取消</button>
          <button class="sc-btn primary" type="button" @click="save">保存</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.sc-dialog {
  width: 440px;
  display: flex;
  flex-direction: column;
}
.sc-sub {
  margin: -6px 0 14px;
  font-size: 0.75rem;
  color: var(--text-3);
}
.sc-src-switch {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 12px;
}
.sc-src-pill {
  padding: 6px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--text-2);
  font-size: 0.75rem;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.sc-src-pill:hover {
  background: var(--bg-card-soft);
}
.sc-src-pill.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.sc-src-pill.sm {
  padding: 4px 12px;
}
.sc-search-row {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  padding: 0 10px;
  margin-bottom: 8px;
  color: var(--text-4);
}
.sc-search {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-1);
  font-size: 0.8125rem;
  font-family: inherit;
  padding: 9px 0;
}
.sc-search::placeholder {
  color: var(--text-4);
}
.sc-pick-list {
  height: 260px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 6px;
}
.sc-pick-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  cursor: pointer;
  text-align: left;
  font-family: inherit;
  transition: background 0.12s;
}
.sc-pick-item:hover {
  background: var(--bg-card-soft);
}
.sc-pick-item.checked {
  background: var(--brand-50, color-mix(in srgb, var(--brand-500) 10%, transparent));
}
.sc-pick-check {
  width: 18px;
  height: 18px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1.5px solid var(--border-soft);
  border-radius: var(--radius-sm);
  color: #fff;
  background: transparent;
}
.sc-pick-item.checked .sc-pick-check {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.sc-pick-order {
  font-size: 0.625rem;
  color: var(--text-4);
}
.sc-pick-item.checked .sc-pick-order {
  display: none;
}
.sc-pick-name {
  flex: 1;
  min-width: 0;
  font-size: 0.8125rem;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sc-pick-kind {
  flex: none;
  font-size: 0.625rem;
  padding: 1px 6px;
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-3);
}
.sc-pick-empty {
  margin: 0;
  padding: 18px 0;
  text-align: center;
  font-size: 0.75rem;
  color: var(--text-4);
}
.sc-hint {
  margin: 10px 0 0;
  font-size: 0.6875rem;
  color: var(--text-4);
}
.sc-mode-hint {
  margin: 4px 0 0;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-3);
}
.sc-sub-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-height: 260px;
  overflow-y: auto;
}
.sc-sub-group-label {
  display: block;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-3);
  margin-bottom: 6px;
}
.sc-sub-opts {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.sc-sub-none {
  font-size: 0.6875rem;
  color: var(--text-4);
}
.sc-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.sc-btn {
  padding: 8px 18px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.15s;
}
.sc-btn:hover {
  background: var(--bg-card-soft);
}
.sc-btn.primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.sc-btn.primary:hover {
  background: var(--brand-600);
}
</style>
