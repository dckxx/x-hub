<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { inject } from 'vue'
import { Check, ChevronDown, ChevronUp, RotateCcw } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import {
  DEFAULT_SYSMON_METRICS,
  resolveSysmonMetrics,
  SYSMON_METRIC_DEFS,
} from '../utils/sysmonMetrics'

/**
 * 工作台「系统资源」卡片显示设置弹窗（入口：布局编辑器 sysmon 卡片格控制钮排）：
 * 勾选要显示的指标 + 行内 ↑↓ 排序 + 恢复默认。确定走 store.setSysMonitorMetrics
 * （config 整体落盘，真卡与编辑器缩印读同一配置自动刷新）；取消/遮罩关闭不保存。
 */
const props = defineProps<{ visible: boolean }>()

const emit = defineEmits<{ (e: 'close'): void }>()

const store = useStore()
const showToast = inject<(msg: string) => void>('showToast')

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

// ---- 草稿态：打开时从当前生效配置快照（空配置 = 内置默认，与卡片 resolve 口径一致） ----
const draft = ref<string[]>([])

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    draft.value = resolveSysmonMetrics(store.state.config.sys_monitor_metrics).map((m) => m.id)
  },
  { immediate: true },
)

// 显示区按草稿顺序，未勾选项按目录序排后
const shown = computed(() =>
  draft.value.flatMap((id) => {
    const def = SYSMON_METRIC_DEFS.find((d) => d.id === id)
    return def ? [def] : []
  }),
)
const hidden = computed(() => SYSMON_METRIC_DEFS.filter((d) => !draft.value.includes(d.id)))

function toggle(id: string) {
  const i = draft.value.indexOf(id)
  if (i >= 0) draft.value.splice(i, 1)
  else draft.value.push(id)
}

function move(index: number, delta: number) {
  const target = index + delta
  if (target < 0 || target >= draft.value.length) return
  const arr = [...draft.value]
  ;[arr[index], arr[target]] = [arr[target], arr[index]]
  draft.value = arr
}

/** 就地重置草稿为内置默认（不打断编辑，确定才落盘） */
function restoreDefault() {
  draft.value = [...DEFAULT_SYSMON_METRICS]
}

function save() {
  store
    .setSysMonitorMetrics([...draft.value])
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
        class="modal-card ssd-dialog"
        role="dialog"
        aria-label="系统资源显示设置"
        aria-modal="true"
      >
        <h2 class="dialog-title">系统资源 · 显示设置</h2>
        <p class="ssd-sub">勾选卡片要显示的指标，勾选项按列表顺序排布（宽卡片一行可排多项）</p>

        <div class="ssd-list">
          <p class="ssd-group-label">显示中 · 顺序即卡片排布</p>
          <div v-for="(d, i) in shown" :key="d.id" class="ssd-row">
            <button
              class="ssd-check on"
              type="button"
              :aria-label="`隐藏${d.label}`"
              @click="toggle(d.id)"
            >
              <Check :size="12" :stroke-width="2.4" aria-hidden="true" />
            </button>
            <span class="ssd-order">{{ i + 1 }}</span>
            <span class="ssd-name">{{ d.label }}</span>
            <span class="ssd-move">
              <button
                class="ssd-move-btn"
                type="button"
                :disabled="i === 0"
                :aria-label="`上移${d.label}`"
                @click="move(i, -1)"
              >
                <ChevronUp :size="13" :stroke-width="2" aria-hidden="true" />
              </button>
              <button
                class="ssd-move-btn"
                type="button"
                :disabled="i === shown.length - 1"
                :aria-label="`下移${d.label}`"
                @click="move(i, 1)"
              >
                <ChevronDown :size="13" :stroke-width="2" aria-hidden="true" />
              </button>
            </span>
          </div>
          <p v-if="shown.length === 0" class="ssd-none">尚未选择任何指标，卡片将没有内容可展示</p>

          <template v-if="hidden.length > 0">
            <p class="ssd-group-label">未显示</p>
            <div v-for="d in hidden" :key="d.id" class="ssd-row">
              <button
                class="ssd-check"
                type="button"
                :aria-label="`显示${d.label}`"
                @click="toggle(d.id)"
              ></button>
              <span class="ssd-name">{{ d.label }}</span>
            </div>
          </template>
        </div>

        <div class="ssd-footer">
          <button class="ssd-btn ghost" type="button" @click="restoreDefault">
            <RotateCcw :size="13" :stroke-width="2" aria-hidden="true" />
            恢复默认
          </button>
          <span class="ssd-spacer"></span>
          <button class="ssd-btn" type="button" @click="emit('close')">取消</button>
          <button
            class="ssd-btn primary"
            type="button"
            :disabled="draft.length === 0"
            @click="save"
          >
            确定
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ssd-dialog {
  width: 380px;
  display: flex;
  flex-direction: column;
}
.ssd-sub {
  margin: -6px 0 14px;
  font-size: 0.75rem;
  color: var(--text-3);
}
.ssd-list {
  max-height: 320px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-right: 2px;
}
.ssd-group-label {
  margin: 8px 0 4px;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-4);
}
.ssd-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border-radius: var(--radius-sm);
  transition: background 0.12s;
}
.ssd-row:hover {
  background: var(--bg-card-soft);
}
.ssd-check {
  width: 18px;
  height: 18px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1.5px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: transparent;
  color: #fff;
  cursor: pointer;
  padding: 0;
  font-family: inherit;
  transition: background 0.12s, border-color 0.12s;
}
.ssd-check:hover {
  border-color: var(--brand-500);
}
.ssd-check.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
}
.ssd-order {
  flex: none;
  width: 16px;
  font-size: 0.625rem;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
  text-align: center;
}
.ssd-name {
  flex: 1;
  min-width: 0;
  font-size: 0.8125rem;
  color: var(--text-1);
}
.ssd-move {
  display: inline-flex;
  gap: 2px;
}
.ssd-move-btn {
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  padding: 0;
  font-family: inherit;
  transition: background 0.12s, color 0.12s;
}
.ssd-move-btn:hover:not(:disabled) {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.ssd-move-btn:disabled {
  opacity: 0.35;
  cursor: default;
}
.ssd-none {
  margin: 0;
  padding: 10px 8px;
  font-size: 0.75rem;
  color: var(--text-4);
}
.ssd-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 16px;
}
.ssd-spacer {
  flex: 1;
}
.ssd-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
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
.ssd-btn:hover {
  background: var(--bg-card-soft);
}
.ssd-btn.ghost {
  color: var(--text-3);
}
.ssd-btn.primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.ssd-btn.primary:hover {
  background: var(--brand-600);
}
.ssd-btn.primary:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
