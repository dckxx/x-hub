# x-hub Design System

## 1. Atmosphere & Identity

x-hub 是一个安静、可靠的本地桌面工作台：用户打开它是为了立刻继续工作，而不是浏览一个复杂的仪表盘。视觉签名是“稳定的左侧轨道 + 轻微的纸张层次”：导航始终固定，主区用细微的明度变化区分今日、笔记和文件，不用装饰性渐变抢夺注意力。

## 2. Color

### Palette

| Role | Token | Light | Dark | Usage |
|---|---|---|---|---|
| Page | `--bg-page` | `#f4f5f8` | `#111217` | 工作区底色 |
| Sidebar | `--bg-sidebar` | `#ebeef4` | `#181a21` | 固定导航、快捷启动 |
| Surface | `--bg-card-solid` | `#ffffff` | `#20222b` | 主面板、弹窗 |
| Surface soft | `--bg-card-soft` | `#f1f3f7` | `#292c36` | 控件、列表 hover |
| Text primary | `--text-1` | `#20232b` | `#f4f5f8` | 标题、正文 |
| Text secondary | `--text-2` | `#596171` | `#c8ccd6` | 辅助信息 |
| Text muted | `--text-3` | `#7f8796` | `#9399a8` | 元数据、图标 |
| Border subtle | `--border-soft` | `rgba(36,43,58,.10)` | `rgba(255,255,255,.09)` | 分隔、输入框 |
| Accent | `--brand-500` | `#5b5bf5` | `#8b8bff` | 当前项、主操作、焦点 |
| Accent soft | `--brand-50` | `#eeefff` | `#2b2c4c` | 激活背景、选中状态 |
| Success | `--c-green-ink` | `#15803d` | `#86efac` | 正向反馈 |
| Danger | `--c-red-ink` | `#b91c1c` | `#fca5a5` | 删除、错误 |

### Rules

- 采用 restrained palette；靛紫只表达当前选择、可执行主操作和键盘焦点。
- 不使用装饰性渐变、彩色侧边条或纯黑背景。
- 状态不能只依靠颜色：当前导航同时使用背景、字重和图标位置变化。

## 3. Typography

### Scale

| Level | Size | Weight | Line Height | Usage |
|---|---:|---:|---:|---|
| App title | 15px | 700 | 1.3 | 顶栏品牌 |
| Page title | 20px | 700 | 1.25 | 工作区标题 |
| Section title | 16px | 650 | 1.35 | 模块标题 |
| Body | 13px | 400 | 1.5 | 正文、列表 |
| Body strong | 13px | 600 | 1.4 | 条目标题、按钮 |
| Caption | 12px | 500 | 1.4 | 元数据、提示 |
| Micro | 11px | 500 | 1.35 | 标签、快捷键 |

### Font Stack

- Primary: `Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif`
- Mono: `ui-monospace, SFMono-Regular, Consolas, monospace`

正文不低于 12px；中文标题和段落使用 `text-wrap: pretty`，避免单字孤行。

## 4. Spacing & Layout

基础单位为 4px。

| Token | Value | Usage |
|---|---:|---|
| `--space-1` | 4px | 图标与文字 |
| `--space-2` | 8px | 紧凑控件 |
| `--space-3` | 12px | 列表项内距 |
| `--space-4` | 16px | 模块间距、卡片内距 |
| `--space-5` | 20px | 主区内距 |
| `--space-6` | 24px | 页面边距 |

桌面基线是 1280×800：顶栏 48px、侧栏 220px、主区内距 24px、模块间距 16px；最小窗口 1100×720。主区采用两行 Grid：上排 `minmax(300px, 0.8fr) minmax(420px, 1.2fr)`，下排文件区横向铺开。Dock 不渲染，快捷启动归入侧栏。

## 5. Components

### App shell

- **Structure**: `header.title-bar` + `aside.sidebar` + `main.workspace`。
- **States**: 正常、暗色主题、最小窗口、主区滚动。
- **Accessibility**: `nav` 使用 `aria-label`，当前项使用 `aria-current`，所有按钮有可见焦点。

### Sidebar navigation

- **Structure**: 品牌区、主导航、快捷启动区、底部本地状态。
- **Variants**: active、hover、disabled/empty。
- **States**: default、hover、active、focus、empty。
- **Motion**: 150ms 背景与颜色变化，不做入场编舞。

### Workspace panel

- **Structure**: 标题行、工具动作、内容区。
- **Variants**: today（待办）、notes、files。
- **States**: default、hover、active、focus、empty、error/drop target。
- **Depth**: tonal shift + subtle border；不同时使用厚边框和大阴影。

### Todo card

- **Structure**: 标题行 + 视图分段（待办/已完成）、添加输入行、待办列表。
- **States**: 待办项 default、hover、done（删除线 + 降透明度）、highlight（全局搜索直达后 3s 高亮）。
- **Interactions**: 勾选切换完成；优先级圆点循环切换（普通→重要→紧急）；双击行内编辑；删除按钮 hover 显现。
- **Accessibility**: 分段使用 `role="tablist"`，优先级圆点可键盘触发，勾选/删除带 `aria-label`。

### Compact control

- **Structure**: icon button / text button / filter tab。
- **States**: default、hover、active、focus、disabled。
- **Accessibility**: 最小 32px 热区；图标按钮使用 `title` 或可读标签。

## 6. Motion & Interaction

- Micro transition: 150ms ease-out，用于按钮和导航状态。
- Standard transition: 200ms ease-out，用于面板/弹窗进入。
- 只动画 `transform`、`opacity` 和颜色；不动画布局尺寸。
- 遵循 `prefers-reduced-motion`，减少动态时直接呈现最终状态。

## 7. Depth & Surface

采用 mixed strategy：页面、侧栏和面板使用 tonal shift；弹窗和拖拽目标保留轻量阴影/边框。主面板圆角 12px，内部控件圆角 8px，禁止 24px 以上的卡片圆角。背景保持稳定，层次来自明度与间距，而非玻璃装饰。
