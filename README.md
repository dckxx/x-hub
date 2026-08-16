<div align="center">

# ⚡ x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![Version](https://img.shields.io/badge/version-0.1.14-blue)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>


## ✨ 功能特性

| 模块 | 能力 |
| --- | --- |
| 🕐 **工作台** | 时钟（含最近倒计时环形进度）+ 系统资源监视器（CPU/内存，2s 轮询）+ 便签（2 槽，600ms 防抖自动保存）+ **中上区块**（默认倒计时，可切换 Token 用量卡 / 速记概览 / 待办概览 / 速达数量）+ 提示词百宝箱 + 待办清单 + 最近使用通栏 |
| ⏳ **倒计时** | 时长/定时/每天/间隔**四种模式**（最多 6 个）；暂停/继续/浮窗/删除；**后台驱动**（Rust 1s 轮询，到点发系统通知，`once` 灰态 / `daily` / `interval` 自动顺延，休眠错过静默顺延）；可浮起为**透明圆形水罐浮窗**（水位水波动画，独立置顶小窗，位置持久化）；可选到点提示音（WebAudio 合成双音） |
| 🚀 **速达** | 应用 / 网页 / 文件**三类资源合一**管理；分组筛选（全部/常用/应用/网页/文件 + 文件二级分类）；拖拽 exe/lnk 导入并**自动提取程序图标**；**扫描已安装应用批量导入**；点击一键启动；右键菜单操作；删除可撤销 |
| 📝 **速记** | 纯文本 + **Markdown 编辑/预览**；600ms 防抖自动保存；**标签管理**与按标签筛选；相对时间/摘要列表 |
| 🔍 **全局搜索** | `Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、待办**，点击直达 |
| ✅ **待办清单** | 添加/完成/删除；优先级循环切换（普通/重要/紧急，10px 纯色圆点）；行内编辑；待办与已完成视图分离；支持全局搜索直达与高亮 |
| 📊 **AI 用量** | 从 opencode 数据库同步**用量明细**（input/cache/output/reasoning/cost）；今日/7日/月/累计汇总；按提供商与模型排行；今日调用次数；Token 用量卡显示今日总量 + 三指标 + 监听绿点；详情页双栏（趋势/排行 + 明细分页） |
| 💬 **提示词百宝箱** | 常用提示词片段管理；置顶 + 复制计数；卡片一键复制 |
| ⚙️ **系统设置** | **三轴主题**（模式 亮/暗/系统 × 10 色 + 10 渐变预设 × 强调色 8 预设/自定义）、**全局快捷键录入**（失焦/回车自动保存）、**中上区块切换**（倒计时/Token/概览卡）、**倒计时提示音开关**、**数据备份与恢复** |
| 🖥️ **窗口能力** | 无边框 + 透明自制标题栏（拖动/最大化/还原/置顶按钮/关闭至托盘）；系统托盘常驻；`Ctrl+Shift+Space` 全局唤起；记忆窗口位置尺寸；便签/倒计时独立浮窗 |

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
| 表单 | reka-ui 无头组件（DatePicker / TimeField / NumberField，样式自绘） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式）+ sysinfo（系统资源）+ tauri-plugin-notification（系统通知） |
| 状态 | `reactive()` + `readonly()` 自定义 store（无 Pinia） |
| 样式 | 设计令牌 CSS 变量（Bento 风格，见 `DESIGN.md`），三轴主题（模式 × 预设 × 强调色） |

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
├── main.ts / App.vue        # 入口与窗口壳（App.vue 按窗口 label 路由：主界面 / 便签浮窗 / 倒计时浮窗）
├── index/index.vue          # 首页：侧栏导航（工作台/速记/速达/用量）+ 三轴主题 + 中上区块切换 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装（23 类模型 + 64 个命令）
├── stores/workbench.ts      # 响应式状态管理（工作台/用量/系统信息/提示词/倒计时）
├── composables/             # 组合式函数（useResourceIcon / useFocusTrap / useTheme）
├── utils/                   # 文件分类 / 时间 / 错误上报 / chime 提示音
└── components/              # 功能组件（工作台卡片/速达/速记/搜索/待办/设置/用量/倒计时…）

src-tauri/
└── src/
    ├── lib.rs               # 应用构建：数据库/托盘/快捷键/窗口状态/数据迁移/64 命令注册
    ├── commands.rs          # 64 个 Tauri 命令
    ├── models.rs / db.rs    # 模型与 SQLite 迁移
    ├── config.rs            # 配置持久化（主题/窗口/全局快捷键/用量游标/提示音开关）
    ├── process.rs           # 程序启动 / URL 打开 / 提权（UAC）
    ├── shortcut.rs / tray.rs
    ├── sysmon.rs            # 系统资源监视（CPU/内存）
    ├── usage.rs             # opencode 用量同步与汇总
    ├── notify.rs            # 系统通知封装（tauri-plugin-notification）
    ├── countdown_ticker.rs  # 倒计时后台驱动线程（1s 轮询 → 通知 + 事件 + 顺延）
    ├── countdown_window.rs  # 倒计时圆形浮窗（创建/销毁/位置持久化）
    └── repo/                # 数据访问层（resource/note/todo/sticky/snippet/tag/countdown）
```

## 🔒 数据与隐私

- **数据库**：`%APPDATA%\x-hub\app.db`（SQLite，resources/notes/todos/stickies/snippets/tags/ai_usage/countdowns）
- **图标**：`%APPDATA%\x-hub\icons\`（拖拽导入/扫描安装应用时自动提取的程序图标）
- **日志**：`%APPDATA%\x-hub\logs\x-hub.log`（文件日志，便于排查）
- **备份**：设置内一键备份/恢复（数据库 + 图标整体复制）
- **AI 用量**：仅本地读取 opencode 生成的数据库，统计结果存本地，**不上传任何云端**

## 配图
<img width="1418" height="911" alt="首页-工作台" src="https://github.com/user-attachments/assets/7e3279fc-468c-4cf5-979d-d391e6ba3927" />
<img width="1418" height="911" alt="速记" src="https://github.com/user-attachments/assets/baf42146-2b24-4be4-b87f-0db405988d67" />
<img width="1418" height="911" alt="速达" src="https://github.com/user-attachments/assets/406510a0-9673-4d42-8767-2818cd57f66b" />
<img width="1418" height="911" alt="token统计" src="https://github.com/user-attachments/assets/c872d175-8392-4d13-a1c2-f29e5728d8d3" />
<img width="1418" height="911" alt="设置" src="https://github.com/user-attachments/assets/361635cb-8dbb-4528-b604-91f36ce768df" />
<img width="1920" height="1030" alt="浮窗" src="https://github.com/user-attachments/assets/b8751fb5-0a2f-459b-bc44-64e8247386d5" />


## 📄 License

MIT
