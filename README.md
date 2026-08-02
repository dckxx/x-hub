# x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript + Tailwind CSS 4** 的桌面效率工具。所有数据默认本地存储（SQLite），不上传云端。

## 功能

- **快捷启动**：录入本地程序 / 网页书签，分组管理，右键菜单操作，点击一键启动，底部 Dock 常用应用
- **速记笔记**：纯文本笔记，条目列表 + 编辑区，600ms 防抖自动保存，标题/内容全局检索
- **全局搜索**：`Ctrl+K` 唤起，同时检索快捷资源与笔记
- **系统设置**：亮色 / 暗色主题切换（持久化）、窗口置顶
- **窗口能力**：无边框 + 自制标题栏（拖动、最小化、最大化、关闭至托盘）、系统托盘常驻、`Ctrl+Shift+Space` 全局快捷键唤出、窗口位置/尺寸记忆

## 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3 (`<script setup>`) + TypeScript + Tailwind CSS 4 + Vite |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 后端 | Rust (Tauri 2) + rusqlite (SQLite) |
| 状态 | `reactive()` + `readonly()` 自定义 store（无 Pinia） |
| 样式 | 设计令牌 CSS 变量（Bento 风格，见 `docs/design-spec.md`），支持暗色主题 |

## 开发

```bash
npm install
npm run dev           # Vite 浏览器预览 (http://localhost:1420)
npm run tauri:dev     # Tauri 桌面窗口（需 Rust 工具链）
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建桌面应用（产物在 src-tauri/target/release/bundle/）
```

## 结构

```
src/
├── main.ts                 # 入口（引入 style.css 设计令牌）
├── App.vue                 # 应用壳：三栏布局 + 主题 + 全局搜索/设置协调
├── style.css               # 设计令牌（亮/暗色）+ 通用组件样式
├── api/tauri.ts            # Tauri invoke 封装 + 类型定义
├── stores/workbench.ts     # 响应式状态管理
└── components/
    ├── TitleBar.vue        # 自制标题栏（拖动/窗口控制/搜索/设置入口）
    ├── QuickLaunch.vue     # 快捷启动：分组 + 资源网格
    ├── NoteList.vue        # 笔记条目列表
    ├── NoteEditor.vue      # 笔记编辑器（防抖自动保存）
    ├── GlobalSearch.vue    # Ctrl+K 全局搜索
    ├── SettingsDialog.vue  # 设置弹窗（主题/置顶）
    ├── ResourceFormDialog.vue / GroupFormDialog.vue  # 资源/分组表单
    ├── ContextMenu.vue     # 通用右键菜单
    └── AppDock.vue         # 底部 Dock 快捷启动

src-tauri/
└── src/
    ├── lib.rs              # Tauri Builder：数据库/托盘/快捷键/窗口状态
    ├── commands.rs         # Tauri 命令
    ├── models.rs           # Group / Resource / Note 结构体
    ├── db.rs               # SQLite 初始化与迁移
    ├── config.rs           # JSON 配置读写
    ├── process.rs          # 外部进程启动 / URL 打开
    ├── shortcut.rs         # 全局快捷键
    ├── tray.rs             # 系统托盘
    └── repo/               # 数据访问层：group.rs / resource.rs / note.rs
```

## 数据

- **数据库**：`app.path().app_data_dir()/app.db`（SQLite）
- **配置**：同目录 JSON 文件（主题、窗口状态、置顶）
- **权限**：新增前端 API 调用需在 `src-tauri/capabilities/default.json` 声明

## 待实现

- 快捷资源拖拽排序（后端 `reorder_*` 命令与 store 接口已就绪）
- 本地 exe 文件选择器（需引入 Tauri dialog 插件）
