<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen } from 'lucide-vue-next'
import { isTauri, type FileEntry } from '../api/tauri'
import { CATEGORIES, categorize } from '../utils/categories'

const props = defineProps<{
  visible: boolean
  editing: FileEntry | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'submit', payload: { id?: number; name: string; path: string; category: string }): void
}>()

const isDir = ref(false)
const name = ref('')
const path = ref('')
const category = ref<string>(CATEGORIES[0])
const error = ref('')

const isEdit = computed(() => props.editing !== null)

watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    if (props.editing) {
      name.value = props.editing.name
      path.value = props.editing.path
      category.value = props.editing.category
      isDir.value = props.editing.category === '文件夹'
    } else {
      name.value = ''
      path.value = ''
      category.value = '其他'
      isDir.value = false
    }
  },
)

async function pickTarget() {
  if (!isTauri()) return
  try {
    const file = await open({
      multiple: false,
      directory: isDir.value,
      filters: isDir.value
        ? undefined
        : [{ name: '所有文件', extensions: ['*'] }],
    })
    if (typeof file !== 'string') return
    path.value = file
    name.value = file.split(/[\\/]/).pop() ?? ''
    category.value = categorize(file, isDir.value)
  } catch (e) {
    error.value = String(e)
  }
}

function submit() {
  const trimmedName = name.value.trim()
  if (!trimmedName) {
    error.value = '请输入名称'
    return
  }
  if (!path.value.trim()) {
    error.value = '请选择文件或文件夹'
    return
  }
  emit('submit', {
    id: props.editing?.id,
    name: trimmedName,
    path: path.value.trim(),
    category: category.value,
  })
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
  if (e.key === 'Enter') submit()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition name="mask">
      <div v-if="visible" class="modal-mask">
        <div class="modal-card form-card" role="dialog" aria-label="文件链接">
          <h2 class="dialog-title">{{ isEdit ? '编辑文件链接' : '添加文件链接' }}</h2>

          <!-- 类型切换（仅新建时可选） -->
          <div v-if="!isEdit" class="kind-switch">
            <button class="kind-pill" :class="{ active: isDir }" @click="isDir = true">
              文件夹
            </button>
            <button class="kind-pill" :class="{ active: !isDir }" @click="isDir = false">
              文件
            </button>
          </div>

          <!-- 路径 -->
          <label class="field-label">{{ isDir ? '文件夹路径' : '文件路径' }}</label>
          <div class="input-with-btn">
            <input
              v-model="path"
              class="field-input"
              type="text"
              readonly
              :placeholder="isDir ? '选择要链接的文件夹' : '选择要链接的文件'"
              @keydown="onKeydown"
            />
            <button class="input-btn" title="选择" @click="pickTarget">
              <FolderOpen :size="15" :stroke-width="1.8" />
            </button>
          </div>

          <!-- 名称 -->
          <label class="field-label">名称</label>
          <input
            v-model="name"
            class="field-input"
            type="text"
            maxlength="80"
            placeholder="显示名称（自动取文件名）"
            @keydown="onKeydown"
          />

          <!-- 分类 -->
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
