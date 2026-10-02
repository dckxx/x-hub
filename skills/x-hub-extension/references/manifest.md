# manifest.json 规范

> **何时读我**：写/改 `manifest.json` 时。**本表已列全所有字段，不需要访问宿主源码**（读者手里也没有）。

`manifest.json` 位于扩展目录**根**。字段名**区分大小写**——`openIn`、`minSize`、`dependsOn`、`backend.engine.minVersion` 是驼峰，写错会解析失败。文件必须是**合法 JSON**（不能带注释）。

## 必填

| 字段 | 说明 |
|---|---|
| `id` | 反向域名——**用你自己的**，如 `com.yourname.weather`。安装目录、市场条目、`dependsOn` 都按它认人。**格式硬规则**：≤128 字符、**必须含 `.`**、只允许小写字母 / 数字 / `.` / `_` / `-`（**不能有大写**）、不以 `.` 开头、不含 `..`。⚠️ **`com.x-hub.*` 是平台保留命名空间**（只给官方自营扩展），第三方用它会在服务端关卡被拒；本地预检只提示不拦，容易一路拖到上传才发现 |
| `name` | 展示名 |
| `version` | **必须是 `x.y.z` 三段纯数字**（如 `0.1.0`）。预检按这个格式卡：`1.0`、`v1.0.0`、`1.0.0-beta` 一律判为不合法（报「版本号不是 x.y.z 形式」），发布会被拒 |

## 运行时与形态

| 字段 | 类型 | 默认 |
|---|---|---|
| `runtime` | `"web"` \| `"service"` | `web` |
| `kind` | `"module"` \| `"view"` \| `"window"` \| `"drawer"` | `view` |
| `surfaces` | string[] | `[]` |
| `openIn` | string[] | `[]` |

## 入口与资源

| 字段 | 类型 | 说明 |
|---|---|---|
| `entry` | `{ [surface]: string }` | 各形态入口，**值统一是 HTML 文件**（相对扩展目录） |
| `icon` | string | 图标（相对路径，SVG 或 PNG） |
| `minSize` | `{ w, h }` | window / drawer 建议尺寸 |
| `description` | string | 一句话描述 |
| `author` | string | 作者署名。市场卡片与详情页会展示它；没填会显示占位符「—」。发布弹窗里也能补填（会写回 manifest.json），但推荐生成时就填好 |

**每个声明的 entry 都必须有真实文件**，否则扩展打不开。

## 权限 `permissions`

可选值：`data:read`、`data:write`、`fs`、`clipboard`、`network`、`open-url`（用默认浏览器打开外链）、`system`（保留，宿主暂无对应能力）、`notify`、`events`（广播自定义事件）、`shared-storage`（跨扩展共享存储）。

没声明就调用会被拒（`PERMISSION_DENIED`）。各权限与桥 API 的对应关系见 `bridge-api.md`。

⚠️ **`network` 是个例外**：它当前**不对应任何可用的桥 API**（`net.fetch` 还是 planned），唯一生效点是 **service 后端对外监听**（`backend.host` 非回环）。普通扩展不需要它。

## 依赖与条件

| 字段 | 类型 | 说明 |
|---|---|---|
| `requires` | string[] | 依赖的宿主能力，写 `namespace.method`（如 `data.notes.list`）；宿主未实现则扩展中心标「缺能力」 |
| `dependsOn` | string[] | 依赖的其它扩展 id；未安装则扩展中心标「缺依赖」、点击打开前拦截 |
| `disabled` | `{ platform: string }` | 条件禁用：`platform` 匹配当前系统（`windows` / `macos` / `linux`）则扩展被禁用 |

⚠️ `requires` 写的是**能力名**（`data.notes.list`），不是**权限名**（`data:read`）。写错会在扩展中心标「缺能力」。

## 跨扩展协作

| 字段 | 类型 | 说明 |
|---|---|---|
| `expose` | string[] | 暴露给其它扩展调用的方法名；本扩展内用 `xhub.expose(method, fn)` 注册，其它扩展用 `xhub.runtime.callExtension(id, method, payload)` 调用 |
| `actions` | `{ id, title, surface }[]` | 快捷动作：扩展中心每个扩展行会渲染动作按钮，点击打开对应形态 |
| `config` | `{ [key]: any }` | **作者默认配置**。用户在扩展设置里改的值写在扩展目录的 `.config.json`（覆盖优先、升级扩展不冲掉）；扩展侧只用 `xhub.config.*` 读写，**别自己读配置文件** |

## service 专属 `backend`

仅 `runtime: "service"` 时使用。

| 字段 | 说明 |
|---|---|
| `entry` | 后端入口（如 `./service/index.js`） |
| `engine` | `{ type: "node", minVersion: "24" }` |
| `cwd` | 后端工作目录（相对路径，可选） |
| `port` | `0` = 动态分配（推荐）；固定端口需冲突检测 |
| `host` | 监听主机。缺省 / `127.0.0.1` / `::1` / `localhost` = **仅本机（安全默认）**；写 `0.0.0.0` 或局域网地址 = **对外开服务**，此时**必须**声明 `network` 权限且用户未关闭该权限，否则宿主拒绝启动后端（`PERMISSION_DENIED`），发布预检也会报「对外监听却没有申请 network 权限」 |
| `health` | 健康检查路径（如 `/healthz`，可选） |

> **`network` 权限只在上面这一种情况需要**（后端对外监听）。后端自己发外部 HTTP 请求、前端走 `service.request` 都是自由的，**不要**顺手声明 `network`——授权提示会多一项，用户会问为什么。

## 工作台模块多形态 `moduleVariants`

可选，仅对 `module` 形态有效。

| 字段 | 说明 |
|---|---|
| `id` | 形态 id（小写、无空格，唯一） |
| `name` | 形态名（编辑器形态芯片 / 切换浮层展示用） |
| `minW` / `minH` | 内容完整展示的最小格子（2–15） |
| `idealW` / `idealH` | 内容正好铺满的推荐格子（≥ min） |

声明后 module 卡片在工作台自定义布局里以多个形态注册：编辑器「先选形态再拖入」、切换浮层、最小尺寸钳制、填充适配徽标全生效（与内置时钟/天气一致）。未声明则只有一个默认形态（min 2×2 / ideal 4×3）。

各形态**共用 `entry.module` 的同一份 HTML**，扩展在运行时按当前形态渲染不同内容——取形态的方式见 `surfaces.md`。

## 工作台模块选项 `moduleOptions`

可选，仅对 `module` 形态有效。

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `defaultHideTitle` | boolean | 不写 | 作者声明这张卡**默认要不要宿主表头**。**不写 = 默认不显示表头**（扩展卡默认没有表头，卡面完全由扩展自己画）；写 `false` = 宿主默认在卡片顶部渲染表头（品牌色图标 + `manifest.name`）。用户在布局编辑器里用「Aa」可按卡片单独覆盖（用户设置优先），这里只是初值 |

- 绝大多数摘要卡**什么都不用写**（默认无表头）。只有想让宿主帮你画标题栏时才写 `"moduleOptions": { "defaultHideTitle": false }`，此时卡名 = `manifest.name`，卡面自己别再画一遍标题。
- 表头本身占一行高度（约 30px），声明与否会改变入口 iframe 的可用高度——入口一律按 `height: 100%` 自适应，别按像素估算。

## 示例

### web 扩展

```json
{
  "id": "com.example.weather",
  "name": "天气卡片",
  "version": "1.0.0",
  "runtime": "web",
  "kind": "view",
  "surfaces": ["module", "view"],
  "openIn": ["view"],
  "entry": {
    "module": "./module/index.html",
    "view": "./view/index.html"
  },
  "permissions": ["data:read"],
  "icon": "./icon.svg",
  "description": "在工作台显示实时天气"
}
```

### service 扩展

```json
{
  "id": "com.example.news",
  "name": "新闻聚合",
  "version": "0.1.0",
  "runtime": "service",
  "kind": "view",
  "surfaces": ["view"],
  "openIn": ["view", "window"],
  "entry": { "view": "./view/index.html" },
  "backend": {
    "entry": "./service/index.js",
    "engine": { "type": "node", "minVersion": "24" },
    "cwd": "./service",
    "port": 0,
    "health": "/healthz"
  },
  "permissions": [],
  "icon": "./icon.svg",
  "description": "聚合多个信息源"
}
```

> 注意这里**没有** `network`：后端默认只监听回环，它自己去抓取外部数据不受权限管辖。只有 `backend.host` 写成对外地址时才需要加 `network`。

### module 多形态

```json
{
  "id": "com.example.calendar",
  "name": "日历",
  "kind": "module",
  "surfaces": ["module", "view"],
  "entry": { "module": "./module/index.html", "view": "./view/index.html" },
  "moduleVariants": [
    { "id": "compact", "name": "紧凑", "minW": 2, "minH": 2, "idealW": 2, "idealH": 2 },
    { "id": "month",   "name": "整月", "minW": 4, "minH": 4, "idealW": 5, "idealH": 5 }
  ]
}
```
