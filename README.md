<div align="center">

# ⚡ x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>

## ✨ 功能特性

| 模块 | 能力 |
| --- | --- |
| 🚀 **快捷启动** | 本地程序 / 网页书签分组管理；拖拽 exe/lnk 导入并**自动提取程序图标**；点击一键启动；分组与资源**拖拽排序**；右键菜单操作 |
| 📝 **速记笔记** | 纯文本 + **Markdown 编辑/预览**；600ms 防抖自动保存；**标签管理**与按标签筛选；相对时间/摘要列表 |
| 📁 **文件管理** | 文件夹与任意文件的**快捷链接**（源文件不移动）；自动分类（文档/图片/视频/音频/压缩包）；拖拽导入；点击系统打开 |
| 🔍 **全局搜索** | `Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、文件**，点击直达 |
| 🗂️ **日历** | 月份切换、「今」按钮一键回到今天，今日高亮 |
| ⚙️ **系统设置** | 亮/暗主题（持久化）、窗口置顶、**数据备份与恢复** |
| 🖥️ **窗口能力** | 无边框 + 自制标题栏（拖动/最大化/还原/关闭至托盘）；系统托盘常驻；`Ctrl+Shift+Space` 全局唤起；记忆窗口位置尺寸；底部 Dock 最近使用 |

## ⌨️ 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl + K` | 唤起全局搜索 |
| `Ctrl + Shift + Space` | 显示 / 隐藏主窗口 |
| `Esc` | 关闭弹窗 |

## 🛠️ 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Tailwind CSS 4 + Vite 8 |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式） |
| 状态 | `reactive()` + `readonly()` 自定义 store（无 Pinia） |
| 样式 | 设计令牌 CSS 变量（Bento 风格，见 `docs/design-spec.md`），亮/暗主题 |

## 🚀 快速开始

### 环境要求

- Node.js 18+
- Rust 1.77.2+（[rustup](https://rustup.rs/)）
- Windows：WebView2（Win10/11 自带）

### 安装与运行

```bash
npm install

npm run dev           # Vite 浏览器预览 (http://localhost:1420)
npm run tauri:dev     # Tauri 桌面开发窗口
npm run build         # vue-tsc 类型检查 + vite build
npm run tauri:build   # 构建桌面安装包（产物在 src-tauri/target/release/bundle/）
```

## 📂 目录结构

```
src/
├── main.ts / App.vue        # 入口与纯壳（App.vue 仅引入 index/index.vue）
├── index/index.vue          # 首页：三栏布局 + 主题 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装
├── stores/workbench.ts      # 响应式状态管理
├── components/              # 功能组件（快捷启动/笔记/文件/搜索/设置…）
└── utils/categories.ts      # 文件分类识别

src-tauri/
└── src/
    ├── lib.rs               # 应用构建：数据库/托盘/快捷键/窗口状态/数据迁移
    ├── commands.rs          # 27 个 Tauri 命令
    ├── models.rs / db.rs    # 模型与 SQLite 迁移
    ├── config.rs            # 配置持久化
    ├── process.rs           # 程序启动 / URL 打开 / 提权（UAC）
    ├── shortcut.rs / tray.rs
    └── repo/                # 数据访问层（group/resource/note/file/tag）
```

## 🔒 数据与隐私

- **数据库**：`%APPDATA%\x-hub\app.db`（SQLite，分组/资源/笔记/文件/标签）
- **图标**：`%APPDATA%\x-hub\icons\`（拖拽导入时自动提取的程序图标）
- **日志**：`%APPDATA%\x-hub\logs\x-hub.log`（文件日志，便于排查）
- **备份**：设置内一键备份/恢复（数据库 + 图标整体复制）
- 所有数据仅存本地，**不会上传任何云端**

## 📄 License

MIT
