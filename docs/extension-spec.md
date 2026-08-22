# x-hub 扩展系统设计规范

> 状态：草案 v3（讨论定稿后实施）
> 关联原型：`extension-ui-prototype.html`（8 场景可交互预览）
> 更新：2026-08-22
>
> **v3 修订**：运行时命名 `sidecar` → `service`；后端声明字段 `service` → `backend`；新增 service 运行时提供策略（复用系统 Node + 按需下载内置 + 启动前健康检查 + 自动降级）。

## 1. 核心概念

- **扩展（Extension）**：一个可安装的功能包，是唯一的一等公民，不再区分「插件 / 小程序」两套体系。
- **形态（Surface）**：扩展运行时可以被渲染成的展示形态，一个扩展可支持多种形态。
- **打开方式（OpenIn）**：app 形态下「怎么打开」（主区视图 / 独立窗口 / 侧边抽屉），由三层配置决定。
- **运行时（Runtime）**：扩展代码的运行方式，分两轨——
  - `web`：纯前端（HTML / JS / WASM），跑在主程序 WebView 沙箱内，无独立进程。
  - `service`：带独立后端服务（默认 Node，宿主统一管理运行时），由主程序托管。

## 2. 运行时：web / service

### 2.1 web（轻扩展，默认）

- 纯 HTML / JS / WASM，跑在主程序 WebView 沙箱，天然跨平台、体积小、单一包格式。
- 无独立进程、无端口、无后端。
- 需要系统能力（读写文件、剪贴板、网络等）时，经**桥 API（`window.xhub.*`）**申请，受 manifest `permissions` 约束。
- 适用：纯前端小工具（ctool、JSON 格式化、图床、正则测试器、摘要卡等）。

### 2.2 service（重扩展）

- 扩展包内自带一个后端服务（默认 Node），随扩展安装而启动。
- 主程序负责该服务的**托管**：启动、端口分配、健康检查、代理转发、卸载清理（见 §5）。
- 后端所需的 Node 运行时由主程序统一管理（复用系统 / 按需下载内置），扩展包只带代码。
- 适用：需要后端 / AI / 平台 API / 原生能力的扩展（DSH、公众号发布、封面生成、视频剪辑、新闻聚合等）。

> **决策演进**：v1「仅 wasm、去 sidecar」→ v2「恢复为宿主托管的 sidecar」→ v3「更名 service，并补运行时提供策略」。原因是自媒体 / AI 类扩展是 Node 服务、无法 wasm 化；service 不再等于「扩展自由塞任意 exe」，而是「扩展声明一个后端，宿主统一托管其进程与运行时」。

### 2.3 选型原则

- 默认 `web`；仅在确实需要后端 / AI / 平台 API / 原生能力时声明 `service`。
- `service` 承担更高信任要求，见 §7 安全分级。

## 3. 四种形态

| 形态 | 说明 | 例子 |
|---|---|---|
| `module` | 工作台里的一张摘要卡，与其他卡片并存 | API 助手：今日 N 次请求 / 平均耗时 |
| `view` | 占满右侧主工作区（侧栏保留），点开切换 | ctool 全屏工具、速记/速达 |
| `window` | 独立子窗口，可与主窗口并排 | 边看文档边调试 |
| `drawer` | 右侧滑出面板，主界面仍在 | 轻量速查工具 |

- `view` 不盖左侧栏：侧栏是全局导航，app 视图只替换右侧主工作区内容，复用现有「速记/速达/用量」视图切换链路。
- **形态描述的是「展示形态」而非「功能类型」**：同一个扩展可同时提供 module 摘要卡和 app 完整工具，manifest 里的 `kind` 只是默认/主形态。

## 4. manifest 字段

> **entry 统一为 HTML**：所有形态的 `entry` 都指向一个 HTML 文件（可引用同目录下的 JS / CSS 资源，相对路径自动解析）。web 扩展入口即前端 HTML（如 Vue 构建产物的 `dist/index.html`）；service 扩展除前端 HTML 外，另有 `backend.entry` 指向后端服务入口（Node 的 `index.js`）。

### web 扩展示例

```jsonc
{
  "id": "com.x-hub.apidebug",       // 唯一，反向域名
  "name": "API 调试助手",
  "version": "1.0.0",
  "runtime": "web",                  // 运行时：web（默认）| service
  "kind": "view",                    // 默认/主形态，决定「点开」后进哪个形态
  "surfaces": ["module", "view"],    // 声明支持哪些形态
  "openIn": ["view", "window"],      // app 形态下支持哪些打开方式
  "entry": {
    "module": "./card/index.html",   // module 形态入口
    "view":   "./app/index.html",    // view/window/drawer 共用入口
    "drawer": "./drawer/index.html"  // drawer 若要独立入口（可选）
  },
  "permissions": ["clipboard"],      // 能力申请，按需授权
  "icon": "./icon.svg",
  "minSize": { "w": 480, "h": 360 }  // window/drawer 建议尺寸
}
```

### service 扩展示例

```jsonc
{
  "id": "com.x-hub.dsh",
  "name": "DeepSeek Harness",
  "version": "0.1.1",
  "runtime": "service",
  "kind": "view",
  "surfaces": ["view"],
  "openIn": ["view", "window"],
  "entry": {
    "view": "./entry/index.html"     // 前端界面，跑在宿主 WebView
  },
  "backend": {                       // service 专属：受托管的后端服务
    "entry": "./service/index.js",   // 后端入口
    "engine": { "type": "node", "minVersion": "22" },  // 运行时引擎要求（宿主据此判断复用或下载）
    "cwd": "./service",              // 工作目录
    "port": 0,                       // 0 = 动态分配；固定端口需冲突检测
    "health": "/healthz"             // 健康检查路径（可选）
  },
  "permissions": ["network", "fs", "process"],
  "icon": "./icon.svg"
}
```

## 5. service 托管机制

### 5.1 进程托管

主程序（宿主）对每个 service 扩展负责：

1. **安装**：解包 `.xhpack`，读 manifest，识别 `runtime: service`。
2. **启动**：按 `backend` 声明启动后端进程（默认 Node），工作目录设为 `backend.cwd`。
3. **端口**：`backend.port` 为 0 时由宿主动态分配 `127.0.0.1` 空闲端口；固定端口需先做占用检测，冲突则报错。
4. **健康检查**：启动后按 `backend.health`（或启动超时）探活，失败则标记扩展不可用并提示。
5. **代理转发**：宿主暴露 `/svc/<extId>/*` 反向代理到后端端口；扩展前端只 fetch 宿主同源路径，不直接接触端口——统一解决 CORS / Origin / 鉴权问题。
6. **注入**：经桥 API 把扩展运行时信息（runtime 类型、service 是否就绪、代理前缀）提供给前端 entry。
7. **卸载 / 清理**：卸载或宿主退出时，终止后端进程、清理扩展本地数据（可选保留）。

> service 后端进程的启动、停止、异常退出、重启策略、日志，均归宿主管理；扩展作者不管理进程生命周期。

### 5.2 运行时提供

service 扩展的后端默认跑在 Node 运行时上，运行时由宿主统一管理，策略如下：

1. **优先复用系统 Node**：检测系统是否已装 Node，且版本满足 `backend.engine.minVersion`；满足则直接复用，零下载。
2. **按需下载内置 Node**：系统无 Node 或版本不符时，首次安装 service 扩展会从扩展市场下载一次「Node 运行时组件」，缓存到本地，所有 service 扩展共用（与扩展包分离分发）。
3. **启动前健康检查**：每次启动 service 扩展前，重新校验运行时（存在性 + 版本），不沿用安装时的结论——应对系统 Node 被卸载 / 升级 / 切换（如 nvm 切版本）等漂移。
4. **自动降级**：校验失败时，自动切换到内置 Node（已下载则直接使用，未下载则触发下载）；下载也失败才报错。
5. **友好报错 + 一键修复**：绝不向用户裸抛底层错误（如 `command not found`），而是提示「扩展需要 Node ≥ 22，检测到系统 Node 为 X，已 / 可切换到内置运行时」，并提供一键修复入口。
6. **设置项**：运行时策略三选一——「自动检测（默认）/ 始终使用内置 / 始终使用系统」。

> 重型应用例外：DSH 这类自带 node_modules 与原生 addon、无法复用共享 Node 的完整应用，作为「自包含运行时」例外处理（独立进程、安装时明确提示、体积大），见 §7。

## 6. 桥 API（`window.xhub.*`）

扩展（web / service 通用）与宿主通信的唯一契约。底层为 Tauri IPC；扩展只依赖这一套稳定 API，不感知宿主内部实现。

**原则**：

- 所有能力经 manifest `permissions` 声明，未授权调用直接拒绝；
- 返回 Promise；宿主侧做权限检查与审计；
- 版本化：API 加命名空间版本，避免破坏扩展。

**最小方法集（骨架，签名另文细化）**：

| 命名空间 | 能力 |
|---|---|
| `window.xhub.data.notes / todos / resources / usage` | 读宿主数据 |
| `window.xhub.fs` | 受控文件读写（需 `fs` 权限） |
| `window.xhub.clipboard` | 读写剪贴板（需 `clipboard`） |
| `window.xhub.net` | 发起网络请求（需 `network`） |
| `window.xhub.runtime` | 扩展自身运行时信息（runtime 类型、service 就绪、代理前缀） |
| `window.xhub.service` | service 扩展调用自身受托管服务（代理转发封装） |
| `window.xhub.storage` | 扩展本地键值存储（隔离、可随卸载清除） |
| `window.xhub.events` | 订阅宿主事件（如用量更新、倒计时触发） |

> 完整方法签名见 `docs/extension-api.md`；本规范只定边界与原则。

## 7. 安全分级

| 运行时 | 信任级别 | 风险 | 措施 |
|---|---|---|---|
| `web` | 低 | 仅 WebView 沙箱 + 受限桥 API | manifest `permissions` 逐项授权；安装轻提示 |
| `service` | 高 | 独立进程 = 任意代码执行 | 安装时明确提示「会运行本地后台进程」；`permissions` 含 `process`；卸载彻底清理进程与数据 |

- `service` 后端仅监听 `127.0.0.1`，不暴露到局域网；
- 自包含运行时的重型应用（如 DSH）安装时额外提示其自带运行环境与体积；
- 未来可加资源限额 / 行为监控，一期不做。

## 8. 预装扩展

- 出厂**预装**一批扩展（基础卡片、可选 DSH 等），新用户开箱即用；
- 预装扩展与普通扩展**无差别**，都可在扩展中心卸载，卸载后回到极简状态；
- 核心壳保持薄：导航、窗口、设置、扩展运行时本身，其余功能尽量以（预装）扩展提供。

## 9. 形态路由

```
点开一个扩展
 ├─ 用户偏好已设 → 用用户偏好
 ├─ 否则用扩展 manifest 的 kind / openIn 默认
 └─ 兜底 → view（主区视图）
```

- `module`：始终以卡片存在，可被拖到工作台任意格子（复用现有 12 列网格）。
- `view`：切右侧 `main.workspace`，侧栏不变。
- `window`：新开子窗口（Tauri 多窗口）。
- `drawer`：右侧滑出 overlay 面板，宽度受 `minSize.w` 约束。

## 10. 三层打开方式配置

1. **扩展作者声明**（manifest `openIn`）——只声明自己适配的。
2. **用户偏好**（扩展设置页，每个扩展一个下拉：视图 / 窗口 / 抽屉）。
3. **运行时临时切换**（app 视图工具栏「在窗口打开 / 在抽屉打开」，仅本次生效）。

**优先级**：用户偏好 > 作者声明 > 全局默认（view）。

## 11. 界面结构

### 侧栏入口
- 「扩展中心」放在**设置上方**（紧邻设置），不插在功能菜单（工作台/速记/速达/用量）下方。

### 扩展中心（列表）
- 每行：应用图标 + 名称/描述 + 默认打开方式下拉 + 右侧「⋯」进入扩展设置。
- **service 扩展标注 `service` 标签**（原型 `.tag.service`，橙色），提示其带后端；web 扩展不标注运行时。
- 不显示权限标签（权限只在详情页展示）。
- 下拉宽度保持一致（统一 `min-width`）。
- 顶部：「+ 安装扩展」主按钮 + 「+ 从本地安装」。

### 扩展设置（单个扩展详情，列表「⋯」进入）
- 扩展信息（图标 / 名称 / id / 版本 / 作者 / 运行时类型）。
- 打开方式（默认形态、app 打开方式）。
- 权限（逐项开关）。
- 本地数据（占用大小 + 清除数据）。
- 卸载（底部危险按钮；service 扩展卸载会同时终止其后端进程）。

### 安装扩展页
- 两个 tab：**扩展市场** / **本地安装**。
- 市场：扩展卡片列表，点卡片进入「扩展详情」。
- 本地安装：拖入 `.xhpack` 包或选择扩展文件夹。

### 扩展详情（市场）
- 截图区、功能介绍、使用说明（步骤）、权限清单。
- 信息区：作者 / 开源地址 / 许可证 / 更新时间 / 安装量 / 版本 / 运行时类型。
- 右上角「安装」按钮；service 扩展安装前弹「会运行本地后台进程」提示。

### 卸载与数据清理
- 卸载入口：扩展设置页底部「卸载扩展」按钮，或列表行悬停直接卸载。
- 数据清理：卸载**默认连同扩展本地数据/设置一并清除**，但提供「保留数据」选项（只删程序、留数据，便于重装恢复）。
- service 扩展卸载时额外终止并清理其托管后端进程。

## 12. 建议落地顺序

> 状态图例：`done` 已完成 · `in-progress` 实现中 · `planned` 已规划未开始
> 本表是落地进度的单一事实来源，改动时更新状态。

1. `done` 扩展 manifest 解析 + 注册表（本地目录扫描），manifest 支持 `runtime` 字段；含扩展中心列表骨架（`list_extensions` 命令 + `ExtensionCenter` 视图，入口在侧栏「设置」上方）。
2. `done` 桥 API 骨架（`window.xhub.runtime / storage / data` 读，`xhub_call` 统一分发命令 + iframe postMessage 桥）。
3. `done` `view` 形态（扩展中心点开 → 主区 iframe 加载扩展入口 HTML + 自动注入桥脚本；端到端加载待实际运行验证）。
4. `done` 跑通 `module` 卡片（扩展声明 module 形态 → 自动进布局编辑器模块库 → 拖入工作台网格，iframe 渲染 module 入口复用桥 API）。
5. `done` 再补 `window` / `drawer`（window 走 Tauri 多窗口 + 复用 iframe 桥；drawer 右侧滑出 overlay）。
6. `done` **service 托管**：进程托管（懒启动 / 动态端口 / TcpStream 探活 / 卸载清理 / 宿主退出停止）+ 运行时提供（复用系统 Node + 启动前校验 + 内置运行时按需下载 + 自动降级）+ 代理（桥 API `service.request` + `/svc/<extId>/*` HTTP 反向代理 + WebSocket 升级双向隧道，统一 CORS）。
7. `done` 扩展中心补全：扩展设置（信息/权限/卸载）+ 本地安装 + 权限授权 + 市场（本地清单文件 `market/registry.json` → 下载 zip → 解包安装；后续接远端市场替换清单数据源即可）。

## 附录：v2 → v3 差异

| 章节 | v2 | v3 |
|---|---|---|
| 运行时命名 | sidecar | service |
| 后端声明字段 | `service: {...}` | `backend: {...}` |
| 运行时提供 | 未定义 | 新增 §5.2（复用系统 Node + 按需下载 + 健康检查 + 自动降级 + 设置项） |
| manifest | `"runtime": "sidecar"` | `"runtime": "service"` + `backend.engine` |
| 界面标签 | 「本地服务」 | `service` |
