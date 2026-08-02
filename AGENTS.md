# x-hub (个人效率工作台)

**生成:** 2026-08-02 | **提交:** b746c81 | **分支:** master

## 概述

基于 Tauri 2 + Vue 3 + NaiveUI 的桌面效率工具，提供快捷启动、速记笔记、全局搜索等功能。Rust 后端管理 SQLite 数据持久化，前端使用 Vite 8 + TypeScript 6 + Tailwind CSS 4。

## 结构

```
x-hub/
├── src/                        # 前端源码 (Vue 3 SPA)
│   ├── main.ts                 # 入口：createApp(App).mount('#app')
│   ├── App.vue                 # 根组件：主题/布局/Ctrl+K 全局搜索
│   ├── api/tauri.ts            # 所有 Tauri invoke 调用 + 类型定义
│   ├── stores/workbench.ts     # 响应式状态管理（无 Pinia，reactive + readonly）
│   ├── components/
│   │   ├── TitleBar.vue        # 自定义标题栏（含窗口控制按钮）
│   │   ├── SideNav.vue         # 左侧导航（快捷启动/笔记/设置）
│   │   ├── QuickLaunch.vue     # 快捷启动主页：分组 + 资源卡片网格
│   │   ├── NotesView.vue       # 速记笔记：左侧列表 + 右侧编辑器（自动保存 600ms 防抖）
│   │   ├── SettingsView.vue    # 主题切换、窗口置顶、全局快捷键测试
│   │   ├── GlobalSearch.vue    # Ctrl+K 全局搜索弹窗（300ms 防抖）
│   │   ├── ResourceForm.vue    # 新增/编辑资源弹窗（网页书签 / 本地程序）
│   │   ├── GroupForm.vue       # 新增/重命名分组弹窗
│   │   ├── Dashboard.vue       # 示例组件（未使用）
│   │   └── HelloWorld.vue      # 示例组件（未使用）
│   └── assets/                 # 静态资源
├── src-tauri/                  # Tauri 后端 (Rust)
│   ├── src/
│   │   ├── main.rs             # Windows 子系统入口 → app_lib::run()
│   │   ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/命令注册
│   │   ├── commands.rs         # 21 个 Tauri 命令处理函数
│   │   ├── models.rs           # Group/Resource/Note/SearchResult 结构体
│   │   ├── db.rs               # rusqlite 数据库初始化与迁移
│   │   ├── config.rs           # JSON 配置文件读写（AppConfig/WindowState）
│   │   ├── process.rs          # 外部进程启动（app 类型资源）
│   │   ├── shortcut.rs         # 全局快捷键注册（Ctrl+Shift+Space）
│   │   ├── tray.rs             # 系统托盘设置
│   │   └── repo/               # 数据访问层：group.rs, resource.rs, note.rs
│   ├── capabilities/default.json  # Tauri 权限声明
│   ├── tauri.conf.json         # 窗口配置（无边框、居中、1100x760）
│   └── Cargo.toml
├── public/                     # 公共资源（Tauri 图标）
├── vite.config.ts              # Vue/Tailwind/NaiveUI 自动导入
└── package.json
```

## 入口点

| 关注点 | 文件 |
|--------|------|
| 前端启动 | `src/main.ts` → `src/App.vue` |
| 状态管理 | `src/stores/workbench.ts` → `useStore()` |
| 后端启动 | `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` → `run()` |
| 路由 | 无 Vue Router，App.vue 内 `activeView` 状态切换 |

## 前后端通信

- **唯一通道：** `@tauri-apps/api/core` → `invoke<ReturnType>('command_name', args)`
- **类型安全：** 所有 invoke 调用封装在 `src/api/tauri.ts` 的 `tauriApi` 对象中，含完整 TypeScript 类型
- **环境守卫：** `isTauri()` 检查 `'__TAURI_INTERNALS__' in window`，确保浏览器预览环境不崩溃
- **命令注册：** `src-tauri/src/lib.rs` 的 `invoke_handler!` 宏列出全部 21 个命令

## 数据模型

| 模型 | 前端类型 | Rust 结构体 | 存储 |
|------|----------|-------------|------|
| Group | `src/api/tauri.ts` | `models.rs` | SQLite `groups` 表 |
| Resource | `src/api/tauri.ts` | `models.rs` | SQLite `resources` 表 |
| Note | `src/api/tauri.ts` | `models.rs` | SQLite `notes` 表 |
| AppConfig | `src/api/tauri.ts` | `config.rs` | JSON 文件（app_data_dir） |

## 关键约定

1. **无 Pinia：** 使用 `reactive()` + `readonly()` 自定义 store 模式
2. **无 Vue Router：** 视图切换通过 `activeView` ref 在 App.vue 内 KeepAlive
3. **NaiveUI 自动导入：** `unplugin-auto-import` + `unplugin-vue-components`，模板中无需手动 import
4. **窗口事件拦截：** 关闭按钮隐藏至托盘而非退出（`lib.rs:87-97`）
5. **窗口状态持久化：** 尺寸/位置保存到 JSON，启动时恢复（`lib.rs:31-45`）
6. **笔记自动保存：** 600ms 防抖（`NotesView.vue:50-65`）
7. **搜索防抖：** 300ms（`GlobalSearch.vue:43-53`）
8. **全局快捷键：** Ctrl+Shift+Space 切换窗口显隐（通过 Tauri `global-shortcut` 插件）

## 命令速查

```bash
npm run dev           # Vite 开发服务器（浏览器预览 http://localhost:1420）
npm run tauri:dev     # Tauri 开发窗口（需 Rust 工具链）
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建桌面应用（产物在 src-tauri/target/release/bundle/）
```

## 注意事项

- **decorations: false**：窗口无边框，所有标题栏/窗口控制需自定义（TitleBar.vue 已实现）
- **`data-tauri-drag-region`**：在 TitleBar.vue 上标记，使自定义区域可拖动窗口
- **数据库位置：** `app.path().app_data_dir()/app.db`（由 Tauri 管理路径）
- **配置位置：** 与数据库同目录的 JSON 文件
- **SQLite：** 使用 `rusqlite` crate，含 `r2d2` 连接池
- **Tauri 权限：** 新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明对应权限
