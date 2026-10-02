<script setup lang="ts">
// 扩展大类的一个分区：Skills —— 把客户端内置的「扩展开发技能包」一键安装到
// 本机 AI 编码助手（Claude Code / DSH / Codex 等）的 skills 目录。
//
// 技能包随客户端二进制内置（版本一致），本面板只做「探测目标 → 安装 / 更新 / 卸载」：
// 自动探测已知助手目录（**仅列出已存在的**，不代用户新建），另有「自定义目录」入口。
// 后端命令见 src-tauri/src/skills.rs。
import { computed, inject, onMounted, ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, FolderPlus, Lock, RefreshCw, Sparkles, Trash2 } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { SkillOverview, SkillTarget } from '../../api/tauri';

const showToast = inject<(msg: string) => void>('showToast', () => {})

const overview = ref<SkillOverview | null>(null)
/** 正在操作的目标路径（空 = 空闲；按钮据此禁用防重入） */
const opPath = ref('')
/** 一键安装进行中 */
const installingAll = ref(false)
/** 待二次确认覆盖的目标（foreign 目录 / 自定义目录撞同名）：再点一次才真正覆盖 */
const confirmPath = ref<string | null>(null)
/** 自定义目录的待确认路径（与列表行分开，避免两个入口互相串台） */
const customConfirm = ref<string | null>(null)
let confirmTimer: ReturnType<typeof setTimeout> | null = null

const skill = computed(() => overview.value?.skill ?? null)
const targets = computed(() => overview.value?.targets ?? [])
/** 检测到但还没装的目录数（一键安装的对象） */
const pendingCount = computed(() => targets.value.filter((t) => !t.installed).length)
/** 已装但内容落后于当前客户端的目录数（一键更新的对象；外来同名目录需逐个确认，不并入） */
const outdatedCount = computed(
  () => targets.value.filter((t) => t.installed && !t.up_to_date && !t.foreign).length,
)
/** 一键更新进行中 */
const updatingAll = ref(false)

async function load() {
  if (!isTauri()) return
  try {
    overview.value = await tauriApi.getSkillOverview()
  } catch (e) {
    showToast(`读取技能包状态失败：${String(e)}`)
  }
}

function fmtSize(bytes: number): string {
  if (!bytes) return '0 B'
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`
}

function statusText(t: SkillTarget): string {
  if (!t.installed) return '未安装'
  if (t.foreign) return '已存在同名目录（非本客户端安装，覆盖前需确认）'
  if (t.up_to_date) return t.installed_version ? `已安装 v${t.installed_version}` : '已安装'
  return t.installed_version
    ? `可更新（已装 v${t.installed_version}）`
    : '可更新'
}

/** 是否需要二次确认才覆盖：非本客户端安装的同名目录 */
function needConfirm(t: SkillTarget): boolean {
  return t.installed && t.foreign
}

function armConfirm(path: string, custom = false) {
  if (custom) customConfirm.value = path
  else confirmPath.value = path
  if (confirmTimer) clearTimeout(confirmTimer)
  confirmTimer = setTimeout(() => {
    confirmPath.value = null
    customConfirm.value = null
  }, 4000)
}

/** 安装 / 更新到某个目标（force 覆盖已存在目录） */
async function installTo(t: SkillTarget, force: boolean) {
  if (opPath.value) return
  if (needConfirm(t) && confirmPath.value !== t.path) {
    armConfirm(t.path)
    showToast('该目录已存在同名技能且非本客户端安装，再点一次确认覆盖')
    return
  }
  opPath.value = t.path
  confirmPath.value = null
  customConfirm.value = null
  try {
    overview.value = await tauriApi.installSkill(t.path, force)
    showToast(t.installed ? `已更新到 ${t.label}` : `已安装到 ${t.label}`)
  } catch (e) {
    showToast(`安装失败：${String(e)}`)
  } finally {
    opPath.value = ''
  }
}

/** 目标行的主按钮：更新（本客户端装的）/ 覆盖（外来同名）/ 安装 */
function onAction(t: SkillTarget) {
  // 已装的都要走 force（后端对已存在目录一律要显式覆盖确认）
  void installTo(t, t.installed)
}

async function uninstall(t: SkillTarget) {
  if (opPath.value) return
  opPath.value = t.path
  try {
    overview.value = await tauriApi.uninstallSkill(t.path)
    showToast(`已从 ${t.label} 卸载`)
  } catch (e) {
    showToast(`卸载失败：${String(e)}`)
  } finally {
    opPath.value = ''
  }
}

/** 自定义目录：只从列表移除（解除登记，不动磁盘文件） */
async function forgetCustom(t: SkillTarget) {
  if (opPath.value) return
  opPath.value = t.path
  try {
    overview.value = await tauriApi.removeSkillRoot(t.path)
    showToast('已从列表移除（磁盘文件未删）')
  } catch (e) {
    showToast(`移除失败：${String(e)}`)
  } finally {
    opPath.value = ''
  }
}

/** 一键安装：装到所有「检测到但还没装」的目录；外来同名目录不在此列（需逐个确认） */
async function installAll() {
  const pending = targets.value.filter((t) => !t.installed)
  if (!pending.length) {
    showToast('检测到的目录都已安装')
    return
  }
  installingAll.value = true
  let ok = 0
  for (const t of pending) {
    try {
      overview.value = await tauriApi.installSkill(t.path, false)
      ok += 1
    } catch (e) {
      showToast(`${t.label} 安装失败：${String(e)}`)
    }
  }
  installingAll.value = false
  showToast(`已安装到 ${ok}/${pending.length} 个目录`)
}

/** 一键更新：把所有「本客户端装的、内容已落后」的目录更新到当前版本 */
async function updateAll() {
  const outdated = targets.value.filter((t) => t.installed && !t.up_to_date && !t.foreign)
  if (!outdated.length) return
  updatingAll.value = true
  let ok = 0
  for (const t of outdated) {
    try {
      overview.value = await tauriApi.installSkill(t.path, true)
      ok += 1
    } catch (e) {
      showToast(`${t.label} 更新失败：${String(e)}`)
    }
  }
  updatingAll.value = false
  showToast(`已更新 ${ok}/${outdated.length} 个目录`)
}

/** 主按钮：有没装的 → 一键安装；全装了但有落后的 → 一键更新；都齐了 → 已全部安装（置灰） */
function onPrimaryAction() {
  if (pendingCount.value) void installAll()
  else if (outdatedCount.value) void updateAll()
}

/** 选择自定义目录安装（撞同名时按钮变为「确认覆盖」） */
async function pickCustom() {
  if (opPath.value) return
  if (customConfirm.value) {
    const path = customConfirm.value
    customConfirm.value = null
    opPath.value = path
    try {
      overview.value = await tauriApi.installSkill(path, true)
      showToast('已安装到自定义目录')
    } catch (e) {
      showToast(`安装失败：${String(e)}`)
    } finally {
      opPath.value = ''
    }
    return
  }
  const picked = await open({
    directory: true,
    multiple: false,
    title: '选择 AI 助手的 skills 目录',
  })
  if (typeof picked !== 'string') return
  opPath.value = picked
  try {
    overview.value = await tauriApi.installSkill(picked, false)
    showToast('已安装到自定义目录')
  } catch (e) {
    const msg = String(e)
    if (msg.includes('CONFLICT')) {
      armConfirm(picked, true)
      showToast('该目录已存在同名技能，再点一次「选择目录安装」确认覆盖')
    } else {
      showToast(`安装失败：${msg}`)
    }
  } finally {
    opPath.value = ''
  }
}

onMounted(() => {
  void load()
})
</script>

<template>
  <section id="sv-sec-skills" class="sv-sec" aria-label="Skills">
    <h3 class="sv-sec-title">Skills</h3>

    <p class="skill-hint">
      把「扩展开发技能包」装到本机 AI 编码助手（Claude Code / ZCode / Codex / 豆包 等）的 skills 目录，装完后对助手说「用 x-hub-extension 给我做一个 XX 扩展」即可。
    </p>

    <!-- 内置技能包卡片 -->
    <div v-if="skill" class="skill-card">
      <div class="skill-card-head">
        <Sparkles :size="15" :stroke-width="2" class="skill-card-icon" aria-hidden="true" />
        <span class="skill-card-name">{{ skill.name }}</span>
        <span class="skill-meta">随客户端 v{{ skill.app_version }}</span>
      </div>
      <p class="skill-card-desc">
        让助手按 x-hub 扩展规范生成扩展（manifest + 入口页面 + 可选 service 后端）：{{ skill.file_count }} 个文件 /
        {{ fmtSize(skill.size) }}，一键装到下面的助手目录。
      </p>
      <div class="skill-actions">
        <button
          class="pill-btn"
          type="button"
          :disabled="installingAll || updatingAll || !!opPath || (!pendingCount && !outdatedCount)"
          @click="onPrimaryAction"
        >
          <Download :size="14" :stroke-width="2" />
          {{
            installingAll
              ? '安装中…'
              : updatingAll
                ? '更新中…'
                : pendingCount
                  ? `一键安装（${pendingCount}）`
                  : outdatedCount
                    ? `一键更新（${outdatedCount}）`
                    : '已全部安装'
          }}
        </button>
        <button class="ghost-btn data-btn" type="button" :disabled="!!opPath" @click="pickCustom">
          <FolderPlus :size="14" :stroke-width="2" />
          {{ customConfirm ? '确认覆盖到所选目录' : '选择目录安装' }}
        </button>
      </div>
    </div>

    <h4 class="sv-subtitle">检测到的助手目录</h4>
    <p v-if="!targets.length" class="skill-empty">
      没检测到已知 AI 助手的 skills 目录。可用上面的「选择目录安装」装到自定义位置。
    </p>

    <div v-for="t in targets" :key="t.path" class="setting-row">
      <div class="setting-info">
        <span class="setting-name">
          {{ t.label }}
          <b v-if="t.installed && t.up_to_date" class="skill-badge">已安装</b>
          <b v-else-if="t.installed" class="skill-badge warn">可更新</b>
        </span>
        <span class="dev-dir-path" :title="t.path">{{ t.path }}</span>
        <span class="skill-state" :class="{ warn: t.foreign || (t.installed && !t.up_to_date) }">
          {{ statusText(t) }}
        </span>
      </div>
      <div class="skill-row-actions">
        <button
          v-if="!t.up_to_date"
          class="ghost-btn data-btn"
          type="button"
          :disabled="!!opPath"
          :class="{ confirm: confirmPath === t.path }"
          @click="onAction(t)"
        >
          <Download v-if="!t.installed" :size="14" :stroke-width="2" />
          <RefreshCw v-else :size="14" :stroke-width="2" />
          {{ confirmPath === t.path ? '确认覆盖' : t.installed ? '更新' : '安装' }}
        </button>
        <button
          v-if="t.installed"
          class="ghost-btn data-btn danger"
          type="button"
          :disabled="!!opPath"
          @click="uninstall(t)"
        >
          <Trash2 :size="14" :stroke-width="2" />
          卸载
        </button>
        <button
          v-if="t.kind === 'custom'"
          class="ghost-btn data-btn"
          type="button"
          :disabled="!!opPath"
          title="只从列表移除，不删磁盘文件"
          @click="forgetCustom(t)"
        >
          移除列表
        </button>
      </div>
    </div>

    <p class="settings-foot">
      <Lock :size="12" :stroke-width="2" class="settings-lock" aria-hidden="true" />
      技能包随客户端内置（版本一致）；自动探测只列出已存在的目录，不会新建
    </p>
  </section>
</template>

<style scoped>
/* 本分区专属样式（通用设置样式在 settings/shared.css，见约定 50） */
.skill-hint {
  margin: 0 0 12px;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-2);
}
.skill-card {
  margin-bottom: 14px;
  padding: 12px 14px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-lg);
  background: var(--bg-card-soft);
}
.skill-card-head {
  display: flex;
  align-items: center;
  gap: 6px;
}
.skill-card-icon {
  color: var(--brand-500);
}
.skill-card-name {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 0.82rem;
  font-weight: 650;
  color: var(--text-1);
}
.skill-meta {
  margin-left: auto;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--bg-card);
  font-size: 0.66rem;
  color: var(--text-3);
}
.skill-card-desc {
  margin: 8px 0 10px;
  font-size: 0.75rem;
  line-height: 1.6;
  color: var(--text-2);
}
.skill-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.skill-empty {
  margin: 0 0 10px;
  font-size: 0.72rem;
  line-height: 1.6;
  color: var(--text-3);
}
.skill-badge {
  margin-left: 6px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--bg-card-soft);
  font-size: 0.66rem;
  font-weight: 600;
  color: var(--c-green-ink, var(--text-2));
}
.skill-badge.warn {
  color: var(--c-orange-ink, var(--text-2));
}
.skill-state {
  font-size: 0.72rem;
  color: var(--text-3);
}
.skill-state.warn {
  color: var(--c-orange-ink, var(--text-2));
}
.skill-row-actions {
  display: flex;
  flex-shrink: 0;
  gap: 6px;
}
</style>
