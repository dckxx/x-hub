<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { Group, Resource } from '../api/tauri'

const props = defineProps<{
  visible: boolean
  groups: readonly Group[]
  editing: Resource | null
  defaultGroupId: number | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (
    e: 'submit',
    payload: {
      id?: number
      groupId: number
      kind: 'app' | 'web'
      name: string
      target: string
      icon?: string | null
      args?: string | null
    },
  ): void
}>()

const kind = ref<'app' | 'web'>('app')
const name = ref('')
const target = ref('')
const args = ref('')
const icon = ref('')
const groupId = ref<number | null>(null)
const error = ref('')

const isEdit = computed(() => props.editing !== null)

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    if (props.editing) {
      kind.value = props.editing.kind
      name.value = props.editing.name
      target.value = props.editing.target
      args.value = props.editing.args ?? ''
      icon.value = props.editing.icon ?? ''
      groupId.value = props.editing.group_id
    } else {
      kind.value = 'app'
      name.value = ''
      target.value = ''
      args.value = ''
      icon.value = ''
      groupId.value = props.defaultGroupId ?? props.groups[0]?.id ?? null
    }
  },
)

function submit() {
  const trimmedName = name.value.trim()
  let trimmedTarget = target.value.trim()
  if (!trimmedName) {
    error.value = '请输入名称'
    return
  }
  if (!trimmedTarget) {
    error.value = kind.value === 'app' ? '请输入程序路径' : '请输入网址'
    return
  }
  if (groupId.value === null) {
    error.value = '请选择分组'
    return
  }
  // web 类型自动补全协议
  if (kind.value === 'web' && !/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmedTarget)) {
    trimmedTarget = 'https://' + trimmedTarget
  }
  emit('submit', {
    id: props.editing?.id,
    groupId: groupId.value,
    kind: kind.value,
    name: trimmedName,
    target: trimmedTarget,
    icon: icon.value.trim() || null,
    args: kind.value === 'app' ? (args.value.trim() || null) : null,
  })
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask" @click.self="emit('close')">
        <div class="modal-card form-card" role="dialog" aria-label="资源编辑">
          <h2 class="dialog-title">{{ isEdit ? '编辑资源' : '添加资源' }}</h2>

          <!-- 类型切换 -->
          <div class="kind-switch">
            <button
              class="kind-pill"
              :class="{ active: kind === 'app' }"
              @click="kind = 'app'"
            >
              本地程序
            </button>
            <button
              class="kind-pill"
              :class="{ active: kind === 'web' }"
              @click="kind = 'web'"
            >
              网页书签
            </button>
          </div>

          <!-- 名称 -->
          <label class="field-label">名称</label>
          <input
            v-model="name"
            class="field-input"
            type="text"
            maxlength="60"
            placeholder="如：VS Code / GitHub"
            @keydown="onKeydown"
          />

          <!-- 目标 -->
          <label class="field-label">{{ kind === 'app' ? '程序路径' : '网址' }}</label>
          <input
            v-model="target"
            class="field-input"
            type="text"
            :placeholder="
              kind === 'app' ? '如：C:\\Program Files\\...\\code.exe' : '如：github.com'
            "
            @keydown="onKeydown"
          />

          <!-- 启动参数（仅 app） -->
          <template v-if="kind === 'app'">
            <label class="field-label">启动参数（可选）</label>
            <input
              v-model="args"
              class="field-input"
              type="text"
              placeholder="如：--new-window"
              @keydown="onKeydown"
            />
          </template>

          <!-- 图标 -->
          <label class="field-label">图标（可选，Emoji）</label>
          <input
            v-model="icon"
            class="field-input"
            type="text"
            maxlength="8"
            placeholder="如：🚀（留空则自动生成）"
            @keydown="onKeydown"
          />

          <!-- 分组 -->
          <label class="field-label">分组</label>
          <div class="group-pills">
            <button
              v-for="g in groups"
              :key="g.id"
              class="group-pill"
              :class="{ active: groupId === g.id }"
              @click="groupId = g.id"
            >
              {{ g.name }}
            </button>
            <span v-if="groups.length === 0" class="no-group-hint">
              暂无分组，请先创建分组
            </span>
          </div>

          <p v-if="error" class="form-error">{{ error }}</p>

          <div class="dialog-actions">
            <button class="ghost-btn btn" @click="emit('close')">取消</button>
            <button class="pill-btn btn" @click="submit">
              {{ isEdit ? '保存' : '添加' }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.form-card {
  width: 420px;
  max-height: calc(100vh - 80px);
  overflow-y: auto;
}
.dialog-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-1);
  margin-bottom: 16px;
}
.kind-switch {
  display: flex;
  gap: 4px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 4px;
  margin-bottom: 16px;
}
.kind-pill {
  flex: 1;
  border: none;
  background: transparent;
  padding: 7px 0;
  border-radius: var(--radius-pill);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.kind-pill.active {
  background: var(--bg-card);
  color: var(--brand-500);
  font-weight: 600;
  box-shadow: var(--shadow-card);
}
.field-label {
  margin-top: 14px;
}
.group-pills {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 96px;
  overflow-y: auto;
}
.group-pill {
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 5px 12px;
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
  transition: all 0.15s;
}
.group-pill:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.group-pill.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.no-group-hint {
  font-size: 12px;
  color: var(--text-4);
}
.form-error {
  margin-top: 12px;
  font-size: 12px;
  color: var(--c-red);
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
}
.btn {
  padding: 7px 20px;
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
