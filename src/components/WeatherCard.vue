<script setup lang="ts">
</script>

<template>
  <div class="card weather-card">
    <div class="weather-info">
      <span class="weather-city">北京</span>
      <span class="weather-temp">26°</span>
      <span class="weather-desc">多云</span>
    </div>
    <svg width="64" height="64" viewBox="0 0 64 64" fill="none">
      <circle cx="32" cy="32" r="14" fill="#FBBF24"/>
      <circle cx="32" cy="32" r="22" fill="none" stroke="#FBBF24" stroke-width="1.5" stroke-dasharray="2 4" opacity="0.5"/>
      <path d="M32 4v6M32 54v6M4 32h6M54 32h6" stroke="#FBBF24" stroke-width="2" stroke-linecap="round"/>
    </svg>
  </div>
</template>

<style scoped>
.card {
  background: var(--surface);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  padding: 20px;
}
.weather-card {
  height: 160px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.weather-info {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.weather-city {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
}
.weather-temp {
  font-size: 42px;
  font-weight: 700;
  color: var(--text-primary);
  line-height: 1;
}
.weather-desc {
  font-size: 13px;
  color: var(--text-tertiary);
}
</style>
