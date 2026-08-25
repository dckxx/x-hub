# x-hub (个人效率工作台)

**生成:** 2026-08-20 | **分支:** master | **版本:** 0.2.3

## 概述

基于 Tauri 2 + Vue 3 + Tailwind CSS 4 的本地桌面效率工作台（Bento 风格 Dashboard）。导航含 5 个视图：**工作台**（时钟/系统监视/便签/Token 用量/提示词/待办/最近使用/**倒计时**）、**速记**（笔记）、**速达**（应用/网页/文件资源）、**用量**（AI 用量统计）、**设置**（侧栏左下角独立入口，双栏分类：通用/外观/工作台/快捷键/数据）。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6。

## 结构

```
x-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：引入 style.css，createApp(App).mount('#app')
│   ├── App.vue                 # 窗口壳：按 label 路由（主窗渲染首页 / sticky-* 渲染便签浮窗 / countdown-* 渲染倒计时浮窗）
│   ├── index/index.vue         # 首页：侧栏导航(工作台/速记/速达/用量) + 左下角设置入口 + 视图协调 + 三轴主题 + 启动欢迎页 + 中上区块切换
│   ├── style.css               # 设计令牌（亮/暗色 CSS 变量）+ Tailwind + 通用组件样式
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用 + 23 类模型类型
│   ├── stores/workbench.ts     # 响应式状态管理（reactive + readonly，无 Pinia）
│   ├── composables/            # useResourceIcon（资源图标渲染）/ useFocusTrap（弹窗焦点陷阱）/ useTheme（三轴主题）
│   ├── utils/                  # categories（文件分类）/ time / web / error-report / chime（提示音）
│   └── components/
│       ├── TitleBar.vue        # 透明自制标题栏（startDragging 拖动 + 窗口控制 + 搜索入口）
│       ├── ClockCard.vue       # 时钟卡片（HH:mm + 日期星期 + 最近倒计时环形进度，30s 轮询）
│       ├── SysMonitorCard.vue  # 系统资源监视器（CPU/内存，2s 轮询，sysinfo 后端）
│       ├── StickyCard.vue      # 便签卡片 ×2（slot 1/2，统一玻璃卡，600ms 防抖自动保存）
│       ├── CountdownCard.vue   # 倒计时卡片（时长/定时/每天/间隔 新建 + 列表 + 暂停/浮窗/删除）；也是中上区块默认内容
│       ├── CountdownFloat.vue  # 倒计时圆形浮窗（水位水波动画，透明置顶小窗）
│       ├── DetachedStickyWindow.vue # 便签浮窗小窗（sticky-* label 专属渲染）
│       ├── TokenStatsCard.vue  # Token 用量统计卡（今日总量 + 三指标 + 监听绿点，5min 自动刷新）
│       ├── NotesOverviewCard.vue / TodoOverviewCard.vue / ResourcesOverviewCard.vue  # 中上区块可选概览卡（速记/待办/速达统计）
│       ├── PromptBoxCard.vue   # 提示词百宝箱卡片（点击复制 + 置顶标 + 复制计数）
│       ├── PromptManageDialog.vue  # 提示词管理弹窗（新增/编辑/删除/置顶）
│       ├── TodoCard.vue        # 待办清单（分段视图 + 优先级圆点 + 行内编辑 + 删除撤销）
│       ├── RecentBar.vue       # 最近使用通栏（按 last_launched_at 排序，前 10）
│       ├── Suda.vue            # 速达资源管理（全部/常用/应用/网页/文件 + 文件二级分类 + 拖拽导入 + 扫描安装应用）
│       ├── SudaFormDialog.vue  # 新增/编辑资源弹窗（app/web/file + 文件选择）
│       ├── SudaScanDialog.vue  # 扫描已安装应用批量导入弹窗
│       ├── AppSelect.vue       # 通用下拉选择器（无头封装，样式自绘）
│       ├── NoteList.vue        # 笔记条目列表（标题/相对时间/摘要 + 标签筛选）
│       ├── NoteEditor.vue      # 笔记编辑弹窗（Markdown 预览 + 600ms 防抖自动保存）
│       ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（资源/笔记/待办 + 300ms 防抖）
│       ├── UsageView.vue       # 用量详情视图（左趋势/提供商排行 + 右明细分页）
│       ├── SettingsView.vue     # 设置视图（双栏：分类导航 通用/外观/工作台/快捷键/数据 + 内容面板）
│       └── ContextMenu.vue     # 通用右键菜单
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态/单实例/数据迁移与恢复/64 命令注册
│   │   ├── commands.rs         # 64 个 Tauri 命令处理函数
│   │   ├── models.rs           # Resource/Note/Todo/Sticky/Snippet/Tag/Usage*/Countdown 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移（init_in_memory 仅测试用）
│   │   ├── config.rs           # JSON 配置文件读写（AppConfig/WindowState + 三轴主题 + 用量游标 + 中上区块 + 提示音开关）
│   │   ├── process.rs          # 外部进程启动/URL 打开/本地路径打开（app/web/file）+ UAC 提权
│   │   ├── shortcut.rs         # 全局快捷键注册（默认 Ctrl+Shift+Space）
│   │   ├── tray.rs             # 系统托盘（显示/隐藏/退出菜单）
│   │   ├── sysmon.rs           # 系统资源监视（CPU/内存，sysinfo crate）
│   │   ├── usage.rs            # AI 用量同步（opencode.db）/汇总/详情/排行
│   │   ├── countdown_ticker.rs # 倒计时后台驱动线程（1s 轮询到期项→通知+事件+顺延）
│   │   ├── countdown_window.rs # 倒计时圆形浮窗（创建/销毁/位置持久化，countdown-{id}）
│   │   ├── sticky_window.rs    # 便签脱离浮窗（创建/销毁，sticky-{id}）
│   │   ├── notify.rs           # 系统通知封装（tauri-plugin-notification）
│   │   └── repo/               # 数据访问层：resource, note, todo, sticky, snippet, tag, countdown
│   ├── capabilities/default.json  # Tauri 权限声明（含 start-dragging/global-shortcut/dialog/notification）
│   └── tauri.conf.json         # 窗口配置（无边框、1400x900）
├── docs/
│   ├── design-spec.md          # 原始 v1.0 设计基线（历史文档，已随实现同步加注；当前基线以 DESIGN.md 为准）
│   └── reka-ui.md              # Reka UI 使用规范（铁律/踩坑记录/调试指南，新增组件前必读）
├── DESIGN.md                   # 当前设计系统（唯一实现基线，与 style.css 对齐；§8 为 Reka UI 组件规范）
└── package.json
```

## 入口点

| 关注点 | 文件 |
|--------|------|
| 前端启动 | `src/main.ts` → `src/App.vue`（窗口壳，按 label 路由） → `src/index/index.vue`（首页） |
| 状态管理 | `src/stores/workbench.ts` → `useStore()` |
| 后端启动 | `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` → `run()` |
| 路由 | 无 Vue Router，侧栏切换 activeView 渲染对应视图/面板 |

## 设计规范

**实现基线见 `DESIGN.md`（当前设计系统）与 `docs/design-spec.md`（原始 v1.0 基线）。** 速览：

- **设计令牌**：全部定义在 `src/style.css`（CSS 变量，亮色 `:root` + 暗色 `[data-theme="dark"]` 覆盖），组件一律引用变量，禁止硬编码色值
- **主色（三轴主题 v0.1.15）**：品牌强调色由 `--accent` 内联 CSS 变量注入（默认亮 `#5B5BF5` / 暗 `#8b8bff`），`--brand-500/600/50/glow` 全部经 `color-mix` 派生自 `--accent`；主题 = 模式（亮/暗/系统）× 预设（10 单色 + 10 渐变）× 强调色（8 预设 + 自定义 hex）三轴独立配置（`useTheme` + 设置「外观」区）
- **玻璃卡片**：常驻表面用 `--frost-surface`（静态烘焙渐变伪毛玻璃）+ `--frost-edge` 顶部高光 + `--shadow-card` + `--radius-lg`(12px)，内部控件 8px；真 `backdrop-filter` 仅用于弹窗/菜单/下拉等瞬态表面（性能策略见 DESIGN.md §7）
- **强调色**：`--c-yellow/red/blue/green/pink/orange/purple/gray` 8 色 + ink/soft 变体，资源图标按名称 hash 取色（`useResourceIcon`）
- **字体层级**：Section title 16/650、Body 13、Caption 12、Micro 11（见 DESIGN.md §3）
- **布局**：`app-body` 两栏 Grid（220px 侧栏 / 56px 收起态）；工作台为三列 Bento 网格；速记/速达/用量为独立视图
- **交互动效**：hover 轻微上浮 + shadow、按钮按下 scale(0.96)、弹窗 0.2s 缩放渐入
- **弹窗遮罩**：统一 `--scrim` 令牌（暗色下保证对比度），`useFocusTrap` 焦点陷阱

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 64 个命令

## 数据模型（SQLite）

| 表 | 说明 |
|----|------|
| `resources` | 速达资源（app/web/file，category/icon/args/sort_order/last_launched_at） |
| `notes` | 速记笔记（title/content） |
| `tags` / `note_tags` | 笔记标签（多对多） |
| `todos` | 待办（done/priority/completed_at） |
| `stickies` / `detached_stickies` | 便签（slot 1/2）与脱离浮窗 |
| `snippets` | 提示词（is_pinned/copy_count/last_copied_at） |
| `ai_usage` | AI 用量明细（session_id/provider/model/tokens*/cost/time_created/source） |
| `countdowns` | 倒计时（repeat_mode once/daily/interval + end_at/total_ms/interval_minutes/paused/finished/floated/float_x/float_y） |

> 旧版 `groups`/`files` 表已并入 `resources`（Speed-to-launch 合一）；索引含 `idx_notes_updated`、`idx_todos_created`、`idx_ai_usage_time`、`idx_resources_category`、`idx_countdowns_end` 等。

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 侧栏 `navigation` 数组 + `activeView` 切换，工作台为组合式面板网格
3. **App.vue 窗口壳：** 按窗口 label 路由（主窗渲染 `src/index/index.vue`，`sticky-*` 渲染便签浮窗，`countdown-*` 渲染倒计时浮窗），仅做路由分发，零业务逻辑；所有首页逻辑在 `src/index/index.vue`
4. **无 NaiveUI：** 全部 UI 自绘，样式基于 `style.css` 设计令牌（Bento 玻璃风格 + 暗色 `[data-theme="dark"]`）
5. **图标用 lucide-vue-next：** 组件内 `import { Xxx } from 'lucide-vue-next'`，按需 `:size`/`:stroke-width`（1.8~2.2）微调，颜色继承 currentColor；仅 TitleBar 品牌 Logo 保留手写 SVG
6. **窗口拖动：** TitleBar 用 `getCurrentWindow().startDragging()` + mousedown 监听（非 `data-tauri-drag-region` 属性）
7. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs` on_window_event + `api.prevent_close()`）
8. **窗口状态持久化：** 尺寸/位置/置顶在关闭时由 Rust 端保存到 JSON，启动时恢复；最大化图标切换用 `isMaximized()` + `onResized` 监听
9. **笔记/便签自动保存：** 600ms 防抖（NoteEditor.vue / StickyCard.vue）
10. **搜索防抖：** 300ms（GlobalSearch.vue）
11. **全局快捷键：** 默认 Ctrl+Shift+Space 切换窗口显隐（`shortcut.rs` + lib.rs）；可在设置中录制，失焦/回车自动保存，录制中失焦取消并还原（SettingsView.vue）
12. **轻提示：** index.vue `provide('showToast')`，子组件 `inject` 使用
13. **只读 props：** store.state 为 readonly 深度代理，组件 props 用 `readonly Note[]` 等类型
14. **拖拽导入：** 拖入 exe/lnk 到窗口 → `onDragDropEvent`（Suda.vue）→ `parse_dropped_path` 命令（.lnk 经 PowerShell COM 解析目标 + System.Drawing 提取图标存 `app_data_dir/icons/`）→ 自动预填资源弹窗；图标经 `convertFileSrc`（assetProtocol 已启用，scope `$APPDATA/**`）渲染，提取失败回退名称 hash 首字母
15. **PowerShell 调用约定：** 一律用**环境变量传参**（`Command::env`）而非 `$args`——实测 `-Command` 模式下 `$args` 不可靠；输出前设 `[Console]::OutputEncoding=UTF8` 防中文乱码
16. **文件选择：** 已集成 tauri-plugin-dialog（`dialog:allow-open` 权限）；SudaFormDialog 路径/图标输入框右侧有选择按钮，选 exe/lnk 自动解析名称与图标，选图标文件经 `import_icon_file` 存入 icons 目录
17. **AI 用量：** `usage.rs` 从 opencode 数据库按 message 粒度同步到 `ai_usage` 表（游标 `usage_sync_cursor` 持久化在 config），避免长会话跨天归因错误；`sync_ai_usage`/`get_usage_summary`/`get_usage_detail` 三命令；汇总含今日/7日/月/累计与今日调用次数
18. **系统监视：** `sysmon.rs` 用 sysinfo crate 返回 CPU/内存，2s 轮询（SysMonitorCard.vue）
19. **GPU 性能约束（v0.1.13）：** 常驻卡片禁用 `backdrop-filter`，一律用 `--frost-surface` 静态烘焙渐变模拟毛玻璃；`backdrop-filter` 只允许出现在瞬态层（弹窗/菜单/下拉/tooltip）；周期性更新的进度条用 `transform: scaleX` 而非 `width`，避免触发布局重排
20. **倒计时驱动（v0.1.13）：** 到期判定、通知、顺延全部在 Rust `countdown_ticker.rs` 后台线程（1s 轮询），**不能依赖前端 setInterval**（WebView 隐藏/最小化会节流）；前端只做展示与用户操作。到点发系统通知（tauri-plugin-notification）+ emit `countdown-fired` / `countdowns-changed` 事件；完全退出/休眠期间错过的提醒（超 5s）静默顺延不补发。`once` 到点置 finished 灰态，`daily` 按 24h 顺延，`interval` 按 `interval_minutes` 顺延
21. **倒计时浮窗（v0.1.13）：** 每个倒计时可浮起为独立透明圆窗（label `countdown-{id}`，300×340 固定、无边框、置顶、skip_taskbar），圆形水位随剩余比例下降 + 双层正弦波滚动动画；浮起状态与位置持久化在 `countdowns` 表，重启恢复；`once` 到点自动收窗。App.vue 按 label 前缀路由到 `CountdownFloat.vue`
22. **倒计时提示音：** 默认关闭（`countdown_sound` 配置，设置视图开关）；开启后前端 WebAudio 合成双音（`utils/chime.ts`，无外部音频文件），仅主窗口播放避免多窗重音
23. **reka-ui（^2.10.3）组件：** 仅用于复杂输入（DatePicker 定时日期 / TimeField 时:分 / NumberField 步进），无头组件样式全部自绘；v-model 绑定 `Time`/`DateValue` 一律用 `shallowRef`（含 `#private` 字段，ref 深度解包破坏类型匹配）——详见 `docs/reka-ui.md`
24. **reka-ui Portal 弹层（铁律）：** `DatePickerContent` 等经 Portal 渲染到 `<body>` 后父组件 scoped `data-v` 不传播到容器，容器样式（`z-index`/背景/边框/阴影）全部失效 → 日历被 `modal-mask`(100) 盖住选不到；容器样式必须用 `:global()`，`z-index` 设 110（CountdownCard.vue `.cc-calendar-content` 即此例）
25. **reka-ui segment 组件（铁律）：** `TimeField`/`DatePickerField` 外层禁止 `<label>` 包裹（segment 是 contenteditable div、非 labelable，label 会激活组件内部隐藏 input → `onFocus` 强制聚焦第一个 segment，表现为点「分」跳「时」）；外层用 `<div class="cc-field">`；`NumberField` 的原生 input 不受影响可继续用 label
26. **主题三轴系统（v0.1.15）：** 主题 = 模式（light/dark/system，`data-theme`）× 预设（10 单色 `data-preset` + 10 渐变，渐变仅覆盖 `--app-bg` 背景）× 强调色（8 预设 + 自定义 hex，inline `--accent`）。`style.css` 中 `--brand-500` = `var(--accent)`，`--brand-600/50/glow` 均 `color-mix` 派生；实现/读取都在 `composables/useTheme.ts`，配置字段 `theme_mode`/`theme_preset`/`accent_color`（旧 `theme` 字段经 serde alias 自动迁移）
27. **工作台中上区块（v0.1.15）：** 中列上半内容由配置 `dashboard_mid_content` 控制，可选 `countdown`（倒计时，默认）/ `token`（Token 统计）/ `notes`（速记概览）/ `todo`（待办概览）/ `resources`（速达数量），设置「工作台」区切换；倒计时卡默认占据中列上半，Token 用量卡仅在切到 `token` 时显示
28. **侧栏默认收起（v0.1.15）：** `sidebarCollapsed` 默认 `true`（56px 图标态，hover 出名称气泡）；展开/收起按钮仅在设置开启 `sidebar_toggle` 后出现（默认关闭）；720px 以下强制恢复文字导航
29. **关于 / 更新日志（v0.1.16）：** 设置「关于」区（`AboutSection.vue`）展示版本号 + 开源声明 + 内置版本历史；changelog 单一来源为仓库根 `RELEASE_NOTES.md`，经 `about.rs` `include_str!` 打包进二进制（**零网络**），`get_app_info` 返回 `{version, changelog, latest_section}`。版本号运行时读 `app.package_info().version`（随 `tauri.conf.json` 烘焙），README badge 是文档侧唯一真相。升级检测 `check_whats_new` 在启动时调用：`last_seen_version` 空→首跑仅记录；与当前版本不同→推进记录，且仅当 `whats_new_enabled`（默认开）开启才返回最新说明给 `WhatsNewDialog.vue` 弹一次。RELEASE_NOTES 累积式：每发版在顶部新增一节 `# vX.Y.Z 发布说明`（`version_sections()` 按 `# ` 一级标题切分，最新在前）
30. **优先复用现成组件：** 需要下拉选择、弹窗、输入等交互控件时，先查 `src/components/` 已有通用组件（如 `AppSelect.vue` 下拉选择器、`ContextMenu.vue` 右键菜单、`useFocusTrap` 焦点陷阱），优先复用而非新写原生控件（如原生 `<select>`）——保证交互与视觉一致、避免样式重复（反例：设置「粘贴方式」曾用原生 `<select>` 加 `min-width` 撑宽，应改用 `AppSelect`）
31. **新增浮窗窗口必须同步多处 label 配置（否则浮窗闪出「欢迎回来」启动页）：** 启动欢迎页 `#boot-splash` 内联在 `index.html`（所有窗口共用），head 内联脚本用**白名单**判定——只要 `window.__TAURI_INTERNALS__.metadata.currentWindow.label !== 'main'` 就 `data-no-splash` 隐藏 splash（切勿改回黑名单逐个罗列，漏加即复现本 bug）。新增浮窗需同步：① `capabilities/default.json` 的 `windows` 数组加 label；② `App.vue` 按 label 路由到浮窗组件；③ Rust 侧窗口 label 常量（如 `float_window.rs`）；④ `lib.rs` 注册对应命令。

## 命令速查

```bash
npm run dev           # Vite 开发服务器（浏览器预览 http://localhost:1420）
npm run tauri:dev     # Tauri 开发窗口（需 Rust 工具链）
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建桌面应用（产物在 src-tauri/target/release/bundle/）
```

## 发版清单（版本号单一来源 = README）

每次发版从 README 向下同步版本号（`README.md` 徽章 → `package.json` → `src-tauri/tauri.conf.json` → `src-tauri/Cargo.toml` → `AGENTS.md` 头部），并：

1. 在 `RELEASE_NOTES.md` **顶部**新增一节 `# vX.Y.Z 发布说明`（累积式，旧版依次排后，勿覆盖历史）。
2. 同步 `README.md` 版本徽章与 `DESIGN.md` 顶部「版本对齐」。
3. git tag 用 `vX.Y.Z` 触发 `.github/workflows/release.yml`（tag 号须与 `tauri.conf.json` version 一致，否则打包产物版本漂移）。

## 注意事项

- **decorations: false**：窗口无边框，标题栏/窗口控制全部自定义（TitleBar.vue）
- **startDragging 权限**：`core:window:allow-start-dragging` 已在 capabilities 声明
- **数据目录：** `app.path().app_data_dir()/` = `%APPDATA%\x-hub`（identifier 为 `x-hub`；旧标识 `com.workbench.desktop` 的数据在启动时自动迁移一次，且 `lib.rs::fix_icon_paths` 会把数据库中的旧图标路径批量替换为新目录）
- **日志：** `tauri-plugin-log` 文件日志 → `%APPDATA%\x-hub\logs\x-hub.log`（Info 级别），同时输出 Stdout + Webview；所有命令入口记录成功/失败，数据查询类用 `log::debug!` 防噪音；启动程序遇 os error 740（需要管理员权限）自动经 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权
- **配置位置：** 与数据库同目录的 JSON 文件（含 theme_mode/theme_preset/accent_color/sidebar_toggle/window/global_shortcut/usage_db_path/usage_sync_cursor/dashboard_mid_content/countdown_sound/whats_new_enabled/last_seen_version）
- **数据恢复：** `backup_data`/`restore_data` 命令只把备份暂存为 `restore.db`/`restore_icons` 并写 `.restore_pending` 标记，重启时 `apply_pending_restore` 才替换正式数据（lib.rs）
- **SQLite：** 使用 `rusqlite` crate（bundled）
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
- **测试工具函数：** `db.rs::init_in_memory` 仅 `#[cfg(test)]` 使用；`repo/*.rs` 含单元测试（snippet/usage 有端到端验证）
- **dialog 插件：** 前端 `@tauri-apps/plugin-dialog` 的 `open()` 需 `dialog:allow-open` 权限（已声明）；浏览器预览环境需 `isTauri()` 守卫
- **reka-ui 调试：** segment 输入/焦点问题必须用**真实键盘事件**验证（Playwright `browser_press_key` 逐个按键），`browser.type` 类工具是直接改 DOM 文本、不触发 `keydown`，会造成「显示变了但 v-model 不同步」的假象；验证时检查快照中 segment 的 `[active]`（焦点位置）与隐藏 input 的 value（如 `15:45:00`）是否同步

## 待实现

- 暂无排期需求；可探索方向：拖拽排序动效打磨、键盘导航、前端单元测试、打包发布全流程验证（tauri:build）
