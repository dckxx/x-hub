<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import TitleBar from './components/TitleBar.vue'
import ProfileCard from './components/ProfileCard.vue'
import NewsCard from './components/NewsCard.vue'
import WeatherCard from './components/WeatherCard.vue'
import CalendarCard from './components/CalendarCard.vue'
import NotesRow from './components/NotesRow.vue'
import FileCard from './components/FileCard.vue'
import Taskbar from './components/Taskbar.vue'

const appContainer = ref<HTMLElement | null>(null)

function scaleApp() {
  const viewport = document.getElementById('viewport')
  const app = appContainer.value
  if (!viewport || !app) return
  const scale = Math.min(viewport.clientWidth / 1440, viewport.clientHeight / 900)
  app.style.transform = `scale(${scale})`
}

onMounted(() => {
  scaleApp()
  window.addEventListener('resize', scaleApp)
})

onUnmounted(() => {
  window.removeEventListener('resize', scaleApp)
})
</script>

<template>
  <div id="viewport">
    <div id="app-container" ref="appContainer">
      <TitleBar />
      <main class="workspace">
        <div class="content-grid">
          <section class="column column-left">
            <ProfileCard />
            <NewsCard />
            <WeatherCard />
          </section>
          <section class="column column-middle">
            <CalendarCard />
            <NotesRow />
          </section>
          <section class="column column-right">
            <FileCard />
          </section>
        </div>
        <Taskbar />
      </main>
    </div>
  </div>
</template>

<style scoped>
.workspace {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 32px;
  gap: 24px;
  overflow: hidden;
}
.content-grid {
  flex: 1;
  display: flex;
  gap: 24px;
  overflow: hidden;
}
.column {
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 100%;
}
.column-left { width: 300px; flex-shrink: 0; }
.column-middle { width: 460px; flex-shrink: 0; }
.column-right { flex: 1; min-width: 0; }
</style>
