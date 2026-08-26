# x-hub 扩展系统 × DeepSeek Harness 插件模型：对照与演进建议

> 生成时间：2026-08-20 ｜ 研究对象：本仓库 `src-tauri/src/{extension,service,xhub_api}.rs` + `src/composables/useExtensionFrame.ts` + `x-hub-extension` skill；对照 DeepSeek Harness（`@deepseek-ai/dsh`，Cordis 底座）与 OpenAI Codex（`codex-rs`）的扩展机制。
>
> 本文是**建议稿**，非实施计划。目标是先对齐「x-hub 现状 vs DSH 模型」的差距，再给出分阶段、可取舍的演进路线。

## 实施进度（2026-08-20 更新）

| 阶段 | 状态 | 落地内容 |
|---|---|---|
| ① 桥 API 注册表化 | ✅ 已完成 | `xhub_api.rs` `CAPABILITIES` 静态表 + `runtime.info` 返回 `capabilities` |
| ② manifest 依赖 + 条件 | ✅ 已完成 | `requires`/`dependsOn`/`disabled` 字段 + 扫描求值 + 扩展中心「缺能力/缺依赖/已禁用」提示 |
| ② 配置分层 | ✅ 已完成 | `config.*` 桥 API：`manifest.config` 默认 ∪ `.config.json` 用户覆盖 |
| ③ 事件总线 | ✅ 已完成 | `events.emit/on` 跨扩展广播（需 `events` 权限），`themeTokens.ts` 的 `activeFrames` 注册表 |
| ③ 跨扩展调用 / 共享存储 | ✅ 已完成 | `runtime.callExtension` + `expose` 白名单 RPC；`sharedStorage.*`（需 `shared-storage` 权限） |
| ④ 能力注入 + 热更新 | ✅ 已完成 | manifest `actions`（扩展中心渲染动作按钮）；`extensions_stamp` 轮询刷新列表 |

---

## 结论先行（TL;DR）

1. **两者的"扩展"本质不同**：DSH 扩展的是 *agent 能力*（工具/服务/UI 注入 agent 循环），x-hub 扩展的是 *工作台面板*（iframe 小应用 + 数据读 + 可选后端）。所以**不应照搬 Cordis**，但值得借鉴它 4 个通用工程模式：服务注册表、依赖注入、配置分层、软失败。
2. **x-hub 当前最痛的一点**：桥 API 是 `xhub_api.rs` 里 `match (namespace, method)` 的手工白名单，每加一个能力都要改宿主核心并重发。这是 DSH 用「服务注册表 + 依赖注入」解决得最好的问题。
3. **演进路线建议分 4 阶段**，前两阶段（桥 API 注册表化、manifest 增加依赖/配置分层）性价比最高、风险最低；后两阶段（扩展间协作、运行时动态挂载）是长期方向，需重新设计安全边界。

---

## 一、x-hub 扩展系统现状（已实现）

### 1.1 架构链路

```
扩展目录 %APPDATA%\x-hub\extensions\<id>\
  └─ manifest.json  +  entry HTML (module/view/window/drawer) + 可选 service/

宿主 Rust (Tauri)                         前端 Vue
───────────────                          ─────────
extension.rs   扫描/解析 manifest          ExtensionCenter.vue     扩展中心
service.rs     Node 后端懒启动/端口/探活    ExtensionView.vue      view 面板
xhub_api.rs    桥 RPC 分发 (xhub_call)     ExtensionWindow.vue    独立窗口
proxy.rs       /svc/<id>/* 反向代理        useExtensionFrame.ts   iframe+postMessage 桥
```

调用链：扩展 iframe 内 `window.xhub.*` → `postMessage` 到主窗口 → 主窗口 `invoke('xhub_call', {extId, namespace, method, args})` → `xhub_api.rs::dispatch` 按 `(namespace, method)` 分发。

### 1.2 能力清单

| 维度 | 现状 |
|---|---|
| manifest | id / name / version / runtime(web\|service) / kind / surfaces / openIn / entry / permissions / icon / minSize / backend / description |
| 形态 surface | `module`(摘要卡) / `view`(主工作区) / `window`(独立窗) / `drawer`(右滑) —— 四种都走 iframe |
| 桥 API 已实现 | `runtime.info/open/callExtension`、`storage.*`、`config.*`、`sharedStorage.*`(需 `shared-storage`)、`data.notes/todos/resources`(只读)、`service.request`、`theme.get`、`events.on/emit`、`expose` |
| 桥 API 未实现 | `data.*` 写、`data.usage.*`、`clipboard.*`、`net.*`、`fs.*`、`system.*`、`ui.*` |
| service 后端 | 懒启动 Node 子进程、动态 127.0.0.1 端口、TCP 探活、`/svc/<id>/*` 反代、宿主退出时 stop_all |
| 权限 | manifest 声明 → 默认授权 → 用户可显式关闭（`.permissions.json` 只存关闭项） |
| storage | 每扩展 `.storage.json` 隔离，随卸载清除 |
| 安装/卸载 | 目录复制进 `extensions/<id>/`；卸载停后端 + 删目录 |
| 分发 | 脚手架 `pack` 打包 `.xhpack`(zip)；市场清单 `downloadUrl` + 本地导入（扩展中心） |

### 1.3 关键设计点（做得对、要保留）

- **iframe 隔离**：扩展无法直接碰宿主 DOM/状态，安全边界清晰。
- **同 origin 入口注入**：桥脚本注入到 `<head>`，临时 HTML 写到扩展目录 `.xhpack/`，Vite 产物相对资源同 origin 加载，规避 srcdoc 的 CORS。
- **软失败**：manifest 损坏的目录标 `invalid` 返回，不 panic、不静默消失。
- **权限默认授权 + 显式关闭**：低摩擦，`.permissions.json` 是覆盖层（已是"分层"雏形）。

---

## 二、DSH 插件模型要点（对照基准）

| 机制 | DSH 做法 |
|---|---|
| 底座 | Cordis（`Context` 依赖容器 + `Service` + `ctx.plugin()` + `inject` 声明依赖 + Fiber 生命周期） |
| 插件单元 | 每个 `@deepseek-ai/dsh-*` npm 包是一个插件，`peerDependencies` 声明服务依赖 |
| 装配 | `cordis.patch.yml`：`- insert:` 插入多行 `{id, name, config, disabled}`；`!!js` 表达式做运行时条件 |
| 分层覆盖 | 空 root → bundle 补丁（有序）→ profile 补丁 → `$DSH_HOME` 补丁 → `--patch`，按 `id` 定位行、后写覆盖 |
| 动态扩展 | 模型用 `cordis_define/run/stop/undefine` 在运行中定义/挂载/卸载插件（vm 沙箱 host half + 浏览器 half） |
| 管理 | `dsh plugin` = pnpm forwarder，reconcile 出 `dsh.profile.bundles` 有序列表 |

一句话：**DSH 让"插件"成为运行时一等公民，宿主核心本身也是插件；配置是分层补丁，能力靠依赖注入激活。**

---

## 三、逐维对照

| 维度 | x-hub 现状 | DSH 模型 | 差距 / 可借鉴点 |
|---|---|---|---|
| 扩展的定位 | 隔离 iframe 小应用 | 宿主运行时的一等公民（服务） | x-hub 隔离是对的；缺的是「宿主能力也能被声明式扩展」 |
| 能力装配 | 硬编码 `match` 分发 | 服务注册表 + `inject` 依赖注入 | **最值得改**：把桥 API 变成可注册的服务表 |
| 配置模型 | manifest 单层 + `.permissions.json` 覆盖 | 分层 patch（bundle→profile→home→--patch） | 可加 1–2 层覆盖（用户/部署），成本低 |
| 运行时条件 | 无（权限只有布尔开关） | `!!js` 表达式 / `disabled:` 条件 | 可在 manifest 增加条件字段（如 `disabled: {platform: win32}`） |
| 扩展间协作 | 完全隔离，无互通 | 服务注入 + 事件总线 | 可加 opt-in 事件总线 + 扩展间消息 |
| 依赖关系 | 无 | peerDependencies + inject | manifest 可加 `requires`（宿主能力版本）/ `dependsOn`（扩展间） |
| 后端形态 | 独立 Node 子进程 + 端口 | 同进程服务（无进程边界） | 保留进程隔离（更安全）；"服务"概念可用于宿主内能力 |
| 热更新/动态挂载 | 无（重启生效） | HMR + 运行时动态插件 | 长期方向，安全边界需重设计 |
| 失败处理 | manifest 损坏→invalid | 软失败（`LoadedPlugin.error`） | 已对齐，可延伸到「运行时某能力缺失→降级而非崩溃」 |
| 分发 | `.xhpack` zip + 市场 downloadUrl | npm 包 + pnpm 安装 | 已有雏形，缺「版本共存 + 升级」语义 |

---

## 四、本质差异：为什么不照搬 Cordis

| | DSH | x-hub |
|---|---|---|
| 平台 | Node/TypeScript 全栈 | **Rust (Tauri) 后端 + Vue 3 前端** |
| 扩展目标 | agent 的工具/服务/UI 注入 agent 循环 | 工作台面板 + 数据访问 + 可选后端 |
| 运行模型 | 插件 = 宿主内服务实例 | 插件 = iframe 沙箱 + 可选子进程 |
| 安全诉求 | 中（模型驱动，有 vm 沙箱仍视为 bash 级） | 高（桌面本地数据，iframe/进程双重隔离是核心资产） |

**结论**：Cordis 是 Node 生态的产物，x-hub 无法也无必要引入。真正可迁移的是**四个语言无关的工程模式**：

1. **服务注册表 + 依赖注入** → 桥 API 从手工 match 升级为可注册能力表。
2. **声明式分层配置** → manifest 之上加"用户覆盖 / 部署覆盖"层。
3. **运行时条件装配** → 权限/形态按 `platform`、宿主能力做条件。
4. **软失败 + 降级** → 能力缺失时扩展可感知（`runtime.info` 报能力），而非调用时裸报错。

---

## 五、演进路线图（建议）

### 阶段 1：桥 API「注册表化」（性价比最高）

**目标**：新增宿主能力不再改 `dispatch` 的 match。

**改动**（宿主 Rust）：
- 定义 trait：`ExtensionCapability { fn name() -> &'static str; fn requires_permission() -> Option<&'static str>; fn handle(args) -> ... }`
- 用一张静态路由表 `HashMap<(namespace, method), Handler>` 替代 `match`，`dispatch` 变成查表 + 权限检查 + 调用。
- `runtime.info` 返回**能力清单**（哪些 namespace/method 可用），扩展可据此降级。

**收益**：新增 `clipboard`/`data:write`/`net` 等能力 = 注册一个 handler + 在 manifest 权限表登记，不动核心分发表；为「第三方贡献宿主能力」铺路。

**成本**：中低。纯 Rust 重构，不碰前端桥协议。

### 阶段 2：manifest 增加「依赖 + 配置分层」

**目标**：可表达依赖与覆盖，向 DSH 的分层模型靠拢。

**改动**：
- manifest 增字段：`requires`（宿主 API 版本，如 `{ "xhubApi": ">=2" }`）、`dependsOn`（扩展间依赖，可选）。
- 配置分层（简化 DSH 为 2 层）：`manifest 默认 → 用户覆盖（现有 .permissions.json 泛化为 .config.json）→ 部署覆盖（可选）`。权限开关并入其中。
- 可选：`disabled` 条件字段（`{ "platform": "win32" }`），扫描时求值。

**收益**：升级/依赖语义清晰；用户与部署配置分离；条件装配能力落地。

**成本**：中。manifest schema 扩展 + 扫描器增加条件求值 + 兼容旧 manifest。

### 阶段 3：扩展间协作 + 事件总线

**目标**：让扩展从"孤岛"变"可组合"。

**改动**：
- 宿主提供 opt-in 事件总线：`events.emit(自定义事件)` / `events.on(...)`，跨 iframe 广播（权限 `events`）。
- 共享存储：opt-in 的跨扩展命名空间 storage（默认仍隔离）。
- 扩展间消息：`runtime.callExtension(targetId, method, payload)`（需目标扩展声明 `expose` 白名单 + 权限）。

**收益**：token-stats 这类"数据生产方"扩展可被"消费方"扩展复用，形成生态。

**成本**：中高。事件跨 iframe 的路由与权限校验要仔细设计（防串扰/防滥用）。

### 阶段 4（长期）：能力注入 + 运行时热更新

**目标**：让扩展不仅"展示数据"，还能"注册能力进宿主"。

**方向**（需重新设计安全边界，仅列方向）：
- **命令/动作注入**：扩展可注册一个"快捷动作"到宿主速达/命令面板（对应 x-hub 是桌面工具，非 agent）。
- **工具注入**（若 x-hub 未来引入 AI 能力）：扩展声明 model-facing tool，宿主注入——此时才真正接近 DSH/Codex 的"工具插件"。
- **热更新**：监听扩展目录变化，重载入口/服务（对应 DSH 的 HMR）。

**前置**：阶段 1 的服务注册表是这一切的地基。

---

## 六、优先级建议与待决策问题

**建议先做**：阶段 1（桥 API 注册表化）——它不改变对外契约、不引新依赖、直接消除"每加一个 API 改一次核心"的摩擦，且是后续所有阶段的地基。

**待你决策**（回应本文时可直接按编号回答）：

1. 桥 API 注册表化（阶段 1）是否纳入近期排期？
2. 扩展间协作（阶段 3 的事件总线/跨扩展调用）是否是你想要的生态方向，还是维持"孤岛隔离"更符合 x-hub 定位？
3. 是否有引入 AI/agent 能力（对应阶段 4 的"工具注入"）的规划？这决定是否值得现在就为"能力注入"预留接口。
4. 配置分层的层级：只要"用户覆盖"，还是要"部署/企业覆盖"（对应是否需要 `--patch` 类机制）？

---

## 附：关键源码索引

| 文件 | 职责 |
|---|---|
| `src-tauri/src/extension.rs` | manifest 解析、目录扫描、桥脚本注入、窗口、安装/卸载、权限覆盖 |
| `src-tauri/src/service.rs` | service 后端懒启动 / 动态端口 / 探活 / 停止 |
| `src-tauri/src/xhub_api.rs` | 桥 RPC 分发（`match (namespace, method)`）、storage/data/service 实现 |
| `src-tauri/src/proxy.rs` | `/svc/<id>/*` 反向代理（CORS） |
| `src/composables/useExtensionFrame.ts` | iframe + postMessage 桥、主题回包、错误码解析 |
| `src/components/Extension{View,Window,Center,SettingsDialog}.vue` | 各形态渲染与扩展中心 UI |
| `E:\workspace\x-hub-extensions\` | 脚手架（new / validate / deploy / pack）+ 示例扩展 + `xhub.d.ts` |
