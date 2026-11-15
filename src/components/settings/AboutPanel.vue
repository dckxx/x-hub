<script setup lang="ts">
// 关于大类（设置 → 关于）：版本 / 开源声明 / 检查更新，复用 AboutSection。
//
// 2026-11-15 自 DataPanel 迁出独立成大类（排在左栏最下）；分区 id=sv-sec-about 不变，
// 设置项搜索索引与子项导航的落点不受影响。
import AboutSection from '../AboutSection.vue';
</script>

<template>
  <section id="sv-sec-about" class="sv-sec" aria-label="关于">
    <h3 class="sv-sec-title">关于</h3>
    <AboutSection />
  </section>
</template>
