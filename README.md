# Tauri2 + Vite + Vue3 + NaiveUI 脚手架

基于 [Tauri 2](https://tauri.app/) + [Vite](https://vite.dev/) + [Vue 3](https://vuejs.org/) + [NaiveUI](https://www.naiveui.com/) 的桌面应用脚手架。

## 技术栈

| 技术 | 版本 |
| ---- | ---- |
| Tauri | 2.x |
| Vue | 3.x |
| Vite | 8.x |
| NaiveUI | 2.x |
| TypeScript | 6.x |

## 特性

- Tauri 2 桌面应用框架（Rust 后端 + Web 前端）
- Vue 3 `<script setup>` 组合式 API
- NaiveUI 按需自动引入（unplugin-vue-components）
- Vue API 与 NaiveUI hooks 自动导入（unplugin-auto-import）
- 深色/浅色主题切换示例
- Rust `greet` 命令与前端 invoke 调用示例
- Tauri 应用版本获取示例

## 快速开始

### 环境要求

- Node.js 18+
- Rust（`rustc` 1.77.2+）
- Linux 系统依赖（Debian/Ubuntu）：

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### 安装依赖

```bash
npm install
```

### 开发模式（浏览器预览）

```bash
npm run dev
```

访问 http://localhost:1420

### 开发模式（Tauri 桌面窗口）

```bash
npm run tauri:dev
```

### 构建桌面应用

```bash
npm run tauri:build
```

产物输出到 `src-tauri/target/release/bundle/`。

## 目录结构

```
├── src/                  # 前端源码
│   ├── App.vue           # 根组件（主题与布局）
│   ├── components/
│   │   └── Dashboard.vue # 示例页面
│   └── main.ts           # 入口
├── src-tauri/            # Tauri 后端
│   ├── src/
│   │   ├── main.rs       # Rust 入口
│   │   └── lib.rs        # Tauri 应用与命令定义
│   ├── capabilities/     # 权限配置
│   ├── tauri.conf.json   # Tauri 配置
│   └── Cargo.toml
└── vite.config.ts        # Vite + NaiveUI 自动导入配置
```

## 前端调用 Rust 命令

在 `src-tauri/src/lib.rs` 中定义命令：

```rust
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
```

注册命令：

```rust
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![greet])
```

前端调用：

```ts
import { invoke } from '@tauri-apps/api/core'

const result = await invoke<string>('greet', { name: 'World' })
```

## 权限（capabilities）

前端调用 Tauri API 需要在 `src-tauri/capabilities/default.json` 中声明权限。脚手架默认包含：

- `core:default` - Tauri 核心默认权限
- `core:app:default` - 应用信息（版本号等）读取权限
