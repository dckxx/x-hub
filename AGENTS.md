# x-hub (个人效率工作台)

**生成:** 2026-08-13 | **分支:** master | **版本:** 0.1.8

## 概述

基于 Tauri 2 + Vue 3 + Tailwind CSS 4 的本地桌面效率工作台（Bento 风格 Dashboard）。导航含 4 个视图：**工作台**（时钟/系统监视/便签/Token 用量/提示词/待办/最近使用）、**速记**（笔记）、**速达**（应用/网页/文件资源）、**用量**（AI 用量统计）。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6。

## 结构

```
x-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：引入 style.css，createApp(App).mount('#app')
│   ├── App.vue                 # 应用根壳：仅引入 src/index/index.vue，无业务逻辑
│   ├── index/index.vue         # 首页：侧栏导航(工作台/速记/速达/用量) + 视图协调 + 主题/搜索/设置
│   ├── style.css               # 设计令牌（亮/暗色 CSS 变量）+ Tailwind + 通用组件样式
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用 + 19 类模型类型
│   ├── stores/workbench.ts     # 响应式状态管理（reactive + readonly，无 Pinia）
│   ├── composables/            # useResourceIcon（资源图标渲染）/ useFocusTrap（弹窗焦点陷阱）
│   ├── utils/                  # categories（文件分类）/ time / web / error-report
│   └── components/
│       ├── TitleBar.vue        # 透明自制标题栏（startDragging 拖动 + 窗口控制 + 搜索/设置入口）
│       ├── ClockCard.vue       # 时钟卡片（HH:mm + 日期星期，30s 轮询）
│       ├── SysMonitorCard.vue  # 系统资源监视器（CPU/内存，2s 轮询，sysinfo 后端）
│       ├── StickyCard.vue      # 便签卡片 ×2（slot 1/2，600ms 防抖自动保存）
│       ├── TokenStatsCard.vue  # Token 用量统计卡（三指标 + 近7日迷你趋势，5min 自动刷新）
│       ├── PromptBoxCard.vue   # 提示词百宝箱卡片（点击复制 + 置顶标 + 复制计数）
│       ├── PromptManageDialog.vue  # 提示词管理弹窗（新增/编辑/删除/置顶）
│       ├── TodoCard.vue        # 待办清单（分段视图 + 优先级圆点 + 行内编辑 + 删除撤销）
│       ├── RecentBar.vue       # 最近使用通栏（按 last_launched_at 排序，前 10）
│       ├── Suda.vue            # 速达资源管理（全部/常用/应用/网页/文件 + 文件二级分类 + 拖拽导入）
│       ├── SudaFormDialog.vue  # 新增/编辑资源弹窗（app/web/file + 文件选择）
│       ├── NoteList.vue        # 笔记条目列表（标题/相对时间/摘要 + 标签筛选）
│       ├── NoteEditor.vue      # 笔记编辑弹窗（Markdown 预览 + 600ms 防抖自动保存）
│       ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（资源/笔记/待办 + 300ms 防抖）
│       ├── UsageView.vue       # 用量详情视图（左趋势/提供商排行 + 右明细分页）
│       ├── SettingsDialog.vue  # 设置弹窗（主题/置顶/全局快捷键/备份恢复）
│       └── ContextMenu.vue     # 通用右键菜单
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态/46 命令注册
│   │   ├── commands.rs         # 46 个 Tauri 命令处理函数
│   │   ├── models.rs           # Resource/Note/Todo/Sticky/Snippet/Tag/Usage* 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移（init_in_memory 仅测试用）
│   │   ├── config.rs           # JSON 配置文件读写（AppConfig/WindowState + 用量游标）
│   │   ├── process.rs          # 外部进程启动/URL 打开/本地路径打开（app/web/file）
│   │   ├── shortcut.rs         # 全局快捷键注册（默认 Ctrl+Shift+Space）
│   │   ├── tray.rs             # 系统托盘（显示/隐藏/退出菜单）
│   │   ├── sysmon.rs           # 系统资源监视（CPU/内存，sysinfo crate）
│   │   ├── usage.rs            # AI 用量同步（opencode.db）/汇总/详情/排行
│   │   └── repo/               # 数据访问层：resource, note, todo, sticky, snippet, tag
│   ├── capabilities/default.json  # Tauri 权限声明（含 start-dragging/global-shortcut/dialog）
│   └── tauri.conf.json         # 窗口配置（无边框、1400x900）
├── docs/
│   └── design-spec.md          # 设计基线 v1.0（Notion × macOS Bento，与 DESIGN.md 并存）
├── DESIGN.md                   # 当前设计系统（唯一实现基线，与 style.css 对齐）
└── package.json
```

## 入口点

| 关注点 | 文件 |
|--------|------|
| 前端启动 | `src/main.ts` → `src/App.vue`（壳） → `src/index/index.vue`（首页） |
| 状态管理 | `src/stores/workbench.ts` → `useStore()` |
| 后端启动 | `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` → `run()` |
| 路由 | 无 Vue Router，侧栏切换 activeView 渲染对应视图/面板 |

## 设计规范

**实现基线见 `DESIGN.md`（当前设计系统）与 `docs/design-spec.md`（原始 v1.0 基线）。** 速览：

- **设计令牌**：全部定义在 `src/style.css`（CSS 变量，亮色 `:root` + 暗色 `[data-theme="dark"]` 覆盖），组件一律引用变量，禁止硬编码色值
- **主色**：品牌靛紫 `#5B5BF5`（`--brand-500`，暗色 `#8b8bff`），仅用于强调态（选中、激活、焦点环）
- **玻璃卡片**：半透明底 `--bg-card` + `--shadow-card` + `--radius-lg`(12px)，内部控件 8px
- **强调色**：`--c-yellow/red/blue/green/pink/orange/purple/gray` 8 色 + ink/soft 变体，资源图标按名称 hash 取色（`useResourceIcon`）
- **字体层级**：Section title 16/650、Body 13、Caption 12、Micro 11（见 DESIGN.md §3）
- **布局**：`app-body` 两栏 Grid（220px 侧栏 / 56px 收起态）；工作台为三列 Bento 网格；速记/速达/用量为独立视图
- **交互动效**：hover 轻微上浮 + shadow、按钮按下 scale(0.96)、弹窗 0.2s 缩放渐入
- **弹窗遮罩**：统一 `--scrim` 令牌（暗色下保证对比度），`useFocusTrap` 焦点陷阱

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 46 个命令

## 数据模型（SQLite）

| 表 | 说明 |
|----|------|
| `resources` | 速达资源（app/web/file，category/icon/args/sort_order/last_launched_at） |
| `notes` | 速记笔记（title/content） |
| `tags` / `note_tags` | 笔记标签（多对多） |
| `todos` | 待办（done/priority/completed_at） |
| `stickies` | 便签（slot 1/2） |
| `snippets` | 提示词（is_pinned/copy_count/last_copied_at） |
| `ai_usage` | AI 用量明细（session_id/provider/model/tokens*/cost/time_created/source） |

> 旧版 `groups`/`files` 表已并入 `resources`（Speed-to-launch 合一）；索引含 `idx_notes_updated`、`idx_todos_created`、`idx_ai_usage_time`、`idx_resources_category` 等。

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 侧栏 `navigation` 数组 + `activeView` 切换，工作台为组合式面板网格
3. **App.vue 纯壳：** 仅 `import Index from './index/index.vue'` 并渲染，零业务逻辑；所有首页逻辑在 `src/index/index.vue`
4. **无 NaiveUI：** 全部 UI 自绘，样式基于 `style.css` 设计令牌（Bento 玻璃风格 + 暗色 `[data-theme="dark"]`）
5. **图标用 lucide-vue-next：** 组件内 `import { Xxx } from 'lucide-vue-next'`，按需 `:size`/`:stroke-width`（1.8~2.2）微调，颜色继承 currentColor；仅 TitleBar 品牌 Logo 保留手写 SVG
6. **窗口拖动：** TitleBar 用 `getCurrentWindow().startDragging()` + mousedown 监听（非 `data-tauri-drag-region` 属性）
7. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs` on_window_event + `api.prevent_close()`）
8. **窗口状态持久化：** 尺寸/位置/置顶在关闭时由 Rust 端保存到 JSON，启动时恢复；最大化图标切换用 `isMaximized()` + `onResized` 监听
9. **笔记/便签自动保存：** 600ms 防抖（NoteEditor.vue / StickyCard.vue）
10. **搜索防抖：** 300ms（GlobalSearch.vue）
11. **全局快捷键：** 默认 Ctrl+Shift+Space 切换窗口显隐（`shortcut.rs` + lib.rs）；可在设置中录制，失焦/回车自动保存，录制中失焦取消并还原（SettingsDialog.vue）
12. **轻提示：** index.vue `provide('showToast')`，子组件 `inject` 使用
13. **只读 props：** store.state 为 readonly 深度代理，组件 props 用 `readonly Note[]` 等类型
14. **拖拽导入：** 拖入 exe/lnk 到窗口 → `onDragDropEvent`（Suda.vue）→ `parse_dropped_path` 命令（.lnk 经 PowerShell COM 解析目标 + System.Drawing 提取图标存 `app_data_dir/icons/`）→ 自动预填资源弹窗；图标经 `convertFileSrc`（assetProtocol 已启用，scope `$APPDATA/**`）渲染，提取失败回退名称 hash 首字母
15. **PowerShell 调用约定：** 一律用**环境变量传参**（`Command::env`）而非 `$args`——实测 `-Command` 模式下 `$args` 不可靠；输出前设 `[Console]::OutputEncoding=UTF8` 防中文乱码
16. **文件选择：** 已集成 tauri-plugin-dialog（`dialog:allow-open` 权限）；SudaFormDialog 路径/图标输入框右侧有选择按钮，选 exe/lnk 自动解析名称与图标，选图标文件经 `import_icon_file` 存入 icons 目录
17. **AI 用量：** `usage.rs` 从 opencode 数据库按 message 粒度同步到 `ai_usage` 表（游标 `usage_sync_cursor` 持久化在 config），避免长会话跨天归因错误；`sync_ai_usage`/`get_usage_summary`/`get_usage_detail` 三命令；汇总含今日/7日/月/累计与今日调用次数
18. **系统监视：** `sysmon.rs` 用 sysinfo crate 返回 CPU/内存，2s 轮询（SysMonitorCard.vue）

## 命令速查

```bash
npm run dev           # Vite 开发服务器（浏览器预览 http://localhost:1420）
npm run tauri:dev     # Tauri 开发窗口（需 Rust 工具链）
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建桌面应用（产物在 src-tauri/target/release/bundle/）
```

## 注意事项

- **decorations: false**：窗口无边框，标题栏/窗口控制全部自定义（TitleBar.vue）
- **startDragging 权限**：`core:window:allow-start-dragging` 已在 capabilities 声明
- **数据目录：** `app.path().app_data_dir()/` = `%APPDATA%\x-hub`（identifier 为 `x-hub`；旧标识 `com.workbench.desktop` 的数据在启动时自动迁移一次，且 `lib.rs::fix_icon_paths` 会把数据库中的旧图标路径批量替换为新目录）
- **日志：** `tauri-plugin-log` 文件日志 → `%APPDATA%\x-hub\logs\x-hub.log`（Info 级别），同时输出 Stdout + Webview；所有命令入口记录成功/失败，数据查询类用 `log::debug!` 防噪音；启动程序遇 os error 740（需要管理员权限）自动经 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权
- **配置位置：** 与数据库同目录的 JSON 文件（含 theme/window/global_shortcut/usage_db_path/usage_sync_cursor）
- **SQLite：** 使用 `rusqlite` crate（bundled）
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
- **测试工具函数：** `db.rs::init_in_memory` 仅 `#[cfg(test)]` 使用；`repo/*.rs` 含单元测试（snippet/usage 有端到端验证）
- **dialog 插件：** 前端 `@tauri-apps/plugin-dialog` 的 `open()` 需 `dialog:allow-open` 权限（已声明）；浏览器预览环境需 `isTauri()` 守卫

## 待实现

- 暂无排期需求；可探索方向：拖拽排序动效打磨、键盘导航、前端单元测试、打包发布全流程验证（tauri:build）
