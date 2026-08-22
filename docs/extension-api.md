# x-hub 桥 API（window.xhub）

> 状态：草案 v1（实现前可调整，实现后走版本化）
> 定位：扩展（web / service 通用）调用宿主能力的唯一契约。
> 关联：`docs/extension-spec.md` §6（本文件为完整方法签名）

## 0. 全局约定

- **挂载点**：`window.xhub`，扩展的 `entry` 脚本加载后即可访问（宿主注入）。
- **异步**：所有方法返回 `Promise`；失败时 `reject` 一个 `XHubError`。
- **权限**：能力按 `manifest.permissions` 声明；未授权的调用直接 `reject`，错误码 `PERMISSION_DENIED`。
- **错误格式**：

```typescript
interface XHubError {
  code: 'PERMISSION_DENIED' | 'NOT_FOUND' | 'INVALID_ARGUMENT' | 'IO_ERROR' | 'NETWORK_ERROR' | 'INTERNAL';
  message: string;   // 面向开发者的可读描述，不泄露敏感信息
}
```

## 1. 基础类型（与宿主数据模型对齐）

```typescript
// 注：桥 API 直接返回宿主模型（snake_case 字段，与 src-tauri/src/models.rs 对齐）。
// 早期草案的 camelCase 已按实现修正为实际字段。
interface Note {
  id: number;
  title: string;
  content: string;
  created_at: string;
  updated_at: string;   // ISO 8601
}

interface Todo {
  id: number;
  title: string;
  done: boolean;
  priority: number;          // 0 低 / 1 中 / 2 高
  created_at: string;
  updated_at: string;
  completed_at: string | null;
}

interface Resource {
  id: number;
  kind: 'app' | 'web' | 'file';
  name: string;
  target: string;
  category: string | null;
  icon: string | null;
  args: string | null;
  sort_order: number;
  last_launched_at: string | null;
  created_at: string;
  updated_at: string;
}

interface UsageSummary {
  today_input: number;        // 今日输入 token
  today_cache_input: number;  // 今日缓存输入
  today_output: number;       // 今日输出
  today_cost: number;         // 今日费用
  today_count: number;        // 今日调用次数
  seven_day_input: number;
  seven_day_cache_input: number;
  seven_day_output: number;
  seven_day_cost: number;
  month_input: number;
  month_cache_input: number;
  month_output: number;
  month_cost: number;
  total_input: number;
  total_cache_input: number;
  total_output: number;
  total_cost: number;
  record_count: number;
  last_sync_at: number | null;
}

interface HttpResult {
  status: number;
  headers: Record<string, string>;
  text(): Promise<string>;
  json(): Promise<unknown>;
}
```

## 2. 权限映射

| manifest 权限 | 覆盖的方法 |
|---|---|
| `data:read` | `data.*` 的所有读方法 |
| `data:write` | `data.*` 的所有写方法 |
| `fs` | `fs.*` |
| `clipboard` | `clipboard.*` |
| `network` | `net.*` |
| `system` | `system.*` |
| `notify` | `ui.notify` |

无需权限的基础能力：`runtime.*`、`storage.*`、`ui.toast`、`service.*`、`events.*`。

## 3. runtime —— 扩展自身信息

```typescript
namespace runtime {
  // 扩展自身元信息与运行时状态
  info(): Promise<{
    id: string;
    name: string;
    version: string;
    runtime: 'web' | 'service';
    serviceReady: boolean;       // service 扩展：后端是否已就绪
    proxyPrefix: string | null;  // service 扩展：/svc/<extId>；web 扩展为 null
  }>;
}
```

## 4. data —— 读 / 写宿主数据

```typescript
namespace data {
  notes: {
    list(): Promise<Note[]>;                                        // data:read
    get(id: number): Promise<Note>;                                 // data:read
    create(payload: { title: string; content: string }): Promise<Note>;            // data:write
    update(id: number, payload: Partial<Pick<Note, 'title' | 'content'>>): Promise<Note>; // data:write
    remove(id: number): Promise<void>;                              // data:write
  };

  todos: {
    list(): Promise<Todo[]>;                                        // data:read
    create(payload: { content: string; priority?: number }): Promise<Todo>;          // data:write
    update(id: number, payload: Partial<Pick<Todo, 'content' | 'done' | 'priority'>>): Promise<Todo>; // data:write
    remove(id: number): Promise<void>;                              // data:write
  };

  resources: {
    list(): Promise<Resource[]>;                                    // data:read
  };

  usage: {
    summary(): Promise<UsageSummary>;                               // data:read
  };
}
```

> 一期只开放 `notes / todos / resources / usage`；其余宿主数据（便签、提示词、倒计时、对话）后续按需追加。

## 5. storage —— 扩展本地键值存储

隔离的、随扩展卸载可清除的本地存储。无需权限。

```typescript
namespace storage {
  get(key: string): Promise<unknown>;                 // 无则返回 null
  set(key: string, value: unknown): Promise<void>;    // value 需可 JSON 序列化
  remove(key: string): Promise<void>;
  clear(): Promise<void>;
}
```

## 6. fs —— 受控文件读写

受 manifest `fs` 权限 + 沙箱约束；只能访问用户授权的目录（默认：扩展自己的数据目录，扩展授权目录经详情页授权后追加）。

```typescript
namespace fs {
  readText(path: string): Promise<string>;
  writeText(path: string, content: string): Promise<void>;
  readDir(path: string): Promise<{ name: string; isDir: boolean }[]>;
  exists(path: string): Promise<boolean>;
}
```

## 7. clipboard —— 剪贴板

```typescript
namespace clipboard {
  readText(): Promise<string>;
  writeText(text: string): Promise<void>;
}
```

## 8. net —— 网络请求

受 manifest `network` 权限。包装 fetch，宿主侧做权限检查与审计。

```typescript
namespace net {
  fetch(
    url: string,
    init?: { method?: string; headers?: Record<string, string>; body?: string },
  ): Promise<HttpResult>;
}
```

## 9. service —— service 扩展调用自身后端

service 扩展专属。等价于用 `runtime.info().proxyPrefix` 做同源请求的便捷封装；也可直接用原生 `fetch` / `WebSocket` 访问 `${proxyPrefix}/...`。

```typescript
namespace service {
  request(
    path: string,              // 相对路径，如 '/api/session.prompt'
    init?: { method?: string; headers?: Record<string, string>; body?: string },
  ): Promise<HttpResult>;
}
```

> 宿主把 `${proxyPrefix}/*` 反向代理到扩展后端的 `127.0.0.1:<port>`。流式 / 双向能力直接用 `new WebSocket('ws://' + location.host + proxyPrefix + '/...')`（一期先不封装，实现时再评估）。

## 10. ui —— 界面与通知

```typescript
namespace ui {
  toast(message: string, options?: { type?: 'info' | 'success' | 'error' }): Promise<void>;  // 无需权限
  notify(title: string, body: string): Promise<void>;                                         // 需 notify 权限（系统通知）
}
```

## 11. system —— 系统能力

受 manifest `system` 权限。对齐宿主现有 `process.rs` 能力。

```typescript
namespace system {
  openUrl(url: string): Promise<void>;        // 打开网页（默认浏览器）
  openPath(path: string): Promise<void>;      // 打开本地文件 / 文件夹
  openApp(path: string, args?: string): Promise<void>;  // 启动应用（含 UAC 提权兜底）
}
```

## 12. events —— 订阅宿主事件

```typescript
namespace events {
  // 订阅事件，返回取消订阅函数
  on(event: string, handler: (payload: unknown) => void): () => void;
  off(event: string, handler: (payload: unknown) => void): void;
}
```

一期预定义事件：

| 事件名 | payload | 说明 |
|---|---|---|
| `usage-updated` | `UsageSummary` | AI 用量更新 |
| `countdown-fired` | `{ id: number }` | 倒计时触发 |
| `theme-changed` | `{ mode: string }` | 主题变化 |

> 事件名走版本化（如 `v1:usage-updated`），实现时确定最终命名。

## 13. 实现进度

> 本表是桥 API 实现进度的单一事实来源，改动时更新状态。

**状态图例**：`planned` 已规划未开始 · `in-progress` 实现中 · `done` 已完成

| 命名空间 / 方法组 | 优先级 | 状态 |
|---|---|---|
| `runtime.*` | 一期 | done |
| `storage.*` | 一期 | done |
| `data`（读） | 一期 | done |
| `data`（写） | 后补 | planned |
| `clipboard.*` | 一期 | planned |
| `ui.toast` | 一期 | planned |
| `events.*` | 一期 | planned |
| `fs.*` | 后补 | planned |
| `net.*` | 后补 | planned |
| `system.*` | 后补 | planned |
| `ui.notify` | 后补 | planned |
| `service.*` | 后补 | in-progress |

> 每完成一个命名空间，同步生成对应 `.d.ts` 供扩展开发者使用。
