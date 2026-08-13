<div align="center">

# ⚡ x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![Version](https://img.shields.io/badge/version-0.1.9-blue)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>

## ✨ 功能特性

| 模块 | 能力 |
| --- | --- |
| 🕐 **工作台** | 时钟 + 系统资源监视器（CPU/内存，2s 轮询）+ 便签（2 槽，600ms 防抖自动保存）+ Token 用量卡（三指标 + 近 7 日迷你趋势）+ 提示词百宝箱 + 待办清单 + 最近使用通栏 |
| 🚀 **速达** | 应用 / 网页 / 文件**三类资源合一**管理；分组筛选（全部/常用/应用/网页/文件 + 文件二级分类）；拖拽 exe/lnk 导入并**自动提取程序图标**；点击一键启动；拖拽排序；右键菜单操作；删除可撤销 |
| 📝 **速记** | 纯文本 + **Markdown 编辑/预览**；600ms 防抖自动保存；**标签管理**与按标签筛选；相对时间/摘要列表 |
| 🔍 **全局搜索** | `Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、待办**，点击直达 |
| ✅ **待办清单** | 添加/完成/删除；优先级循环切换（普通/重要/紧急，10px 纯色圆点）；行内编辑；待办与已完成视图分离；支持全局搜索直达与高亮 |
| 📊 **AI 用量** | 从 opencode 数据库同步**用量明细**（input/cache/output/reasoning/cost）；今日/7日/月/累计汇总；按提供商与模型排行；今日调用次数；详情页双栏（趋势/排行 + 明细分页） |
| 💬 **提示词百宝箱** | 常用提示词片段管理；置顶 + 复制计数；卡片一键复制 |
| ⚙️ **系统设置** | 亮/暗主题（持久化）、窗口置顶、**全局快捷键录入**（失焦/回车自动保存）、**数据备份与恢复** |
| 🖥️ **窗口能力** | 无边框 + 透明自制标题栏（拖动/最大化/还原/关闭至托盘）；系统托盘常驻；`Ctrl+Shift+Space` 全局唤起；记忆窗口位置尺寸 |

## ⌨️ 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl + K` | 唤起全局搜索 |
| `Ctrl + Shift + Space` | 显示 / 隐藏主窗口（可在设置中自定义） |
| `Esc` | 关闭弹窗 |

## 🛠️ 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Tailwind CSS 4 + Vite 8 |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式）+ sysinfo（系统资源） |
| 状态 | `reactive()` + `readonly()` 自定义 store（无 Pinia） |
| 样式 | 设计令牌 CSS 变量（Bento 风格，见 `DESIGN.md`），亮/暗主题 |

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
├── index/index.vue          # 首页：侧栏导航（工作台/速记/速达/用量）+ 主题 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装（19 类模型 + 46 个命令）
├── stores/workbench.ts      # 响应式状态管理（工作台/用量/系统信息/提示词）
├── composables/             # 组合式函数（useResourceIcon 资源图标渲染）
├── utils/categories.ts      # 文件分类识别
└── components/              # 功能组件（工作台卡片/速达/速记/搜索/待办/设置/用量…）

src-tauri/
└── src/
    ├── lib.rs               # 应用构建：数据库/托盘/快捷键/窗口状态/数据迁移
    ├── commands.rs          # 46 个 Tauri 命令
    ├── models.rs / db.rs    # 模型与 SQLite 迁移
    ├── config.rs            # 配置持久化（主题/窗口/全局快捷键/用量游标）
    ├── process.rs           # 程序启动 / URL 打开 / 提权（UAC）
    ├── shortcut.rs / tray.rs
    ├── sysmon.rs            # 系统资源监视（CPU/内存）
    ├── usage.rs             # opencode 用量同步与汇总
    └── repo/                # 数据访问层（resource/note/todo/sticky/snippet/tag）
```

## 🔒 数据与隐私

- **数据库**：`%APPDATA%\x-hub\app.db`（SQLite，resources/notes/todos/stickies/snippets/tags/ai_usage）
- **图标**：`%APPDATA%\x-hub\icons\`（拖拽导入时自动提取的程序图标）
- **日志**：`%APPDATA%\x-hub\logs\x-hub.log`（文件日志，便于排查）
- **备份**：设置内一键备份/恢复（数据库 + 图标整体复制）
- **AI 用量**：仅本地读取 opencode 生成的数据库，统计结果存本地，**不上传任何云端**

## 📄 License

MIT
