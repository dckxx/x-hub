import { computed, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri, tauriApi } from '../api/tauri'
import { useStore } from '../stores/workbench'

/**
 * 工作台自定义布局（设置页两栏编辑器 + 主界面渲染共用）
 *
 * - 12 列 × 15 行整数格棋盘；编辑器预览行高 1fr 均分画布（12×15 填满、窗口缩放自适应），主界面按 1fr 填满视口
 * - 形态注册表：每个模块声明一个或多个「形态」，每个形态带 min（最小完整尺寸）/ ideal（推荐尺寸）
 *   - 布局项记住用户选的形态（DashPlacement.variant），真实工作台按形态渲染对应内容
 *   - 老数据无 variant → 回退模块 defaultVariant，天然兼容
 * - 模块库：左侧「未放置」模块；画布：右侧「已放置」模块，单实例（左右二选一）
 * - 拖入 = 放置（按当前形态推荐尺寸落位）；画布内拖动 = 移动；右下角拖拽 = 调整尺寸（钳制到形态最小尺寸）；删除 = 退回左侧库
 * - 自由布局：模块放到哪就落在哪（不左上贪心压实）；目标被占时自动向下找最近空位落位（不弹回）
 * - 草稿语义：进入编辑器 beginEdit 快照，确认 commitEdit 才写库，未确认切走 cancelEdit 恢复编辑前
 * - 持久化到应用配置（AppConfig.dashboard_layout，经 Rust config.json 落盘）；浏览器预览回退 localStorage
 * - 默认布局快照：编辑器「存为默认布局」把当前画布存进 AppConfig.dashboard_default_layout（预览回退 localStorage），
 *   「恢复默认布局」随时一键回到快照——编辑器内 = 回到草稿（确认后生效），编辑器外 = 立即生效
 * - 单例状态：编辑器与主界面共享同一份 placements，变更实时同步
 */

export const DASH_COLS = 12
export const MIN_SIZE = 2

/** 自定义标题最大长度（编辑器输入框与解析共用一个口径） */
export const TITLE_MAX = 24

/** 旧版 localStorage 存储 key（仅用于迁移到应用配置，迁移后清除） */
const STORAGE_KEY = 'xhub.dashboard.layout.v2'

/** 默认布局快照的 localStorage key（仅浏览器预览用；Tauri 走 AppConfig.dashboard_default_layout） */
const STORAGE_KEY_DEFAULT = 'xhub.dashboard.layout.default.v1'

/** 形态定义：min = 内容完整展示的最小格子，ideal = 内容正好铺满的推荐格子 */
export interface DashVariantDef {
  id: string
  name: string
  minW: number
  minH: number
  idealW: number
  idealH: number
  desc: string
}

export interface DashModuleDef {
  id: string
  title: string
  defaultVariant: string
  variants: DashVariantDef[]
  /** 作者声明的表头默认显隐（仅扩展 module 有，加载时已解析为布尔：未声明 = true；内置模块恒为 false/显示） */
  defaultHideTitle?: boolean
}

export interface DashPlacement {
  id: string
  x: number
  y: number
  w: number
  h: number
  /** 用户选的形态 id（缺省 = 模块 defaultVariant，老数据自动回退） */
  variant?: string
  /** 用户自定义的卡片标题（缺省 = 模块内置标题 dashModuleTitle） */
  title?: string
  /** 关闭标题行（true = 不渲染表头，卡片自行补偿上边距） */
  hideTitle?: boolean
}

function v(
  id: string,
  name: string,
  minW: number,
  minH: number,
  idealW: number,
  idealH: number,
  desc: string,
): DashVariantDef {
  return { id, name, minW, minH, idealW, idealH, desc }
}

/** 模块目录 + 形态注册表：多形态只有真渲染的模块才暴露（clock/weather），其余单形态保持现状 */
export const DASH_MODULES: DashModuleDef[] = [
  {
    id: 'clock',
    title: '时钟',
    defaultVariant: 'big',
    variants: [
      v('big', '大时钟', 3, 3, 4, 3, '时间 + 日期 + 天气 + 语录'),
      v('lunar', '今日阴阳历', 2, 2, 3, 3, '时间 + 阳历 + 农历'),
      v('minimal', '极简时间', 2, 1, 2, 2, '只留大号时间'),
    ],
  },
  {
    id: 'weather',
    title: '天气',
    defaultVariant: 'now',
    variants: [
      v('now', '简版', 2, 1, 2, 2, '图标 + 温度 + 城市'),
      v('detail', '详情版', 3, 3, 4, 4, '体感 / 湿度 / 风 / 紫外线'),
    ],
  },
  {
    id: 'sysmon',
    title: '系统资源',
    defaultVariant: 'monitor',
    variants: [v('monitor', '监视器', 2, 1, 4, 3, 'CPU / 内存')],
  },
  {
    id: 'sticky1',
    title: '便签 1',
    defaultVariant: 'note',
    variants: [v('note', '便签', 2, 1, 2, 2, '随手记')],
  },
  {
    id: 'sticky2',
    title: '便签 2',
    defaultVariant: 'note',
    variants: [v('note', '便签', 2, 1, 2, 2, '随手记')],
  },
  {
    id: 'notes',
    title: '速记概览',
    defaultVariant: 'overview',
    variants: [v('overview', '概览', 3, 2, 4, 3, '最近笔记')],
  },
  {
    id: 'todo_overview',
    title: '待办概览',
    defaultVariant: 'overview',
    variants: [v('overview', '概览', 2, 1, 2, 2, '今日进度')],
  },
  {
    id: 'resources',
    title: '速达数量',
    defaultVariant: 'overview',
    variants: [v('overview', '概览', 2, 1, 2, 2, '资源计数')],
  },
  {
    // 自定义速达 1..4：固定槽位（同便签 1/2 池子模式），内容各自配置
    // （手动挑选 / 整个大类 / 指定小类，存 AppConfig.suda_custom_modules，见 utils/sudaCustom.ts）
    id: 'suda1',
    title: '自定义速达 1',
    defaultVariant: 'grid',
    variants: [v('grid', '网格', 2, 2, 4, 3, '自定义快捷启动格')],
  },
  {
    id: 'suda2',
    title: '自定义速达 2',
    defaultVariant: 'grid',
    variants: [v('grid', '网格', 2, 2, 4, 3, '自定义快捷启动格')],
  },
  {
    id: 'suda3',
    title: '自定义速达 3',
    defaultVariant: 'grid',
    variants: [v('grid', '网格', 2, 2, 4, 3, '自定义快捷启动格')],
  },
  {
    id: 'suda4',
    title: '自定义速达 4',
    defaultVariant: 'grid',
    variants: [v('grid', '网格', 2, 2, 4, 3, '自定义快捷启动格')],
  },
  {
    id: 'countdown',
    title: '倒计时',
    defaultVariant: 'list',
    variants: [v('list', '列表', 2, 2, 4, 4, '进行中 + 新建')],
  },
  {
    id: 'prompts',
    title: '提示词',
    defaultVariant: 'list',
    variants: [v('list', '列表', 3, 2, 4, 3, '提示词列表')],
  },
  {
    id: 'todo',
    title: '待办',
    defaultVariant: 'list',
    variants: [v('list', '列表', 3, 3, 4, 5, '优先级列表')],
  },
  {
    id: 'calendar',
    title: '日历',
    defaultVariant: 'month',
    variants: [v('month', '整月日历', 4, 4, 5, 5, '待办分布（含周期待办的虚拟实例）')],
  },
  {
    id: 'recent',
    title: '最近使用',
    defaultVariant: 'bar',
    variants: [v('bar', '通栏', 3, 1, 12, 3, '最近启动的应用，窄格换行、格子越大显示越多')],
  },
]

/**
 * 编辑器画布内直接挂载真实组件（所见即所得）的模块。
 * 其余模块（含扩展 module）用 DashModulePreview 做真实卡片结构的等比缩印；
 * clock / weather 在形态切换浮层的缩略图里也走 DashModulePreview 的静态形态分支。
 */
export function isLivePreview(modId: string): boolean {
  return modId === 'clock' || modId === 'weather'
}

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

/** 卡片实际显示的标题：用户自定义优先，其次模块内置标题 */
export function dashPlacementTitle(p: DashPlacement): string {
  return p.title ?? dashModuleTitle(p.id)
}

/**
 * 卡片是否隐藏表头：用户显式设置优先，其次扩展作者声明的默认（manifest.moduleOptions.defaultHideTitle）；
 * 内置模块没有作者默认，恒为显示。三态语义：undefined = 跟随作者默认，true/false = 用户已明确表态。
 * 扩展的 `defaultHideTitle` 在 loadExtensionModules 里已解析成布尔（未声明 = true = 默认不显示表头）。
 */
export function dashPlacementHideTitle(p: DashPlacement): boolean {
  if (typeof p.hideTitle === 'boolean') return p.hideTitle
  return dashModuleDef(p.id)?.defaultHideTitle === true
}

/** 取模块指定形态；未指定 / 不存在时回退 defaultVariant / 第一个形态 */
export function dashVariantDef(id: string, variant?: string): DashVariantDef | undefined {
  const def = dashModuleDef(id)
  if (!def) return undefined
  return (
    def.variants.find((x) => x.id === variant) ??
    def.variants.find((x) => x.id === def.defaultVariant) ??
    def.variants[0]
  )
}

export function dashModuleVariants(id: string): DashVariantDef[] {
  return dashModuleDef(id)?.variants ?? []
}

export type DashFitLevel = 'below' | 'mid' | 'ideal' | 'room'

/** 格子尺寸 vs 形态 min/ideal 的适配状态（编辑器徽标 + 拖拽警示共用） */
export function dashFitState(p: DashPlacement): { level: DashFitLevel; label: string } {
  const vd = dashVariantDef(p.id, p.variant)
  if (!vd) return { level: 'ideal', label: '' }
  if (p.w < vd.minW || p.h < vd.minH) {
    return { level: 'below', label: `低于最小 ${vd.minW}×${vd.minH}` }
  }
  if (p.w >= vd.idealW && p.h >= vd.idealH) {
    return p.w === vd.idealW && p.h === vd.idealH
      ? { level: 'ideal', label: '正好铺满' }
      : { level: 'room', label: `弹性空间 · 推荐 ${vd.idealW}×${vd.idealH}` }
  }
  return { level: 'mid', label: `紧凑可读 · 推荐 ${vd.idealW}×${vd.idealH}` }
}

/** 推荐布局：沿用改动前的 8 模块模板（历史习惯布局） */
const PRESET: DashPlacement[] = [
  { id: 'clock', variant: 'big', x: 0, y: 0, w: 4, h: 3 },
  { id: 'countdown', variant: 'list', x: 4, y: 0, w: 5, h: 4 },
  { id: 'todo', variant: 'list', x: 9, y: 0, w: 3, h: 12 },
  { id: 'sysmon', variant: 'monitor', x: 0, y: 3, w: 4, h: 3 },
  { id: 'prompts', variant: 'list', x: 4, y: 4, w: 5, h: 8 },
  { id: 'sticky1', variant: 'note', x: 0, y: 6, w: 2, h: 6 },
  { id: 'sticky2', variant: 'note', x: 2, y: 6, w: 2, h: 6 },
  { id: 'recent', variant: 'bar', x: 0, y: 12, w: 12, h: 3 },
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
      .map((s): DashPlacement | null => {
        const vd = dashVariantDef(s.id!, s.variant)
        if (!vd) return null
        const w = Number.isInteger(s.w)
          ? Math.min(Math.max(clampSize(s.w as number), vd.minW), DASH_COLS)
          : vd.idealW
        const h = Number.isInteger(s.h)
          ? Math.max(clampSize(s.h as number), vd.minH)
          : vd.idealH
        const x = Math.min(Math.max(s.x!, 0), DASH_COLS - w)
        const y = Math.max(s.y!, 0)
        // 标题：trim 后为空视为「用默认标题」；超长截断，避免手改 JSON 撑破卡片
        const title =
          typeof s.title === 'string' && s.title.trim() ? s.title.trim().slice(0, TITLE_MAX) : undefined
        // 表头显隐三态：true/false = 用户明确设置；缺省 = 跟随扩展作者声明的默认
        const hideTitle = typeof s.hideTitle === 'boolean' ? s.hideTitle : undefined
        // 用回退后的生效形态 id 归一：无效/过期 variant（JSON 手改、扩展升级改名）不透传
        return { id: s.id!, x, y, w, h, variant: vd.id, title, hideTitle }
      })
      .filter((p): p is DashPlacement => p !== null)
    if (valid.length) {
      // 形态 min 钳制可能放大老数据格子（旧统一 2×2 → 新形态 min 更大），逐项消解重叠：
      // 从上往下扫，与已确认项碰撞的让位到最近空位，正常布局不受影响
      const settled: DashPlacement[] = []
      for (const p of [...valid].sort((a, b) => a.y - b.y || a.x - b.x)) {
        const rect = { ...p }
        if (settled.some((q) => collides(rect, q))) {
          const spot = findFreeSpot(settled, p.w, p.h, p.x, p.y)
          rect.x = spot.x
          rect.y = spot.y
        }
        settled.push(rect)
      }
      return settled
    }
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

/** 当前 placements 序列化为 placements JSON（标题/表头字段只在设置过时写入） */
function serializePlacements(): string {
  return JSON.stringify(
    placements.value.map((p) => {
      const o: DashPlacement = {
        id: p.id,
        x: p.x,
        y: p.y,
        w: p.w,
        h: p.h,
        variant: p.variant,
      }
      // 标题相关字段只在设置过时写入，老数据保持原样（少字段 = 默认标题 + 显示标题行）
      if (p.title) o.title = p.title
      // 表头显隐三态都要落盘：false 也是用户表态（要覆盖扩展作者的 defaultHideTitle）
      if (typeof p.hideTitle === 'boolean') o.hideTitle = p.hideTitle
      return o
    }),
  )
}

// ---- 持久化：写应用配置（Tauri）/ 回退 localStorage（浏览器预览） ----
function persist() {
  const data = serializePlacements()
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

// ---- 默认布局快照：编辑器「存为默认」写入，「恢复默认」读取（Tauri 走 AppConfig，预览回退 localStorage） ----
function loadDefaultFromLocalStorage(): string {
  try {
    return localStorage.getItem(STORAGE_KEY_DEFAULT) ?? ''
  } catch {
    return ''
  }
}
const savedDefaultRaw = ref(isTauri() ? '' : loadDefaultFromLocalStorage())
const hasSavedDefault = computed(() => savedDefaultRaw.value.trim().length > 0)

/** 加载声明 module 形态的扩展，注册进工作台模块库（id 用 `ext:<扩展id>` 前缀与内置模块区分）。
 *  manifest.moduleVariants 声明的形态直接进模块库（形态芯片 / 尺寸钳制 / 适配徽标全生效）；
 *  未声明则注册单个默认形态（沿用历史 min 2×2 / ideal 4×3）。 */
export async function loadExtensionModules() {
  if (!isTauri()) return
  try {
    const exts = await tauriApi.listExtensions()
    registerExtensionModules(
      exts
        .filter((e) => !e.invalid && e.surfaces.includes('module'))
        .map((e) => {
          const variants: DashVariantDef[] =
            e.module_variants.length > 0
              ? e.module_variants.map((mv) => {
                  // manifest 作者输入不设防：钳制到合法网格范围（min ≥ 1、ideal ≥ min、宽 ≤ 12 列），
                  // 防止 idealW>12 产生越界网格、0 尺寸破坏落位
                  const minW = Math.max(1, Math.min(Math.round(mv.minW) || 1, DASH_COLS))
                  const minH = Math.max(1, Math.round(mv.minH) || 1)
                  const idealW = Math.max(minW, Math.min(Math.round(mv.idealW) || minW, DASH_COLS))
                  const idealH = Math.max(minH, Math.round(mv.idealH) || minH)
                  return v(mv.id, mv.name, minW, minH, idealW, idealH, mv.name)
                })
              : [v('module', '扩展', 2, 2, 4, 3, e.name)]
          return {
            id: `ext:${e.id}`,
            title: e.name,
            defaultVariant: variants[0].id,
            variants,
            // 作者声明宿主表头默认显隐：不写（null）= 默认不显示表头；写 false = 默认显示
            defaultHideTitle: e.module_options?.default_hide_title !== false,
          }
        }),
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
    // 默认布局快照：config 非空才覆盖（浏览器预览 config 为空串，保留 localStorage 快照）
    const cfgDefault = store.state.config.dashboard_default_layout
    if (cfgDefault) savedDefaultRaw.value = cfgDefault
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

function addModule(id: string, x: number, y: number, variant?: string): boolean {
  if (placements.value.some((p) => p.id === id)) return false
  const vd = dashVariantDef(id, variant) ?? dashVariantDef(id)
  if (!vd) return false
  // 目标位置被占时自动向下找最近的空位，拖入的模块总能落进布局（按所选形态推荐尺寸落位）
  const spot = findFreeSpot(placements.value, vd.idealW, vd.idealH, x, y)
  placements.value.push({
    id,
    x: spot.x,
    y: spot.y,
    w: vd.idealW,
    h: vd.idealH,
    variant: vd.id,
  })
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

/** 调整尺寸：钳制到形态最小尺寸，宽不超过 12 列右边界；碰撞时拒绝 */
function resizeModule(id: string, w: number, h: number): boolean {
  const p = placements.value.find((q) => q.id === id)
  if (!p) return false
  const vd = dashVariantDef(id, p.variant)
  const minW = vd?.minW ?? MIN_SIZE
  const minH = vd?.minH ?? MIN_SIZE
  const nw = Math.min(Math.max(clampSize(w), minW), DASH_COLS)
  // 右边界：先定宽再向左收缩 x（而非用 DASH_COLS - p.x 压宽度），保证 nw 始终 ≥ minW
  const nx = nw > DASH_COLS - p.x ? DASH_COLS - nw : p.x
  const nh = Math.max(clampSize(h), minH)
  const rect: DashPlacement = { ...p, x: nx, w: nw, h: nh }
  if (overlaps(rect, id)) return false
  p.x = nx
  p.w = nw
  p.h = nh
  persistIfIdle()
  return true
}

/** 切换形态：格子小于新形态最小尺寸时自动补到最小并就近让位 */
function setModuleVariant(id: string, variant: string): boolean {
  const p = placements.value.find((q) => q.id === id)
  const vd = dashVariantDef(id, variant)
  if (!p || !vd) return false
  p.variant = variant
  if (p.w < vd.minW || p.h < vd.minH) {
    const nw = Math.min(Math.max(p.w, vd.minW), DASH_COLS)
    // 右边界：先定宽再向左收缩 x，保证 nw 始终 ≥ 新形态 minW
    if (nw > DASH_COLS - p.x) p.x = DASH_COLS - nw
    const nh = Math.max(p.h, vd.minH)
    const rect: DashPlacement = { ...p, w: nw, h: nh }
    if (overlaps(rect, id)) {
      const others = placements.value.filter((q) => q.id !== id)
      const spot = findFreeSpot(others, nw, nh, rect.x, rect.y)
      p.x = spot.x
      p.y = spot.y
    }
    p.w = nw
    p.h = nh
  }
  persistIfIdle()
  return true
}

/**
 * 设置卡片标题：title 传空 = 用模块内置标题；
 * hideTitle = 显式 true/false 记为用户表态（会落盘，可覆盖扩展作者的默认），传 null = 清回「跟随作者默认」
 */
function setModuleTitle(id: string, title?: string | null, hideTitle?: boolean | null): boolean {
  const p = placements.value.find((q) => q.id === id)
  if (!p) return false
  const t = typeof title === 'string' ? title.trim().slice(0, TITLE_MAX) : ''
  p.title = t || undefined
  if (hideTitle === null) p.hideTitle = undefined
  else if (typeof hideTitle === 'boolean') p.hideTitle = hideTitle
  persistIfIdle()
  return true
}

function applyPreset() {
  placements.value = defaultPlacements()
  persistIfIdle()
}

/** 把当前布局存为默认布局快照（编辑器内 = 草稿现状，独立于草稿的确认/取消；空布局不允许存，避免恢复出空工作台） */
function saveDefaultLayout(): boolean {
  if (!placements.value.length) return false
  const data = serializePlacements()
  savedDefaultRaw.value = data
  if (isTauri()) {
    void store.setDashboardDefaultLayout(data)
  } else {
    try {
      localStorage.setItem(STORAGE_KEY_DEFAULT, data)
    } catch {
      // 存储失败静默，不影响交互
    }
  }
  return true
}

/** 恢复为保存的默认布局：编辑器内回到草稿（确认后生效），编辑器外立即生效并落盘。无快照/快照失效返回 false */
function restoreDefaultLayout(): boolean {
  const parsed = parsePlacements(savedDefaultRaw.value)
  if (!parsed) return false
  placements.value = parsed
  persistIfIdle()
  return true
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
    setModuleVariant,
    setModuleTitle,
    applyPreset,
    clear,
    saveDefaultLayout,
    restoreDefaultLayout,
    hasSavedDefault,
    beginEdit,
    commitEdit,
    cancelEdit,
  }
}
