<script setup lang="ts">
// 数据与关于大类（存储路径 / 备份恢复 / 速记维护 / 关于）
//
// 从 SettingsView.vue 拆出（见该文件顶部说明）：设置页按大类按需加载，
// 首次打开只需外壳 + 当前大类的代码，切大类时才加载对应面板。
import { computed, inject, onMounted, ref } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { Download, Eraser, FolderCog, FolderOutput, Lock, Upload } from 'lucide-vue-next';
import { isTauri, tauriApi } from '../../api/tauri';
import type { DataPathInfo, NoteImageGcReport } from '../../api/tauri';
import { useStore } from '../../stores/workbench';
import AppSelect from '../AppSelect.vue';
import AboutSection from '../AboutSection.vue';

const showToast = inject<(msg: string) => void>('showToast', () => {})
const store = useStore()

// ---- 数据存储路径 ----
const dataPathInfo = ref<DataPathInfo | null>(null)
const changeDataTarget = ref<string | null>(null)
const changeDataBusy = ref(false)

const dataPathLabel = computed(() => {
  const info = dataPathInfo.value
  if (!info) return '加载中…'
  if (info.mode === 'portable') return `便携版 · ${info.path}`
  return info.path
})

async function loadDataPath() {
  if (!isTauri()) return
  try {
    dataPathInfo.value = await tauriApi.getDataPath()
  } catch (e) {
    showToast(`读取数据路径失败：${String(e)}`)
  }
}

async function onChangeDataDir() {
  if (!isTauri() || dataPathInfo.value?.mode === 'portable') return
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  if (dir === dataPathInfo.value?.path) {
    showToast('所选目录与当前目录相同')
    return
  }
  changeDataTarget.value = dir
}

function cancelChangeDataDir() {
  changeDataTarget.value = null
}

async function confirmChangeDataDir() {
  if (!changeDataTarget.value || changeDataBusy.value) return
  changeDataBusy.value = true
  try {
    await tauriApi.changeDataDir(changeDataTarget.value)
    changeDataTarget.value = null
    showToast('数据已迁移，即将重启')
    setTimeout(() => void tauriApi.restartApp(), 700)
  } catch (e) {
    showToast(`迁移失败：${String(e)}`)
  } finally {
    changeDataBusy.value = false
  }
}

// ---- 数据备份 / 恢复 ----
const confirmRestore = ref(false)
let confirmTimer: ReturnType<typeof setTimeout> | null = null

async function backupData() {
  if (!isTauri()) return
  const dir = await open({ multiple: false, directory: true })
  if (typeof dir !== 'string') return
  try {
    const name = await tauriApi.backupData(dir)
    showToast(`备份完成：${name}`)
  } catch (e) {
    showToast(`备份失败：${String(e)}`)
  }
}

async function restoreData() {
  if (!isTauri()) return
  // 两段式确认：第二次点击才执行
  if (!confirmRestore.value) {
    confirmRestore.value = true
    if (confirmTimer) clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => {
      confirmRestore.value = false
    }, 3000)
    return
  }
  confirmRestore.value = false
  const file = await open({
    multiple: false,
    directory: false,
    filters: [{ name: '备份压缩包', extensions: ['zip'] }],
  })
  if (typeof file !== 'string') return
  try {
    await tauriApi.restoreData(file)
    showToast('恢复已暂存，重启应用后生效')
  } catch (e) {
    showToast(`恢复失败：${String(e)}`)
  }
}

// ---- 速记维护：回收站保留天数 / 孤儿图片清理 / 导出导入（docs/speednote-plan.md） ----
const RETENTION_OPTIONS = [
  { value: '0', label: '永久保留' },
  { value: '7', label: '保留 7 天' },
  { value: '30', label: '保留 30 天' },
  { value: '90', label: '保留 90 天' },
  { value: '365', label: '保留 1 年' },
]

const noteRetention = computed({
  get: () => String(store.state.config.note_trash_retention_days ?? 0),
  set: (v: string) => void setNoteRetention(Number(v) || 0),
})

async function setNoteRetention(days: number) {
  if (!isTauri()) return
  try {
    const r = await store.setNoteTrashRetention(days)
    showToast(r && r.purged > 0 ? `已清理回收站 ${r.purged} 条过期笔记` : '回收站保留策略已更新')
  } catch (e) {
    showToast(`设置失败：${String(e)}`)
  }
}

const gcBusy = ref(false)
const gcReport = ref<NoteImageGcReport | null>(null)

async function scanOrphanImages() {
  if (!isTauri() || gcBusy.value) return
  gcBusy.value = true
  try {
    gcReport.value = await tauriApi.gcOrphanNoteImages(true)
  } catch (e) {
    showToast(`扫描失败：${String(e)}`)
  } finally {
    gcBusy.value = false
  }
}

async function cleanOrphanImages() {
  if (!isTauri() || gcBusy.value || !gcReport.value) return
  gcBusy.value = true
  try {
    const r = await tauriApi.gcOrphanNoteImages(false)
    gcReport.value = r
    showToast(
      r.failed > 0
        ? `已删除 ${r.removed} 张孤儿图片，${r.failed} 张删除失败（可能被占用）`
        : `已删除 ${r.removed} 张孤儿图片`,
    )
  } catch (e) {
    showToast(`清理失败：${String(e)}`)
  } finally {
    gcBusy.value = false
  }
}

const exportBusy = ref(false)

async function exportNotes() {
  if (!isTauri() || exportBusy.value) return
  const dir = await open({ multiple: false, directory: true, title: '选择导出目录' })
  if (typeof dir !== 'string') return
  exportBusy.value = true
  try {
    const r = await tauriApi.exportNotes(dir)
    showToast(
      r.failed.length > 0
        ? `导出 ${r.exported} 条笔记（${r.failed.length} 条失败）→ ${r.dir}`
        : `已导出 ${r.exported} 条笔记、${r.images} 张图片 → ${r.dir}`,
    )
  } catch (e) {
    showToast(`导出失败：${String(e)}`)
  } finally {
    exportBusy.value = false
  }
}

const importBusy = ref(false)
const importing = ref(false)

async function importNotes() {
  if (!isTauri() || importBusy.value) return
  const dir = await open({ multiple: false, directory: true, title: '选择导入目录（x-hub 导出的产物）' })
  if (typeof dir !== 'string') return
  importBusy.value = true
  importing.value = true
  try {
    const r = await tauriApi.importNotes(dir, { overwrite: false })
    importing.value = false
    void store.refreshNotes()
    void store.refreshNoteFolders()
    const failNote = r.failed.length > 0 ? `，${r.failed.length} 条失败` : ''
    const cancelNote = r.cancelled ? '（已取消）' : ''
    showToast(
      `导入完成：成功 ${r.imported} 条、跳过 ${r.skipped} 条${failNote}${cancelNote}`,
    )
  } catch (e) {
    showToast(`导入失败：${String(e)}`)
  } finally {
    importing.value = false
    importBusy.value = false
  }
}

async function cancelImport() {
  try {
    await tauriApi.importNotesCancel()
  } catch {
    /* 无导入在进行时静默 */
  }
}

onMounted(() => {

  void loadDataPath()

})
</script>

<template>
        <section id="sv-sec-data" class="sv-sec" aria-label="数据">
          <h3 class="sv-sec-title">数据</h3>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据存储路径</span>
              <span class="setting-desc data-path">{{ dataPathLabel }}</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :disabled="dataPathInfo?.mode === 'portable'"
              :title="dataPathInfo?.mode === 'portable' ? '便携版数据跟随程序目录，不可更改' : '更改数据存储目录'"
              @click="onChangeDataDir"
            >
              <FolderCog :size="14" :stroke-width="2" />
              更改
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据备份</span>
              <span class="setting-desc">数据库与图标，打包成压缩包</span>
            </div>
            <button class="ghost-btn data-btn" @click="backupData">
              <Download :size="14" :stroke-width="2" />
              备份
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">数据恢复</span>
              <span class="setting-desc">从备份压缩包恢复，重启后生效</span>
            </div>
            <button
              class="ghost-btn data-btn"
              :class="{ confirm: confirmRestore }"
              @click="restoreData"
            >
              <Upload :size="14" :stroke-width="2" />
              {{ confirmRestore ? '确认恢复？' : '恢复' }}
            </button>
          </div>

          <h3 class="sv-sec-title gap-above">速记维护</h3>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">回收站保留天数</span>
              <span class="setting-desc">速记删除先进回收站；到期自动清理（0 = 永久保留），启动与更改设置时生效</span>
            </div>
            <AppSelect
              v-model="noteRetention"
              :options="RETENTION_OPTIONS"
              style="width: 140px"
            />
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">孤儿图片清理</span>
              <span class="setting-desc">
                <template v-if="gcReport">
                  共 {{ gcReport.total_files }} 张图片，{{ gcReport.referenced }} 张被笔记引用
                  <template v-if="gcReport.orphan_files.length">，{{ gcReport.orphan_files.length }} 张孤儿</template>
                </template>
                <template v-else>扫描未被任何笔记引用的图片（含回收站笔记的引用），先扫描后清理</template>
              </span>
            </div>
            <button class="ghost-btn data-btn" :disabled="gcBusy" @click="scanOrphanImages">
              <Eraser :size="14" :stroke-width="2" />
              {{ gcBusy ? '处理中…' : gcReport ? '重新扫描' : '扫描' }}
            </button>
            <button
              v-if="gcReport && gcReport.orphan_files.length > 0"
              class="ghost-btn data-btn"
              :disabled="gcBusy"
              @click="cleanOrphanImages"
            >
              清理 {{ gcReport.orphan_files.length }} 张
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">导出笔记</span>
              <span class="setting-desc">按文件夹导出为 Markdown（含 front-matter 与图片 assets），可再导入回来</span>
            </div>
            <button class="ghost-btn data-btn" :disabled="exportBusy" @click="exportNotes">
              <FolderOutput :size="14" :stroke-width="2" />
              {{ exportBusy ? '导出中…' : '导出' }}
            </button>
          </div>

          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-name">导入笔记</span>
              <span class="setting-desc">仅支持导入 x-hub 自己导出的产物；来源相同（source_url）的笔记默认跳过</span>
            </div>
            <button class="ghost-btn data-btn" :disabled="importBusy" @click="importNotes">
              <Download :size="14" :stroke-width="2" />
              {{ importBusy ? '导入中…' : '导入' }}
            </button>
            <button v-if="importing" class="ghost-btn data-btn" @click="cancelImport">取消</button>
          </div>

          <p class="settings-foot">
            <Lock :size="12" :stroke-width="2" class="settings-lock" aria-hidden="true" />
            所有数据默认存储在本地，不会上传云端
          </p>
        </section>

        <!-- 更改数据存储路径确认弹窗 -->
        <Teleport to="body">
          <Transition name="mask">
            <div
              v-if="changeDataTarget"
              class="modal-mask"
              role="presentation"
              @click.self="cancelChangeDataDir"
            >
              <div
                class="modal-card data-move-card"
                role="dialog"
                aria-modal="true"
                aria-label="更改数据存储路径"
              >
                <h3 class="dm-title">迁移数据目录</h3>
                <p class="dm-desc">是否确认将 x-hub 的所有数据挪到以下目录？确认后将重启软件。</p>
                <div class="dm-paths">
                  <div class="dm-path">
                    <span class="dm-label">新目录</span>
                    <span class="dm-val">{{ changeDataTarget }}</span>
                  </div>
                </div>
                <footer class="dm-footer">
                  <button class="ghost-btn" type="button" @click="cancelChangeDataDir">取消</button>
                  <button
                    class="pill-btn"
                    type="button"
                    :disabled="changeDataBusy"
                    @click="confirmChangeDataDir"
                  >
                    {{ changeDataBusy ? '迁移中…' : '确认迁移' }}
                  </button>
                </footer>
              </div>
            </div>
          </Transition>
        </Teleport>

        <section id="sv-sec-about" class="sv-sec" aria-label="关于">
          <h3 class="sv-sec-title">关于</h3>
          <AboutSection />
        </section>
</template>

<style scoped>
/* 迁移数据弹窗：Teleport 到 body，不在 .settings-view 子树里，只能靠 scoped 命中 */
.data-move-card {
  width: 480px;
  max-width: calc(100vw - 48px);
}
.dm-title {
  margin: 0 0 10px;
  font-size: 1rem;
  font-weight: 700;
  color: var(--text-1);
}
.dm-desc {
  margin: 0 0 14px;
  font-size: 0.8125rem;
  line-height: 1.6;
  color: var(--text-2);
}
.dm-paths {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 16px;
}
.dm-path {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-card-soft);
}
.dm-label {
  font-size: 0.6875rem;
  font-weight: 700;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.dm-val {
  font-size: 0.75rem;
  color: var(--text-2);
  overflow-wrap: anywhere;
}
.dm-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--border-soft);
}

/* 弹窗遮罩过渡 */
.mask-enter-active,
.mask-leave-active {
  transition: opacity 0.18s ease-out;
}
.mask-enter-from,
.mask-leave-to {
  opacity: 0;
}
</style>
