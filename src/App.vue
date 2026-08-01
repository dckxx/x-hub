<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import {
  NConfigProvider,
  NMessageProvider,
  NDialogProvider,
  darkTheme,
  lightTheme,
} from 'naive-ui'
import TitleBar from './components/TitleBar.vue'
import SideNav from './components/SideNav.vue'
import QuickLaunch from './components/QuickLaunch.vue'
import NotesView from './components/NotesView.vue'
import SettingsView from './components/SettingsView.vue'
import GlobalSearch from './components/GlobalSearch.vue'
import { useStore } from './stores/workbench'

const store = useStore()
const activeView = ref<'quick' | 'notes' | 'settings'>('quick')
const searchOpen = ref(false)

const darkMode = computed(() => store.state.config.theme === 'dark')

function handleKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
    e.preventDefault()
    searchOpen.value = true
  }
}

onMounted(async () => {
  await store.loadInitialData()
  document.addEventListener('keydown', handleKeydown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <n-config-provider :theme="darkMode ? darkTheme : lightTheme">
    <n-message-provider>
      <n-dialog-provider>
        <div class="app-shell" :class="{ 'app-shell--dark': darkMode }">
          <TitleBar @toggle-search="searchOpen = true" />
          <div class="app-body">
            <SideNav v-model:view="activeView" />
            <main class="app-main">
              <KeepAlive>
                <QuickLaunch v-if="activeView === 'quick'" />
                <NotesView v-else-if="activeView === 'notes'" />
                <SettingsView v-else />
              </KeepAlive>
            </main>
          </div>
          <GlobalSearch v-model:open="searchOpen" />
        </div>
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<style scoped>
.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background-color: var(--n-color);
}
.app-body {
  flex: 1;
  display: flex;
  min-height: 0;
}
.app-main {
  flex: 1;
  min-width: 0;
  overflow: auto;
}
</style>
