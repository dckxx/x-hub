<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, ImageDown, ImagePlus, Link } from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { categorize } from '../utils/categories'
import { useFocusTrap } from '../composables/useFocusTrap'
import { useStore } from '../stores/workbench'
import { deriveFaviconUrl, normalizeWebUrl } from '../utils/web'

const store = useStore()

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
const args = ref('')
const icon = ref('')
/** 小类名；null = 编辑时「未归类」/ 新建时跟随默认小类（后端自动归入） */
const category = ref<string | null>(null)
const isDir = ref(false)
const error = ref('')
const cardRef = ref<HTMLElement | null>(null)
const nameInputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, nameInputRef)

const isEdit = computed(() => props.editing !== null)

/** 当前大类的小类库名单（各大类一套、允许同名不同义） */
const kindOptions = computed(() => store.subcategoriesOf(kind.value).map((s) => s.name))

/** 新建/切换大类时的缺省小类 = 该大类默认小类（还没有小类库时为 null → 未归类） */
function defaultCategoryFor(k: 'app' | 'web' | 'file'): string | null {
  return store.defaultSubcategoryName(k)
}

const isExtractedIcon = computed(() => /\.(png|jpg|jpeg|ico|gif|webp)$/i.test(icon.value))
const targetLabel = computed(() => {
  if (kind.value === 'file') return isDir.value ? '文件夹路径' : '文件路径'
  if (kind.value === 'app') return '程序路径'
  return '网址'
})

const targetPlaceholder = computed(() => {
  if (kind.value === 'file') return '选择要链接的文件或文件夹'
  if (kind.value === 'app') return '如：C:\\Program Files\\...\\code.exe'
  return '如：github.com 或 https://github.com'
})

const iconPlaceholder = computed(() => {
  if (kind.value === 'web') return '留空使用当前网站 favicon'
  return 'Emoji 或留空自动生成'
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
      category.value = props.editing.category ?? null
      isDir.value = props.editing.category === '文件夹'
    } else {
      kind.value = 'app'
      name.value = ''
      target.value = ''
      args.value = ''
      icon.value = ''
      category.value = defaultCategoryFor('app')
      isDir.value = false
      if (props.prefill) {
        kind.value = props.prefill.kind ?? 'app'
        name.value = props.prefill.name ?? ''
        target.value = props.prefill.target ?? ''
        if (kind.value === 'web') {
          icon.value = props.prefill.icon ?? deriveFaviconUrl(normalizeWebUrl(target.value)) ?? ''
        } else {
          icon.value = props.prefill.icon ?? ''
        }
        category.value = props.prefill.category ?? defaultCategoryFor(kind.value)
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
    normalizeWebTarget()
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
    trimmedTarget = normalizeWebUrl(trimmedTarget)
  }
  emit('submit', {
    id: props.editing?.id,
    kind: kind.value,
    name: trimmedName,
    target: trimmedTarget,
    // 新建传 null = 后端自动归默认小类；编辑传 null = 显式未归类
    category: category.value,
    icon: icon.value.trim() || null,
    args: kind.value === 'app' ? (args.value.trim() || null) : null,
  })
  emit('close')
}

function normalizeWebTarget() {
  if (kind.value !== 'web' || !target.value.trim()) return
  target.value = normalizeWebUrl(target.value)
  if (!icon.value.trim()) icon.value = deriveFaviconUrl(target.value) ?? ''
}

function onKindChange(nextKind: 'app' | 'web' | 'file') {
  kind.value = nextKind
  // 小类归属随大类切换重置为该大类的默认小类（小类库各大类独立）
  category.value = defaultCategoryFor(nextKind)
  if (nextKind === 'web' && target.value.trim() && !icon.value.trim()) normalizeWebTarget()
}

function onIconInputBlur() {
  if (kind.value === 'web' && target.value.trim() && !icon.value.trim()) {
    icon.value = deriveFaviconUrl(normalizeWebUrl(target.value)) ?? ''
  }
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
              @click="onKindChange('app')"
            >
              本地程序
            </button>
            <button
              class="kind-pill"
              :class="{ active: kind === 'web' }"
              @click="onKindChange('web')"
            >
              网页书签
            </button>
            <button
              class="kind-pill"
              :class="{ active: kind === 'file' }"
              @click="onKindChange('file')"
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
          <div class="input-with-btn">
            <input
              v-model="target"
              class="field-input"
              type="text"
              :readonly="kind === 'file'"
              :placeholder="targetPlaceholder"
              @keydown="onKeydown"
              @blur="normalizeWebTarget"
            />
            <button
              class="input-btn"
              :title="kind === 'web' ? '自动抓取图标' : '选择'"
              @click="pickTarget"
            >
              <ImageDown v-if="kind === 'web'" :size="15" :stroke-width="1.8" />
              <FolderOpen v-else :size="15" :stroke-width="1.8" />
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
                :placeholder="iconPlaceholder"
                @keydown="onKeydown"
                @blur="onIconInputBlur"
              />
              <button class="input-btn" title="选择本地图标" @click="pickIcon">
                <ImagePlus :size="15" :stroke-width="1.8" />
              </button>
              <span v-if="isExtractedIcon" class="extracted-badge" title="已从文件导入图标">
                ✓ 已导入
              </span>
            </div>
          </template>

          <!-- 小类（ADR 0012）：各大类一套小类库；编辑时可选「未归类」清空归属 -->
          <template v-if="kindOptions.length">
            <label class="field-label">小类</label>
            <div class="cat-pills">
              <button
                v-if="isEdit"
                class="cat-pill"
                :class="{ active: category === null }"
                @click="category = null"
              >
                未归类
              </button>
              <button
                v-for="c in kindOptions"
                :key="c"
                class="cat-pill"
                :class="{ active: category === c }"
                @click="category = c"
              >
                {{ c }}
              </button>
            </div>
          </template>
          <template v-else>
            <label class="field-label">小类</label>
            <p class="link-hint">
              该大类还没有小类，可到 设置 → 功能 → 速达 中新增；当前将显示为「未归类」
            </p>
          </template>
          <p v-if="kind === 'file'" class="link-hint">
            <Link :size="12" :stroke-width="2" class="link-hint-icon" aria-hidden="true" />
            仅创建链接，源文件保留在原位置
          </p>

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
  font-size: 1rem;
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
  font-size: 0.8125rem;
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
  padding-right: 44px;
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
  font-size: 0.6875rem;
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
  font-size: 0.75rem;
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
  font-size: 0.75rem;
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
  font-size: 0.75rem;
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
