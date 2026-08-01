<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  NModal,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NButton,
  NSpace,
  useMessage,
} from 'naive-ui'
import { useStore } from '../stores/workbench'
import type { Resource } from '../api/tauri'

const props = defineProps<{
  open: boolean
  editing: Resource | null
  defaultGroupId: number | null
}>()

const emit = defineEmits<{
  (e: 'update:open', open: boolean): void
}>()

const store = useStore()
const message = useMessage()

const form = ref({
  groupId: null as number | null,
  kind: 'web' as 'app' | 'web',
  name: '',
  target: '',
  icon: '',
  args: '',
})

const groupOptions = computed(() =>
  store.state.groups.map((g) => ({ label: g.name, value: g.id })),
)

watch(
  () => props.open,
  (open) => {
    if (open) {
      if (props.editing) {
        form.value = {
          groupId: props.editing.group_id,
          kind: props.editing.kind,
          name: props.editing.name,
          target: props.editing.target,
          icon: props.editing.icon ?? '',
          args: props.editing.args ?? '',
        }
      } else {
        form.value = {
          groupId: props.defaultGroupId ?? groupOptions.value[0]?.value ?? null,
          kind: 'web',
          name: '',
          target: '',
          icon: '',
          args: '',
        }
      }
    }
  },
)

async function submit() {
  if (!form.value.name.trim()) {
    message.warning('请输入名称')
    return
  }
  if (!form.value.target.trim()) {
    message.warning(form.value.kind === 'web' ? '请输入网址' : '请输入程序路径')
    return
  }
  if (!form.value.groupId) {
    message.warning('请选择分组')
    return
  }
  const payload = {
    groupId: form.value.groupId,
    kind: form.value.kind,
    name: form.value.name.trim(),
    target: form.value.target.trim(),
    icon: form.value.icon || null,
    args: form.value.kind === 'app' ? form.value.args || null : null,
  }
  try {
    if (props.editing) {
      await store.editResource({ id: props.editing.id, ...payload })
      message.success('已保存')
    } else {
      await store.addResource(payload)
      message.success('已添加')
    }
    emit('update:open', false)
  } catch (e: unknown) {
    message.error(String(e) || '保存失败')
  }
}
</script>

<template>
  <NModal
    :show="open"
    @update:show="(v) => emit('update:open', v)"
    preset="card"
    :title="editing ? '编辑快捷资源' : '新增快捷资源'"
    class="resource-form-modal"
  >
    <NForm label-placement="top">
      <NFormItem label="所属分组">
        <NSelect v-model:value="form.groupId" :options="groupOptions" />
      </NFormItem>
      <NFormItem label="资源类型">
        <NSpace>
          <NButton
            size="small"
            :type="form.kind === 'web' ? 'primary' : 'default'"
            @click="form.kind = 'web'"
          >
            网页书签
          </NButton>
          <NButton
            size="small"
            :type="form.kind === 'app' ? 'primary' : 'default'"
            @click="form.kind = 'app'"
          >
            本地程序
          </NButton>
        </NSpace>
      </NFormItem>
      <NFormItem :label="form.kind === 'web' ? '标题' : '名称'">
        <NInput v-model:value="form.name" placeholder="显示名称" />
      </NFormItem>
      <NFormItem :label="form.kind === 'web' ? '网址' : '程序路径'">
        <NInput v-model:value="form.target" placeholder="https:// 或 C:\path\to\app.exe" />
      </NFormItem>
      <NFormItem v-if="form.kind === 'app'" label="附加启动参数">
        <NInput v-model:value="form.args" placeholder="可留空，空格分隔多个参数" />
      </NFormItem>
      <NFormItem label="自定义图标">
        <NInput v-model:value="form.icon" placeholder="图标名称或路径，留空使用默认" />
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
.resource-form-modal {
  width: 460px;
  max-width: 90vw;
}
</style>
