# Requirements Document

## Introduction

本地个人效率工作台桌面客户端（Personal Workbench）。基于 Tauri 2 + Vite + Vue3 + NaiveUI 技术栈构建，面向个人用户的本地效率工具。所有业务数据默认存储在本地 SQLite 数据库，基础配置存储在本地 JSON 文件，无主动联网上传行为。

MVP 版本包含四个模块：窗口基础能力、快捷启动工作台、速记笔记、系统设置。

## Glossary

- **System**: 本地个人效率工作台桌面客户端
- **User**: 个人桌面客户端使用者
- **Window**: 主应用窗口
- **Tray**: 系统托盘图标
- **Global Shortcut**: 全局快捷键（应用未聚焦时仍可触发的快捷键）
- **Shortcut Resource**: 快捷启动资源（本地程序或网页书签）
- **Resource Group**: 快捷资源的分类分组
- **Note**: 速记笔记（纯文本）
- **SQLite**: 本地业务数据库
- **JSON Config**: 基础配置文件（本地存储）

## Requirements

### 1. 窗口基础能力

**User Story:** 作为用户，我希望窗口具备无边框自定义能力、托盘常驻与全局快捷键唤起，以便获得轻量原生桌面体验。

#### Acceptance Criteria

1. WHEN 用户启动应用，System SHALL 创建无边框窗口并以自制标题栏替代系统标题栏
2. WHEN 用户点击标题栏最小化按钮，System SHALL 最小化主窗口
3. WHEN 用户点击标题栏最大化按钮，System SHALL 在主窗口最大化与还原之间切换
4. WHEN 用户点击标题栏关闭按钮，System SHALL 隐藏主窗口至系统托盘而非退出进程
5. WHEN 用户单击系统托盘图标，System SHALL 切换主窗口的显示与隐藏状态
6. WHEN 用户右键系统托盘图标，System SHALL 显示托盘右键菜单
7. WHEN 用户通过系统托盘菜单选择"退出"，System SHALL 结束进程
8. WHEN 用户在任意应用界面按下全局快捷键，System SHALL 切换主窗口的显示与隐藏状态（默认快捷键为 Ctrl+Shift+Space）
9. WHILE 主窗口显示中，System SHALL 保持窗口在系统托盘常驻
10. WHEN 用户关闭主窗口后再次打开，System SHALL 恢复上次关闭时的窗口位置与尺寸
11. WHEN 用户开启窗口置顶选项，System SHALL 保持主窗口置于其他窗口之上
12. IF 用户未开启窗口置顶选项，System SHALL 保持主窗口正常层叠顺序

### 2. 快捷启动工作台模块

**User Story:** 作为用户，我希望集中管理本地程序与网页书签并通过点击快速启动，以便减少桌面查找时间。

#### Acceptance Criteria

1. WHEN 用户新增本地程序资源，System SHALL 支持选择本地可执行文件并记录自定义名称、自定义图标与附加启动参数
2. WHEN 用户点击本地程序资源，System SHALL 使用记录的启动参数启动该程序
3. WHEN 用户新增网页书签资源，System SHALL 支持记录标题、网址与自定义图标
4. WHEN 用户点击网页书签资源，System SHALL 调用系统默认浏览器打开该网址
5. WHEN 用户管理快捷资源，System SHALL 支持对资源进行分组管理
6. WHEN 用户调整资源排序，System SHALL 支持通过拖拽方式调整资源顺序（组内与跨分组）并持久化保存
7. WHEN 用户管理单个资源，System SHALL 支持新增、编辑、删除操作
8. WHEN 用户对资源使用右键操作菜单，System SHALL 显示包含编辑、删除等操作的菜单
9. WHEN 用户在全局搜索框输入关键词，System SHALL 检索所有快捷资源的名称并展示匹配结果

### 3. 速记笔记模块

**User Story:** 作为用户，我希望快速记录纯文本笔记并检索，以便随时捕捉与回顾想法。

#### Acceptance Criteria

1. WHEN 用户新建笔记，System SHALL 创建一条纯文本笔记并允许自定义标题
2. WHEN 用户编辑笔记文本内容，System SHALL 保存文本内容修改
3. WHEN 用户查看笔记列表，System SHALL 在左侧展示笔记条目列表，在右侧展示编辑区域
4. WHEN 用户查看笔记条目，System SHALL 展示笔记标题、创建时间与内容摘要
5. WHEN 用户点击笔记条目，System SHALL 切换右侧编辑区域显示所选笔记内容
6. WHEN 用户删除笔记，System SHALL 移除该笔记
7. WHEN 用户在全局搜索框输入关键词，System SHALL 检索笔记标题与文本内容并展示匹配结果

### 4. 系统设置模块

**User Story:** 作为用户，我希望基础配置保存在本地并支持主题切换，以便按偏好使用应用。

#### Acceptance Criteria

1. WHEN 用户更改软件基础配置，System SHALL 将配置持久化保存至本地 JSON 文件
2. WHEN 用户切换亮色/暗色主题，System SHALL 更新应用主题并持久化保存该选择
3. WHEN 应用启动，System SHALL 读取本地配置并应用上次保存的主题

## 预留功能（本版本不开发，仅规划）

1. 剪贴板历史记录助手
2. Markdown 富文本笔记编辑器
3. 数据导入、备份与恢复功能

## 非功能约束

1. System SHALL 定位为个人工具工作台而非后台管理系统
2. System SHALL 保持轻量化运行，减少资源占用
3. System SHALL 将系统文件、进程调用、持久化存储逻辑放在 Rust 后端实现，前端仅负责交互展示
4. System SHALL 除打开外部网页外无主动联网上传行为
5. System SHALL 使用 SQLite 本地数据库管理业务数据，使用 JSON 文件存储基础配置
