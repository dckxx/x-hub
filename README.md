<div align="center">

# ⚡ x-hub — 本地个人效率工作台

基于 **Tauri 2 + Vue 3 + TypeScript** 的桌面效率工具，Bento 风格界面。
**所有数据默认本地存储，不上传云端。**

![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3.x-42b883?logo=vuedotjs&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-6.x-3178c6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77+-dea584?logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-local-003b57?logo=sqlite&logoColor=white)
![Version](https://img.shields.io/badge/version-0.9.2-blue)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>


## ✨ 功能特性

### 🕐 工作台
**自由编排 Bento 网格**：时钟（**大时钟 / 今日阴阳历 / 极简时间**三形态，语录接入在线名言 hitokoto，离线自动回退本地语料）、**天气卡**（简版/详情版两形态，Open-Meteo 实时温度/体感/湿度/风速 + 城市/IP 定位）、系统资源监视器（CPU/内存，2s 轮询）、便签 ×2（600ms 防抖自动保存）、**日记**（回车即存入速记）、速记/待办/速达**概览卡**、提示词百宝箱、待办清单、**待办日历**（月历标注待办分布与逾期）、倒计时列表、**自定义速达 ×4**（钉选常用资源成组一键启动）、最近使用通栏——全部部件可在**所见即所得布局编辑器**中增删、拖拽、调宽高、切换形态，并存为默认布局随时恢复。

### ⏳ 倒计时
时长/定时/每天/间隔**四种模式**（最多 6 个）；暂停/继续/浮窗/删除；**后台驱动**（Rust 1s 轮询，到点发系统通知，`once` 灰态 / `daily` / `interval` 自动顺延，休眠错过静默顺延）；可浮起为**透明圆形水罐浮窗**（水位水波动画，独立置顶小窗，位置持久化）；可选到点提示音（WebAudio 合成双音）。

### 🚀 速达
应用 / 网页 / 文件**三类资源合一**管理；**大类 × 小类**两级筛选与**自定义分区**成组陈列；拖拽 exe/lnk 导入并**自动提取高清程序图标**（256×256 原生提取，含 .lnk 解析）；**扫描已安装应用批量导入**；点击一键启动——**已在运行的应用 / 浏览器直接把它已有窗口调度到前台**（含最小化与收进托盘的），不再重复开一个实例；网页可**应用内打开**（主窗内嵌面板或独立浏览器小窗，多页签）或**指定浏览器**打开；用途说明 + **加密备注**（本机加密落盘，小眼睛查看）；右键菜单操作；删除可撤销。

### 📝 速记
**所见即所得 Markdown 编辑器**（实时预览 / 分屏 / 源码 / 仅预览四种模式，600ms 防抖自动保存，标题自动派生）；**图片粘贴 / 拖拽 / 点击上传**直接落盘本地（内容哈希去重）；**双链** `[[笔记]]` 补全、跳转与反链面板；**文件夹树**管理（嵌套 / 拖拽移动）、标签筛选、**回收站**（软删可还原）、**剪藏**（带来源链接去重）；AI **深度整理**（整篇 / 选中片段语义重组，发送前明示）与**一键美化格式**；笔记支持整库**导出 / 导入**。

### 🔍 全局搜索
`Ctrl+K` 唤起，300ms 防抖，同时检索**资源、笔记、待办**，点击直达。

### ✅ 待办清单
优先级圆点、**截止日期与提醒**（到点系统通知）、**子待办**（父行显示进度）、**置顶**、**周期待办**（每天/每周/每月…勾选即滚动到下一轮，绝不堆积逾期副本）、**标签**、组内**拖拽排序**、行内编辑、删除可撤销；独立待办视图含**月 / 周日历**（格子拖拽改期、逾期淡红标记、含周期虚拟实例）与「范围 × 形态」双维筛选；支持全局搜索直达与高亮。

### 📋 剪贴板历史
快捷键 `Ctrl+\`` 全局唤起浮层；记录**文本 / 图片 / 文件**三类内容——复制图片/截图自动落盘缩略图、复制文件记路径；支持粘贴回剪贴板、图片预览与「保存图片」、相同内容自动去重；图片/文件记录开关可配。

### 🤖 AI 助手（对话 + Agent）
**OpenAI 兼容流式对话**（SSE 打字机 + Markdown 渲染，多会话管理；内嵌面板**四方位停靠**或**独立小窗**两形态，`Ctrl+Shift+K` 唤起），之上内置**宿主级 Agent 底座**——AI 不只聊天，还能直接把事办完：**34 个内置工具**横跨笔记 / 待办 / 速达资源 / 倒计时 / 剪贴板 / 提示词 / 便签的查询与读写（含启动应用），扩展亦可向 AI 助手自描述暴露工具。**权限三档**：仅可查看 / 每次都问（对话流内联确认卡片 + 「本会话内允许这类操作」）/ 完全权限；每笔工具调用在时间线留痕可回看，随时**中断**。**模型接入双路并存**：平台免费额度（**积分制**按 token 用量计费，对话框顶栏实时显示剩余积分、不足时给兑换指引）与 **BYOK 自备 Key** 直连（主流供应商**预设一键填**，支持测试连通性与批量拉取模型列表，API Key 存**系统钥匙串**、界面脱敏、不明文落盘），设置中随时切换。

### 💬 提示词百宝箱
常用提示词片段管理；置顶 + 复制计数；卡片一键复制。

### 🧩 扩展系统
**扩展中心**本地清单展示已安装扩展，支持从 **GitHub 仓库地址下载 zip** 解包安装、卸载、检查更新；**manifest 注册表**解析权限声明与版本；扩展支持 **module 卡片**（嵌入主界面）/ **view 页面** / **window / drawer 浮窗** 四种形态；宿主注入 **桥 API（window.xhub）** 与主程序交互（权限按 manifest 逐项授权，**安装前单独明示高风险能力**；打开外链需声明 `open-url` 权限，未声明时给出可见提示而非静默失败）；**service 托管**内置运行时按需下载、自动降级；扩展可**固定到左侧栏**，点击即打开，并选择「视图 / 窗口 / 抽屉」打开方式；内置**扩展市场**（远程清单验签下载安装、分类筛选、官方标识、安装计数、一键更新）；扩展可经 manifest 把自身能力**暴露给 AI 助手**调用；内置**扩展开发技能包（Skills）**一键装入本机 AI 编码助手。

### ⚙️ 系统设置
两级导航 **8 大类**（常规/外观/工作台/AI 助手/功能/扩展/账号/数据与关于）+ **设置项搜索**；**三轴主题**（模式 亮/暗/系统 × 10 色 + 10 渐变预设 × 强调色 8 预设/自定义）、**应用壁纸**（模糊 / 蒙版 / 沉浸模式 / 卡片玻璃透明度）、**全局快捷键录入**（五个全局快捷键均可改键或单独禁用，冲突预检）、**开机自启动**（Run 键方式，登录后静默驻留托盘）、**工作台布局编辑器**、**联网**（总开关 / 城市设置 / 名言来源）、**桌面悬浮球**开关、**AI 助手**（权限档 / 供应商模型 / 额度 / 对话形态）、**数据备份与恢复**、**数据存储路径**。

### 🖥️ 窗口能力
无边框 + 透明自制标题栏（拖动/最大化/还原/置顶按钮/关闭至托盘）；系统托盘常驻；全局快捷键唤起；记忆窗口位置尺寸（多显示器 / 缩放自适应）；便签 / 倒计时 / 提示词 / 待办独立浮窗；**桌面悬浮球**（贴边半隐、悬停滑出、环形菜单直达常用功能，拖拽自由落位与吸附）。

### 🔄 应用自动更新
**自研升级链路**：从 `releases/update.json` 升级清单拉取**新版本信息**（Ed25519 分离签名验签，内嵌公钥验签通过才信任）→ semver 版本比较 + **跳级保护**（`minimumUpgradable` 下限）→ **自动静默检查**（启动 5s 后 + 默认每 4 小时，可在 About 关闭）；发现新版本弹出**全局更新弹窗**（版本号 / 说明 / 体积 / 便携版标记），支持**跳过此版本**（记录到配置）/**立即更新**（流式下载 + 实时进度条 + sha256 完整性校验）/ 就绪后**立即重启**；重启时解包并两步 rename **自替换**，失败自动回滚、下次启动重试；About「检查更新」可手动触发。更新包分发在腾讯云 COS，支持标准版 / 便携版分别取包。

### 💾 数据存储与便携
所有数据默认本地存储，支持三种形态灵活切换：**标准版**数据默认在 `%APPDATA%\x-hub`，可在设置中「更改数据存储路径」迁移到任意目录（迁移后重启生效）；**便携版**只需在 exe 同目录放一个空文件 `portable`，数据即固定跟随 `exe\data` 子目录，整个文件夹拷到 U 盘即可随身携带；**数据备份/恢复**打包为 `x-hub-backup-时间戳.zip` 单个压缩包，便于归档与迁移。

## ⌨️ 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl + Shift + Space` | 显示 / 隐藏主窗口 |
| `Ctrl + K` | 唤起全局搜索 |
| `Ctrl + Shift + K` | 唤起 / 收起 AI 对话 |
| `Ctrl + \`` | 唤起剪贴板历史浮层 |
| `Ctrl + Shift + N` | 唤起主窗并切到速记 |
| `Esc` | 关闭弹窗 |

五个全局快捷键均可在设置中自定义改键，或单独禁用。

## 🛠️ 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | Vue 3（`<script setup>`）+ TypeScript + Tailwind CSS 4 + Vite 8 |
| 图标 | lucide-vue-next（按需引入，颜色继承 currentColor） |
| 表单 | reka-ui 无头组件（DatePicker / TimeField / NumberField，样式自绘） |
| 编辑器 | Milkdown Crepe（速记所见即所得，Markdown 为真相源） |
| 后端 | Rust（Tauri 2）+ rusqlite（SQLite, WAL 模式）+ sysinfo（系统资源）+ 自绘右下角通知窗（跨 Win10/11 一致，见 notify.rs）+ reqwest（OpenAI 兼容 SSE 流式）+ keyring（API Key 系统钥匙串） |
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
├── main.ts / App.vue        # 入口与窗口壳（App.vue 按窗口 label 路由：主界面 / 便签 / 倒计时 / 剪贴板 / 扩展 / 提示词 / 待办浮窗 / AI 对话窗 / 悬浮球 / 通知）
├── index/index.vue          # 首页：侧栏导航（工作台/待办/速记/速达 + 扩展中心）+ 三轴主题 + 布局协调 + 搜索/设置协调
├── style.css                # 设计令牌（亮/暗色）+ 通用样式
├── api/tauri.ts             # Tauri invoke 类型安全封装（227 个命令）
├── stores/workbench.ts      # 响应式状态管理（工作台/便签/待办/倒计时/提示词/AI 对话/扩展/自动更新）
├── composables/             # 组合式函数（主题 / 布局编辑器 / 扩展桥接 / 快捷键录制 / 速达拖拽…）
├── utils/                   # 文件分类 / 时间 / 农历 / 天气码 / 待办排期 / 提示音 / 错误上报
└── components/              # 功能组件（工作台卡片/速达/速记/待办/搜索/AI 对话/扩展中心/设置/更新弹窗…）

src-tauri/src/
├── lib.rs                   # 应用构建：数据库/托盘/快捷键/窗口状态/单实例/数据迁移/命令注册
├── commands.rs / models.rs  # Tauri 命令处理与数据模型
├── db.rs / config.rs        # SQLite 迁移 + 配置持久化
├── agent/                   # Agent 底座（34 内置工具注册表 / turn 循环 / 权限三档与会话级授权 / 上下文截断剪枝 / system prompt / 扩展工具聚合）
├── chat.rs                  # OpenAI 兼容 SSE 流式客户端（含 tool_calls 解析）+ API Key 钥匙串存取
├── account.rs               # 平台账号（登录/注册/积分额度）
├── extension.rs / xhub_api.rs / service.rs / proxy.rs  # 扩展系统（扫描安装 / 桥 API / Node 托管 / 反向代理）
├── market.rs / updater.rs   # 扩展市场与应用自动更新（Ed25519 验签 + 下载校验 + 重启自替换）
├── suda_browser.rs          # 速达「应用内打开网页」（内嵌面板 + 浏览器窗池）
├── app_icon.rs              # 程序图标原生提取（256×256，.lnk 解析）
├── process.rs               # 程序启动（已在运行则调度窗口到前台）/ URL 打开 / 提权（UAC）
├── note_io.rs               # 速记导出/导入
├── todo_recurrence.rs / todo_reminder.rs  # 周期待办规则（唯一实现）+ 提醒后台线程
├── countdown_ticker.rs / countdown_window.rs  # 倒计时后台驱动 + 圆形浮窗
├── floating_ball.rs         # 桌面悬浮球（贴边/滑出/环形菜单）
├── notify.rs                # 右下角自绘通知窗（跨 Win10/11，前端 NoticeOverlay.vue 渲染卡片）
├── clipboard.rs             # 剪贴板监听与历史 + 粘贴注入
├── shortcut.rs / tray.rs    # 全局快捷键（5 个）/ 系统托盘
├── sysmon.rs                # 系统资源监视（CPU/内存）
└── repo/                    # 数据访问层（资源/笔记/待办/便签/提示词/倒计时/对话/剪贴板/标签…）
```

## 🔒 数据与隐私

数据统一存放在「数据根」目录下（标准版默认为 `%APPDATA%\x-hub`，可经设置改到任意目录；便携版为 `exe\data`）：

- **数据库**：`数据根\app.db`（SQLite，20+ 张表：速达资源与小类/分区、笔记与文件夹/双链、待办与标签、便签、提示词、倒计时、AI 对话、剪贴板历史等）
- **图标**：`数据根\icons\`（拖拽导入/扫描安装应用时自动提取的程序图标）
- **笔记图片**：`数据根\notes\images\`（编辑器上传的图片，内容哈希命名天然去重）
- **壁纸**：`数据根\wallpapers\`（导入的应用壁纸，内容哈希命名）
- **日志**：`数据根\logs\x-hub.log`（文件日志，便于排查）
- **剪贴板快照**：`数据根\clipboard\images\`（复制图片时落盘的缩略图；删除 / 清空 / 过期清理时联动删除）
- **备份**：设置内一键备份/恢复，打包为 `x-hub-backup-时间戳.zip` 压缩包（数据库 + 图标）
- **应用更新**：下载的更新包暂存在 `数据根\updates\`，自替换成功 / 失败回滚后自动清理
- **AI 对话**：会话与消息存本地 SQLite；API Key 存入**系统钥匙串**（keyring），界面脱敏展示，**不明文落盘、不上传**

## 配图
<img width="1400" height="933" alt="工作台​" src="https://github.com/user-attachments/assets/ffb4358b-8e39-4388-aab6-3827e64ed883" />
<img width="1400" height="933" alt="workbench" src="https://github.com/user-attachments/assets/aa7323e6-e66b-411e-9c9b-b63f3c057c88" />
<img width="1400" height="933" alt="todo" src="https://github.com/user-attachments/assets/324cc42f-8e6f-477d-bd2b-aa2d3d38f1ee" />
<img width="1400" height="933" alt="settings" src="https://github.com/user-attachments/assets/7ea3db47-f5a6-40db-b632-f2fee0ec4215" />
<img width="1400" height="933" alt="notes" src="https://github.com/user-attachments/assets/8c0bd21e-187d-4117-a098-640c3b95451c" />
<img width="1400" height="933" alt="launcher" src="https://github.com/user-attachments/assets/c3e827c0-fcb4-4cec-ad5e-789cf8669bff" />
<img width="1400" height="933" alt="extensions" src="https://github.com/user-attachments/assets/6ddd20ce-b35c-4db0-8a37-cd1c4439dab1" />

## 💬 交流群

使用中遇到问题、有功能建议，或想交流效率工具心得，欢迎加入 **x-hub 交流群**：

<img width="360" height="544" alt="群聊：x-hub交流 ② 群" src="assets/wechat-group-qr.png" />



> 二维码 7 天内有效，过期后重新进入会更新。若二维码失效，请到 [Issues](https://github.com/dckxx/x-hub/issues) 留言获取最新二维码。

## 📄 License

MIT
