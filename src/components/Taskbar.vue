<script setup lang="ts">
import { ref } from 'vue'

const apps = [
  { id: 'files', color: '#FCD34D', icon: 'folder' },
  { id: 'browser', color: '#60A5FA', icon: 'globe' },
  { id: 'mail', color: '#F87171', icon: 'mail' },
  { id: 'calendar', color: '#34D399', icon: 'calendar' },
  { id: 'notes', color: '#FBBF24', icon: 'notes' },
  { id: 'music', color: '#A78BFA', icon: 'music' },
  { id: 'settings', color: '#9CA3AF', icon: 'settings' },
  { id: 'profile', color: '#E5E7EB', icon: 'profile' },
]

const activeApp = ref('files')
</script>

<template>
  <div class="taskbar-container">
    <footer class="taskbar">
      <button
        v-for="app in apps"
        :key="app.id"
        class="taskbar-icon"
        :class="{ 'taskbar-icon--active': activeApp === app.id }"
        @click="activeApp = app.id"
      >
        <svg width="40" height="40" viewBox="0 0 56 56" fill="none">
          <rect width="56" height="56" rx="10" :fill="app.color"/>
          <template v-if="app.icon === 'folder'">
            <path d="M14 20c0-2.21 1.79-4 4-4h9l5 4h12c2.21 0 4 1.79 4 4v18c0 2.21-1.79 4-4 4H18c-2.21 0-4-1.79-4-4V20z" fill="#FFF7ED"/>
          </template>
          <template v-else-if="app.icon === 'globe'">
            <circle cx="28" cy="28" r="14" stroke="white" stroke-width="3"/>
            <ellipse cx="28" cy="28" rx="6" ry="14" stroke="white" stroke-width="2"/>
            <path d="M16 28h24" stroke="white" stroke-width="2"/>
          </template>
          <template v-else-if="app.icon === 'mail'">
            <rect x="12" y="18" width="32" height="22" rx="4" stroke="white" stroke-width="3"/>
            <path d="M12 22l16 10 16-10" stroke="white" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>
          </template>
          <template v-else-if="app.icon === 'calendar'">
            <rect x="12" y="16" width="32" height="28" rx="4" stroke="white" stroke-width="3"/>
            <path d="M12 24h32M20 12v8M36 12v8" stroke="white" stroke-width="3" stroke-linecap="round"/>
          </template>
          <template v-else-if="app.icon === 'notes'">
            <rect x="14" y="12" width="28" height="34" rx="3" fill="white"/>
            <path d="M19 22h18M19 30h18M19 38h12" stroke="#D97706" stroke-width="2" stroke-linecap="round"/>
          </template>
          <template v-else-if="app.icon === 'music'">
            <circle cx="28" cy="28" r="14" fill="white"/>
            <path d="M24 20v16l12-8-12-8z" fill="#A78BFA"/>
          </template>
          <template v-else-if="app.icon === 'settings'">
            <circle cx="28" cy="28" r="7" stroke="white" stroke-width="3"/>
            <path d="M28 11v6M28 39v6M11 28h6M39 28h6M16.44 16.44l4.24 4.24M35.32 35.32l4.24 4.24M16.44 39.56l4.24-4.24M35.32 20.68l4.24-4.24" stroke="white" stroke-width="3" stroke-linecap="round"/>
          </template>
          <template v-else-if="app.icon === 'profile'">
            <circle cx="28" cy="22" r="9" fill="#9CA3AF"/>
            <path d="M14 46c0-7.732 6.268-14 14-14s14 6.268 14 14v2H14v-2z" fill="#9CA3AF"/>
          </template>
        </svg>
      </button>
    </footer>
  </div>
</template>

<style scoped>
.taskbar-container {
  display: flex;
  justify-content: center;
  flex-shrink: 0;
}
.taskbar {
  height: 56px;
  background: rgba(255, 255, 255, 0.85);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-radius: var(--radius-md);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 8px 12px;
  box-shadow: var(--shadow-lg), 0 0 0 1px rgba(255, 255, 255, 0.6) inset;
}
.taskbar-icon {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.15s, transform 0.15s;
  padding: 4px;
  position: relative;
  border: none;
  background: transparent;
}
.taskbar-icon:hover {
  background: rgba(0, 0, 0, 0.05);
  transform: translateY(-2px);
}
.taskbar-icon--active {
  background: rgba(79, 70, 229, 0.08);
}
.taskbar-icon--active::after {
  content: '';
  position: absolute;
  bottom: -6px;
  left: 50%;
  transform: translateX(-50%);
  width: 16px;
  height: 3px;
  border-radius: 2px;
  background: var(--accent);
}
</style>
