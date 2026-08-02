<script setup lang="ts">
</script>

<template>
  <div class="card profile-card">
    <div class="profile-avatar">L</div>
    <div class="profile-info">
      <span class="profile-name">lebrawork</span>
      <span class="profile-email">lebrawork@gmail.com</span>
      <div class="pro-badge">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none">
          <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z" fill="#4F46E5"/>
        </svg>
        <span>Pro</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card {
  background: var(--surface);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  padding: 20px;
}
.profile-card {
  display: flex;
  align-items: center;
  gap: 16px;
}
.profile-avatar {
  width: 56px;
  height: 56px;
  border-radius: 28px;
  background: var(--accent);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  font-size: 24px;
  font-weight: 700;
  letter-spacing: -0.5px;
}
.profile-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.profile-name {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
}
.profile-email {
  font-size: 12px;
  color: var(--text-tertiary);
}
.pro-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--accent-light);
  padding: 3px 8px;
  border-radius: 8px;
  width: fit-content;
}
.pro-badge span {
  font-size: 11px;
  font-weight: 700;
  color: var(--accent);
}
</style>
