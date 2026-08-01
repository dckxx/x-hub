<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  NModal,
  NInput,
  NList,
  NListItem,
  NThing,
  NTag,
  NEmpty,
  NIcon,
} from 'naive-ui'
import { LinkOutline, CodeWorkingOutline } from '@vicons/ionicons5'
import { useStore } from '../stores/workbench'
import type { Note, Resource } from '../api/tauri'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'update:open', open: boolean): void
}>()

const store = useStore()
const keyword = ref('')
const resources = ref<Resource[]>([])
const notes = ref<Note[]>([])
let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.open,
  (open) => {
    if (open) {
      keyword.value = ''
      resources.value = []
      notes.value = []
    }
  },
)

function onSearch() {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = setTimeout(async () => {
    if (!keyword.value.trim()) {
      resources.value = []
      notes.value = []
      return
    }
    const result = await store.searchAll(keyword.value.trim())
    resources.value = result.resources
    notes.value = result.notes
  }, 300)
}

function formatTime(iso: string) {
  return iso.slice(0, 16).replace('T', ' ')
}

async function launch(resourceId: number) {
  try {
    await store.launchResource(resourceId)
  } catch {
    /* ignore */
  }
}
</script>

<template>
  <NModal
    :show="open"
    @update:show="(v) => emit('update:open', v)"
    preset="card"
    class="global-search-modal"
    :mask-closable="true"
  >
    <NInput
      v-model:value="keyword"
      placeholder="搜索快捷资源与笔记…"
      clearable
      @keyup.enter="onSearch"
      @input="onSearch"
    />
    <div v-if="resources.length || notes.length" class="search-results">
      <NList>
        <NListItem v-for="r in resources" :key="r.id" @click="launch(r.id)">
          <NThing :title="r.name">
            <template #header-extra>
              <NTag size="small" type="info">
                <NIcon :component="r.kind === 'web' ? LinkOutline : CodeWorkingOutline" />
                {{ r.kind === 'web' ? '网页' : '程序' }}
              </NTag>
            </template>
          </NThing>
        </NListItem>
        <NListItem v-for="n in notes" :key="n.id">
          <NThing :title="n.title">
            <template #header-extra>
              <NTag size="small" type="success">笔记</NTag>
              <span class="search-results__time">{{ formatTime(n.updated_at) }}</span>
            </template>
            <template #description>
              {{ n.content.slice(0, 60) }}
            </template>
          </NThing>
        </NListItem>
      </NList>
    </div>
    <NEmpty v-else-if="keyword && !resources.length && !notes.length" description="无匹配结果" />
  </NModal>
</template>

<style scoped>
.global-search-modal {
  width: 520px;
  max-width: 90vw;
}
.search-results {
  margin-top: 12px;
  max-height: 400px;
  overflow: auto;
}
.search-results__time {
  margin-left: 8px;
  font-size: 12px;
  opacity: 0.5;
}
</style>
