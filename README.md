<div align="center">

# ⚡ x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![Version](https://img.shields.io/badge/version-0.4.3-blue)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>


## ✨ 功能特性

### 🕐 工作台
时钟（含**实时天气** Open-Meteo 温度/体感/湿度/风速 + 城市/IP 定位）、系统资源监视器（CPU/内存，2s 轮询）、便签（2 槽，600ms 防抖自动保存）、提示词百宝箱、待办清单、最近使用通栏。工作台为**自由编排 Bento 网格**：9 种部件（时钟/便签×2/速记概览/待办概览/速达数量/倒计时/提示词/待办）可在布局编辑器中增删、拖拽、调宽高并记忆。时钟语录接入**在线名言（hitokoto）**，离线自动回退本地语料，点击换一句。

### ⏳ 倒计时
时长/定时/每天/间隔**四种模式**（最多 6 个）；暂停/继续/浮窗/删除；**后台驱动**（Rust 1s 轮询，到点发系统通知，`once` 灰态 / `daily` / `interval` 自动顺延，休眠错过静默顺延）；可浮起为**透明圆形水罐浮窗**（水位水波动画，独立置顶小窗，位置持久化）；可选到点提示音（WebAudio 合成双音）。

### 🚀 速达
应用 / 网页 / 文件**三类资源合一**管理；分组筛选（全部/常用/应用/网页/文件 + 文件二级分类）；拖拽 exe/lnk 导入并**自动提取程序图标**；**扫描已安装应用批量导入**；点击一键启动；右键菜单操作；删除可撤销。

### 📝 速记
纯文本 + **Markdown 编辑/预览**；600ms 防抖自动保存；**标签管理**与按标签筛选；相对时间/摘要列表。

### 🔍 全局搜索
`Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、待办**，点击直达。

### ✅ 待办清单
添加/完成/删除；优先级循环切换（普通/重要/紧急，10px 纯色圆点）；行内编辑；待办与已完成视图分离；支持全局搜索直达与高亮。

### 📋 剪贴板历史
快捷键 `Ctrl+\`` 全局唤起浮层；记录**文本 / 图片 / 文件**三类内容——复制图片/截图自动落盘缩略图、复制文件记路径；支持粘贴回剪贴板、图片预览与「保存图片」、相同内容自动去重；图片/文件记录开关可配。

### 🤖 AI 对话
**OpenAI 兼容流式对话**（DeepSeek/OpenAI/Ollama/one-api 等，SSE 打字机效果）；**多会话管理**（新建/切换/删除）；**面板四方位停靠**（左/右/上/下，可拖拽调整尺寸并记忆位置）；**Markdown 渲染**回复（代码块/表格/列表等）；供应商模型管理——**测试连通性** + **拉取模型批量勾选添加** + 同供应商模型共享 API Key；API Key 存**系统钥匙串**，界面脱敏（👁 查看 / 📋 复制）；面板透明度可调（50%–100%）；`Ctrl+Shift+K` 唤起。

### 💬 提示词百宝箱
常用提示词片段管理；置顶 + 复制计数；卡片一键复制。

### 🧩 扩展系统
**扩展中心**本地清单展示已安装扩展，支持从 **GitHub 仓库地址下载 zip** 解包安装、卸载、检查更新；**manifest 注册表**解析权限声明与版本；扩展支持 **module 卡片**（嵌入主界面）/ **view 页面** / **window / drawer 浮窗** 四种形态；宿主注入 **桥 API（window.xhub）** 与主程序交互（权限按 manifest 逐项授权）；**service 托管**内置运行时按需下载、自动降级；扩展可**固定到左侧栏**，点击即打开，并选择「视图 / 窗口 / 抽屉」打开方式。

### ⚙️ 系统设置
**三轴主题**（模式 亮/暗/系统 × 10 色 + 10 渐变预设 × 强调色 8 预设/自定义）、**全局快捷键录入**（失焦/回车自动保存）、**开机自启动**（Run 键方式，登录后静默驻留托盘）、**工作台布局编辑器**（9 种部件自由编排）、**联网**（总开关 / 城市设置 / 名言来源）、**倒计时提示音开关**、**AI 助手**（供应商/模型配置）、**AI 对话面板透明度**、**数据备份与恢复**、**数据存储路径**。

### 🖥️ 窗口能力
无边框 + 透明自制标题栏（拖动/最大化/还原/置顶按钮/关闭至托盘）；系统托盘常驻；`Ctrl+Shift+Space` 全局唤起；记忆窗口位置尺寸；便签/倒计时独立浮窗。

### 🔄 应用自动更新
**自研升级链路**：从 `releases/update.json` 升级清单拉取**新版本信息**（Ed25519 分离签名验签，内嵌公钥验签通过才信任）→ semver 版本比较 + **跳级保护**（`minimumUpgradable` 下限）→ **自动静默检查**（启动 5s 后 + 默认每 4 小时，可在 About 关闭）；发现新版本弹出**全局更新弹窗**（版本号 / 说明 / 体积 / 便携版标记），支持**跳过此版本**（记录到配置）/**立即更新**（流式下载 + 实时进度条 + sha256 完整性校验）/ 就绪后**立即重启**；重启时解包并两步 rename **自替换**，失败自动回滚、下次启动重试；About「检查更新」可手动触发。更新包分发在 R2 CDN，支持标准版 / 便携版分别取包。

### 💾 数据存储与便携
所有数据默认本地存储，支持三种形态灵活切换：**标准版**数据默认在 `%APPDATA%\x-hub`，可在设置中「更改数据存储路径」迁移到任意目录（迁移后重启生效）；**便携版**只需在 exe 同目录放一个空文件 `portable`，数据即固定跟随 `exe\data` 子目录，整个文件夹拷到 U 盘即可随身携带；**数据备份/恢复**打包为 `x-hub-backup-时间戳.zip` 单个压缩包，便于归档与迁移。

## ⌨️ 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl + K` | 唤起全局搜索 |
| `Ctrl + Shift + K` | 唤起 / 收起 AI 对话面板 |
| `Ctrl + Shift + Space` | 显示 / 隐藏主窗口（可在设置中自定义） |
| `Ctrl + \`` | 唤起剪贴板历史浮层（可在设置中自定义） |
| `Esc` | 关闭弹窗 |

## 🛠️ 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Tailwind CSS 4 + Vite 8 |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 表单 | reka-ui 无头组件（DatePicker / TimeField / NumberField，样式自绘） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式）+ sysinfo（系统资源）+ tauri-plugin-notification（系统通知）+ reqwest（OpenAI 兼容 SSE 流式）+ keyring（API Key 系统钥匙串） |
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
├── main.ts / App.vue        # 入口与窗口壳（App.vue 按窗口 label 路由：主界面 / 便签浮窗 / 倒计时浮窗 / 剪贴板浮层 / 扩展浮窗 / 提示词浮窗 / 待办浮窗）
├── index/index.vue          # 首页：侧栏导航（工作台/速记/速达/扩展）+ 三轴主题 + 工作台布局协调 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装（120+ 个命令）
├── stores/workbench.ts      # 响应式状态管理（工作台/系统信息/提示词/倒计时/AI 对话/扩展/自动更新）
├── composables/             # 组合式函数（useResourceIcon / useFocusTrap / useTheme）
├── utils/                   # 文件分类 / 时间 / 错误上报 / chime 提示音
└── components/              # 功能组件（工作台卡片/速达/速记/搜索/待办/设置/倒计时/AI 对话/扩展中心/更新弹窗…）

src-tauri/
└── src/
    ├── lib.rs               # 应用构建：数据库/托盘/快捷键/窗口状态/数据迁移/命令注册 + 定时检查更新
    ├── commands.rs          # Tauri 命令
    ├── models.rs / db.rs    # 模型与 SQLite 迁移
    ├── config.rs            # 配置持久化（主题/窗口/全局快捷键/提示音/AI 模型/更新源与自动更新开关）
    ├── process.rs           # 程序启动 / URL 打开 / 提权（UAC）
    ├── shortcut.rs / tray.rs
    ├── sysmon.rs            # 系统资源监视（CPU/内存）
    ├── notify.rs            # 系统通知封装（tauri-plugin-notification）
    ├── chat.rs              # OpenAI 兼容 SSE 流式对话客户端 + API Key 钥匙串存取
    ├── countdown_ticker.rs  # 倒计时后台驱动线程（1s 轮询 → 通知 + 事件 + 顺延）
    ├── countdown_window.rs  # 倒计时圆形浮窗（创建/销毁/位置持久化）
    ├── market.rs            # 扩展市场远端清单 + Ed25519 验签 + 下载/更新/卸载
    ├── updater.rs           # 应用自动更新（update.json 验签 + 下载校验 + 重启自替换）
    └── repo/                # 数据访问层（resource/note/todo/sticky/snippet/tag/countdown/chat）
```

## 🔒 数据与隐私

数据统一存放在「数据根」目录下（标准版默认为 `%APPDATA%\x-hub`，可经设置改到任意目录；便携版为 `exe\data`）：

- **数据库**：`数据根\app.db`（SQLite，resources/notes/todos/stickies/snippets/tags/countdowns/chat_sessions/chat_messages）
- **图标**：`数据根\icons\`（拖拽导入/扫描安装应用时自动提取的程序图标）
- **日志**：`数据根\logs\x-hub.log`（文件日志，便于排查）
- **剪贴板快照**：`数据根\clipboard\images\`（复制图片时落盘的缩略图；删除 / 清空 / 过期清理时联动删除）
- **备份**：设置内一键备份/恢复，打包为 `x-hub-backup-时间戳.zip` 压缩包（数据库 + 图标）
- **应用更新**：下载的更新包暂存在 `数据根\updates\`，自替换成功 / 失败回滚后自动清理
- **AI 对话**：会话与消息存本地 SQLite；API Key 存入**系统钥匙串**（keyring），界面脱敏展示，**不明文落盘、不上传**

## 配图
<img width="1418" height="911" alt="首页-工作台" src="https://github.com/user-attachments/assets/7e3279fc-468c-4cf5-979d-d391e6ba3927" />
<img width="1408" height="911" alt="1b6dfac7d89ccc73912a22463d8cb2f2" src="https://github.com/user-attachments/assets/0390f712-3fad-4ea5-b916-8340051acbbc" />
<img width="1418" height="911" alt="速记" src="https://github.com/user-attachments/assets/baf42146-2b24-4be4-b87f-0db405988d67" />
<img width="1418" height="911" alt="速达" src="https://github.com/user-attachments/assets/406510a0-9673-4d42-8767-2818cd57f66b" />
<img width="1418" height="911" alt="设置" src="https://github.com/user-attachments/assets/361635cb-8dbb-4528-b604-91f36ce768df" />
<img width="1920" height="1030" alt="浮窗" src="https://github.com/user-attachments/assets/b8751fb5-0a2f-459b-bc44-64e8247386d5" />
<img width="1405" height="905" alt="076e394832d4e0feba0d46ef558bd3ea" src="https://github.com/user-attachments/assets/7f911d2a-bbf4-4d14-8b2d-9921928c55f4" />


## 💬 交流群

使用中遇到问题、有功能建议，或想交流效率工具心得，欢迎加入 **x-hub 交流群**：

<img width="1279" height="1788" alt="微信图片_20260901004655_65_53" src="https://github.com/user-attachments/assets/a876d313-95cb-4f42-98bb-c7e0c14cc36f" />

> 二维码 7 天内有效，过期后重新进入会更新。若二维码失效，请到 [Issues](https://github.com/dckxx/x-hub/issues) 留言获取最新二维码。

## 📄 License

MIT
