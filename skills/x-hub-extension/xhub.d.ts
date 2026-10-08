/**
 * x-hub 桥 API（window.xhub）全局类型声明。
 *
 * 这是扩展与宿主通信的唯一契约，对齐 `docs/extension-api.md`。
 * 宿主在加载扩展入口 HTML 时自动注入 `window.xhub`，扩展脚本直接调用，
 * 无需 import 任何包。
 *
 * 状态标注约定：
 *   - `@done`    已实现，可直接使用
 *   - `@planned` 契约已定义，宿主尚未实现（调用会报错，请勿依赖）
 */

// ---------------------------------------------------------------------------
// 错误类型
// ---------------------------------------------------------------------------

/** 桥 API 统一的错误对象 */
interface XHubError extends Error {
  code:
    | 'PERMISSION_DENIED'
    | 'NOT_FOUND'
    | 'INVALID_ARGUMENT'
    | 'IO_ERROR'
    | 'NETWORK_ERROR'
    | 'INTERNAL'
  message: string
}

// ---------------------------------------------------------------------------
// 基础数据模型（与宿主 src-tauri/src/models.rs 对齐，字段为 snake_case）
// ---------------------------------------------------------------------------

/** 笔记 */
interface XHubNote {
  id: number
  title: string
  content: string
  created_at: string
  updated_at: string
}

/** 待办（写接口带乐观锁：update/toggle/delete/schedule 等可选传 expectedVersion） */
interface XHubTodo {
  id: number
  title: string
  done: boolean
  /** 0 低 / 1 中 / 2 高 */
  priority: number
  created_at: string
  updated_at: string
  completed_at: string | null
  /** 到期时间（毫秒时间戳） */
  due_at: number | null
  /** 提醒时间（毫秒时间戳） */
  remind_at: number | null
  remind_fired: boolean
  parent_id: number | null
  sort_order: number | null
  /** 每次写操作 +1；作为 expectedVersion 传回可实现并发冲突检测 */
  version: number
  /** 轻量 Markdown 正文（勾选一律用子待办） */
  description: string
  /** 置顶：脱离日期分组，固定排在列表最顶部「置顶」区 */
  pinned: boolean
  /** 周期规则总开关；'once' = 一次性 */
  repeat_mode: XHubRepeatMode
  /** custom：每 N 个 repeat_unit */
  repeat_every: number | null
  /** custom：day / week / month / year */
  repeat_unit: XHubRepeatUnit | null
  /** 位掩码 bit0=周一 … bit6=周日（weekly 多选、custom+week 用） */
  repeat_weekdays: number | null
  /** monthly：1..31，-1 = 月末 */
  repeat_month_day: number | null
  /** monthly：第几个（1..5，-1 = 最后一个） */
  repeat_month_nth: number | null
  /** 结束条件：never / until / count */
  repeat_end_mode: XHubRepeatEndMode | null
  /** until：截止日（毫秒时间戳，含当天） */
  repeat_end_at: number | null
  /** count：共 N 次 */
  repeat_count: number | null
  /** 累计完成次数（统计用，不逐次留历史） */
  repeat_done_count: number
  /** 上次完成本轮的时间 */
  repeat_last_done_at: string | null
}

/** 周期规则总开关 */
type XHubRepeatMode = 'once' | 'daily' | 'weekly' | 'monthly' | 'yearly' | 'weekdays' | 'custom'
/** custom 的重复单位 */
type XHubRepeatUnit = 'day' | 'week' | 'month' | 'year'
/** 周期结束条件 */
type XHubRepeatEndMode = 'never' | 'until' | 'count'

/** 写入用的周期规则（setRepeat 参数；非 'once' 时该待办必须有 due_at 作为基准时刻） */
interface XHubRepeatRuleInput {
  mode: XHubRepeatMode
  every?: number | null
  unit?: XHubRepeatUnit | null
  weekdays?: number | null
  monthDay?: number | null
  monthNth?: number | null
  endMode?: XHubRepeatEndMode | null
  endAt?: number | null
  count?: number | null
}

/** 待办标签定义（**与笔记标签 XHubTag 是两套独立定义**，互不同步） */
interface XHubTodoTag {
  id: number
  name: string
  /** '' = 用默认色；否则 #rrggbb */
  color: string
  sort_order: number
  created_at: string
}

/** 待办-标签关联对（构建筛选映射用） */
interface XHubTodoTagLink {
  todo_id: number
  tag_id: number
}

/** 周期待办在区间内的虚拟实例（日历渲染用；不落库） */
interface XHubTodoOccurrence {
  todo_id: number
  /** 实例时刻（毫秒时间戳） */
  at_ms: number
}

/** 速达资源 */
interface XHubResource {
  id: number
  kind: 'app' | 'web' | 'file'
  name: string
  target: string
  category: string | null
  icon: string | null
  args: string | null
  sort_order: number
  last_launched_at: string | null
  created_at: string
  updated_at: string
}

/** 桌面便签（主卡，槽位固定 1-2，upsert 带乐观锁） */
interface XHubSticky {
  id: number
  slot: number
  content: string
  version: number
  created_at: string
  updated_at: string
}

/** 脱离为主卡浮窗的便签（仅已存在时可写） */
interface XHubDetachedSticky {
  id: number
  slot: number
  content: string
  version: number
  x: number | null
  y: number | null
  always_on_top: boolean
  created_at: string
  updated_at: string
}

/** 快捷复制片段 */
interface XHubSnippet {
  id: number
  title: string
  content: string
  is_pinned: boolean
  copy_count: number
  last_copied_at: string
  created_at: string
  updated_at: string
}

/** 笔记标签 */
interface XHubTag {
  id: number
  name: string
  created_at: string
}

/** AI 用量汇总 */
interface XHubUsageSummary {
  today_input: number
  today_cache_input: number
  today_output: number
  today_cost: number
  today_count: number
  seven_day_input: number
  seven_day_cache_input: number
  seven_day_output: number
  seven_day_cost: number
  month_input: number
  month_cache_input: number
  month_output: number
  month_cost: number
  total_input: number
  total_cache_input: number
  total_output: number
  total_cost: number
  record_count: number
  last_sync_at: number | null
}

/** HTTP 响应封装 */
interface XHubHttpResult {
  status: number
  headers: Record<string, string>
  text(): Promise<string>
  json(): Promise<unknown>
}

// ---------------------------------------------------------------------------
// 各命名空间
// ---------------------------------------------------------------------------

/** @done 宿主提供的一个桥 API 能力（阶段1：能力注册表化后由 runtime.info 返回） */
interface XHubCapability {
  /** 命名空间，如 runtime / storage / data / service */
  namespace: string
  /** 方法名，如 info / get / notes.list */
  method: string
  /** 调用所需权限；null 表示无需权限 */
  permission: string | null
}

/** @done 扩展自身运行时信息 */
interface XHubRuntimeInfo {
  id: string
  name: string
  version: string
  runtime: 'web' | 'service'
  /** service 扩展：后端是否已就绪 */
  serviceReady: boolean
  /** service 扩展：完整代理前缀 URL（http://127.0.0.1:<port>/svc/<extId>）；web 扩展为 null */
  proxyPrefix: string | null
  /** @done 宿主当前提供的全部桥 API 能力清单（用于能力探测与优雅降级） */
  capabilities: XHubCapability[]
}

interface XHubRuntime {
  /** @done */
  info(): Promise<XHubRuntimeInfo>
  /**
   * @done 调用另一扩展暴露的方法（目标扩展需 manifest 声明 `expose` 并在内调用 `xhub.expose(method, fn)`）。
   * 目标扩展未打开或未暴露该方法会 reject。
   */
  callExtension(targetId: string, method: string, payload?: unknown): Promise<unknown>
  /**
   * @done 请求宿主把本扩展切到指定形态打开（缺省 'view'），如 module 卡片上「点开看全量」。
   * 无需权限；目标形态应在 manifest.surfaces / openIn 中声明，否则宿主可能无入口可打开。
   */
  open(surface?: 'view' | 'window' | 'drawer' | 'module'): Promise<void>
}

/**
 * 宿主数据读写（@done 全部已实装：读需 data:read、写需 data:write 权限）
 * 注意两点语义：
 * 1. update 类均为**全量覆盖**——可省字段缺省会被写入空值（如 notes.update 不传 content 清空正文），
 *    改前先 get 合并再提交；
 * 2. todos / stickies / detachedStickies 的写方法支持乐观锁：传 expectedVersion（=上次读到的
 *    version），与他人（含宿主 UI）并发修改冲突时报 VERSION_CONFLICT，缺省不校验。
 */
interface XHubData {
  notes: {
    /** 需 data:read */
    list(): Promise<XHubNote[]>
    /** 需 data:read；不存在报 NOT_FOUND */
    get(id: number): Promise<XHubNote>
    /** 需 data:write；只建标题，正文用 update 补 */
    create(opts: { title: string }): Promise<XHubNote>
    /** 需 data:write；全量覆盖：title 必填非空，content 缺省写空串 */
    update(opts: { id: number; title: string; content?: string }): Promise<XHubNote>
    /** 需 data:write */
    delete(opts: { id: number }): Promise<void>
  }
  todos: {
    /** 需 data:read */
    list(): Promise<XHubTodo[]>
    /** 需 data:read */
    get(id: number): Promise<XHubTodo>
    /** 需 data:write；createdAt 可显式指定（同步场景） */
    create(opts: { title: string; parentId?: number; createdAt?: string }): Promise<XHubTodo>
    /** 需 data:write；全量覆盖：priority 缺省 0 */
    update(opts: {
      id: number
      title: string
      priority?: number
      expectedVersion?: number
    }): Promise<XHubTodo>
    /** 需 data:write；完成态翻转 */
    toggle(opts: { id: number; expectedVersion?: number }): Promise<XHubTodo>
    /** 需 data:write */
    delete(opts: { id: number; expectedVersion?: number }): Promise<void>
    /** 需 data:write；dueAt/remindAt 毫秒时间戳，传 null 清除排期 */
    schedule(opts: {
      id: number
      dueAt?: number | null
      remindAt?: number | null
      expectedVersion?: number
    }): Promise<XHubTodo>
    /** 需 data:write；轻量 Markdown 正文（全量覆盖）。最长 200 字，超限报 INVALID_ARGUMENT */
    setDescription(opts: { id: number; description: string; expectedVersion?: number }): Promise<XHubTodo>
    /** 需 data:write；置顶开关（置顶条目脱离日期分组，固定在最顶部「置顶」区） */
    setPinned(opts: { id: number; pinned: boolean; expectedVersion?: number }): Promise<XHubTodo>
    /** 需 data:write；整组写入周期规则（mode:'once' 即取消周期）。非法取值 fail-fast（INVALID_ARGUMENT） */
    setRepeat(opts: { id: number; repeat: XHubRepeatRuleInput; expectedVersion?: number }): Promise<XHubTodo>
    /** 需 data:write；全量替换该待办的标签 */
    setTags(opts: { id: number; tagIds: number[] }): Promise<void>
    /** 需 data:write；周期待办「完成本轮」：due_at 滚到下一个未来时刻、计数 +1、子待办复位（不置 done） */
    completeRecurring(opts: { id: number; expectedVersion?: number }): Promise<XHubTodo>
    /** 需 data:write；撤销「完成本轮」：计数 −1、due_at 滚回上一轮 */
    undoRecurring(opts: { id: number; expectedVersion?: number }): Promise<XHubTodo>
    /**
     * 需 data:read；展开区间内的周期虚拟实例（规则只实现于宿主 Rust 侧，扩展不要自己算）。
     * **区间上限 366 天**：超了报 RECURRENCE_RANGE_TOO_LARGE，请分段查询（宿主日历本身只查 42 天）。
     */
    expandOccurrences(opts: { fromMs: number; toMs: number }): Promise<XHubTodoOccurrence[]>
  }
  todoTags: {
    /** 需 data:read */
    list(): Promise<XHubTodoTag[]>
    /** 需 data:read；全部待办-标签关联 */
    links(): Promise<XHubTodoTagLink[]>
    /** 需 data:write；同名已存在则直接返回既有标签（不覆盖颜色） */
    create(opts: { name: string; color?: string }): Promise<XHubTodoTag>
    /** 需 data:write；改名/改色（重名会报错） */
    update(opts: { id: number; name: string; color?: string }): Promise<XHubTodoTag>
    /** 需 data:write；标签与所有关联一并删除 */
    delete(opts: { id: number }): Promise<void>
  }
  stickies: {
    /** 需 data:read */
    list(): Promise<XHubSticky[]>
    /** 需 data:write；slot 1-2，upsert：无记录自动建档 */
    save(opts: { slot: number; content?: string; expectedVersion?: number }): Promise<XHubSticky>
  }
  detachedStickies: {
    /** 需 data:read */
    list(): Promise<XHubDetachedSticky[]>
    /** 需 data:write；仅更新已脱离的浮窗便签，无记录报 NOT_FOUND（不会凭空造浮窗） */
    save(opts: { slot: number; content?: string; expectedVersion?: number }): Promise<void>
  }
  resources: {
    /** 需 data:read */
    list(): Promise<XHubResource[]>
    /** 需 data:read */
    get(id: number): Promise<XHubResource>
    /** 需 data:write */
    create(opts: {
      kind: 'app' | 'web' | 'file'
      name: string
      target: string
      category?: string | null
      icon?: string | null
      args?: string | null
    }): Promise<XHubResource>
    /** 需 data:write；全量覆盖，字段同 create */
    update(opts: { id: number } & Parameters<XHubData['resources']['create']>[0]): Promise<XHubResource>
    /** 需 data:write */
    delete(opts: { id: number }): Promise<void>
  }
  snippets: {
    /** 需 data:read */
    list(): Promise<XHubSnippet[]>
    /** 需 data:read */
    get(id: number): Promise<XHubSnippet>
    /** 需 data:write */
    create(opts: { title: string; content?: string }): Promise<XHubSnippet>
    /** 需 data:write；全量覆盖：content 缺省写空串 */
    update(opts: { id: number; title: string; content?: string }): Promise<XHubSnippet>
    /** 需 data:write */
    delete(opts: { id: number }): Promise<void>
    /** 需 data:write；置顶态翻转 */
    togglePin(opts: { id: number }): Promise<XHubSnippet>
  }
  tags: {
    /** 需 data:read */
    list(): Promise<XHubTag[]>
    /** 需 data:read；某笔记的标签 */
    ofNote(noteId: number): Promise<XHubTag[]>
    /** 需 data:write */
    create(opts: { name: string }): Promise<XHubTag>
    /** 需 data:write */
    delete(opts: { id: number }): Promise<void>
    /** 需 data:write；全量替换语义：先清空再写入，tagIds 必须整数数组 */
    setNoteTags(opts: { noteId: number; tagIds: number[] }): Promise<void>
  }
  usage: {
    /** @planned 宿主未实现（桥脚本未暴露 usage 命名空间） */
    summary(): Promise<XHubUsageSummary>
  }
}

/** @done 扩展本地键值存储（隔离、随卸载清除，无需权限） */
interface XHubStorage {
  /** 无则返回 null */
  get(key: string): Promise<unknown>
  /** value 需可 JSON 序列化 */
  set(key: string, value: unknown): Promise<void>
  remove(key: string): Promise<void>
  clear(): Promise<void>
}

/** @done 扩展配置（分层覆盖：manifest.config 默认 ∪ 用户覆盖，用户覆盖优先，无需权限） */
interface XHubConfig {
  /** 返回合并后的完整配置对象 */
  all(): Promise<Record<string, unknown>>
  /** 单键读取：用户覆盖值 ?? manifest 默认值 ?? null */
  get(key: string): Promise<unknown>
  /** 写用户覆盖层（升级扩展不会冲掉用户覆盖） */
  set(key: string, value: unknown): Promise<void>
  /** 删除用户覆盖，回退到 manifest 默认值 */
  remove(key: string): Promise<void>
}

/** @done 跨扩展共享键值存储（需 manifest 声明 `shared-storage` 权限） */
interface XHubSharedStorage {
  get(key: string): Promise<unknown>
  set(key: string, value: unknown): Promise<void>
  remove(key: string): Promise<void>
}

/**
 * 文件保存（需 `fs` 权限）。
 *
 * ⚠️ 别和「受控读写」混为一谈：`save*` 这套是**另存为** —— 扩展把生成的内容交给用户，
 *    不读用户目录、也不能往任意路径写。单文件上限 64MB，重名自动加序号。
 */
interface XHubFs {
  /** @done 文本存到**系统下载目录**，返回落盘的文件名与完整路径 */
  saveText(opts: { name: string; content: string }): Promise<{ path: string; name: string }>
  /** @done 二进制（base64）存到系统下载目录 */
  saveFile(opts: { name: string; base64: string }): Promise<{ path: string; name: string }>
  /** @done 弹原生「保存到…」对话框由用户选路径；用户取消时返回 `{ canceled: true }` */
  saveAs(opts: { name: string; base64: string }): Promise<{ path: string; name: string } | { canceled: true }>

  /** @planned 读文件（受控读写，宿主尚未实现） */
  readText(path: string): Promise<string>
  /** @planned 写文件（受控读写，宿主尚未实现） */
  writeText(path: string, content: string): Promise<void>
  /** @planned 列目录（受控读写，宿主尚未实现） */
  readDir(path: string): Promise<{ name: string; isDir: boolean }[]>
  /** @planned 存在性检查（受控读写，宿主尚未实现） */
  exists(path: string): Promise<boolean>
}

/** @planned 剪贴板（需 clipboard 权限） */
interface XHubClipboard {
  readText(): Promise<string>
  writeText(text: string): Promise<void>
}

/** @planned 网络请求（需 network 权限） */
interface XHubNet {
  fetch(
    url: string,
    init?: { method?: string; headers?: Record<string, string>; body?: string },
  ): Promise<XHubHttpResult>
}

/** @done service 扩展调用自身受托管后端 */
interface XHubService {
  request(
    path: string,
    init?: { method?: string; headers?: Record<string, string>; body?: string },
  ): Promise<XHubHttpResult>
}

/** @done 宿主主题令牌（window.xhub.theme.get() 返回，同时自动注入为 --xhub-* CSS 变量） */
interface XHubThemeTokens {
  /** 强调色 */
  accent: string
  /** 品牌主色 */
  brand: string
  /** 品牌弱色（半透明背景） */
  brandSoft: string
  /**
   * 宿主**整页背景**（渐变，alpha=1 不透明）。
   * ⚠️ 别用它铺扩展的页面底：壁纸态下会把壁纸整块盖住，透底态又因文字翻白而变成"白底白字"。
   * 且它是 gradient —— `color: var(--xhub-bg-page)` 是无效声明，不能当颜色用。
   */
  bgPage: string
  /**
   * **扩展该铺的页面底**（推荐用它，而不是 `bgPage`）：
   * 无壁纸时 = 宿主整页背景（与其它 View 观感一致）；**有壁纸时 = `transparent`**（否则会盖住壁纸）。
   * 用法：`body { background: var(--xhub-page-bg, transparent) }` —— 这是跨形态通用的写法。
   */
  pageBg: string
  /** 卡片底色（亮色态 ~0.88 白；**透底态会变成 `rgba(255,255,255,.2)`**）——适合做按钮/输入区的"井"底 */
  bgCard: string
  /** 玻璃表面（渐变，**已含玻璃透明度乘数**）——**扩展的卡片/面板底优先用它**；页面本身保持 transparent */
  surface: string
  text1: string
  text2: string
  text3: string
  border: string
  red: string
  green: string
  yellow: string
  blue: string
  orange: string
  radiusLg: string
}

/** @done 宿主主题 */
interface XHubTheme {
  mode: 'light' | 'dark'
  preset: string | null
  accent: string
  /**
   * 壁纸状态。宿主还会把同一份状态转写成 `<html>` 上的属性（CSS 直接用，无需调 API）：
   * `data-xhub-wallpaper="1"`（有壁纸）、`data-xhub-wallpaper-clear="1"`（真实透底 ——
   * 此时 `--xhub-text-*` 整体翻白、壁纸蒙版翻深）、`data-xhub-immersive="1"`（沉浸模式）。
   */
  wallpaper: { on: boolean; clear: boolean; immersive: boolean }
  tokens: XHubThemeTokens
}

/** @done 主题能力（无需权限；扩展 CSS 直接引用 --xhub-* 变量即可，通常无需调用） */
interface XHubThemeApi {
  get(): Promise<XHubTheme>
}

/** 界面与通知 */
interface XHubUi {
  /** @planned 无需权限 */
  toast(message: string, options?: { type?: 'info' | 'success' | 'error' }): Promise<void>
  /**
   * @done 推送一条右下角通知（**需 manifest 声明 `notify` 权限**）。
   * 复用宿主的自绘通知窗（跨 Win10/11 一致），`title` 截 80、`body` 截 300 字符。
   */
  notify(title: string, body: string): Promise<void>
}

/** @planned 系统能力（需 system 权限） */
interface XHubSystem {
  openUrl(url: string): Promise<void>
  openPath(path: string): Promise<void>
  openApp(path: string, args?: string): Promise<void>
}

/** 订阅宿主事件（已实现 `theme-changed` + `xhub:variant-changed` + 扩展间事件总线 emit，其余 @planned） */
interface XHubEvents {
  /**
   * 订阅事件，返回取消订阅函数。
   * - `theme-changed`：payload 为 XHubTheme
   * - `xhub:variant-changed`：payload 为 string（工作台模块形态 id，声明 moduleVariants 时由宿主广播）
   */
  on(event: string, handler: (payload: unknown) => void): () => void
  off(event: string, handler: (payload: unknown) => void): void
  /**
   * @done 广播自定义事件给其它扩展（需 manifest 声明 `events` 权限）。
   * 其它扩展用 `events.on(event, fn)` 订阅；payload 需可结构化克隆。
   */
  emit(event: string, payload?: unknown): Promise<void>
}

// ---------------------------------------------------------------------------
// window.xhub 总入口
// ---------------------------------------------------------------------------

interface XHub {
  runtime: XHubRuntime
  data: XHubData
  storage: XHubStorage
  config: XHubConfig
  sharedStorage: XHubSharedStorage
  fs: XHubFs
  clipboard: XHubClipboard
  net: XHubNet
  service: XHubService
  theme: XHubThemeApi
  ui: XHubUi
  system: XHubSystem
  events: XHubEvents
  /**
   * @done 打开外链（**需 manifest 声明 `open-url` 权限**）。只放行 `http(s)://`。
   * 默认在**应用内置浏览器窗口**打开；用户可在扩展详情弹窗把单个扩展改为
   * 「浏览器」（系统默认浏览器）。扩展侧代码无需关心区分。
   *
   * 为什么必须用它而不是 `target="_blank"`：宿主用 Tauri/wry 承载扩展 iframe，wry 在宿主
   * 未注册新窗口处理器时对 WebView2 的 NewWindowRequested 直接 SetHandled(true) 拒绝，
   * 于是 `target="_blank"` 与 `window.open()` 在宿主里**静默失效**（点了没反应）。
   */
  openExternal(url: string): Promise<void>
  /**
   * @done 请求宿主打开本扩展的设置/授权弹窗（**无需权限**，只能打开自己的设置）。
   * 典型用途：service 后端未授权（`service.request` 报 PERMISSION_DENIED）时，
   * 页面展示提示 + 「去授权」按钮，点击调用本方法直接落到权限管理界面。
   */
  openPermissions(): Promise<void>
  /**
   * @done 暴露一个方法供其它扩展调用（配合 manifest `expose` 声明）。
   * handler 返回 Promise 或值；返回值需可结构化克隆。
   */
  expose(method: string, handler: (payload?: unknown) => unknown): void
}

declare global {
  interface Window {
    /** 宿主注入的桥 API，扩展脚本加载后即可访问 */
    xhub: XHub
  }
}

export {}
