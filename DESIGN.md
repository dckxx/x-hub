# x-hub Design System

> 版本对齐：v0.1.18。本文档为当前实现的唯一设计基线，UI 改动以本文件 + `src/style.css` 为准。

## 1. Atmosphere & Identity

x-hub 是一个安静、可靠的本地桌面工作台：用户打开它是为了立刻继续工作，而不是浏览一个复杂的仪表盘。视觉签名是「**玻璃质感卡片 + 极简侧边轨道 + 轻微纸张层次**」：侧栏导航始终固定且可收起，主区用细微信号变化区分各模块，不使用装饰性渐变或高饱和色抢夺注意力。标题栏与背景透明，让内容沉浸感更强。

## 2. Color

### Palette（亮色 `:root` / 暗色 `[data-theme="dark"]`）

| Role | Token | Light | Dark | Usage |
|---|---|---|---|---|
| Page | `--bg-page` | `#eceff6` | `#12131b` | 工作区底色 |
| Sidebar | `--bg-sidebar` | `rgba(255,255,255,.62)` | `rgba(255,255,255,.08)` | 固定导航栏 |
| Surface | `--bg-card` | `rgba(255,255,255,.74)` | `rgba(255,255,255,.12)` | 主面板玻璃卡 |
| Surface solid | `--bg-card-solid` | `rgba(255,255,255,.88)` | `rgba(255,255,255,.20)` | 弹窗、浮层 |
| Surface soft | `--bg-card-soft` | `rgba(255,255,255,.55)` | `rgba(255,255,255,.10)` | 列表 hover |
| Input field | `--input-bg` | `rgba(255,255,255,.86)` | `#1d1e29` | 输入框、下拉框背景（暗色下不透明，避免 color-scheme 退化透出原生白色层） |
| Frost surface | `--frost-surface` | 靛蓝/粉/蓝三色径向渐变 + 半透明白基底 | 同构暗色版 | 常驻卡片伪毛玻璃底色（静态烘焙，见 §7） |
| Frost edge | `--frost-edge` | `inset 0 1px 0 rgba(255,255,255,.55)` | `inset 0 1px 0 rgba(255,255,255,.10)` | 卡片顶部玻璃反光高光 |
| Text primary | `--text-1` | `#26231d` | `#f2efe8` | 标题、正文 |
| Text secondary | `--text-2` | `#57524a` | `#ccc8bf` | 辅助信息 |
| Text muted | `--text-3` | `#8d877d` | `#9b968c` | 元数据、图标 |
| Border subtle | `--border-soft` | `rgba(255,255,255,.55)` | `rgba(255,255,255,.16)` | 分隔、输入框 |
| Border strong | `--border-strong` | `rgba(40,35,60,.22)` | `rgba(255,255,255,.28)` | 焦点、强调描边 |
| Accent | `--brand-500` | `var(--accent)`（默认 `#5b5bf5`） | `var(--accent)`（默认 `#8b8bff`） | 当前项、主操作、焦点 |
| Accent soft | `--brand-50` | `color-mix(in srgb, var(--accent) 12%, transparent)` | `color-mix(in srgb, var(--accent) 16%, #16161f)` | 激活背景、选中状态 |
| Accent glow | `--brand-glow` | `color-mix(in srgb, var(--accent) 18%, transparent)` | `color-mix(in srgb, var(--accent) 28%, transparent)` | 焦点环、光晕 |
| Scrim | `--scrim` | `rgba(38,35,29,.48)` | 暗色同值 | 弹窗遮罩（暗色下避免过亮） |
| Success | `--c-green-ink` | `#15803d` | 暗色同值 | 正向反馈 |

> **三轴主题（v0.1.15）**：品牌强调色不再写死，由 inline `--accent` CSS 变量注入（`useTheme` composable 写入 `:root`），`--brand-500` = `var(--accent)`，`--brand-600/50/glow` 全部 `color-mix` 派生。主题三轴独立配置：**模式**（light/dark/system → `data-theme`）、**预设**（10 单色 `data-preset` + 10 渐变，渐变仅覆盖 `--app-bg` 背景）、**强调色**（8 预设 + 自定义 hex → inline `--accent`）。配置字段 `theme_mode`/`theme_preset`/`accent_color`（旧 `theme` 字段经 serde alias 自动迁移）。

### 强调色（8 色 + ink/soft 变体）

| Token | Hex | 用途 |
|---|---|---|
| `--c-yellow / -ink / -soft` | `#facc15 / #806600 / #fde68a` | 常用、便签 |
| `--c-red / -ink / -soft` | `#ef4444 / #b91c1c / #fecaca` | 紧急、删除 |
| `--c-blue / -ink / -soft` | `#3b82f6 / #1d4ed8 / #bfdbfe` | 网页、链接 |
| `--c-green / -ink / -soft` | `#22c55e / #15803d / #bbf7d0` | 完成、应用 |
| `--c-pink / -ink / -soft` | `#ec4899 / #be185d / #fbcfe8` | 标记、素材 |
| `--c-orange / -ink / -soft` | `#f59e0b / #b45309 / #fde68a` | 提醒 |
| `--c-purple / -ink / -soft` | `#a78bfa / #6d28d9 / #ddd6fe` | 代码、文档 |
| `--c-gray / -ink / -soft` | `#9ca3af / #4b5563 / #e5e7eb` | 默认、周报 |

> 资源图标按名称 hash 取色（`useResourceIcon` composable），使用 soft 底 + 主色图标 + ink 文字的组合，保证明度统一。

### Rules

- 采用 restrained palette；靛紫只表达当前选择、可执行主操作和键盘焦点。
- 卡片使用「半透明玻璃底色 + 静态烘焙渐变 + 阴影分层」实现磨砂观感（见 §7），不使用纯色装饰条或实时渐变动画。
- 状态不能只依靠颜色：当前导航同时使用背景、字重和图标位置变化。

## 3. Typography

### Scale

| Level | Size | Weight | Line Height | Usage |
|---|---|---:|---:|---:|---|
| App title | 15px | 700 | 1.3 | 顶栏品牌 |
| Page title | 20px | 700 | 1.25 | 视图标题 |
| Section title | 16px | 650 | 1.35 | 卡片标题 |
| Body | 13px | 400 | 1.5 | 正文、列表 |
| Body strong | 13px | 600 | 1.4 | 条目标题、按钮 |
| Caption | 12px | 500 | 1.4 | 元数据、提示 |
| Micro | 11px | 500 | 1.35 | 标签、快捷键 |

### Font Scale（字体缩放）

字号不再散落硬编码 px，而是映射到相对单位，配合两层可调系数（设置「外观」区「字体大小」，范围 0.85–1.30）：

- **全局字体大小**：根字号 `html { font-size: calc(16px * var(--fs-global, 1)) }`；全局 UI 字号统一用 `rem`（1rem = 16px × `--fs-global`）。
- **单模块字体大小**：便签（`--fs-sticky`）、速记（`--fs-notes`）、提示词（`--fs-prompt`）、待办（`--fs-todo`）、用量（`--fs-usage`）5 个内容模块，模块根 `font-size: calc(1rem * var(--fs-xxx, 1))`，模块内部字号用 `em` 相对模块根；嵌套在已缩放字号内的元素（如 Markdown 标题、标签删除按钮）用 `calc(Nrem * var(--fs-xxx, 1))` 精确表达。
- `--fs-*` 由 `useTheme` 依据 config（`font_scale` / `font_sticky` 等）注入 inline；rem 相对根、em 相对模块根，两层缩放相乘。

### Font Stack

- Primary: `Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif`
- Mono: `ui-monospace, SFMono-Regular, Consolas, monospace`

正文不低于 12px；中文标题和段落使用 `text-wrap: pretty`，避免单字孤行。英文/数字 `letter-spacing: -0.01em`。

## 4. Spacing & Layout

基础单位为 4px。令牌：`--space-1..6`（4/8/12/16/20/24px）。

### App shell

```
┌────────────────────────────────────────────────────────┐
│  TitleBar（透明，48px）                                 │
├───────┬────────────────────────────────────────────────┤
│ 侧栏   │  主工作区                                      │
│ 220px  │  （展开 220px / 收起 56px）                    │
│ 收起态 │                                                │
└───────┴────────────────────────────────────────────────┘
```

- 窗口默认 1400×900，最小 800×600。
- `app-body` 为两栏 Grid：`220px minmax(0,1fr)`；收起态 `56px minmax(0,1fr)`，180ms 过渡。
- 标题栏 48px，背景透明（无底部分隔线）。
- **侧栏默认收起**（56px 图标态，hover 出名称气泡）；展开/收起按钮仅在设置开启 `sidebar_toggle` 后出现（默认关闭）；720px 以下强制恢复文字导航。

### Dashboard（工作台）— 三列 Bento 网格

```
┌───────────────┬───────────────────┬──────────────┐
│ 时钟          │ 中上区块           │  待办清单     │
│ 系统资源监视器 │ (默认倒计时，可切换 │  (grid-row    │
│ 便签 ×2       │  Token/概览卡)     │   1/3)       │
│               │ 提示词百宝箱       │              │
└───────────────┴───────────────────┴──────────────┘
│              最近使用通栏（跨三列）                 │
└──────────────────────────────────────────────────┘
```

- `grid-template-columns: minmax(0,1.2fr) minmax(0,1.8fr) minmax(0,1fr)`，行 `auto minmax(0,1fr) auto`，gap 16px。
- 左列 flex 栈：时钟 → 系统监视 → 便签（两张 1fr 并排）。
- 中列上半为**可切换区块** `dashboard_mid_content`（设置「工作台」区切换）：`countdown` 倒计时（默认）/ `token` Token 统计 / `notes` 速记概览 / `todo` 待办概览 / `resources` 速达数量；中列下半为提示词百宝箱。
- 待办占右列整列（grid-row 1/3），最近使用通栏占底部整行。
- 首页铺满视口无滚动，卡片内容区内滚动；960px 以下折两列，720px 以下单列堆叠。

### 独立视图

导航项（工作台 / 速记 / 速达 / 用量）各对应一个独立视图，非弹窗：

- **速记**：笔记列表 + 标签筛选。
- **速达**：资源管理（全部/常用/应用/网页/文件 + 文件二级分类 tabs）。
- **用量**：双栏（左趋势/提供商排行 + 右明细分页）。

## 5. Components

### App shell

- **Structure**: `header.title-bar`（透明）+ `aside.sidebar` + `main.workspace`。
- **States**: 正常、暗色主题、最小窗口、主区滚动。
- **Accessibility**: `nav` 使用 `aria-label`，当前项使用 `aria-current`，所有按钮有可见焦点。

### Sidebar navigation

- **Structure**: 主导航（工作台/速记/速达/用量）、底部设置、收起按钮（仅 `sidebar_toggle` 开启后显示）。
- **Variants**: active、hover、disabled/empty。
- **States**: default、hover、active、focus、empty。
- **收起态**: 侧栏默认收起，仅图标 + hover 右侧名称气泡（data-tip，300ms 延迟显示）。
- **Motion**: 150ms 背景与颜色变化，不做入场编舞。
- **主题切换**：不在侧栏/标题栏，位于设置「外观」区（模式/预设/强调色三轴）。

### Glass card（基础卡片）

- 常驻表面统一使用 `--frost-surface` 伪毛玻璃底色（烘焙渐变 + 半透明基底）+ `--frost-edge` 顶部高光 + `--shadow-card` + `--radius-lg`(12px)，内部控件 `--radius-md`(8px)。
- 所有工作台卡片同一高度语义；标题行（icon + 标题 + 右侧动作）统一 16px/650。
- 真 `backdrop-filter` 仅保留给弹窗、右键菜单、下拉、tooltip 等瞬态表面（一次性打开成本），常驻卡片不启用（详见 §7）。

### Todo card

- **Structure**: 标题行 + 分段（待办/已完成）、添加输入行、待办列表。
- **States**: default、hover、done（删除线 + 降透明度）、highlight（全局搜索直达后 3s 高亮）。
- **Interactions**: 勾选切换完成；**优先级圆点**（10px 纯色点：中性灰 `--todo-pri-default`（亮 `#c6cad4` / 暗 `#52525f`）/ 柔黄 `--c-yellow-soft` / 柔红 `--c-red-soft` = 普通/重要/紧急，点击循环，hover 放大）；**多行显示**（自动换行，最多 5 行截断，超过时 hover 悬浮全文）；双击行内编辑；删除按钮 hover 显现；删除可撤销。
- **Accessibility**: 分段使用 `role="tablist"`，优先级圆点可键盘触发，勾选/删除带 `aria-label`。

### 其他卡片组件

| 组件 | 说明 |
|---|---|
| ClockCard | 时间 HH:mm + 日期星期 + 最近进行中倒计时的环形进度（SVG stroke-dashoffset），30s 轮询 |
| SysMonitorCard | CPU/内存进度条（品牌渐变，≥85% 红色警示渐变）+ 2s 轮询（sysinfo 后端）；进度条用 `transform: scaleX` 动画（走合成器，不触发布局重排） |
| StickyCard | 便签 x2（slot 1/2，统一玻璃卡），600ms 防抖自动保存 |
| CountdownCard | 倒计时卡（v0.1.13）：时长/定时/每天/间隔四种新建 + 列表（每秒刷新剩余时间 + 圆形水位）+ 暂停/恢复/浮窗/删除；`once` 到点灰态「已结束」待删；工作台中上区块默认内容 |
| CountdownFloat | 倒计时圆形浮窗（v0.1.13）：透明置顶圆窗，水位随剩余比例下降 + 双层正弦波滚动（`cf-wave-a/b` 平移动画），悬停出暂停/关闭按钮；`once` 到点自动收窗 |
| DetachedStickyWindow | 便签脱离浮窗小窗（sticky-* label 专属渲染，App.vue 路由分发） |
| TokenStatsCard | 今日用量总量 + 非缓存/缓存/输出三指标 + 三段分割条 + 监听绿点，5min 自动刷新 + 手动刷新 + 「查看详情」跳转用量视图；仅在 `dashboard_mid_content = 'token'` 时显示 |
| NotesOverviewCard / TodoOverviewCard / ResourcesOverviewCard | 中上区块可选概览卡：速记统计 / 待办概览 / 速达数量，点击跳转对应视图 |
| PromptBoxCard | 提示词列表卡，点击复制 + 置顶标 + 管理入口 |
| RecentBar | 最近使用通栏（按 last_launched_at 排序，前 10） |
| Suda | 速达资源管理视图（筛选 tabs + 卡片网格 + 拖拽导入 + 扫描安装应用 + 右键菜单） |
| SudaFormDialog / SudaScanDialog | 新增/编辑资源弹窗（app/web/file + 文件选择）/ 扫描已安装应用批量导入弹窗 |
| AppSelect | 通用下拉选择器（无头封装，样式自绘） |
| UsageView | 用量详情视图（双栏：左趋势/提供商排行 + 右明细分页） |

### 弹窗 / 浮层

- 统一 `modal-card`：`--bg-card-solid` + 遮罩 `--scrim`（暗色下保证对比度）+ `--shadow-dock`。
- 弹窗进入 200ms 缩放渐入；支持焦点陷阱与键盘方向键导航。

## 6. Motion & Interaction

- Micro transition: 150ms ease-out，用于按钮和导航状态。
- Standard transition: 200ms ease-out，用于面板/弹窗进入。
- 只动画 `transform`、`opacity` 和颜色；不动画布局尺寸。
- 遵循 `prefers-reduced-motion`，减少动态时直接呈现最终状态。
- 按钮按下 `scale(0.96)`，卡片 hover 轻微上浮。

## 7. Depth & Surface

采用 mixed strategy：页面、侧栏和面板使用 tonal shift（玻璃半透明）；弹窗和拖拽目标保留轻量阴影/边框。主面板圆角 12px，内部控件圆角 8px，禁止 24px 以上的卡片圆角。背景保持稳定，层次来自明度与间距，而非玻璃装饰。

### 玻璃拟态与性能（v0.1.13 起）

为把 GPU 占用压到低位，毛玻璃效果按「常驻 / 瞬态」分两层实现：

- **常驻表面（卡片 `.card`、用量卡 `.uv-card` / `.uv-section`）= 伪毛玻璃**：页面背景是静态渐变（`body` 的 radial/linear），因此把"背景被模糊后的柔和晕染"直接烘焙成卡片自身的多层静态渐变（`--frost-surface`，靛蓝/粉/蓝三色低透明径向斑 + 半透明基底），加 `--frost-edge` 顶部内高光模拟玻璃反光。这些层只绘制一次、之后零计算。
- **瞬态表面（`.modal-card` / `.ctx-menu` / 下拉 / tooltip / 关闭确认遮罩）= 真 `backdrop-filter`**：弹窗、菜单、下拉这类"打开一下就消失"的层保留真实背景模糊，一次性打开成本，不影响常驻开销。
- **不动画布局属性**：进度条等周期性刷新一律用 `transform`（走合成器），禁用 `width/height` 这类触发布局重排的属性。
- 收益：磨砂观感保留（透出背景色 + 玻璃边缘反光），但常驻卡片的 GPU 重采样成本归零，整体占用从 ~26% 明显回落。

## 8. Reka UI 组件规范（v0.1.13 起）

> 详细文档见 `docs/reka-ui.md`。Reka UI 为无头组件库（不提供样式），外观一律用项目设计令牌自绘。当前仅用于复杂输入组件（`CountdownCard.vue`）：`DatePicker`（定时日期）、`TimeField`（定时/每天时:分）、`NumberField`（时长/间隔步进）。

### 8.1 Portal 弹层：容器样式必须 `:global()`

`DatePickerContent` 经 `PopoverPortal` 渲染到 `<body>` 后，父组件的 **scoped `data-v` 属性不传播到容器元素**（日历内部插槽元素有，唯独 reka-ui 渲染的容器没有），`.cc-calendar-content[data-v-xxx]` 规则全部失效，`z-index` 退化为 `auto` 会被 `modal-mask`（`z-index: 100`）盖住。

```css
/* 必须 :global()，否则日历 z-index/背景/阴影全失效 */
:global(.cc-calendar-content) {
  background: var(--frost-surface);
  z-index: 110; /* > modal-mask 的 100 */
  /* …边框/阴影/padding/min-width/backdrop-filter（瞬态层允许） */
}
```

### 8.2 segment 输入组件：外层禁止 `<label>`，用 `<div>`

`TimeField`/`DatePickerField` 的 segment 是 `contenteditable` div（**非 labelable 元素**），字段内唯一的 labelable 元素是组件内部的隐藏 input；`<label>` 包裹时点击 segment 会激活该隐藏 input，其 `onFocus` 强制聚焦第一个 segment → 点「分」跳「时」、点「日」跳「年」。

```vue
<!-- 错误：label 包裹 → 焦点跳到第一个 segment -->
<div class="cc-field">
  <span class="cc-field-label">时间</span>
  <TimeFieldRoot … />
</div>
```

**例外**：`NumberField` 的原生 input 是 labelable，`<label>` 包裹正常。

### 8.3 v-model 绑定日期/时间值：用 `shallowRef`

`Time`/`DateValue` 含 `#private` 字段，`ref` 深度解包会破坏与 Reka UI 的类型匹配；必须 `shallowRef` 持有（`CountdownCard.vue` 的 `scheduleTime`/`dailyTime`/`scheduleDate` 即此模式）。

### 8.4 浮层层级

| 层 | z-index |
|---|---|
| `modal-mask`（遮罩） | 100 |
| 日历弹层 `.cc-calendar-content` | 110 |
