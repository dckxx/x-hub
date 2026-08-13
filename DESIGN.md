# x-hub Design System

> 版本对齐：v0.1.8。本文档为当前实现的唯一设计基线，UI 改动以本文件 + `src/style.css` 为准。

## 1. Atmosphere & Identity

x-hub 是一个安静、可靠的本地桌面工作台：用户打开它是为了立刻继续工作，而不是浏览一个复杂的仪表盘。视觉签名是「**玻璃质感卡片 + 极简侧边轨道 + 轻微纸张层次**」：侧栏导航始终固定且可收起，主区用细微信号变化区分各模块，不使用装饰性渐变或高饱和色抢夺注意力。标题栏与背景透明，让内容沉浸感更强。

## 2. Color

### Palette（亮色 `:root` / 暗色 `[data-theme="dark"]`）

| Role | Token | Light | Dark | Usage |
|---|---|---|---|---|
| Page | `--bg-page` | `#eceff6` | `#12131b` | 工作区底色 |
| Sidebar | `--bg-sidebar` | `rgba(255,255,255,.42)` | `rgba(255,255,255,.05)` | 固定导航栏 |
| Surface | `--bg-card` | `rgba(255,255,255,.52)` | `rgba(255,255,255,.06)` | 主面板玻璃卡 |
| Surface solid | `--bg-card-solid` | `rgba(255,255,255,.75)` | `rgba(255,255,255,.10)` | 弹窗、浮层 |
| Surface soft | `--bg-card-soft` | `rgba(255,255,255,.40)` | `rgba(255,255,255,.07)` | 控件、列表 hover |
| Text primary | `--text-1` | `#26231d` | `#f2efe8` | 标题、正文 |
| Text secondary | `--text-2` | `#57524a` | `#ccc8bf` | 辅助信息 |
| Text muted | `--text-3` | `#8d877d` | `#9b968c` | 元数据、图标 |
| Border subtle | `--border-soft` | `rgba(255,255,255,.55)` | `rgba(255,255,255,.16)` | 分隔、输入框 |
| Border strong | `--border-strong` | `rgba(40,35,60,.22)` | `rgba(255,255,255,.28)` | 焦点、强调描边 |
| Accent | `--brand-500` | `#5b5bf5` | `#8b8bff` | 当前项、主操作、焦点 |
| Accent soft | `--brand-50` | `#eeeeff` | `#26263f` | 激活背景、选中状态 |
| Accent glow | `--brand-glow` | `rgba(91,91,245,.18)` | `rgba(139,139,255,.28)` | 焦点环、光晕 |
| Scrim | `--scrim` | `rgba(38,35,29,.48)` | 暗色同值 | 弹窗遮罩（暗色下避免过亮） |
| Success | `--c-green-ink` | `#15803d` | 暗色同值 | 正向反馈 |

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
- 卡片使用半透明玻璃底色 + 阴影分层，不使用纯色装饰条或渐变。
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

### Dashboard（工作台）— 三列 Bento 网格

```
┌───────────────┬───────────────────┬──────────────┐
│ 时钟          │ Token 用量卡       │  待办清单     │
│ 系统资源监视器 │ 提示词百宝箱       │  (grid-row    │
│ 便签 ×2       │ (中列下半)         │   1/3)       │
└───────────────┴───────────────────┴──────────────┘
│              最近使用通栏（跨三列）                 │
└──────────────────────────────────────────────────┘
```

- `grid-template-columns: minmax(0,1.2fr) minmax(0,1.8fr) minmax(0,1fr)`，行 `auto minmax(0,1fr) auto`，gap 16px。
- 左列 flex 栈：时钟 → 系统监视 → 便签（两张 1fr 并排）。
- 待办占右列整列（grid-row 1/3），最近使用通栏占底部整行。
- 首页铺满视口无滚动，卡片内容区内滚动；960px 以下折两列，720px 以下单列堆叠。
- 侧栏默认收起，会话内可手动展开。

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

- **Structure**: 主导航（工作台/速记/速达/用量）、底部设置、主题切换、收起按钮。
- **Variants**: active、hover、disabled/empty。
- **States**: default、hover、active、focus、empty。
- **收起态**: 仅图标 + hover 右侧名称气泡（data-tip，300ms 延迟显示）。
- **Motion**: 150ms 背景与颜色变化，不做入场编舞。

### Glass card（基础卡片）

- 半透明玻璃底色（`--bg-card`）+ `--shadow-card` + `--radius-lg`(12px)，内部控件 `--radius-md`(8px)。
- 所有工作台卡片同一高度语义；标题行（icon + 标题 + 右侧动作）统一 16px/650。

### Todo card

- **Structure**: 标题行 + 分段（待办/已完成）、添加输入行、待办列表。
- **States**: default、hover、done（删除线 + 降透明度）、highlight（全局搜索直达后 3s 高亮）。
- **Interactions**: 勾选切换完成；**优先级圆点**（10px 纯色点：灰/黄/红 = 普通/重要/紧急，点击循环，hover 放大）；双击行内编辑；删除按钮 hover 显现；删除可撤销。
- **Accessibility**: 分段使用 `role="tablist"`，优先级圆点可键盘触发，勾选/删除带 `aria-label`。

### 其他卡片组件

| 组件 | 说明 |
|---|---|
| ClockCard | 时间 HH:mm + 日期星期，30s 轮询 |
| SysMonitorCard | CPU/内存进度条 + 2s 轮询（sysinfo 后端） |
| StickyCard | 便签 x2（slot 1/2），600ms 防抖自动保存 |
| TokenStatsCard | 用量三指标 + 近 7 日迷你趋势，5min 自动刷新 + 手动刷新 |
| PromptBoxCard | 提示词列表卡，点击复制 + 置顶标 + 管理入口 |
| RecentBar | 最近使用通栏（按 last_launched_at 排序，前 10） |
| Suda | 速达资源管理视图（筛选 tabs + 卡片网格 + 拖拽导入 + 右键菜单） |
| UsageView | 用量详情视图（双栏，明细分页） |

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
