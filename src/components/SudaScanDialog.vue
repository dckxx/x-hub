<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { Check, Loader2, Search } from 'lucide-vue-next'
import { isTauri, tauriApi, type InstalledAppInfo } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import { accentOf, iconSrc } from '../composables/useResourceIcon'

const props = defineProps<{ visible: boolean }>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'imported', apps: InstalledAppInfo[]): void
}>()

const store = useStore()
const cardRef = ref<HTMLElement | null>(null)
const searchRef = ref<HTMLInputElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, searchRef)

const loading = ref(false)
const error = ref('')
const apps = ref<InstalledAppInfo[]>([])
const checked = ref<Set<string>>(new Set())
const keyword = ref('')
const brokenIcons = ref<Set<string>>(new Set())

const keyOf = (a: InstalledAppInfo) => a.target.toLowerCase()

// 已在速达中的应用（按目标路径判重）→ 列表中禁用勾选
const existingTargets = computed(() => {
  const s = new Set<string>()
  for (const r of store.state.resources) {
    if (r.kind === 'app' && r.target) s.add(r.target.toLowerCase())
  }
  return s
})

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  if (!kw) return apps.value
  return apps.value.filter((a) => a.name.toLowerCase().includes(kw))
})

const selectedCount = computed(() => {
  let n = 0
  for (const a of apps.value) {
    const k = keyOf(a)
    if (checked.value.has(k) && !existingTargets.value.has(k)) n++
  }
  return n
})

const allVisibleChecked = computed(() => {
  const visible = filtered.value.filter((a) => !existingTargets.value.has(keyOf(a)))
  return visible.length > 0 && visible.every((a) => checked.value.has(keyOf(a)))
})

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    void startScan()
  },
)

async function startScan() {
  if (!isTauri()) return
  loading.value = true
  error.value = ''
  apps.value = []
  checked.value = new Set()
  keyword.value = ''
  brokenIcons.value = new Set()
  try {
    apps.value = await tauriApi.scanInstalledApps()
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
    // 列表渲染后聚焦搜索框（无列表时为 no-op）
    requestAnimationFrame(() => searchRef.value?.focus())
  }
}

function isExisting(a: InstalledAppInfo) {
  return existingTargets.value.has(keyOf(a))
}

function showImg(a: InstalledAppInfo) {
  return !!a.icon && !brokenIcons.value.has(keyOf(a))
}

function onImgError(a: InstalledAppInfo) {
  brokenIcons.value.add(keyOf(a))
}

function toggleApp(a: InstalledAppInfo) {
  if (isExisting(a)) return
  const k = keyOf(a)
  const next = new Set(checked.value)
  if (next.has(k)) next.delete(k)
  else next.add(k)
  checked.value = next
}

function toggleAll() {
  const next = new Set(checked.value)
  const visible = filtered.value.filter((a) => !isExisting(a))
  const willSelect = !allVisibleChecked.value
  for (const a of visible) {
    const k = keyOf(a)
    if (willSelect) next.add(k)
    else next.delete(k)
  }
  checked.value = next
}

function confirm() {
  const selected = apps.value.filter((a) => {
    const k = keyOf(a)
    return checked.value.has(k) && !existingTargets.value.has(k)
  })
  if (selected.length === 0) return
  emit('imported', selected)
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && props.visible) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div
          ref="cardRef"
          class="modal-card scan-card"
          role="dialog"
          aria-label="扫描已安装应用"
          aria-modal="true"
        >
          <header class="scan-head">
            <h2 class="dialog-title">扫描已安装应用</h2>
            <p class="scan-sub">勾选要加入速达的应用，未勾选的将忽略</p>
          </header>

          <!-- 搜索 + 全选 -->
          <div v-if="!loading && apps.length > 0" class="scan-toolbar">
            <div class="scan-search-wrap">
              <Search :size="14" :stroke-width="2" class="scan-search-icon" aria-hidden="true" />
              <input
                ref="searchRef"
                v-model="keyword"
                class="field-input scan-search"
                type="text"
                placeholder="搜索应用名称…"
                @keydown="onKeydown"
              />
            </div>
            <button class="ghost-btn scan-select-all" @click="toggleAll">
              {{ allVisibleChecked ? '全不选' : '全选' }}
            </button>
          </div>

          <!-- 扫描中 -->
          <div v-if="loading" class="scan-state">
            <Loader2 :size="26" :stroke-width="1.5" class="spin" />
            <p>正在扫描已安装应用…</p>
            <span>首次扫描需提取程序图标，可能稍慢</span>
          </div>

          <!-- 错误 -->
          <div v-else-if="error" class="scan-state">
            <p class="scan-error">{{ error }}</p>
          </div>

          <!-- 空结果 -->
          <div v-else-if="apps.length === 0" class="scan-state">
            <p>未扫描到可导入的应用</p>
          </div>

          <!-- 列表 -->
          <div v-else class="scan-list">
            <label
              v-for="a in filtered"
              :key="a.target"
              class="scan-row"
              :class="{ disabled: isExisting(a), selected: checked.has(keyOf(a)) }"
            >
              <input
                type="checkbox"
                class="scan-checkbox"
                :checked="checked.has(keyOf(a))"
                :disabled="isExisting(a)"
                @change="toggleApp(a)"
              />
              <span class="scan-icon" :style="{ background: accentOf(a.name).soft }">
                <img
                  v-if="showImg(a)"
                  class="scan-img"
                  :src="iconSrc(a.icon!)"
                  alt=""
                  @error="onImgError(a)"
                />
                <span v-else class="scan-letter" :style="{ color: accentOf(a.name).text }">
                  {{ a.name.charAt(0).toUpperCase() }}
                </span>
              </span>
              <span class="scan-info">
                <span class="scan-name">{{ a.name }}</span>
                <span class="scan-target" :title="a.target">{{ a.target }}</span>
              </span>
              <span v-if="isExisting(a)" class="scan-added">已添加</span>
              <span v-else class="scan-check" :class="{ on: checked.has(keyOf(a)) }">
                <Check v-if="checked.has(keyOf(a))" :size="13" :stroke-width="3" />
              </span>
            </label>
          </div>

          <!-- 底部操作 -->
          <footer v-if="!loading && apps.length > 0" class="scan-footer">
            <span class="scan-count">已选 {{ selectedCount }} 项</span>
            <div class="scan-actions">
              <button class="ghost-btn btn" @click="emit('close')">取消</button>
              <button class="pill-btn btn" :disabled="selectedCount === 0" @click="confirm">
                添加选中（{{ selectedCount }}）
              </button>
            </div>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.scan-card {
  width: 560px;
  max-height: calc(100vh - 80px);
  display: flex;
  flex-direction: column;
  padding: 20px;
}
.dialog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
}
.scan-sub {
  margin-top: 2px;
  font-size: 12px;
  color: var(--text-3);
}
.scan-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 14px 0 10px;
}
.scan-search-wrap {
  position: relative;
  flex: 1;
  min-width: 0;
}
.scan-search {
  padding-left: 32px;
}
.scan-search-icon {
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-3);
  pointer-events: none;
}
.scan-select-all {
  flex-shrink: 0;
  padding: 6px 14px;
}
.scan-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  padding: 4px;
}
.scan-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background 0.12s;
}
.scan-row:hover {
  background: var(--bg-card-soft);
}
.scan-row.disabled {
  opacity: 0.45;
  cursor: default;
}
.scan-row.disabled:hover {
  background: transparent;
}
.scan-checkbox {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}
.scan-icon {
  width: 38px;
  height: 38px;
  border-radius: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  overflow: hidden;
}
.scan-img {
  width: 38px;
  height: 38px;
  object-fit: contain;
  background: var(--bg-card);
}
.scan-letter {
  font-size: 17px;
  font-weight: 700;
}
.scan-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.scan-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.scan-target {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.scan-added {
  flex-shrink: 0;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-green);
  background: var(--c-green-soft);
  padding: 2px 8px;
  border-radius: var(--radius-pill);
}
.scan-check {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  border: 1.5px solid var(--border-strong);
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-card-solid);
  color: var(--text-on-accent);
  transition: background 0.15s, border-color 0.15s;
}
.scan-check.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.scan-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 14px;
}
.scan-count {
  font-size: 12px;
  color: var(--text-3);
}
.scan-actions {
  display: flex;
  gap: 10px;
}
.btn {
  padding: 7px 16px;
}
.scan-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 52px 16px;
  color: var(--text-3);
  font-size: 13px;
}
.scan-state span {
  font-size: 11px;
  color: var(--text-4);
}
.scan-error {
  color: var(--c-red);
}
.spin {
  animation: scan-spin 0.9s linear infinite;
}
@keyframes scan-spin {
  to {
    transform: rotate(360deg);
  }
}

.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
