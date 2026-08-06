<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, ImagePlus } from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'

const props = defineProps<{
  visible: boolean
  editing: Resource | null
  prefill: { name?: string; target?: string; icon?: string | null; kind?: 'app' | 'web' } | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (
    e: 'submit',
    payload: {
      id?: number
      kind: 'app' | 'web' | 'file'
      name: string
      target: string
      category?: string | null
      icon?: string | null
      args?: string | null
    },
  ): void
}>()

const kind = ref<'app' | 'web' | 'file'>('app')
const name = ref('')
const target = ref('')
const args = ref('')
const icon = ref('')
const category = ref<string>(CATEGORIES[0])
const isDir = ref(false)
const error = ref('')

const isEdit = computed(() => props.editing !== null)

const isExtractedIcon = computed(() => /\.(png|jpg|jpeg|ico|gif|webp)$/i.test(icon.value))

const targetLabel = computed(() => {
  if (kind.value === 'file') return isDir.value ? '文件夹路径' : '文件路径'
  if (kind.value === 'app') return '程序路径'
  return '网址'
})

const targetPlaceholder = computed(() => {
  if (kind.value === 'file') return '选择要链接的文件或文件夹'
  if (kind.value === 'app') return '如：C:\\Program Files\\...\\code.exe'
  return '如：github.com'
})

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    if (props.editing) {
      kind.value = props.editing.kind === 'file' ? 'file' : props.editing.kind
      name.value = props.editing.name
      target.value = props.editing.target
      args.value = props.editing.args ?? ''
      icon.value = props.editing.icon ?? ''
      category.value = props.editing.category ?? '其他'
      isDir.value = props.editing.category === '文件夹'
    } else {
      kind.value = 'app'
      name.value = ''
      target.value = ''
      args.value = ''
      icon.value = ''
      category.value = '其他'
      isDir.value = false
      if (props.prefill) {
        kind.value = props.prefill.kind ?? 'app'
        name.value = props.prefill.name ?? ''
        target.value = props.prefill.target ?? ''
        icon.value = props.prefill.icon ?? ''
      }
    }
  },
)

async function pickTarget() {
  if (!isTauri()) return
  if (kind.value === 'file') {
    try {
      const file = await open({
        multiple: false,
        directory: isDir.value,
        filters: isDir.value
          ? undefined
          : [{ name: '所有文件', extensions: ['*'] }],
      })
      if (typeof file !== 'string') return
      target.value = file
      name.value = file.split(/[\\/]/).pop() ?? ''
      category.value = categorize(file, isDir.value)
    } catch (e) {
      error.value = String(e)
    }
    return
  }
  if (kind.value === 'app') {
    const file = await open({
      multiple: false,
      directory: false,
      filters: [{ name: '程序', extensions: ['exe', 'lnk'] }],
    })
    if (typeof file !== 'string') return
    target.value = file
    try {
      const info = await tauriApi.parseDroppedPath(file)
      if (!name.value.trim()) name.value = info.name
      if (!icon.value.trim()) icon.value = info.icon ?? ''
    } catch {
      // 解析失败时仅保留手动填写的路径
    }
  }
}

async function pickIcon() {
  if (!isTauri()) return
  const file = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: '图标', extensions: ['ico', 'png', 'jpg', 'jpeg', 'webp'] },
    ],
  })
  if (typeof file !== 'string') return
  try {
    const imported = await tauriApi.importIconFile(file)
    if (imported) icon.value = imported
  } catch (e) {
    error.value = String(e)
  }
}

function submit() {
  const trimmedName = name.value.trim()
  let trimmedTarget = target.value.trim()
  if (!trimmedName) {
    error.value = '请输入名称'
    return
  }
  if (!trimmedTarget) {
    if (kind.value === 'file') error.value = '请选择文件或文件夹'
    else if (kind.value === 'app') error.value = '请输入程序路径'
    else error.value = '请输入网址'
    return
  }
  // web 类型自动补全协议
  if (kind.value === 'web' && !/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmedTarget)) {
    trimmedTarget = 'https://' + trimmedTarget
  }
  emit('submit', {
    id: props.editing?.id,
    kind: kind.value,
    name: trimmedName,
    target: trimmedTarget,
    category: kind.value === 'file' ? category.value : null,
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
      <div v-if="visible" class="modal-mask">
        <div class="modal-card form-card" role="dialog" aria-label="速达资源编辑">
          <h2 class="dialog-title">{{ isEdit ? '编辑' : '添加' }}</h2>

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
            <button
              class="kind-pill"
              :class="{ active: kind === 'file' }"
              @click="kind = 'file'"
            >
              文件/文件夹
            </button>
          </div>

          <!-- 名称 -->
          <label class="field-label">名称</label>
          <input
            v-model="name"
            class="field-input"
            type="text"
            maxlength="80"
            :placeholder="kind === 'file' ? '自动取文件名' : '如：VS Code / GitHub'"
            @keydown="onKeydown"
          />

          <!-- 目标 -->
          <label class="field-label">{{ targetLabel }}</label>
          <div class="input-with-btn">
            <input
              v-model="target"
              class="field-input"
              type="text"
              :readonly="kind === 'file'"
              :placeholder="targetPlaceholder"
              @keydown="onKeydown"
            />
            <button
              class="input-btn"
              :title="kind === 'web' ? '手动输入网址' : '选择'"
              @click="pickTarget"
            >
              <FolderOpen :size="15" :stroke-width="1.8" />
            </button>
          </div>

          <!-- 文件/文件夹切换（仅 file 类型） -->
          <div v-if="kind === 'file'" class="dir-toggle">
            <button
              class="kind-pill"
              :class="{ active: isDir }"
              @click="isDir = true"
            >
              文件夹
            </button>
            <button
              class="kind-pill"
              :class="{ active: !isDir }"
              @click="isDir = false"
            >
              文件
            </button>
          </div>

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

          <!-- 图标（app/web） -->
          <template v-if="kind !== 'file'">
            <label class="field-label">图标（可选）</label>
            <div class="icon-row">
              <input
                v-model="icon"
                class="field-input"
                type="text"
                maxlength="260"
                placeholder="Emoji 或留空自动生成"
                @keydown="onKeydown"
              />
              <button class="input-btn" title="选择本地图标" @click="pickIcon">
                <ImagePlus :size="15" :stroke-width="1.8" />
              </button>
              <span v-if="isExtractedIcon" class="extracted-badge" title="已从文件导入图标">
                ✓ 已导入
              </span>
            </div>
          </template>

          <!-- 分类（仅 file） -->
          <template v-if="kind === 'file'">
            <label class="field-label">分类</label>
            <div class="cat-pills">
              <button
                v-for="c in CATEGORIES"
                :key="c"
                class="cat-pill"
                :class="{ active: category === c }"
                @click="category = c"
              >
                {{ c }}
              </button>
            </div>
            <p class="link-hint">🔗 仅创建链接，源文件保留在原位置</p>
          </template>

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
.icon-row {
  position: relative;
}
.input-with-btn {
  position: relative;
}
.input-with-btn .field-input {
  padding-right: 40px;
}
.input-btn {
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  width: 28px;
  height: 28px;
  border: none;
  background: var(--bg-card-soft);
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.input-btn:hover {
  background: var(--brand-50);
  color: var(--brand-500);
}
.icon-row .field-input {
  padding-right: 88px;
}
.icon-row .input-btn {
  right: 62px;
}
.extracted-badge {
  position: absolute;
  right: 10px;
  top: 50%;
  transform: translateY(-50%);
  font-size: 11px;
  font-weight: 600;
  color: var(--c-green);
  background: var(--c-green-soft);
  padding: 2px 8px;
  border-radius: var(--radius-pill);
  pointer-events: none;
}
.dir-toggle {
  display: flex;
  gap: 4px;
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 4px;
  margin-top: 12px;
}
.cat-pills {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.cat-pill {
  border: 1px solid var(--border-soft);
  background: var(--bg-card-soft);
  border-radius: var(--radius-pill);
  padding: 5px 12px;
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
  transition: all 0.15s;
}
.cat-pill:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.cat-pill.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
}
.link-hint {
  margin-top: 14px;
  font-size: 12px;
  color: var(--text-3);
}
.form-error {
  margin-top: 10px;
  font-size: 12px;
  color: var(--c-red);
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 18px;
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
