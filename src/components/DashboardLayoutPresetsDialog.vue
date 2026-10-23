<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { RotateCcw, Save, Trash2, X } from 'lucide-vue-next'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  DASHBOARD_LAYOUT_PRESET_NAME_MAX,
  MAX_DASHBOARD_LAYOUT_PRESETS,
  useDashboardLayout,
} from '../composables/useDashboardLayout'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const layout = useDashboardLayout()
const showToast = inject<(msg: string) => void>('showToast', () => {})

const cardRef = ref<HTMLElement | null>(null)
const nameRef = ref<HTMLInputElement | null>(null)
const name = ref('')

useFocusTrap(toRef(props, 'visible'), cardRef, nameRef)

const canSaveNew = computed(
  () =>
    layout.placements.value.length > 0 &&
    name.value.trim().length > 0 &&
    layout.layoutPresets.value.length < MAX_DASHBOARD_LAYOUT_PRESETS,
)

function defaultName(): string {
  return `布局 ${Math.min(layout.layoutPresets.value.length + 1, MAX_DASHBOARD_LAYOUT_PRESETS)}`
}

function resetForm() {
  name.value =
    layout.layoutPresets.value.length < MAX_DASHBOARD_LAYOUT_PRESETS
      ? defaultName()
      : layout.layoutPresets.value[0]?.name ?? ''
}

function close() {
  emit('close')
}

function saveToSlot(index: number) {
  const preset = layout.layoutPresets.value[index]
  const cleanName = name.value.trim() || preset?.name || defaultName()
  if (!layout.saveLayoutPreset(cleanName, index)) {
    showToast(
      layout.placements.value.length
        ? '布局名称不能为空'
        : '画布为空，先拖入模块再保存布局',
    )
    return
  }
  showToast(preset ? `已覆盖布局「${cleanName}」` : `已保存布局「${cleanName}」`)
  close()
}

function saveNew() {
  if (!canSaveNew.value) {
    showToast(
      layout.placements.value.length
        ? '已达到 3 个布局上限，请覆盖已有布局'
        : '画布为空，先拖入模块再保存布局',
    )
    return
  }
  saveToSlot(layout.layoutPresets.value.length)
}

function restore(index: number) {
  const preset = layout.layoutPresets.value[index]
  if (!preset) return
  if (!layout.restoreLayoutPreset(index)) {
    showToast(`布局「${preset.name}」已失效，请重新保存`)
    return
  }
  showToast(`已载入布局「${preset.name}」，点「确认」后生效`)
  close()
}

function remove(index: number) {
  const preset = layout.layoutPresets.value[index]
  if (!preset || !layout.removeLayoutPreset(index)) return
  resetForm()
  showToast(`已删除布局「${preset.name}」`)
}

function formatSlot(index: number): string {
  return `布局 ${index + 1}`
}

function onKeydown(e: KeyboardEvent) {
  if (!props.visible || e.key !== 'Escape') return
  e.preventDefault()
  close()
}

watch(
  () => props.visible,
  async (visible) => {
    if (visible) {
      resetForm()
      await nextTick()
      nameRef.value?.select()
    }
    if (typeof window === 'undefined') return
    if (visible) window.addEventListener('keydown', onKeydown)
    else window.removeEventListener('keydown', onKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="close">
        <div
          ref="cardRef"
          class="modal-card dlp-card"
          role="dialog"
          aria-modal="true"
          aria-label="工作台布局方案"
        >
          <div class="dlp-head">
            <div>
              <h2 class="dialog-title">布局方案</h2>
              <p class="dlp-hint">最多保存 3 个布局，保存内容包含模块位置、尺寸与展示形态。</p>
            </div>
            <button class="icon-btn" type="button" aria-label="关闭" title="关闭" @click="close">
              <X :size="14" :stroke-width="2" />
            </button>
          </div>

          <div class="dlp-save">
            <input
              ref="nameRef"
              v-model="name"
              class="field-input"
              type="text"
              :maxlength="DASHBOARD_LAYOUT_PRESET_NAME_MAX"
              placeholder="布局名称，如「专注」"
              aria-label="布局名称"
              @keydown.enter.prevent="saveNew"
            />
            <button
              class="pill-btn"
              type="button"
              :disabled="!canSaveNew"
              @click="saveNew"
            >
              <Save :size="14" :stroke-width="2" aria-hidden="true" />
              保存为新布局
            </button>
          </div>

          <div class="dlp-list">
            <div
              v-for="i in MAX_DASHBOARD_LAYOUT_PRESETS"
              :key="i"
              class="dlp-row"
              :class="{ empty: !layout.layoutPresets.value[i - 1] }"
            >
              <template v-if="layout.layoutPresets.value[i - 1]">
                <div class="dlp-row-main">
                  <strong class="dlp-row-name">{{ layout.layoutPresets.value[i - 1]?.name }}</strong>
                  <span class="dlp-row-meta">{{ formatSlot(i - 1) }}</span>
                </div>
                <div class="dlp-row-actions">
                  <button
                    class="ghost-btn dlp-action"
                    type="button"
                    title="载入到当前编辑草稿"
                    @click="restore(i - 1)"
                  >
                    <RotateCcw :size="13" :stroke-width="2" aria-hidden="true" />
                    载入
                  </button>
                  <button
                    class="ghost-btn dlp-action"
                    type="button"
                    title="用当前画布覆盖此布局"
                    @click="saveToSlot(i - 1)"
                  >
                    覆盖保存
                  </button>
                  <button
                    class="icon-btn dlp-delete"
                    type="button"
                    :aria-label="`删除${layout.layoutPresets.value[i - 1]?.name ?? formatSlot(i - 1)}`"
                    :title="`删除${layout.layoutPresets.value[i - 1]?.name ?? formatSlot(i - 1)}`"
                    @click="remove(i - 1)"
                  >
                    <Trash2 :size="13" :stroke-width="2" aria-hidden="true" />
                  </button>
                </div>
              </template>
              <template v-else>
                <div class="dlp-row-main">
                  <strong class="dlp-row-name">{{ formatSlot(i - 1) }}</strong>
                  <span class="dlp-row-meta">空槽位</span>
                </div>
                <button
                  class="ghost-btn dlp-action"
                  type="button"
                  :disabled="!name.trim() || i - 1 > layout.layoutPresets.value.length"
                  title="把当前画布保存到此槽位"
                  @click="saveToSlot(i - 1)"
                >
                  <Save :size="13" :stroke-width="2" aria-hidden="true" />
                  保存到此槽位
                </button>
              </template>
            </div>
          </div>

          <div class="dlp-foot">
            <button class="ghost-btn" type="button" @click="close">关闭</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dlp-card {
  width: 560px;
  max-width: calc(100vw - 32px);
  padding: 0;
}

.dlp-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 18px 20px 0;
}

.dlp-hint {
  margin: 5px 0 0;
  font-size: 0.72rem;
  line-height: 1.5;
  color: var(--text-3);
}

.dlp-save {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 16px 20px 12px;
}

.dlp-save .field-input {
  flex: 1;
  min-width: 0;
}

.dlp-save .pill-btn {
  flex-shrink: 0;
  white-space: nowrap;
}

.dlp-list {
  display: grid;
  gap: 8px;
  max-height: 260px;
  padding: 0 20px;
  overflow-y: auto;
}

.dlp-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 48px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}

.dlp-row.empty {
  border-style: dashed;
  background: transparent;
}

.dlp-row-main {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  flex: 1;
}

.dlp-row-name {
  min-width: 0;
  overflow: hidden;
  color: var(--text-1);
  font-size: 0.8rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dlp-row-meta {
  flex-shrink: 0;
  color: var(--text-4);
  font-size: 0.68rem;
}

.dlp-row-actions {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
}

.dlp-action {
  gap: 4px;
  padding: 5px 9px;
  font-size: 0.72rem;
}

.dlp-delete {
  width: 26px;
  height: 26px;
  color: var(--text-3);
}

.dlp-delete:hover {
  color: var(--c-red-ink);
}

.dlp-foot {
  display: flex;
  justify-content: flex-end;
  padding: 14px 20px 18px;
}

@media (max-width: 700px) {
  .dlp-save {
    align-items: stretch;
    flex-direction: column;
  }

  .dlp-save .pill-btn {
    justify-content: center;
  }

  .dlp-row {
    align-items: stretch;
    flex-direction: column;
  }

  .dlp-row-actions {
    justify-content: flex-end;
  }
}
</style>
