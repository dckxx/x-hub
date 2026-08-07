<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, ImagePlus, Link } from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'
import { useFocusTrap } from '../composables/useFocusTrap'
import { deriveFaviconUrl, joinWebTarget, splitWebTarget, type WebScheme } from '../utils/web'

const props = defineProps<{
  visible: boolean
  editing: Resource | null
  prefill: {
    name?: string
    target?: string
    icon?: string | null
    kind?: 'app' | 'web' | 'file'
    category?: string | null
    isDir?: boolean
  } | null
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
const webScheme = ref<WebScheme>('https')
const args = ref('')
const icon = ref('')
const category = ref<string>(CATEGORIES[0])
const isDir = ref(false)
const error = ref('')
const cardRef = ref<HTMLElement | null>(null)
const nameInputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, nameInputRef)

const isEdit = computed(() => props.editing !== null)

const isExtractedIcon = computed(() => /\.(png|jpg|jpeg|ico|gif|webp)$/i.test(icon.value))
const showWebIconInput = computed(() => kind.value !== 'web')

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
      if (props.editing.kind === 'web') {
        const split = splitWebTarget(props.editing.target)
        webScheme.value = split.scheme
        target.value = split.value
      } else {
        target.value = props.editing.target
      }
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
      webScheme.value = 'https'
      if (props.prefill) {
        kind.value = props.prefill.kind ?? 'app'
        name.value = props.prefill.name ?? ''
        if (kind.value === 'web') {
          const split = splitWebTarget(props.prefill.target ?? '')
          webScheme.value = split.scheme
          target.value = split.value
          icon.value = props.prefill.icon ?? deriveFaviconUrl(joinWebTarget(split.scheme, split.value)) ?? ''
        } else {
          target.value = props.prefill.target ?? ''
          icon.value = props.prefill.icon ?? ''
        }
        category.value = props.prefill.category ?? '其他'
        isDir.value = props.prefill.isDir ?? false
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
    return
  }
  if (kind.value === 'web') {
    const normalized = joinWebTarget(webScheme.value, target.value)
    target.value = splitWebTarget(normalized).value
    if (!icon.value.trim()) {
      icon.value = deriveFaviconUrl(normalized) ?? ''
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
  if (kind.value === 'web') {
    trimmedTarget = joinWebTarget(webScheme.value, trimmedTarget)
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

function normalizeWebTarget() {
  if (kind.value !== 'web') return
  const split = splitWebTarget(target.value)
  webScheme.value = split.scheme
  target.value = split.value
  const normalized = joinWebTarget(split.scheme, split.value)
  if (!icon.value.trim()) icon.value = deriveFaviconUrl(normalized) ?? ''
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
          class="modal-card form-card"
          role="dialog"
          aria-label="速达资源编辑"
          aria-modal="true"
        >
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
            ref="nameInputRef"
            v-model="name"
            class="field-input"
            type="text"
            maxlength="80"
            :placeholder="kind === 'file' ? '自动取文件名' : '如：VS Code / GitHub'"
            @keydown="onKeydown"
          />

          <!-- 目标 -->
          <label class="field-label">{{ targetLabel }}</label>
          <div v-if="kind === 'web'" class="web-input-row">
            <select v-model="webScheme" class="scheme-select" aria-label="网址协议">
              <option value="http">http://</option>
              <option value="https">https://</option>
            </select>
            <div class="input-with-btn web-target-wrap">
              <input
                v-model="target"
                class="field-input web-target-input"
                type="text"
                :placeholder="targetPlaceholder"
                @keydown="onKeydown"
                @blur="normalizeWebTarget"
              />
              <button class="input-btn" title="自动抓取图标" @click="pickTarget">
                <FolderOpen :size="15" :stroke-width="1.8" />
              </button>
            </div>
          </div>
          <div v-else class="input-with-btn">
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
              title="选择"
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
            <div class="icon-row" :class="{ 'icon-row--web': kind === 'web' }">
              <input
                v-if="showWebIconInput"
                v-model="icon"
                class="field-input"
                type="text"
                maxlength="260"
                placeholder="Emoji 或留空自动生成"
                @keydown="onKeydown"
              />
              <button v-if="showWebIconInput" class="input-btn" title="选择本地图标" @click="pickIcon">
                <ImagePlus :size="15" :stroke-width="1.8" />
              </button>
              <span v-if="isExtractedIcon" class="extracted-badge" title="已从文件导入图标">
                ✓ 已导入
              </span>
              <span v-else-if="kind === 'web'" class="web-icon-hint">
                图标将自动抓取当前网站 favicon
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
            <p class="link-hint">
              <Link :size="12" :stroke-width="2" class="link-hint-icon" aria-hidden="true" />
              仅创建链接，源文件保留在原位置
            </p>
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
.icon-row--web {
  display: flex;
  align-items: center;
  gap: 8px;
}
.icon-row--web .field-input {
  padding-right: 12px;
}
.input-with-btn {
  position: relative;
}
.input-with-btn .field-input {
  padding-right: 40px;
}
.web-input-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.scheme-select {
  flex-shrink: 0;
  width: 92px;
  height: 38px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
  color: var(--text-2);
  font-size: 13px;
  padding: 0 10px;
  outline: none;
}
.web-target-wrap {
  flex: 1;
}
.web-target-input {
  padding-right: 82px;
}
.web-target-wrap .input-btn {
  right: 46px;
}
.web-icon-hint {
  font-size: 12px;
  color: var(--text-3);
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
.icon-row .field-input {
  text-align: left;
}
.icon-row .field-input::placeholder {
  text-align: left;
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
  transition: border-color 0.15s, color 0.15s, background 0.15s;
}
.cat-pill:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}
.cat-pill.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: var(--text-on-accent);
}
.link-hint {
  margin-top: 14px;
  font-size: 12px;
  color: var(--text-3);
  display: flex;
  align-items: center;
  gap: 4px;
}
.link-hint-icon {
  flex-shrink: 0;
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
