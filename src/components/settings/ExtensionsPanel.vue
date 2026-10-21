<script setup lang="ts">
// 扩展大类（左栏两个子项，对应下面两个 section）：
//   ① 扩展：运行环境开关（service 运行时策略）+ 去扩展中心的入口 —— 只管「怎么跑」
//   ② Skills：内置扩展开发技能包一键装到本机 AI 助手的 skills 目录（独立子组件 SkillsSection）
//
// 「我的扩展」（本机源码目录的增删）在扩展中心，与「已安装 / 市场」并列为第三个标签页：
// 加进来就是要调试，所以**没有开关**（登记即加载，见 docs/adr/0005 的 v0.6.x 修订）。
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { inject, onMounted, ref } from 'vue';
import { Puzzle } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { DevModeStatus } from '../../api/tauri';
import AppSelect from '../AppSelect.vue';
import SkillsSection from './SkillsSection.vue';
import { useStore } from '../../stores/workbench';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

const emit = defineEmits<{
  /** 跳到扩展中心（本机源码目录的增删在那里） */
  (e: 'open-extensions'): void
}>()

const RUNTIME_STRATEGY_OPTIONS = [
  { value: 'auto', label: '自动检测（系统优先，缺失自动下载内置）' },
  { value: 'builtin', label: '始终内置（统一用下载的内置运行时）' },
  { value: 'system', label: '始终系统（只用系统 Node，不下载）' },
] as const

function onRuntimeStrategyChange(value: string) {
  void store.setRuntimeStrategy(value as 'auto' | 'builtin' | 'system').then(() => {
    showToast('运行时策略已更新，下次启动 service 扩展生效')
  })
}

/** 扩展中心「已安装 / 我的扩展」列表的点击行为：打开详情（默认）或直接打开 */
const ROW_CLICK_OPTIONS = [
  { value: 'detail', label: '打开详情' },
  { value: 'open', label: '直接打开' },
] as const

function onRowClickModeChange(value: string) {
  void store.setExtensionRowClick(value as 'detail' | 'open').then(
    () => showToast(value === 'open' ? '已改为：点击扩展行直接打开' : '已改为：点击扩展行打开详情'),
    () => showToast('保存失败，请重试'),
  )
}

/** 全局自动信任 service 扩展：开启后新装/更新的 service 扩展不再弹「去授权」 */
function onToggleAutoTrust() {
  const next = !store.state.config.service_auto_trust
  void store.setServiceAutoTrust(next).then(
    () => showToast(next ? '已开启自动信任：新装或更新的 service 扩展可直接运行' : '已关闭自动信任，之后按单个扩展逐个确认'),
    () => showToast('保存失败，请重试'),
  )
}

// ---- 我的扩展（只为显示已添加几个目录；增删都在扩展中心） ----
const devMode = ref<DevModeStatus>({ enabled: true, extensions: [] })

async function loadDevMode() {
  if (!isTauri()) return
  try {
    devMode.value = await tauriApi.getDevModeStatus()
  } catch {
    // 后端不可用（浏览器预览）时静默：该分区只在桌面端有意义
  }
}

onMounted(() => {
  void loadDevMode()
})
</script>

<template>
        <section id="sv-sec-extensions" class="sv-sec" aria-label="扩展">
          <h3 class="sv-sec-title">扩展</h3>
          <p class="dev-hint">
            装扩展、更新、写扩展都在侧栏「<b>扩展中心</b>」：<b>已安装</b>管理已装扩展，<b>市场</b>发现并安装新扩展，<b>我的扩展</b>添加本机源码目录边写边看。这一页只管运行环境。
          </p>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">service 运行时策略</span>
              <span class="setting-desc">service 扩展后端的 Node 运行时来源：自动检测 / 始终内置 / 始终系统</span>
            </div>
            <AppSelect
              :model-value="store.state.config.runtime_strategy || 'auto'"
              :options="RUNTIME_STRATEGY_OPTIONS"
              aria-label="service 运行时策略"
              @update:model-value="onRuntimeStrategyChange"
            />
          </div>

          <!-- 列表点击行为：两种用户习惯（点行打开 vs 点行看详情），做成可配 -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">点击扩展行时</span>
              <span class="setting-desc">扩展中心「已安装 / 我的扩展」里单击某一行的行为：打开详情，或直接打开扩展（改为「直接打开」后，列表右侧会出现 ⋯ 按钮用于查看详情）</span>
            </div>
            <AppSelect
              :model-value="store.state.config.extension_row_click || 'detail'"
              :options="ROW_CLICK_OPTIONS"
              aria-label="点击扩展行时"
              @update:model-value="onRowClickModeChange"
            />
          </div>

          <!-- 统一授权：免掉「每个新装 service 扩展都要去授权一次」的重复动作。
               安全语义：本地后端不受沙箱隔离，描述里必须把代价说清；单独关掉某扩展后端仍优先于本开关 -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">自动信任 service 扩展</span>
              <span class="setting-desc">
                开启后新装或更新版本的 service 扩展无需逐个「去授权」即可运行本地后端。⚠️ 本地后端可读取你有权限访问的文件并联网、不受系统沙箱隔离，请仅在你信任扩展来源时开启；单独关掉某扩展「运行本地后端」的选择始终优先于本开关
              </span>
            </div>
            <button
              class="toggle"
              role="switch"
              type="button"
              :aria-checked="store.state.config.service_auto_trust"
              aria-label="自动信任 service 扩展"
              :class="{ on: store.state.config.service_auto_trust }"
              @click="onToggleAutoTrust"
            >
              <span class="toggle-knob"></span>
            </button>
          </div>

          <!-- 我的扩展的入口指路：目录增删都在扩展中心，这里只留一句话 + 一键跳过去 -->
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">我的扩展</span>
              <span class="setting-desc">
                {{ devMode.extensions.length ? `已添加 ${devMode.extensions.length} 个本机源码目录` : '还没有添加本机源码目录' }}（须含 manifest.json）。添加后立即加载、改代码即重载，可先在本机调试；发布需要开发者认证 —— 增删都在扩展中心「我的扩展」标签页
              </span>
            </div>
            <button class="ghost-btn data-btn ext-entry-btn" type="button" @click="emit('open-extensions')">
              <Puzzle :size="14" :stroke-width="2" />
              打开扩展中心
            </button>
          </div>
        </section>

        <!-- 技能包：内置扩展开发 skill 一键装到本机 AI 助手的 skills 目录 -->
        <SkillsSection />
</template>

<style scoped>
/* 本面板专属样式（通用设置样式在 settings/shared.css，见约定 50） */
.dev-hint {
  margin: 0 0 10px;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-2);
}
.dev-hint b {
  color: var(--text-1);
}
/* 「打开扩展中心」按钮：左侧长描述会把行内剩余宽度挤到很小，按钮被压缩后
   图标和文字会折行成两行——锁死单行（不收缩、不换行），保证图标 + 文字一行排布 */
.ext-entry-btn {
  flex-shrink: 0;
  flex-wrap: nowrap;
  white-space: nowrap;
}
</style>
