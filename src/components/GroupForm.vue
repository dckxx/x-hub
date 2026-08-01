<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  NModal,
  NForm,
  NFormItem,
  NInput,
  NButton,
  NSpace,
  useMessage,
} from 'naive-ui'
import { useStore } from '../stores/workbench'
import type { Group } from '../api/tauri'

const props = defineProps<{
  open: boolean
  editing: Group | null
}>()

const emit = defineEmits<{
  (e: 'update:open', open: boolean): void
}>()

const store = useStore()
const message = useMessage()
const name = ref('')

watch(
  () => props.open,
  (open) => {
    if (open) {
      name.value = props.editing?.name ?? ''
    }
  },
)

async function submit() {
  if (!name.value.trim()) {
    message.warning('请输入分组名称')
    return
  }
  try {
    if (props.editing) {
      await store.renameGroup(props.editing.id, name.value.trim())
      message.success('已重命名')
    } else {
      await store.addGroup(name.value.trim())
      message.success('已创建')
    }
    emit('update:open', false)
  } catch (e: unknown) {
    message.error(String(e) || '操作失败')
  }
}
</script>

<template>
  <NModal
    :show="open"
    @update:show="(v) => emit('update:open', v)"
    preset="card"
    :title="editing ? '重命名分组' : '新建分组'"
    class="group-form-modal"
  >
    <NForm label-placement="top">
      <NFormItem label="分组名称">
        <NInput v-model:value="name" placeholder="例如：开发工具、常用网址" @keyup.enter="submit" />
      </NFormItem>
    </NForm>
    <template #footer>
      <NSpace justify="end">
        <NButton @click="emit('update:open', false)">取消</NButton>
        <NButton type="primary" @click="submit">保存</NButton>
      </NSpace>
    </template>
  </NModal>
</template>

<style scoped>
.group-form-modal {
  width: 380px;
  max-width: 90vw;
}
</style>
