<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { Eye, EyeOff, FolderOpen, ImageDown, ImagePlus, Link } from 'lucide-vue-next'
import { isTauri, tauriApi, type Resource } from '../api/tauri'
import { categorize } from '../utils/categories'
import { useFocusTrap } from '../composables/useFocusTrap'
import { useStore } from '../stores/workbench'
import { deriveFaviconUrl, normalizeWebUrl } from '../utils/web'
import AppSelect, { type AppSelectOption } from './AppSelect.vue'

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
      zoneId?: number | null
      description?: string | null
      remark?: string | null
      remarkLabel?: string | null
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
/** 所属分区（「全部」tab 成组陈列）；null = 未分区 */
const zoneId = ref<number | null>(null)
/** 用途说明（非敏感）：这个网页/程序/文件是什么 */
const description = ref('')
/** 备注（敏感，可存账号密码/文件解压密码等；后端 DPAPI 加密落盘） */
const remark = ref('')
/** 备注自定义标签（如「账号密码」「解压密码」）；空 = 默认「备注」 */
const remarkLabel = ref('')
/** 备注明文可见性：默认密文（圆点），小眼睛切换 */
const showRemark = ref(false)
/** 编辑态备注明文是否已按需拉到：明文不随资源列表下发（加密收窄暴露面），
 *  没拉到就拦截提交，防止把空值当「清空备注」写回库 */
const remarkLoaded = ref(true)
const isDir = ref(false)
const error = ref('')
const cardRef = ref<HTMLElement | null>(null)
const nameInputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(toRef(props, 'visible'), cardRef, nameInputRef)

const isEdit = computed(() => props.editing !== null)

/** 当前大类的小类选项（书签导入的小类带「/」层级，展示为「祖先 / 叶名」便于区分同名目录） */
const kindOptions = computed(() =>
  store.subcategoriesOf(kind.value).map((s) => {
    const segs = s.name.split('/').filter(Boolean)
    const leaf = segs.length > 0 ? segs[segs.length - 1] : s.name
    return {
      full: s.name,
      leaf,
      ancestor: segs.length > 1 ? segs.slice(0, -1).join(' / ') : '',
    }
  }),
)

/** 新建/切换大类时的缺省小类 = 该大类默认小类（还没有小类库时为 null → 未归类） */
function defaultCategoryFor(k: 'app' | 'web' | 'file'): string | null {
  return store.defaultSubcategoryName(k)
}

/** 分区下拉选项（建了分区才渲染该行；'' = 未分区） */
const zoneOptions = computed<AppSelectOption[]>(() => [
  { value: '', label: '未分区' },
  ...store.state.zones.map((z) => ({ value: String(z.id), label: z.name })),
])

const zoneValue = computed<string>({
  get: () => (zoneId.value == null ? '' : String(zoneId.value)),
  set: (v) => {
    zoneId.value = v === '' ? null : Number(v)
  },
})

const isExtractedIcon = computed(() => /\.(png|jpg|jpeg|ico|gif|webp)$/i.test(icon.value))
const targetLabel = computed(() => {
  if (kind.value === 'file') return isDir.value ? '文件夹路径' : '文件路径'
  if (kind.value === 'app') return '程序路径'
  return '网址'
})

const targetPlaceholder = computed(() => {
  if (kind.value === 'file') return '选择要链接的文件或文件夹'
  if (kind.value === 'app') return '如：C:\\Program Files\\...\\code.exe'
  return '如：github.com、https://… 或 smb://nas/share、ftp://…'
})

const iconPlaceholder = computed(() => {
  if (kind.value === 'web') return '留空使用当前网站 favicon（smb/ftp 无图标）'
  return 'Emoji 或留空自动生成'
})

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    showRemark.value = false
    if (props.editing) {
      kind.value = props.editing.kind === 'file' ? 'file' : props.editing.kind
      name.value = props.editing.name
      target.value = props.editing.target
      args.value = props.editing.args ?? ''
      icon.value = props.editing.icon ?? ''
      category.value = props.editing.category ?? null
      zoneId.value = props.editing.zone_id ?? null
      description.value = props.editing.description ?? ''
      // 备注明文不随资源列表下发：编辑时按需解密拉一次；拉失败拦截提交防误清空
      remark.value = ''
      remarkLabel.value = props.editing.remark_label ?? ''
      remarkLoaded.value = false
      tauriApi
        .getResourceRemark(props.editing.id)
        .then((plain) => {
          remark.value = plain ?? ''
          remarkLoaded.value = true
        })
        .catch(() => {
          remarkLoaded.value = false
        })
      isDir.value = props.editing.category === '文件夹'
    } else {
      kind.value = 'app'
      name.value = ''
      target.value = ''
      args.value = ''
      icon.value = ''
      category.value = defaultCategoryFor('app')
      zoneId.value = null
      description.value = ''
      remark.value = ''
      remarkLabel.value = ''
      remarkLoaded.value = true
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
  if (isEdit.value && !remarkLoaded.value) {
    // 备注明文没拉到就提交 = 把空值当「清空」写回库，宁可拦下
    error.value = '备注读取失败，请关闭弹窗后重试'
    return
  }
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
    zoneId: zoneId.value,
    description: description.value.trim() || null,
    // 备注不 trim：密码里的空格可能是有意义的内容
    remark: remark.value || null,
    remarkLabel: remarkLabel.value.trim() || null,
  })
  // 不在此处 emit('close')：保存成败由父组件决定关窗（失败时输入保留可重试）
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

          <!-- 名称 + 图标：一行两列（file 类型无图标，名称占满整行） -->
          <div class="grid-2">
            <div :class="{ 'span-2': kind === 'file' }">
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
            </div>
            <div v-if="kind !== 'file'">
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
            </div>
          </div>

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

          <!-- 小类（ADR 0012）：各大类一套小类库；带层级的小类以「祖先 / 叶名」展示；编辑时可选「未归类」清空归属 -->
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
                :key="c.full"
                class="cat-pill"
                :class="{ active: category === c.full }"
                :title="c.full"
                @click="category = c.full"
              >
                <span v-if="c.ancestor" class="cat-pill-ancestor">{{ c.ancestor }} / </span>{{ c.leaf }}
              </button>
            </div>
          </template>
          <template v-else>
            <label class="field-label">小类</label>
            <p class="link-hint">
              该大类还没有小类，可到 设置 → 功能 → 速达 中新增；当前将显示为「未归类」
            </p>
          </template>
          <!-- 分区（「全部」tab 成组陈列）：建了分区才出现；不影响应用/网页/文件 tab 的小类筛选 -->
          <template v-if="zoneOptions.length > 1">
            <label class="field-label">分区</label>
            <AppSelect
              v-model="zoneValue"
              :options="zoneOptions"
              aria-label="所属分区"
              style="width: 100%"
            />
          </template>

          <!-- 说明 + 备注：一行两列。备注是复合输入框：左侧前缀标签可改名（如「账号密码」），
               值默认密文圆点、小眼睛切明文；整体一个边框，前缀带独立底色与分隔线 -->
          <div class="grid-2">
            <div>
              <label class="field-label">说明（可选）</label>
              <input
                v-model="description"
                class="field-input"
                type="text"
                maxlength="120"
                placeholder="如：公司邮箱 / 备份压缩包"
                @keydown="onKeydown"
              />
            </div>
            <div>
              <label class="field-label">备注（可选）</label>
              <div class="remark-field">
                <input
                  v-model="remarkLabel"
                  class="remark-prefix"
                  type="text"
                  maxlength="12"
                  placeholder="备注名"
                  title="前缀名称可改，如「账号密码」「解压密码」"
                  @keydown="onKeydown"
                />
                <input
                  v-model="remark"
                  class="remark-value"
                  :type="showRemark ? 'text' : 'password'"
                  maxlength="200"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder="如：网站账号密码"
                  @keydown="onKeydown"
                />
                <button
                  v-if="remark"
                  class="remark-eye"
                  type="button"
                  :title="showRemark ? '隐藏明文' : '显示明文'"
                  @click="showRemark = !showRemark"
                >
                  <EyeOff v-if="showRemark" :size="15" :stroke-width="1.8" />
                  <Eye v-else :size="15" :stroke-width="1.8" />
                </button>
              </div>
            </div>
          </div>
          <p class="remark-hint">备注经系统加密保存在本机，其它设备/系统账户读不到</p>

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
  width: 640px;
  max-height: calc(100vh - 80px);
  overflow-y: auto;
}
/* 一行两列的短字段对（名称|图标、说明|备注）；span-2 = 单字段占满整行 */
.grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  column-gap: 14px;
  align-items: start;
}
.span-2 {
  grid-column: 1 / -1;
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
/* 备注：单边框复合输入框（规格对齐全局 .field-input）——左侧可改名前缀（独立底色 + 分隔线），
   右侧密文值 + 小眼睛；三者 flex 同行，任何宽度下都不会换行错位 */
.remark-field {
  display: flex;
  align-items: stretch;
  width: 100%;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--input-bg);
  overflow: hidden;
  transition: border-color 0.18s, box-shadow 0.18s, background 0.18s;
}
.remark-field:focus-within {
  border-color: var(--brand-500);
  box-shadow: var(--shadow-focus);
  background: color-mix(in srgb, var(--input-bg) 88%, #fff);
}
.remark-prefix {
  width: 92px;
  flex-shrink: 0;
  border: none;
  outline: none;
  background: var(--bg-card-soft);
  border-right: 1px solid var(--border-soft);
  padding: 9px 6px;
  font-size: 0.8125rem;
  font-family: inherit;
  color: var(--text-2);
  text-align: center;
}
/* 占位符要一眼看出「可填写」：--text-4 在亮色下偏深（#6f6a63）像固定标签，
   用 --text-3 再降不透明度得到明确的占位灰，亮暗两态都成立 */
.remark-prefix::placeholder {
  color: color-mix(in srgb, var(--text-3) 75%, transparent);
}
.remark-value {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  padding: 9px 12px;
  font-size: 0.8125rem;
  font-family: inherit;
  color: var(--text-1);
}
.remark-value::placeholder {
  color: color-mix(in srgb, var(--text-3) 75%, transparent);
}
.remark-value[type='password'] {
  letter-spacing: 2px;
}
.remark-eye {
  flex-shrink: 0;
  align-self: stretch;
  width: 32px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}
.remark-eye:hover {
  color: var(--brand-500);
}
.remark-hint {
  margin-top: 6px;
  font-size: 0.6875rem;
  color: var(--text-4);
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
/* 层级小类的祖先路径前缀：弱化显示，选中时同样压暗（保持主文字对比） */
.cat-pill-ancestor {
  color: var(--text-4);
  font-weight: 400;
}
.cat-pill.active .cat-pill-ancestor {
  color: color-mix(in srgb, var(--text-on-accent) 75%, transparent);
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
