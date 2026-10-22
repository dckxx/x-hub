<script setup lang="ts">
import { onBeforeUnmount, ref, toRef, watch } from 'vue'
import { useFocusTrap } from '../composables/useFocusTrap'
import type { ExportFormat } from '../utils/noteExport'

/**
 * 速记随手导出弹窗（单条/整夹共用，2026-10-22 #t69）：
 * 格式三选一（Markdown / HTML / PDF）+ 确定；整夹批量时切进度态（逐条 PDF 较慢，可取消）。
 * 导出动作由宿主（SpeednoteView）执行，本组件只管选格式与展示进度。
 */
const props = defineProps<{
  visible: boolean
  /** note = 单条导出；folder = 整夹导出 */
  kind: 'note' | 'folder'
  /** 笔记标题 / 文件夹名（展示用） */
  title: string
  busy: boolean
  progress: { done: number; total: number } | null
}>()

const emit = defineEmits<{ confirm: [format: ExportFormat]; cancel: []; cancelExport: [] }>()

const format = ref<ExportFormat>('markdown')

// 每次打开重置为 Markdown（最高频选择）
watch(
  () => props.visible,
  (on) => {
    if (on) format.value = 'markdown'
  },
)

const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

function onKeydown(e: KeyboardEvent) {
  if (!props.visible || props.busy) return
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('cancel')
  } else if (e.key === 'Enter') {
    e.preventDefault()
    emit('confirm', format.value)
  }
}

watch(
  () => props.visible,
  (on) => {
    if (typeof window === 'undefined') return
    if (on) window.addEventListener('keydown', onKeydown)
    else window.removeEventListener('keydown', onKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (typeof window !== 'undefined') window.removeEventListener('keydown', onKeydown)
})

const options: { value: ExportFormat; label: string; desc: string }[] = [
  { value: 'markdown', label: 'Markdown（.md）', desc: '通用笔记格式，图片存到旁边的 assets 文件夹' },
  { value: 'html', label: 'HTML（.html）', desc: '单文件网页，双击即可查看，图片内嵌其中' },
  { value: 'pdf', label: 'PDF（.pdf）', desc: '适合打印与分享，文字可选中、可搜索' },
]
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="modal-mask" @click.self="!busy && emit('cancel')">
      <div ref="cardRef" class="modal-card export-card" role="dialog" aria-modal="true">
        <template v-if="!busy">
          <h2 class="export-title">导出{{ kind === 'note' ? '笔记' : '文件夹' }}</h2>
          <p class="export-sub">{{ title }}</p>

          <div class="export-opts" role="radiogroup" aria-label="导出格式">
            <label
              v-for="opt in options"
              :key="opt.value"
              class="export-opt"
              :class="{ on: format === opt.value }"
            >
              <input v-model="format" type="radio" name="export-format" :value="opt.value" />
              <span class="export-opt-label">{{ opt.label }}</span>
              <span class="export-opt-desc">{{ opt.desc }}</span>
            </label>
          </div>

          <p class="export-hint">
            {{
              kind === 'folder'
                ? '将导出到所选目录下以文件夹名命名的子目录，每条笔记一个文件'
                : '选择保存位置后自动生成'
            }}
          </p>

          <div class="export-foot">
            <button type="button" class="export-btn" @click="emit('cancel')">取消</button>
            <button
              type="button"
              class="export-btn export-btn--primary"
              @click="emit('confirm', format)"
            >
              导出
            </button>
          </div>
        </template>

        <template v-else>
          <h2 class="export-title">正在导出…</h2>
          <p class="export-sub">{{ title }}</p>
          <div class="export-progress" role="status">
            <div class="export-progress-bar">
              <div
                class="export-progress-fill"
                :style="{ width: progress ? `${Math.round((progress.done / Math.max(progress.total, 1)) * 100)}%` : '100%' }"
              />
            </div>
            <p class="export-progress-text">
              {{ progress ? `${progress.done} / ${progress.total} 条` : '处理中…' }}
            </p>
          </div>
          <div class="export-foot">
            <button type="button" class="export-btn" @click="emit('cancelExport')">取消导出</button>
          </div>
        </template>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.export-card {
  width: min(420px, calc(100vw - 48px));
}
.export-title {
  margin: 0 0 4px;
  font-size: 1rem;
  font-weight: 650;
  color: var(--text-1);
}
.export-sub {
  margin: 0 0 14px;
  font-size: 0.8125rem;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.export-opts {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 12px;
}
.export-opt {
  display: grid;
  grid-template-columns: auto 1fr;
  grid-template-rows: auto auto;
  column-gap: 10px;
  align-items: center;
  padding: 10px 12px;
  border: 1px solid var(--border-1, rgba(128, 128, 128, 0.25));
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.export-opt:hover {
  background: var(--bg-card-soft);
}
.export-opt.on {
  border-color: var(--brand-500);
  background: var(--bg-card-soft);
}
.export-opt input {
  grid-row: 1 / span 2;
  accent-color: var(--brand-500);
}
.export-opt-label {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-1);
}
.export-opt.on .export-opt-label {
  color: var(--brand-500);
}
.export-opt-desc {
  font-size: 0.75rem;
  color: var(--text-3);
  margin-top: 2px;
}
.export-hint {
  margin: 0 0 14px;
  font-size: 0.75rem;
  color: var(--text-4);
}
.export-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.export-btn {
  padding: 6px 16px;
  border: 1px solid var(--border-1, rgba(128, 128, 128, 0.25));
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-2);
  font-size: 0.8125rem;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.export-btn:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.export-btn--primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.export-btn--primary:hover {
  background: var(--brand-600, var(--brand-500));
  color: #fff;
}
.export-progress {
  margin-bottom: 16px;
}
.export-progress-bar {
  height: 6px;
  border-radius: 999px;
  background: var(--bg-card-soft);
  overflow: hidden;
}
.export-progress-fill {
  height: 100%;
  border-radius: 999px;
  background: var(--brand-500);
  transition: width 0.25s ease;
}
.export-progress-text {
  margin: 8px 0 0;
  font-size: 0.75rem;
  color: var(--text-3);
  text-align: center;
}
</style>
