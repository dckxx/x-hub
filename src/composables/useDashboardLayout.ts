import { computed, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

/**
 * 工作台自定义布局（设置页两栏编辑器 + 主界面渲染共用）
 *
 * - 12 列 × 15 行整数格棋盘；编辑器预览行高 1fr 均分画布（12×15 填满、窗口缩放自适应），主界面按 1fr 填满视口
 * - 模块库：左侧「未放置」模块；画布：右侧「已放置」模块，单实例（左右二选一）
 * - 拖入 = 放置；画布内拖动 = 移动；右下角拖拽 = 调整尺寸（最小 2×2）；删除 = 退回左侧库
 * - 每个放置项自带 w/h（初始取模块默认尺寸 4×3，可缩放），宽以 12 格为上限
 * - 自由布局：模块放到哪就落在哪（不左上贪心压实）；目标被占时自动向下找最近空位落位（不弹回）
 * - 草稿语义：进入编辑器 beginEdit 快照，确认 commitEdit 才写库，未确认切走 cancelEdit 恢复编辑前
 * - 持久化到应用配置（AppConfig.dashboard_layout，经 Rust config.json 落盘）；浏览器预览回退 localStorage
 * - 单例状态：编辑器与主界面共享同一份 placements，变更实时同步
 */

export const DASH_COLS = 12
export const MIN_SIZE = 2

/** 旧版 localStorage 存储 key（仅用于迁移到应用配置，迁移后清除） */
const STORAGE_KEY = 'xhub.dashboard.layout.v2'

export interface DashModuleDef {
  id: string
  title: string
  w: number
  h: number
}

export interface DashPlacement {
  id: string
  x: number
  y: number
  w: number
  h: number
}

/** 模块目录：w/h 为「拖入时的初始尺寸」，统一 4×3 方便摆入右侧空位；拖入后可在画布内缩放（最小 2×2） */
export const DASH_MODULES: DashModuleDef[] = [
  { id: 'clock', title: '时钟', w: 4, h: 3 },
  { id: 'sysmon', title: '系统资源', w: 4, h: 3 },
  { id: 'sticky1', title: '便签 1', w: 4, h: 3 },
  { id: 'sticky2', title: '便签 2', w: 4, h: 3 },
  { id: 'notes', title: '速记概览', w: 4, h: 3 },
  { id: 'todo_overview', title: '待办概览', w: 4, h: 3 },
  { id: 'resources', title: '速达数量', w: 4, h: 3 },
  { id: 'countdown', title: '倒计时', w: 4, h: 3 },
  { id: 'prompts', title: '提示词', w: 4, h: 3 },
  { id: 'todo', title: '待办', w: 4, h: 3 },
  { id: 'recent', title: '最近使用', w: 4, h: 3 },
]

const moduleMap = new Map(DASH_MODULES.map((m) => [m.id, m]))

/** 扩展 module 动态注册表（运行时从 listExtensions 填充；id 用 `ext:<扩展id>` 前缀与内置模块区分） */
const extensionModules = ref<DashModuleDef[]>([])

export function registerExtensionModules(mods: DashModuleDef[]) {
  extensionModules.value = mods
}

export function dashModuleDef(id: string): DashModuleDef | undefined {
  if (id.startsWith('ext:')) return extensionModules.value.find((m) => m.id === id)
  return moduleMap.get(id)
}

export function dashModuleTitle(id: string): string {
  return dashModuleDef(id)?.title ?? id
}

/** 推荐布局：8 个模块按 12×15 棋盘整齐拼满、无空洞；Token/速记/待办概览/速达数量等留待用户自行拖入 */
const PRESET: DashPlacement[] = [
  { id: 'clock', x: 0, y: 0, w: 4, h: 3 },
  { id: 'countdown', x: 4, y: 0, w: 5, h: 4 },
  { id: 'todo', x: 9, y: 0, w: 3, h: 12 },
  { id: 'sysmon', x: 0, y: 3, w: 4, h: 3 },
  { id: 'prompts', x: 4, y: 4, w: 5, h: 8 },
  { id: 'sticky1', x: 0, y: 6, w: 2, h: 6 },
  { id: 'sticky2', x: 2, y: 6, w: 2, h: 6 },
  { id: 'recent', x: 0, y: 12, w: 12, h: 3 },
]

/** 默认布局：首次启动/空布局回退到推荐模板（不再空白） */
function defaultPlacements(): DashPlacement[] {
  return PRESET.map((p) => ({ ...p }))
}

function clampSize(n: number): number {
  return Math.max(MIN_SIZE, Math.round(n))
}

function collides(a: DashPlacement, b: DashPlacement) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

const store = useStore()

/** 解析 JSON 字符串为合法 placements（校验 id/x/y 并钳制 w/h/x/y），失败返回 null */
function parsePlacements(raw: string): DashPlacement[] | null {
  try {
    const saved = JSON.parse(raw) as Array<Partial<DashPlacement>>
    const valid = saved
      .filter((s) => s && dashModuleDef(s.id!) && Number.isInteger(s.x) && Number.isInteger(s.y))
      .map((s) => {
        const def = dashModuleDef(s.id!)!
        const w = Number.isInteger(s.w) ? Math.min(clampSize(s.w as number), DASH_COLS) : def.w
        const h = Number.isInteger(s.h) ? clampSize(s.h as number) : def.h
        const x = Math.min(Math.max(s.x!, 0), DASH_COLS - w)
        const y = Math.max(s.y!, 0)
        return { id: s.id!, x, y, w, h }
      })
    if (valid.length) return valid
  } catch {
    // 忽略损坏数据
  }
  return null
}

/** 旧 localStorage 数据（迁移用） */
function loadFromLocalStorage(): DashPlacement[] | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) return parsePlacements(raw)
  } catch {
    // 忽略
  }
  return null
}

// ---- 持久化：写应用配置（Tauri）/ 回退 localStorage（浏览器预览） ----
function persist() {
  const data = JSON.stringify(
    placements.value.map((p) => ({ id: p.id, x: p.x, y: p.y, w: p.w, h: p.h })),
  )
  if (isTauri()) {
    void store.setDashboardLayout(data)
  } else {
    try {
      localStorage.setItem(STORAGE_KEY, data)
    } catch {
      // 存储失败静默，不影响交互
    }
  }
  syncCommitted()
}

// ---- 草稿机制：编辑器「确认」才提交，未确认切走回滚 ----
let editSnapshot: DashPlacement[] | null = null

function persistIfIdle() {
  // 仅非编辑状态（编辑器之外）即时持久化；编辑期间的变更统一由 commitEdit 提交
  if (!editSnapshot) persist()
}

function beginEdit() {
  if (editSnapshot) return
  editSnapshot = placements.value.map((p) => ({ ...p }))
}

function commitEdit() {
  if (!editSnapshot) return
  editSnapshot = null
  persist()
}

function cancelEdit() {
  if (!editSnapshot) return
  placements.value = editSnapshot.map((p) => ({ ...p }))
  editSnapshot = null
}

// ---- 单例状态：所有调用方共享同一份布局，编辑器变更后主界面立即同步 ----
// 初始先读 localStorage（浏览器预览 / 老数据），config 加载完成后再对齐到应用配置
const placements = ref<DashPlacement[]>(loadFromLocalStorage() ?? defaultPlacements())

// ---- 倒计时卡片可见性上报 ----
// 以「已提交（落盘）」的布局为准：编辑器草稿期间的增删不算，落盘后才同步到后端。
// 卡片不在工作台时后端冻结全部非浮窗倒计时（不计时、到点不提醒），恢复显示时续跑。
const committed = ref<DashPlacement[]>([])
const hasCountdownCard = computed(() => committed.value.some((p) => p.id === 'countdown'))

function reportCountdownCardVisible() {
  if (!isTauri() || getCurrentWindow().label !== 'main') return
  void tauriApi.setCountdownCardVisible(hasCountdownCard.value).catch(() => {
    // 后端未就绪时静默，下次布局同步再上报
  })
}

function syncCommitted() {
  committed.value = placements.value.map((p) => ({ ...p }))
  reportCountdownCardVisible()
}

/** 加载声明 module 形态的扩展，注册进工作台模块库（id 用 `ext:<扩展id>` 前缀与内置模块区分） */
export async function loadExtensionModules() {
  if (!isTauri()) return
  try {
    const exts = await tauriApi.listExtensions()
    registerExtensionModules(
      exts
        .filter((e) => !e.invalid && e.surfaces.includes('module'))
        .map((e) => ({ id: `ext:${e.id}`, title: e.name, w: 4, h: 3 })),
    )
  } catch {
    // 命令未就绪时保持无扩展模块
  }
}

// config 就绪后：先加载扩展模块再恢复布局（避免 config 里的 ext: 模块在 parse 时被过滤）
// 优先读 AppConfig.dashboard_layout；为空则把 localStorage 老数据迁移进 config；否则回退推荐布局
watch(
  () => store.state.loaded,
  async (loaded) => {
    if (!loaded) return
    await loadExtensionModules()
    const cfg = store.state.config.dashboard_layout
    if (cfg) {
      const parsed = parsePlacements(cfg)
      if (parsed) {
        placements.value = parsed
        // 已迁移到 config，清理旧 localStorage 数据
        try {
          localStorage.removeItem(STORAGE_KEY)
        } catch {
          // 忽略
        }
        syncCommitted()
        return
      }
    }
    const ls = loadFromLocalStorage()
    if (ls) {
      placements.value = ls
      persist() // persist 内部已 syncCommitted + 上报
      try {
        localStorage.removeItem(STORAGE_KEY)
      } catch {
        // 忽略
      }
      return
    }
    placements.value = defaultPlacements()
    syncCommitted()
  },
)

/** 左侧库 = 未放置的模块（内置 + 扩展，保持目录顺序） */
const available = computed(() => {
  const all = [...DASH_MODULES, ...extensionModules.value]
  return all.filter((m) => !placements.value.some((p) => p.id === m.id))
})

/** 目标矩形是否与除自身外的其他模块重叠 */
function overlaps(rect: DashPlacement, ignoreId: string): boolean {
  return placements.value.some((q) => q.id !== ignoreId && collides(rect, q))
}

/** 在目标列带内，从 y 向下找第一个能容纳 w×h 且不与现有模块碰撞的位置（超出底部则落在布局最下方） */
export function findFreeSpot(
  placements: DashPlacement[],
  w: number,
  h: number,
  x: number,
  y: number,
): { x: number; y: number } {
  const cx = Math.min(Math.max(x, 0), DASH_COLS - w)
  let maxY = 0
  for (const p of placements) maxY = Math.max(maxY, p.y + p.h)
  const startY = Math.max(y, 0)
  const limit = maxY + h
  for (let yy = startY; yy <= limit; yy++) {
    const rect: DashPlacement = { id: '', x: cx, y: yy, w, h }
    if (!placements.some((q) => collides(rect, q))) return { x: cx, y: yy }
  }
  return { x: cx, y: startY }
}

function addModule(id: string, x: number, y: number): boolean {
  if (placements.value.some((p) => p.id === id)) return false
  const def = dashModuleDef(id)!
  // 目标位置被占时自动向下找最近的空位，拖入的模块总能落进布局
  const spot = findFreeSpot(placements.value, def.w, def.h, x, y)
  placements.value.push({ id, x: spot.x, y: spot.y, w: def.w, h: def.h })
  persistIfIdle()
  return true
}

function removeModule(id: string) {
  placements.value = placements.value.filter((p) => p.id !== id)
  persistIfIdle()
}

function moveModule(id: string, x: number, y: number): boolean {
  const p = placements.value.find((q) => q.id === id)
  if (!p) return false
  const rect: DashPlacement = {
    ...p,
    x: Math.min(Math.max(x, 0), DASH_COLS - p.w),
    y: Math.max(y, 0),
  }
  if (overlaps(rect, id)) {
    // 目标被占：在目标列带内向下找最近空位（与拖入一致，移动总能落位、不弹回）
    const others = placements.value.filter((q) => q.id !== id)
    const spot = findFreeSpot(others, p.w, p.h, rect.x, rect.y)
    p.x = spot.x
    p.y = spot.y
  } else {
    p.x = rect.x
    p.y = rect.y
  }
  persistIfIdle()
  return true
}

/** 调整尺寸：最小 2×2，宽不超过 12 列右边界；碰撞时拒绝 */
function resizeModule(id: string, w: number, h: number): boolean {
  const p = placements.value.find((q) => q.id === id)
  if (!p) return false
  const nw = Math.min(clampSize(w), DASH_COLS - p.x)
  const nh = clampSize(h)
  const rect: DashPlacement = { ...p, w: nw, h: nh }
  if (overlaps(rect, id)) return false
  p.w = nw
  p.h = nh
  persistIfIdle()
  return true
}

function applyPreset() {
  placements.value = PRESET.map((p) => ({ ...p }))
  persistIfIdle()
}

function clear() {
  placements.value = []
  persistIfIdle()
}

export function useDashboardLayout() {
  return {
    placements,
    available,
    addModule,
    removeModule,
    moveModule,
    resizeModule,
    applyPreset,
    clear,
    beginEdit,
    commitEdit,
    cancelEdit,
  }
}
