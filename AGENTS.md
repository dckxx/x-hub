# x-hub (个人效率工作台)

**生成:** 2026-08-02 | **分支:** master

## 概述

基于 Tauri 2 + Vue 3 + Tailwind CSS 4 的本地桌面效率工作台（Bento 风格 Dashboard），提供快捷启动、速记笔记、全局搜索等功能。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6。

## 结构

```
x-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：引入 style.css，createApp(App).mount('#app')
│   ├── App.vue                 # 应用根壳：仅引入 src/index/index.vue，无业务逻辑
│   ├── index/index.vue         # 首页：三栏布局 + 主题切换 + 全局搜索/设置协调（所有首页逻辑）
│   ├── style.css               # 设计令牌（亮/暗色 CSS 变量）+ Tailwind + 通用组件样式
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用 + 类型定义
│   ├── stores/workbench.ts     # 响应式状态管理（reactive + readonly，无 Pinia）
│   └── components/
│       ├── TitleBar.vue        # 自制标题栏（startDragging 拖动 + 窗口控制 + 搜索/设置入口）
│       ├── CalendarCard.vue    # 日历卡片（年月切换 + 今按钮 + 今日高亮）
│       ├── QuickLaunch.vue     # 快捷启动：分组 tabs + 资源卡片网格 + 右键菜单 + 拖拽导入
│       ├── NoteList.vue        # 笔记条目列表（标题/相对时间/摘要）
│       ├── NoteEditor.vue      # 笔记编辑弹窗（600ms 防抖自动保存）
│       ├── FileManager.vue     # 文件管理：分类 tabs（选中黑底）+ 文件链接网格
│       ├── FileFormDialog.vue  # 新增/编辑文件链接弹窗（文件夹/文件 + 自动分类）
│       ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（资源/笔记/文件 + 300ms 防抖）
│       ├── SettingsDialog.vue  # 设置弹窗（亮/暗主题、窗口置顶）
│       ├── ResourceFormDialog.vue  # 新增/编辑资源弹窗（app/web + 文件选择）
│       ├── GroupFormDialog.vue     # 新建/重命名分组弹窗
│       ├── ContextMenu.vue     # 通用右键菜单
│       └── AppDock.vue         # 底部 Dock（最近使用 app 前 8 个，布局内，无则自动收起）
│   └── utils/categories.ts     # 文件分类定义与自动识别
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态/命令注册
│   │   ├── commands.rs         # 17 个 Tauri 命令处理函数
│   │   ├── models.rs           # Group/Resource/Note/FileEntry/SearchResult 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移（init_in_memory 仅测试用）
│   │   ├── config.rs           # JSON 配置文件读写（AppConfig/WindowState）
│   │   ├── process.rs          # 外部进程启动/URL 打开/本地路径打开（app/web/文件链接）
│   │   ├── shortcut.rs         # 全局快捷键注册（Ctrl+Shift+Space）
│   │   ├── tray.rs             # 系统托盘（显示/隐藏/退出菜单）
│   │   └── repo/               # 数据访问层：group.rs, resource.rs, note.rs, file.rs
│   ├── capabilities/default.json  # Tauri 权限声明（含 allow-start-dragging）
│   └── tauri.conf.json         # 窗口配置（无边框、1100x760）
├── docs/
│   └── design-spec.md          # 设计规范（Bento 风格基线，见「设计规范」章节）
└── package.json
```

## 入口点

| 关注点 | 文件 |
|--------|------|
| 前端启动 | `src/main.ts` → `src/App.vue`（壳） → `src/index/index.vue`（首页） |
| 状态管理 | `src/stores/workbench.ts` → `useStore()` |
| 后端启动 | `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` → `run()` |
| 路由 | 无 Vue Router，单页 Dashboard 三栏布局 |

## 设计规范

**完整基线见 `docs/design-spec.md`（Notion × macOS Bento 风格），实现 UI 前必读。** 速览：

- **设计令牌**：全部定义在 `src/style.css`（CSS 变量，亮色 `:root` + 暗色 `[data-theme="dark"]` 覆盖），组件一律引用变量，禁止硬编码色值
- **主色**：品牌靛紫 `#5B5BF5`（`--brand-500`），仅用于强调态（选中、激活、今日高亮）
- **卡片**：白底 `--bg-card` + `--radius-lg`(16px) + `--shadow-card`，几乎无边框
- **强调色**：`--c-yellow/red/blue/green/pink/orange/purple/gray` 8 色 + soft 变体（图标底），资源图标按名称 hash 取色
- **字体层级**：H1 20/600、H2 16/600、Body 13、Caption 12（详见 spec §3.2）
- **布局**：三栏 Grid `360px 240px 1fr`（左栏：日历卡片 + 快捷启动；中栏：笔记列表；右栏：文件管理），笔记编辑为弹窗；Dock 在布局底部（最近使用 app 前 8 个，无则自动收起）
- **交互动效**：hover 上浮 2px + shadow、按钮按下 scale(0.96)、弹窗 0.2s 缩放渐入

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 17 个命令

## 数据模型

| 模型 | 前端类型 | Rust 结构体 | 存储 |
|------|----------|-------------|------|
| Group | `src/api/tauri.ts` | `models.rs` | SQLite `groups` 表 |
| Resource | `src/api/tauri.ts` | `models.rs` | SQLite `resources` 表 |
| Note | `src/api/tauri.ts` | `models.rs` | SQLite `notes` 表 |
| FileEntry | `src/api/tauri.ts` | `models.rs` | SQLite `files` 表（仅存链接，不复制源文件） |
| AppConfig | `src/api/tauri.ts` | `config.rs` | JSON 文件（app_data_dir） |

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 单页三栏布局（360px 快捷启动 / 240px 笔记列表 / 1fr 编辑器），index/index.vue 内协调
3. **App.vue 纯壳：** 仅 `import Index from './index/index.vue'` 并渲染，零业务逻辑；所有首页逻辑在 `src/index/index.vue`
4. **无 NaiveUI：** 全部 UI 自绘，样式基于 `style.css` 设计令牌（Bento 风格 + 暗色 `[data-theme="dark"]`）
5. **图标用 lucide-vue-next：** 组件内 `import { Xxx } from 'lucide-vue-next'`，按需 `:size`/`:stroke-width`（1.8~2.2）微调，颜色继承 currentColor；全项目仅 TitleBar 品牌 Logo 保留手写 SVG
6. **窗口拖动：** TitleBar 用 `getCurrentWindow().startDragging()` + mousedown 监听（非 `data-tauri-drag-region` 属性）
7. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs` on_window_event + `api.prevent_close()`）
8. **窗口状态持久化：** 尺寸/位置在关闭时由 Rust 端保存到 JSON，启动时恢复（`lib.rs`）；最大化图标切换用 `isMaximized()` + `onResized` 监听（TitleBar.vue）
9. **笔记自动保存：** 600ms 防抖（`NoteEditor.vue`）
10. **搜索防抖：** 300ms（`GlobalSearch.vue`）
11. **全局快捷键：** Ctrl+Shift+Space 切换窗口显隐（`shortcut.rs` + `lib.rs` app.listen("global-shortcut-toggle")）；Ctrl+K 唤起全局搜索（index.vue）
12. **轻提示：** index.vue `provide('showToast')`，子组件 `inject` 使用
13. **只读 props：** store.state 为 readonly 深度代理，组件 props 用 `readonly Note[]` 等类型
14. **拖拽导入应用：** 拖入 exe/.lnk 到窗口 → `onDragDropEvent`（QuickLaunch.vue）→ `parse_dropped_path` 命令（.lnk 经 PowerShell COM 解析目标 + System.Drawing 提取图标存 `app_data_dir/icons/`）→ 自动预填资源弹窗；图标经 `convertFileSrc`（assetProtocol 已启用，scope `$APPDATA/**`）渲染，提取失败回退名称 hash 首字母
15. **PowerShell 调用约定：** 一律用**环境变量传参**（`Command::env`）而非 `$args`——实测 `-Command` 模式下 `$args` 不可靠；输出前设 `[Console]::OutputEncoding=UTF8` 防中文乱码
16. **文件选择：** 已集成 tauri-plugin-dialog（`dialog:allow-open` 权限）；ResourceFormDialog 的路径/图标输入框右侧有选择按钮（FolderOpen / ImagePlus），选 exe/lnk 后自动解析名称与图标，选图标文件经 `import_icon_file` 存入 icons 目录

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
- **日志：** `tauri-plugin-log` 文件日志 → `%APPDATA%\x-hub\logs\x-hub.log`（Folder target 指定目录，Info 级别），同时输出 Stdout + Webview；所有命令入口记录成功/失败（`log::info!`/`log::error!`），数据查询类用 `log::debug!` 防噪音；启动程序遇 os error 740（需要管理员权限）自动经 PowerShell `Start-Process -Verb RunAs` 触发 UAC 提权
- **配置位置：** 与数据库同目录的 JSON 文件
- **SQLite：** 使用 `rusqlite` crate（bundled）
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
- **测试工具函数：** `db.rs::init_in_memory`、`repo/resource.rs::list_by_group` 仅 `#[cfg(test)]` 使用
- **dialog 插件：** 前端 `@tauri-apps/plugin-dialog` 的 `open()` 需 `dialog:allow-open` 权限（已声明）；浏览器预览环境需 `isTauri()` 守卫

## 待实现

- 暂无排期需求；可探索方向：拖拽排序的动效打磨、键盘导航（spec §5）、前端单元测试、打包发布全流程验证（tauri:build）
