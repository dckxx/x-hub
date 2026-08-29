# x-hub (个人效率工作台)

**生成:** 2026-08-29 | **分支:** master | **版本:** 0.3.0

## 概述

基于 Tauri 2 + Vue 3 + Tailwind CSS 4 的本地桌面效率工作台（Bento 风格 Dashboard）。侧栏导航含 **工作台**（自由网格布局：时钟/便签×2/系统监视/提示词/待办/倒计时/概览卡 + 最近使用通栏，可经布局编辑器编排）、**速记**（笔记）、**速达**（应用/网页/文件资源），另有**扩展中心/扩展视图**与侧栏左下角独立入口**设置**（双栏 10 分区：常规/AI助手/外观/工作台/快捷键/剪贴板/联网/扩展/数据/关于）。主要子系统：**AI 对话面板**（OpenAI 兼容流式，Ctrl+Shift+K）、**剪贴板历史**（Ctrl+`）、**扩展系统**（module/view/window/drawer 四形态 + service 托管 + 市场）、**应用自动更新**、**倒计时浮窗**、**天气/一言**。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6。

## 结构

```
x-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：引入 style.css，createApp(App).mount('#app')
│   ├── App.vue                 # 窗口壳：按 label 路由（main 主窗 / sticky-* 便签浮窗 / countdown-* 倒计时浮窗 / clipboard 剪贴板浮层 / ext-* 扩展浮窗 / prompt-float 提示词浮窗 / todo-float 待办浮窗）
│   ├── index/index.vue         # 首页：侧栏导航(工作台/速记/速达) + 扩展中心/扩展视图 + 左下角设置入口 + 视图协调 + 三轴主题 + 启动欢迎页
│   ├── style.css               # 设计令牌（亮/暗色 CSS 变量）+ Tailwind + 通用组件样式
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用封装（126 个命令）+ 34 个模型/配置类型
│   ├── stores/workbench.ts     # 响应式状态管理（reactive + readonly，无 Pinia；工作台/便签/待办/倒计时/提示词/AI 对话/扩展/更新）
│   ├── composables/            # useResourceIcon（资源图标）/ useFocusTrap（焦点陷阱）/ useTheme + themeTokens（三轴主题，后者广播给扩展 iframe）/ useDashboardLayout（工作台网格布局）/ useExtensionFrame（扩展 webview 桥接）/ useShortcutRecorder（快捷键录制）
│   ├── utils/                  # categories（文件分类）/ time / web / error-report / chime（提示音）/ weather（Open-Meteo 天气码映射）/ quotes（本地名言兜底语料）/ todoParse（序号列表拆多条待办）
│   └── components/
│       ├── TitleBar.vue        # 透明自制标题栏（startDragging 拖动 + 窗口控制 + AI 对话/搜索入口）
│       ├── ClockCard.vue       # 时钟卡片（HH:mm + 日期星期 + 实时天气 + 一言语录点击换一句）
│       ├── SysMonitorCard.vue  # 系统资源监视器（CPU/内存，2s 轮询，sysinfo 后端）
│       ├── StickyCard.vue      # 便签卡片 ×2（布局部件 slot 1/2，统一玻璃卡，600ms 防抖自动保存）
│       ├── CountdownCard.vue   # 倒计时卡片（时长/定时/每天/间隔 新建 + 列表 + 暂停/浮窗/删除）
│       ├── CountdownFloat.vue  # 倒计时圆形浮窗（水位水波动画，透明置顶小窗，countdown-{id} label）
│       ├── DetachedStickyWindow.vue # 便签脱离浮窗小窗（sticky-{id} label 专属渲染）
│       ├── NotesOverviewCard.vue / TodoOverviewCard.vue / ResourcesOverviewCard.vue  # 速记/待办/速达概览布局部件
│       ├── PromptBoxCard.vue   # 提示词百宝箱卡片（点击复制 + 置顶标 + 复制计数）
│       ├── PromptManageDialog.vue  # 提示词管理弹窗（新增/编辑/删除/置顶）
│       ├── PromptFloat.vue     # 提示词整列表浮窗（prompt-float label 专属渲染）
│       ├── TodoCard.vue        # 待办清单（分段视图 + 优先级圆点 + 行内编辑 + 删除撤销）
│       ├── TodoFloat.vue       # 待办整列表浮窗（todo-float label 专属渲染）
│       ├── RecentBar.vue       # 最近使用通栏（按 last_launched_at 排序，前 10）
│       ├── Suda.vue            # 速达资源管理（全部/常用/应用/网页/文件 + 文件二级分类 + 拖拽导入 + 扫描安装应用 + 指定浏览器打开）
│       ├── SudaFormDialog.vue  # 新增/编辑资源弹窗（app/web/file + 文件选择）
│       ├── SudaScanDialog.vue  # 扫描已安装应用批量导入弹窗
│       ├── AppSelect.vue       # 通用下拉选择器（无头封装，样式自绘）
│       ├── NoteList.vue        # 笔记条目列表（标题/相对时间/摘要 + 标签筛选）
│       ├── NoteEditor.vue      # 笔记编辑弹窗（Markdown 预览 + 600ms 防抖自动保存）
│       ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（资源/笔记/待办 + 300ms 防抖）
│       ├── ChatPanel.vue       # AI 对话面板（OpenAI 兼容 SSE 流式 + 多会话 + 四方位停靠/拖拽调尺寸 + Markdown 渲染）
│       ├── AiProviders.vue     # 设置「AI 助手」区：供应商/模型管理（连通性测试 + 拉取模型批量添加 + API Key 钥匙串/界面脱敏）
│       ├── ClipboardOverlay.vue # 剪贴板历史浮层（clipboard label 专属渲染：文本/图片/文件 + 粘贴回填 + 置顶）
│       ├── ExtensionCenter.vue # 扩展中心视图（已装清单/市场安装/权限管理/固定到侧栏）
│       ├── ExtensionView.vue   # 扩展 view 形态（主区内嵌扩展页面）
│       ├── ExtensionWindow.vue # 扩展 window 形态浮窗（ext-{id} label 专属渲染）
│       ├── ExtensionSettingsDialog.vue # 扩展设置/权限详情弹窗
│       ├── MarketDetailDialog.vue  # 扩展市场详情/安装弹窗
│       ├── UpdateCheckDialog.vue   # 应用更新全局弹窗（新版本信息/下载进度/跳过此版本/立即重启）
│       ├── DashboardLayoutEditor.vue # 工作台布局编辑器视图（部件增删/拖拽/调尺寸，保存 dashboard_layout）
│       ├── AboutSection.vue    # 设置「关于」区（版本/开源声明/版本历史/检查更新）
│       ├── SettingsView.vue    # 设置视图（双栏：分类导航 常规/AI助手/外观/工作台/快捷键/剪贴板/联网/扩展/数据/关于 + 内容面板）
│       └── ContextMenu.vue     # 通用右键菜单
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态/单实例/数据迁移与恢复/126 命令注册/退出停 service
│   │   ├── commands.rs         # Tauri 命令处理函数（资源/笔记/待办/便签/提示词/倒计时/对话/剪贴板/配置/窗口/备份等）
│   │   ├── models.rs           # Resource/Note/Todo/Sticky/DetachedSticky/Snippet/ClipboardItem/Tag/Countdown/Chat* 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移（init_in_memory 仅测试用）
│   │   ├── config.rs           # 数据根下 app.json 读写（AppConfig 全字段 serde default，字段清单见「注意事项·配置位置」）
│   │   ├── paths.rs            # 数据根目录解析（标准版 %APPDATA%\x-hub / 便携版 exe\data / 设置自定义迁移）
│   │   ├── process.rs          # 外部进程启动/URL 打开/本地路径打开（app/web/file）+ UAC 提权
│   │   ├── browsers.rs         # 已安装浏览器枚举（注册表 StartMenuInternet）+ open_url_with_browser 指定浏览器打开
│   │   ├── shortcut.rs         # 全局快捷键注册（主窗 Ctrl+Shift+Space + 剪贴板 Ctrl+`，均可自定义）
│   │   ├── tray.rs             # 系统托盘（显示/隐藏/退出菜单）
│   │   ├── sysmon.rs           # 系统资源监视（CPU/内存，sysinfo crate）
│   │   ├── chat.rs             # OpenAI 兼容 SSE 流式对话客户端 + API Key 系统钥匙串（keyring）存取
│   │   ├── clipboard.rs        # 剪贴板监听与历史（文本/图片落盘/文件）+ 粘贴注入（clipboard_paste_method）
│   │   ├── online.rs           # 联网服务：连通性探活/天气（Open-Meteo）/城市地理编码/IP 定位/名言（hitokoto）
│   │   ├── extension.rs        # 扩展扫描/安装/卸载/权限 + extensions_stamp 热更新检测
│   │   ├── market.rs           # 扩展市场（registry.json Ed25519 验签 + GitHub zip 安装/更新/卸载）
│   │   ├── xhub_api.rs         # 扩展桥 API：CAPABILITIES 静态注册表 + xhub_call 分发（见关键约定 32）
│   │   ├── service.rs          # service 扩展托管（Node 后端进程启动/动态端口/探活/停止）
│   │   ├── runtime.rs          # service 运行时解析（系统 Node 优先 → 内置运行时按需下载，自动降级）
│   │   ├── proxy.rs            # /svc/<extId>/* 本地反向代理（扩展前端 → service 后端）
│   │   ├── updater.rs          # 应用自动更新（update.json 验签 + sha256 下载校验 + 重启两步 rename 自替换/回滚）
│   │   ├── signing.rs          # Ed25519 分离签名验签（市场清单 + 更新清单共用，内嵌公钥）
│   │   ├── autostart.rs        # 开机自启动（HKCU Run 键 + --autostart-hidden 静默驻留托盘；清理旧计划任务残留）
│   │   ├── about.rs            # 版本历史打包（include_str! RELEASE_NOTES）+ get_app_info
│   │   ├── countdown_ticker.rs # 倒计时后台驱动线程（1s 轮询到期项→通知+事件+顺延）
│   │   ├── countdown_window.rs # 倒计时圆形浮窗（创建/销毁/位置持久化，countdown-{id}）
│   │   ├── sticky_window.rs    # 便签脱离浮窗（创建/销毁，sticky-{id}）
│   │   ├── float_window.rs     # 通用整列表浮窗（prompt-float 提示词 / todo-float 待办）
│   │   ├── notify.rs           # 系统通知封装（tauri-plugin-notification）
│   │   └── repo/               # 数据访问层：resource, note, todo, sticky, detached_sticky, snippet, tag, countdown, chat, clipboard
│   ├── capabilities/default.json  # Tauri 权限声明（含 start-dragging/global-shortcut/dialog/notification）
│   └── tauri.conf.json         # 窗口配置（无边框、1400x900）
├── docs/
│   ├── design-spec.md          # 原始 v1.0 设计基线（历史文档，已随实现同步加注；当前基线以 DESIGN.md 为准）
│   ├── reka-ui.md              # Reka UI 使用规范（铁律/踩坑记录/调试指南，新增组件前必读）
│   ├── （已迁移）扩展系统文档   # extension-spec/api/evolution 等现位于 x-hub-extensions 仓库 docs/
│   ├── r2-distribution-and-updater.md        # 扩展市场 R2 分发与应用自动更新方案（P0–P3 已实施）
│   ├── file-search-plan.md                   # 本地文件搜索方案（已定稿待实现）
│   └── adr/                    # 架构决策记录（0001 首次使用引导：已决策未实施）
├── DESIGN.md                   # 当前设计系统（唯一实现基线，与 style.css 对齐；§8 为 Reka UI 组件规范）
├── PRODUCT.md                  # 产品定义（用户/目标/品牌个性/设计原则/无障碍基线）
├── CONTEXT.md                  # 领域术语表（易混淆概念精确区分；「引导」节为规划中未实施）
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
- **玻璃卡片**：常驻表面用 `--frost-surface`（静态烘焙渐变伪毛玻璃）+ `--frost-edge` 顶部高光 + `--shadow-card` + `--radius-lg`(12px)，内部控件 8px；真 `backdrop-filter` 仅用于弹窗/菜单/下拉等瞬态表面（性能策略见 DESIGN.md §7），唯一例外是 opt-in 沉浸模式的静态 `.card`（见 `docs/adr/0003`）；铬件（侧栏/标题栏）任何壁纸形态下都保持全透明，与背景构成同一连续平面，勿给铬件垫材质
- **强调色**：`--c-yellow/red/blue/green/pink/orange/purple/gray` 8 色 + ink/soft 变体，资源图标按名称 hash 取色（`useResourceIcon`）
- **字体层级**：Section title 16/650、Body 13、Caption 12、Micro 11（见 DESIGN.md §3）
- **布局**：`app-body` 两栏 Grid（220px 侧栏 / 56px 收起态）；工作台为自由编排 Bento 网格（`useDashboardLayout` + 布局编辑器）；速记/速达/扩展中心为独立视图
- **交互动效**：hover 轻微上浮 + shadow、按钮按下 scale(0.96)、弹窗 0.2s 缩放渐入
- **弹窗遮罩**：统一 `--scrim` 令牌（暗色下保证对比度），`useFocusTrap` 焦点陷阱

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 126 个命令（前端封装一一对应 `src/api/tauri.ts`）

## 数据模型（SQLite）

| 表 | 说明 |
|----|------|
| `resources` | 速达资源（app/web/file，category/icon/args/sort_order/last_launched_at） |
| `notes` | 速记笔记（title/content） |
| `tags` / `note_tags` | 笔记标签（多对多） |
| `todos` | 待办（done/priority/completed_at） |
| `stickies` / `detached_stickies` | 便签（slot 1/2）与脱离浮窗 |
| `snippets` | 提示词（is_pinned/copy_count/last_copied_at） |
| `countdowns` | 倒计时（repeat_mode once/daily/interval + end_at/total_ms/interval_minutes/paused/finished/floated/float_x/float_y） |
| `chat_sessions` / `chat_messages` | AI 对话会话与消息（session_id 索引，模型/角色/内容/用量） |
| `clipboard_history` | 剪贴板历史（kind text/image/file + image_path/file_paths/source_app/is_pinned） |

> 旧版 `groups`/`files` 表已并入 `resources`（Speed-to-launch 合一）；索引含 `idx_notes_updated`、`idx_todos_created`、`idx_resources_category`、`idx_countdowns_end` 等。

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 侧栏 `navigation` 数组 + `activeView` 切换，工作台为组合式面板网格
3. **App.vue 窗口壳：** 按窗口 label 路由（`main` 主窗渲染 `src/index/index.vue`、`sticky-{id}` 便签浮窗、`countdown-{id}` 倒计时浮窗、`clipboard` 剪贴板浮层、`ext-{id}` 扩展浮窗、`prompt-float` 提示词浮窗、`todo-float` 待办浮窗），仅做路由分发，零业务逻辑；所有首页逻辑在 `src/index/index.vue`
4. **无 NaiveUI：** 全部 UI 自绘，样式基于 `style.css` 设计令牌（Bento 玻璃风格 + 暗色 `[data-theme="dark"]`）
5. **图标用 lucide-vue-next：** 组件内 `import { Xxx } from 'lucide-vue-next'`，按需 `:size`/`:stroke-width`（1.8~2.2）微调，颜色继承 currentColor；仅 TitleBar 品牌 Logo 保留手写 SVG
6. **窗口拖动：** TitleBar 用 `getCurrentWindow().startDragging()` + mousedown 监听（非 `data-tauri-drag-region` 属性）
7. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs` on_window_event + `api.prevent_close()`）
8. **窗口状态持久化：** 尺寸/位置/置顶在关闭时由 Rust 端保存到 JSON，启动时恢复；最大化图标切换用 `isMaximized()` + `onResized` 监听
9. **笔记/便签自动保存：** 600ms 防抖（NoteEditor.vue / StickyCard.vue）
10. **搜索防抖：** 300ms（GlobalSearch.vue）
11. **全局快捷键：** 两个全局快捷键都在 `shortcut.rs` 统一注册——主窗显隐默认 Ctrl+Shift+Space、剪贴板浮层默认 Ctrl+`（避开 Ctrl+Shift+V 无格式粘贴）；均可在设置中录制，失焦/回车自动保存，录制中失焦取消并还原（`useShortcutRecorder` + SettingsView.vue）
12. **轻提示：** index.vue `provide('showToast')`，子组件 `inject` 使用
13. **只读 props：** store.state 为 readonly 深度代理，组件 props 用 `readonly Note[]` 等类型
14. **拖拽导入：** 拖入 exe/lnk 到窗口 → `onDragDropEvent`（Suda.vue）→ `parse_dropped_path` 命令（.lnk 经 PowerShell COM 解析目标 + System.Drawing 提取图标存 `app_data_dir/icons/`）→ 自动预填资源弹窗；图标经 `convertFileSrc`（assetProtocol 已启用，scope `$APPDATA/**`）渲染，提取失败回退名称 hash 首字母
15. **PowerShell 调用约定：** 一律用**环境变量传参**（`Command::env`）而非 `$args`——实测 `-Command` 模式下 `$args` 不可靠；输出前设 `[Console]::OutputEncoding=UTF8` 防中文乱码
16. **文件选择：** 已集成 tauri-plugin-dialog（`dialog:allow-open` 权限）；SudaFormDialog 路径/图标输入框右侧有选择按钮，选 exe/lnk 自动解析名称与图标，选图标文件经 `import_icon_file` 存入 icons 目录
17. **AI 用量：** 已拆分为 service 扩展 `com.x-hub.token-stats`（实时读 opencode 数据库聚合，宿主零 token 代码）；详见 `x-hub-extensions/extensions/com.x-hub.token-stats`。宿主侧旧用量代码（`usage.rs`/TokenStatsCard/用量视图）已全部移除，侧栏无「用量」入口
18. **系统监视：** `sysmon.rs` 用 sysinfo crate 返回 CPU/内存，2s 轮询（SysMonitorCard.vue）
19. **GPU 性能约束（v0.1.13）：** 常驻卡片默认禁用 `backdrop-filter`，一律用 `--frost-surface` 静态烘焙渐变模拟毛玻璃；`backdrop-filter` 只允许出现在瞬态层（弹窗/菜单/下拉/tooltip）+ 沉浸模式的静态 `.card`（opt-in 受控例外，见 `docs/adr/0003`）；周期性更新的进度条用 `transform: scaleX` 而非 `width`，避免触发布局重排
20. **倒计时驱动（v0.1.13）：** 到期判定、通知、顺延全部在 Rust `countdown_ticker.rs` 后台线程（1s 轮询），**不能依赖前端 setInterval**（WebView 隐藏/最小化会节流）；前端只做展示与用户操作。到点发系统通知（tauri-plugin-notification）+ emit `countdown-fired` / `countdowns-changed` 事件；完全退出/休眠期间错过的提醒（超 5s）静默顺延不补发。`once` 到点置 finished 灰态，`daily` 按 24h 顺延，`interval` 按 `interval_minutes` 顺延
21. **倒计时浮窗（v0.1.13）：** 每个倒计时可浮起为独立透明圆窗（label `countdown-{id}`，300×340 固定、无边框、置顶、skip_taskbar），圆形水位随剩余比例下降 + 双层正弦波滚动动画；浮起状态与位置持久化在 `countdowns` 表，重启恢复；`once` 到点自动收窗。App.vue 按 label 前缀路由到 `CountdownFloat.vue`
22. **倒计时提示音：** 默认关闭（`countdown_sound` 配置，设置视图开关）；开启后前端 WebAudio 合成双音（`utils/chime.ts`，无外部音频文件），仅主窗口播放避免多窗重音
23. **reka-ui（^2.10.3）组件：** 仅用于复杂输入（DatePicker 定时日期 / TimeField 时:分 / NumberField 步进），无头组件样式全部自绘；v-model 绑定 `Time`/`DateValue` 一律用 `shallowRef`（含 `#private` 字段，ref 深度解包破坏类型匹配）——详见 `docs/reka-ui.md`
24. **reka-ui Portal 弹层（铁律）：** `DatePickerContent` 等经 Portal 渲染到 `<body>` 后父组件 scoped `data-v` 不传播到容器，容器样式（`z-index`/背景/边框/阴影）全部失效 → 日历被 `modal-mask`(100) 盖住选不到；容器样式必须用 `:global()`，`z-index` 设 110（CountdownCard.vue `.cc-calendar-content` 即此例）
25. **reka-ui segment 组件（铁律）：** `TimeField`/`DatePickerField` 外层禁止 `<label>` 包裹（segment 是 contenteditable div、非 labelable，label 会激活组件内部隐藏 input → `onFocus` 强制聚焦第一个 segment，表现为点「分」跳「时」）；外层用 `<div class="cc-field">`；`NumberField` 的原生 input 不受影响可继续用 label
26. **主题三轴系统（v0.1.15）：** 主题 = 模式（light/dark/system，`data-theme`）× 预设（10 单色 `data-preset` + 10 渐变，渐变仅覆盖 `--app-bg` 背景）× 强调色（8 预设 + 自定义 hex，inline `--accent`）。`style.css` 中 `--brand-500` = `var(--accent)`，`--brand-600/50/glow` 均 `color-mix` 派生；实现/读取都在 `composables/useTheme.ts`，配置字段 `theme_mode`/`theme_preset`/`accent_color`（旧 `theme` 字段经 serde alias 自动迁移）
27. **工作台自由网格布局（v0.3.0，取代 v0.1.15 中上区块）：** 工作台卡片由配置 `dashboard_layout`（JSON 网格坐标）驱动，部件目录在 `useDashboardLayout.ts`：clock / sticky1 / sticky2 / notes / todo_overview / resources / countdown / prompts / todo 共 9 种，可增删、拖拽、调宽高并持久化；编辑入口为设置 →「布局编辑器」（独立视图 `DashboardLayoutEditor.vue`，完成后回工作台）。旧字段 `dashboard_mid_content` 已废弃不再被 UI 读取（保留在配置结构中向后兼容）
28. **侧栏默认收起（v0.1.15）：** `sidebarCollapsed` 默认 `true`（56px 图标态，hover 出名称气泡）；展开/收起按钮仅在设置开启 `sidebar_toggle` 后出现（默认关闭）；720px 以下强制恢复文字导航
29. **关于 / 更新日志（v0.1.16，v0.3.0 改版）：** 设置「关于」区（`AboutSection.vue`）展示版本号 + 开源声明 + 内置版本历史 + 检查更新；changelog 单一来源为仓库根 `RELEASE_NOTES.md`，经 `about.rs` `include_str!` 打包进二进制（**零网络**），`get_app_info` 返回 `{version, changelog, latest_section}`。版本号运行时读 `app.package_info().version`（随 `tauri.conf.json` 烘焙），README badge 是文档侧唯一真相。升级提醒已改走**应用自动更新**链路（见约定 35：updater.rs 静默检查 + UpdateCheckDialog 弹窗），旧 `check_whats_new`/`whats_new_enabled`/`last_seen_version` 机制已移除。RELEASE_NOTES 累积式：每发版在顶部新增一节 `# vX.Y.Z 发布说明`（`version_sections()` 按 `# ` 一级标题切分，最新在前）
30. **优先复用现成组件：** 需要下拉选择、弹窗、输入等交互控件时，先查 `src/components/` 已有通用组件（如 `AppSelect.vue` 下拉选择器、`ContextMenu.vue` 右键菜单、`useFocusTrap` 焦点陷阱），优先复用而非新写原生控件（如原生 `<select>`）——保证交互与视觉一致、避免样式重复（反例：设置「粘贴方式」曾用原生 `<select>` 加 `min-width` 撑宽，应改用 `AppSelect`）
31. **新增浮窗窗口必须同步多处 label 配置（否则浮窗闪出「欢迎回来」启动页）：** 启动欢迎页 `#boot-splash` 内联在 `index.html`（所有窗口共用），head 内联脚本用**白名单**判定——只要 `window.__TAURI_INTERNALS__.metadata.currentWindow.label !== 'main'` 就 `data-no-splash` 隐藏 splash（切勿改回黑名单逐个罗列，漏加即复现本 bug）。新增浮窗需同步：① `capabilities/default.json` 的 `windows` 数组加 label；② `App.vue` 按 label 路由到浮窗组件；③ Rust 侧窗口 label 常量（如 `float_window.rs`）；④ `lib.rs` 注册对应命令。
32. **扩展桥 API 能力注册表（v0.2.3）：** 桥 API 不再在 `xhub_api.rs` 手工 `match` 分发，而是集中在一张 `CAPABILITIES` 静态表（每项 `namespace`/`method`/`permission`/`handler`）；**新增能力 = 表里加一行 + 写 handler**，`runtime.info` 返回 `capabilities` 清单供扩展探测。manifest 支持 `requires`（依赖宿主能力，写 `namespace.method`）/`dependsOn`（依赖其它扩展 id，驼峰）/`disabled`（条件禁用 `{platform}`）/`expose`（跨扩展调用白名单）/`actions`（快捷动作，扩展中心渲染按钮）；扫描器求值后在扩展中心标「缺能力/缺依赖/已禁用」并拦截打开。扩展配置走 `config.*` 桥 API：`manifest.config` 为作者默认，`.config.json` 为「用户覆盖」层（覆盖优先，升级扩展不冲掉）；`storage.*` 仍是扩展私有键值、与 `config.*` 各自独立；`sharedStorage.*` 为跨扩展共享键值（需 `shared-storage` 权限）。扩展间协作两条路：事件总线 `events.emit/on`（emit 需 `events` 权限，广播走前端 `broadcastExtensionEvent`）+ 跨扩展调用 `runtime.callExtension`（前端 `routeExtensionCall` 路由 + Rust 校验 expose 白名单 + 目标扩展 `xhub.expose(method, fn)` 注册）。运行时热更新：`extensions_stamp` 命令对 manifest 的路径+mtime 做 FNV 哈希，扩展中心 5s 轮询变化即刷新列表（无需重启）
33. **AI 对话（v0.3.0）：** `chat.rs` 为 OpenAI 兼容 SSE 流式客户端（reqwest），API Key 存系统钥匙串 keyring（**不明文落盘**），界面脱敏（查看/复制）；面板为主形态（标题栏按钮 / Ctrl+Shift+K 唤起，侧栏「对话」入口暂时隐藏——`index.vue` visibleNavigation 过滤，勿直接删导航项），四方位停靠 `chat_panel_side` + 宽/高/透明度均持久化；供应商模型支持测试连通性 + 拉取模型列表批量勾选添加，同供应商共享 Key
34. **剪贴板历史（v0.2.x）：** `clipboard.rs` 后台监听，记录文本/图片/文件三类（相同内容去重，图片缩略图落盘 `数据根\clipboard\images`，删除/清空/过期联动删文件）；浮层是独立窗口（label `clipboard`，ClipboardOverlay.vue），全局快捷键默认 Ctrl+`；条数/TTL/媒体开关/粘贴方式（`clipboard_paste_method`）均入配置
35. **应用自动更新（v0.3.0）：** `updater.rs` 自研链路——启动 5s 后 + 每 `update_interval_hours`（默认 4h）静默检查 `update_endpoint` 的 `releases/update.json`（`signing.rs` Ed25519 验签通过才信任 + `minimumUpgradable` 跳级保护，标准版/便携版分别取包）→ UpdateCheckDialog 弹窗（可跳过版本，记 `skipped_update_version`）→ 流式下载 sha256 校验 → 重启时「exe → exe.old / 新 exe → exe」两步 rename 自替换，失败回滚下次重试；更新包暂存 `数据根\updates\` 用完即清
36. **联网开关（v0.3.0）：** `online.rs` 提供连通性探活 / 天气（Open-Meteo）/ 城市地理编码 / IP 定位 / 名言（hitokoto），受设置「联网」`online_enabled` 总开关控制；语录离线自动回退 `utils/quotes.ts` 本地语料
37. **应用壁纸 + 卡片玻璃透明度：** 壁纸仅主窗口渲染（index.vue 首子元素 `z-index:-1` 固定层，浮窗不跟随），导入走 `import_wallpaper`（复制进 `数据根\wallpapers\` 内容哈希命名 + 清目录旧文件，仅收 png/jpg/webp/bmp、≤30MB，gif 等动图拒收），assetProtocol `$APPDATA/**` 经 `convertFileSrc` 渲染；模糊作用于壁纸层整体（静态 `filter: blur`，**不是**卡片局部 backdrop——见 `docs/adr/0002`）；壁纸蒙版 `wallpaper_veil`（0–0.85 默认 0.3，主题中性底色 `--bg-base-a/b` 罩层，亮色提亮/暗色压暗）解决照片壁纸上侧栏/标题栏灰字与图标对比度不足的问题；沉浸模式 `wallpaper_immersive`（默认关，ADR 0003 受控例外）：`.card` 启用 backdrop-filter blur(16px) 局部取景 + 基底 alpha 大幅下调（亮 0.18/0.12 暗 0.10/0.06，仍乘 `--glass-dim`），开启时整屏静态模糊自动让位、设置里隐藏其开关；铬件（侧栏/标题栏）任何壁纸形态下都保持全透明与背景同一平面（勿垫材质/蒙纱，观感割裂——ADR 0003 有记录），其灰字/图标可读性由壁纸蒙版 + 壁纸态可读性增强负责：html `data-wallpaper` 标记驱动，灰阶令牌（--text-2/3/4）在铬件与主区（main）子树内向 ink 端压两档（不碰全局令牌），文字光晕双层分级——铬件为描边级（四向 1px text-shadow + 柔光）+ svg 双层 drop-shadow，卡片在真实透底时（`data-wallpaper-clear` = 玻璃透明度 <0.9 或沉浸模式）才加弱一档柔光晕（.card/.sv-content/.extension-center/.le-root），不透底保持锐利，侧栏 hover 气泡等瞬态表面排除在外；卡片玻璃透明度 `glass_opacity`（0.4–1.0）经 `--glass-dim` 乘进 `--frost-base` alpha，亮暗两套基底共用同一乘数，弹窗/菜单等瞬态表面不受影响；壁纸文件被外部删除时前端静默回退渐变背景（不抹配置）

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
- **数据目录：** 数据根默认 `app.path().app_data_dir()/` = `%APPDATA%\x-hub`（identifier 为 `x-hub`；旧标识 `com.workbench.desktop` 的数据在启动时自动迁移一次，且 `lib.rs::fix_icon_paths` 会把数据库中的旧图标路径批量替换为新目录）；支持设置中「更改数据存储路径」（迁移后重启生效）与**便携版**（exe 同目录放空文件 `portable` → 数据固定为 `exe\data`），统一由 `paths.rs` 解析
- **日志：** `tauri-plugin-log` 文件日志 → `%APPDATA%\x-hub\logs\x-hub.log`（Info 级别），同时输出 Stdout + Webview；所有命令入口记录成功/失败，数据查询类用 `log::debug!` 防噪音；启动程序遇 os error 740（需要管理员权限）自动经 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权
- **配置位置：** 数据根目录下 `app.json`（与数据库同目录，随「更改数据目录」一起迁移；`config.rs` AppConfig 全字段 serde default，新增字段天然兼容老配置）。主要字段：主题三件套（theme_mode/theme_preset/accent_color）+ 外观（wallpaper_path/wallpaper_blur/wallpaper_veil/wallpaper_immersive/glass_opacity）、sidebar_toggle、window、global_shortcut、dashboard_layout（工作台网格）+ dashboard_mid_content（废弃遗留）、countdown_sound、clock_quote、联网（online_enabled/weather_city/weather_lat/weather_lng/quote_source）、AI 对话（chat_models/chat_panel_width/open/side/height/opacity）、剪贴板（clipboard_shortcut/max_items/ttl_days/paused/paste_method/image_enabled/file_enabled）、字号（font_scale/font_sticky/font_notes/font_prompt/font_todo）、扩展（runtime_strategy/sidebar_extensions/extension_open_modes/market_endpoint）、自启动（run_at_startup）、自动更新（update_endpoint/auto_update_enabled/update_interval_hours/skipped_update_version）
- **数据恢复：** `backup_data`/`restore_data` 命令只把备份暂存为 `restore.db`/`restore_icons` 并写 `.restore_pending` 标记，重启时 `apply_pending_restore` 才替换正式数据（lib.rs）
- **SQLite：** 使用 `rusqlite` crate（bundled）
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
- **测试工具函数：** `db.rs::init_in_memory` 仅 `#[cfg(test)]` 使用；`repo/*.rs` 大多含单元测试（snippet/chat/clipboard 等有端到端验证）
- **dialog 插件：** 前端 `@tauri-apps/plugin-dialog` 的 `open()` 需 `dialog:allow-open` 权限（已声明）；浏览器预览环境需 `isTauri()` 守卫
- **reka-ui 调试：** segment 输入/焦点问题必须用**真实键盘事件**验证（Playwright `browser_press_key` 逐个按键），`browser.type` 类工具是直接改 DOM 文本、不触发 `keydown`，会造成「显示变了但 v-model 不同步」的假象；验证时检查快照中 segment 的 `[active]`（焦点位置）与隐藏 input 的 value（如 `15:45:00`）是否同步
- **指定浏览器打开网页（v0.3.0）：** 速达右键网页资源可「用 XX 打开」；浏览器列表来自注册表 `SOFTWARE\Clients\StartMenuInternet`（HKLM+HKCU+Wow6432Node，`browsers.rs` 枚举，按 exe 路径去重、按显示名排序；显示名取键默认值/LocalizedString，间接字符串或缺失时按 exe 文件名兜底）；`open_url_with_browser` 仅放行 Web 资源 + http/https，成功后与默认打开一致刷新 `last_launched_at`；前端在 Suda.vue 挂载时预热列表缓存，右键零等待
- **右键菜单置位必须在事件派发外（陷阱）：** `ContextMenu.vue` 在 window 上监听 `contextmenu`/`click` 用于点别处关闭菜单；若在卡片 `@contextmenu` 处理器里**同步**置 `menu.visible = true`，同一事件冒泡到 window 的关闭监听会把刚开的菜单立刻关掉（表现为右键无反应），且菜单已开时在另一资源上右键会因 visible 未变化不触发定位 watch（菜单出现在旧位置）。Suda.vue `openMenu` 用 `setTimeout(0)` 把置位推迟到派发结束后——新增右键入口时必须沿用此模式

## 待实现

- 已决策未实施：**首次使用引导**（快速设置弹窗 + 帮助视图；决策见 `docs/adr/0001-first-run-onboarding.md`，术语预登记于 `CONTEXT.md`「引导」节，实施前代码中无 OnboardingDialog/HelpView/onboarding_done）
- 方案已定稿未实施：**本地文件搜索**（索引工作区模型，见 `docs/file-search-plan.md`）
- 可探索方向：拖拽排序动效打磨、键盘导航、前端单元测试、打包发布全流程验证（tauri:build）
