/**
 * 标题栏小部件：扩展经桥 API `titlebar.set/clear`（需 `titlebar` 权限）把一条
 * media（媒体控制档）/ status（通用信息档）小部件推上主窗标题栏正中。
 *
 * 职责链：Rust 侧只校验载荷并广播 `titlebar-widget-changed`；**常驻闸在本文件**——
 * extId 必须在 keepAliveRunning 运行态清单里才允许占位。常驻实例是唯一能持续
 * 维护小部件的形态：普通实例切页即销毁，放行只会留下再也无人更新的死部件。
 * 「结束本次常驻」只销毁实例不动配置，所以闸必须读运行态而非配置。
 *
 * 单占位：标题栏正中只有一条位，后 set 者顶掉前者；clear 只清自己的占位。
 * 按钮点击经定向事件 `titlebar.action`（payload `{ id }`）回传给占用扩展，
 * 扩展用 `xhub.events.on('titlebar.action', fn)` 接。media 档固定三键的
 * 约定 id：`previous` / `play`|`pause`（按当前 playing 态二选一）/ `next`。
 */
import { readonly, ref } from 'vue'
import { isTauri } from '../api/tauri'
import { useStore } from '../stores/workbench'
import { sendEventToExtension } from './themeTokens'

export interface TitlebarAction {
  id: string
  label: string
}

/** 段式排布的公共布局字段 */
interface TitlebarSegmentBase {
  /** 固定宽度（px，Rust 侧钳 16..=240）；与 grow 二选一，缺省按段类型取默认宽度 */
  width?: number
  /** true = 吃掉剩余宽度（弹性段）；同一行通常只给一个段 */
  grow?: boolean
}

/** 文本段：`primary` 主文本（12px 粗）/ `muted` 次要文本（11px 淡） */
export interface TitlebarTextSegment extends TitlebarSegmentBase {
  type: 'text'
  text: string
  tone?: 'primary' | 'muted'
}

/** 字符图标段（emoji / 字形，≤8 字符） */
export interface TitlebarIconSegment extends TitlebarSegmentBase {
  type: 'icon'
  glyph: string
}

/** 进度线段：value 0..=1 */
export interface TitlebarProgressSegment extends TitlebarSegmentBase {
  type: 'progress'
  value: number
}

/** 高度条组段：values 每个元素是一根竖条的高度（0..=1，最多 32 根） */
export interface TitlebarBarsSegment extends TitlebarSegmentBase {
  type: 'bars'
  values: number[]
}

/** 按钮组段：点击经 titlebar.action 回传 id */
export interface TitlebarActionsSegment extends TitlebarSegmentBase {
  type: 'actions'
  items: TitlebarAction[]
}

/** 纵向叠放容器段：children 依次上下排列（最多嵌套 2 层） */
export interface TitlebarStackSegment extends TitlebarSegmentBase {
  type: 'stack'
  children: TitlebarSegment[]
}

/**
 * status 档内容段：宿主只提供这几种几何原语，按数组顺序横向排布。
 * 扩展要什么就拼什么——没有进度的扩展不传 progress 段，需要「进度 + 上方竖条」
 * 这类组合就用 stack 包一层。宿主不理解业务语义，也不为具体场景加字段。
 */
export type TitlebarSegment =
  | TitlebarTextSegment
  | TitlebarIconSegment
  | TitlebarProgressSegment
  | TitlebarBarsSegment
  | TitlebarActionsSegment
  | TitlebarStackSegment

export interface TitlebarWidget {
  kind: 'media' | 'status'
  title: string
  subtitle?: string
  /** status 档 emoji 图标（≤8 字符） */
  icon?: string
  /** media 档封面：http(s) URL 或 ≤64KB dataURL */
  cover?: string
  playing?: boolean
  positionMs?: number
  durationMs?: number
  /** status 档细进度（0..=1；扁平模式下贴部件底边） */
  progress?: number
  /**
   * status 档段式排布（通用原语）：存在时忽略 title/subtitle/icon/progress/actions
   * 的扁平渲染，完全按段顺序拼条。`title` 仍必填，作为无障碍名称与回退文本
   */
  segments?: TitlebarSegment[]
  actions?: TitlebarAction[]
}

interface TitlebarEventPayload {
  extId: string
  widget: TitlebarWidget | null
}

/** 主窗单占位状态（模块级：index.vue 监听/清理与 TitleBar.vue 渲染共享同一份） */
const current = ref<{ extId: string; widget: TitlebarWidget } | null>(null)

let started = false
let listenInFlight = false

export function useTitlebarWidget() {
  const store = useStore()

  function apply(payload: TitlebarEventPayload) {
    const { extId, widget } = payload
    if (widget) {
      // 常驻闸：不在跑的实例连占位资格都没有（载荷合法性 Rust 已验）
      if (!store.state.keepAliveRunning.includes(extId)) return
      current.value = { extId, widget }
    } else if (current.value?.extId === extId) {
      current.value = null
    }
  }

  /** 实例销毁（结束本次常驻/剪枝/重载）时撤下小部件，防死 iframe 的僵尸占位 */
  function clearOf(extId: string) {
    if (current.value?.extId === extId) current.value = null
  }

  function sendAction(actionId: string) {
    const cur = current.value
    if (cur) sendEventToExtension(cur.extId, 'titlebar.action', { id: actionId })
  }

  /** 主窗挂载后调用一次：开始监听 Rust 转发的小部件事件（仅主窗渲染小部件）。
   *  注册成功才置 started；失败不置位，下次调用可重试（in-flight 守卫防并发双注册） */
  function start() {
    if (started || listenInFlight || !isTauri()) return
    listenInFlight = true
    void import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<TitlebarEventPayload>('titlebar-widget-changed', (e) => apply(e.payload)),
      )
      .then(() => {
        started = true
      })
      .catch(() => {})
      .finally(() => {
        listenInFlight = false
      })
  }

  return { widget: readonly(current), start, clearOf, sendAction }
}
