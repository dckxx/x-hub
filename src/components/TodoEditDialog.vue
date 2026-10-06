<script setup lang="ts">
import { computed, inject, ref, toRef, watch } from 'vue'
import { Plus, Trash2, X } from 'lucide-vue-next'
import { useStore } from '../stores/workbench'
import { useFocusTrap } from '../composables/useFocusTrap'
import type { RepeatEndMode, RepeatMode, RepeatUnit, Todo } from '../api/tauri'
import { addDays, nextMonday, repeatEndLabel, repeatLabel } from '../utils/todoSchedule'
import TodoDateTimeField from './TodoDateTimeField.vue'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'

/**
 * 待办编辑弹层：标题 / 描述（轻量 Markdown）/ 标签 / 置顶 / 周期 / 截止与提醒。
 * 周期规则的**权威解释在 Rust 侧**（`todo_recurrence.rs`），这里只收集参数并落库；
 * 预览文案由 `repeatLabel` 生成，与后端规则保持同一套命名。
 */
const props = defineProps<{
  visible: boolean
  /** null = 新建（此时用 presetDueMs 预填截止） */
  todo: Todo | null
  presetDueMs?: number | null
}>()

const emit = defineEmits<{ close: []; saved: [] }>()

const store = useStore()
/** 轻提示（宿主提供；独立使用时为 noop） */
const showToast = inject<(msg: string, action?: { label: string; onClick: () => void }) => void>(
  'showToast',
  () => {},
)

/** 焦点陷阱：Tab 循环留在弹层内（与其它弹窗同一口径） */
const cardRef = ref<HTMLElement | null>(null)
useFocusTrap(toRef(props, 'visible'), cardRef)

const title = ref('')
/** 描述长度上限：与后端 repo::todo::MAX_DESCRIPTION_LEN 对齐（超了后端会报错拒写） */
const DESC_MAX = 200
const description = ref('')
const pinned = ref(false)
const tagIds = ref<number[]>([])
const repeatMode = ref<RepeatMode>('once')
const repeatEvery = ref(1)
const repeatUnit = ref<RepeatUnit>('day')
const weekdayMask = ref(0)
const monthDay = ref(1)
const monthNthMode = ref<'day' | 'nth'>('day')
const monthNth = ref(1)
const monthNthWeekday = ref(0)
const endMode = ref<RepeatEndMode>('never')
const endAtMs = ref<number | null>(null)
const repeatCount = ref(1)
const dueMs = ref<number | null>(null)
const remindMs = ref<number | null>(null)
const duePresets = ref<Array<{ label: string; ms: number }>>([])
const newTagName = ref('')
const busy = ref(false)

const WEEKDAYS = ['一', '二', '三', '四', '五', '六', '日'] as const
const PRESET_MODES: Array<{ mode: RepeatMode; label: string }> = [
  { mode: 'once', label: '不重复' },
  { mode: 'daily', label: '每天' },
  { mode: 'weekdays', label: '每个工作日' },
  { mode: 'weekly', label: '每周' },
  { mode: 'monthly', label: '每月' },
  { mode: 'yearly', label: '每年' },
  { mode: 'custom', label: '自定义…' },
]

// 下拉一律用系统通用组件 AppSelect（见 DESIGN.md §5「下拉」），不用原生 <select>：
// 原生下拉的展开列表由系统绘制，配色/圆角/字号都不受主题控制。
const REPEAT_UNIT_OPTIONS: AppSelectOption[] = [
  { value: 'day', label: '天' },
  { value: 'week', label: '周' },
  { value: 'month', label: '个月' },
  { value: 'year', label: '年' },
]
const MONTH_NTH_MODE_OPTIONS: AppSelectOption[] = [
  { value: 'day', label: '每月第几天' },
  { value: 'nth', label: '每月第几个星期几' },
]
const MONTH_NTH_OPTIONS: AppSelectOption[] = [
  { value: '1', label: '第 1 个' },
  { value: '2', label: '第 2 个' },
  { value: '3', label: '第 3 个' },
  { value: '4', label: '第 4 个' },
  { value: '5', label: '第 5 个' },
  { value: '-1', label: '最后一个' },
]
const WEEKDAY_SELECT_OPTIONS: AppSelectOption[] = WEEKDAYS.map((w, i) => ({
  value: String(i),
  label: `星期${w}`,
}))
const END_MODE_OPTIONS: AppSelectOption[] = [
  { value: 'never', label: '永不结束' },
  { value: 'until', label: '到某日结束' },
  { value: 'count', label: '共 N 次' },
]

/** 默认截止：预设日期（新建）或 23:59 */
function defaultDue(): number | null {
  if (props.presetDueMs == null) return null
  const d = new Date(props.presetDueMs)
  d.setHours(23, 59, 0, 0)
  return d.getTime()
}

/** 某天 23:59 的时刻戳（截止快捷时间统一落在当天结束） */
function endOfDay(d: Date): number {
  const x = new Date(d)
  x.setHours(23, 59, 0, 0)
  return x.getTime()
}

/** 某天 hh:mm 的时刻戳（提醒快捷时间） */
function atTime(d: Date, h: number, m: number): number {
  const x = new Date(d)
  x.setHours(h, m, 0, 0)
  return x.getTime()
}

/** 本周六（今天是周六就是今天，周日则顺延到下周） */
function thisSaturday(from: Date): Date {
  const idx = (from.getDay() + 6) % 7 // 周一=0 … 周日=6
  return addDays(from, (5 - idx + 7) % 7)
}

/** 截止快捷时间：今天 / 明天 / 本周六 / 下周一 / 本月末 / 下月初，都落在当天 23:59 */
function buildDuePresets(): Array<{ label: string; ms: number }> {
  const now = new Date()
  return [
    { label: '今天', ms: endOfDay(now) },
    { label: '明天', ms: endOfDay(addDays(now, 1)) },
    { label: '本周六', ms: endOfDay(thisSaturday(now)) },
    { label: '下周一', ms: endOfDay(nextMonday(now)) },
    { label: '本月末', ms: endOfDay(new Date(now.getFullYear(), now.getMonth() + 1, 0)) },
    { label: '下月初', ms: endOfDay(new Date(now.getFullYear(), now.getMonth() + 1, 1)) },
  ]
}

/** 提醒快捷时间：跟着截止走的三档 + 两个绝对时刻 */
const remindPresets = computed(() => {
  const list: Array<{ label: string; ms: number }> = []
  if (dueMs.value != null) {
    list.push({ label: '同截止', ms: dueMs.value })
    list.push({ label: '提前 1 小时', ms: dueMs.value - 3_600_000 })
    list.push({ label: '提前 1 天', ms: dueMs.value - 86_400_000 })
  }
  const now = new Date()
  list.push({ label: '今天 09:00', ms: atTime(now, 9, 0) })
  list.push({ label: '明天 09:00', ms: atTime(addDays(now, 1), 9, 0) })
  return list
})

watch(
  () => props.visible,
  (on) => {
    if (!on) return
    const t = props.todo
    title.value = t?.title ?? ''
    description.value = t?.description ?? ''
    pinned.value = t?.pinned ?? false
    tagIds.value = t ? store.todoTagIds(t.id) : []
    repeatMode.value = (t?.repeat_mode as RepeatMode) ?? 'once'
    repeatEvery.value = t?.repeat_every ?? 1
    repeatUnit.value = (t?.repeat_unit as RepeatUnit) ?? 'day'
    weekdayMask.value = t?.repeat_weekdays ?? 0
    monthNthMode.value = t?.repeat_month_nth != null ? 'nth' : 'day'
    monthDay.value = t?.repeat_month_day ?? 1
    monthNth.value = t?.repeat_month_nth ?? 1
    monthNthWeekday.value = t?.repeat_weekdays ? Math.log2(t.repeat_weekdays & -t.repeat_weekdays) : 0
    endMode.value = (t?.repeat_end_mode as RepeatEndMode) ?? 'never'
    endAtMs.value = t?.repeat_end_at ?? null
    repeatCount.value = t?.repeat_count ?? 1
    dueMs.value = t ? t.due_at : defaultDue()
    remindMs.value = t ? t.remind_at : null
    duePresets.value = buildDuePresets()
    newTagName.value = ''
  },
)

const allTags = computed(() => store.state.todoTags)

/** 当前规则的可读预览（结束条件另附） */
const repeatPreview = computed(() => {
  const end = repeatEndLabel({
    repeat_end_mode: endMode.value === 'never' ? null : endMode.value,
    repeat_end_at: endAtMs.value,
    repeat_count: repeatCount.value,
  })
  const rule = {
    repeat_mode: repeatMode.value,
    repeat_every: repeatMode.value === 'custom' ? repeatEvery.value : null,
    repeat_unit: repeatMode.value === 'custom' ? repeatUnit.value : null,
    repeat_weekdays: weekdaysForSave(),
    repeat_month_day: repeatMode.value === 'monthly' && monthNthMode.value === 'day' ? monthDay.value : null,
    repeat_month_nth: repeatMode.value === 'monthly' && monthNthMode.value === 'nth' ? monthNth.value : null,
    due_at: dueMs.value,
  }
  const base = repeatLabel(rule)
  return base ? `${base}${end ? ` · ${end}` : ''}` : ''
})

function weekdaysForSave(): number | null {
  if (repeatMode.value === 'weekly') return weekdayMask.value || null
  if (repeatMode.value === 'custom' && repeatUnit.value === 'week') return weekdayMask.value || null
  if (repeatMode.value === 'monthly' && monthNthMode.value === 'nth') {
    return 1 << monthNthWeekday.value
  }
  return null
}

function toggleWeekday(i: number) {
  weekdayMask.value ^= 1 << i
}

const canSave = computed(() => title.value.trim() !== '' && !busy.value)

const repeatInvalid = computed(() => {
  // 周期规则的基准时刻就是「截止」的时分（Rust 侧按它滚动）。没有截止时间时后端
  // 既算不出下一轮、也拒绝「完成本轮」——勾选会永远失败，所以这里先拦住。
  if (repeatMode.value !== 'once' && dueMs.value == null)
    return '周期待办需要先设置「截止」时间（作为重复的基准时刻）'
  if (repeatMode.value === 'weekly' && weekdayMask.value === 0) return '每周至少要选一天'
  if (repeatMode.value === 'custom' && repeatEvery.value < 1) return '间隔至少为 1'
  if (repeatMode.value === 'custom' && repeatUnit.value === 'week' && weekdayMask.value === 0)
    return '按周重复至少要选一天'
  if (repeatMode.value === 'monthly' && monthNthMode.value === 'day' && (monthDay.value < 1 || monthDay.value > 31))
    return '每月日期取 1-31'
  if (endMode.value === 'until' && endAtMs.value == null) return '请选择结束日期'
  if (endMode.value === 'count' && repeatCount.value < 1) return '总次数至少为 1'
  return ''
})

async function addTag() {
  const name = newTagName.value.trim()
  if (!name) return
  try {
    const tag = await store.createTodoTag(name)
    if (tag && !tagIds.value.includes(tag.id)) tagIds.value.push(tag.id)
    newTagName.value = ''
  } catch {
    showToast('标签没能创建，请重试')
  }
}

function toggleTag(id: number) {
  const i = tagIds.value.indexOf(id)
  if (i >= 0) tagIds.value.splice(i, 1)
  else tagIds.value.push(id)
}

async function save() {
  const name = title.value.trim()
  if (!name) return
  busy.value = true
  try {
    const due = dueMs.value
    const remind = remindMs.value
    let id = props.todo?.id ?? null
    if (id == null) {
      const created = await store.createTodo(name)
      id = created.id
    } else {
      await store.updateTodo(id, name, props.todo?.priority ?? 0)
    }
    await store.setTodoDescription(id, description.value)
    await store.setTodoPinned(id, pinned.value)
    await store.setTodoTags(id, [...tagIds.value])
    await store.setTodoRepeat(id, {
      mode: repeatMode.value,
      every: repeatMode.value === 'custom' ? repeatEvery.value : null,
      unit: repeatMode.value === 'custom' ? repeatUnit.value : null,
      weekdays: weekdaysForSave(),
      month_day: repeatMode.value === 'monthly' && monthNthMode.value === 'day' ? monthDay.value : null,
      month_nth: repeatMode.value === 'monthly' && monthNthMode.value === 'nth' ? monthNth.value : null,
      end_mode: endMode.value === 'never' ? null : endMode.value,
      end_at: endMode.value === 'until' ? endAtMs.value : null,
      count: endMode.value === 'count' ? repeatCount.value : null,
    })
    await store.scheduleTodo(id, due, remind)
    emit('saved')
    emit('close')
  } catch {
    // 这几步是串行写入（建条目 → 描述 → 置顶 → 标签 → 周期 → 排期），中途失败会留下半成品：
    // 至少要让用户知道没存完，而不是弹层静默不动
    showToast('保存失败，部分改动可能没生效，请重试')
  } finally {
    busy.value = false
  }
}

async function remove() {
  if (props.todo == null) return
  try {
    await store.deleteTodo(props.todo.id)
    emit('saved')
    emit('close')
  } catch {
    showToast('删除失败，请重试')
  }
}
</script>

<template>
  <Teleport to="body">
    <!-- 点遮罩不关：编辑项多（标签/周期/时刻字段），误触遮罩丢整表输入，只能走右上 × / 取消 -->
    <div v-if="visible" class="modal-mask">
      <div ref="cardRef" class="modal-card te-card" role="dialog" aria-modal="true" aria-label="编辑待办">
        <div class="te-head">
          <h2 class="te-title">{{ props.todo ? '编辑待办' : '新建待办' }}</h2>
          <button class="te-close" type="button" aria-label="关闭" @click="emit('close')">
            <X :size="15" :stroke-width="2" />
          </button>
        </div>

        <label class="te-field">
          <span class="te-lab">标题</span>
          <input v-model="title" class="te-input" placeholder="要做什么？" @keydown.enter="save" />
        </label>

        <label class="te-field">
          <span class="te-lab">
            描述 <i class="te-opt">轻量 Markdown · 勾选一律用子待办</i>
            <!-- 超长会被后端拒（最多 20k 字），这里直接卡住并给出计数 -->
            <i v-if="description.length > DESC_MAX * 0.8" class="te-count">
              {{ description.length }}/{{ DESC_MAX }}
            </i>
          </span>
          <textarea
            v-model="description"
            class="te-input te-textarea"
            rows="3"
            :maxlength="DESC_MAX"
            placeholder="补充说明（可选，鼠标移到列表里的这一行可查看）"
          ></textarea>
        </label>

        <div class="te-field">
          <span class="te-lab">标签 <i class="te-opt">待办专属，与笔记标签互不影响</i></span>
          <div class="te-chips">
            <button
              v-for="tag in allTags"
              :key="tag.id"
              type="button"
              class="te-chip"
              :class="{ on: tagIds.includes(tag.id) }"
              @click="toggleTag(tag.id)"
            >
              <i class="te-dot" :style="{ background: tag.color || 'var(--brand-500)' }"></i>{{ tag.name }}
            </button>
            <span class="te-newtag">
              <input
                v-model="newTagName"
                class="te-input te-newtag-input"
                placeholder="+ 新建标签"
                @keydown.enter.prevent="addTag"
              />
              <button v-if="newTagName.trim()" type="button" class="te-mini" @click="addTag">
                <Plus :size="12" :stroke-width="2" />
              </button>
            </span>
          </div>
        </div>

        <label class="te-inline">
          <input v-model="pinned" type="checkbox" />
          <span>置顶（与日期无关，固定在列表最顶部「置顶」区）</span>
        </label>

        <div class="te-field">
          <span class="te-lab">周期 <i class="te-opt">完成即滚到下一轮，不逐次留历史</i></span>
          <div class="te-chips">
            <button
              v-for="p in PRESET_MODES"
              :key="p.mode"
              type="button"
              class="te-chip"
              :class="{ on: repeatMode === p.mode }"
              @click="repeatMode = p.mode"
            >
              {{ p.label }}
            </button>
          </div>

          <div v-if="repeatMode === 'weekly' || (repeatMode === 'custom' && repeatUnit === 'week')" class="te-chips te-weekdays">
            <button
              v-for="(w, i) in WEEKDAYS"
              :key="w"
              type="button"
              class="te-chip te-wd"
              :class="{ on: weekdayMask & (1 << i) }"
              @click="toggleWeekday(i)"
            >
              {{ w }}
            </button>
          </div>

          <div v-if="repeatMode === 'custom'" class="te-row2">
            <div class="te-inline">
              <span class="te-sub">每</span>
              <input v-model.number="repeatEvery" type="number" min="1" max="99" class="te-input te-num" />
              <AppSelect
                class="te-select"
                :model-value="repeatUnit"
                :options="REPEAT_UNIT_OPTIONS"
                aria-label="重复单位"
                @update:model-value="repeatUnit = $event as RepeatUnit"
              />
            </div>
          </div>

          <div v-if="repeatMode === 'monthly'" class="te-row2">
            <AppSelect
              class="te-select"
              :model-value="monthNthMode"
              :options="MONTH_NTH_MODE_OPTIONS"
              aria-label="每月方式"
              @update:model-value="monthNthMode = $event as 'day' | 'nth'"
            />
            <template v-if="monthNthMode === 'day'">
              <input v-model.number="monthDay" type="number" min="1" max="31" class="te-input te-num" />
              <span class="te-sub">日（超出当月天数时取当月最后一天）</span>
            </template>
            <template v-else>
              <AppSelect
                class="te-select"
                :model-value="String(monthNth)"
                :options="MONTH_NTH_OPTIONS"
                aria-label="第几个星期几"
                @update:model-value="monthNth = Number($event)"
              />
              <AppSelect
                class="te-select"
                :model-value="String(monthNthWeekday)"
                :options="WEEKDAY_SELECT_OPTIONS"
                aria-label="星期几"
                @update:model-value="monthNthWeekday = Number($event)"
              />
            </template>
          </div>

          <div v-if="repeatMode !== 'once'" class="te-row2">
            <AppSelect
              class="te-select"
              :model-value="endMode"
              :options="END_MODE_OPTIONS"
              aria-label="结束条件"
              @update:model-value="endMode = $event as RepeatEndMode"
            />
            <input v-if="endMode === 'count'" v-model.number="repeatCount" type="number" min="1" class="te-input te-num" />
          </div>

          <TodoDateTimeField
            v-if="repeatMode !== 'once' && endMode === 'until'"
            v-model="endAtMs"
            label="结束日期"
            :presets="[]"
            date-only
            hint="含当天：结束日的实例仍会出现，之后不再滚动。"
          />

          <p v-if="repeatPreview" class="te-preview">规则：{{ repeatPreview }}</p>
          <p v-if="repeatInvalid" class="te-error">{{ repeatInvalid }}</p>
          <p v-if="repeatMode !== 'once'" class="te-opt">
            基准时刻取截止时间的时分；逾期未完成不补做历史，直接滚到最近一个未过的周期。
          </p>
        </div>

        <TodoDateTimeField
          v-model="dueMs"
          label="截止"
          :presets="duePresets"
        />
        <TodoDateTimeField
          v-model="remindMs"
          label="提醒"
          :presets="remindPresets"
          hint="不设提醒即不通知；到点后由后端弹系统通知。"
        />

        <div class="te-foot">
          <button v-if="props.todo" type="button" class="te-btn te-btn--danger" @click="remove">
            <Trash2 :size="13" :stroke-width="2" />删除
          </button>
          <span class="te-spacer"></span>
          <button type="button" class="te-btn" @click="emit('close')">取消</button>
          <button
            type="button"
            class="te-btn te-btn--primary"
            :disabled="!canSave || !!repeatInvalid"
            @click="save"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.te-card {
  width: 520px;
  max-height: calc(100vh - 80px);
  overflow: auto;
  padding: 18px 20px;
}
.te-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
}
.te-title {
  margin: 0;
  font-size: 0.92rem;
  font-weight: 700;
  color: var(--text-1);
}
.te-close {
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  display: inline-flex;
  padding: 4px;
  border-radius: var(--radius-sm);
}
.te-close:hover {
  background: var(--bg-card-soft);
  color: var(--text-1);
}
.te-field {
  display: block;
  margin-bottom: 12px;
}
.te-lab {
  display: block;
  margin-bottom: 5px;
  font-size: 0.72rem;
  font-weight: 700;
  color: var(--text-3);
}
.te-opt {
  font-style: normal;
  font-weight: 400;
  color: var(--text-4);
}
/* 描述接近上限时的计数（接近才显示，平时不占视觉） */
.te-count {
  float: right;
  font-style: normal;
  font-weight: 500;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}
.te-sub {
  font-size: 0.72rem;
  color: var(--text-4);
}
.te-input {
  width: 100%;
  padding: 6px 9px;
  font-size: 0.78rem;
  font-family: inherit;
  color: var(--text-1);
  background: var(--bg-card-soft);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  outline: none;
}
.te-input:focus {
  border-color: var(--brand-500);
}
.te-textarea {
  resize: vertical;
  line-height: 1.6;
}
.te-num {
  width: 70px;
  flex: 0 0 auto;
}
/* AppSelect 触发器：贴合弹层输入框的紧凑档（覆盖其默认 38px 高度）。
   AppSelect 的根是 fragment（触发器 + Teleport），父级 scoped class 落不到触发器上
   （Vue 只把父 scope id 给单一根元素），所以必须用 :deep() 穿透，见 DESIGN.md §5。 */
.te-row2 :deep(.te-select) {
  min-height: 0;
  padding: 6px 9px;
  font-size: 0.78rem;
  background: var(--bg-card-soft);
  width: auto;
  flex: 0 0 auto;
}
.te-chips {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.te-weekdays {
  margin-top: 6px;
}
.te-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 10px;
  font-size: 0.72rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-pill);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
}
.te-chip.on {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
  font-weight: 600;
}
.te-wd {
  min-width: 28px;
  justify-content: center;
}
.te-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
}
.te-newtag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.te-newtag-input {
  width: 110px;
}
.te-mini {
  display: inline-flex;
  align-items: center;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-sm);
  background: var(--bg-card-soft);
  color: var(--text-3);
  cursor: pointer;
  padding: 3px 5px;
}
.te-inline {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.75rem;
  color: var(--text-2);
}
.te-row2 {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}
.te-row2 > .te-field {
  flex: 1 1 180px;
  margin-bottom: 0;
}
.te-preview {
  margin: 8px 0 0;
  font-size: 0.72rem;
  color: var(--c-blue-ink);
}
.te-error {
  margin: 6px 0 0;
  font-size: 0.72rem;
  color: var(--c-red-ink);
}
.te-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 16px;
}
.te-spacer {
  flex: 1;
}
.te-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 6px 14px;
  font-size: 0.78rem;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-2);
  cursor: pointer;
}
.te-btn:hover {
  color: var(--text-1);
  border-color: var(--border-strong);
}
.te-btn--primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
  font-weight: 600;
}
.te-btn--primary:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.te-btn--danger {
  color: var(--c-red-ink);
  border-color: var(--c-red-soft);
}
</style>
